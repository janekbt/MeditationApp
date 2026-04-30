//! One sync attempt: read configured account + password, build a
//! `HttpWebDav`, run `Sync::sync`, write the outcome to `sync_state`
//! so the status indicator can pick it up. Synchronous — meant to be
//! called from a worker thread (see `application::trigger_sync`).
//!
//! The pure-logic core is `run_with_webdav`, which takes any `WebDav`
//! impl: tests pass a `FakeWebDav`, the production path passes an
//! `HttpWebDav`. That separation keeps the unit tests fast and
//! offline.

use meditate_core::db::Database as CoreDb;
use meditate_core::sync::{Sync, SyncStats, WebDav};
use std::error::Error;
use std::fmt;
use std::path::Path;

use crate::keychain::{self, KeychainError};
use crate::sync_settings::{KEY_LAST_SYNC_ERROR, KEY_LAST_SYNC_UNIX_TS, KEY_URL, KEY_USERNAME};

/// Path under the WebDAV root where this app's data lives.
pub const REMOTE_BASE_PATH: &str = "Meditate";

#[derive(Debug)]
pub enum SyncRunnerError {
    /// Couldn't open the database. Should never happen at runtime
    /// (app startup already opened it via the same path), but the
    /// runner has its own connection so we surface this distinctly.
    OpenDb(meditate_core::db::DbError),

    /// Either URL or username is empty in `sync_state`. Caller should
    /// surface "set up sync first" rather than try to sync.
    Unconfigured,

    /// Account is configured but the keychain has no matching item —
    /// user wiped the keyring, or saved URL/username without a
    /// password yet. Distinct from Unconfigured because the action
    /// is different ("re-enter your password" vs "set up sync").
    PasswordMissing,

    /// Keychain backend error (D-Bus down, locked, …).
    Keychain(KeychainError),

    /// Database error while reading config or writing status.
    Db(meditate_core::db::DbError),

    /// The sync proper failed — pull/push couldn't complete.
    Sync(meditate_core::sync::SyncError),
}

impl fmt::Display for SyncRunnerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OpenDb(e) => write!(f, "couldn't open database: {e:?}"),
            Self::Unconfigured =>
                write!(f, "sync isn't set up yet — open Preferences → Data"),
            Self::PasswordMissing =>
                write!(f, "no password in keyring — re-enter it in Preferences"),
            Self::Keychain(e) => write!(f, "{e}"),
            Self::Db(e) => write!(f, "database error: {e:?}"),
            Self::Sync(e) => write!(f, "{e}"),
        }
    }
}

impl Error for SyncRunnerError {}

impl From<meditate_core::db::DbError> for SyncRunnerError {
    fn from(e: meditate_core::db::DbError) -> Self { Self::Db(e) }
}

impl From<KeychainError> for SyncRunnerError {
    fn from(e: KeychainError) -> Self { Self::Keychain(e) }
}

impl From<meditate_core::sync::SyncError> for SyncRunnerError {
    fn from(e: meditate_core::sync::SyncError) -> Self { Self::Sync(e) }
}

/// Run one sync attempt against the database at `db_path`. Reads the
/// configured account from `sync_state`, the password from libsecret,
/// constructs an `HttpWebDav`, runs `Sync::sync`. Writes a successful
/// timestamp on success, or the error message to `last_sync_error` on
/// failure — both via the same database connection so the next opener
/// (the GTK shell) sees them on its next read.
pub fn run_sync_attempt(db_path: &Path) -> Result<SyncStats, SyncRunnerError> {
    let db = CoreDb::open(db_path).map_err(SyncRunnerError::OpenDb)?;

    // Account configuration is read here (not by callers) so a single
    // function handles the full attempt — no half-runs.
    let url = db.get_sync_state(KEY_URL, "")?;
    let username = db.get_sync_state(KEY_USERNAME, "")?;
    if url.is_empty() || username.is_empty() {
        return Err(SyncRunnerError::Unconfigured);
    }

    let password = match keychain::read_password(&url, &username)? {
        Some(p) => p,
        None => return Err(SyncRunnerError::PasswordMissing),
    };

    let webdav = meditate_core::sync::HttpWebDav::new(&url, &username, &password);
    run_with_webdav(&db, &webdav)
}

