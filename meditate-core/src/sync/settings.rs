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
//! The connection-test helper (`test_connection_with`) lives here
//! too — it's a small synchronous WebDAV-trait call that any shell
//! authoring sync settings needs.

use crate::db::{Database, Result};
use crate::sync::{WebDav, WebDavError};
use std::fmt;

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

// ── Connection test ──────────────────────────────────────────────────
//
// User-facing "Test connection" button in the sync settings dialog.
// Validates a (URL, username, password) tuple by issuing a single
// PROPFIND against the user's WebDAV root — cheap, doesn't touch the
// local DB or keychain, doesn't write anything to the remote. Maps
// the typed `WebDavError` variants to user-readable outcomes.

/// Outcome of a connection test. Display impl is the toast text.
#[derive(Debug, PartialEq, Eq)]
pub enum TestConnectionResult {
    /// PROPFIND returned 207 (Multi-Status) — auth + URL are good.
    Ok,
    /// 401 — credentials wrong (username, app-password, or both).
    Unauthorized,
    /// DNS / connection refused / timeout — couldn't reach the host.
    /// The string is the underlying error for diagnostics.
    Network(String),
    /// 404 — the URL points somewhere that exists but isn't a WebDAV
    /// folder. Almost always a typo in the path component.
    NotWebDavRoot,
    /// Anything else: 5xx, malformed XML, etc.
    Other(String),
}

impl fmt::Display for TestConnectionResult {
    /// Toast text — kept terse so it fits on narrow viewports
    /// (Librem 5 truncates around 30 chars). Longer diagnostic
    /// strings live in `detail()` and go to the diagnostics log.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ok => write!(f, "Connection OK"),
            Self::Unauthorized => write!(f, "Authentication failed"),
            Self::Network(_) => write!(f, "Network error"),
            Self::NotWebDavRoot => write!(f, "Not a WebDAV folder"),
            Self::Other(_) => write!(f, "Server error"),
        }
    }
}

impl TestConnectionResult {
    /// Detailed text for the diagnostics log — includes the
    /// underlying error string for Network/Other so post-hoc
    /// debugging has the full picture even though the toast is short.
    pub fn detail(&self) -> String {
        match self {
            Self::Ok => "Connection OK".to_string(),
            Self::Unauthorized => "Authentication failed (HTTP 401)".to_string(),
            Self::Network(s) => format!("Network error: {s}"),
            Self::NotWebDavRoot => "URL is not a WebDAV folder (HTTP 404)".to_string(),
            Self::Other(s) => format!("Server error: {s}"),
        }
    }
}

/// Transport-agnostic connection test. Lifts the `WebDav` trait so
/// unit tests can pass a fake impl that produces specific error
/// variants. The shell composes this with `HttpWebDav` via
/// `test_connection` for the real "Test connection" button.
pub fn test_connection_with<W: WebDav>(webdav: &W) -> TestConnectionResult {
    match webdav.list_collection("/") {
        Ok(_) => TestConnectionResult::Ok,
        Err(WebDavError::Unauthorized) => TestConnectionResult::Unauthorized,
        Err(WebDavError::Network(s)) => TestConnectionResult::Network(s),
        Err(WebDavError::NotFound) => TestConnectionResult::NotWebDavRoot,
        Err(e) => TestConnectionResult::Other(e.to_string()),
    }
}

/// Real connection test against `HttpWebDav`. Synchronous — caller
/// runs from a worker thread so the UI doesn't freeze on slow
/// networks. Doesn't touch local state.
pub fn test_connection(url: &str, username: &str, password: &str) -> TestConnectionResult {
    let webdav = crate::sync::HttpWebDav::new(url, username, password);
    test_connection_with(&webdav)
}

// ── Sync-settings save/test prep ────────────────────────────────────
//
// The Preferences → Data page's Save and Test Connection buttons run
// near-identical validation chains over (url, username, typed
// password, stored password). The decisions — trim, reject empty,
// fall back to keychain — are portable; the keychain access itself is
// shell-specific. These helpers own the decision shape so every shell
// agrees on the rules.

