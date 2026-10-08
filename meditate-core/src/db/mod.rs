//! SQLite persistence tier: the materialised cache tables, the
//! append-only event log, the apply/replay dispatch that
//! materialises events into cache rows, and the per-entity CRUD
//! around it. Submodules split the surface by entity type
//! (sessions, labels, presets, bell_sounds, …) plus the cross-
//! cutting machinery (events, device, schema, error, seeds).
//!
//! `Database` is the single owning struct — every public method on
//! it is part of the contract the shells consume. The `pub(super)`
//! helpers (read_kv / write_kv / existing_rowid_by_uuid /
//! map_unique_err / winning_mutate) are crate-internal building
//! blocks shared across the submodules.
//!
//! **Reader / writer convention.** Read-only helpers are free
//! functions named `*_from_db` in their domain submodule (e.g.
//! `labels::list_labels_from_db(db: &Database)`); mutating
//! helpers stay as methods on `Database`. Free fns are re-exported
//! at `meditate_core::db::*_from_db` so callers don't import the
//! submodule. The naming makes reads vs writes visible at every
//! call site — a `db.foo()` call mutates by inspection, a
//! `foo_from_db(db)` call reads.
//!
//! No GTK dependencies; an Android shell consumes this crate
//! verbatim. Mechanism conventions (per-connection PRAGMAs, the
//! 8-second busy_timeout, prepare_cached for hot loops) live
//! inside `init`.

use rusqlite::Connection;
use std::path::Path;
use std::time::Duration;

/// How long an app write waits for another connection's write lock.
const APP_BUSY_TIMEOUT: Duration = Duration::from_secs(1);
/// The sync worker runs off the UI thread and can afford to wait.
pub const SYNC_BUSY_TIMEOUT: Duration = Duration::from_secs(8);

mod bell_sounds;
mod box_breath_phases;
mod device;
mod error;
mod events;
mod guided_files;
mod interval_bells;
mod known_remote;
mod labels;
mod local_changes;
mod presets;
mod schema;
mod seeds;
mod session_in_progress;
mod sessions;
mod settings;
mod sync_state;
mod uuids;
mod vibration_patterns;

#[cfg(test)]
mod test_helpers;

pub use bell_sounds::{BellSound, BellSoundCategory};
pub use crate::breath::{BoxBreathPhase, BoxBreathPhaseId};
pub use events::Event;
pub use guided_files::{
    find_guided_file_by_uuid_from_db, guided_file_name_for, guided_file_names_from_db,
    is_guided_file_name_taken_from_db, list_guided_files_from_db, GuidedFile,
};
pub use interval_bells::{IntervalBell, IntervalBellKind};
// Domain enums whose persisted-string mapping lives with the type
// definition in the domain module. Re-exported here so the `db`
// API surface stays stable across the promotion pass.
pub use crate::bells::SignalMode;
pub use local_changes::{LocalChangeWatch, LocalChanges};
pub use labels::{
    count_labels_from_db, find_label_by_name_from_db, is_label_name_taken_from_db,
    label_session_count_from_db, list_labels_from_db, Label,
};
pub use presets::{
    find_preset_by_uuid_from_db, is_preset_name_taken_from_db,
    list_presets_for_mode_from_db, list_presets_from_db, list_starred_presets_for_mode_from_db,
    Preset,
};
pub use session_in_progress::{FinalizedSession, SessionInProgress};
pub use sessions::{
    active_days_in_month_from_db, active_months_from_db, count_sessions_from_db, get_best_streak_from_db,
    get_daily_totals_from_db, get_daily_totals_since_from_db,
    get_longest_session_from_db, get_median_duration_secs_from_db,
    get_running_average_secs_from_db, get_streak_from_db,
    hour_buckets_from_db, label_totals_seconds_from_db, list_sessions_for_label_from_db,
    list_sessions_from_db, month_total_secs_from_db, query_sessions_from_db,
    total_secs_since_from_db,
    total_seconds_from_db, Session, SessionFilter, SessionMode,
};
pub use uuids::{
    BellSoundUuid, GuidedFileUuid, IntervalBellUuid, LabelUuid, PresetUuid,
    SessionUuid, VibrationPatternUuid,
};
pub use vibration_patterns::{
    find_vibration_pattern_by_uuid_from_db, is_vibration_pattern_name_taken_from_db,
    list_vibration_patterns_from_db, VibrationPattern,
};
pub use crate::vibration::ChartKind;
pub use error::{target_id_is_well_formed_for, DbError, Result};
pub(crate) use schema::{CACHE_SCHEMA_VERSION, CACHE_SCHEMA_VERSION_KEY, SCHEMA_VERSION};
use error::{conflict_suffixed_name, is_unique_constraint_error, map_unique_err};
use schema::schema;