/// The transport-agnostic core of the runner. Tests pass a FakeWebDav;
/// the production caller above passes an HttpWebDav. Either way: run
/// Sync::sync, record the outcome in sync_state, propagate the result.
pub fn run_with_webdav<W: WebDav>(
    db: &CoreDb,
    webdav: &W,
) -> Result<SyncStats, SyncRunnerError> {
    let result = Sync::new(db, webdav, REMOTE_BASE_PATH).sync();
    record_outcome(db, &result)?;
    result.map_err(SyncRunnerError::Sync)
}

/// Persist the sync outcome so the status indicator (Phase E.5) can
/// surface it. Success clears any previous error; failure leaves the
/// previous successful timestamp intact (the user wants "last
/// successful sync was 3 minutes ago" to keep being accurate even
/// when the most recent attempt failed).
fn record_outcome(
    db: &CoreDb,
    result: &Result<SyncStats, meditate_core::sync::SyncError>,
) -> Result<(), SyncRunnerError> {
    match result {
        Ok(_) => {
            let now = unix_now();
            db.set_sync_state(KEY_LAST_SYNC_UNIX_TS, &now.to_string())?;
            db.set_sync_state(KEY_LAST_SYNC_ERROR, "")?;
        }
        Err(e) => {
            db.set_sync_state(KEY_LAST_SYNC_ERROR, &e.to_string())?;
        }
    }
    Ok(())
}

/// Current unix timestamp (UTC seconds). Defaults to 0 on the
/// pathological "system clock before epoch" case rather than panicking.
fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

// ── Connection test ────────────────────────────────────────────────────────
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
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ok => write!(f, "Connection OK — your Nextcloud is reachable"),
            Self::Unauthorized => write!(
                f, "Authentication failed — check your username and app password"),
            Self::Network(s) => write!(f, "Couldn't reach the server: {s}"),
            Self::NotWebDavRoot => write!(
                f, "The URL doesn't point at a WebDAV folder. Check the path component."),
            Self::Other(s) => write!(f, "Unexpected server response: {s}"),
        }
    }
}

/// Run a connection test using a real `HttpWebDav` against the given
/// credentials. Synchronous — call from a worker thread so the UI
/// doesn't freeze on slow networks. Doesn't read or write any local
/// state.
pub fn test_connection(url: &str, username: &str, password: &str) -> TestConnectionResult {
    let webdav = meditate_core::sync::HttpWebDav::new(url, username, password);
    test_connection_with(&webdav)
}