/// Failure modes the save-button validation chain surfaces. Shell
/// maps each variant to its gettext toast.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveSyncError {
    EmptyUrl,
    EmptyUsername,
    /// URL scheme isn't `https://` (or a missing scheme entirely).
    /// HTTP would send the Basic-auth password in cleartext on every
    /// request — the auth header is just `base64(user:pw)` over the
    /// wire — so the sync layer refuses to attempt it. Shell maps
    /// this to a user-facing "URL must start with https://" toast.
    InsecureUrl,
}

/// What the shell should do with the password row's contents after
/// the user taps Save.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PasswordAction {
    /// User left the password row empty — keep the existing
    /// keychain entry untouched. Avoids clobbering on a "fix the
    /// URL typo, leave password alone" edit.
    Keep,
    /// User typed a non-empty password — write it to the keychain.
    Store(String),
}

/// Resolved plan from a successful Save validation. Shell calls
/// `keychain::store_password` per `password` then
/// `set_nextcloud_account(url, username)` then `trigger_sync`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveSyncPlan {
    pub url: String,
    pub username: String,
    pub password: PasswordAction,
}

/// Validate the user's Save-button input. Trims url + username,
/// rejects empty, decides whether the typed password should be
/// stored or skipped. The actual keychain write + DB update +
/// `trigger_sync` ordering stays in the shell (it owns the
/// keychain transport + the threading model).
pub fn prepare_save(
    url: &str,
    username: &str,
    typed_password: &str,
) -> std::result::Result<SaveSyncPlan, SaveSyncError> {
    let url = url.trim();
    if url.is_empty() {
        return Err(SaveSyncError::EmptyUrl);
    }
    // URL schemes are case-insensitive per RFC 3986; lowercase
    // before comparing so "HTTPS://…" and "Https://…" pass.
    if !url.to_ascii_lowercase().starts_with("https://") {
        return Err(SaveSyncError::InsecureUrl);
    }
    let username = username.trim();
    if username.is_empty() {
        return Err(SaveSyncError::EmptyUsername);
    }
    let password = if typed_password.is_empty() {
        PasswordAction::Keep
    } else {
        PasswordAction::Store(typed_password.to_string())
    };
    Ok(SaveSyncPlan {
        url: url.to_string(),
        username: username.to_string(),
        password,
    })
}

/// Failure modes the Test-Connection button's prerequisite check
/// surfaces. Shell maps each to a gettext toast.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestPrereq {
    EmptyUrl,
    EmptyUsername,
    /// User left the password row empty AND no keychain entry
    /// exists — they need to type one in.
    NoPassword,
    /// Keychain access failed (D-Bus error, locked keyring, etc.).
    /// Shell logs the underlying error to diag; the toast just
    /// signals the user to try again.
    KeyringFailed,
}

/// Outcome of the shell's keychain lookup for the stored password.
/// Carries the three states the prep function dispatches on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoredPassword {
    /// Keychain returned a password for the configured account.
    Found(String),
    /// Keychain succeeded but had no entry.
    Missing,
    /// Keychain access errored out.
    Failed,
}

/// Validated credentials ready for the `test_connection` call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Credentials {
    pub url: String,
    pub username: String,
    pub password: String,
}