/// Whether a library row (preset, guided file) is currently flagged
/// as a favourite. Persisted as a SQLite INTEGER 0/1. Used at the
/// `update_*_starred` / `set_*_starred` API surface so callers can't
/// pass the wrong bool to the wrong method — `foo(uuid, true)` reads
/// no more clearly than `foo(uuid, StarredState::Starred)` writes,
/// but the typed form refuses to accept a `bool` from elsewhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StarredState {
    Starred,
    Unstarred,
}

impl StarredState {
    pub fn from_flag(is_starred: bool) -> Self {
        if is_starred { StarredState::Starred } else { StarredState::Unstarred }
    }
    pub fn is_starred(self) -> bool {
        matches!(self, StarredState::Starred)
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
    local_changes: LocalChanges,
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
    /// Open an ephemeral in-memory database — same shape as
    /// `open`, including the full SQL schema, but with no on-disk
    /// state and no WAL pragmas (a no-op on `:memory:`). Used by
    /// every unit test in the workspace as the standard fixture.
    ///
    /// Like `open`, the caller still has to drive the seeds
    /// (`seed_all_non_audio`, `seed_bell_sounds_with_paths`) —
    /// `Database::open*` does the schema + integrity check only.
    ///
    /// # Example
    ///
    /// ```
    /// use meditate_core::Database;
    /// let db = Database::open_in_memory().unwrap();
    /// // Schema is applied; no seeds yet.
    /// assert_eq!(meditate_core::db::list_labels_from_db(&db).unwrap().len(), 0);
    /// db.seed_default_labels().unwrap();
    /// assert!(meditate_core::db::list_labels_from_db(&db).unwrap().len() >= 2);
    /// ```
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        Self::init(conn)
    }