/// Transport-agnostic core. Lifts the WebDav trait so unit tests can
/// pass a fake impl that produces specific error variants.
pub fn test_connection_with<W: WebDav>(webdav: &W) -> TestConnectionResult {
    use meditate_core::sync::WebDavError;
    match webdav.list_collection("/") {
        Ok(_) => TestConnectionResult::Ok,
        Err(WebDavError::Unauthorized) => TestConnectionResult::Unauthorized,
        Err(WebDavError::Network(s)) => TestConnectionResult::Network(s),
        Err(WebDavError::NotFound) => TestConnectionResult::NotWebDavRoot,
        Err(e) => TestConnectionResult::Other(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    //! Tests use core's `Database::open_in_memory` plus a `FakeWebDav`
    //! to exercise `run_with_webdav` end-to-end without touching the
    //! filesystem, the network, or the keychain. The keychain path
    //! is exercised by hand on the laptop / Librem 5 (E.7).

    use super::*;
    use meditate_core::db::{Session, SessionMode};
    use meditate_core::sync::FakeWebDav;

    fn fresh_db_with_session() -> CoreDb {
        let db = CoreDb::open_in_memory().unwrap();
        db.insert_session(&Session {
            start_iso: "2026-04-30T10:00:00".into(),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Countdown,
            uuid: String::new(),
        }).unwrap();
        db
    }

    #[test]
    fn run_with_webdav_pushes_local_event_to_remote() {
        // The integration: runner → Sync::sync → push.
        let db = fresh_db_with_session();
        let fake = FakeWebDav::new();
        let stats = run_with_webdav(&db, &fake).unwrap();
        assert_eq!(stats.pushed, 1, "the local session_insert event must be pushed");
        assert_eq!(stats.pulled, 0);
        assert_eq!(fake.file_count(), 1, "remote must have one event file");
    }

    #[test]
    fn run_with_webdav_writes_last_sync_unix_ts_on_success() {
        // Status indicator depends on this. Don't assert the exact
        // value (now() varies), just that a non-zero one was written.
        let db = fresh_db_with_session();
        let fake = FakeWebDav::new();
        run_with_webdav(&db, &fake).unwrap();
        let raw = db.get_sync_state(KEY_LAST_SYNC_UNIX_TS, "").unwrap();
        assert!(!raw.is_empty(), "timestamp must be written on success");
        let ts: i64 = raw.parse().expect("timestamp must be a parseable i64");
        assert!(ts > 1_700_000_000,
            "ts must be a recent unix timestamp, got {ts}");
    }

    #[test]
    fn run_with_webdav_clears_prior_last_sync_error_on_success() {
        // A previous failure left an error message; success must wipe
        // it so the status indicator stops showing the old failure.
        let db = fresh_db_with_session();
        db.set_sync_state(KEY_LAST_SYNC_ERROR, "401 Unauthorized").unwrap();
        let fake = FakeWebDav::new();
        run_with_webdav(&db, &fake).unwrap();
        assert_eq!(
            db.get_sync_state(KEY_LAST_SYNC_ERROR, "fallback").unwrap(),
            "",
            "success must clear the prior error",
        );
    }

    #[test]
    fn run_with_webdav_records_error_on_failure() {
        // Failing transport: a WebDav impl that always returns a
        // server error. The runner must capture the error message.
        struct BrokenWebDav;
        impl WebDav for BrokenWebDav {
            fn list_collection(&self, _: &str)
                -> meditate_core::sync::WebDavResult<Vec<String>>
            { Err(meditate_core::sync::WebDavError::Server {
                status: 500, body: "boom".into() }) }
            fn get(&self, _: &str)
                -> meditate_core::sync::WebDavResult<Vec<u8>>
            { unreachable!() }
            fn put(&self, _: &str, _: &[u8])
                -> meditate_core::sync::WebDavResult<()>
            { Err(meditate_core::sync::WebDavError::Server {
                status: 500, body: "boom".into() }) }
            fn mkcol(&self, _: &str)
                -> meditate_core::sync::WebDavResult<()>
            { Err(meditate_core::sync::WebDavError::Server {
                status: 500, body: "boom".into() }) }
            fn delete(&self, _: &str)
                -> meditate_core::sync::WebDavResult<()>
            { unreachable!() }
        }
        let db = fresh_db_with_session();
        let result = run_with_webdav(&db, &BrokenWebDav);
        assert!(result.is_err());

        let err_msg = db.get_sync_state(KEY_LAST_SYNC_ERROR, "").unwrap();
        assert!(!err_msg.is_empty(), "error message must be recorded");
        assert!(err_msg.contains("500"),
            "error must include the HTTP status, got: {err_msg}");
    }

    #[test]
    fn run_with_webdav_failure_does_not_overwrite_a_prior_success_ts() {
        // The user wants to see "last successful sync was N minutes
        // ago" stay accurate even after a failure. Recording an error
        // must not touch the success timestamp.
        let db = fresh_db_with_session();
        // Seed a known successful-sync timestamp.
        db.set_sync_state(KEY_LAST_SYNC_UNIX_TS, "1700000000").unwrap();

        struct AlwaysFail;
        impl WebDav for AlwaysFail {
            fn list_collection(&self, _: &str)
                -> meditate_core::sync::WebDavResult<Vec<String>>
            { Err(meditate_core::sync::WebDavError::Network("offline".into())) }
            fn get(&self, _: &str)
                -> meditate_core::sync::WebDavResult<Vec<u8>>
            { unreachable!() }
            fn put(&self, _: &str, _: &[u8])
                -> meditate_core::sync::WebDavResult<()>
            { Err(meditate_core::sync::WebDavError::Network("offline".into())) }
            fn mkcol(&self, _: &str)
                -> meditate_core::sync::WebDavResult<()>
            { Err(meditate_core::sync::WebDavError::Network("offline".into())) }
            fn delete(&self, _: &str)
                -> meditate_core::sync::WebDavResult<()>
            { unreachable!() }
        }
        let _ = run_with_webdav(&db, &AlwaysFail);
        assert_eq!(
            db.get_sync_state(KEY_LAST_SYNC_UNIX_TS, "").unwrap(),
            "1700000000",
            "failure must not clobber the prior success timestamp",
        );
    }

    #[test]
    fn two_devices_running_runner_against_same_fake_converge() {
        // End-to-end: A runs the runner; B runs the runner. Both
        // converge on the union of their events. Mirrors what
        // `Sync::sync` already tests, but pinned at this layer too
        // since this is the boundary the GTK shell calls into.
        let db_a = fresh_db_with_session();
        let db_b = CoreDb::open_in_memory().unwrap();
        db_b.insert_session(&Session {
            start_iso: "B's session".into(),
            duration_secs: 1200,
            label_id: None,
            notes: None,
            mode: SessionMode::Stopwatch,
            uuid: String::new(),
        }).unwrap();
        let shared = FakeWebDav::new();

        run_with_webdav(&db_a, &shared).unwrap();
        run_with_webdav(&db_b, &shared).unwrap();
        // A doesn't have B's session yet — needs another sync round.
        run_with_webdav(&db_a, &shared).unwrap();

        let a_starts: std::collections::HashSet<String> = db_a
            .list_sessions().unwrap()
            .iter().map(|(_, s)| s.start_iso.clone()).collect();
        let b_starts: std::collections::HashSet<String> = db_b
            .list_sessions().unwrap()
            .iter().map(|(_, s)| s.start_iso.clone()).collect();
        assert_eq!(a_starts, b_starts, "both devices converge on the same set");
        assert_eq!(a_starts.len(), 2);
    }

    #[test]
    fn sync_runner_error_display_is_user_actionable() {
        // The string here flows into the status indicator's tooltip
        // and the diagnostics log. Make sure the user-actionable
        // variants ("you haven't set this up", "re-enter your
        // password") read sensibly.
        assert_eq!(
            SyncRunnerError::Unconfigured.to_string(),
            "sync isn't set up yet — open Preferences → Data",
        );
        assert_eq!(
            SyncRunnerError::PasswordMissing.to_string(),
            "no password in keyring — re-enter it in Preferences",
        );
    }

    // ── test_connection_with ─────────────────────────────────────────────────

    #[test]
    fn test_connection_with_succeeds_on_a_reachable_webdav() {
        // FakeWebDav's list_collection always returns Ok([]) for an
        // empty store — that's what we expect when the URL points at
        // a working but empty user root.
        let fs = FakeWebDav::new();
        assert_eq!(test_connection_with(&fs), TestConnectionResult::Ok);
    }

    /// Tiny scripted WebDav that returns a fixed error from every method.
    /// Easier than per-test inline impls and lets us exercise the error
    /// mapping branches one variant at a time.
    struct AlwaysErrs(meditate_core::sync::WebDavError);
    impl WebDav for AlwaysErrs {
        fn list_collection(&self, _: &str)
            -> meditate_core::sync::WebDavResult<Vec<String>>
        { Err(self.clone_err()) }
        fn get(&self, _: &str)
            -> meditate_core::sync::WebDavResult<Vec<u8>> { unreachable!() }
        fn put(&self, _: &str, _: &[u8])
            -> meditate_core::sync::WebDavResult<()> { unreachable!() }
        fn mkcol(&self, _: &str)
            -> meditate_core::sync::WebDavResult<()> { unreachable!() }
        fn delete(&self, _: &str)
            -> meditate_core::sync::WebDavResult<()> { unreachable!() }
    }
    impl AlwaysErrs {
        fn clone_err(&self) -> meditate_core::sync::WebDavError {
            use meditate_core::sync::WebDavError as E;
            match &self.0 {
                E::NotFound => E::NotFound,
                E::Unauthorized => E::Unauthorized,
                E::Conflict => E::Conflict,
                E::Network(s) => E::Network(s.clone()),
                E::Server { status, body } => E::Server {
                    status: *status, body: body.clone() },
                E::MalformedResponse(s) => E::MalformedResponse(s.clone()),
            }
        }
    }

    #[test]
    fn test_connection_with_maps_401_to_unauthorized() {
        // Wrong app password is THE failure mode users will hit most.
        // The toast must read "Authentication failed" so they know to
        // re-check the password (not the URL, not the network).
        let w = AlwaysErrs(meditate_core::sync::WebDavError::Unauthorized);
        assert_eq!(test_connection_with(&w), TestConnectionResult::Unauthorized);
    }

    #[test]
    fn test_connection_with_maps_dns_failure_to_network_error() {
        // The exact error pattern we hit on the Librem 5 with stale
        // resolver state — surface as Network, not as a generic Server
        // error, so the toast tells the user "couldn't reach" rather
        // than "server returned bad data".
        let w = AlwaysErrs(meditate_core::sync::WebDavError::Network(
            "Dns Failed: ...".to_string()));
        assert_eq!(
            test_connection_with(&w),
            TestConnectionResult::Network("Dns Failed: ...".to_string()),
        );
    }

    #[test]
    fn test_connection_with_maps_404_to_not_webdav_root() {
        // Distinguishing 404 from generic-server-error matters because
        // the user-actionable advice is different: 404 means "fix the
        // URL"; 5xx means "wait / contact admin".
        let w = AlwaysErrs(meditate_core::sync::WebDavError::NotFound);
        assert_eq!(
            test_connection_with(&w),
            TestConnectionResult::NotWebDavRoot,
        );
    }

    #[test]
    fn test_connection_with_routes_500_to_other() {
        // Server-side 500 isn't a config bug on our end, so the toast
        // should be diagnostic ("unexpected response") rather than
        // pointing fingers at the user's credentials or path.
        let w = AlwaysErrs(meditate_core::sync::WebDavError::Server {
            status: 500, body: "internal".to_string() });
        match test_connection_with(&w) {
            TestConnectionResult::Other(s) => {
                assert!(s.contains("500"),
                    "Other variant must include the status code, got: {s}");
            }
            other => panic!("expected Other, got {other:?}"),
        }
    }

    #[test]
    fn test_connection_result_display_text_is_actionable() {
        // The Display impl IS the toast text — pin the user-facing
        // wording so a future copy edit doesn't accidentally make it
        // jargon-heavy.
        assert!(TestConnectionResult::Ok.to_string().contains("Connection OK"));
        assert!(TestConnectionResult::Unauthorized.to_string()
            .contains("Authentication failed"));
        assert!(TestConnectionResult::NotWebDavRoot.to_string()
            .contains("doesn't point at a WebDAV"));
        assert!(TestConnectionResult::Network("dns".into()).to_string()
            .contains("Couldn't reach"));
    }
}
