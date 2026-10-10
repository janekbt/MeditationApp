//! Headerbar sync-status indicator: state derivation + click-action
//! dispatch. The visual rendering (icon, CSS class, tooltip) stays
//! shell-side; this module owns the rule for which state applies
//! given the persisted sync facts and whether a sync is in flight,
//! plus the rule for which action a tap on the indicator should
//! trigger.
//!
//! The gtk shell currently runs the rules in two places —
//! `wire_sync_status`'s click handler reads `last_error` +
//! `is_data_lost` to pick its action, and `refresh_sync_status`
//! reads `account` + `last_ts` + `last_error` + `is_syncing` to
//! decide the visual. Both want the same source of truth.

/// What the headerbar sync-status indicator should display right
/// now. The shell maps each variant to its icon / css class /
/// tooltip; the typed enum makes the dispatch exhaustive and lets
/// the click-handler dispatch off the same value rather than
/// re-deriving from raw DB reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncIndicatorState {
    /// No Nextcloud account configured — hide the button entirely.
    /// Nothing for the user to see or click on.
    Hidden,
    /// A sync is currently running. Show the animated spinner.
    Syncing,
    /// Last sync attempt failed. `detail` is the error message the
    /// tooltip surfaces; `data_lost` is true when the orchestrator
    /// reported the remote-data-lost variant (i.e. the WebDAV root
    /// is missing the events tree we expect).
    Error {
        detail: String,
        data_lost: bool,
    },
    /// At least one sync succeeded. `ts` is the Unix timestamp of
    /// the last success — fed to `synced_ago_key` for the tooltip.
    OkWithTs(i64),
    /// Account configured, but no sync has completed yet. Shown in
    /// neutral foreground to avoid a blank period between
    /// "save credentials" and "first sync".
    OkNoTs,
    /// Sync works, but these audio files weren't uploaded; automatic
    /// syncs skip them until the user taps Retry.
    NotUploaded(Vec<NotUploaded>),
}

/// An audio file that wasn't uploaded, by its current name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotUploaded {
    pub name: String,
    pub reason: crate::sync::settings::UploadRefusal,
}

/// What a tap on the sync-status button should trigger given the
/// indicator's current state. The shell routes each variant to its
/// own UI path: `OpenRecovery` pushes the recovery dialog,
/// `RetrySync` calls `app.trigger_sync()`, and `OpenPrefsData`
/// opens preferences on the Data page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncIndicatorAction {
    /// Remote-data-lost variant — the WebDAV root was wiped. Send
    /// the user to the recovery dialog where they can pick push-
    /// local or wipe-local-and-pull.
    OpenRecovery,
    /// Generic sync failure — let the user retry. Often resolves
    /// transient network errors without dialog ceremony.
    RetrySync,
    /// All other states — the click is informational; opening
    /// preferences lets the user check / change settings.
    OpenPrefsData,
}

/// Resolve the indicator state from the inputs the shell already
/// reads each refresh tick: whether a Nextcloud account is
/// configured, whether a sync is in flight, the last-success
/// timestamp, the last error detail, and the remote-data-lost
/// flag. `Hidden` short-circuits when there's no account;
/// otherwise `Syncing` wins over a stale `last_error` (the spinner
/// reflects current reality, not the prior pass).
pub fn derive(
    has_account: bool,
    is_syncing: bool,
    last_ts: Option<i64>,
    last_error: Option<String>,
    is_data_lost: bool,
) -> SyncIndicatorState {
    if !has_account {
        return SyncIndicatorState::Hidden;
    }
    if is_syncing {
        return SyncIndicatorState::Syncing;
    }
    if let Some(detail) = last_error {
        return SyncIndicatorState::Error {
            detail,
            data_lost: is_data_lost,
        };
    }
    match last_ts {
        Some(ts) => SyncIndicatorState::OkWithTs(ts),
        None => SyncIndicatorState::OkNoTs,
    }
}

/// Resolve the indicator state straight off a `Database` handle +
/// the shell's `is_syncing` bool. Bundles the four sync_state
/// reads (account, last_ts, last_error, data_lost flag) and the
/// `derive` dispatch so the shell calls one function instead of
/// stitching the snapshot together inline. Each underlying DB
/// read collapses to its safe default on error (no account, no
/// timestamp, no error string, not data-lost) — the indicator is
/// observability, not a hard guarantee, and a transient DB read
/// failure shouldn't change which button the user sees.
pub fn state_from_db(
    db: &crate::db::Database,
    is_syncing: bool,
) -> SyncIndicatorState {
    use crate::sync::settings;
    let has_account = settings::nextcloud_account_from_db(db)
        .is_ok_and(|opt| opt.is_some());
    let last_ts = settings::get_last_sync_unix_ts(db).unwrap_or(None);
    let last_error = settings::get_last_sync_error(db).unwrap_or(None);
    let is_data_lost = settings::is_last_sync_remote_data_lost(db).unwrap_or(false);
    let state = derive(has_account, is_syncing, last_ts, last_error, is_data_lost);
    if matches!(state, SyncIndicatorState::OkWithTs(_) | SyncIndicatorState::OkNoTs) {
        let not_uploaded = not_uploaded(db);
        if !not_uploaded.is_empty() {
            return SyncIndicatorState::NotUploaded(not_uploaded);
        }
    }
    state
}

