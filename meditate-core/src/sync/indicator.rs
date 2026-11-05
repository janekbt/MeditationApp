//! Headerbar sync-status indicator: state derivation, the action a
//! tap triggers and the status panel's buttons. The visual rendering
//! (icon, CSS class, the panel's text) stays shell-side; this module
//! owns the rules, from the persisted sync facts and whether a sync
//! is in flight, so both apps behave alike.

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
    /// Last sync attempt failed. `reason` is what the status names,
    /// `None` for an error stored before reasons existed; `data_lost`
    /// is true when the orchestrator reported the remote-data-lost
    /// variant (i.e. the WebDAV root is missing the events tree we
    /// expect).
    Error {
        reason: Option<SyncFailureKey>,
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
    /// No password is stored for the account, so no sync can start.
    NeedsPassword,
}

/// An audio file that wasn't uploaded, by its current name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotUploaded {
    pub name: String,
    pub reason: crate::sync::settings::UploadRefusal,
}

/// Why the last sync failed, as the status names it. Each app
/// translates it; the English detail stays in the diagnostics log.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SyncFailureKey {
    Network,
    Unauthorized,
    /// The server's HTTP status, `None` where several share one error.
    Server(Option<u16>),
    /// A redirect: usually a wrong address or http for https.
    Redirected,
    UnreadableReply,
    LocalDatabase,
    Keyring,
}

/// What a tap on the sync-status button does: the recovery dialog
/// after a wiped server, else the status panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncIndicatorAction {
    /// Remote-data-lost variant — the WebDAV root was wiped. Send
    /// the user to the recovery dialog where they can pick push-
    /// local or wipe-local-and-pull.
    OpenRecovery,
    /// Show the status panel with `status_buttons`.
    ShowStatus,
}

/// A button on the status panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusButton {
    /// Preferences on the sync page.
    Preferences,
    /// Clear the refused uploads and start a sync.
    SyncNow,
    /// Preferences on the sync page, to enter the password.
    EnterPassword,
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
    if last_error.is_some() {
        return SyncIndicatorState::Error {
            reason: None,
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
    if let SyncIndicatorState::Error { data_lost, .. } = state {
        if settings::is_last_sync_password_missing(db).unwrap_or(false) {
            return SyncIndicatorState::NeedsPassword;
        }
        let reason = settings::last_sync_failure_key(db).unwrap_or(None);
        return SyncIndicatorState::Error { reason, data_lost };
    }
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
/// recovery; everything else shows the status.
pub fn action_for(state: &SyncIndicatorState) -> SyncIndicatorAction {
    match state {
        SyncIndicatorState::Error { data_lost: true, .. } => SyncIndicatorAction::OpenRecovery,
        _ => SyncIndicatorAction::ShowStatus,
    }
}

/// The status panel's buttons. Empty where no panel shows (no account,
/// a wiped server): an open panel closes.
pub fn status_buttons(state: &SyncIndicatorState) -> &'static [StatusButton] {
    use StatusButton::*;
    match state {
        SyncIndicatorState::Hidden | SyncIndicatorState::Error { data_lost: true, .. } => &[],
        SyncIndicatorState::Syncing => &[Preferences],
        SyncIndicatorState::NeedsPassword => &[EnterPassword],
        SyncIndicatorState::Error { .. }
        | SyncIndicatorState::NotUploaded(_)
        | SyncIndicatorState::OkWithTs(_)
        | SyncIndicatorState::OkNoTs => &[Preferences, SyncNow],
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
        assert_eq!(action_for(&state), SyncIndicatorAction::ShowStatus);
        assert_eq!(status_buttons(&state), [StatusButton::Preferences, StatusButton::SyncNow]);
        settings::clear_refused_uploads(&db).unwrap();
        assert_eq!(state_from_db(&db, false), SyncIndicatorState::OkNoTs);
    }

    /// A sync that never started for want of a password showed as
    /// healthy; and "retry" can't help there, so only the settings.
    #[test]
    fn a_missing_password_offers_only_enter_password() {
        use crate::sync::settings;
        let db = crate::db::Database::open_in_memory().unwrap();
        settings::set_nextcloud_account(&db, "https://cloud.example", "me").unwrap();
        settings::record_password_missing(&db).unwrap();
        let state = state_from_db(&db, false);
        assert_eq!(state, SyncIndicatorState::NeedsPassword);
        assert_eq!(action_for(&state), SyncIndicatorAction::ShowStatus);
        assert_eq!(status_buttons(&state), [StatusButton::EnterPassword]);
        assert_eq!(state_from_db(&db, true), SyncIndicatorState::Syncing);
    }

    #[test]
    fn a_good_sync_offers_preferences_and_sync_now() {
        use crate::sync::settings;
        let db = crate::db::Database::open_in_memory().unwrap();
        settings::set_nextcloud_account(&db, "https://cloud.example", "me").unwrap();
        let s = state_from_db(&db, false);
        assert_eq!(s, SyncIndicatorState::OkNoTs);
        assert_eq!(status_buttons(&s), [StatusButton::Preferences, StatusButton::SyncNow]);
        settings::record_successful_sync(&db, 1_700_000_000).unwrap();
        let s = state_from_db(&db, false);
        // The time the status turns into "Synced N ago".
        assert_eq!(s, SyncIndicatorState::OkWithTs(1_700_000_000));
        assert_eq!(action_for(&s), SyncIndicatorAction::ShowStatus);
        assert_eq!(status_buttons(&s), [StatusButton::Preferences, StatusButton::SyncNow]);
    }

    #[test]
    fn a_running_sync_offers_no_second_sync() {
        let s = SyncIndicatorState::Syncing;
        assert_eq!(action_for(&s), SyncIndicatorAction::ShowStatus);
        assert_eq!(status_buttons(&s), [StatusButton::Preferences]);
    }

    /// 26.11.0 stored only the English text; a tag this build doesn't
    /// know reads the same way.
    #[test]
    fn an_error_from_before_reasons_shows_no_reason() {
        use crate::sync::settings::{self, KEY_LAST_SYNC_ERROR, KEY_LAST_SYNC_ERROR_KIND};
        let db = crate::db::Database::open_in_memory().unwrap();
        settings::set_nextcloud_account(&db, "https://cloud.example", "me").unwrap();
        db.set_sync_state(KEY_LAST_SYNC_ERROR, "sync: webdav: network error: dns").unwrap();
        for kind in ["", "server:", "server:abc", "from_a_newer_build"] {
            db.set_sync_state(KEY_LAST_SYNC_ERROR_KIND, kind).unwrap();
            assert_eq!(
                state_from_db(&db, false),
                SyncIndicatorState::Error { reason: None, data_lost: false },
                "{kind}",
            );
        }
        settings::record_sync_error(&db, "dns", Some(SyncFailureKey::Network)).unwrap();
        assert_eq!(
            state_from_db(&db, false),
            SyncIndicatorState::Error { reason: Some(SyncFailureKey::Network), data_lost: false },
        );
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
            SyncIndicatorState::Error { reason: None, data_lost: true },
        );
    }

    #[test]
    fn derive_returns_error_without_data_lost_flag() {
        let s = derive(true, false, Some(100), Some("fail".into()), false);
        assert_eq!(
            s,
            SyncIndicatorState::Error { reason: None, data_lost: false },
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
        let s = SyncIndicatorState::Error { reason: None, data_lost: true };
        assert_eq!(action_for(&s), SyncIndicatorAction::OpenRecovery);
        // No panel for it: an open one closes.
        assert!(status_buttons(&s).is_empty());
        assert!(status_buttons(&SyncIndicatorState::Hidden).is_empty());
    }
}