/// Validate the user's Test-Connection input. Trims url + username,
/// rejects empty, falls back to the keychain when typed password is
/// empty. The shell supplies the keychain read via the closure so
/// the helper stays portable.
pub fn prepare_test(
    url: &str,
    username: &str,
    typed_password: &str,
    stored_password: impl FnOnce() -> StoredPassword,
) -> std::result::Result<Credentials, TestPrereq> {
    let url = url.trim();
    if url.is_empty() {
        return Err(TestPrereq::EmptyUrl);
    }
    let username = username.trim();
    if username.is_empty() {
        return Err(TestPrereq::EmptyUsername);
    }
    let password = if typed_password.is_empty() {
        match stored_password() {
            StoredPassword::Found(p) => p,
            StoredPassword::Missing => return Err(TestPrereq::NoPassword),
            StoredPassword::Failed => return Err(TestPrereq::KeyringFailed),
        }
    } else {
        typed_password.to_string()
    };
    Ok(Credentials {
        url: url.to_string(),
        username: username.to_string(),
        password,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh() -> Database {
        // In-memory DB so each test starts clean.
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
        assert_eq!(db.list_labels().unwrap().len(), 0);
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
        let account = get_nextcloud_account(&db).unwrap();
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

    // ── test_connection_with ─────────────────────────────────────────────

    use crate::sync::{FakeWebDav, WebDavError, WebDavResult};

    /// Tiny scripted WebDav that returns a fixed error from every
    /// method. Lets us exercise the error-mapping branches one
    /// variant at a time without touching the network.
    struct AlwaysErrs(WebDavError);

    impl AlwaysErrs {
        fn clone_err(&self) -> WebDavError {
            match &self.0 {
                WebDavError::NotFound => WebDavError::NotFound,
                WebDavError::Unauthorized => WebDavError::Unauthorized,
                WebDavError::Conflict => WebDavError::Conflict,
                WebDavError::Network(s) => WebDavError::Network(s.clone()),
                WebDavError::RateLimited { retry_after } => {
                    WebDavError::RateLimited { retry_after: *retry_after }
                }
                WebDavError::Server { status, body } => WebDavError::Server {
                    status: *status,
                    body: body.clone(),
                },
                WebDavError::MalformedResponse(s) => {
                    WebDavError::MalformedResponse(s.clone())
                }
            }
        }
    }

    impl WebDav for AlwaysErrs {
        fn list_collection(&self, _: &str) -> WebDavResult<Vec<String>> {
            Err(self.clone_err())
        }
        fn get(&self, _: &str) -> WebDavResult<Vec<u8>> { unreachable!() }
        fn put(&self, _: &str, _: &[u8]) -> WebDavResult<()> { unreachable!() }
        fn mkcol(&self, _: &str) -> WebDavResult<()> { unreachable!() }
        fn delete(&self, _: &str) -> WebDavResult<()> { unreachable!() }
        fn move_to(&self, _: &str, _: &str) -> WebDavResult<()> { unreachable!() }
    }

    #[test]
    fn test_connection_with_ok_when_propfind_succeeds() {
        let fake = FakeWebDav::new();
        assert_eq!(test_connection_with(&fake), TestConnectionResult::Ok);
    }

    #[test]
    fn test_connection_with_unauthorized_when_propfind_401s() {
        let w = AlwaysErrs(WebDavError::Unauthorized);
        assert_eq!(test_connection_with(&w), TestConnectionResult::Unauthorized);
    }

    #[test]
    fn test_connection_with_network_error_carries_underlying_string() {
        let w = AlwaysErrs(WebDavError::Network("dns".into()));
        match test_connection_with(&w) {
            TestConnectionResult::Network(s) => assert_eq!(s, "dns"),
            other => panic!("expected Network(\"dns\"), got {other:?}"),
        }
    }

    #[test]
    fn test_connection_with_not_webdav_root_when_404() {
        let w = AlwaysErrs(WebDavError::NotFound);
        assert_eq!(test_connection_with(&w), TestConnectionResult::NotWebDavRoot);
    }

    #[test]
    fn test_connection_with_other_for_server_errors() {
        let w = AlwaysErrs(WebDavError::Server {
            status: 500,
            body: "internal".into(),
        });
        match test_connection_with(&w) {
            TestConnectionResult::Other(_) => {}
            other => panic!("expected Other(_), got {other:?}"),
        }
    }

    // ── prepare_save ────────────────────────────────────────────────

    #[test]
    fn prepare_save_rejects_empty_url() {
        assert_eq!(prepare_save("", "user", "pw"), Err(SaveSyncError::EmptyUrl));
        assert_eq!(prepare_save("   ", "user", "pw"), Err(SaveSyncError::EmptyUrl));
    }

    #[test]
    fn prepare_save_rejects_empty_username() {
        assert_eq!(
            prepare_save("https://nx.example", "", "pw"),
            Err(SaveSyncError::EmptyUsername),
        );
        assert_eq!(
            prepare_save("https://nx.example", "  ", "pw"),
            Err(SaveSyncError::EmptyUsername),
        );
    }

    #[test]
    fn prepare_save_rejects_http_url() {
        // Basic-auth over HTTP sends `base64(user:pw)` in cleartext
        // on every request — the sync layer refuses to attempt it.
        assert_eq!(
            prepare_save("http://nx.example", "user", "pw"),
            Err(SaveSyncError::InsecureUrl),
        );
    }

    #[test]
    fn prepare_save_rejects_scheme_less_url() {
        // A typo without scheme would otherwise fall through to ureq
        // which infers http — also cleartext.
        assert_eq!(
            prepare_save("nx.example", "user", "pw"),
            Err(SaveSyncError::InsecureUrl),
        );
    }

    #[test]
    fn prepare_save_rejects_non_http_schemes() {
        // Belt-and-braces: only https is accepted. ftp, file,
        // gemini, anything else gets the same insecure-url toast.
        assert_eq!(
            prepare_save("ftp://nx.example", "user", "pw"),
            Err(SaveSyncError::InsecureUrl),
        );
    }

    #[test]
    fn prepare_save_accepts_https_url_case_insensitively() {
        // RFC 3986: URL schemes are case-insensitive.
        for scheme in ["https://", "HTTPS://", "Https://", "HttPs://"] {
            let url = format!("{scheme}nx.example");
            let plan = prepare_save(&url, "user", "pw")
                .unwrap_or_else(|e| panic!("{url} should be accepted, got {e:?}"));
            assert_eq!(plan.username, "user");
        }
    }

    #[test]
    fn prepare_save_keep_when_password_empty() {
        let plan = prepare_save("https://nx.example", "user", "").unwrap();
        assert_eq!(plan.password, PasswordAction::Keep);
        assert_eq!(plan.url, "https://nx.example");
        assert_eq!(plan.username, "user");
    }

    #[test]
    fn prepare_save_store_when_password_present() {
        let plan = prepare_save("https://nx.example", "user", "secret").unwrap();
        assert_eq!(plan.password, PasswordAction::Store("secret".into()));
    }

    #[test]
    fn prepare_save_trims_url_and_username() {
        let plan = prepare_save("  https://nx.example  ", "  user  ", "x").unwrap();
        assert_eq!(plan.url, "https://nx.example");
        assert_eq!(plan.username, "user");
    }

    // ── prepare_test ────────────────────────────────────────────────

    #[test]
    fn prepare_test_rejects_empty_url() {
        assert_eq!(
            prepare_test("", "user", "pw", || StoredPassword::Missing),
            Err(TestPrereq::EmptyUrl),
        );
    }

    #[test]
    fn prepare_test_rejects_empty_username() {
        assert_eq!(
            prepare_test("https://nx", "", "pw", || StoredPassword::Missing),
            Err(TestPrereq::EmptyUsername),
        );
    }

    #[test]
    fn prepare_test_uses_typed_password_directly() {
        let creds = prepare_test(
            "https://nx", "user", "typed",
            || panic!("should not be called when typed_pw is non-empty"),
        ).unwrap();
        assert_eq!(creds.password, "typed");
    }

    #[test]
    fn prepare_test_falls_back_to_stored_password_when_typed_empty() {
        let creds = prepare_test(
            "https://nx", "user", "",
            || StoredPassword::Found("from-keyring".into()),
        ).unwrap();
        assert_eq!(creds.password, "from-keyring");
    }

    #[test]
    fn prepare_test_returns_no_password_when_typed_empty_and_keyring_empty() {
        assert_eq!(
            prepare_test("https://nx", "user", "", || StoredPassword::Missing),
            Err(TestPrereq::NoPassword),
        );
    }

    #[test]
    fn prepare_test_returns_keyring_failed_on_keychain_error() {
        assert_eq!(
            prepare_test("https://nx", "user", "", || StoredPassword::Failed),
            Err(TestPrereq::KeyringFailed),
        );
    }
}