    /// Open or create the SQLite database at `path`. On return:
    ///
    /// - The full SQL schema is applied (idempotent — re-opens
    ///   skip table creation via `CREATE TABLE IF NOT EXISTS`).
    /// - `PRAGMA user_version` is stamped at `SCHEMA_VERSION`.
    ///   A DB whose version exceeds this build is rejected with
    ///   `DbError::SchemaVersionTooNew` (downgrade guard).
    /// - WAL is on; `synchronous=NORMAL`; `foreign_keys=ON`;
    ///   `busy_timeout=8s`.
    /// - `PRAGMA quick_check` has run; corruption is logged via
    ///   `diag::log` but does NOT block the open.
    /// - Reads + writes are immediately safe.
    ///
    /// Seed data (default labels, presets, vibration patterns,
    /// bell sounds) is NOT inserted here — the caller drives those
    /// via `seed_all_non_audio` + the shell-specific
    /// `seed_bell_sounds_with_paths` after open returns. This
    /// keeps the platform-specific bits (gresource paths on gtk,
    /// asset URIs on Android) out of core.
    ///
    /// Not thread-safe: a `Database` value wraps a single rusqlite
    /// `Connection`; callers needing cross-thread access wrap the
    /// value in `Arc<Mutex<_>>`.
    pub fn open(path: &Path) -> Result<Self> {
        // Touch the file with mode 0600 BEFORE letting SQLite open it,
        // so a fresh DB lands user-only-readable. `rusqlite::Connection::
        // open` creates the file under the default umask (typically
        // 0644 on Linux) — fine inside the Flatpak sandbox, but
        // exposes session contents on a non-Flatpak install where the
        // home dir isn't private. Existing files are left as-is; users
        // who've explicitly chmod'd their DB don't get clobbered. Skip
        // the touch for SQLite's `":memory:"` special path (callers
        // should use `open_in_memory` for that, but a `Path` argument
        // can technically carry the string).
        #[cfg(unix)]
        if path.to_str() != Some(":memory:") && !path.exists() {
            use std::os::unix::fs::OpenOptionsExt;
            let _ = std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .mode(0o600)
                .open(path);
        }
        let conn = Connection::open(path)?;
        // Wait up to 1s when another writer holds the lock instead of
        // failing instantly with SQLITE_BUSY. The apps write on their
        // UI thread, so the wait is a freeze: 1s covers a normal sync
        // write (milliseconds) and caps the rare long one (a first
        // full pull). The sync worker raises its own connection to
        // SYNC_BUSY_TIMEOUT. Pinned rather than rusqlite's default so
        // a version bump can't silently change it.
        conn.busy_timeout(APP_BUSY_TIMEOUT)?;
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

    /// Change how long a write waits for another connection's lock.
    /// The sync worker calls it with [`SYNC_BUSY_TIMEOUT`].
    pub fn set_busy_timeout(&self, timeout: Duration) -> Result<()> {
        Ok(self.conn.busy_timeout(timeout)?)
    }

    fn init(mut conn: Connection) -> Result<Self> {
        // Writers take the lock when their transaction begins, so they
        // wait out busy_timeout. A deferred transaction that reads
        // first fails at once (SQLITE_BUSY) when another connection
        // writes in between. Every transaction here writes.
        conn.set_transaction_behavior(rusqlite::TransactionBehavior::Immediate);
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
        // 16 MB page cache (default is ~2 MB). On the Librem 5 with
        // 3 GB RAM this is negligible; in exchange the events index
        // stays resident across a sync pull's storm of `INSERT OR
        // IGNORE` + `recompute_*` round-trips. Negative values are
        // SQLite's "size in KiB" convention.
        conn.execute_batch("PRAGMA cache_size=-16000;")?;
        conn.execute_batch(&schema())?;
        schema::add_missing_columns(&conn)?;
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
            crate::diag::log("db.integrity_check", &format!("failed: {integrity}"));
        }
        // Bump the prepared-statement cache so the `recompute_*`
        // family + hot stats queries all stay parsed. rusqlite
        // defaults to 16 — too small once you count the seven
        // recompute targets times two queries each (winning_mutate)
        // plus the per-tab stats queries. 32 fits the working set
        // comfortably with headroom; cost is per-statement memory,
        // negligible at this count.
        conn.set_prepared_statement_cache_capacity(32);
        let db = Self { conn, local_changes: LocalChanges::default() };
        db.maybe_walk_events_for_cache_upgrade()?;
        Ok(db)
    }

    /// Read a `(key, value)`-shaped row from a table where keys are
    /// strings and values are strings. Returns `default` when the key
    /// has never been set. Shared by `get_setting` (event-sourced
    /// settings table) and `get_sync_state` (device-local sync KV)
    /// — both tables have the same `(key TEXT PK, value TEXT)` shape.
    pub(super) fn read_kv(
        &self,
        table: &'static str,
        key: &str,
        default: &str,
    ) -> Result<String> {
        let sql = format!("SELECT value FROM {table} WHERE key = ?1");
        match self.conn.query_row(&sql, rusqlite::params![key], |row| {
            row.get::<_, String>(0)
        }) {
            Ok(val) => Ok(val),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(default.to_string()),
            Err(e) => Err(DbError::Sqlite(e)),
        }
    }

    /// UPSERT a `(key, value)` row. The settings table caller wraps
    /// this in a transaction that also emits a `setting_changed`
    /// event so peers converge; the sync_state caller uses it
    /// directly (sync_state is device-local and doesn't emit).
    pub(super) fn write_kv(
        &self,
        table: &'static str,
        key: &str,
        value: &str,
    ) -> Result<()> {
        let sql = format!(
            "INSERT INTO {table} (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value"
        );
        self.conn.execute(&sql, rusqlite::params![key, value])?;
        Ok(())
    }

