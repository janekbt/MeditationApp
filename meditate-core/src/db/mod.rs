use rusqlite::Connection;
use std::path::Path;

mod bell_sounds;
mod box_breath_phases;
mod device;
mod error;
mod events;
mod guided_files;
mod interval_bells;
mod known_remote;
mod labels;
mod presets;
mod schema;
mod seeds;
mod session_in_progress;
mod sessions;
mod settings;
mod sync_state;
mod vibration_patterns;

#[cfg(test)]
mod test_helpers;

pub use bell_sounds::{BellSound, BellSoundCategory};
pub use box_breath_phases::{BoxBreathPhase, BoxBreathPhaseId};
pub use events::Event;
pub use guided_files::GuidedFile;
pub use interval_bells::{IntervalBell, IntervalBellKind};
pub use labels::Label;
pub use presets::Preset;
pub use session_in_progress::{FinalizedSession, SessionInProgress};
pub use sessions::{Session, SessionFilter, SessionMode};
pub use vibration_patterns::{ChartKind, VibrationPattern};
pub use error::{target_id_is_well_formed_for, DbError, Result};
pub use schema::{CACHE_SCHEMA_VERSION, CACHE_SCHEMA_VERSION_KEY, SCHEMA_VERSION};
use error::{conflict_suffixed_name, is_unique_constraint_error};
use schema::SCHEMA;

/// One audio file in the bell-sound library — bundled CC0 sounds the
/// app ships with, plus user-imported custom files. Referenced by
/// every bell-fire site (starting bell, interval bells, completion
/// sound) via the `uuid` column. The `is_bundled` flag distinguishes
/// what the audio system does with `file_path`: bundled rows hold a
/// GResource path the binary contains; custom rows hold a filesystem
/// path under `$XDG_DATA_HOME`. Bundled rows ride sync (so a peer
/// without the bundle inherits the same UUIDs from the seeding device)
/// but the audio itself doesn't — peers compile in their own copy.
///
/// `category` partitions the library by usage context: General sounds
/// (bells / gongs / chimes) feed the Starting / Interval / End bell
/// choosers; BoxBreath sounds (voice cues / phase markers) feed the
/// One configured bell entry in the user's interval-bell library.
/// What channels a bell or phase plays through. Mirrors the
/// Sound / Vibration / Both `Adw.ToggleGroup` segments in the
/// timer setup. Used as the persisted enum behind every per-bell
/// signal-mode setting key + the `interval_bells.signal_mode`
/// column + the `box_breath_phases.signal_mode` column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalMode {
    Sound,
    Vibration,
    Both,
}

/// One row in `box_breath_phases` — the per-phase cue config for
impl SignalMode {
    pub fn as_db_str(self) -> &'static str {
        match self {
            SignalMode::Sound     => "sound",
            SignalMode::Vibration => "vibration",
            SignalMode::Both      => "both",
        }
    }
    pub fn from_db_str(s: &str) -> Option<Self> {
        match s {
            "sound"     => Some(SignalMode::Sound),
            "vibration" => Some(SignalMode::Vibration),
            "both"      => Some(SignalMode::Both),
            _           => None,
        }
    }

    /// Does this mode include the sound channel? `Sound | Both`.
    pub fn includes_sound(self) -> bool {
        matches!(self, SignalMode::Sound | SignalMode::Both)
    }

    /// Does this mode include the vibration channel? `Vibration | Both`.
    pub fn includes_vibration(self) -> bool {
        matches!(self, SignalMode::Vibration | SignalMode::Both)
    }
}


/// One entry in the append-only sync event log. A self-contained
/// description of a state-changing operation — sessions inserted /
/// updated / deleted, labels renamed, settings changed. Every field
/// is part of the cross-device identity or ordering contract:
///
/// - `event_uuid` is the dedup key. Receiving the same uuid twice
///   (retry, peer-forwarding) is a silent no-op.
/// - `lamport_ts` orders events; ties break on `device_id` per the
///   conflict-resolution rules.
/// - `device_id` records authorship.
pub struct Database {
    conn: Connection,
}

