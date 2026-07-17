//! One sync attempt (Phase 7 SY-4) — the Android mirror of
//! `meditate-gtk/src/sync_runner.rs`. Reads the configured account
//! from `sync_state`, the app-password from the Android Keystore
//! (via `crate::keychain`), builds core's `HttpWebDav`, runs
//! `Sync::sync_with_progress`, and records the outcome through the
//! same core helpers so the status indicator picks it up.
//!
//! Opens its OWN database connection (same `meditate.db` path) on
//! the worker thread — exactly like the GTK runner — so a long
//! sync never holds the UI thread's shared `DATABASE` mutex.
//! SQLite's WAL handles the cross-connection concurrency.
//!
//! One shape difference vs GTK: the Android keychain read returns
//! `Option<String>` with failures already logged + collapsed to
//! `None` inside the bridge, so `PasswordMissing` covers both
//! "no entry" and "Keystore failure" here (GTK splits them). The
//! user action is the same either way: re-enter the password.

#![cfg(target_os = "android")]

use std::fmt;
use std::path::Path;

use android_activity::AndroidApp;
use meditate_core::sync::settings::{KEY_URL, KEY_USERNAME};
use meditate_core::sync::REMOTE_BASE_PATH;
use meditate_core::sync::{Sync, SyncStats};
use meditate_core::Database as CoreDb;

#[derive(Debug)]
pub enum SyncRunnerError {
    /// Couldn't open the worker's own DB connection.
    OpenDb(meditate_core::db::DbError),
    /// URL or username empty in `sync_state` — "set up sync first".
    Unconfigured,
    /// Account configured but no matching Keystore entry (or the
    /// Keystore read failed — see module docs) — "re-enter your
    /// password".
    PasswordMissing,
    /// Database error while reading config or writing status.
    Db(meditate_core::db::DbError),
    /// The sync proper failed — pull/push couldn't complete.
    Sync(meditate_core::SyncError),
    /// Remote folder wiped between attempts → recovery dialog
    /// (SY-5), not the generic error path. The previous-success
    /// timestamp is intentionally NOT updated when this fires.
    RemoteDataLost,
}

impl fmt::Display for SyncRunnerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OpenDb(e) => {
                write!(f, "couldn't open database: {e:?}")
            }
            Self::Unconfigured => write!(
                f,
                "sync isn't set up yet — open Preferences"
            ),
            Self::PasswordMissing => write!(
                f,
                "no password in the Keystore — re-enter it in Preferences"
            ),
            Self::Db(e) => write!(f, "database error: {e:?}"),
            Self::Sync(e) => write!(f, "{e}"),
            Self::RemoteDataLost => write!(
                f,
                "remote data appears wiped — previously synced batches \
                 are missing from the Nextcloud folder",
            ),
        }
    }
}

impl From<meditate_core::db::DbError> for SyncRunnerError {
    fn from(e: meditate_core::db::DbError) -> Self {
        Self::Db(e)
    }
}

impl From<meditate_core::SyncError> for SyncRunnerError {
    fn from(e: meditate_core::SyncError) -> Self {
        match e {
            meditate_core::SyncError::RemoteDataLost => {
                Self::RemoteDataLost
            }
            other => Self::Sync(other),
        }
    }
}

/// Run one sync attempt. Mirrors GTK's `run_sync_attempt`:
/// account from `sync_state`, password from the Keystore, one
/// `Sync::sync_with_progress` run, outcome recorded via core's
/// `record_*` helpers on the same worker connection.
pub fn run_sync_attempt(
    app: &AndroidApp,
    db_path: &Path,
    sounds_dir: std::path::PathBuf,
    guided_dir: std::path::PathBuf,
) -> Result<SyncStats, SyncRunnerError> {
    let db = CoreDb::open(db_path).map_err(SyncRunnerError::OpenDb)?;

    let url = db.get_sync_state(KEY_URL, "")?;
    let username = db.get_sync_state(KEY_USERNAME, "")?;
    if url.is_empty() || username.is_empty() {
        return Err(SyncRunnerError::Unconfigured);
    }

    let Some(password) =
        crate::keychain::read_password(app, &url, &username)
    else {
        return Err(SyncRunnerError::PasswordMissing);
    };

    let webdav = meditate_core::sync::HttpWebDav::new(
        &url, &username, &password,
    );

    let started = std::time::Instant::now();
    let pending_at_start = db.pending_events().map_or(0, |v| v.len());
    meditate_core::log(
        "sync.attempt",
        &format!("starting pending={pending_at_start}"),
    );

    // Bulk-file push → the callback fires at most once at the end
    // (same note as the GTK runner).
    let progress = |pushed: usize, total: usize| {
        let secs = started.elapsed().as_secs_f64().max(0.001);
        meditate_core::log(
            "sync.push",
            &format!(
                "progress {pushed}/{total} in {secs:.1}s ({:.1}/s)",
                pushed as f64 / secs,
            ),
        );
    };

    let result = Sync::new(
        &db,
        &webdav,
        REMOTE_BASE_PATH,
        sounds_dir,
        guided_dir,
    )
    .sync_with_progress(progress);
    let elapsed = started.elapsed();

    if let Ok(stats) = &result {
        let total = stats.pulled + stats.pushed;
        if total > 0 {
            let secs = elapsed.as_secs_f64().max(0.001);
            meditate_core::log(
                "sync.done",
                &format!(
                    "pulled={} pushed={} in {:.2}s ({:.1}/s)",
                    stats.pulled,
                    stats.pushed,
                    secs,
                    total as f64 / secs,
                ),
            );
        }
    }

    record_outcome(&db, &result)?;
    result.map_err(SyncRunnerError::from)
}

/// Persist the outcome for the status indicator. Success clears
/// any previous error; failure leaves the previous successful
/// timestamp intact ("last successful sync was 3 minutes ago"
/// stays accurate). Identical to the GTK runner.
fn record_outcome(
    db: &CoreDb,
    result: &Result<SyncStats, meditate_core::SyncError>,
) -> Result<(), SyncRunnerError> {
    use meditate_core::sync::settings::{
        record_remote_data_lost, record_successful_sync,
        record_sync_error,
    };
    use meditate_core::SyncError;
    match result {
        Ok(_) => record_successful_sync(
            db,
            meditate_core::time::unix_now(),
        )?,
        Err(e @ SyncError::RemoteDataLost) => {
            record_remote_data_lost(db, &e.to_string())?
        }
        Err(e) => record_sync_error(db, &e.to_string())?,
    }
    Ok(())
}
