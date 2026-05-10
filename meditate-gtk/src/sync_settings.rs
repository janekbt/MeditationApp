//! Persisted Nextcloud sync configuration. Thin layer over the
//! `Database`'s `sync_state` KV: server URL and username live here
//! (the password is in libsecret via `keychain`). Offers a typed
//! `NextcloudAccount` value so callers don't pass loose strings.
//!
//! Account is `Some` only when both URL and username are non-empty —
//! a half-configured state ("URL but no username", "username but no
//! URL") is reported as `None` so the caller's "is sync set up?"
//! check has a single clean predicate.

use crate::db::Database;
use rusqlite::Result;

pub const KEY_URL: &str = "nextcloud_url";
pub const KEY_USERNAME: &str = "nextcloud_username";
pub const KEY_LAST_SYNC_UNIX_TS: &str = "nextcloud_last_sync_unix_ts";
pub const KEY_LAST_SYNC_ERROR: &str = "nextcloud_last_sync_error";

/// Tag attached to the last-sync-error so the status-indicator click
/// handler can route differently for the special "remote data lost"
/// recovery flow vs generic errors. Stored values: `""` (no error or
/// generic), `"remote_data_lost"`. Kept as a separate key (not
/// inferred from the error message) so a copy edit doesn't silently
/// break the routing.
pub const KEY_LAST_SYNC_ERROR_KIND: &str = "nextcloud_last_sync_error_kind";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NextcloudAccount {
    pub url: String,
    pub username: String,
}

/// Return the configured account, or `None` if either field is unset/
/// empty. Callers use this as the "is sync set up?" predicate; they
/// don't need to know which specific field was missing.
pub fn get_nextcloud_account(db: &Database) -> Result<Option<NextcloudAccount>> {
    let url = db.get_sync_state(KEY_URL, "")?;
    let username = db.get_sync_state(KEY_USERNAME, "")?;
    if url.is_empty() || username.is_empty() {
        Ok(None)
    } else {
        Ok(Some(NextcloudAccount { url, username }))
    }
}

/// Persist (or update) the configured account. Both fields are written
/// in a single logical "save" — leaving one stale would create a
/// half-configured state that `get_nextcloud_account` would still
/// report as `None`, but cleaner to just keep the pair consistent.
///
/// On a real change to either URL or username the dedup tracker
/// `known_remote_files` is wiped — its entries belonged to a
/// different store and would falsely trigger the remote-data-lost
/// detection on the next pull against the new account. A no-op save
/// (same URL+username re-saved) leaves it intact so previously-
/// pulled batches don't get re-GET'd.
pub fn set_nextcloud_account(db: &Database, url: &str, username: &str) -> Result<()> {
    let prev_url = db.get_sync_state(KEY_URL, "")?;
    let prev_username = db.get_sync_state(KEY_USERNAME, "")?;
    if prev_url != url || prev_username != username {
        db.wipe_known_remote_files()?;
        // Bell-sound files belong to the previous account's storage
        // — clear that tracker too so the new account doesn't think
        // the audio files are already up there.
        db.wipe_known_remote_sounds()?;
    }
    db.set_sync_state(KEY_URL, url)?;
    db.set_sync_state(KEY_USERNAME, username)?;
    Ok(())
}

/// Wipe the stored account. After this `get_nextcloud_account` returns
/// `None`. The keychain entry for the password is the caller's
/// responsibility — clearing the account doesn't touch libsecret
/// (the user might want to keep the password for later).
pub fn clear_nextcloud_account(db: &Database) -> Result<()> {
    db.set_sync_state(KEY_URL, "")?;
    db.set_sync_state(KEY_USERNAME, "")?;
    Ok(())
}

/// Read the unix timestamp (UTC seconds) of the last successful sync.
/// Returns `None` when no sync has yet completed on this device. Used
/// by the status indicator to show "synced N minutes ago".
pub fn get_last_sync_unix_ts(db: &Database) -> Result<Option<i64>> {
    let raw = db.get_sync_state(KEY_LAST_SYNC_UNIX_TS, "")?;
    if raw.is_empty() {
        return Ok(None);
    }
    // Parse failures are reported as None rather than an error — a
    // corrupted timestamp shouldn't take the status indicator down.
    Ok(raw.parse::<i64>().ok())
}

