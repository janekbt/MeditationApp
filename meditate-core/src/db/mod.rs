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

use events::EventKind;
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
pub(crate) use known_remote::Known;
pub(crate) mod local_guided_files;
mod labels;
mod local_changes;
mod order_key;
mod presets;
mod schema;
mod seeds;
mod session_in_progress;
mod sessions;
mod starred_order;
mod settings;
mod sync_state;
mod uuids;
mod vibration_patterns;

#[cfg(test)]
pub(crate) mod test_helpers;

pub use bell_sounds::{BellSound, BellSoundCategory};
pub use crate::breath::{BoxBreathPhase, BoxBreathPhaseId};
pub use events::{Event, Refresh};
pub use guided_files::{
    find_guided_file_by_uuid_from_db, guided_file_name_for, guided_file_names_from_db,
    is_guided_file_name_taken_from_db, list_guided_files_from_db, GuidedFile,
};
pub use interval_bells::{IntervalBell, IntervalBellKind};
pub use local_guided_files::{
    find_local_file_from_db, list_local_files_from_db, list_starred_local_files_from_db,
    local_folder_from_db, walk_folder, FolderScan, LocalGuidedFile,
    ScannedFile,
};
// Domain enums whose persisted-string mapping lives with the type
// definition in the domain module. Re-exported here so the `db`
// API surface stays stable across the promotion pass.
pub use crate::bells::SignalMode;
pub use local_changes::{LocalChangeWatch, LocalChanges};
pub use labels::{
    count_labels_from_db, find_label_by_name_from_db, is_label_name_taken_from_db, label_color_places_from_db, LABEL_COLOR_COUNT,
    label_names_from_db, label_session_count_from_db, list_labels_from_db, Label,
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
    list_sessions_from_db, query_sessions_from_db,
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
use error::{conflict_suffixed_name, is_unique_constraint_error, map_unique_err, strip_conflict_suffix};
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
    /// The screens the open write's local events touch so far; counted
    /// at its commit, when a sync on another connection can see it.
    changed: std::cell::Cell<Refresh>,
    /// What committed writes touched, until `take_committed`.
    committed: std::cell::Cell<Refresh>,
    /// Writes open now (`Tx`s alive); 0 means the next is outermost.
    depth: std::cell::Cell<u32>,
    /// A nested write's undo failed, so its writes may still be there:
    /// the outer write must not commit.
    broken: std::cell::Cell<bool>,
}

/// A write from [`Database::tx`]. Dropped without [`Tx::commit`] (an
/// error, a panic) it undoes its own writes and nothing outside it.
pub(crate) struct Tx<'a> {
    db: &'a Database,
    outer: bool,
    /// What was marked before this write began, so an undone nested
    /// write drops only its own.
    changed_before: Refresh,
    committed: bool,
}

/// A write refused because the write it is part of can't land whole.
fn aborted(why: &str) -> DbError {
    rusqlite::Error::SqliteFailure(rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_ABORT), Some(why.into())).into()
}

impl Tx<'_> {
    pub(crate) fn commit(mut self) -> Result<()> {
        if self.db.broken.get() {
            // Drop undoes all of it.
            return Err(aborted("a part of this write could not be undone"));
        }
        self.db.conn.execute_batch(if self.outer { "COMMIT" } else { "RELEASE w" })?;
        self.committed = true;
        if self.outer {
            let changed = self.db.changed.take();
            if changed.any() {
                self.db.committed.set(self.db.committed.get() | changed);
                self.db.local_changes.bump();
            }
        }
        Ok(())
    }
}

impl Drop for Tx<'_> {
    fn drop(&mut self) {
        self.db.depth.set(self.db.depth.get() - 1);
        if self.committed {
            return;
        }
        // Drop can't return it; the write's own error already went up.
        // Back in autocommit, SQLite undid it all itself (disk full).
        if !self.db.conn.is_autocommit() {
            let undo = if self.outer { "ROLLBACK" } else { "ROLLBACK TO w; RELEASE w" };
            if let Err(e) = self.db.conn.execute_batch(undo) {
                crate::diag::log("db.tx", &format!("undo failed: {e}"));
                self.db.broken.set(!self.outer);
            }
        }
        if self.outer {
            self.db.broken.set(false);
        }
        self.db.changed.set(if self.outer { Refresh::NONE } else { self.changed_before });
    }
}

