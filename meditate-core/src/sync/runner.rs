//! One sync attempt, shared by both apps: the account from
//! `sync_state`, the password from the app's keyring, one pass, and
//! its outcome recorded for the status indicator, including the
//! failures that stop a pass before it starts. Meant for a worker
//! thread: it opens its own connection, so a long sync never holds
//! the UI's; SQLite's WAL handles the two connections.

use std::fmt;
use std::path::{Path, PathBuf};

use super::{settings, HttpWebDav, Sync, SyncError, SyncStats, WebDav, REMOTE_BASE_PATH};
use crate::db::{Database, DbError};

#[derive(Debug)]
pub enum RunError {
    /// The worker's own connection didn't open, so nothing is recorded.
    OpenDb(DbError),
    /// URL or username empty: sync isn't set up, the status is hidden.
    Unconfigured,
    /// No password stored for the account.
    PasswordMissing,
    /// The keyring couldn't be read; the app's error text.
    Keyring(String),
    /// Database error while reading the account or writing the status.
    Db(DbError),
    /// The pass itself failed.
    Sync(SyncError),
}

impl fmt::Display for RunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OpenDb(e) => write!(f, "couldn't open database: {e:?}"),
            Self::Unconfigured => write!(f, "sync isn't set up"),
            Self::PasswordMissing => write!(f, "no password saved"),
            Self::Keyring(e) => write!(f, "{e}"),
            Self::Db(e) => write!(f, "database error: {e:?}"),
            Self::Sync(e) => write!(f, "{e}"),
        }
    }
}

impl From<DbError> for RunError {
    fn from(e: DbError) -> Self { Self::Db(e) }
}

/// Run one sync attempt against the database at `db_path`.
/// `read_password(url, username)` is the app's keyring read.
pub fn run_attempt(
    db_path: &Path,
    sounds_dir: PathBuf,
    guided_dir: PathBuf,
    read_password: impl FnOnce(&str, &str) -> Result<Option<String>, String>,
) -> Result<SyncStats, RunError> {
    let db = Database::open(db_path).map_err(RunError::OpenDb)?;
    run_with(&db, sounds_dir, guided_dir, read_password, HttpWebDav::new)
}

/// The attempt over any transport: tests pass a fake server.
fn run_with<W: WebDav>(
    db: &Database,
    sounds_dir: PathBuf,
    guided_dir: PathBuf,
    read_password: impl FnOnce(&str, &str) -> Result<Option<String>, String>,
    connect: impl FnOnce(&str, &str, &str) -> W,
) -> Result<SyncStats, RunError> {
    let result = attempt(db, sounds_dir, guided_dir, read_password, connect);
    record_outcome(db, &result)?;
    result
}

fn attempt<W: WebDav>(
    db: &Database,
    sounds_dir: PathBuf,
    guided_dir: PathBuf,
    read_password: impl FnOnce(&str, &str) -> Result<Option<String>, String>,
    connect: impl FnOnce(&str, &str, &str) -> W,
) -> Result<SyncStats, RunError> {
    // Off the UI thread: wait out an app write rather than fail.
    db.set_busy_timeout(crate::db::SYNC_BUSY_TIMEOUT)?;
    let account = settings::nextcloud_account_from_db(db)?.ok_or(RunError::Unconfigured)?;
    let password = read_password(&account.url, &account.username)
        .map_err(RunError::Keyring)?
        .ok_or(RunError::PasswordMissing)?;
    settings::adopt_account(db, &account.url, &account.username)?;
    let webdav = connect(&account.url, &account.username, &password);

    let started = std::time::Instant::now();
    let pending_at_start = db.pending_events().map_or(0, |v| v.len());
    crate::diag::log("sync.attempt", &format!("starting pending={pending_at_start}"));
    // One bulk PUT per push, so this fires at most once, at the end.
    let progress = |pushed: usize, total: usize| {
        let secs = started.elapsed().as_secs_f64().max(0.001);
        crate::diag::log(
            "sync.push",
            &format!("progress {pushed}/{total} in {secs:.1}s ({:.1}/s)", pushed as f64 / secs),
        );
    };
    let stats = Sync::new(db, &webdav, REMOTE_BASE_PATH, sounds_dir, guided_dir)
        .sync_with_progress(progress)
        .map_err(RunError::Sync)?;
    let total = stats.pulled + stats.pushed;
    if total > 0 {
        let secs = started.elapsed().as_secs_f64().max(0.001);
        crate::diag::log(
            "sync.done",
            &format!(
                "pulled={} pushed={} in {secs:.2}s ({:.1}/s)",
                stats.pulled, stats.pushed, total as f64 / secs,
            ),
        );
    }
    Ok(stats)
}