/// Mint a fresh v4 UUID. Exposed so the shell (which doesn't have
/// the `uuid` crate as a direct dep) can generate UUIDs for places
/// where the id has to be known before the row is created — e.g.,
/// the custom-bell-import path that needs a UUID for the destination
/// filename before the DB insert.
pub fn mint_uuid() -> String {
    uuid::Uuid::new_v4().to_string()
}


impl Database {
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        Self::init(conn)
    }

    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        // Wait up to 8s when another writer holds the lock instead of
        // failing instantly with SQLITE_BUSY. Main thread holds one
        // connection under Arc<Mutex<…>>; the sync worker opens its
        // own connection via this same path. WAL allows concurrent
        // reader + one writer, but two writers (e.g. a main-thread
        // set_setting landing during the sync worker's replay_events
        // transaction) still need this back-off to coexist. rusqlite
        // happens to default to 5s today; we pin the value explicitly
        // so a future version bump can't silently change the contract.
        conn.busy_timeout(std::time::Duration::from_secs(8))?;
        // For on-disk databases, enable WAL with synchronous=NORMAL.
        // The default (rollback journal + synchronous=FULL) does a
        // full fsync on every commit — autocommit UPDATEs become
        // ~50–200 ms each on phone eMMC, which bottlenecks any
        // hot-loop write. WAL+NORMAL fsyncs only on checkpoint and
        // the WAL header on commit, two orders of magnitude cheaper.
        // Durability tradeoff: a power loss between commit and
        // checkpoint may roll back a small number of recently
        // committed transactions. Acceptable here — events are
        // append-only and idempotent on re-sync.
        //
        // In-memory `open_in_memory` skips this — WAL on `:memory:` is
        // a no-op (the journal is also in memory) and synchronous
        // doesn't apply.
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")?;
        Self::init(conn)
    }

    fn init(conn: Connection) -> Result<Self> {
        // Refuse to open a DB whose user_version exceeds what this
        // build knows how to read — a downgrade from a future build
        // could otherwise drop forward-only data silently. A fresh
        // DB reads user_version=0 and falls through to the stamp.
        let db_version: u32 = conn.query_row(
            "PRAGMA user_version", [], |row| row.get(0),
        )?;
        if db_version > SCHEMA_VERSION {
            return Err(DbError::SchemaVersionTooNew {
                db: db_version,
                build: SCHEMA_VERSION,
            });
        }
        // Explicit PRAGMAs — even when rusqlite enables them by default,
        // the intent is part of the source so it can't be silently
        // dropped by a dependency upgrade. The FK clause on
        // sessions.label_id only fires when this is ON.
        //
        // ORDER MATTERS: `PRAGMA foreign_keys=ON` MUST run before
        // `execute_batch(SCHEMA)`. FK enforcement is per-connection
        // and is only checked while DML executes — setting the pragma
        // *after* the schema parse does not retroactively enforce
        // existing rows or constraint declarations parsed under the
        // OFF state. A future refactor that reorders these will
        // silently disable FK checks.
        conn.execute_batch("PRAGMA foreign_keys=ON;")?;
        conn.execute_batch(SCHEMA)?;
        // Stamp the current version. `execute_batch` is required because
        // PRAGMA values aren't bindable via params.
        conn.execute_batch(&format!("PRAGMA user_version = {SCHEMA_VERSION};"))?;
        // One-shot integrity check. `quick_check` returns "ok" on a
        // clean DB or one or more issue lines on corruption — we log
        // the first non-ok line via diag and continue, since refusing
        // to open would lock the user out without a recovery path.
        let integrity: String = conn.query_row(
            "PRAGMA quick_check", [], |row| row.get(0),
        ).unwrap_or_else(|_| "ok".to_string());
        if integrity != "ok" {
            crate::diag::log(&format!("db_integrity_check_failed: {integrity}"));
        }
        let db = Self { conn };
        db.maybe_walk_events_for_cache_upgrade()?;
        Ok(db)
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Cache schema version + walk-on-upgrade ────────────────────────

    #[test]
    fn fresh_open_in_memory_stamps_cache_schema_version_to_current() {
        // First-ever open: no events, but the marker still lands so
        // subsequent opens take the fast path and skip the walk.
        let db = Database::open_in_memory().unwrap();
        let stored = db
            .get_sync_state(CACHE_SCHEMA_VERSION_KEY, "missing")
            .unwrap();
        assert_eq!(stored, CACHE_SCHEMA_VERSION.to_string());
    }

    #[test]
    fn re_open_with_old_cache_version_walks_events_and_rematerialises_cache() {
        // Simulate a DB that was last opened by a build whose
        // apply_event_inner skipped some kind we now understand. The
        // walk-on-upgrade must re-apply every event so the cache
        // catches up to the current dispatch, then stamp the new
        // cache version to gate future fast paths.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("walk.db");
        {
            let db = Database::open(&path).unwrap();
            // Author some events the normal way.
            db.insert_label("Focus").unwrap();
            db.insert_label("Calm").unwrap();
            assert_eq!(db.list_labels().unwrap().len(), 2);
            // Manually delete the cache rows AND roll the cache
            // version back to 0 — pretending the previous build had
            // recorded these events without materialising them.
            db.conn.execute("DELETE FROM labels", []).unwrap();
            db.set_sync_state(CACHE_SCHEMA_VERSION_KEY, "0").unwrap();
            assert!(db.list_labels().unwrap().is_empty(),
                "labels cache must be empty before the re-open");
        }
        // Reopen. init must walk and re-materialise the labels.
        let db = Database::open(&path).unwrap();
        let labels = db.list_labels().unwrap();
        assert_eq!(labels.len(), 2, "labels must be re-materialised from event log");
        let mut names: Vec<_> = labels.iter().map(|l| l.name.clone()).collect();
        names.sort();
        assert_eq!(names, vec!["Calm".to_string(), "Focus".to_string()]);
        // Marker advances so the next open skips the walk.
        assert_eq!(
            db.get_sync_state(CACHE_SCHEMA_VERSION_KEY, "missing").unwrap(),
            CACHE_SCHEMA_VERSION.to_string(),
        );
    }

    #[test]
    fn re_open_at_current_cache_version_does_not_re_walk() {
        // Fast path: a DB already at the current cache version must
        // skip the walk so re-opens stay O(1) regardless of how many
        // events the log holds.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fast.db");
        {
            let db = Database::open(&path).unwrap();
            db.insert_label("Focus").unwrap();
            // Manually corrupt the cache and leave the marker at
            // current — a re-walk would fix this; the fast path must
            // therefore leave it broken (proving the walk was
            // skipped).
            db.conn.execute("DELETE FROM labels", []).unwrap();
            assert_eq!(
                db.get_sync_state(CACHE_SCHEMA_VERSION_KEY, "missing").unwrap(),
                CACHE_SCHEMA_VERSION.to_string(),
            );
        }
        let db = Database::open(&path).unwrap();
        assert!(
            db.list_labels().unwrap().is_empty(),
            "fast path must NOT have re-walked (labels stay empty)",
        );
    }

    // ── Schema version sentinel ───────────────────────────────────────

    #[test]
    fn open_in_memory_stamps_current_schema_version() {
        // A fresh DB starts at user_version=0; init must apply schema
        // and stamp SCHEMA_VERSION so a future reopen finds a matching
        // value rather than treating the DB as fresh again.
        let db = Database::open_in_memory().unwrap();
        let v: u32 = db.conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(v, SCHEMA_VERSION);
    }

    #[test]
    fn init_refuses_db_written_by_a_future_build() {
        // A DB whose user_version exceeds our SCHEMA_VERSION was written
        // by a build that may have added forward-only columns; opening
        // it risks silent corruption on subsequent writes.
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(&format!(
            "PRAGMA user_version = {};", SCHEMA_VERSION + 1
        )).unwrap();
        match Database::init(conn) {
            Err(DbError::SchemaVersionTooNew { db, build }) => {
                assert_eq!(db, SCHEMA_VERSION + 1);
                assert_eq!(build, SCHEMA_VERSION);
            }
            Err(other) => panic!("expected SchemaVersionTooNew, got {other:?}"),
            Ok(_) => panic!("expected error, init succeeded"),
        }
    }

    #[test]
    fn init_accepts_db_already_at_current_schema_version() {
        // Reopening a previously-stamped DB is the common case after
        // the first launch; init must accept it without re-stamping
        // or rejecting.
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(&format!(
            "PRAGMA user_version = {};", SCHEMA_VERSION
        )).unwrap();
        Database::init(conn).expect("init succeeds at current version");
    }

    // ── session_in_progress table ─────────────────────────────────────

    #[test]
    fn open_in_memory_creates_session_in_progress_table() {
        // The table is created at init via CREATE TABLE IF NOT EXISTS
        // alongside the others. Empty on a fresh DB — the shell writes
        // the single row at session start.
        let db = Database::open_in_memory().unwrap();
        let count: i64 = db.conn
            .query_row("SELECT COUNT(*) FROM session_in_progress", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0,
            "session_in_progress is empty on a fresh DB");
    }

    #[test]
    fn session_in_progress_rejects_id_not_equal_to_one() {
        // The CHECK constraint enforces single-row semantics: the
        // shell can have at most one in-flight session, period.
        // Inserting id=2 must fail at the CHECK level (not just any
        // SQLite error) so a buggy caller can't accidentally
        // accumulate ghost rows.
        let db = Database::open_in_memory().unwrap();
        let res = db.conn.execute(
            "INSERT INTO session_in_progress
                (id, start_iso, accumulated_secs, mode, mode_payload, label_id, guided_file_uuid)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                2_i64,
                "2026-05-13T10:00:00",
                0_i64,
                "timer",
                "{}",
                None::<i64>,
                None::<String>,
            ],
        );
        match res {
            Err(rusqlite::Error::SqliteFailure(err, _))
                if err.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_CHECK => {}
            other => panic!("expected SQLITE_CONSTRAINT_CHECK, got {other:?}"),
        }
    }

    #[test]
    fn session_in_progress_accepts_a_single_row_with_id_one() {
        // Sanity: the schema permits the legitimate single-row write
        // the shell will issue. UPSERT on the id=1 PK keeps the row
        // singleton; this test exercises the bare INSERT path.
        let db = Database::open_in_memory().unwrap();
        db.conn.execute(
            "INSERT INTO session_in_progress
                (id, start_iso, accumulated_secs, mode, mode_payload, label_id, guided_file_uuid)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                1_i64,
                "2026-05-13T10:00:00",
                60_i64,
                "timer",
                "{}",
                None::<i64>,
                None::<String>,
            ],
        ).expect("legitimate single-row write succeeds");
        let count: i64 = db.conn
            .query_row("SELECT COUNT(*) FROM session_in_progress", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn session_in_progress_rejects_unknown_mode() {
        // CHECK constraint on the mode column mirrors `sessions.mode`
        // — only the three known mode strings are accepted. Verifies
        // the actual CHECK error specifically, not any failure.
        let db = Database::open_in_memory().unwrap();
        let res = db.conn.execute(
            "INSERT INTO session_in_progress
                (id, start_iso, accumulated_secs, mode, mode_payload, label_id, guided_file_uuid)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                1_i64,
                "2026-05-13T10:00:00",
                0_i64,
                "stopwatch",
                "{}",
                None::<i64>,
                None::<String>,
            ],
        );
        match res {
            Err(rusqlite::Error::SqliteFailure(err, _))
                if err.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_CHECK => {}
            other => panic!("expected SQLITE_CONSTRAINT_CHECK, got {other:?}"),
        }
    }

    // ── busy_timeout ──────────────────────────────────────────────────

    #[test]
    fn open_sets_busy_timeout_on_file_backed_connection() {
        // Without a busy_timeout, a main-thread `set_setting` racing
        // the sync worker's `replay_events` transaction returns
        // SQLITE_BUSY instantly. We explicitly set 8s rather than
        // relying on rusqlite's current 5s default — the value is
        // part of our runtime contract, not an inherited accident.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("busy.db");
        let db = Database::open(&path).unwrap();
        let timeout_ms: i64 = db.conn
            .query_row("PRAGMA busy_timeout", [], |row| row.get(0))
            .unwrap();
        assert_eq!(timeout_ms, 8000,
            "Database::open must explicitly set busy_timeout to 8s");
    }
}