/// The refused uploads whose file is still here, by their name now.
fn not_uploaded(db: &crate::db::Database) -> Vec<NotUploaded> {
    let refused = crate::sync::settings::refused_uploads(db).unwrap_or_default();
    if refused.is_empty() {
        return Vec::new();
    }
    let mut names: std::collections::HashMap<String, String> =
        db.list_bell_sounds().unwrap_or_default().into_iter().map(|s| (s.uuid.0, s.name)).collect();
    names.extend(
        crate::db::list_guided_files_from_db(db).unwrap_or_default().into_iter().map(|g| (g.uuid.0, g.name)),
    );
    refused
        .into_iter()
        .filter_map(|r| Some(NotUploaded { name: names.get(&r.uuid)?.clone(), reason: r.reason }))
        .collect()
}

/// Click-action dispatch. Errors with the data-lost flag route to
/// recovery; plain errors retry; everything else opens prefs.
pub fn action_for(state: &SyncIndicatorState) -> SyncIndicatorAction {
    match state {
        SyncIndicatorState::Error { data_lost: true, .. } => SyncIndicatorAction::OpenRecovery,
        SyncIndicatorState::Error { .. } | SyncIndicatorState::NotUploaded(_) => SyncIndicatorAction::RetrySync,
        _ => SyncIndicatorAction::OpenPrefsData,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refused_uploads_show_by_name_until_retried() {
        // Skipping a refused file kept the status green while the file
        // was missing on every other device.
        use crate::sync::settings::{self, RefusedUpload, UploadRefusal};
        let db = crate::db::Database::open_in_memory().unwrap();
        settings::set_nextcloud_account(&db, "https://cloud.example", "me").unwrap();
        let guide = "5d4c3b2a-1f0e-4d9c-8b7a-6f5e4d3c2b1a";
        db.insert_guided_file_with_uuid(guide, "Body scan", "/x/b.ogg", 600, false).unwrap();
        settings::set_refused_uploads(
            &db,
            &[
                RefusedUpload { uuid: guide.into(), reason: UploadRefusal::ServerFull },
                // Its row was deleted since: it drops off.
                RefusedUpload { uuid: "gone".into(), reason: UploadRefusal::Refused(500) },
            ],
        )
        .unwrap();
        let state = state_from_db(&db, false);
        assert_eq!(
            state,
            SyncIndicatorState::NotUploaded(vec![NotUploaded { name: "Body scan".into(), reason: UploadRefusal::ServerFull }]),
        );
        assert_eq!(action_for(&state), SyncIndicatorAction::RetrySync);
        settings::clear_refused_uploads(&db).unwrap();
        assert_eq!(state_from_db(&db, false), SyncIndicatorState::OkNoTs);
    }

    #[test]
    fn derive_hides_when_no_account() {
        let s = derive(false, false, None, None, false);
        assert_eq!(s, SyncIndicatorState::Hidden);
        // Still hidden even with errors / timestamps — no account
        // wins over everything else.
        let s = derive(false, true, Some(100), Some("err".into()), true);
        assert_eq!(s, SyncIndicatorState::Hidden);
    }

    #[test]
    fn derive_returns_syncing_when_in_flight() {
        // is_syncing wins over a stale last_error.
        let s = derive(true, true, Some(100), Some("stale".into()), false);
        assert_eq!(s, SyncIndicatorState::Syncing);
    }

    #[test]
    fn derive_returns_error_with_data_lost_flag() {
        let s = derive(true, false, Some(100), Some("fail".into()), true);
        assert_eq!(
            s,
            SyncIndicatorState::Error { detail: "fail".into(), data_lost: true },
        );
    }

    #[test]
    fn derive_returns_error_without_data_lost_flag() {
        let s = derive(true, false, Some(100), Some("fail".into()), false);
        assert_eq!(
            s,
            SyncIndicatorState::Error { detail: "fail".into(), data_lost: false },
        );
    }

    #[test]
    fn derive_returns_ok_with_ts_after_success() {
        let s = derive(true, false, Some(12345), None, false);
        assert_eq!(s, SyncIndicatorState::OkWithTs(12345));
    }

    #[test]
    fn derive_returns_ok_no_ts_for_fresh_account() {
        let s = derive(true, false, None, None, false);
        assert_eq!(s, SyncIndicatorState::OkNoTs);
    }

    #[test]
    fn action_for_data_lost_error_opens_recovery() {
        let s = SyncIndicatorState::Error { detail: "x".into(), data_lost: true };
        assert_eq!(action_for(&s), SyncIndicatorAction::OpenRecovery);
    }

    #[test]
    fn action_for_other_error_retries() {
        let s = SyncIndicatorState::Error { detail: "x".into(), data_lost: false };
        assert_eq!(action_for(&s), SyncIndicatorAction::RetrySync);
    }

    #[test]
    fn action_for_non_error_states_opens_prefs() {
        for s in [
            SyncIndicatorState::Hidden,
            SyncIndicatorState::Syncing,
            SyncIndicatorState::OkWithTs(0),
            SyncIndicatorState::OkNoTs,
        ] {
            assert_eq!(action_for(&s), SyncIndicatorAction::OpenPrefsData);
        }
    }
}
