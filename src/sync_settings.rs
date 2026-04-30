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
/// recorded last-sync-error since success supersedes the previous
/// failure for status-display purposes.
pub fn record_successful_sync(db: &Database, unix_ts: i64) -> Result<()> {
    db.set_sync_state(KEY_LAST_SYNC_UNIX_TS, &unix_ts.to_string())?;
    db.set_sync_state(KEY_LAST_SYNC_ERROR, "")?;
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
/// when the most recent attempt has failed.
pub fn record_sync_error(db: &Database, message: &str) -> Result<()> {
    db.set_sync_state(KEY_LAST_SYNC_ERROR, message)?;
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