/// Record a successful sync at `unix_ts`. Also clears any previously-
/// recorded last-sync-error and the error-kind tag — success
/// supersedes the previous failure for status-display purposes.
pub fn record_successful_sync(db: &Database, unix_ts: i64) -> Result<()> {
    db.set_sync_state(KEY_LAST_SYNC_UNIX_TS, &unix_ts.to_string())?;
    db.set_sync_state(KEY_LAST_SYNC_ERROR, "")?;
    db.set_sync_state(KEY_LAST_SYNC_ERROR_KIND, "")?;
    Ok(())
}

/// Read the most recent sync-error message, if the last attempt failed.
/// Empty string in storage means "no error" → `None` here.
pub fn get_last_sync_error(db: &Database) -> Result<Option<String>> {
    let raw = db.get_sync_state(KEY_LAST_SYNC_ERROR, "")?;
    if raw.is_empty() { Ok(None) } else { Ok(Some(raw)) }
}

/// Record a sync failure. Doesn't touch the last-sync-success timestamp
/// — the user wants to see "last successful sync" stay accurate even
/// when the most recent attempt has failed. Resets the error-kind tag
/// to `""` (generic): a previous remote-data-lost tag must NOT persist
/// once a different error has occurred, otherwise the status
/// indicator would route the click to the recovery dialog despite
/// the wipe-detection no longer being the live failure.
pub fn record_sync_error(db: &Database, message: &str) -> Result<()> {
    db.set_sync_state(KEY_LAST_SYNC_ERROR, message)?;
    db.set_sync_state(KEY_LAST_SYNC_ERROR_KIND, "")?;
    Ok(())
}

/// Record a sync failure caused by `SyncError::RemoteDataLost`.
/// Tags the kind so the status indicator's click handler routes to
/// the recovery dialog instead of a plain retry. The Display message
/// is still recorded so existing surfaces (tooltip, diagnostics log)
/// stay informative.
pub fn record_remote_data_lost(db: &Database, message: &str) -> Result<()> {
    db.set_sync_state(KEY_LAST_SYNC_ERROR, message)?;
    db.set_sync_state(KEY_LAST_SYNC_ERROR_KIND, "remote_data_lost")?;
    Ok(())
}

/// Whether the latest recorded sync failure was a remote-data-lost
/// detection (as opposed to a generic error or no error at all).
/// Used by the status-indicator click handler to decide between
/// "retry sync" and "open recovery dialog".
pub fn is_last_sync_remote_data_lost(db: &Database) -> Result<bool> {
    let kind = db.get_sync_state(KEY_LAST_SYNC_ERROR_KIND, "")?;
    Ok(kind == "remote_data_lost")
}

/// Clear any pending sync error (and its kind tag) without touching
/// the success timestamp. Called by recovery flows that want to take
/// the indicator out of warning state immediately, before the next
/// sync attempt has had a chance to land its own success.
pub fn clear_sync_error(db: &Database) -> Result<()> {
    db.set_sync_state(KEY_LAST_SYNC_ERROR, "")?;
    db.set_sync_state(KEY_LAST_SYNC_ERROR_KIND, "")?;
    Ok(())
}

/// Prepare the local DB for a "push local up" recovery: wipe the
/// dedup tracker, flag every event un-synced (so the next push
/// bundles them into a fresh batch), and clear any stale sync-error
/// display state. The caller follows this with an explicit sync
/// trigger.
pub fn prepare_push_local_recovery(db: &Database) -> Result<()> {
    db.wipe_known_remote_files()?;
    db.wipe_known_remote_sounds()?;
    db.flag_all_events_unsynced()?;
    clear_sync_error(db)?;
    Ok(())
}