impl std::ops::Deref for Tx<'_> {
    type Target = Connection;
    fn deref(&self) -> &Connection {
        &self.db.conn
    }
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
        // plus the per-tab stats queries. 64 fits the working set
        // with headroom, now that every table's readers, deletes and
        // the known-remote trackers are cached too; cost is
        // per-statement memory, negligible at this count.
        conn.set_prepared_statement_cache_capacity(64);
        let db = Self { conn, local_changes: LocalChanges::default(), changed: std::cell::Cell::default(), committed: std::cell::Cell::default(), depth: std::cell::Cell::new(0), broken: std::cell::Cell::new(false) };
        db.rebuild_events_from_older_builds()?;
        db.rename_unrebuilt_kansho()?;
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
        use rusqlite::OptionalExtension;
        let sql = format!("SELECT value FROM {table} WHERE key = ?1");
        let value = self.conn.query_row(&sql, rusqlite::params![key], |row| row.get(0)).optional()?;
        Ok(value.unwrap_or_else(|| default.to_string()))
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

    /// Start one write. The outermost is a transaction that waits for
    /// the lock; one started inside it is a savepoint, so writes nest.
    /// IMMEDIATE takes the lock at the start, so a write waits out
    /// busy_timeout; a deferred one that reads first would fail at once
    /// (SQLITE_BUSY) when another connection writes in between.
    pub(crate) fn tx(&self) -> Result<Tx<'_>> {
        let outer = self.depth.get() == 0;
        if outer && !self.conn.is_autocommit() {
            // Left open by an undo that failed (its write already said
            // so); nesting in it would lose every later write.
            crate::diag::log("db.tx", "ending a transaction no write owns");
            self.conn.execute_batch("ROLLBACK")?;
        }
        if !outer && self.conn.is_autocommit() {
            // SQLite undid the write this one is part of (disk full);
            // a savepoint now would commit on its own.
            return Err(aborted("the write this belongs to was undone"));
        }
        self.conn.execute_batch(if outer { "BEGIN IMMEDIATE" } else { "SAVEPOINT w" })?;
        self.depth.set(self.depth.get() + 1);
        Ok(Tx { db: self, outer, changed_before: self.changed.get(), committed: false })
    }

    /// Write a new synced row through `write` and return its rowid. A
    /// row already there under `uuid` (a repeated import or seed) is
    /// returned as it is, with no write and no event.
    pub(super) fn insert_new_in(
        &self,
        table: &'static str,
        uuid: &str,
        write: impl FnOnce() -> Result<()>,
    ) -> Result<i64> {
        if let Some(existing) = self.existing_rowid_by_uuid(table, uuid)? {
            return Ok(existing);
        }
        write()?;
        Ok(self.existing_rowid_by_uuid(table, uuid)?.expect("just written"))
    }

    /// Delete the synced row `uuid` and record `kind`, its tombstone.
    /// An unknown uuid writes nothing and sends no tombstone: a peer
    /// would otherwise get one for a row it never knew.
    pub(super) fn delete_synced_row(&self, table: &'static str, uuid: &str, kind: EventKind) -> Result<bool> {
        let tx = self.tx()?;
        let deleted = self.delete_row(table, uuid)?;
        if deleted {
            self.emit_tombstone(&tx, kind, uuid)?;
        }
        tx.commit()?;
        Ok(deleted)
    }

    /// Delete row `uuid` from `table`; the caller decides on the event.
    /// True when a row was there.
    pub(super) fn delete_row(&self, table: &'static str, uuid: &str) -> Result<bool> {
        let sql = format!("DELETE FROM {table} WHERE uuid = ?1");
        Ok(self.conn.prepare_cached(&sql)?.execute(rusqlite::params![uuid])? > 0)
    }

    pub(super) fn emit_tombstone(&self, tx: &Tx<'_>, kind: EventKind, uuid: &str) -> Result<()> {
        self.emit_event(tx, kind, uuid, serde_json::json!({ "uuid": uuid }).to_string())
    }

    /// Every row `sql` returns, each read by `from_row`, collected
    /// into a `Vec`, a set or a map.
    pub(super) fn query_rows<T, C: FromIterator<T>>(
        &self,
        sql: &str,
        params: impl rusqlite::Params,
        from_row: impl FnMut(&rusqlite::Row) -> rusqlite::Result<T>,
    ) -> Result<C> {
        let mut stmt = self.conn.prepare_cached(sql)?;
        let rows = stmt.query_map(params, from_row)?.collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    /// The first row `sql` returns, read by `from_row`, or `None`.
    pub(super) fn query_one<T>(
        &self,
        sql: &str,
        params: impl rusqlite::Params,
        from_row: impl FnOnce(&rusqlite::Row) -> rusqlite::Result<T>,
    ) -> Result<Option<T>> {
        use rusqlite::OptionalExtension;
        Ok(self.conn.prepare_cached(sql)?.query_row(params, from_row).optional()?)
    }

    /// `SELECT id FROM <table> WHERE uuid = ?1`. Used by every
    /// `insert_*_with_uuid` entry point to make the insert
    /// idempotent — a row with the requested uuid already present
    /// returns its rowid without writing anything (and without
    /// emitting a sync event, since no state changed). The pre-
    /// flight check is what makes "import the same payload twice"
    /// safe across sync replay.
    pub(super) fn uuid_by_id(&self, table: &'static str, id: i64) -> Result<Option<String>> {
        self.query_one(&format!("SELECT uuid FROM {table} WHERE id = ?1"), rusqlite::params![id], |row| row.get(0))
    }

    pub(crate) fn existing_rowid_by_uuid(
        &self,
        table: &'static str,
        uuid: &str,
    ) -> Result<Option<i64>> {
        self.query_one(&format!("SELECT id FROM {table} WHERE uuid = ?1"), rusqlite::params![uuid], |row| row.get(0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_macros::assert_matches;

    fn kv(db: &Database, key: &str) -> Option<String> {
        db.query_one("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0)).unwrap()
    }

    fn put(db: &Database, key: &str) -> Result<()> {
        db.conn.execute("INSERT INTO settings (key, value) VALUES (?1, 'x')", [key])?;
        Ok(())
    }

    #[test]
    fn a_transaction_left_open_by_a_failed_undo_does_not_swallow_later_writes() {
        // An undo that failed left the connection in a transaction no
        // write owns; the next write must still land, not nest in it.
        let db = Database::open_in_memory().unwrap();
        db.conn.execute_batch("BEGIN;").unwrap();
        db.set_setting("k", "v").unwrap();
        assert!(db.conn.is_autocommit(), "the write committed");
        assert_eq!(kv(&db, "k").as_deref(), Some("v"));
    }

    #[test]
    fn an_unchanged_write_after_a_change_keeps_the_change_signal() {
        // An unchanged setting returns without committing its nested
        // write; the change before it in the same write still counts.
        let db = Database::open_in_memory().unwrap();
        let mut watch = LocalChangeWatch::new(db.local_changes());
        let outer = db.tx().unwrap();
        db.set_setting("k", "v").unwrap();
        db.set_setting("k", "v").unwrap();
        outer.commit().unwrap();
        assert!(watch.take_new());
    }

    #[test]
    fn a_write_inside_one_sqlite_undid_itself_fails() {
        // Disk full: SQLite rolls the whole write back on its own. A
        // nested write after that must fail, not commit on its own.
        let db = Database::open_in_memory().unwrap();
        let outer = db.tx().unwrap();
        db.conn.execute_batch("ROLLBACK;").unwrap();
        assert!(db.set_setting("k", "v").is_err());
        assert_eq!(kv(&db, "k"), None);
        drop(outer);
        db.set_setting("k", "w").expect("the next write works");
    }

    #[test]
    fn a_nested_write_whose_undo_failed_fails_the_whole_write() {
        // Its writes may still be there: the outer commit must not keep
        // half of it.
        let db = Database::open_in_memory().unwrap();
        let outer = db.tx().unwrap();
        put(&db, "outer").unwrap();
        let inner = db.tx().unwrap();
        put(&db, "inner").unwrap();
        db.conn.execute_batch("RELEASE w;").unwrap(); // its undo now fails
        drop(inner);
        assert!(outer.commit().is_err());
        assert_eq!(kv(&db, "inner"), None);
        assert_eq!(kv(&db, "outer"), None);
        db.set_setting("k", "v").expect("the next write works");
    }

    #[test]
    fn a_nested_write_undone_sends_no_change_signal() {
        let db = Database::open_in_memory().unwrap();
        let mut watch = LocalChangeWatch::new(db.local_changes());
        let outer = db.tx().unwrap();
        put(&db, "outer").unwrap();
        let inner = db.tx().unwrap();
        db.set_setting("k", "v").unwrap();
        drop(inner);
        outer.commit().unwrap();
        assert!(!watch.take_new(), "the only event was undone");
    }

    #[test]
    fn a_nested_write_that_fails_rolls_back_only_itself() {
        let db = Database::open_in_memory().unwrap();
        let outer = db.tx().unwrap();
        put(&db, "outer").unwrap();
        let inner = db.tx().unwrap();
        put(&db, "inner").unwrap();
        drop(inner); // an error before its commit
        outer.commit().unwrap();
        assert_eq!(kv(&db, "outer").as_deref(), Some("x"));
        assert_eq!(kv(&db, "inner"), None);
    }

    #[test]
    fn an_outer_write_that_fails_rolls_back_the_nested_ones() {
        let db = Database::open_in_memory().unwrap();
        let outer = db.tx().unwrap();
        let inner = db.tx().unwrap();
        put(&db, "inner").unwrap();
        inner.commit().unwrap();
        put(&db, "outer").unwrap();
        drop(outer); // an error before its commit
        assert_eq!(kv(&db, "inner"), None);
        assert_eq!(kv(&db, "outer"), None);
        // The connection is usable again: the next write commits.
        let next = db.tx().unwrap();
        put(&db, "after").unwrap();
        next.commit().unwrap();
        assert_eq!(kv(&db, "after").as_deref(), Some("x"));
    }

    #[test]
    fn every_list_has_the_same_order_on_every_device() {
        // Rowids follow the order a device received its rows in, so a
        // list sorted by them differs between devices. Each table gets
        // row "b" first and row "a" second; "a" is created earlier, or
        // at the same instant, so it must come first everywhere.
        let db = Database::open_in_memory().unwrap();
        let tables = ["bell_sounds", "vibration_patterns", "guided_files", "presets", "interval_bells"];
        db.insert_bell_sound_with_uuid("b", "B", "/b.ogg", false, "audio/ogg", BellSoundCategory::General).unwrap();
        db.insert_bell_sound_with_uuid("a", "A", "/a.ogg", false, "audio/ogg", BellSoundCategory::General).unwrap();
        db.insert_vibration_pattern_with_uuid("b", "B", 100, &[1.0], ChartKind::Line, false).unwrap();
        db.insert_vibration_pattern_with_uuid("a", "A", 100, &[1.0], ChartKind::Line, false).unwrap();
        db.insert_guided_file_with_uuid("b", "B", "/b.ogg", 60, false).unwrap();
        db.insert_guided_file_with_uuid("a", "A", "/a.ogg", 60, false).unwrap();
        db.insert_preset_with_uuid("b", "B", SessionMode::Timer, false, "{}").unwrap();
        db.insert_preset_with_uuid("a", "A", SessionMode::Timer, false, "{}").unwrap();
        for uuid in ["b", "a"] {
            db.insert_interval_bell(IntervalBellKind::Interval, crate::bells::BellTime::new(5, crate::bells::TimeUnit::Min), crate::bells::BellTime::ZERO, crate::bells::BellTime::ZERO, "", "", SignalMode::Sound).unwrap();
            db.conn.execute("UPDATE interval_bells SET uuid = ?1 WHERE uuid NOT IN ('a', 'b')", [uuid]).unwrap();
            db.insert_session(&Session {
                start_iso: "2026-01-01T08:00:00".into(),
                duration_secs: 60,
                label_id: None,
                notes: None,
                mode: SessionMode::Timer,
                uuid: SessionUuid::new(""),
                guided_file_uuid: None,
                guided_file_name: None,
            })
            .unwrap();
            db.conn.execute("UPDATE sessions SET uuid = ?1 WHERE uuid NOT IN ('a', 'b')", [uuid]).unwrap();
        }
        let (older, newer) = ("2026-01-01T00:00:00+00:00", "2026-01-02T00:00:00+00:00");
        for b_created in [newer, older] {
            for t in tables {
                db.conn.execute(&format!("UPDATE {t} SET created_iso = ?1 WHERE uuid = 'b'"), [b_created]).unwrap();
                db.conn.execute(&format!("UPDATE {t} SET created_iso = ?1 WHERE uuid = 'a'"), [older]).unwrap();
            }
            let ab = ["a", "b"];
            assert_eq!(db.list_bell_sounds().unwrap().iter().map(|r| r.uuid.as_str()).collect::<Vec<_>>(), ab);
            assert_eq!(list_vibration_patterns_from_db(&db).unwrap().iter().map(|r| r.uuid.as_str()).collect::<Vec<_>>(), ab);
            assert_eq!(list_guided_files_from_db(&db).unwrap().iter().map(|r| r.uuid.as_str()).collect::<Vec<_>>(), ab);
            assert_eq!(list_presets_from_db(&db).unwrap().iter().map(|r| r.uuid.as_str()).collect::<Vec<_>>(), ab);
            assert_eq!(db.list_interval_bells().unwrap().iter().map(|r| r.uuid.as_str()).collect::<Vec<_>>(), ab);
        }
        let sessions = query_sessions_from_db(&db, &SessionFilter::default()).unwrap();
        assert_eq!(sessions.iter().map(|(_, s)| s.uuid.as_str()).collect::<Vec<_>>(), ["a", "b"]);
    }

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
    fn re_open_with_old_cache_version_rebuilds_the_cache() {
        // Simulate a DB that was last opened by a build whose
        // apply_event_record_only skipped some kind we now understand.
        // The start-up rebuild must rebuild every event's target once so
        // the cache catches up to the current dispatch, then stamp the
        // new cache version to gate future fast paths.
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
    fn a_bell_from_the_last_release_keeps_its_times() {
        use crate::bells::{BellTime, TimeUnit};
        let conn = crate::db::test_helpers::release_v26_10_1_database();
        conn.execute_batch(
            "INSERT INTO interval_bells (uuid, kind, minutes, jitter_pct, created_iso)
                 VALUES ('b1', 'interval', 7, 14, '2026-10-01T00:00:00Z');",
        )
        .unwrap();
        let db = Database::init(conn).unwrap();
        let b = &db.list_interval_bells().unwrap()[0];
        assert_eq!(
            (b.time, b.jitter, b.step),
            (BellTime::new(7, TimeUnit::Min), BellTime::new(59, TimeUnit::Sec), BellTime::ZERO)
        );
    }

    fn bell_event(lamport_ts: i64, step_secs: u32) -> String {
        serde_json::json!({
            "uuid": "b1", "kind": "interval", "minutes": 2, "jitter_pct": 33,
            "secs": 90, "secs_unit": "s", "jitter_secs": 30, "jitter_unit": "s",
            "step_secs": step_secs, "step_unit": "s",
            "sound_uuid": crate::seeds::BUNDLED_BOWL_UUID, "enabled": true,
            "created_iso": "2026-10-01T00:00:00Z", "volume": 50,
            "lamport": lamport_ts,
        })
        .to_string()
    }

    fn times(db: &Database) -> (crate::bells::BellTime, crate::bells::BellTime, crate::bells::BellTime) {
        let b = &db.list_interval_bells().unwrap()[0];
        (b.time, b.jitter, b.step)
    }

    #[test]
    fn a_bell_a_newer_device_sent_before_the_update_keeps_its_seconds() {
        use crate::bells::{BellTime, TimeUnit::Sec};
        // 26.10.1 applied the newer device's event: it kept the payload
        // and filled only the columns it knows.
        let conn = crate::db::test_helpers::release_v26_10_1_database();
        conn.execute(
            "INSERT INTO events (event_uuid, lamport_ts, device_id, kind, target_id, payload)
             VALUES ('e1', 7, 'phone', 'interval_bell_insert', 'b1', ?1)",
            [bell_event(7, 5)],
        )
        .unwrap();
        conn.execute_batch(
            "INSERT INTO interval_bells (uuid, kind, minutes, jitter_pct, created_iso)
                 VALUES ('b1', 'interval', 2, 33, '2026-10-01T00:00:00Z');",
        )
        .unwrap();
        let db = Database::init(conn).unwrap();
        assert_eq!(times(&db), (BellTime::new(90, Sec), BellTime::new(30, Sec), BellTime::new(5, Sec)));
    }

    #[test]
    fn a_change_received_on_the_older_version_is_kept_after_updating_again() {
        use crate::bells::{BellTime, TimeUnit::Sec};
        let db = Database::open_in_memory().unwrap();
        db.apply_event(&Event {
            event_uuid: "e1".into(), lamport_ts: 7, device_id: "phone".into(),
            kind: "interval_bell_insert".into(), target_id: "b1".into(), payload: bell_event(7, 5),
        })
        .unwrap();
        // Back on 26.10.1, the step changes on the phone: the release
        // records the event and fills only the columns it knows, which
        // stay the same (2 min, 33 %).
        db.conn
            .execute(
                "INSERT INTO events (event_uuid, lamport_ts, device_id, kind, target_id, payload)
                 VALUES ('e2', 8, 'phone', 'interval_bell_update', 'b1', ?1)",
                [bell_event(8, 10)],
            )
            .unwrap();
        let Database { conn, .. } = db;
        let db = Database::init(conn).unwrap();
        assert_eq!(times(&db).2, BellTime::new(10, Sec));
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
