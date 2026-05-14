//! Persisted Nextcloud sync configuration. Thin layer over the
//! `Database`'s `sync_state` KV: server URL and username live here
//! (the password is in libsecret via the shell's `keychain` glue).
//! Offers a typed `NextcloudAccount` value so callers don't pass
//! loose strings.
//!
//! Account is `Some` only when both URL and username are non-empty —
//! a half-configured state ("URL but no username", "username but no
//! URL") is reported as `None` so the caller's "is sync set up?"
//! check has a single clean predicate.
//!
//! Connection-test + save/test validation live in the sibling
//! `sync::credentials` module — they don't touch the DB so they
//! stayed split out.

use crate::db::{Database, Result};

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
pub fn nextcloud_account_from_db(db: &Database) -> Result<Option<NextcloudAccount>> {
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
/// half-configured state that `nextcloud_account_from_db` would still
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
        // Bell-sound + guided-file audio files belong to the previous
        // account's storage — clear those trackers too so the new
        // account doesn't think the audio files are already up there.
        db.wipe_known_remote_sounds()?;
        db.wipe_known_remote_guided_files()?;
    }
    db.set_sync_state(KEY_URL, url)?;
    db.set_sync_state(KEY_USERNAME, username)?;
    Ok(())
}

/// Wipe the stored account. After this `nextcloud_account_from_db` returns
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
    db.wipe_known_remote_guided_files()?;
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
        // In-memory DB so each test starts clean.
        Database::open_in_memory().unwrap()
    }

    // ── NextcloudAccount round-trip ──────────────────────────────────────────

    #[test]
    fn get_account_on_fresh_db_returns_none() {
        let db = fresh();
        assert_eq!(nextcloud_account_from_db(&db).unwrap(), None);
    }

    #[test]
    fn set_then_get_round_trips_url_and_username() {
        let db = fresh();
        set_nextcloud_account(&db, "https://nc.example.com/", "janek").unwrap();
        assert_eq!(
            nextcloud_account_from_db(&db).unwrap(),
            Some(NextcloudAccount {
                url: "https://nc.example.com/".to_string(),
                username: "janek".to_string(),
            }),
        );
    }

    #[test]
    fn set_account_replaces_prior_values() {
        let db = fresh();
        set_nextcloud_account(&db, "https://old.example/",  "old-user").unwrap();
        set_nextcloud_account(&db, "https://new.example/",  "new-user").unwrap();
        let got = nextcloud_account_from_db(&db).unwrap().unwrap();
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
        let db = fresh();
        db.record_known_remote_file("a").unwrap();
        db.record_known_remote_file("b").unwrap();
        prepare_push_local_recovery(&db).unwrap();
        assert!(db.known_remote_file_uuids().unwrap().is_empty());
    }

    #[test]
    fn prepare_push_local_recovery_flags_all_events_unsynced() {
        let db = fresh();
        // Authoring a label emits a `label_insert` event.
        db.insert_label("focus").unwrap();
        let pending_before_recovery = db.pending_events().unwrap().len();
        assert!(pending_before_recovery >= 1,
            "sanity: authoring must create a pending event");
        prepare_push_local_recovery(&db).unwrap();
        assert_eq!(db.pending_events().unwrap().len(), pending_before_recovery,
            "recovery must leave events in pending state");
    }

    #[test]
    fn prepare_push_local_recovery_clears_error_and_kind() {
        let db = fresh();
        record_remote_data_lost(&db, "remote data appears wiped").unwrap();
        prepare_push_local_recovery(&db).unwrap();
        assert_eq!(get_last_sync_error(&db).unwrap(), None);
        assert!(!is_last_sync_remote_data_lost(&db).unwrap());
    }

    #[test]
    fn prepare_push_local_recovery_preserves_last_sync_unix_ts() {
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
        let db = fresh();
        db.insert_label("focus").unwrap();
        let pending_before = db.pending_events().unwrap().len();
        assert!(pending_before > 0,
            "sanity: authoring a label must create a pending event");
        prepare_wipe_local_recovery(&db).unwrap();
        assert_eq!(crate::db::list_labels_from_db(&db).unwrap().len(), 0);
        assert_eq!(db.pending_events().unwrap().len(), 0);
    }

    #[test]
    fn prepare_wipe_local_recovery_preserves_sync_account() {
        // Same constraint as set_nextcloud_account: the user is
        // wiping local state to match the configured Nextcloud, NOT
        // unconfiguring sync. URL+username must survive.
        let db = fresh();
        set_nextcloud_account(&db, "https://nc.example/", "alice").unwrap();
        prepare_wipe_local_recovery(&db).unwrap();
        let account = nextcloud_account_from_db(&db).unwrap();
        assert_eq!(account, Some(NextcloudAccount {
            url: "https://nc.example/".to_string(),
            username: "alice".to_string(),
        }));
    }

    #[test]
    fn prepare_wipe_local_recovery_clears_error_and_kind() {
        let db = fresh();
        record_remote_data_lost(&db, "remote data appears wiped").unwrap();
        prepare_wipe_local_recovery(&db).unwrap();
        assert_eq!(get_last_sync_error(&db).unwrap(), None);
        assert!(!is_last_sync_remote_data_lost(&db).unwrap());
    }

    // ── Error recording invariants ───────────────────────────────────────

    #[test]
    fn record_sync_error_does_not_clobber_last_success_ts() {
        let db = fresh();
        record_successful_sync(&db, 1_700_000_000).unwrap();
        record_sync_error(&db, "boom").unwrap();
        assert_eq!(get_last_sync_unix_ts(&db).unwrap(), Some(1_700_000_000));
        assert_eq!(get_last_sync_error(&db).unwrap(), Some("boom".to_string()));
    }

    #[test]
    fn record_successful_sync_clears_prior_error_and_kind() {
        let db = fresh();
        record_remote_data_lost(&db, "wiped").unwrap();
        record_successful_sync(&db, 1_700_000_100).unwrap();
        assert_eq!(get_last_sync_error(&db).unwrap(), None);
        assert!(!is_last_sync_remote_data_lost(&db).unwrap());
    }

    #[test]
    fn record_sync_error_resets_remote_data_lost_kind_to_generic() {
        // After a remote-data-lost failure, the next *different*
        // failure must NOT keep the remote-data-lost tag — that
        // would route the indicator click to the recovery dialog
        // for a generic error.
        let db = fresh();
        record_remote_data_lost(&db, "wiped").unwrap();
        record_sync_error(&db, "network down").unwrap();
        assert_eq!(get_last_sync_error(&db).unwrap(), Some("network down".to_string()));
        assert!(!is_last_sync_remote_data_lost(&db).unwrap());
    }
}