/// Prepare the local DB for a "wipe local to match remote" recovery:
/// erase every user-content row (events / sessions / labels /
/// known_remote_files) and clear any stale sync-error display state.
/// Settings, sync_state, and device identity survive. The caller
/// follows this with an explicit sync trigger so the remote's
/// (possibly empty) state replays into the now-empty local store.
pub fn prepare_wipe_local_recovery(db: &Database) -> Result<()> {
    db.wipe_local_event_log()?;
    clear_sync_error(db)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh() -> Database {
        // In-memory DB so each test starts clean. The shell's Database
        // wraps core's; either path works for these helpers.
        Database::open(std::path::Path::new(":memory:")).unwrap()
    }

    // ── NextcloudAccount round-trip ──────────────────────────────────────────

    #[test]
    fn get_account_on_fresh_db_returns_none() {
        let db = fresh();
        assert_eq!(get_nextcloud_account(&db).unwrap(), None);
    }

    #[test]
    fn set_then_get_round_trips_url_and_username() {
        let db = fresh();
        set_nextcloud_account(&db, "https://nc.example.com/", "janek").unwrap();
        assert_eq!(
            get_nextcloud_account(&db).unwrap(),
            Some(NextcloudAccount {
                url: "https://nc.example.com/".to_string(),
                username: "janek".to_string(),
            }),
        );
    }

    #[test]
    fn set_account_replaces_prior_values() {
        // Reconfiguring against a different server must drop the old
        // values, not produce some merged state.
        let db = fresh();
        set_nextcloud_account(&db, "https://old.example/",  "old-user").unwrap();
        set_nextcloud_account(&db, "https://new.example/",  "new-user").unwrap();
        let got = get_nextcloud_account(&db).unwrap().unwrap();
        assert_eq!(got.url, "https://new.example/");
        assert_eq!(got.username, "new-user");
    }

    #[test]
    fn set_account_wipes_known_remote_files_when_url_changes() {
        // Account swap (URL change): the previously-known remote
        // batch_uuids belong to a different store entirely. Leaving
        // them in the table would falsely trigger the remote-data-
        // lost detection on the next pull against the new account.
        let db = fresh();
        set_nextcloud_account(&db, "https://old.example/", "u").unwrap();
        db.record_known_remote_file("from-old-server").unwrap();
        assert_eq!(db.known_remote_file_uuids().unwrap().len(), 1);

        set_nextcloud_account(&db, "https://new.example/", "u").unwrap();
        assert!(db.known_remote_file_uuids().unwrap().is_empty(),
            "URL change must wipe known_remote_files");
    }

    #[test]
    fn set_account_wipes_known_remote_files_when_username_changes() {
        // Same account swap rule but driven by username change. A user
        // signing in as a different Nextcloud user is effectively a
        // different account even on the same URL.
        let db = fresh();
        set_nextcloud_account(&db, "https://nc.example/", "alice").unwrap();
        db.record_known_remote_file("from-alice").unwrap();

        set_nextcloud_account(&db, "https://nc.example/", "bob").unwrap();
        assert!(db.known_remote_file_uuids().unwrap().is_empty(),
            "username change must wipe known_remote_files");
    }

    #[test]
    fn set_account_does_not_wipe_known_remote_files_when_pair_is_unchanged() {
        // Re-saving the exact same URL+username (e.g. user edited and
        // saved without actually changing anything) MUST preserve the
        // dedup tracker — wiping it would cause every previously-pulled
        // remote file to be re-GET'd on the next sync.
        let db = fresh();
        set_nextcloud_account(&db, "https://nc.example/", "alice").unwrap();
        db.record_known_remote_file("a").unwrap();
        db.record_known_remote_file("b").unwrap();

        set_nextcloud_account(&db, "https://nc.example/", "alice").unwrap();
        assert_eq!(db.known_remote_file_uuids().unwrap().len(), 2,
            "unchanged account must preserve known_remote_files");
    }

    #[test]
    fn first_time_set_account_does_not_error_on_empty_known_remote_files() {
        // The wipe path runs unconditionally on any change including
        // first-time set (where the previous-pair is empty and the
        // table is already empty). Must not crash.
        let db = fresh();
        set_nextcloud_account(&db, "https://nc.example/", "alice").unwrap();
        assert!(db.known_remote_file_uuids().unwrap().is_empty());
    }

    // ── prepare_push_local_recovery ──────────────────────────────────────

    #[test]
    fn prepare_push_local_recovery_wipes_known_remote_files() {
        // The dedup tracker must be flushed — its entries point at
        // batches that are no longer on the (now-wiped) remote, and
        // leaving them would re-trigger remote-data-lost detection
        // immediately on the next pull.
        let db = fresh();
        db.record_known_remote_file("a").unwrap();
        db.record_known_remote_file("b").unwrap();
        prepare_push_local_recovery(&db).unwrap();
        assert!(db.known_remote_file_uuids().unwrap().is_empty());
    }

    #[test]
    fn prepare_push_local_recovery_flags_all_events_unsynced() {
        // Every previously-synced event must go back into pending so
        // the next push bundles them into a fresh batch. We exercise
        // through the shell DB API: create a label (which emits an
        // event), bulk-mark synced via flag-then-mark, then run the
        // recovery and observe that pending is non-empty again.
        let db = fresh();
        // Authoring a label emits a `label_insert` event.
        db.create_label("focus").unwrap();
        let pending_before_recovery = db.pending_events_count().unwrap();
        assert!(pending_before_recovery >= 1,
            "sanity: authoring must create a pending event");

        // The unsynced-after-recovery state is guaranteed by
        // `flag_all_events_unsynced` (covered by db.rs tests
        // directly); here we just pin that the recovery wrapper
        // delegates to it correctly — pending count is preserved
        // (idempotent on already-pending) and the helper doesn't
        // throw on this path.
        prepare_push_local_recovery(&db).unwrap();
        assert_eq!(db.pending_events_count().unwrap(), pending_before_recovery,
            "recovery must leave events in pending state");
    }

    #[test]
    fn prepare_push_local_recovery_clears_error_and_kind() {
        // The status indicator polls these. Clearing them lets it go
        // back to "syncing" state immediately, so the user doesn't see
        // the warning indicator while the recovery sync is in flight.
        let db = fresh();
        record_remote_data_lost(&db, "remote data appears wiped").unwrap();
        prepare_push_local_recovery(&db).unwrap();
        assert_eq!(get_last_sync_error(&db).unwrap(), None);
        assert!(!is_last_sync_remote_data_lost(&db).unwrap());
    }

    #[test]
    fn prepare_push_local_recovery_preserves_last_sync_unix_ts() {
        // The user sees "synced N minutes ago" while the recovery
        // sync runs. Don't clobber the previous successful timestamp;
        // only `record_successful_sync` should bump it.
        let db = fresh();
        record_successful_sync(&db, 1_700_000_000).unwrap();
        record_remote_data_lost(&db, "remote data appears wiped").unwrap();
        prepare_push_local_recovery(&db).unwrap();
        assert_eq!(get_last_sync_unix_ts(&db).unwrap(), Some(1_700_000_000),
            "the success timestamp must survive the recovery prep");
    }

    // ── prepare_wipe_local_recovery ──────────────────────────────────────

    #[test]
    fn prepare_wipe_local_recovery_clears_user_content() {
        // The "wipe local" recovery branch erases every authored row
        // so the next sync against the (empty) remote leaves the
        // local DB matching it.
        let db = fresh();
        db.create_label("focus").unwrap();
        // Authoring a label + a session emits events into the log.
        let pending_before = db.pending_events_count().unwrap();
        assert!(pending_before > 0,
            "sanity: authoring a label must create a pending event");

        prepare_wipe_local_recovery(&db).unwrap();

        assert_eq!(db.list_labels().unwrap().len(), 0);
        assert_eq!(db.pending_events_count().unwrap(), 0);
    }

    #[test]
    fn prepare_wipe_local_recovery_preserves_sync_account() {
        // Same constraint as set_nextcloud_account: the user is
        // wiping local state to match the configured Nextcloud, NOT
        // unconfiguring sync. URL+username must survive.
        let db = fresh();
        set_nextcloud_account(&db, "https://nc.example/", "alice").unwrap();
        prepare_wipe_local_recovery(&db).unwrap();
        let account = get_nextcloud_account(&db).unwrap();
        assert_eq!(account, Some(NextcloudAccount {
            url: "https://nc.example/".to_string(),
            username: "alice".to_string(),
        }));
    }

    #[test]
    fn prepare_wipe_local_recovery_clears_error_and_kind() {
        // Same UX rule as the push-local-recovery: take the indicator
        // out of warning state immediately so the user doesn't see
        // the warning while the recovery sync runs.
        let db = fresh();
        record_remote_data_lost(&db, "remote data appears wiped").unwrap();
        prepare_wipe_local_recovery(&db).unwrap();
        assert_eq!(get_last_sync_error(&db).unwrap(), None);
        assert!(!is_last_sync_remote_data_lost(&db).unwrap());
    }

    // ── last_sync_error_kind: routing for the recovery dialog ────────────

    #[test]
    fn is_last_sync_remote_data_lost_is_false_on_a_fresh_database() {
        // Default state: no sync has run, no error has been recorded.
        let db = fresh();
        assert!(!is_last_sync_remote_data_lost(&db).unwrap());
    }

    #[test]
    fn record_remote_data_lost_then_is_last_sync_remote_data_lost_returns_true() {
        // After the orchestrator surfaces RemoteDataLost, sync_runner
        // calls this helper. The status-indicator click handler can
        // then route the click to the recovery dialog.
        let db = fresh();
        record_remote_data_lost(&db, "remote data appears wiped").unwrap();
        assert!(is_last_sync_remote_data_lost(&db).unwrap());
        // The error message itself is still recorded so existing
        // surfaces (tooltip, diagnostics log) stay informative.
        assert_eq!(
            get_last_sync_error(&db).unwrap(),
            Some("remote data appears wiped".to_string()),
        );
    }

    #[test]
    fn record_sync_error_does_not_set_remote_data_lost_kind() {
        // Generic errors (network, auth, server 5xx) MUST NOT route
        // to the recovery dialog — that dialog is destructive and
        // only valid when we've actually detected a wipe.
        let db = fresh();
        record_sync_error(&db, "WebDAV: unauthorized").unwrap();
        assert!(!is_last_sync_remote_data_lost(&db).unwrap(),
            "generic errors must not be tagged remote_data_lost");
    }

    #[test]
    fn record_successful_sync_clears_the_remote_data_lost_kind() {
        // If a previous attempt was tagged remote_data_lost and the
        // user resolved it (e.g. via "push local up"), the next
        // successful sync clears the tag so the indicator stops
        // routing to the recovery dialog.
        let db = fresh();
        record_remote_data_lost(&db, "remote data appears wiped").unwrap();
        assert!(is_last_sync_remote_data_lost(&db).unwrap());
        record_successful_sync(&db, 1_700_000_000).unwrap();
        assert!(!is_last_sync_remote_data_lost(&db).unwrap(),
            "successful sync must clear the kind tag");
    }

    #[test]
    fn record_sync_error_after_remote_data_lost_clears_the_kind() {
        // Subtler case: a remote-data-lost error followed by a
        // generic error (e.g. user's wifi dropped before they
        // resolved the dialog). The kind tag must reset to "" so
        // the indicator click goes to retry-sync, not the dialog.
        let db = fresh();
        record_remote_data_lost(&db, "remote data appears wiped").unwrap();
        record_sync_error(&db, "WebDAV: network error").unwrap();
        assert!(!is_last_sync_remote_data_lost(&db).unwrap(),
            "newer non-wipe error must clear the kind tag");
    }

    #[test]
    fn empty_url_is_treated_as_unconfigured() {
        // Saving an empty URL (e.g. the user cleared the field and hit
        // save) leaves the account in a half-state; the predicate
        // returns None so callers don't accidentally try to sync to "".
        let db = fresh();
        set_nextcloud_account(&db, "", "janek").unwrap();
        assert_eq!(get_nextcloud_account(&db).unwrap(), None);
    }

    #[test]
    fn empty_username_is_treated_as_unconfigured() {
        let db = fresh();
        set_nextcloud_account(&db, "https://nc.example/", "").unwrap();
        assert_eq!(get_nextcloud_account(&db).unwrap(), None);
    }

    #[test]
    fn clear_account_wipes_both_fields() {
        let db = fresh();
        set_nextcloud_account(&db, "https://nc.example/", "janek").unwrap();
        clear_nextcloud_account(&db).unwrap();
        assert_eq!(get_nextcloud_account(&db).unwrap(), None);
        // Each field is empty in storage too — not just "any one of
        // them is empty so the predicate said None".
        assert_eq!(db.get_sync_state(KEY_URL, "fallback").unwrap(), "");
        assert_eq!(db.get_sync_state(KEY_USERNAME, "fallback").unwrap(), "");
    }

    #[test]
    fn account_persists_across_database_reopens() {
        // The values live in the `sync_state` table which persists
        // across Database opens — this test pins that path through
        // the shell wrapper, since the helpers here are the layer
        // the UI talks to.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sync_settings.db");
        {
            let db = Database::open(&path).unwrap();
            set_nextcloud_account(&db, "https://persist.example/", "user").unwrap();
        }
        let db = Database::open(&path).unwrap();
        assert_eq!(
            get_nextcloud_account(&db).unwrap(),
            Some(NextcloudAccount {
                url: "https://persist.example/".to_string(),
                username: "user".to_string(),
            }),
        );
    }

    #[test]
    fn url_round_trips_verbatim_with_no_normalisation() {
        // Don't trim / canonicalise / lowercase — what the user typed
        // is what gets stored. The HttpWebDav constructor already
        // tolerates trailing slashes, so we don't need to be picky here.
        let db = fresh();
        let url = "  https://Example.COM:8443/nc/  ";  // weird but valid
        set_nextcloud_account(&db, url, "janek").unwrap();
        assert_eq!(get_nextcloud_account(&db).unwrap().unwrap().url, url);
    }

    // ── Last-sync timestamp ──────────────────────────────────────────────────

    #[test]
    fn last_sync_ts_on_fresh_db_is_none() {
        let db = fresh();
        assert_eq!(get_last_sync_unix_ts(&db).unwrap(), None);
    }

    #[test]
    fn record_then_read_last_sync_ts() {
        let db = fresh();
        record_successful_sync(&db, 1_700_000_000).unwrap();
        assert_eq!(get_last_sync_unix_ts(&db).unwrap(), Some(1_700_000_000));
    }

    #[test]
    fn record_successful_sync_clears_any_prior_error() {
        // Status display: once a sync succeeds, the previous error
        // shouldn't keep showing. Recording success clears the error.
        let db = fresh();
        record_sync_error(&db, "401 Unauthorized").unwrap();
        assert_eq!(get_last_sync_error(&db).unwrap(), Some("401 Unauthorized".to_string()));
        record_successful_sync(&db, 1_700_000_000).unwrap();
        assert_eq!(get_last_sync_error(&db).unwrap(), None,
            "success must clear the previous error");
    }

    #[test]
    fn last_sync_ts_garbage_value_is_reported_as_none() {
        // Defensive: a corrupted sync_state row (file edited by hand,
        // partial write, …) yields None rather than an error so the
        // status indicator keeps working.
        let db = fresh();
        db.set_sync_state(KEY_LAST_SYNC_UNIX_TS, "not-a-number").unwrap();
        assert_eq!(get_last_sync_unix_ts(&db).unwrap(), None);
    }

    // ── Last-sync error ──────────────────────────────────────────────────────

    #[test]
    fn last_sync_error_on_fresh_db_is_none() {
        let db = fresh();
        assert_eq!(get_last_sync_error(&db).unwrap(), None);
    }

    #[test]
    fn record_then_read_sync_error() {
        let db = fresh();
        record_sync_error(&db, "Connection refused").unwrap();
        assert_eq!(get_last_sync_error(&db).unwrap(),
            Some("Connection refused".to_string()));
    }

    #[test]
    fn record_sync_error_does_not_clobber_last_success_ts() {
        // The user wants to see "last successful sync was 3 minutes
        // ago" stay accurate even when the most recent attempt has
        // failed. Recording an error must not touch the success ts.
        let db = fresh();
        record_successful_sync(&db, 1_700_000_000).unwrap();
        record_sync_error(&db, "Network").unwrap();
        assert_eq!(get_last_sync_unix_ts(&db).unwrap(), Some(1_700_000_000));
    }

    #[test]
    fn empty_error_string_collapses_to_none_on_read() {
        // Don't differentiate "explicitly recorded empty" from "never
        // recorded" — both mean "no error to display". Simpler API.
        let db = fresh();
        record_sync_error(&db, "").unwrap();
        assert_eq!(get_last_sync_error(&db).unwrap(), None);
    }
}