    /// `SELECT id FROM <table> WHERE uuid = ?1`. Used by every
    /// `insert_*_with_uuid` entry point to make the insert
    /// idempotent — a row with the requested uuid already present
    /// returns its rowid without writing anything (and without
    /// emitting a sync event, since no state changed). The pre-
    /// flight check is what makes "import the same payload twice"
    /// safe across sync replay.
    pub(super) fn existing_rowid_by_uuid(
        &self,
        table: &'static str,
        uuid: &str,
    ) -> Result<Option<i64>> {
        use rusqlite::OptionalExtension;
        let sql = format!("SELECT id FROM {table} WHERE uuid = ?1");
        Ok(self
            .conn
            .query_row(&sql, rusqlite::params![uuid], |row| row.get::<_, i64>(0))
            .optional()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_macros::assert_matches;

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
            assert_eq!(crate::db::list_labels_from_db(&db).unwrap().len(), 2);
            // Manually delete the cache rows AND roll the cache
            // version back to 0 — pretending the previous build had
            // recorded these events without materialising them.
            db.conn.execute("DELETE FROM labels", []).unwrap();
            db.set_sync_state(CACHE_SCHEMA_VERSION_KEY, "0").unwrap();
            assert!(crate::db::list_labels_from_db(&db).unwrap().is_empty(),
                "labels cache must be empty before the re-open");
        }
        // Reopen. init must walk and re-materialise the labels.
        let db = Database::open(&path).unwrap();
        let labels = crate::db::list_labels_from_db(&db).unwrap();
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
            crate::db::list_labels_from_db(&db).unwrap().is_empty(),
            "fast path must NOT have re-walked (labels stay empty)",
        );
    }

    // ── Upgrade from the last release (per-bell volumes) ─────────────

    /// A database exactly as v26.8.5 left it: its schema, a bell, the
    /// four Box Breath rows, and the cache marked current.
    fn release_v26_8_5_database() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(include_str!("fixtures/schema_v26_8_5.sql")).unwrap();
        conn.execute_batch(
            "PRAGMA user_version = 1;
             INSERT INTO sync_state (key, value) VALUES ('cache_schema_version', '1');
             INSERT INTO interval_bells (uuid, kind, minutes, created_iso)
                 VALUES ('b1', 'interval', 5, '2026-09-01T00:00:00Z');
             INSERT INTO box_breath_phases (phase) VALUES ('in'), ('holdin'), ('out'), ('holdout');",
        )
        .unwrap();
        conn
    }

    #[test]
    fn a_release_database_opens_with_every_bell_at_the_middle_volume() {
        let db = Database::init(release_v26_8_5_database()).unwrap();
        let bells = db.list_interval_bells().unwrap();
        assert_eq!(bells.len(), 1);
        assert_eq!(bells[0].volume, crate::bell_volume::BellVolume::default());
        let phases = db.list_box_breath_phases().unwrap();
        assert_eq!(phases.len(), 4);
        assert!(phases.iter().all(|p| p.volume == crate::bell_volume::BellVolume::default()));
    }

    #[test]
    fn the_upgrade_keeps_volumes_that_arrived_before_it() {
        // A newer peer synced a bell with a volume to a device still on
        // the release: the event was recorded, the release's cache had
        // nowhere to put the volume. Opening with this build replays it.
        let conn = release_v26_8_5_database();
        conn.execute(
            "INSERT INTO events (event_uuid, lamport_ts, device_id, kind, target_id, payload)
             VALUES ('e1', 7, 'peer', 'interval_bell_update', 'b1', ?1)",
            [serde_json::json!({
                "uuid": "b1", "kind": "interval", "minutes": 5, "jitter_pct": 0,
                "sound_uuid": crate::seeds::BUNDLED_BOWL_UUID, "enabled": true,
                "created_iso": "2026-09-01T00:00:00Z", "volume": 30,
            }).to_string()],
        )
        .unwrap();
        let db = Database::init(conn).unwrap();
        assert_eq!(db.list_interval_bells().unwrap()[0].volume.percent(), 30);
    }

    #[test]
    fn the_release_can_still_open_an_upgraded_database() {
        // Going back to the F-Droid release must keep working: the
        // version stamp stays, and the release's writes (which don't
        // name the new column) still succeed.
        let db = Database::init(release_v26_8_5_database()).unwrap();
        let v: u32 = db.conn.query_row("PRAGMA user_version", [], |row| row.get(0)).unwrap();
        assert_eq!(v, 1);
        db.conn
            .execute(
                "INSERT INTO interval_bells (uuid, kind, minutes, created_iso)
                 VALUES ('b2', 'interval', 9, '2026-09-02T00:00:00Z')",
                [],
            )
            .unwrap();
        db.conn
            .execute("UPDATE box_breath_phases SET enabled = 1 WHERE phase = 'in'", [])
            .unwrap();
    }

    #[test]
    fn an_upgraded_database_has_the_same_columns_as_a_fresh_one() {
        fn columns(conn: &Connection, table: &str) -> Vec<(String, String, Option<String>)> {
            let mut stmt = conn
                .prepare(&format!("SELECT name, type, dflt_value FROM pragma_table_info('{table}')"))
                .unwrap();
            let mut cols: Vec<(String, String, Option<String>)> = stmt
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
                .unwrap()
                .map(|c| c.unwrap())
                .collect();
            cols.sort();
            cols
        }
        let fresh = Database::open_in_memory().unwrap();
        let upgraded = Database::init(release_v26_8_5_database()).unwrap();
        let mut stmt = fresh.conn
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'")
            .unwrap();
        let tables: Vec<String> =
            stmt.query_map([], |r| r.get(0)).unwrap().map(|t| t.unwrap()).collect();
        assert!(tables.len() > 10);
        for table in tables {
            assert_eq!(columns(&upgraded.conn, &table), columns(&fresh.conn, &table), "{table}");
        }
    }

    #[test]
    fn opening_twice_is_harmless() {
        let db = Database::init(release_v26_8_5_database()).unwrap();
        let Database { conn, .. } = db;
        let db = Database::init(conn).unwrap();
        assert_eq!(db.list_interval_bells().unwrap().len(), 1);
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
            "PRAGMA user_version = {SCHEMA_VERSION};"
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
        assert_matches!(
            res,
            Err(rusqlite::Error::SqliteFailure(err, _))
                if err.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_CHECK,
        );
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
        assert_matches!(
            res,
            Err(rusqlite::Error::SqliteFailure(err, _))
                if err.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_CHECK,
        );
    }

    // ── busy_timeout ──────────────────────────────────────────────────

    #[test]
    fn open_sets_busy_timeout_on_file_backed_connection() {
        // Without a busy_timeout, a main-thread `set_setting` racing
        // the sync worker's `replay_events` transaction returns
        // SQLITE_BUSY instantly. The values are part of the runtime
        // contract, not rusqlite's inherited default.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("busy.db");
        let db = Database::open(&path).unwrap();
        let timeout_ms = |db: &Database| -> i64 {
            db.conn.query_row("PRAGMA busy_timeout", [], |row| row.get(0)).unwrap()
        };
        assert_eq!(timeout_ms(&db), 1000, "Database::open sets the app's 1s");
        db.set_busy_timeout(SYNC_BUSY_TIMEOUT).unwrap();
        assert_eq!(timeout_ms(&db), 8000, "the sync worker raises its own");
    }

    /// A write that reads first must wait for a sync write on another
    /// connection, not fail: a deferred transaction's read goes stale
    /// when the other writer commits, and SQLite then refuses the
    /// upgrade at once (SQLITE_BUSY_SNAPSHOT), whatever busy_timeout
    /// says. Transactions start IMMEDIATE so they wait for the lock
    /// up front instead.
    #[test]
    fn an_edit_waits_for_a_write_on_another_connection() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("two.db");
        let app = Database::open(&path).unwrap();
        let id = app.insert_label("Old").unwrap();
        let (started, wait) = std::sync::mpsc::channel();
        let sync_path = path.clone();
        let sync = std::thread::spawn(move || {
            let worker = Database::open(&sync_path).unwrap();
            worker.conn.execute_batch(
                "BEGIN IMMEDIATE;
                 INSERT INTO sync_state (key, value) VALUES ('busy-test', '1');",
            ).unwrap();
            started.send(()).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(300));
            worker.conn.execute_batch("COMMIT;").unwrap();
        });
        wait.recv().unwrap();
        app.update_label(id, "New").expect("the edit waits instead of failing");
        sync.join().unwrap();
    }
}