/// Persist the outcome for the status indicator. Success clears any
/// previous error; a failure leaves the last success time alone, so
/// "synced 3 minutes ago" stays true.
fn record_outcome(db: &Database, result: &Result<SyncStats, RunError>) -> Result<(), RunError> {
    match result {
        Ok(_) => settings::record_successful_sync(db, crate::time::unix_now())?,
        // Without an account the status is hidden; nothing to show.
        Err(RunError::Unconfigured) => {}
        Err(RunError::PasswordMissing) => settings::record_password_missing(db)?,
        Err(RunError::Sync(e @ SyncError::RemoteDataLost)) => {
            settings::record_remote_data_lost(db, &e.to_string())?
        }
        Err(e) => settings::record_sync_error(db, &e.to_string())?,
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::indicator::{state_from_db, SyncIndicatorState};
    use crate::sync::settings::{self, KEY_LAST_SYNC_UNIX_TS};
    use crate::sync::FakeWebDav;

    fn configured_db() -> Database {
        let db = Database::open_in_memory().unwrap();
        settings::set_nextcloud_account(&db, "https://a.example", "me").unwrap();
        db.insert_label("focus").unwrap();
        db
    }

    fn run(db: &Database, password: Result<Option<String>, String>, server: &FakeWebDav) -> Result<SyncStats, RunError> {
        let dir = tempfile::tempdir().unwrap();
        run_with(db, dir.path().join("sounds"), dir.path().join("guided"), |_, _| password, |_, _, _| server.clone())
    }

    /// A sync that never started for want of a password only reached
    /// the log: the status kept saying "synced".
    #[test]
    fn a_missing_password_shows_in_the_status() {
        let db = configured_db();
        settings::record_successful_sync(&db, 1_700_000_000).unwrap();
        let server = FakeWebDav::new();
        assert!(matches!(run(&db, Ok(None), &server), Err(RunError::PasswordMissing)));
        assert_eq!(state_from_db(&db, false), SyncIndicatorState::NeedsPassword);
        assert_eq!(server.file_count(), 0);
        assert_eq!(db.get_sync_state(KEY_LAST_SYNC_UNIX_TS, "").unwrap(), "1700000000");
    }

    #[test]
    fn a_keyring_failure_shows_in_the_status() {
        let db = configured_db();
        let server = FakeWebDav::new();
        assert!(matches!(run(&db, Err("keychain: locked".into()), &server), Err(RunError::Keyring(_))));
        match state_from_db(&db, false) {
            SyncIndicatorState::Error { detail, data_lost: false } => assert!(detail.contains("keychain: locked")),
            other => panic!("expected a retryable error, got {other:?}"),
        }
    }

    #[test]
    fn without_an_account_nothing_runs_or_shows() {
        let db = Database::open_in_memory().unwrap();
        let server = FakeWebDav::new();
        assert!(matches!(run(&db, Ok(Some("pw".into())), &server), Err(RunError::Unconfigured)));
        assert_eq!(state_from_db(&db, false), SyncIndicatorState::Hidden);
        assert_eq!(settings::get_last_sync_error(&db).unwrap(), None);
    }

    #[test]
    fn a_pass_uploads_and_records_its_time() {
        let db = configured_db();
        settings::record_sync_error(&db, "401 Unauthorized").unwrap();
        let server = FakeWebDav::new();
        let stats = run(&db, Ok(Some("pw".into())), &server).unwrap();
        assert_eq!(stats.pushed, 1);
        assert_eq!(server.file_count(), 1);
        assert!(matches!(state_from_db(&db, false), SyncIndicatorState::OkWithTs(ts) if ts > 1_700_000_000));
    }

    #[test]
    fn a_failed_pass_records_why_and_keeps_the_last_success() {
        struct Broken;
        impl WebDav for Broken {
            fn list_collection(&self, _: &str) -> crate::sync::WebDavResult<Vec<String>> {
                Err(crate::sync::WebDavError::Server { status: 500, body: "boom".into() })
            }
            fn get(&self, _: &str, _: u64) -> crate::sync::WebDavResult<Vec<u8>> { unreachable!() }
            fn put(&self, _: &str, _: &[u8]) -> crate::sync::WebDavResult<()> { unreachable!() }
            fn mkcol(&self, _: &str) -> crate::sync::WebDavResult<()> { unreachable!() }
            fn delete(&self, _: &str) -> crate::sync::WebDavResult<()> { unreachable!() }
            fn move_to(&self, _: &str, _: &str) -> crate::sync::WebDavResult<()> { unreachable!() }
        }
        let db = configured_db();
        settings::record_successful_sync(&db, 1_700_000_000).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let result = run_with(&db, dir.path().join("s"), dir.path().join("g"), |_, _| Ok(Some("pw".into())), |_, _, _| Broken);
        assert!(matches!(result, Err(RunError::Sync(_))));
        match state_from_db(&db, false) {
            SyncIndicatorState::Error { detail, data_lost: false } => assert!(detail.contains("500"), "{detail}"),
            other => panic!("expected an error, got {other:?}"),
        }
        assert_eq!(db.get_sync_state(KEY_LAST_SYNC_UNIX_TS, "").unwrap(), "1700000000");
    }

    #[test]
    fn a_wiped_server_opens_the_recovery_and_keeps_the_last_success() {
        let db = configured_db();
        let server = FakeWebDav::new();
        run(&db, Ok(Some("pw".into())), &server).unwrap();
        let ts = db.get_sync_state(KEY_LAST_SYNC_UNIX_TS, "").unwrap();
        for name in server.list_collection("/Meditate/events/").unwrap() {
            server.delete(&format!("/Meditate/events/{name}")).unwrap();
        }
        assert!(matches!(run(&db, Ok(Some("pw".into())), &server), Err(RunError::Sync(SyncError::RemoteDataLost))));
        assert!(matches!(state_from_db(&db, false), SyncIndicatorState::Error { data_lost: true, .. }));
        assert_eq!(db.get_sync_state(KEY_LAST_SYNC_UNIX_TS, "").unwrap(), ts);
    }

    /// After a move to another Nextcloud only new edits went up, so a
    /// new phone there saw almost no history.
    #[test]
    fn the_first_pass_on_a_new_account_uploads_the_whole_history() {
        let db = configured_db();
        run(&db, Ok(Some("pw".into())), &FakeWebDav::new()).unwrap();

        settings::set_nextcloud_account(&db, "https://b.example", "me").unwrap();
        let new_server = FakeWebDav::new();
        let stats = run(&db, Ok(Some("pw".into())), &new_server).unwrap();
        assert_eq!(stats.pushed, 1);

        let fresh = Database::open_in_memory().unwrap();
        crate::sync::Sync::new(&fresh, &new_server, REMOTE_BASE_PATH, PathBuf::new(), PathBuf::new()).pull().unwrap();
        assert_eq!(crate::db::list_labels_from_db(&fresh).unwrap().len(), 1);
    }
}
