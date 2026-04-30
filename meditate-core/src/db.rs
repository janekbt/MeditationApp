use rusqlite::{params, Connection, OptionalExtension};
use std::io::{Read, Write};
use std::path::Path;

#[derive(Debug)]
pub enum DbError {
    DuplicateLabel(String),
    Sqlite(rusqlite::Error),
    Csv(String),
}

impl From<rusqlite::Error> for DbError {
    fn from(e: rusqlite::Error) -> Self {
        DbError::Sqlite(e)
    }
}

pub type Result<T> = std::result::Result<T, DbError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Label {
    pub id: i64,
    pub name: String,
    /// Stable cross-device identity, assigned by the DB at insert time.
    /// Same semantics as `Session::uuid` — populated on read.
    pub uuid: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub start_iso: String,
    pub duration_secs: u32,
    pub label_id: Option<i64>,
    pub notes: Option<String>,
    pub mode: SessionMode,
    /// Stable cross-device identity, assigned by the DB at insert time.
    /// Callers may set this to `String::new()` before insert — the value
    /// is overwritten with a freshly generated v4 UUID. Always populated
    /// on read paths.
    pub uuid: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionMode {
    Countdown,
    Stopwatch,
    BoxBreath,
}

impl SessionMode {
    /// On-disk and CSV string representation. Exposed so callers
    /// (CSV import/export, debug logging) don't need to re-implement
    /// this match against the enum.
    pub fn as_db_str(self) -> &'static str {
        match self {
            SessionMode::Countdown => "countdown",
            SessionMode::Stopwatch => "stopwatch",
            SessionMode::BoxBreath => "box_breath",
        }
    }

    /// Inverse of `as_db_str`. Returns `None` for unknown / typo'd
    /// values; callers decide whether to fall back to Countdown
    /// (the historical pre-feature default) or treat the row as
    /// corrupt.
    pub fn from_db_str(s: &str) -> Option<Self> {
        match s {
            "countdown" => Some(SessionMode::Countdown),
            "stopwatch" => Some(SessionMode::Stopwatch),
            "box_breath" => Some(SessionMode::BoxBreath),
            _ => None,
        }
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
/// - `kind` is the event type (e.g. `"session_insert"`); `payload`
///   is its JSON-encoded specifics. Both opaque at this layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    pub event_uuid: String,
    pub lamport_ts: i64,
    pub device_id: String,
    pub kind: String,
    pub payload: String,
}

/// Pagination + filter for `query_sessions`. Default-constructed value
/// matches every session with no pagination.
#[derive(Debug, Clone, Default)]
pub struct SessionFilter {
    /// Only sessions referencing this label id. `None` ⇒ every label
    /// (and unlabeled).
    pub label_id: Option<i64>,
    /// Only sessions with a non-empty `notes` field.
    pub only_with_notes: bool,
    /// Hard cap on returned rows. `None` ⇒ no cap.
    pub limit: Option<u32>,
    /// Skip the first `offset` rows of the (filtered, ordered) result.
    /// `None` ⇒ no skip.
    pub offset: Option<u32>,
}

pub struct Database {
    conn: Connection,
}

const SCHEMA: &str = "
    CREATE TABLE IF NOT EXISTS labels (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        name TEXT NOT NULL COLLATE NOCASE UNIQUE,
        uuid TEXT NOT NULL UNIQUE
    );
    CREATE TABLE IF NOT EXISTS sessions (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        start_iso TEXT NOT NULL,
        duration_secs INTEGER NOT NULL,
        label_id INTEGER REFERENCES labels(id) ON DELETE SET NULL,
        notes TEXT,
        mode TEXT NOT NULL CHECK (mode IN ('countdown', 'stopwatch', 'box_breath')),
        uuid TEXT NOT NULL UNIQUE
    );
    CREATE TABLE IF NOT EXISTS settings (
        key   TEXT PRIMARY KEY,
        value TEXT NOT NULL
    );
    -- Single-row per database. Holds the stable per-device UUID that
    -- tags every locally-authored event in the sync log, plus the
    -- monotonic Lamport counter used to order events across devices.
    -- `lamport_clock` defaults to 0 and is bumped on local writes /
    -- max-merged on remote observations.
    CREATE TABLE IF NOT EXISTS device (
        device_id     TEXT PRIMARY KEY,
        lamport_clock INTEGER NOT NULL DEFAULT 0
    );
    -- Append-only event log for Nextcloud sync. Every row is a
    -- self-contained description of a state-changing operation. Reads
    -- (replay, push) sort by `lamport_ts` for causal ordering;
    -- `event_uuid` UNIQUE makes append idempotent against retries and
    -- peer-forwarded duplicates. `synced` is the push-queue gate.
    CREATE TABLE IF NOT EXISTS events (
        id          INTEGER PRIMARY KEY AUTOINCREMENT,
        event_uuid  TEXT NOT NULL UNIQUE,
        lamport_ts  INTEGER NOT NULL,
        device_id   TEXT NOT NULL,
        kind        TEXT NOT NULL,
        payload     TEXT NOT NULL,
        synced      INTEGER NOT NULL DEFAULT 0
    );
    -- Index on (lamport_ts, device_id) supports the canonical
    -- replay-order scan; SQLite tie-breaks on device_id so the order is
    -- deterministic across peers.
    CREATE INDEX IF NOT EXISTS events_lamport_idx
        ON events(lamport_ts, device_id);
    -- Index on `synced` makes `pending_events` (which scans WHERE
    -- synced = 0) efficient even when the log grows large.
    CREATE INDEX IF NOT EXISTS events_synced_idx
        ON events(synced);
    -- Sync-loop bookkeeping: server URL, last-pull cursor, last
    -- successful sync timestamp, etc. Separate namespace from `settings`
    -- so user-facing prefs and sync internals don't share a key space.
    -- Sensitive values (app password) belong in libsecret/Keystore, not
    -- here.
    CREATE TABLE IF NOT EXISTS sync_state (
        key   TEXT PRIMARY KEY,
        value TEXT NOT NULL
    );
";

impl Database {
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        Self::init(conn)
    }

    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        Self::init(conn)
    }

    fn init(conn: Connection) -> Result<Self> {
        // Explicit PRAGMAs — even when rusqlite enables them by default,
        // the intent is part of the source so it can't be silently
        // dropped by a dependency upgrade. The FK clause on
        // sessions.label_id only fires when this is ON.
        conn.execute_batch("PRAGMA foreign_keys=ON;")?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn })
    }

    /// Read the value of a settings key. Returns `default` (without
    /// inserting it) when the key has never been set.
    pub fn get_setting(&self, key: &str, default: &str) -> Result<String> {
        match self.conn.query_row(
            "SELECT value FROM settings WHERE key = ?1",
            params![key],
            |row| row.get::<_, String>(0),
        ) {
            Ok(val) => Ok(val),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(default.to_string()),
            Err(e) => Err(DbError::Sqlite(e)),
        }
    }

    /// Write a settings value. Upserts: subsequent calls overwrite.
    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    /// This database's stable device UUID. Generated lazily on first call
    /// after a fresh DB and persisted in the single-row `device` table —
    /// every subsequent call (including after the process restarts and
    /// reopens the file) returns the same value. The id tags every
    /// locally-authored event so devices can attribute writes during
    /// merge.
    pub fn device_id(&self) -> Result<String> {
        if let Some(existing) = self
            .conn
            .query_row("SELECT device_id FROM device LIMIT 1", [], |row| {
                row.get::<_, String>(0)
            })
            .optional()?
        {
            return Ok(existing);
        }
        // First call on a fresh DB — mint a new id and remember it.
        let new_id = uuid::Uuid::new_v4().to_string();
        self.conn.execute(
            "INSERT INTO device (device_id) VALUES (?1)",
            params![new_id],
        )?;
        Ok(new_id)
    }

    /// Current Lamport clock value (0 on a fresh DB). Returns even before
    /// `device_id()` has been called: an empty `device` table reads back
    /// the column default rather than failing.
    pub fn lamport_clock(&self) -> Result<i64> {
        let v: Option<i64> = self
            .conn
            .query_row("SELECT lamport_clock FROM device LIMIT 1", [], |row| {
                row.get::<_, i64>(0)
            })
            .optional()?;
        Ok(v.unwrap_or(0))
    }

    /// Increment the Lamport clock by 1; return the new value (so the
    /// caller can stamp the event they're about to author with it). On
    /// a fresh DB this also seeds the single `device` row.
    pub fn bump_lamport_clock(&self) -> Result<i64> {
        // Make sure a row exists — sharing the existing seed path with
        // `device_id` keeps the device_id and lamport_clock in the same
        // single row, as the schema requires (device_id is PRIMARY KEY).
        let _ = self.device_id()?;
        self.conn.execute(
            "UPDATE device SET lamport_clock = lamport_clock + 1",
            [],
        )?;
        self.lamport_clock()
    }

    /// Apply the Lamport observation rule: set local = max(local, remote)
    /// + 1. Returns the new local value. Always strictly increases the
    /// clock, so any event authored after observation sorts after the
    /// remote one we just witnessed.
    pub fn observe_remote_lamport(&self, remote_ts: i64) -> Result<i64> {
        let _ = self.device_id()?;
        // Single statement, no read-modify-write race: SQL computes
        // max(stored, ?) + 1 inline.
        self.conn.execute(
            "UPDATE device SET lamport_clock = MAX(lamport_clock, ?1) + 1",
            params![remote_ts],
        )?;
        self.lamport_clock()
    }

    /// Append an event to the sync log. Returns the local rowid (the
    /// cache key inside this device — distinct from `event.event_uuid`,
    /// the cross-device identity). A second append with an
    /// `event_uuid` already present is a silent no-op; this makes
    /// delivery at-most-once on the local cache regardless of retries
    /// or peer forwarding.
    pub fn append_event(&self, event: &Event) -> Result<i64> {
        // INSERT OR IGNORE handles the dedup case without raising the
        // UNIQUE-constraint error to the caller. On ignore, last_insert_rowid
        // is unchanged (could be 0 on a brand-new connection, or stale
        // from a prior insert) — query the row by event_uuid to get the
        // authoritative local id.
        self.conn.execute(
            "INSERT OR IGNORE INTO events
                (event_uuid, lamport_ts, device_id, kind, payload)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                event.event_uuid,
                event.lamport_ts,
                event.device_id,
                event.kind,
                event.payload,
            ],
        )?;
        Ok(self.conn.query_row(
            "SELECT id FROM events WHERE event_uuid = ?1",
            params![event.event_uuid],
            |row| row.get::<_, i64>(0),
        )?)
    }

    /// All events not yet pushed to remote, ordered by `lamport_ts` ASC
    /// (then by local `id` as a stable tie-break). Sync's push phase
    /// drains this list in order; mark each entry with `mark_event_synced`
    /// once the WebDAV PUT succeeds.
    pub fn pending_events(&self) -> Result<Vec<(i64, Event)>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, event_uuid, lamport_ts, device_id, kind, payload
             FROM events
             WHERE synced = 0
             ORDER BY lamport_ts ASC, id ASC",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    Event {
                        event_uuid: row.get(1)?,
                        lamport_ts: row.get(2)?,
                        device_id: row.get(3)?,
                        kind: row.get(4)?,
                        payload: row.get(5)?,
                    },
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Flip the `synced` flag on the event with this local rowid so it
    /// drops out of `pending_events`. Unknown ids are silently no-ops —
    /// SQLite's UPDATE-on-no-match behaviour, exposed verbatim so a
    /// stale id from a partial sync doesn't escalate to an error.
    pub fn mark_event_synced(&self, id: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE events SET synced = 1 WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    }

    /// Read a sync-state value (server URL, last-pull cursor, …),
    /// returning `default` if the key has never been set. Mirrors
    /// `get_setting` but keyed against the `sync_state` namespace.
    pub fn get_sync_state(&self, key: &str, default: &str) -> Result<String> {
        match self.conn.query_row(
            "SELECT value FROM sync_state WHERE key = ?1",
            params![key],
            |row| row.get::<_, String>(0),
        ) {
            Ok(val) => Ok(val),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(default.to_string()),
            Err(e) => Err(DbError::Sqlite(e)),
        }
    }

    /// Upsert a sync-state value. Subsequent calls overwrite. Mirrors
    /// `set_setting`'s semantics in the `sync_state` namespace.
    pub fn set_sync_state(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO sync_state (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    /// True iff some label OTHER THAN `except_id` already uses `name`
    /// (case-insensitive — the column is COLLATE NOCASE). UI-side
    /// pre-validation for renames: pass the row's own id as
    /// `except_id` so renaming-to-self isn't reported as a collision.
    /// Pass any non-existent id (e.g. 0) when validating a brand-new
    /// label.
    pub fn is_label_name_taken(&self, name: &str, except_id: i64) -> Result<bool> {
        Ok(self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM labels WHERE name = ?1 AND id != ?2)",
            params![name, except_id],
            |row| row.get(0),
        )?)
    }

    /// How many sessions reference the label with `id`. Returns 0 for
    /// unreferenced or non-existent labels (no error). Used by the UI's
    /// "delete N sessions?" confirmation before unlabel-on-delete.
    pub fn label_session_count(&self, id: i64) -> Result<i64> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM sessions WHERE label_id = ?1",
            params![id],
            |row| row.get(0),
        )?)
    }

    /// Remove the label with `id`. Sessions that referenced it survive
    /// with `label_id = None` (FK is `ON DELETE SET NULL`). Unknown ids
    /// are silently no-ops.
    pub fn delete_label(&self, id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM labels WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// Rename the label with `id` to `name`. Unknown ids are silently
    /// no-ops (SQLite UPDATE matches zero rows). If `name` collides
    /// case-insensitively with another label, returns
    /// `DbError::DuplicateLabel`. Renaming a row to its own current name
    /// (incl. a case variant of itself) succeeds, since SQLite's UNIQUE
    /// check excludes the row being updated.
    pub fn update_label(&self, id: i64, name: &str) -> Result<()> {
        match self.conn.execute(
            "UPDATE labels SET name = ?1 WHERE id = ?2",
            params![name, id],
        ) {
            Ok(_) => Ok(()),
            Err(rusqlite::Error::SqliteFailure(err, _))
                if err.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE =>
            {
                Err(DbError::DuplicateLabel(name.to_string()))
            }
            Err(e) => Err(DbError::Sqlite(e)),
        }
    }

    /// Insert a new label and return its AUTOINCREMENT rowid. Returns
    /// `DbError::DuplicateLabel` if `name` (case-insensitive) already
    /// exists — the column is `COLLATE NOCASE UNIQUE`. UIs that want to
    /// silently reuse an existing row (e.g. CSV import) should call
    /// `find_or_create_label` instead.
    pub fn insert_label(&self, name: &str) -> Result<i64> {
        let uuid = uuid::Uuid::new_v4().to_string();
        match self
            .conn
            .execute("INSERT INTO labels (name, uuid) VALUES (?1, ?2)", params![name, uuid])
        {
            Ok(_) => Ok(self.conn.last_insert_rowid()),
            Err(rusqlite::Error::SqliteFailure(err, _))
                if err.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE =>
            {
                Err(DbError::DuplicateLabel(name.to_string()))
            }
            Err(e) => Err(DbError::Sqlite(e)),
        }
    }

    pub fn count_labels(&self) -> Result<i64> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM labels", [], |row| row.get(0))?)
    }

    /// Every label as a `Label { id, name, uuid }`, alphabetic by name
    /// with the column's NOCASE collation so 'apple', 'Banana', 'cherry'
    /// come back in dictionary order regardless of casing.
    pub fn list_labels(&self) -> Result<Vec<Label>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, uuid FROM labels ORDER BY name",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok(Label { id: row.get(0)?, name: row.get(1)?, uuid: row.get(2)? })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Return a label id by name, creating the label if missing. Lookup
    /// is case-insensitive (column COLLATE NOCASE), so an import of
    /// "Meditation" finds an existing "meditation" instead of producing
    /// a duplicate row.
    pub fn find_or_create_label(&self, name: &str) -> Result<i64> {
        if let Some(id) = self.find_label_by_name(name)? {
            return Ok(id);
        }
        self.insert_label(name)
    }

    pub fn find_label_by_name(&self, name: &str) -> Result<Option<i64>> {
        let id = self
            .conn
            .query_row(
                "SELECT id FROM labels WHERE name = ?1",
                [name],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;
        Ok(id)
    }

    pub fn count_sessions(&self) -> Result<i64> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))?)
    }

    pub fn insert_session(&self, session: &Session) -> Result<i64> {
        let uuid = uuid::Uuid::new_v4().to_string();
        self.conn.execute(
            "INSERT INTO sessions (start_iso, duration_secs, label_id, notes, mode, uuid)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                session.start_iso,
                session.duration_secs,
                session.label_id,
                session.notes,
                session.mode.as_db_str(),
                uuid,
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Insert many sessions inside a single transaction — orders of
    /// magnitude faster than calling `insert_session` in a loop. Atomic:
    /// if any row fails a constraint, the whole batch is rolled back and
    /// the caller never sees a partially-imported DB.
    pub fn bulk_insert_sessions(&self, sessions: &[Session]) -> Result<usize> {
        let tx = self.conn.unchecked_transaction()?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO sessions (start_iso, duration_secs, label_id, notes, mode, uuid)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?;
            for s in sessions {
                let uuid = uuid::Uuid::new_v4().to_string();
                stmt.execute(params![
                    s.start_iso,
                    s.duration_secs,
                    s.label_id,
                    s.notes,
                    s.mode.as_db_str(),
                    uuid,
                ])?;
            }
        }
        tx.commit()?;
        Ok(sessions.len())
    }

    /// Remove the row with `id`. Unknown ids are silently no-ops.
    pub fn delete_session(&self, id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM sessions WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// Remove every session row. Returns how many rows were deleted.
    /// Labels and settings are untouched.
    pub fn delete_all_sessions(&self) -> Result<usize> {
        let n = self.conn.execute("DELETE FROM sessions", [])?;
        Ok(n)
    }

    /// Replace every field of the row with `id`. Unknown ids are silently
    /// no-ops (SQLite UPDATE matches zero rows).
    pub fn update_session(&self, id: i64, session: &Session) -> Result<()> {
        self.conn.execute(
            "UPDATE sessions
             SET start_iso = ?1, duration_secs = ?2, label_id = ?3,
                 notes = ?4, mode = ?5
             WHERE id = ?6",
            params![
                session.start_iso,
                session.duration_secs,
                session.label_id,
                session.notes,
                session.mode.as_db_str(),
                id,
            ],
        )?;
        Ok(())
    }

    pub fn get_best_streak(&self) -> Result<i64> {
        self.best_streak_filtered(None)
    }

    pub fn get_best_streak_for_label(&self, label_id: i64) -> Result<i64> {
        self.best_streak_filtered(Some(label_id))
    }

    fn best_streak_filtered(&self, label_filter: Option<i64>) -> Result<i64> {
        let days = self.distinct_session_days_ascending(label_filter)?;
        if days.is_empty() {
            return Ok(0);
        }
        let mut best = 1i64;
        let mut current = 1i64;
        for window in days.windows(2) {
            if window[1] == window[0].succ_opt().expect("date overflow") {
                current += 1;
                best = best.max(current);
            } else {
                current = 1;
            }
        }
        Ok(best)
    }

    pub fn import_sessions_csv<R: Read>(&self, reader: R) -> Result<usize> {
        let mut rdr = csv::Reader::from_reader(reader);
        let mut count = 0;
        for record in rdr.records() {
            let record = record.map_err(|e| DbError::Csv(e.to_string()))?;
            let start_iso = record
                .get(0)
                .ok_or_else(|| DbError::Csv("missing start_iso".to_string()))?
                .to_string();
            let duration_secs: u32 = record
                .get(1)
                .unwrap_or("")
                .parse()
                .map_err(|_| DbError::Csv("bad duration_secs".to_string()))?;
            let label = record
                .get(2)
                .map(str::to_string)
                .filter(|s| !s.is_empty());
            let notes = record
                .get(3)
                .map(str::to_string)
                .filter(|s| !s.is_empty());
            let mode_str = record.get(4).unwrap_or("countdown");
            let mode = SessionMode::from_db_str(mode_str)
                .ok_or_else(|| DbError::Csv(format!("unknown mode: {mode_str}")))?;

            let label_id = match label {
                Some(name) => Some(self.find_or_create_label(&name)?),
                None => None,
            };

            self.insert_session(&Session {
                start_iso,
                duration_secs,
                label_id,
                notes,
                mode,
                uuid: String::new(),
            })?;
            count += 1;
        }
        Ok(count)
    }

    pub fn export_sessions_csv<W: Write>(&self, writer: W) -> Result<()> {
        let mut wtr = csv::Writer::from_writer(writer);
        wtr.write_record(["start_iso", "duration_secs", "label", "notes", "mode"])
            .map_err(|e| DbError::Csv(e.to_string()))?;

        let mut stmt = self.conn.prepare(
            "SELECT s.start_iso, s.duration_secs, l.name, s.notes, s.mode
             FROM sessions s
             LEFT JOIN labels l ON s.label_id = l.id
             ORDER BY s.id",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, u32>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, String>(4)?,
            ))
        })?;
        for row in rows {
            let (start, dur, label, notes, mode) = row?;
            wtr.write_record([
                &start,
                &dur.to_string(),
                label.as_deref().unwrap_or(""),
                notes.as_deref().unwrap_or(""),
                &mode,
            ])
            .map_err(|e| DbError::Csv(e.to_string()))?;
        }
        wtr.flush().map_err(|e| DbError::Csv(e.to_string()))?;
        Ok(())
    }

    pub fn get_median_duration_secs(&self) -> Result<u32> {
        let mut stmt = self
            .conn
            .prepare("SELECT duration_secs FROM sessions ORDER BY duration_secs")?;
        let durations: Vec<u32> = stmt
            .query_map([], |row| row.get(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        if durations.is_empty() {
            return Ok(0);
        }
        // Lower-median: index (len-1)/2 hits the lower middle on even counts,
        // and the exact middle on odd counts.
        Ok(durations[(durations.len() - 1) / 2])
    }

    pub fn get_running_average_secs(&self, today: chrono::NaiveDate, days: i64) -> Result<f64> {
        if days <= 0 {
            return Ok(0.0);
        }
        let cutoff = today - chrono::Duration::days(days - 1);
        let cutoff_str = cutoff.format("%Y-%m-%d").to_string();
        let total: i64 = self.conn.query_row(
            "SELECT COALESCE(SUM(duration_secs), 0) FROM sessions
             WHERE SUBSTR(start_iso, 1, 10) >= ?1",
            [cutoff_str],
            |row| row.get(0),
        )?;
        Ok(total as f64 / days as f64)
    }

    pub fn get_daily_totals(&self) -> Result<Vec<(chrono::NaiveDate, i64)>> {
        self.daily_totals_filtered(None)
    }

    pub fn get_daily_totals_for_label(
        &self,
        label_id: i64,
    ) -> Result<Vec<(chrono::NaiveDate, i64)>> {
        self.daily_totals_filtered(Some(label_id))
    }

    fn daily_totals_filtered(
        &self,
        label_filter: Option<i64>,
    ) -> Result<Vec<(chrono::NaiveDate, i64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT SUBSTR(start_iso, 1, 10) AS day, SUM(duration_secs)
             FROM sessions
             WHERE ?1 IS NULL OR label_id = ?1
             GROUP BY day
             ORDER BY day",
        )?;
        let totals = stmt
            .query_map(params![label_filter], |row| {
                let day_str: String = row.get(0)?;
                let total_secs: i64 = row.get(1)?;
                Ok((day_str, total_secs))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?
            .into_iter()
            .filter_map(|(s, secs)| {
                chrono::NaiveDate::parse_from_str(&s, "%Y-%m-%d")
                    .ok()
                    .map(|d| (d, secs))
            })
            .collect();
        Ok(totals)
    }

    fn distinct_session_days_ascending(
        &self,
        label_filter: Option<i64>,
    ) -> Result<Vec<chrono::NaiveDate>> {
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT SUBSTR(start_iso, 1, 10) FROM sessions
             WHERE ?1 IS NULL OR label_id = ?1
             ORDER BY 1",
        )?;
        let days = stmt
            .query_map(params![label_filter], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?
            .into_iter()
            .filter_map(|s| chrono::NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok())
            .collect();
        Ok(days)
    }

    pub fn get_streak(&self, today: chrono::NaiveDate) -> Result<i64> {
        self.streak_filtered(today, None)
    }

    pub fn get_streak_for_label(&self, today: chrono::NaiveDate, label_id: i64) -> Result<i64> {
        self.streak_filtered(today, Some(label_id))
    }

    fn streak_filtered(
        &self,
        today: chrono::NaiveDate,
        label_filter: Option<i64>,
    ) -> Result<i64> {
        let days = self.distinct_session_days_ascending(label_filter)?;
        let Some(&most_recent) = days.last() else {
            return Ok(0);
        };
        let yesterday = today.pred_opt().expect("date underflow");
        let mut expected = if most_recent == today {
            today
        } else if most_recent == yesterday {
            yesterday
        } else {
            return Ok(0);
        };

        let mut count = 0;
        for day in days.iter().rev() {
            if *day == expected {
                count += 1;
                expected = expected.pred_opt().expect("date underflow");
            } else {
                break;
            }
        }
        Ok(count)
    }

    /// The longest single session — `(id, Session)`, or None on empty DB.
    /// Tie-break is unspecified (whichever SQLite returns first); callers
    /// should not depend on the order of equal-duration rows.
    pub fn get_longest_session(&self) -> Result<Option<(i64, Session)>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, start_iso, duration_secs, label_id, notes, mode, uuid
             FROM sessions
             ORDER BY duration_secs DESC LIMIT 1",
        )?;
        let mut rows = stmt.query([])?;
        match rows.next()? {
            None => Ok(None),
            Some(row) => {
                let mode_str: String = row.get(5)?;
                let mode = SessionMode::from_db_str(&mode_str)
                    .expect("DB CHECK constraint should restrict mode");
                Ok(Some((
                    row.get::<_, i64>(0)?,
                    Session {
                        start_iso: row.get(1)?,
                        duration_secs: row.get(2)?,
                        label_id: row.get(3)?,
                        notes: row.get(4)?,
                        mode,
                        uuid: row.get(6)?,
                    },
                )))
            }
        }
    }

    /// Counts of sessions bucketed by start hour: morning < 12 (hours
    /// 0-11), afternoon 12-17, evening ≥ 18 (18-23). Returns
    /// `(morning, afternoon, evening)`. Every session lands in exactly
    /// one bucket.
    pub fn hour_buckets(&self) -> Result<(i64, i64, i64)> {
        // Hour is at chars 12-13 of start_iso (0-indexed in SQL it's 12).
        // Cast to integer once and bucket in a single pass.
        let mut stmt = self.conn.prepare_cached(
            "SELECT
               COALESCE(SUM(CASE WHEN h <  12 THEN 1 ELSE 0 END), 0),
               COALESCE(SUM(CASE WHEN h >= 12 AND h < 18 THEN 1 ELSE 0 END), 0),
               COALESCE(SUM(CASE WHEN h >= 18 THEN 1 ELSE 0 END), 0)
             FROM (
               SELECT CAST(SUBSTR(start_iso, 12, 2) AS INTEGER) AS h
               FROM sessions
             )",
        )?;
        Ok(stmt.query_row([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?)
    }

    /// Distinct (year, month) pairs that have at least one session,
    /// ordered most-recent first. Used by the calendar-picker dropdown.
    pub fn active_months(&self) -> Result<Vec<(i32, u32)>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT DISTINCT
                 CAST(SUBSTR(start_iso, 1, 4) AS INTEGER),
                 CAST(SUBSTR(start_iso, 6, 2) AS INTEGER)
             FROM sessions
             ORDER BY 1 DESC, 2 DESC",
        )?;
        let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Day-of-month numbers in `(year, month)` that have at least one
    /// session, ascending. Caller maps these directly to calendar cells.
    /// December rolls cleanly to next-year January for the upper bound.
    pub fn active_days_in_month(&self, year: i32, month: u32) -> Result<Vec<u32>> {
        let start = format!("{year:04}-{month:02}-01");
        let (next_year, next_month) =
            if month == 12 { (year + 1, 1) } else { (year, month + 1) };
        let end = format!("{next_year:04}-{next_month:02}-01");
        let mut stmt = self.conn.prepare_cached(
            "SELECT DISTINCT CAST(SUBSTR(start_iso, 9, 2) AS INTEGER)
             FROM sessions
             WHERE start_iso >= ?1 AND start_iso < ?2
             ORDER BY 1",
        )?;
        let rows = stmt.query_map(params![start, end], |row| row.get::<_, u32>(0))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Sum of `duration_secs` for sessions inside a calendar month
    /// (`year`, `month` 1-12). Boundaries are at local midnight on the
    /// first and last day of the month. December rolls cleanly into
    /// January of the next year.
    pub fn month_total_secs(&self, year: i32, month: u32) -> Result<i64> {
        let start = format!("{year:04}-{month:02}-01");
        let (next_year, next_month) =
            if month == 12 { (year + 1, 1) } else { (year, month + 1) };
        let end = format!("{next_year:04}-{next_month:02}-01");
        Ok(self.conn.query_row(
            "SELECT COALESCE(SUM(duration_secs), 0)
             FROM sessions
             WHERE start_iso >= ?1 AND start_iso < ?2",
            params![start, end],
            |row| row.get(0),
        )?)
    }

    /// Sum of `duration_secs` for sessions whose `start_iso` is on or
    /// after the start of `since` (interpreted as the user's local
    /// midnight). Returns 0 if no sessions match.
    ///
    /// Lexicographic comparison on ISO 8601 strings works because the
    /// format sorts chronologically as ASCII text. The cut-off is at
    /// the START of the date — a session at 00:00:00 on `since` is
    /// included.
    pub fn total_secs_since(&self, since: chrono::NaiveDate) -> Result<i64> {
        let prefix = since.format("%Y-%m-%d").to_string();
        Ok(self.conn.query_row(
            "SELECT COALESCE(SUM(duration_secs), 0)
             FROM sessions
             WHERE start_iso >= ?1",
            params![prefix],
            |row| row.get(0),
        )?)
    }

    /// Total of `duration_secs` across every session (no filter). Returns
    /// 0 on an empty DB. Use this when you want the underlying precision
    /// (e.g. weekly-goal ring, longest-session display); use
    /// `total_minutes` for stats lines that show "X min".
    pub fn total_seconds(&self) -> Result<i64> {
        Ok(self.conn.query_row(
            "SELECT COALESCE(SUM(duration_secs), 0) FROM sessions",
            [],
            |row| row.get(0),
        )?)
    }

    pub fn total_minutes(&self) -> Result<i64> {
        Ok(self.total_seconds()? / 60)
    }

    /// Per-label session count. `None` represents unlabeled sessions.
    pub fn count_sessions_by_label(&self) -> Result<Vec<(Option<String>, i64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT l.name, COUNT(*)
             FROM sessions s
             LEFT JOIN labels l ON s.label_id = l.id
             GROUP BY l.name
             ORDER BY l.name",
        )?;
        let rows = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Per-label `(name, total_secs, session_count)` ordered by total
    /// seconds DESC, ties broken by name NOCASE ASC. Excludes unlabeled
    /// sessions AND labels with zero sessions (INNER JOIN drops both).
    /// Used by the stats panel's per-label breakdown.
    pub fn label_totals_seconds(&self) -> Result<Vec<(String, i64, i64)>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT labels.name,
                    SUM(sessions.duration_secs) AS total,
                    COUNT(sessions.id) AS n
             FROM labels
             INNER JOIN sessions ON sessions.label_id = labels.id
             GROUP BY labels.id, labels.name
             ORDER BY total DESC, labels.name COLLATE NOCASE ASC",
        )?;
        let rows = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Per-label total minutes. `None` represents unlabeled sessions.
    pub fn total_minutes_by_label(&self) -> Result<Vec<(Option<String>, i64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT l.name, SUM(s.duration_secs) / 60
             FROM sessions s
             LEFT JOIN labels l ON s.label_id = l.id
             GROUP BY l.name
             ORDER BY l.name",
        )?;
        let rows = stmt
            .query_map([], |row| {
                let name: Option<String> = row.get(0)?;
                let mins: i64 = row.get(1)?;
                Ok((name, mins))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Rich-filter session query for the log feed: pagination, label
    /// filter, notes-only. Rows are ordered `start_iso DESC` so the
    /// caller's first page is the newest sessions.
    ///
    /// SQLite quirks handled here:
    /// - `LIMIT -1` means "no limit" (used when `filter.limit` is None).
    /// - `OFFSET 0` is the no-skip default.
    /// - The four (notes × label) combinations get distinct static
    ///   queries so each is independently cached by `prepare_cached`.
    pub fn query_sessions(&self, filter: &SessionFilter) -> Result<Vec<(i64, Session)>> {
        let limit_val: i64 = filter.limit.map(|n| n as i64).unwrap_or(-1);
        let offset_val: i64 = filter.offset.map(|n| n as i64).unwrap_or(0);

        let map_row = |row: &rusqlite::Row<'_>| -> rusqlite::Result<(i64, Session)> {
            let mode_str: String = row.get(5)?;
            let mode = SessionMode::from_db_str(&mode_str)
                .expect("DB CHECK constraint should restrict mode to known values");
            Ok((
                row.get::<_, i64>(0)?,
                Session {
                    start_iso: row.get(1)?,
                    duration_secs: row.get(2)?,
                    label_id: row.get(3)?,
                    notes: row.get(4)?,
                    mode,
                    uuid: row.get(6)?,
                },
            ))
        };

        let rows: rusqlite::Result<Vec<(i64, Session)>> = match (filter.only_with_notes, filter.label_id) {
            (false, None) => {
                let mut s = self.conn.prepare_cached(
                    "SELECT id, start_iso, duration_secs, label_id, notes, mode, uuid
                     FROM sessions
                     ORDER BY start_iso DESC
                     LIMIT ?1 OFFSET ?2",
                )?;
                let it = s.query_map(params![limit_val, offset_val], map_row)?;
                it.collect()
            }
            (true, None) => {
                let mut s = self.conn.prepare_cached(
                    "SELECT id, start_iso, duration_secs, label_id, notes, mode, uuid
                     FROM sessions
                     WHERE notes IS NOT NULL AND notes != ''
                     ORDER BY start_iso DESC
                     LIMIT ?1 OFFSET ?2",
                )?;
                let it = s.query_map(params![limit_val, offset_val], map_row)?;
                it.collect()
            }
            (false, Some(lid)) => {
                let mut s = self.conn.prepare_cached(
                    "SELECT id, start_iso, duration_secs, label_id, notes, mode, uuid
                     FROM sessions
                     WHERE label_id = ?1
                     ORDER BY start_iso DESC
                     LIMIT ?2 OFFSET ?3",
                )?;
                let it = s.query_map(params![lid, limit_val, offset_val], map_row)?;
                it.collect()
            }
            (true, Some(lid)) => {
                let mut s = self.conn.prepare_cached(
                    "SELECT id, start_iso, duration_secs, label_id, notes, mode, uuid
                     FROM sessions
                     WHERE label_id = ?1 AND notes IS NOT NULL AND notes != ''
                     ORDER BY start_iso DESC
                     LIMIT ?2 OFFSET ?3",
                )?;
                let it = s.query_map(params![lid, limit_val, offset_val], map_row)?;
                it.collect()
            }
        };
        Ok(rows?)
    }

    pub fn list_sessions(&self) -> Result<Vec<(i64, Session)>> {
        self.list_sessions_filtered(None)
    }

    pub fn list_sessions_for_label(&self, label_id: i64) -> Result<Vec<(i64, Session)>> {
        self.list_sessions_filtered(Some(label_id))
    }

    fn list_sessions_filtered(&self, label_filter: Option<i64>) -> Result<Vec<(i64, Session)>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, start_iso, duration_secs, label_id, notes, mode, uuid FROM sessions
             WHERE ?1 IS NULL OR label_id = ?1
             ORDER BY id",
        )?;
        let sessions = stmt
            .query_map(params![label_filter], |row| {
                let mode_str: String = row.get(5)?;
                let mode = SessionMode::from_db_str(&mode_str).expect(
                    "DB CHECK constraint should restrict mode to known values",
                );
                Ok((
                    row.get::<_, i64>(0)?,
                    Session {
                        start_iso: row.get(1)?,
                        duration_secs: row.get(2)?,
                        label_id: row.get(3)?,
                        notes: row.get(4)?,
                        mode,
                        uuid: row.get(6)?,
                    },
                ))
            })?
            .collect::<rusqlite::Result<Vec<(i64, Session)>>>()?;
        Ok(sessions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── SessionMode serialization ─────────────────────────────────────────────

    #[test]
    fn session_mode_as_db_str_returns_canonical_strings() {
        // These are the values that go into the sessions.mode column AND
        // the CSV mode column — pinning them so a refactor that quietly
        // changes one (e.g. 'box_breath' → 'breath') gets caught.
        assert_eq!(SessionMode::Countdown.as_db_str(), "countdown");
        assert_eq!(SessionMode::Stopwatch.as_db_str(), "stopwatch");
        assert_eq!(SessionMode::BoxBreath.as_db_str(), "box_breath");
    }

    #[test]
    fn session_mode_from_db_str_parses_canonical_strings() {
        assert_eq!(SessionMode::from_db_str("countdown"), Some(SessionMode::Countdown));
        assert_eq!(SessionMode::from_db_str("stopwatch"), Some(SessionMode::Stopwatch));
        assert_eq!(SessionMode::from_db_str("box_breath"), Some(SessionMode::BoxBreath));
    }

    #[test]
    fn session_mode_from_db_str_returns_none_for_unknown() {
        // Caller decides the fallback (e.g. Countdown for tolerant import,
        // hard-error for strict). We don't bake one in.
        assert_eq!(SessionMode::from_db_str(""), None);
        assert_eq!(SessionMode::from_db_str("COUNTDOWN"), None);  // case-sensitive
        assert_eq!(SessionMode::from_db_str("breathing"), None);  // old name
        assert_eq!(SessionMode::from_db_str("box-breath"), None); // dash, not underscore
        assert_eq!(SessionMode::from_db_str("garbage"), None);
    }

    #[test]
    fn session_mode_db_str_round_trip() {
        for &mode in &[SessionMode::Countdown, SessionMode::Stopwatch, SessionMode::BoxBreath] {
            assert_eq!(SessionMode::from_db_str(mode.as_db_str()), Some(mode));
        }
    }

    #[test]
    fn inserting_label_increases_count() {
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        assert_eq!(db.count_labels().unwrap(), 1);
    }

    #[test]
    fn inserting_two_distinct_labels_yields_count_of_two() {
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        db.insert_label("Evening").unwrap();
        assert_eq!(db.count_labels().unwrap(), 2);
    }

    #[test]
    fn inserting_duplicate_label_returns_err() {
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        let second = db.insert_label("Morning");
        assert!(second.is_err(), "second insert of same label should fail");
        // The first insert is preserved; no duplicate row is created.
        assert_eq!(db.count_labels().unwrap(), 1);
    }

    // ── label_totals_seconds (name, secs, count) ─────────────────────────────

    #[test]
    fn label_totals_seconds_is_empty_for_empty_db() {
        let db = Database::open_in_memory().unwrap();
        assert!(db.label_totals_seconds().unwrap().is_empty());
    }

    #[test]
    fn label_totals_seconds_groups_secs_and_counts_per_label() {
        // (name, total_secs, session_count) per label. Unlabeled sessions
        // and labels with zero sessions are excluded — INNER JOIN drops
        // them at the SQL level. Sort: total_secs DESC, name ASC NOCASE.
        let db = Database::open_in_memory().unwrap();
        let morning = db.insert_label("Morning").unwrap();
        let evening = db.insert_label("Evening").unwrap();
        // An extra label with no sessions — must NOT appear in output.
        let _unused = db.insert_label("Unused").unwrap();

        // Morning: 2 sessions, 900s total.
        db.insert_session(&Session {
            start_iso: "2026-04-27T07:00:00".to_string(),
            duration_secs: 600, label_id: Some(morning), notes: None,
            mode: SessionMode::Countdown,
            uuid: String::new(),
        }).unwrap();
        db.insert_session(&Session {
            start_iso: "2026-04-28T07:00:00".to_string(),
            duration_secs: 300, label_id: Some(morning), notes: None,
            mode: SessionMode::Countdown,
            uuid: String::new(),
        }).unwrap();
        // Evening: 1 session, 1200s total — larger total, should sort first.
        db.insert_session(&Session {
            start_iso: "2026-04-27T20:00:00".to_string(),
            duration_secs: 1200, label_id: Some(evening), notes: None,
            mode: SessionMode::Countdown,
            uuid: String::new(),
        }).unwrap();
        // Unlabeled session — must NOT appear.
        db.insert_session(&Session {
            start_iso: "2026-04-27T12:00:00".to_string(),
            duration_secs: 500, label_id: None, notes: None,
            mode: SessionMode::Countdown,
            uuid: String::new(),
        }).unwrap();

        let got = db.label_totals_seconds().unwrap();
        assert_eq!(got.len(), 2,
            "Unused label and unlabeled session must be excluded: {got:?}");
        assert_eq!(got[0], ("Evening".to_string(), 1200, 1));
        assert_eq!(got[1], ("Morning".to_string(), 900, 2));
    }

    #[test]
    fn label_totals_seconds_ties_break_case_insensitive_alphabetic() {
        // Same total ⇒ secondary sort by name, NOCASE.
        let db = Database::open_in_memory().unwrap();
        let zebra = db.insert_label("Zebra").unwrap();
        let alpha = db.insert_label("alpha").unwrap();
        db.insert_session(&Session {
            start_iso: "2026-04-27T12:00:00".to_string(),
            duration_secs: 600, label_id: Some(zebra), notes: None,
            mode: SessionMode::Countdown,
            uuid: String::new(),
        }).unwrap();
        db.insert_session(&Session {
            start_iso: "2026-04-28T12:00:00".to_string(),
            duration_secs: 600, label_id: Some(alpha), notes: None,
            mode: SessionMode::Countdown,
            uuid: String::new(),
        }).unwrap();
        let got = db.label_totals_seconds().unwrap();
        // 'alpha' (lowercase) sorts before 'Zebra' under NOCASE collation.
        assert_eq!(got[0].0, "alpha");
        assert_eq!(got[1].0, "Zebra");
    }

    #[test]
    fn label_totals_seconds_preserves_full_seconds_precision() {
        // total_minutes_by_label returns minutes (lossy integer division).
        // This variant must NOT lose sub-minute precision.
        let db = Database::open_in_memory().unwrap();
        let lid = db.insert_label("Morning").unwrap();
        // 90s + 45s = 135s — would round to 2 minutes (=120s) under
        // the minutes-then-converted approach.
        db.insert_session(&Session {
            start_iso: "2026-04-27T10:00:00".to_string(),
            duration_secs: 90, label_id: Some(lid), notes: None,
            mode: SessionMode::Countdown,
            uuid: String::new(),
        }).unwrap();
        db.insert_session(&Session {
            start_iso: "2026-04-28T10:00:00".to_string(),
            duration_secs: 45, label_id: Some(lid), notes: None,
            mode: SessionMode::Countdown,
            uuid: String::new(),
        }).unwrap();
        let got = db.label_totals_seconds().unwrap();
        assert_eq!(got[0], ("Morning".to_string(), 135, 2));
    }

    // ── hour_buckets ─────────────────────────────────────────────────────────

    #[test]
    fn hour_buckets_is_zero_zero_zero_for_empty_db() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(db.hour_buckets().unwrap(), (0, 0, 0));
    }

    #[test]
    fn hour_buckets_assigns_each_session_to_exactly_one_bucket() {
        // Boundaries: morning < 12 (00:00–11:59), afternoon 12–17,
        // evening ≥ 18 (18:00–23:59). Pin every boundary explicitly.
        let db = Database::open_in_memory().unwrap();
        let make = |hh: u32, mm: u32| Session {
            start_iso: format!("2026-04-27T{hh:02}:{mm:02}:00"),
            duration_secs: 600, label_id: None, notes: None,
            mode: SessionMode::Countdown,
            uuid: String::new(),
        };
        // Morning (5 sessions, hours 0, 6, 11:00, 11:59).
        db.insert_session(&make(0, 0)).unwrap();
        db.insert_session(&make(6, 30)).unwrap();
        db.insert_session(&make(11, 0)).unwrap();
        db.insert_session(&make(11, 59)).unwrap();
        db.insert_session(&make(8, 15)).unwrap();
        // Afternoon (3 sessions, hours 12:00, 15:30, 17:59).
        db.insert_session(&make(12, 0)).unwrap();  // boundary into afternoon
        db.insert_session(&make(15, 30)).unwrap();
        db.insert_session(&make(17, 59)).unwrap(); // last minute of afternoon
        // Evening (2 sessions, hours 18:00, 23:59).
        db.insert_session(&make(18, 0)).unwrap();  // boundary into evening
        db.insert_session(&make(23, 59)).unwrap();

        let (morning, afternoon, evening) = db.hour_buckets().unwrap();
        assert_eq!(morning, 5, "five sessions in 00:00–11:59");
        assert_eq!(afternoon, 3, "three sessions in 12:00–17:59");
        assert_eq!(evening, 2, "two sessions in 18:00–23:59");
    }

    #[test]
    fn hour_buckets_total_equals_session_count() {
        // Defensive: every session lands in exactly one bucket, no
        // sessions are dropped or double-counted.
        let db = Database::open_in_memory().unwrap();
        let hours = [3u32, 7, 11, 12, 13, 17, 18, 22];
        for &h in &hours {
            db.insert_session(&Session {
                start_iso: format!("2026-04-27T{h:02}:00:00"),
                duration_secs: 600, label_id: None, notes: None,
                mode: SessionMode::Countdown,
                uuid: String::new(),
            }).unwrap();
        }
        let (m, a, e) = db.hour_buckets().unwrap();
        assert_eq!(m + a + e, hours.len() as i64);
        assert_eq!(m + a + e, db.count_sessions().unwrap());
    }

    // ── active_months ────────────────────────────────────────────────────────

    #[test]
    fn active_months_is_empty_for_empty_db() {
        let db = Database::open_in_memory().unwrap();
        assert!(db.active_months().unwrap().is_empty());
    }

    #[test]
    fn active_months_returns_distinct_year_month_pairs_descending() {
        // Each session contributes its (year, month) — duplicates within
        // the same month collapse to one entry. Order is most-recent first
        // (the calendar picker shows latest months at the top).
        let db = Database::open_in_memory().unwrap();
        // Three sessions in 2026-04, two in 2026-03, one in 2025-12.
        for d in 1..=3 {
            db.insert_session(&Session {
                start_iso: format!("2026-04-{d:02}T10:00:00"),
                duration_secs: 600, label_id: None, notes: None,
                mode: SessionMode::Countdown,
                uuid: String::new(),
            }).unwrap();
        }
        for d in 5..=6 {
            db.insert_session(&Session {
                start_iso: format!("2026-03-{d:02}T10:00:00"),
                duration_secs: 600, label_id: None, notes: None,
                mode: SessionMode::Countdown,
                uuid: String::new(),
            }).unwrap();
        }
        db.insert_session(&Session {
            start_iso: "2025-12-25T10:00:00".to_string(),
            duration_secs: 600, label_id: None, notes: None,
            mode: SessionMode::Countdown,
            uuid: String::new(),
        }).unwrap();

        let got = db.active_months().unwrap();
        // Three distinct months, newest first.
        assert_eq!(got, vec![(2026, 4), (2026, 3), (2025, 12)]);
    }

    #[test]
    fn active_months_orders_correctly_across_year_boundary() {
        // 2025-12 must sort BEFORE 2026-01 in newest-first ordering.
        let db = Database::open_in_memory().unwrap();
        db.insert_session(&Session {
            start_iso: "2026-01-15T10:00:00".to_string(),
            duration_secs: 600, label_id: None, notes: None,
            mode: SessionMode::Countdown,
            uuid: String::new(),
        }).unwrap();
        db.insert_session(&Session {
            start_iso: "2025-12-15T10:00:00".to_string(),
            duration_secs: 600, label_id: None, notes: None,
            mode: SessionMode::Countdown,
            uuid: String::new(),
        }).unwrap();
        let got = db.active_months().unwrap();
        assert_eq!(got, vec![(2026, 1), (2025, 12)]);
    }

    // ── active_days_in_month ─────────────────────────────────────────────────

    #[test]
    fn active_days_in_month_is_empty_for_silent_month() {
        let db = Database::open_in_memory().unwrap();
        assert!(db.active_days_in_month(2026, 4).unwrap().is_empty());
    }

    #[test]
    fn active_days_in_month_returns_distinct_days_ascending() {
        // Each day with at least one session contributes once. Multiple
        // sessions on the same day collapse to one entry. Returned in
        // ascending order (1, 2, 3, …) so callers can directly map to
        // calendar cells.
        let db = Database::open_in_memory().unwrap();
        // Two sessions on day 5, one on day 12, one on day 28.
        for hr in 9..=10 {
            db.insert_session(&Session {
                start_iso: format!("2026-04-05T{hr:02}:00:00"),
                duration_secs: 600, label_id: None, notes: None,
                mode: SessionMode::Countdown,
                uuid: String::new(),
            }).unwrap();
        }
        db.insert_session(&Session {
            start_iso: "2026-04-12T10:00:00".to_string(),
            duration_secs: 600, label_id: None, notes: None,
            mode: SessionMode::Countdown,
            uuid: String::new(),
        }).unwrap();
        db.insert_session(&Session {
            start_iso: "2026-04-28T10:00:00".to_string(),
            duration_secs: 600, label_id: None, notes: None,
            mode: SessionMode::Countdown,
            uuid: String::new(),
        }).unwrap();
        // A session in March — must NOT appear in April's days.
        db.insert_session(&Session {
            start_iso: "2026-03-15T10:00:00".to_string(),
            duration_secs: 600, label_id: None, notes: None,
            mode: SessionMode::Countdown,
            uuid: String::new(),
        }).unwrap();

        let got = db.active_days_in_month(2026, 4).unwrap();
        assert_eq!(got, vec![5u32, 12, 28]);
    }

    #[test]
    fn active_days_in_month_handles_december() {
        // The 'next month' boundary in code must roll to next-year-Jan
        // for December queries.
        let db = Database::open_in_memory().unwrap();
        db.insert_session(&Session {
            start_iso: "2026-12-31T23:00:00".to_string(),
            duration_secs: 600, label_id: None, notes: None,
            mode: SessionMode::Countdown,
            uuid: String::new(),
        }).unwrap();
        // Jan 1 next year — must NOT contribute.
        db.insert_session(&Session {
            start_iso: "2027-01-01T00:30:00".to_string(),
            duration_secs: 600, label_id: None, notes: None,
            mode: SessionMode::Countdown,
            uuid: String::new(),
        }).unwrap();
        let got = db.active_days_in_month(2026, 12).unwrap();
        assert_eq!(got, vec![31u32]);
    }

    // ── month_total_secs ─────────────────────────────────────────────────────

    #[test]
    fn month_total_secs_is_zero_for_empty_month() {
        let db = Database::open_in_memory().unwrap();
        // Far past — guaranteed empty.
        assert_eq!(db.month_total_secs(1999, 1).unwrap(), 0);
        // Mid-future — also empty.
        assert_eq!(db.month_total_secs(2099, 12).unwrap(), 0);
    }

    #[test]
    fn month_total_secs_sums_only_target_month() {
        // Adjacent-month boundary edges: last second of March and first
        // second of May must NOT count toward April.
        let db = Database::open_in_memory().unwrap();
        // March 31, very late.
        db.insert_session(&Session {
            start_iso: "2026-03-31T23:59:59".to_string(),
            duration_secs: 9999, label_id: None, notes: None,
            mode: SessionMode::Countdown,
            uuid: String::new(),
        }).unwrap();
        // April 1, midnight — INCLUDED in April.
        db.insert_session(&Session {
            start_iso: "2026-04-01T00:00:00".to_string(),
            duration_secs: 600, label_id: None, notes: None,
            mode: SessionMode::Countdown,
            uuid: String::new(),
        }).unwrap();
        // April 30, late evening — INCLUDED.
        db.insert_session(&Session {
            start_iso: "2026-04-30T23:59:59".to_string(),
            duration_secs: 1200, label_id: None, notes: None,
            mode: SessionMode::Countdown,
            uuid: String::new(),
        }).unwrap();
        // May 1, midnight — EXCLUDED.
        db.insert_session(&Session {
            start_iso: "2026-05-01T00:00:00".to_string(),
            duration_secs: 8888, label_id: None, notes: None,
            mode: SessionMode::Countdown,
            uuid: String::new(),
        }).unwrap();

        assert_eq!(db.month_total_secs(2026, 4).unwrap(), 600 + 1200);
    }

    #[test]
    fn month_total_secs_handles_december_year_rollover() {
        // The "next month" boundary is built in code; December must
        // roll to next-year-January cleanly.
        let db = Database::open_in_memory().unwrap();
        db.insert_session(&Session {
            start_iso: "2026-12-15T10:00:00".to_string(),
            duration_secs: 600, label_id: None, notes: None,
            mode: SessionMode::Countdown,
            uuid: String::new(),
        }).unwrap();
        // Jan 1, 2027 — must NOT count toward Dec 2026.
        db.insert_session(&Session {
            start_iso: "2027-01-01T00:00:00".to_string(),
            duration_secs: 9999, label_id: None, notes: None,
            mode: SessionMode::Countdown,
            uuid: String::new(),
        }).unwrap();
        assert_eq!(db.month_total_secs(2026, 12).unwrap(), 600);
    }

    // ── total_secs_since: weekly goal ring etc. ──────────────────────────────

    #[test]
    fn total_secs_since_is_zero_for_empty_db() {
        let db = Database::open_in_memory().unwrap();
        let since = chrono::NaiveDate::from_ymd_opt(2026, 4, 27).unwrap();
        assert_eq!(db.total_secs_since(since).unwrap(), 0);
    }

    #[test]
    fn total_secs_since_includes_sessions_on_or_after_date() {
        // Cut-off is at the START of the local-naive `since` date — a
        // session at 00:00:00 on `since` IS included.
        let db = Database::open_in_memory().unwrap();
        // On the cut-off date.
        db.insert_session(&Session {
            start_iso: "2026-04-27T00:00:00".to_string(),
            duration_secs: 600, label_id: None, notes: None,
            mode: SessionMode::Countdown,
            uuid: String::new(),
        }).unwrap();
        // Later that day.
        db.insert_session(&Session {
            start_iso: "2026-04-27T18:00:00".to_string(),
            duration_secs: 1200, label_id: None, notes: None,
            mode: SessionMode::Countdown,
            uuid: String::new(),
        }).unwrap();
        // Following day.
        db.insert_session(&Session {
            start_iso: "2026-04-28T10:00:00".to_string(),
            duration_secs: 300, label_id: None, notes: None,
            mode: SessionMode::Countdown,
            uuid: String::new(),
        }).unwrap();
        let since = chrono::NaiveDate::from_ymd_opt(2026, 4, 27).unwrap();
        assert_eq!(db.total_secs_since(since).unwrap(), 600 + 1200 + 300);
    }

    #[test]
    fn total_secs_since_excludes_sessions_before_date() {
        let db = Database::open_in_memory().unwrap();
        // Day before the cut-off.
        db.insert_session(&Session {
            start_iso: "2026-04-26T23:59:59".to_string(),
            duration_secs: 9999, label_id: None, notes: None,
            mode: SessionMode::Countdown,
            uuid: String::new(),
        }).unwrap();
        // On / after cut-off — counted.
        db.insert_session(&Session {
            start_iso: "2026-04-27T00:00:00".to_string(),
            duration_secs: 600, label_id: None, notes: None,
            mode: SessionMode::Countdown,
            uuid: String::new(),
        }).unwrap();
        let since = chrono::NaiveDate::from_ymd_opt(2026, 4, 27).unwrap();
        assert_eq!(db.total_secs_since(since).unwrap(), 600);
    }

    #[test]
    fn total_secs_since_far_future_date_returns_zero() {
        // Asking for a date past every session's start returns 0.
        let db = Database::open_in_memory().unwrap();
        db.insert_session(&Session {
            start_iso: "2026-04-27T10:00:00".to_string(),
            duration_secs: 600, label_id: None, notes: None,
            mode: SessionMode::Countdown,
            uuid: String::new(),
        }).unwrap();
        let since = chrono::NaiveDate::from_ymd_opt(2099, 1, 1).unwrap();
        assert_eq!(db.total_secs_since(since).unwrap(), 0);
    }

    // ── get_longest_session ──────────────────────────────────────────────────

    #[test]
    fn get_longest_session_is_none_for_empty_db() {
        let db = Database::open_in_memory().unwrap();
        assert!(db.get_longest_session().unwrap().is_none());
    }

    #[test]
    fn get_longest_session_returns_only_session_for_single_row_db() {
        let db = Database::open_in_memory().unwrap();
        let mut session = Session {
            start_iso: "2026-04-27T10:00:00Z".to_string(),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Countdown,
            uuid: String::new(),
        };
        let id = db.insert_session(&session).unwrap();
        let (got_id, got) = db.get_longest_session().unwrap().unwrap();
        assert!(looks_like_uuid_v4(&got.uuid),
            "longest-session result must carry a v4 uuid");
        session.uuid = got.uuid.clone();
        assert_eq!((got_id, got), (id, session));
    }

    #[test]
    fn get_longest_session_returns_largest_duration() {
        // The longest among many — every other session must be shorter,
        // and the returned Session is the LONG one with all its fields
        // intact (not just the duration).
        let db = Database::open_in_memory().unwrap();
        for &secs in &[300u32, 600, 900, 1200, 450] {
            db.insert_session(&Session {
                start_iso: format!("2026-04-2{secs}T10:00:00Z"),
                duration_secs: secs,
                label_id: None,
                notes: None,
                mode: SessionMode::Countdown,
                uuid: String::new(),
            }).unwrap();
        }
        let mut longest_session = Session {
            start_iso: "2026-04-30T20:00:00Z".to_string(),
            duration_secs: 3600,
            label_id: None,
            notes: Some("the long one".to_string()),
            mode: SessionMode::Stopwatch,
            uuid: String::new(),
        };
        let longest_id = db.insert_session(&longest_session).unwrap();
        // Add one more shorter after — the order of insertion must not
        // affect which row wins.
        db.insert_session(&Session {
            start_iso: "2026-05-01T10:00:00Z".to_string(),
            duration_secs: 700,
            label_id: None,
            notes: None,
            mode: SessionMode::Countdown,
            uuid: String::new(),
        }).unwrap();

        let (got_id, got) = db.get_longest_session().unwrap().unwrap();
        assert!(looks_like_uuid_v4(&got.uuid));
        longest_session.uuid = got.uuid.clone();
        assert_eq!(got_id, longest_id);
        assert_eq!(got, longest_session,
            "the returned Session must have every field of the long row, not just duration");
    }

    // ── total_seconds: precision-preserving aggregate ─────────────────────────

    #[test]
    fn total_seconds_is_zero_for_empty_db() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(db.total_seconds().unwrap(), 0);
    }

    #[test]
    fn total_seconds_sums_all_durations() {
        // Sums every session, regardless of label / mode / notes.
        let db = Database::open_in_memory().unwrap();
        db.insert_session(&Session {
            start_iso: "2026-04-27T10:00:00Z".to_string(),
            duration_secs: 600, label_id: None, notes: None,
            mode: SessionMode::Countdown,
            uuid: String::new(),
        }).unwrap();
        db.insert_session(&Session {
            start_iso: "2026-04-27T11:00:00Z".to_string(),
            duration_secs: 1245, label_id: None, notes: None,
            mode: SessionMode::Stopwatch,
            uuid: String::new(),
        }).unwrap();
        // Sub-minute remainder must NOT be lost — the whole point of
        // having a seconds aggregate alongside total_minutes.
        db.insert_session(&Session {
            start_iso: "2026-04-27T12:00:00Z".to_string(),
            duration_secs: 17, label_id: None, notes: None,
            mode: SessionMode::BoxBreath,
            uuid: String::new(),
        }).unwrap();
        assert_eq!(db.total_seconds().unwrap(), 600 + 1245 + 17);
    }

    #[test]
    fn total_minutes_agrees_with_total_seconds_div_60() {
        // After refactoring total_minutes to delegate to total_seconds,
        // the contract is: minutes = seconds / 60 (integer division).
        let db = Database::open_in_memory().unwrap();
        for &secs in &[59i64, 60, 61, 119, 120, 600, 1245] {
            db.insert_session(&Session {
                start_iso: format!("2026-04-27T10:{:02}:00Z", secs % 60),
                duration_secs: secs as u32, label_id: None, notes: None,
                mode: SessionMode::Countdown,
                uuid: String::new(),
            }).unwrap();
        }
        let secs = db.total_seconds().unwrap();
        let mins = db.total_minutes().unwrap();
        assert_eq!(mins, secs / 60);
    }

    // ── query_sessions: rich filter for the log feed ──────────────────────────

    #[test]
    fn query_sessions_default_filter_returns_all_newest_first() {
        // Default-constructed SessionFilter: no filter, no pagination —
        // every session, ordered start_iso DESC (newest first), to match
        // the log feed UX.
        let db = Database::open_in_memory().unwrap();
        let make = |iso: &str| Session {
            start_iso: iso.to_string(),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Countdown,
            uuid: String::new(),
        };
        let _id_old = db.insert_session(&make("2026-04-25T10:00:00Z")).unwrap();
        let _id_new = db.insert_session(&make("2026-04-27T10:00:00Z")).unwrap();
        let _id_mid = db.insert_session(&make("2026-04-26T10:00:00Z")).unwrap();

        let rows = db.query_sessions(&SessionFilter::default()).unwrap();
        let isos: Vec<&str> = rows.iter().map(|(_, s)| s.start_iso.as_str()).collect();
        assert_eq!(
            isos,
            vec!["2026-04-27T10:00:00Z", "2026-04-26T10:00:00Z", "2026-04-25T10:00:00Z"],
            "rows must be ordered start_iso DESC",
        );
    }

    #[test]
    fn query_sessions_empty_db_returns_empty_vec() {
        // No rows — not an error, just an empty Vec.
        let db = Database::open_in_memory().unwrap();
        let rows = db.query_sessions(&SessionFilter::default()).unwrap();
        assert!(rows.is_empty());
    }

    #[test]
    fn query_sessions_limit_caps_result_count() {
        // limit=N returns at most N rows; the cap applies AFTER ordering,
        // so the newest N are returned.
        let db = Database::open_in_memory().unwrap();
        for d in 20..28 {
            db.insert_session(&Session {
                start_iso: format!("2026-04-{d:02}T10:00:00Z"),
                duration_secs: 600,
                label_id: None,
                notes: None,
                mode: SessionMode::Countdown,
                uuid: String::new(),
            }).unwrap();
        }
        let rows = db.query_sessions(&SessionFilter {
            limit: Some(3), ..Default::default()
        }).unwrap();
        let isos: Vec<&str> = rows.iter().map(|(_, s)| s.start_iso.as_str()).collect();
        assert_eq!(
            isos,
            vec!["2026-04-27T10:00:00Z", "2026-04-26T10:00:00Z", "2026-04-25T10:00:00Z"],
            "limit=3 must return the newest 3",
        );
    }

    #[test]
    fn query_sessions_offset_skips_initial_rows() {
        // offset=N skips the first N (in DESC order). Combined with
        // limit, this is the pagination contract: "give me page p of size s"
        // is offset = (p-1)*s, limit = s.
        let db = Database::open_in_memory().unwrap();
        for d in 20..28 {
            db.insert_session(&Session {
                start_iso: format!("2026-04-{d:02}T10:00:00Z"),
                duration_secs: 600,
                label_id: None,
                notes: None,
                mode: SessionMode::Countdown,
                uuid: String::new(),
            }).unwrap();
        }
        // Page 2 of size 3: skip 3, take 3.
        let rows = db.query_sessions(&SessionFilter {
            limit: Some(3),
            offset: Some(3),
            ..Default::default()
        }).unwrap();
        let isos: Vec<&str> = rows.iter().map(|(_, s)| s.start_iso.as_str()).collect();
        assert_eq!(
            isos,
            vec!["2026-04-24T10:00:00Z", "2026-04-23T10:00:00Z", "2026-04-22T10:00:00Z"],
            "page 2 of size 3 must be rows 4-6 in DESC order",
        );
    }

    #[test]
    fn query_sessions_offset_past_total_returns_empty() {
        // Asking for a page past the end is not an error.
        let db = Database::open_in_memory().unwrap();
        db.insert_session(&Session {
            start_iso: "2026-04-27T10:00:00Z".to_string(),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Countdown,
            uuid: String::new(),
        }).unwrap();
        let rows = db.query_sessions(&SessionFilter {
            offset: Some(100),
            ..Default::default()
        }).unwrap();
        assert!(rows.is_empty());
    }

    #[test]
    fn query_sessions_label_id_filters_by_label() {
        // label_id=Some(id) keeps only sessions referencing that label.
        let db = Database::open_in_memory().unwrap();
        let morning = db.insert_label("Morning").unwrap();
        let evening = db.insert_label("Evening").unwrap();
        // 2 Morning, 1 Evening, 1 unlabeled.
        db.insert_session(&Session {
            start_iso: "2026-04-27T10:00:00Z".to_string(),
            duration_secs: 600, label_id: Some(morning),
            notes: None, mode: SessionMode::Countdown,
            uuid: String::new(),
        }).unwrap();
        db.insert_session(&Session {
            start_iso: "2026-04-27T11:00:00Z".to_string(),
            duration_secs: 600, label_id: Some(morning),
            notes: None, mode: SessionMode::Countdown,
            uuid: String::new(),
        }).unwrap();
        db.insert_session(&Session {
            start_iso: "2026-04-27T19:00:00Z".to_string(),
            duration_secs: 600, label_id: Some(evening),
            notes: None, mode: SessionMode::Countdown,
            uuid: String::new(),
        }).unwrap();
        db.insert_session(&Session {
            start_iso: "2026-04-27T20:00:00Z".to_string(),
            duration_secs: 600, label_id: None,
            notes: None, mode: SessionMode::Countdown,
            uuid: String::new(),
        }).unwrap();

        let rows = db.query_sessions(&SessionFilter {
            label_id: Some(morning), ..Default::default()
        }).unwrap();
        assert_eq!(rows.len(), 2);
        for (_, s) in &rows {
            assert_eq!(s.label_id, Some(morning));
        }
    }

    #[test]
    fn query_sessions_only_with_notes_excludes_empty_and_null() {
        // only_with_notes=true matches when notes IS NOT NULL AND notes != ''.
        // Both None (NULL in DB) and Some("") must be excluded.
        let db = Database::open_in_memory().unwrap();
        // With note.
        db.insert_session(&Session {
            start_iso: "2026-04-27T10:00:00Z".to_string(),
            duration_secs: 600, label_id: None,
            notes: Some("kept focus".to_string()),
            mode: SessionMode::Countdown,
            uuid: String::new(),
        }).unwrap();
        // Without note (None).
        db.insert_session(&Session {
            start_iso: "2026-04-27T11:00:00Z".to_string(),
            duration_secs: 600, label_id: None,
            notes: None, mode: SessionMode::Countdown,
            uuid: String::new(),
        }).unwrap();
        // Empty-string note — also excluded.
        db.insert_session(&Session {
            start_iso: "2026-04-27T12:00:00Z".to_string(),
            duration_secs: 600, label_id: None,
            notes: Some("".to_string()),
            mode: SessionMode::Countdown,
            uuid: String::new(),
        }).unwrap();

        let rows = db.query_sessions(&SessionFilter {
            only_with_notes: true, ..Default::default()
        }).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].1.notes, Some("kept focus".to_string()));
    }

    #[test]
    fn query_sessions_combines_label_filter_and_notes_filter() {
        // Compound filter: label_id AND only_with_notes both apply.
        let db = Database::open_in_memory().unwrap();
        let morning = db.insert_label("Morning").unwrap();
        // Morning + note → kept.
        db.insert_session(&Session {
            start_iso: "2026-04-27T10:00:00Z".to_string(),
            duration_secs: 600, label_id: Some(morning),
            notes: Some("yes".to_string()),
            mode: SessionMode::Countdown,
            uuid: String::new(),
        }).unwrap();
        // Morning, no note → dropped (notes filter).
        db.insert_session(&Session {
            start_iso: "2026-04-27T11:00:00Z".to_string(),
            duration_secs: 600, label_id: Some(morning),
            notes: None, mode: SessionMode::Countdown,
            uuid: String::new(),
        }).unwrap();
        // No label, with note → dropped (label filter).
        db.insert_session(&Session {
            start_iso: "2026-04-27T12:00:00Z".to_string(),
            duration_secs: 600, label_id: None,
            notes: Some("orphan".to_string()),
            mode: SessionMode::Countdown,
            uuid: String::new(),
        }).unwrap();

        let rows = db.query_sessions(&SessionFilter {
            label_id: Some(morning),
            only_with_notes: true,
            ..Default::default()
        }).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].1.notes, Some("yes".to_string()));
    }

    #[test]
    fn query_sessions_pagination_walks_all_rows_without_overlap() {
        // Walking pages of size N covers every row exactly once.
        let db = Database::open_in_memory().unwrap();
        for d in 1..=10 {
            db.insert_session(&Session {
                start_iso: format!("2026-04-{d:02}T10:00:00Z"),
                duration_secs: 600, label_id: None,
                notes: None, mode: SessionMode::Countdown,
                uuid: String::new(),
            }).unwrap();
        }
        let mut seen: Vec<i64> = Vec::new();
        let mut offset = 0u32;
        loop {
            let page = db.query_sessions(&SessionFilter {
                limit: Some(3),
                offset: Some(offset),
                ..Default::default()
            }).unwrap();
            if page.is_empty() { break; }
            for (id, _) in &page { seen.push(*id); }
            offset += page.len() as u32;
        }
        assert_eq!(seen.len(), 10);
        // No duplicates.
        let mut sorted = seen.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), 10);
    }

    #[test]
    fn get_setting_returns_default_when_key_missing() {
        // Reads of unset keys fall back to the caller-provided default
        // (no INSERT, no error).
        let db = Database::open_in_memory().unwrap();
        assert_eq!(
            db.get_setting("timer_presets", "5,10,15,20,30").unwrap(),
            "5,10,15,20,30",
        );
        // The key remained absent — getting it again returns the same default.
        assert_eq!(
            db.get_setting("timer_presets", "5,10,15,20,30").unwrap(),
            "5,10,15,20,30",
        );
    }

    #[test]
    fn set_setting_then_get_setting_round_trip() {
        // Setting a key persists the value; subsequent gets ignore the
        // default and return the stored value verbatim.
        let db = Database::open_in_memory().unwrap();
        db.set_setting("timer_presets", "3,7,12").unwrap();
        assert_eq!(
            db.get_setting("timer_presets", "5,10,15,20,30").unwrap(),
            "3,7,12",
        );
    }

    #[test]
    fn set_setting_overwrites_existing_value() {
        // Repeat sets overwrite (UPSERT semantics). The second value
        // wins; the row count stays at 1 per key.
        let db = Database::open_in_memory().unwrap();
        db.set_setting("daily_goal_mins", "20").unwrap();
        db.set_setting("daily_goal_mins", "25").unwrap();
        assert_eq!(db.get_setting("daily_goal_mins", "0").unwrap(), "25");
    }

    #[test]
    fn settings_keys_are_independent() {
        // Setting key A does not affect key B's value or default.
        let db = Database::open_in_memory().unwrap();
        db.set_setting("daily_goal_mins", "20").unwrap();
        // Other keys still return their defaults.
        assert_eq!(db.get_setting("weekly_goal_mins", "150").unwrap(), "150");
        // The set key is unaffected.
        assert_eq!(db.get_setting("daily_goal_mins", "0").unwrap(), "20");
    }

    #[test]
    fn set_setting_accepts_empty_string_and_unicode() {
        // Values are opaque to the DB layer — UTF-8 string in, UTF-8 string out.
        let db = Database::open_in_memory().unwrap();
        db.set_setting("note_template", "").unwrap();
        assert_eq!(db.get_setting("note_template", "fallback").unwrap(), "");
        db.set_setting("greeting", "こんにちは ☀️").unwrap();
        assert_eq!(db.get_setting("greeting", "").unwrap(), "こんにちは ☀️");
    }

    #[test]
    fn is_label_name_taken_false_for_empty_db() {
        // Nothing exists ⇒ no name is taken.
        let db = Database::open_in_memory().unwrap();
        assert!(!db.is_label_name_taken("Morning", 0).unwrap());
    }

    #[test]
    fn is_label_name_taken_true_for_existing_other_label() {
        // Another row holds this name. Exclude id is something else.
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        let evening = db.insert_label("Evening").unwrap();
        // Asking "is 'Morning' taken by anyone other than `evening`?"
        // returns true because Morning is held by a different row.
        assert!(db.is_label_name_taken("Morning", evening).unwrap());
    }

    #[test]
    fn is_label_name_taken_false_when_only_owner_is_excluded() {
        // The single row holding this name is the one being excluded —
        // typical pre-rename validation: 'is this name taken by anyone
        // OTHER THAN the row I'm about to update?'
        let db = Database::open_in_memory().unwrap();
        let morning = db.insert_label("Morning").unwrap();
        assert!(!db.is_label_name_taken("Morning", morning).unwrap());
    }

    #[test]
    fn is_label_name_taken_is_case_insensitive() {
        // The column is COLLATE NOCASE — name comparison must follow.
        let db = Database::open_in_memory().unwrap();
        let morning = db.insert_label("Morning").unwrap();
        // Different casing of an existing name is still 'taken'.
        assert!(db.is_label_name_taken("morning", 0).unwrap());
        assert!(db.is_label_name_taken("MORNING", 0).unwrap());
        // …unless the holder is the excluded row.
        assert!(!db.is_label_name_taken("morning", morning).unwrap());
    }

    #[test]
    fn label_session_count_zero_for_unreferenced_label() {
        // A freshly-created label has no sessions referencing it.
        let db = Database::open_in_memory().unwrap();
        let id = db.insert_label("Morning").unwrap();
        assert_eq!(db.label_session_count(id).unwrap(), 0);
    }

    #[test]
    fn label_session_count_counts_referencing_sessions() {
        // Counts only sessions whose label_id matches this label's id.
        // Sessions without labels and sessions with OTHER labels are not
        // counted.
        let db = Database::open_in_memory().unwrap();
        let morning = db.insert_label("Morning").unwrap();
        let evening = db.insert_label("Evening").unwrap();

        // Three Morning sessions.
        for i in 0..3 {
            db.insert_session(&Session {
                start_iso: format!("2026-04-2{i}T10:00:00Z"),
                duration_secs: 600,
                label_id: Some(morning),
                notes: None,
                mode: SessionMode::Countdown,
                uuid: String::new(),
            }).unwrap();
        }
        // One Evening session — must not contribute to Morning's count.
        db.insert_session(&Session {
            start_iso: "2026-04-27T19:00:00Z".to_string(),
            duration_secs: 600,
            label_id: Some(evening),
            notes: None,
            mode: SessionMode::Countdown,
            uuid: String::new(),
        }).unwrap();
        // Two unlabeled sessions — must not contribute either.
        for i in 0..2 {
            db.insert_session(&Session {
                start_iso: format!("2026-04-2{i}T20:00:00Z"),
                duration_secs: 300,
                label_id: None,
                notes: None,
                mode: SessionMode::Stopwatch,
                uuid: String::new(),
            }).unwrap();
        }

        assert_eq!(db.label_session_count(morning).unwrap(), 3);
        assert_eq!(db.label_session_count(evening).unwrap(), 1);
    }

    #[test]
    fn label_session_count_unknown_id_is_zero() {
        // No row ⇒ no references; not an error.
        let db = Database::open_in_memory().unwrap();
        assert_eq!(db.label_session_count(9999).unwrap(), 0);
    }

    #[test]
    fn delete_label_removes_only_that_row() {
        // Delete addresses one row by id; siblings survive.
        let db = Database::open_in_memory().unwrap();
        let morning = db.insert_label("Morning").unwrap();
        let evening = db.insert_label("Evening").unwrap();

        db.delete_label(morning).unwrap();

        // Morning is gone, Evening remains.
        assert_eq!(db.find_label_by_name("Morning").unwrap(), None);
        assert_eq!(db.find_label_by_name("Evening").unwrap(), Some(evening));
        let names: Vec<String> =
            db.list_labels().unwrap().into_iter().map(|l| l.name).collect();
        assert_eq!(names, vec!["Evening"]);
        assert_eq!(db.count_labels().unwrap(), 1);
    }

    #[test]
    fn delete_label_unknown_id_is_noop() {
        // Matches SQLite DELETE semantics.
        let db = Database::open_in_memory().unwrap();
        let id = db.insert_label("Morning").unwrap();
        db.delete_label(id + 999).unwrap();
        assert_eq!(db.count_labels().unwrap(), 1);
        assert_eq!(db.find_label_by_name("Morning").unwrap(), Some(id));
    }

    #[test]
    fn delete_label_unlinks_sessions_via_set_null() {
        // Deleting a label must NOT destroy historical sessions — the
        // FK is ON DELETE SET NULL on the sessions side, so referenced
        // sessions survive with label_id = None.
        let db = Database::open_in_memory().unwrap();
        let morning = db.insert_label("Morning").unwrap();

        let labeled_id = db.insert_session(&Session {
            start_iso: "2026-04-27T10:00:00Z".to_string(),
            duration_secs: 600,
            label_id: Some(morning),
            notes: Some("first sit".to_string()),
            mode: SessionMode::Countdown,
            uuid: String::new(),
        }).unwrap();
        // A second labeled session — proves the unlink happens for ALL
        // referencing rows, not just the first.
        let labeled_id2 = db.insert_session(&Session {
            start_iso: "2026-04-27T11:00:00Z".to_string(),
            duration_secs: 1200,
            label_id: Some(morning),
            notes: None,
            mode: SessionMode::Stopwatch,
            uuid: String::new(),
        }).unwrap();
        // An unlabeled control — must remain unlabeled (was None, stays None).
        let unlabeled_id = db.insert_session(&Session {
            start_iso: "2026-04-27T12:00:00Z".to_string(),
            duration_secs: 300,
            label_id: None,
            notes: None,
            mode: SessionMode::BoxBreath,
            uuid: String::new(),
        }).unwrap();

        db.delete_label(morning).unwrap();

        // Both formerly-labeled sessions survive but have lost their label.
        let rows = db.list_sessions().unwrap();
        assert_eq!(rows.len(), 3, "all sessions must survive label deletion");
        let by_id: std::collections::HashMap<i64, &Session> =
            rows.iter().map(|(i, s)| (*i, s)).collect();
        assert_eq!(by_id[&labeled_id].label_id, None);
        assert_eq!(by_id[&labeled_id2].label_id, None);
        assert_eq!(by_id[&unlabeled_id].label_id, None);

        // The label row is gone.
        assert_eq!(db.count_labels().unwrap(), 0);
    }

    #[test]
    fn delete_label_does_not_affect_unrelated_sessions() {
        // Sessions referencing OTHER labels are untouched when one
        // label is deleted.
        let db = Database::open_in_memory().unwrap();
        let morning = db.insert_label("Morning").unwrap();
        let evening = db.insert_label("Evening").unwrap();

        let evening_id = db.insert_session(&Session {
            start_iso: "2026-04-27T19:00:00Z".to_string(),
            duration_secs: 600,
            label_id: Some(evening),
            notes: None,
            mode: SessionMode::Countdown,
            uuid: String::new(),
        }).unwrap();

        db.delete_label(morning).unwrap();

        // Evening session still points at Evening label.
        let row = &db.list_sessions().unwrap()[0];
        assert_eq!(row.0, evening_id);
        assert_eq!(row.1.label_id, Some(evening));
    }

    #[test]
    fn update_label_renames_row() {
        // Rename takes id + new name. The row keeps its id but the
        // name changes; sibling labels are untouched.
        let db = Database::open_in_memory().unwrap();
        let morning = db.insert_label("Morning").unwrap();
        let evening = db.insert_label("Evening").unwrap();

        db.update_label(morning, "Pre-coffee").unwrap();

        // Morning row now reports the new name.
        assert_eq!(db.find_label_by_name("Pre-coffee").unwrap(), Some(morning));
        // Old name is gone.
        assert_eq!(db.find_label_by_name("Morning").unwrap(), None);
        // Sibling untouched.
        assert_eq!(db.find_label_by_name("Evening").unwrap(), Some(evening));
        // Count unchanged.
        assert_eq!(db.count_labels().unwrap(), 2);
    }

    #[test]
    fn update_label_to_same_name_is_idempotent() {
        // Renaming to the current name is a no-op, not a UNIQUE violation.
        // The row updates "to itself" — SQLite UPDATE allows this.
        let db = Database::open_in_memory().unwrap();
        let id = db.insert_label("Morning").unwrap();
        db.update_label(id, "Morning").unwrap();
        // Still one row, still the same id.
        assert_eq!(db.count_labels().unwrap(), 1);
        assert_eq!(db.find_label_by_name("Morning").unwrap(), Some(id));
    }

    #[test]
    fn list_labels_returns_label_per_row_alphabetic_by_name() {
        // Each retrieved Label carries its rowid so callers can address it
        // for update/delete. Order is alphabetic-by-name (case-insensitive)
        // for stable UI rendering.
        let db = Database::open_in_memory().unwrap();
        let evening = db.insert_label("Evening").unwrap();
        let morning = db.insert_label("Morning").unwrap();
        let afternoon = db.insert_label("Afternoon").unwrap();

        let rows = db.list_labels().unwrap();
        assert_eq!(rows.len(), 3);
        // Every label must carry a v4 uuid, and uuids must be pairwise
        // distinct (UNIQUE constraint at the column level guarantees that;
        // assert it here to document the contract from the read side).
        let uuids: std::collections::HashSet<_> =
            rows.iter().map(|l| l.uuid.clone()).collect();
        assert_eq!(uuids.len(), 3, "labels must have distinct uuids");
        for label in &rows {
            assert!(looks_like_uuid_v4(&label.uuid),
                "label {label:?} missing v4 uuid");
        }
        // Compare full structs — bind each label's uuid into the expected
        // value so id, name AND uuid all participate in the assertion.
        let by_name: std::collections::HashMap<_, _> =
            rows.iter().map(|l| (l.name.clone(), l.uuid.clone())).collect();
        assert_eq!(rows, vec![
            Label { id: afternoon, name: "Afternoon".to_string(),
                uuid: by_name["Afternoon"].clone() },
            Label { id: evening,   name: "Evening".to_string(),
                uuid: by_name["Evening"].clone() },
            Label { id: morning,   name: "Morning".to_string(),
                uuid: by_name["Morning"].clone() },
        ]);
    }

    #[test]
    fn list_labels_returns_label_per_row_case_insensitive_sort() {
        // The column is COLLATE NOCASE — sort must follow, so 'apple',
        // 'Banana', 'cherry' come back in that order even with mixed case.
        let db = Database::open_in_memory().unwrap();
        let banana = db.insert_label("Banana").unwrap();
        let cherry = db.insert_label("cherry").unwrap();
        let apple = db.insert_label("apple").unwrap();
        let rows = db.list_labels().unwrap();
        let names: Vec<&str> = rows.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, vec!["apple", "Banana", "cherry"]);
        // Each row carries the original casing (no normalisation on read).
        assert_eq!(rows[0].id, apple);
        assert_eq!(rows[1].id, banana);
        assert_eq!(rows[2].id, cherry);
    }

    #[test]
    fn update_label_to_case_variant_of_own_name_succeeds() {
        // Capitalising "morning" → "Morning" is a legitimate rename of
        // the same row. Because of COLLATE NOCASE on UNIQUE, SQLite
        // does NOT see this as a collision against itself.
        let db = Database::open_in_memory().unwrap();
        let id = db.insert_label("morning").unwrap();
        db.update_label(id, "Morning").unwrap();
        // Lookup by either case still finds the row (NOCASE column).
        assert_eq!(db.find_label_by_name("morning").unwrap(), Some(id));
        assert_eq!(db.find_label_by_name("Morning").unwrap(), Some(id));
        // The actual stored value is the new casing.
        let names: Vec<String> =
            db.list_labels().unwrap().into_iter().map(|l| l.name).collect();
        assert_eq!(names, vec!["Morning"]);
    }

    #[test]
    fn update_label_to_existing_other_name_returns_duplicate_error() {
        // Renaming to a name another row already has must fail with
        // DuplicateLabel. The DB stays unchanged.
        let db = Database::open_in_memory().unwrap();
        let morning = db.insert_label("Morning").unwrap();
        let _evening = db.insert_label("Evening").unwrap();

        let result = db.update_label(morning, "Evening");
        assert!(
            matches!(result, Err(DbError::DuplicateLabel(ref n)) if n == "Evening"),
            "expected DuplicateLabel(\"Evening\"), got {result:?}"
        );
        // Both rows survive with their original names.
        assert_eq!(db.find_label_by_name("Morning").unwrap(), Some(morning));
        let names: Vec<String> =
            db.list_labels().unwrap().into_iter().map(|l| l.name).collect();
        assert_eq!(names, vec!["Evening", "Morning"]);
    }

    #[test]
    fn update_label_to_case_variant_of_other_name_returns_duplicate_error() {
        // Case-insensitive collision: renaming "Morning" to "evening"
        // collides with existing "Evening" because labels.name is
        // COLLATE NOCASE.
        let db = Database::open_in_memory().unwrap();
        let morning = db.insert_label("Morning").unwrap();
        db.insert_label("Evening").unwrap();

        let result = db.update_label(morning, "evening");
        assert!(
            matches!(result, Err(DbError::DuplicateLabel(ref n)) if n == "evening"),
            "expected DuplicateLabel(\"evening\"), got {result:?}"
        );
    }

    #[test]
    fn update_label_unknown_id_is_noop() {
        // Matches the SQLite UPDATE-zero-rows convention shared by
        // update_session: missing id is silent.
        let db = Database::open_in_memory().unwrap();
        let id = db.insert_label("Morning").unwrap();
        db.update_label(id + 999, "Phantom").unwrap();
        // Original row untouched; phantom name not present.
        assert_eq!(db.find_label_by_name("Morning").unwrap(), Some(id));
        assert_eq!(db.find_label_by_name("Phantom").unwrap(), None);
        assert_eq!(db.count_labels().unwrap(), 1);
    }

    #[test]
    fn insert_label_returns_new_rowid() {
        // insert_label returns the AUTOINCREMENT id of the new row,
        // matching insert_session's contract. AUTOINCREMENT starts at 1.
        let db = Database::open_in_memory().unwrap();
        let id1 = db.insert_label("Morning").unwrap();
        let id2 = db.insert_label("Evening").unwrap();
        let id3 = db.insert_label("Afternoon").unwrap();
        assert_eq!(id1, 1);
        assert_eq!(id2, 2);
        assert_eq!(id3, 3);
        // The returned id matches what find_label_by_name reports.
        assert_eq!(db.find_label_by_name("Morning").unwrap(), Some(id1));
        assert_eq!(db.find_label_by_name("Evening").unwrap(), Some(id2));
    }

    #[test]
    fn find_or_create_label_creates_when_missing() {
        // First call to a fresh DB inserts the label and returns its new id.
        let db = Database::open_in_memory().unwrap();
        let id = db.find_or_create_label("Morning").unwrap();
        assert_eq!(db.count_labels().unwrap(), 1);
        // The returned id matches what find_label_by_name reports.
        assert_eq!(db.find_label_by_name("Morning").unwrap(), Some(id));
    }

    #[test]
    fn find_or_create_label_returns_existing_id() {
        // If the label already exists, the existing id is returned and
        // no new row is created.
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        let existing = db.find_label_by_name("Morning").unwrap().unwrap();
        let got = db.find_or_create_label("Morning").unwrap();
        assert_eq!(got, existing);
        assert_eq!(db.count_labels().unwrap(), 1);
    }

    #[test]
    fn find_or_create_label_is_case_insensitive() {
        // CSV import frequently differs in case from what the user
        // already has; we must reuse the existing row, not duplicate.
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        let existing = db.find_label_by_name("Morning").unwrap().unwrap();
        // Lookup with different casings — same id, no new rows.
        assert_eq!(db.find_or_create_label("morning").unwrap(), existing);
        assert_eq!(db.find_or_create_label("MORNING").unwrap(), existing);
        assert_eq!(db.count_labels().unwrap(), 1);
    }

    #[test]
    fn find_or_create_label_idempotent_across_calls() {
        // Calling repeatedly never inflates the row count.
        let db = Database::open_in_memory().unwrap();
        let id1 = db.find_or_create_label("Evening").unwrap();
        let id2 = db.find_or_create_label("Evening").unwrap();
        let id3 = db.find_or_create_label("evening").unwrap(); // case variant
        assert_eq!(id1, id2);
        assert_eq!(id1, id3);
        assert_eq!(db.count_labels().unwrap(), 1);
    }

    #[test]
    fn label_uniqueness_is_case_insensitive() {
        // Avoid "Morning" / "morning" as separate rows. The DB enforces
        // case-insensitive uniqueness so UI bugs that skip pre-validation
        // (is_label_name_taken) still get caught at the DB layer.
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        let result = db.insert_label("morning");
        assert!(
            matches!(result, Err(DbError::DuplicateLabel(ref name)) if name == "morning"),
            "expected DuplicateLabel for 'morning', got {result:?}"
        );
        // Different mixed-case is also a duplicate.
        assert!(matches!(db.insert_label("MORNING"), Err(DbError::DuplicateLabel(_))));
        assert!(matches!(db.insert_label("MoRnInG"), Err(DbError::DuplicateLabel(_))));
        // Only the original survives.
        assert_eq!(db.count_labels().unwrap(), 1);
    }

    #[test]
    fn find_label_by_name_is_case_insensitive() {
        // Lookups follow the column's NOCASE collation so a case-different
        // search still finds the existing row — same id, same row.
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        let canonical_id = db.find_label_by_name("Morning").unwrap();
        assert!(canonical_id.is_some());
        // All these case variants must return the SAME id.
        assert_eq!(db.find_label_by_name("morning").unwrap(), canonical_id);
        assert_eq!(db.find_label_by_name("MORNING").unwrap(), canonical_id);
        assert_eq!(db.find_label_by_name("MoRnInG").unwrap(), canonical_id);
    }

    #[test]
    fn duplicate_label_error_identifies_offending_name() {
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        let err = db.insert_label("Morning").unwrap_err();
        assert!(
            matches!(err, DbError::DuplicateLabel(ref name) if name == "Morning"),
            "expected DuplicateLabel(\"Morning\"), got {err:?}"
        );
    }

    #[test]
    fn list_labels_returns_inserted_names_alphabetically() {
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        db.insert_label("Afternoon").unwrap();
        db.insert_label("Evening").unwrap();
        let names: Vec<String> =
            db.list_labels().unwrap().into_iter().map(|l| l.name).collect();
        assert_eq!(names, vec!["Afternoon", "Evening", "Morning"]);
    }

    #[test]
    fn find_label_by_name_returns_some_id_when_present() {
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        let id = db.find_label_by_name("Morning").unwrap();
        assert!(id.is_some());
    }

    #[test]
    fn find_label_by_name_returns_none_when_absent() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(db.find_label_by_name("Morning").unwrap(), None);
    }

    #[test]
    fn empty_database_has_zero_sessions() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(db.count_sessions().unwrap(), 0);
    }

    #[test]
    fn insert_session_increases_count() {
        let db = Database::open_in_memory().unwrap();
        let session = Session {
            start_iso: "2026-04-27T10:00:00Z".to_string(),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Countdown,
            uuid: String::new(),
        };
        db.insert_session(&session).unwrap();
        assert_eq!(db.count_sessions().unwrap(), 1);
    }

    #[test]
    fn list_sessions_for_label_filters_by_label_id() {
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        let morning = db.find_label_by_name("Morning").unwrap().unwrap();
        let mut labeled = Session {
            start_iso: "2026-04-27T10:00:00Z".to_string(),
            duration_secs: 600,
            label_id: Some(morning),
            notes: None,
            mode: SessionMode::Countdown,
            uuid: String::new(),
        };
        let unlabeled = Session {
            start_iso: "2026-04-27T19:00:00Z".to_string(),
            duration_secs: 300,
            label_id: None,
            notes: None,
            mode: SessionMode::BoxBreath,
            uuid: String::new(),
        };
        let labeled_id = db.insert_session(&labeled).unwrap();
        db.insert_session(&unlabeled).unwrap();
        let rows = db.list_sessions_for_label(morning).unwrap();
        assert_eq!(rows.len(), 1, "only the labeled session must be returned");
        assert!(looks_like_uuid_v4(&rows[0].1.uuid));
        labeled.uuid = rows[0].1.uuid.clone();
        assert_eq!(rows, vec![(labeled_id, labeled)]);
    }

    #[test]
    fn list_sessions_round_trips_inserted_session() {
        let db = Database::open_in_memory().unwrap();
        let mut session = Session {
            start_iso: "2026-04-27T10:00:00Z".to_string(),
            duration_secs: 600,
            label_id: None,
            notes: Some("felt clear today".to_string()),
            mode: SessionMode::BoxBreath,
            uuid: String::new(),
        };
        let id = db.insert_session(&session).unwrap();
        let rows = db.list_sessions().unwrap();
        assert_eq!(rows.len(), 1);
        assert!(looks_like_uuid_v4(&rows[0].1.uuid),
            "round-tripped session must carry a v4 uuid");
        // Adopt the DB-assigned uuid into the expected value so the full
        // struct comparison below covers every other field exactly.
        session.uuid = rows[0].1.uuid.clone();
        assert_eq!(rows, vec![(id, session)]);
    }

    #[test]
    fn list_sessions_returns_id_per_row_in_insert_order() {
        // Each retrieved row carries its DB rowid so callers can address it
        // for update / delete. Ids are SQLite AUTOINCREMENT, so they
        // increase strictly and start at 1 on a fresh DB.
        let db = Database::open_in_memory().unwrap();
        let make = |start: &str| Session {
            start_iso: start.to_string(),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Countdown,
            uuid: String::new(),
        };
        let id1 = db.insert_session(&make("2026-04-27T10:00:00Z")).unwrap();
        let id2 = db.insert_session(&make("2026-04-27T11:00:00Z")).unwrap();
        let id3 = db.insert_session(&make("2026-04-27T12:00:00Z")).unwrap();
        assert_eq!(id1, 1);
        assert_eq!(id2, 2);
        assert_eq!(id3, 3);
        let rows = db.list_sessions().unwrap();
        let got_ids: Vec<i64> = rows.iter().map(|(id, _)| *id).collect();
        assert_eq!(got_ids, vec![id1, id2, id3]);
    }

    #[test]
    fn update_session_replaces_all_fields() {
        // Update is destructive: every field of the new Session value
        // overwrites the row, identified by id. The other rows stay
        // untouched.
        let db = Database::open_in_memory().unwrap();
        let original = Session {
            start_iso: "2026-04-27T10:00:00Z".to_string(),
            duration_secs: 600,
            label_id: None,
            notes: Some("first take".to_string()),
            mode: SessionMode::Countdown,
            uuid: String::new(),
        };
        let id = db.insert_session(&original).unwrap();

        // Insert a sibling that must remain untouched.
        let other_id = db.insert_session(&Session {
            start_iso: "2026-04-27T11:00:00Z".to_string(),
            duration_secs: 300,
            label_id: None,
            notes: None,
            mode: SessionMode::Stopwatch,
            uuid: String::new(),
        }).unwrap();

        db.insert_label("Evening").unwrap();
        let evening = db.find_label_by_name("Evening").unwrap().unwrap();
        let mut updated = Session {
            start_iso: "2026-04-28T19:00:00Z".to_string(),
            duration_secs: 1500,
            label_id: Some(evening),
            notes: Some("after dinner".to_string()),
            mode: SessionMode::BoxBreath,
            uuid: String::new(),
        };
        db.update_session(id, &updated).unwrap();

        let rows = db.list_sessions().unwrap();
        assert_eq!(rows.len(), 2);
        // Updated row reflects every new field. Its uuid is whatever the
        // DB assigned at insert time and must survive an update unchanged
        // — bind it into `updated.uuid` for the full struct comparison.
        let updated_row = rows.iter().find(|(rid, _)| *rid == id).unwrap();
        assert!(looks_like_uuid_v4(&updated_row.1.uuid));
        updated.uuid = updated_row.1.uuid.clone();
        assert_eq!(updated_row.1, updated);
        // Sibling row untouched.
        let other_row = rows.iter().find(|(rid, _)| *rid == other_id).unwrap();
        assert_eq!(other_row.1.start_iso, "2026-04-27T11:00:00Z");
        assert_eq!(other_row.1.duration_secs, 300);
        assert_eq!(other_row.1.mode, SessionMode::Stopwatch);
        // Each row must carry its own distinct uuid.
        assert!(looks_like_uuid_v4(&other_row.1.uuid));
        assert_ne!(updated_row.1.uuid, other_row.1.uuid);
    }

    #[test]
    fn update_session_can_clear_label_and_notes() {
        // Optional fields go round-trip in both directions: a session
        // with a label/note can have them cleared by update.
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        let morning = db.find_label_by_name("Morning").unwrap().unwrap();
        let id = db.insert_session(&Session {
            start_iso: "2026-04-27T10:00:00Z".to_string(),
            duration_secs: 600,
            label_id: Some(morning),
            notes: Some("had a label".to_string()),
            mode: SessionMode::Countdown,
            uuid: String::new(),
        }).unwrap();
        db.update_session(id, &Session {
            start_iso: "2026-04-27T10:00:00Z".to_string(),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Countdown,
            uuid: String::new(),
        }).unwrap();
        let row = &db.list_sessions().unwrap()[0].1;
        assert_eq!(row.label_id, None);
        assert_eq!(row.notes, None);
    }

    #[test]
    fn update_session_unknown_id_is_noop() {
        // Updating a non-existent row is silent — matches SQLite's
        // UPDATE-by-id behaviour. The DB stays unchanged.
        let db = Database::open_in_memory().unwrap();
        let id = db.insert_session(&Session {
            start_iso: "2026-04-27T10:00:00Z".to_string(),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Countdown,
            uuid: String::new(),
        }).unwrap();
        db.update_session(id + 999, &Session {
            start_iso: "2099-01-01T00:00:00Z".to_string(),
            duration_secs: 9999,
            label_id: None,
            notes: None,
            mode: SessionMode::Countdown,
            uuid: String::new(),
        }).unwrap();
        // Original row is intact.
        let rows = db.list_sessions().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].1.duration_secs, 600);
        assert_eq!(rows[0].1.start_iso, "2026-04-27T10:00:00Z");
    }

    #[test]
    fn delete_session_removes_only_the_addressed_row() {
        // Delete addresses one row by id; siblings are untouched.
        let db = Database::open_in_memory().unwrap();
        let make = |start: &str| Session {
            start_iso: start.to_string(),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Countdown,
            uuid: String::new(),
        };
        let id1 = db.insert_session(&make("2026-04-27T10:00:00Z")).unwrap();
        let id2 = db.insert_session(&make("2026-04-27T11:00:00Z")).unwrap();
        let id3 = db.insert_session(&make("2026-04-27T12:00:00Z")).unwrap();

        db.delete_session(id2).unwrap();

        let surviving_ids: Vec<i64> =
            db.list_sessions().unwrap().into_iter().map(|(i, _)| i).collect();
        assert_eq!(surviving_ids, vec![id1, id3]);
        assert_eq!(db.count_sessions().unwrap(), 2);
    }

    #[test]
    fn delete_session_unknown_id_is_noop() {
        // Matches SQLite DELETE semantics: missing id is silent.
        let db = Database::open_in_memory().unwrap();
        let id = db.insert_session(&Session {
            start_iso: "2026-04-27T10:00:00Z".to_string(),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Countdown,
            uuid: String::new(),
        }).unwrap();
        db.delete_session(id + 999).unwrap();
        // Original row still there.
        assert_eq!(db.count_sessions().unwrap(), 1);
    }

    #[test]
    fn delete_session_does_not_remove_referenced_label() {
        // Labels survive their sessions — the FK is set-null on the
        // sessions side, not cascade-delete on the labels side.
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        let morning = db.find_label_by_name("Morning").unwrap().unwrap();
        let id = db.insert_session(&Session {
            start_iso: "2026-04-27T10:00:00Z".to_string(),
            duration_secs: 600,
            label_id: Some(morning),
            notes: None,
            mode: SessionMode::Countdown,
            uuid: String::new(),
        }).unwrap();

        db.delete_session(id).unwrap();

        // Label outlives the session.
        let names: Vec<String> =
            db.list_labels().unwrap().into_iter().map(|l| l.name).collect();
        assert_eq!(names, vec!["Morning"]);
        assert_eq!(db.count_labels().unwrap(), 1);
    }

    #[test]
    fn insert_session_with_unknown_label_id_is_rejected_by_fk() {
        // The labels.id ↔ sessions.label_id link is an enforced foreign key,
        // not just documentation. Inserting a session that points at a
        // non-existent label fails — the DB is the last line of defense
        // against UI bugs that pass through bad ids.
        let db = Database::open_in_memory().unwrap();
        // Sanity: the PRAGMA must be on for the FK clause to actually fire.
        let pragma: i64 = db.conn.query_row("PRAGMA foreign_keys", [], |r| r.get(0)).unwrap();
        assert_eq!(pragma, 1, "PRAGMA foreign_keys must be ON");

        let bad_id = 9999i64;
        let result = db.insert_session(&Session {
            start_iso: "2026-04-27T10:00:00Z".to_string(),
            duration_secs: 600,
            label_id: Some(bad_id),
            notes: None,
            mode: SessionMode::Countdown,
            uuid: String::new(),
        });
        assert!(result.is_err(), "expected FK violation, got {result:?}");
        // No row landed.
        assert_eq!(db.count_sessions().unwrap(), 0);
    }

    #[test]
    fn bulk_insert_sessions_inserts_every_row_and_returns_count() {
        // Bulk insert is the import-CSV path's transactional API: every
        // row in the slice goes in (or none on error — see rollback test).
        // Returns the count for "imported N sessions" toasts.
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        let morning = db.find_label_by_name("Morning").unwrap().unwrap();

        let to_insert = vec![
            Session {
                start_iso: "2026-04-27T10:00:00Z".to_string(),
                duration_secs: 600,
                label_id: Some(morning),
                notes: Some("first".to_string()),
                mode: SessionMode::Countdown,
                uuid: String::new(),
            },
            Session {
                start_iso: "2026-04-27T11:00:00Z".to_string(),
                duration_secs: 1200,
                label_id: None,
                notes: None,
                mode: SessionMode::Stopwatch,
                uuid: String::new(),
            },
            Session {
                start_iso: "2026-04-27T12:00:00Z".to_string(),
                duration_secs: 300,
                label_id: Some(morning),
                notes: None,
                mode: SessionMode::BoxBreath,
                uuid: String::new(),
            },
        ];

        let n = db.bulk_insert_sessions(&to_insert).unwrap();
        assert_eq!(n, 3);
        assert_eq!(db.count_sessions().unwrap(), 3);

        // Every row round-trips through the DB unchanged. The DB assigns
        // each row a fresh v4 uuid that the input doesn't carry — verify
        // each is well-formed, then graft it onto the expected value
        // before comparing the rest of the fields.
        let mut stored: Vec<Session> = db.list_sessions()
            .unwrap()
            .into_iter()
            .map(|(_, s)| s)
            .collect();
        let mut expected = to_insert.clone();
        for (got, want) in stored.iter().zip(expected.iter_mut()) {
            assert!(looks_like_uuid_v4(&got.uuid),
                "bulk-inserted row missing v4 uuid: {got:?}");
            want.uuid = got.uuid.clone();
        }
        // All uuids must also be distinct.
        let unique: std::collections::HashSet<_> =
            stored.iter().map(|s| s.uuid.clone()).collect();
        assert_eq!(unique.len(), stored.len(), "bulk insert must give unique uuids");
        // Strip nothing here: we've populated `expected.uuid` to match.
        let _ = stored.iter_mut(); // silence "doesn't need mut" if linter trips
        assert_eq!(stored, expected);
    }

    #[test]
    fn bulk_insert_sessions_empty_slice_is_zero_and_no_op() {
        // Empty input is not an error; the DB is unchanged.
        let db = Database::open_in_memory().unwrap();
        let n = db.bulk_insert_sessions(&[]).unwrap();
        assert_eq!(n, 0);
        assert_eq!(db.count_sessions().unwrap(), 0);
    }

    #[test]
    fn bulk_insert_sessions_rolls_back_on_constraint_violation() {
        // If any row in the batch violates a constraint (here: a foreign-key
        // pointing at a non-existent label), the WHOLE batch is reverted —
        // the caller never gets a half-imported DB.
        let db = Database::open_in_memory().unwrap();
        let pre_id = db.insert_session(&Session {
            start_iso: "2026-04-27T09:00:00Z".to_string(),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Countdown,
            uuid: String::new(),
        }).unwrap();
        assert_eq!(db.count_sessions().unwrap(), 1);

        let bad_label = 9999i64; // No label has this id.
        let batch = vec![
            Session {
                start_iso: "2026-04-27T10:00:00Z".to_string(),
                duration_secs: 600,
                label_id: None, // OK
                notes: None,
                mode: SessionMode::Countdown,
                uuid: String::new(),
            },
            Session {
                start_iso: "2026-04-27T11:00:00Z".to_string(),
                duration_secs: 600,
                label_id: Some(bad_label), // FK violation
                notes: None,
                mode: SessionMode::Countdown,
                uuid: String::new(),
            },
        ];
        let result = db.bulk_insert_sessions(&batch);
        assert!(result.is_err(), "expected FK violation, got {result:?}");

        // No rows from the failed batch landed; the pre-existing row is intact.
        assert_eq!(db.count_sessions().unwrap(), 1);
        let rows = db.list_sessions().unwrap();
        assert_eq!(rows[0].0, pre_id);
    }

    #[test]
    fn bulk_insert_sessions_is_atomic_with_no_partial_state_visible() {
        // Atomic-on-error: even after a failed bulk insert, count_sessions
        // and list_sessions agree on the pre-batch state. (This pins the
        // contract: "rolled back" means no observable side effect, not
        // just "rows aren't there".)
        let db = Database::open_in_memory().unwrap();
        let bad_label = 9999i64;
        let batch = vec![
            Session {
                start_iso: "2026-04-27T10:00:00Z".to_string(),
                duration_secs: 600,
                label_id: Some(bad_label), // fails immediately
                notes: None,
                mode: SessionMode::Countdown,
                uuid: String::new(),
            },
        ];
        let _ = db.bulk_insert_sessions(&batch);
        assert_eq!(db.count_sessions().unwrap(), 0);
        assert!(db.list_sessions().unwrap().is_empty());
    }

    #[test]
    fn delete_all_sessions_returns_count_and_clears_table() {
        // Wipe-all returns the row count so the caller can show "deleted N
        // sessions" toasts. Labels survive (this is a sessions-only nuke).
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        let morning = db.find_label_by_name("Morning").unwrap().unwrap();
        for i in 0..3 {
            db.insert_session(&Session {
                start_iso: format!("2026-04-2{i}T10:00:00Z"),
                duration_secs: 600,
                label_id: Some(morning),
                notes: None,
                mode: SessionMode::Countdown,
                uuid: String::new(),
            }).unwrap();
        }
        assert_eq!(db.count_sessions().unwrap(), 3);

        let removed = db.delete_all_sessions().unwrap();
        assert_eq!(removed, 3);
        assert_eq!(db.count_sessions().unwrap(), 0);
        assert!(db.list_sessions().unwrap().is_empty());

        // Labels untouched.
        let names: Vec<String> =
            db.list_labels().unwrap().into_iter().map(|l| l.name).collect();
        assert_eq!(names, vec!["Morning"]);
    }

    #[test]
    fn delete_all_sessions_on_empty_db_returns_zero() {
        // Idempotent: nothing to delete is not an error.
        let db = Database::open_in_memory().unwrap();
        assert_eq!(db.delete_all_sessions().unwrap(), 0);
        assert_eq!(db.count_sessions().unwrap(), 0);
    }

    #[test]
    fn list_sessions_for_label_returns_id_per_row() {
        // Filtered list must also carry ids — same contract.
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        let morning = db.find_label_by_name("Morning").unwrap().unwrap();
        let mut labeled = Session {
            start_iso: "2026-04-27T10:00:00Z".to_string(),
            duration_secs: 600,
            label_id: Some(morning),
            notes: None,
            mode: SessionMode::Countdown,
            uuid: String::new(),
        };
        let id = db.insert_session(&labeled).unwrap();
        // Insert a second, unlabeled session — must not appear.
        db.insert_session(&Session {
            start_iso: "2026-04-27T11:00:00Z".to_string(),
            duration_secs: 300,
            label_id: None,
            notes: None,
            mode: SessionMode::Countdown,
            uuid: String::new(),
        }).unwrap();
        let rows = db.list_sessions_for_label(morning).unwrap();
        assert_eq!(rows.len(), 1, "only the labeled session must be returned");
        assert!(looks_like_uuid_v4(&rows[0].1.uuid));
        labeled.uuid = rows[0].1.uuid.clone();
        assert_eq!(rows, vec![(id, labeled)]);
    }

    #[test]
    fn total_minutes_sums_durations_across_sessions() {
        let db = Database::open_in_memory().unwrap();
        let session_with_dur = |dur_secs| Session {
            start_iso: "2026-04-27T10:00:00Z".to_string(),
            duration_secs: dur_secs,
            label_id: None,
            notes: None,
            mode: SessionMode::Countdown,
            uuid: String::new(),
        };
        db.insert_session(&session_with_dur(600)).unwrap(); // 10 min
        db.insert_session(&session_with_dur(900)).unwrap(); // 15 min
        assert_eq!(db.total_minutes().unwrap(), 25);
    }

    #[test]
    fn total_minutes_is_zero_for_empty_db() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(db.total_minutes().unwrap(), 0);
    }

    #[test]
    fn total_minutes_by_label_groups_per_label() {
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Evening").unwrap();
        db.insert_label("Morning").unwrap();
        let evening = db.find_label_by_name("Evening").unwrap();
        let morning = db.find_label_by_name("Morning").unwrap();
        // Morning: 600 + 1200 = 1800s = 30m
        db.insert_session(&Session {
            duration_secs: 600,
            label_id: morning,
            ..session_on("2026-04-27")
        })
        .unwrap();
        db.insert_session(&Session {
            duration_secs: 1200,
            label_id: morning,
            ..session_on("2026-04-26")
        })
        .unwrap();
        // Evening: 300s = 5m
        db.insert_session(&Session {
            duration_secs: 300,
            label_id: evening,
            ..session_on("2026-04-27")
        })
        .unwrap();
        // SQLite default ORDER BY name puts ASCII "Evening" before "Morning".
        assert_eq!(
            db.total_minutes_by_label().unwrap(),
            vec![
                (Some("Evening".to_string()), 5),
                (Some("Morning".to_string()), 30),
            ]
        );
    }

    #[test]
    fn total_minutes_by_label_includes_unlabeled_as_none() {
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        let morning = db.find_label_by_name("Morning").unwrap();
        db.insert_session(&Session {
            duration_secs: 600,
            label_id: morning,
            ..session_on("2026-04-27")
        })
        .unwrap();
        db.insert_session(&Session {
            duration_secs: 300,
            label_id: None,
            ..session_on("2026-04-27")
        })
        .unwrap();
        // SQLite ORDER BY ASC sorts NULL first.
        assert_eq!(
            db.total_minutes_by_label().unwrap(),
            vec![(None, 5), (Some("Morning".to_string()), 10)]
        );
    }

    #[test]
    fn total_minutes_by_label_is_empty_for_empty_db() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(db.total_minutes_by_label().unwrap(), vec![]);
    }

    #[test]
    fn count_sessions_by_label_groups_per_label() {
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        let morning = db.find_label_by_name("Morning").unwrap();
        db.insert_session(&Session {
            label_id: morning,
            ..session_on("2026-04-27")
        })
        .unwrap();
        db.insert_session(&Session {
            label_id: morning,
            ..session_on("2026-04-26")
        })
        .unwrap();
        db.insert_session(&Session {
            label_id: None,
            ..session_on("2026-04-25")
        })
        .unwrap();
        assert_eq!(
            db.count_sessions_by_label().unwrap(),
            vec![(None, 1), (Some("Morning".to_string()), 2)]
        );
    }

    fn date(y: i32, m: u32, d: u32) -> chrono::NaiveDate {
        chrono::NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn streak_is_zero_for_empty_db() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(db.get_streak(date(2026, 4, 27)).unwrap(), 0);
    }

    fn session_on(day: &str) -> Session {
        Session {
            start_iso: format!("{day}T10:00:00Z"),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Countdown,
            uuid: String::new(),
        }
    }

    #[test]
    fn streak_is_one_with_single_session_today() {
        let db = Database::open_in_memory().unwrap();
        db.insert_session(&session_on("2026-04-27")).unwrap();
        assert_eq!(db.get_streak(date(2026, 4, 27)).unwrap(), 1);
    }

    #[test]
    fn streak_counts_consecutive_days_back_from_today() {
        let db = Database::open_in_memory().unwrap();
        db.insert_session(&session_on("2026-04-27")).unwrap();
        db.insert_session(&session_on("2026-04-26")).unwrap();
        db.insert_session(&session_on("2026-04-25")).unwrap();
        assert_eq!(db.get_streak(date(2026, 4, 27)).unwrap(), 3);
    }

    #[test]
    fn streak_breaks_at_first_gap() {
        let db = Database::open_in_memory().unwrap();
        db.insert_session(&session_on("2026-04-27")).unwrap();
        // gap on 2026-04-26
        db.insert_session(&session_on("2026-04-25")).unwrap();
        db.insert_session(&session_on("2026-04-24")).unwrap();
        assert_eq!(db.get_streak(date(2026, 4, 27)).unwrap(), 1);
    }

    #[test]
    fn streak_includes_yesterday_when_no_session_today() {
        // Forgiving variant: streak still alive if you meditated yesterday.
        let db = Database::open_in_memory().unwrap();
        db.insert_session(&session_on("2026-04-26")).unwrap();
        db.insert_session(&session_on("2026-04-25")).unwrap();
        assert_eq!(db.get_streak(date(2026, 4, 27)).unwrap(), 2);
    }

    #[test]
    fn streak_is_zero_when_most_recent_session_is_older_than_yesterday() {
        let db = Database::open_in_memory().unwrap();
        db.insert_session(&session_on("2026-04-24")).unwrap();
        assert_eq!(db.get_streak(date(2026, 4, 27)).unwrap(), 0);
    }

    #[test]
    fn streak_counts_each_day_once_even_with_multiple_sessions() {
        let db = Database::open_in_memory().unwrap();
        db.insert_session(&Session {
            start_iso: "2026-04-27T08:00:00Z".to_string(),
            ..session_on("2026-04-27")
        })
        .unwrap();
        db.insert_session(&Session {
            start_iso: "2026-04-27T19:00:00Z".to_string(),
            ..session_on("2026-04-27")
        })
        .unwrap();
        assert_eq!(db.get_streak(date(2026, 4, 27)).unwrap(), 1);
    }

    #[test]
    fn best_streak_is_zero_for_empty_db() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(db.get_best_streak().unwrap(), 0);
    }

    #[test]
    fn streak_for_label_only_counts_sessions_with_that_label() {
        let db = Database::open_in_memory().unwrap();
        let today = date(2026, 4, 27);
        db.insert_label("Morning").unwrap();
        db.insert_label("Evening").unwrap();
        let morning = db.find_label_by_name("Morning").unwrap().unwrap();
        let evening = db.find_label_by_name("Evening").unwrap().unwrap();
        // Today: Morning + Evening sessions.
        db.insert_session(&Session {
            label_id: Some(morning),
            ..session_on("2026-04-27")
        })
        .unwrap();
        db.insert_session(&Session {
            label_id: Some(evening),
            ..session_on("2026-04-27")
        })
        .unwrap();
        // Yesterday: Morning only.
        db.insert_session(&Session {
            label_id: Some(morning),
            ..session_on("2026-04-26")
        })
        .unwrap();
        // 2 days ago: Evening only.
        db.insert_session(&Session {
            label_id: Some(evening),
            ..session_on("2026-04-25")
        })
        .unwrap();
        // Morning streak: today + yesterday = 2 (gap on day-2).
        assert_eq!(db.get_streak_for_label(today, morning).unwrap(), 2);
        // Evening streak: today only (gap on yesterday).
        assert_eq!(db.get_streak_for_label(today, evening).unwrap(), 1);
        // Overall streak (no filter): today + yesterday + day-2 = 3.
        assert_eq!(db.get_streak(today).unwrap(), 3);
    }

    #[test]
    fn streak_and_best_streak_diverge_when_current_run_is_shorter() {
        // Mirrors `streak_gap_separates_current_from_best` from the existing app:
        // an old 6-day run, a gap, then a recent 3-day run ending today.
        let db = Database::open_in_memory().unwrap();
        let today = date(2026, 4, 27);
        // Old run: 30..25 days ago (6 days).
        for offset in 25..=30 {
            let day = today - chrono::Duration::days(offset);
            db.insert_session(&session_on(&day.format("%Y-%m-%d").to_string()))
                .unwrap();
        }
        // Current run: 0..2 days ago (3 days).
        for offset in 0..=2 {
            let day = today - chrono::Duration::days(offset);
            db.insert_session(&session_on(&day.format("%Y-%m-%d").to_string()))
                .unwrap();
        }
        assert_eq!(db.get_streak(today).unwrap(), 3, "current streak");
        assert_eq!(db.get_best_streak().unwrap(), 6, "best historical streak");
    }

    #[test]
    fn best_streak_for_label_only_counts_sessions_with_that_label() {
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        db.insert_label("Evening").unwrap();
        let morning = db.find_label_by_name("Morning").unwrap().unwrap();
        let evening = db.find_label_by_name("Evening").unwrap().unwrap();
        // Morning has a 3-day run.
        for d in ["2026-04-25", "2026-04-26", "2026-04-27"] {
            db.insert_session(&Session {
                label_id: Some(morning),
                ..session_on(d)
            })
            .unwrap();
        }
        // Evening has a 5-day run (longer overall, but for Morning it's irrelevant).
        for d in [
            "2026-04-01", "2026-04-02", "2026-04-03", "2026-04-04", "2026-04-05",
        ] {
            db.insert_session(&Session {
                label_id: Some(evening),
                ..session_on(d)
            })
            .unwrap();
        }
        assert_eq!(db.get_best_streak_for_label(morning).unwrap(), 3);
        assert_eq!(db.get_best_streak_for_label(evening).unwrap(), 5);
        // Overall best ignores label and finds the longest run anywhere.
        assert_eq!(db.get_best_streak().unwrap(), 5);
    }

    #[test]
    fn best_streak_finds_longest_run_across_history() {
        let db = Database::open_in_memory().unwrap();
        // Run of 2: Apr 1-2
        db.insert_session(&session_on("2026-04-01")).unwrap();
        db.insert_session(&session_on("2026-04-02")).unwrap();
        // Run of 4: Apr 10-13 (the best)
        db.insert_session(&session_on("2026-04-10")).unwrap();
        db.insert_session(&session_on("2026-04-11")).unwrap();
        db.insert_session(&session_on("2026-04-12")).unwrap();
        db.insert_session(&session_on("2026-04-13")).unwrap();
        // Run of 1: Apr 20
        db.insert_session(&session_on("2026-04-20")).unwrap();
        assert_eq!(db.get_best_streak().unwrap(), 4);
    }

    #[test]
    fn daily_totals_groups_durations_by_day() {
        let db = Database::open_in_memory().unwrap();
        // Two sessions same day → summed.
        db.insert_session(&Session {
            duration_secs: 600,
            ..session_on("2026-04-26")
        })
        .unwrap();
        db.insert_session(&Session {
            duration_secs: 300,
            ..session_on("2026-04-26")
        })
        .unwrap();
        // Different day, distinct entry.
        db.insert_session(&Session {
            duration_secs: 1200,
            ..session_on("2026-04-27")
        })
        .unwrap();
        assert_eq!(
            db.get_daily_totals().unwrap(),
            vec![(date(2026, 4, 26), 900), (date(2026, 4, 27), 1200)]
        );
    }

    #[test]
    fn daily_totals_is_empty_for_empty_db() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(db.get_daily_totals().unwrap(), vec![]);
    }

    #[test]
    fn daily_totals_for_label_filters_per_day() {
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        let morning = db.find_label_by_name("Morning").unwrap().unwrap();
        // Morning on Apr 26 (600s) and Apr 27 (1200s).
        db.insert_session(&Session {
            duration_secs: 600,
            label_id: Some(morning),
            ..session_on("2026-04-26")
        })
        .unwrap();
        db.insert_session(&Session {
            duration_secs: 1200,
            label_id: Some(morning),
            ..session_on("2026-04-27")
        })
        .unwrap();
        // Unlabeled on Apr 27 — must NOT show up in Morning's totals.
        db.insert_session(&Session {
            duration_secs: 9999,
            label_id: None,
            ..session_on("2026-04-27")
        })
        .unwrap();
        assert_eq!(
            db.get_daily_totals_for_label(morning).unwrap(),
            vec![(date(2026, 4, 26), 600), (date(2026, 4, 27), 1200)]
        );
    }

    #[test]
    fn open_creates_database_at_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.db");
        let db = Database::open(&path).unwrap();
        db.insert_label("Morning").unwrap();
        assert_eq!(db.count_labels().unwrap(), 1);
    }

    #[test]
    fn inserting_session_with_unknown_label_id_is_rejected() {
        let db = Database::open_in_memory().unwrap();
        let result = db.insert_session(&Session {
            start_iso: "2026-04-27T10:00:00Z".to_string(),
            duration_secs: 600,
            label_id: Some(999), // does not exist
            notes: None,
            mode: SessionMode::Countdown,
            uuid: String::new(),
        });
        assert!(result.is_err(), "FK constraint should reject unknown label");
    }

    #[test]
    fn data_persists_across_reopens() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.db");
        {
            let db = Database::open(&path).unwrap();
            db.insert_label("Morning").unwrap();
            db.insert_session(&session_on("2026-04-27")).unwrap();
        }
        let db = Database::open(&path).unwrap();
        let names: Vec<String> =
            db.list_labels().unwrap().into_iter().map(|l| l.name).collect();
        assert_eq!(names, vec!["Morning"]);
        assert_eq!(db.count_sessions().unwrap(), 1);
    }

    #[test]
    fn running_average_is_zero_for_empty_db() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(
            db.get_running_average_secs(date(2026, 4, 27), 7).unwrap(),
            0.0
        );
    }

    #[test]
    fn running_average_handles_zero_days_without_divide_by_zero() {
        let db = Database::open_in_memory().unwrap();
        db.insert_session(&session_on("2026-04-27")).unwrap();
        assert_eq!(
            db.get_running_average_secs(date(2026, 4, 27), 0).unwrap(),
            0.0
        );
    }

    #[test]
    fn running_average_divides_total_by_window_days() {
        let db = Database::open_in_memory().unwrap();
        // 600s today, window of 1 day → average = 600.
        db.insert_session(&Session {
            duration_secs: 600,
            ..session_on("2026-04-27")
        })
        .unwrap();
        assert_eq!(
            db.get_running_average_secs(date(2026, 4, 27), 1).unwrap(),
            600.0
        );
        // Same data, window of 2 days → average = 300.
        assert_eq!(
            db.get_running_average_secs(date(2026, 4, 27), 2).unwrap(),
            300.0
        );
    }

    #[test]
    fn running_average_excludes_sessions_outside_window() {
        let db = Database::open_in_memory().unwrap();
        // Today: 600s — inside any window.
        db.insert_session(&Session {
            duration_secs: 600,
            ..session_on("2026-04-27")
        })
        .unwrap();
        // 10 days ago: 1200s — outside a 7-day window.
        db.insert_session(&Session {
            duration_secs: 1200,
            ..session_on("2026-04-17")
        })
        .unwrap();
        // Window of 7 days = today and 6 prior days; only today's 600s counts.
        let avg = db.get_running_average_secs(date(2026, 4, 27), 7).unwrap();
        assert!((avg - (600.0 / 7.0)).abs() < 1e-9, "got {avg}");
    }

    #[test]
    fn median_duration_is_zero_for_empty_db() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(db.get_median_duration_secs().unwrap(), 0);
    }

    #[test]
    fn median_duration_returns_middle_for_odd_count() {
        let db = Database::open_in_memory().unwrap();
        for d in [300u32, 600, 900, 1200, 1500] {
            db.insert_session(&Session {
                duration_secs: d,
                ..session_on("2026-04-27")
            })
            .unwrap();
        }
        assert_eq!(db.get_median_duration_secs().unwrap(), 900);
    }

    #[test]
    fn median_duration_uses_lower_median_for_even_count() {
        let db = Database::open_in_memory().unwrap();
        // Sorted: [300, 600, 900, 1200]. Lower median = 600.
        for d in [600u32, 1200, 300, 900] {
            db.insert_session(&Session {
                duration_secs: d,
                ..session_on("2026-04-27")
            })
            .unwrap();
        }
        assert_eq!(db.get_median_duration_secs().unwrap(), 600);
    }

    #[test]
    fn csv_round_trips_sessions_with_labels() {
        let src = Database::open_in_memory().unwrap();
        src.insert_label("Morning").unwrap();
        let morning_id = src.find_label_by_name("Morning").unwrap();
        src.insert_session(&Session {
            start_iso: "2026-04-27T10:00:00Z".to_string(),
            duration_secs: 600,
            label_id: morning_id,
            notes: Some("clear, focused".to_string()), // comma forces CSV quoting
            mode: SessionMode::Countdown,
            uuid: String::new(),
        })
        .unwrap();
        src.insert_session(&Session {
            start_iso: "2026-04-27T19:00:00Z".to_string(),
            duration_secs: 1200,
            label_id: None,
            notes: None,
            mode: SessionMode::BoxBreath,
            uuid: String::new(),
        })
        .unwrap();

        let mut buf = Vec::new();
        src.export_sessions_csv(&mut buf).unwrap();

        let dst = Database::open_in_memory().unwrap();
        let imported = dst.import_sessions_csv(&buf[..]).unwrap();
        assert_eq!(imported, 2);

        // Label was created on import.
        let dst_names: Vec<String> =
            dst.list_labels().unwrap().into_iter().map(|l| l.name).collect();
        assert_eq!(dst_names, vec!["Morning"]);
        let dst_morning_id = dst.find_label_by_name("Morning").unwrap();

        // CSV import generates fresh v4 uuids on the destination DB
        // (uuids aren't part of the CSV format). Verify each row carries
        // one, then bind it into the expected struct so the full
        // comparison below also covers the rest of the fields.
        let sessions = dst.list_sessions().unwrap();
        assert_eq!(sessions.len(), 2);
        assert!(looks_like_uuid_v4(&sessions[0].1.uuid));
        assert!(looks_like_uuid_v4(&sessions[1].1.uuid));
        assert_ne!(sessions[0].1.uuid, sessions[1].1.uuid);
        assert_eq!(
            sessions[0].1,
            Session {
                start_iso: "2026-04-27T10:00:00Z".to_string(),
                duration_secs: 600,
                label_id: dst_morning_id,
                notes: Some("clear, focused".to_string()),
                mode: SessionMode::Countdown,
                uuid: sessions[0].1.uuid.clone(),
            }
        );
        assert_eq!(
            sessions[1].1,
            Session {
                start_iso: "2026-04-27T19:00:00Z".to_string(),
                duration_secs: 1200,
                label_id: None,
                notes: None,
                mode: SessionMode::BoxBreath,
                uuid: sessions[1].1.uuid.clone(),
            }
        );
    }

    #[test]
    fn export_csv_writes_header_and_session_with_label_name() {
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        let label_id = db.find_label_by_name("Morning").unwrap();
        db.insert_session(&Session {
            start_iso: "2026-04-27T10:00:00Z".to_string(),
            duration_secs: 600,
            label_id,
            notes: Some("clear mind".to_string()),
            mode: SessionMode::Countdown,
            uuid: String::new(),
        })
        .unwrap();

        let mut buf = Vec::new();
        db.export_sessions_csv(&mut buf).unwrap();
        let csv = String::from_utf8(buf).unwrap();

        assert!(
            csv.contains("start_iso,duration_secs,label,notes,mode"),
            "missing header in:\n{csv}"
        );
        assert!(csv.contains("2026-04-27T10:00:00Z"));
        assert!(csv.contains("Morning"));
        assert!(csv.contains("clear mind"));
        assert!(csv.contains("countdown"));
    }

    // ── UUIDs on sessions and labels (Nextcloud-Sync phase A1) ───────────────
    //
    // Every session and label row must carry a stable cross-device UUID.
    // The DB generates it at insert time — the value the caller puts in
    // the struct's `uuid` field is ignored. Reads round-trip the stored
    // UUID into the returned struct so the rest of the app (including
    // the future event log) can address rows by it.

    fn looks_like_uuid_v4(s: &str) -> bool {
        // 8-4-4-4-12 hex with v4 marker and RFC 4122 variant. Cheap shape
        // check — we don't need a full parser, just confidence that
        // generation actually used `Uuid::new_v4()` rather than (say) a
        // timestamp string or a counter.
        if s.len() != 36 { return false; }
        let bytes = s.as_bytes();
        if bytes[8] != b'-' || bytes[13] != b'-' || bytes[18] != b'-' || bytes[23] != b'-' {
            return false;
        }
        if bytes[14] != b'4' { return false; }                 // version
        if !matches!(bytes[19], b'8' | b'9' | b'a' | b'b') {  // variant
            return false;
        }
        bytes.iter().enumerate().all(|(i, c)| {
            matches!(i, 8 | 13 | 18 | 23) || c.is_ascii_hexdigit()
        })
    }

    #[test]
    fn inserted_session_has_a_uuid_in_query_results() {
        let db = Database::open_in_memory().unwrap();
        db.insert_session(&Session {
            start_iso: "2026-04-27T10:00:00".to_string(),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Countdown,
                        uuid: String::new(),  // ignored — DB assigns
        })
        .unwrap();
        let rows = db.list_sessions().unwrap();
        assert_eq!(rows.len(), 1);
        assert!(!rows[0].1.uuid.is_empty(), "uuid must be populated on read");
    }

    #[test]
    fn two_inserted_sessions_get_distinct_uuids() {
        let db = Database::open_in_memory().unwrap();
        for i in 0..2 {
            db.insert_session(&Session {
                start_iso: format!("2026-04-2{}T10:00:00", 7 + i),
                duration_secs: 600,
                label_id: None,
                notes: None,
                mode: SessionMode::Countdown,
                                uuid: String::new(),
            })
            .unwrap();
        }
        let rows = db.list_sessions().unwrap();
        assert_eq!(rows.len(), 2);
        assert_ne!(rows[0].1.uuid, rows[1].1.uuid,
            "two inserts must produce distinct uuids");
    }

    #[test]
    fn inserted_session_uuid_is_v4_shaped() {
        let db = Database::open_in_memory().unwrap();
        db.insert_session(&Session {
            start_iso: "2026-04-27T10:00:00".to_string(),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Countdown,
                        uuid: String::new(),
        })
        .unwrap();
        let uuid = &db.list_sessions().unwrap()[0].1.uuid;
        assert!(looks_like_uuid_v4(uuid),
            "session uuid `{uuid}` doesn't match v4 shape");
    }

    #[test]
    fn caller_supplied_session_uuid_is_ignored_in_favour_of_a_fresh_one() {
        // Documents that uuid is DB-assigned, not caller-controlled.
        // Belt-and-braces: if a caller accidentally reuses a uuid string
        // the DB still produces fresh, unique values — no collision risk.
        let db = Database::open_in_memory().unwrap();
        let bogus = "00000000-0000-4000-8000-000000000000".to_string();
        for i in 0..2 {
            db.insert_session(&Session {
                start_iso: format!("2026-04-2{}T10:00:00", 7 + i),
                duration_secs: 600,
                label_id: None,
                notes: None,
                mode: SessionMode::Countdown,
                                uuid: bogus.clone(),
            })
            .unwrap();
        }
        let rows = db.list_sessions().unwrap();
        assert_ne!(rows[0].1.uuid, bogus, "DB must override caller's uuid");
        assert_ne!(rows[1].1.uuid, bogus, "DB must override caller's uuid");
        assert_ne!(rows[0].1.uuid, rows[1].1.uuid);
    }

    #[test]
    fn inserted_label_has_a_uuid() {
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        let labels = db.list_labels().unwrap();
        assert_eq!(labels.len(), 1);
        assert!(!labels[0].uuid.is_empty(), "uuid must be populated on read");
    }

    #[test]
    fn two_inserted_labels_get_distinct_uuids() {
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        db.insert_label("Evening").unwrap();
        let labels = db.list_labels().unwrap();
        assert_eq!(labels.len(), 2);
        assert_ne!(labels[0].uuid, labels[1].uuid);
    }

    #[test]
    fn inserted_label_uuid_is_v4_shaped() {
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        let uuid = &db.list_labels().unwrap()[0].uuid;
        assert!(looks_like_uuid_v4(uuid),
            "label uuid `{uuid}` doesn't match v4 shape");
    }

    // ── Device identity (Nextcloud-Sync phase A2.1) ──────────────────────────
    //
    // Each device gets a stable UUID that survives across app restarts and
    // tags every locally-authored event. Generated lazily on first call to
    // `device_id()` so a fresh in-memory test DB doesn't pay the cost
    // unless something asks for it.

    #[test]
    fn device_id_is_a_v4_uuid() {
        let db = Database::open_in_memory().unwrap();
        let id = db.device_id().unwrap();
        assert!(looks_like_uuid_v4(&id),
            "device_id `{id}` doesn't match v4 shape");
    }

    #[test]
    fn device_id_is_stable_across_calls_within_one_process() {
        // Two calls in succession must agree — the second call must not
        // re-generate. Otherwise every event we author would be tagged
        // with a different device, defeating the conflict-resolution
        // rule that ties-break by device_id.
        let db = Database::open_in_memory().unwrap();
        let a = db.device_id().unwrap();
        let b = db.device_id().unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn device_id_is_stable_across_database_reopens() {
        // Persistence: closing the DB and reopening the same file must
        // yield the same device_id. This is the actual cross-restart
        // contract; the in-memory variant above only proves "same call,
        // same answer".
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("device_id.db");

        let id_first = {
            let db = Database::open(&path).unwrap();
            db.device_id().unwrap()
        };
        let id_second = {
            let db = Database::open(&path).unwrap();
            db.device_id().unwrap()
        };
        assert_eq!(id_first, id_second,
            "device_id must persist across DB reopens");
    }

    #[test]
    fn two_separate_databases_get_different_device_ids() {
        // Two fresh DBs simulate two devices on the same network. Their
        // device_ids must differ so events authored on each can be
        // distinguished by `device_id` in the conflict-resolution rules.
        let db_a = Database::open_in_memory().unwrap();
        let db_b = Database::open_in_memory().unwrap();
        assert_ne!(db_a.device_id().unwrap(), db_b.device_id().unwrap());
    }

    // ── Lamport clock (Nextcloud-Sync phase A2.2) ────────────────────────────
    //
    // Logical clock for event ordering: bumped on local writes, max-merged
    // with observed remote timestamps. Persisted in the single `device`
    // row so it survives restarts. Conflict resolution depends on a
    // monotonic counter — subtle bugs here cause silent data divergence.

    #[test]
    fn lamport_clock_is_zero_on_a_fresh_database() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(db.lamport_clock().unwrap(), 0);
    }

    #[test]
    fn lamport_clock_starts_at_zero_even_before_device_id_is_minted() {
        // Reading the clock must not implicitly require device_id() to
        // have been called. The single-row `device` table is shared
        // state — a query on an empty table returns the column default
        // (0), not an error.
        let db = Database::open_in_memory().unwrap();
        let _ = db.lamport_clock().unwrap();          // no panic
        assert_eq!(db.lamport_clock().unwrap(), 0);   // and idempotent
    }

    #[test]
    fn bump_lamport_clock_returns_post_increment_value() {
        // Caller-friendly contract: the returned value is the timestamp
        // to attach to the event we're about to author. So bump() yields
        // 1 on a fresh DB, then 2, then 3 — never 0.
        let db = Database::open_in_memory().unwrap();
        assert_eq!(db.bump_lamport_clock().unwrap(), 1);
        assert_eq!(db.bump_lamport_clock().unwrap(), 2);
        assert_eq!(db.bump_lamport_clock().unwrap(), 3);
    }

    #[test]
    fn bump_lamport_clock_persists_the_increment() {
        // After a bump, the *plain* read must reflect it — otherwise
        // `lamport_clock()` and `bump_lamport_clock()` disagree, and
        // observe_remote_lamport's max-merge logic breaks.
        let db = Database::open_in_memory().unwrap();
        assert_eq!(db.bump_lamport_clock().unwrap(), 1);
        assert_eq!(db.lamport_clock().unwrap(), 1);
    }

    #[test]
    fn bump_lamport_clock_persists_across_database_reopens() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lamport.db");

        let mid = {
            let db = Database::open(&path).unwrap();
            db.bump_lamport_clock().unwrap();
            db.bump_lamport_clock().unwrap()
        };
        assert_eq!(mid, 2);
        let after_reopen = {
            let db = Database::open(&path).unwrap();
            db.lamport_clock().unwrap()
        };
        assert_eq!(after_reopen, 2,
            "lamport_clock must survive process restart unchanged");
        let bumped = {
            let db = Database::open(&path).unwrap();
            db.bump_lamport_clock().unwrap()
        };
        assert_eq!(bumped, 3,
            "the next bump after restart continues from the persisted value");
    }

    #[test]
    fn observe_remote_lamport_advances_when_remote_is_ahead() {
        // The Lamport rule: on observing a remote ts, set local =
        // max(local, remote) + 1. When remote > local, this jumps the
        // clock forward — necessary so any event we author after
        // observing the remote will sort *after* it.
        let db = Database::open_in_memory().unwrap();
        let new_local = db.observe_remote_lamport(42).unwrap();
        assert_eq!(new_local, 43);
        assert_eq!(db.lamport_clock().unwrap(), 43);
    }

    #[test]
    fn observe_remote_lamport_keeps_advancing_when_local_is_already_ahead() {
        // Conversely, when local > remote, we use local+1 — local was
        // already ahead, so we must still produce a strictly larger
        // value to satisfy "every observation advances the clock".
        let db = Database::open_in_memory().unwrap();
        // Get local to 100.
        for _ in 0..100 { db.bump_lamport_clock().unwrap(); }
        let new_local = db.observe_remote_lamport(7).unwrap();
        assert_eq!(new_local, 101);
        assert_eq!(db.lamport_clock().unwrap(), 101);
    }

    #[test]
    fn observe_remote_lamport_treats_equal_as_max_plus_one() {
        // Tie case: max(5, 5) + 1 = 6. Documents that "max" really is
        // max, not "strictly greater" — this is what guarantees a total
        // order even when two devices independently land on the same ts.
        let db = Database::open_in_memory().unwrap();
        for _ in 0..5 { db.bump_lamport_clock().unwrap(); }
        let new_local = db.observe_remote_lamport(5).unwrap();
        assert_eq!(new_local, 6);
    }

    #[test]
    fn observe_remote_lamport_handles_zero() {
        // The very first observation of a fresh remote (ts=0) must still
        // bump the local clock past it — otherwise an event tagged 0
        // would be indistinguishable from never-set state.
        let db = Database::open_in_memory().unwrap();
        let new_local = db.observe_remote_lamport(0).unwrap();
        assert_eq!(new_local, 1);
    }

    // ── Event log: append + pending + mark_synced (A2.3) ─────────────────────
    //
    // The append-only event log is the single source of truth for all
    // mutations. `append_event` is idempotent on `event_uuid`: receiving
    // the same event twice (e.g. on retry, or from a peer that already
    // forwarded it) is a no-op rather than a constraint error escalated
    // to the caller. `pending_events` is the push-queue contract — sorted
    // by `lamport_ts` so peers see events in causal order.

    fn sample_event(seed: i64) -> Event {
        Event {
            event_uuid: format!("00000000-0000-4000-8000-{:012x}", seed),
            lamport_ts: seed,
            device_id: "00000000-0000-4000-8000-aaaaaaaaaaaa".to_string(),
            kind: "session_insert".to_string(),
            payload: format!("{{\"seed\":{seed}}}"),
        }
    }

    #[test]
    fn pending_events_is_empty_on_a_fresh_database() {
        let db = Database::open_in_memory().unwrap();
        assert!(db.pending_events().unwrap().is_empty());
    }

    #[test]
    fn append_event_then_read_back_via_pending_events() {
        let db = Database::open_in_memory().unwrap();
        let event = sample_event(7);
        db.append_event(&event).unwrap();
        let rows = db.pending_events().unwrap();
        assert_eq!(rows.len(), 1);
        let (_, got) = &rows[0];
        assert_eq!(got, &event,
            "appended event must round-trip every field unchanged");
    }

    #[test]
    fn append_event_returns_a_distinct_local_rowid_per_call() {
        // The local rowid is the cache key inside this device — distinct
        // from `event_uuid` (the cross-device identity). Two appends must
        // get two different rowids so callers can address them locally.
        let db = Database::open_in_memory().unwrap();
        let id_a = db.append_event(&sample_event(1)).unwrap();
        let id_b = db.append_event(&sample_event(2)).unwrap();
        assert_ne!(id_a, id_b);
    }

    #[test]
    fn append_event_with_duplicate_uuid_is_idempotent_no_op() {
        // `event_uuid` is UNIQUE — a second insert of the same uuid must
        // succeed silently and NOT create a second row. This makes
        // event delivery at-most-once on the local cache regardless of
        // how often the caller (or a sync retry) submits it.
        let db = Database::open_in_memory().unwrap();
        let event = sample_event(1);
        db.append_event(&event).unwrap();
        let res = db.append_event(&event);
        assert!(res.is_ok(),
            "duplicate-event_uuid append must be a silent no-op, got: {res:?}");
        assert_eq!(db.pending_events().unwrap().len(), 1,
            "duplicate append must not create a second row");
    }

    #[test]
    fn pending_events_orders_by_lamport_ts_ascending() {
        // Peers replay in lamport order to converge on a consistent
        // state. The push queue must hand events out in that same order
        // so a peer with a slow-then-fast connection still gets them
        // monotonically.
        let db = Database::open_in_memory().unwrap();
        // Insert out of order — ts 5, then 1, then 3.
        db.append_event(&sample_event(5)).unwrap();
        db.append_event(&sample_event(1)).unwrap();
        db.append_event(&sample_event(3)).unwrap();
        let timestamps: Vec<i64> = db.pending_events().unwrap()
            .iter().map(|(_, e)| e.lamport_ts).collect();
        assert_eq!(timestamps, vec![1, 3, 5]);
    }

    #[test]
    fn mark_event_synced_removes_it_from_pending_events() {
        let db = Database::open_in_memory().unwrap();
        let id_a = db.append_event(&sample_event(1)).unwrap();
        let _id_b = db.append_event(&sample_event(2)).unwrap();
        db.mark_event_synced(id_a).unwrap();
        let pending: Vec<i64> = db.pending_events().unwrap()
            .iter().map(|(_, e)| e.lamport_ts).collect();
        assert_eq!(pending, vec![2],
            "synced event must drop out of the pending list");
    }

    #[test]
    fn mark_event_synced_unknown_id_is_a_silent_no_op() {
        // Defensive: a stale id from a partial sync attempt must not
        // panic or surface an error. SQLite UPDATE on no-match is
        // already a no-op; the wrapper preserves that.
        let db = Database::open_in_memory().unwrap();
        db.append_event(&sample_event(1)).unwrap();
        let res = db.mark_event_synced(999);
        assert!(res.is_ok());
        assert_eq!(db.pending_events().unwrap().len(), 1,
            "the existing event must still be pending — nothing was marked");
    }

    #[test]
    fn pending_events_excludes_synced_rows() {
        // After every event has been synced, pending_events is empty
        // again. Documents the boundary case of "fully caught up".
        let db = Database::open_in_memory().unwrap();
        let id_a = db.append_event(&sample_event(1)).unwrap();
        let id_b = db.append_event(&sample_event(2)).unwrap();
        db.mark_event_synced(id_a).unwrap();
        db.mark_event_synced(id_b).unwrap();
        assert!(db.pending_events().unwrap().is_empty());
    }

    #[test]
    fn append_event_persists_across_database_reopens() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("events.db");
        let event = sample_event(42);
        {
            let db = Database::open(&path).unwrap();
            db.append_event(&event).unwrap();
        }
        let db = Database::open(&path).unwrap();
        let rows = db.pending_events().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(&rows[0].1, &event);
    }

    // ── sync_state KV (A2.4) ─────────────────────────────────────────────────
    //
    // Holds sync-loop bookkeeping — server URL, last-pull cursor, last
    // successful sync timestamp, etc. Sensitive values (app password)
    // live in libsecret/Keystore, not here. Mirrors the existing
    // `settings` table in shape but is a separate namespace so a UI
    // export of sync diagnostics doesn't have to filter prefs out, and
    // vice-versa.

    #[test]
    fn get_sync_state_returns_default_on_a_fresh_database() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(db.get_sync_state("server_url", "fallback").unwrap(),
                   "fallback");
    }

    #[test]
    fn get_sync_state_returns_default_for_unknown_key_after_other_keys_set() {
        // Defensive: setting key A must not affect get on key B.
        let db = Database::open_in_memory().unwrap();
        db.set_sync_state("server_url", "https://nc.example").unwrap();
        assert_eq!(db.get_sync_state("missing", "fallback").unwrap(),
                   "fallback");
    }

    #[test]
    fn set_then_get_sync_state_round_trips_the_value() {
        let db = Database::open_in_memory().unwrap();
        db.set_sync_state("server_url", "https://nc.example").unwrap();
        assert_eq!(db.get_sync_state("server_url", "fallback").unwrap(),
                   "https://nc.example");
    }

    #[test]
    fn set_sync_state_overwrites_an_existing_value() {
        // Upsert semantics — same as `set_setting`. A second `set` must
        // replace, not silently no-op.
        let db = Database::open_in_memory().unwrap();
        db.set_sync_state("interval_seconds", "1800").unwrap();
        db.set_sync_state("interval_seconds", "300").unwrap();
        assert_eq!(db.get_sync_state("interval_seconds", "0").unwrap(),
                   "300");
    }

    #[test]
    fn sync_state_persists_across_database_reopens() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sync_state.db");
        {
            let db = Database::open(&path).unwrap();
            db.set_sync_state("server_url", "https://nc.example").unwrap();
        }
        let db = Database::open(&path).unwrap();
        assert_eq!(db.get_sync_state("server_url", "x").unwrap(),
                   "https://nc.example");
    }

    #[test]
    fn sync_state_and_settings_are_separate_namespaces() {
        // Same key in both tables must NOT collide — they're conceptually
        // independent stores. Pinning this makes future "let's just merge
        // them" refactors visible in CI.
        let db = Database::open_in_memory().unwrap();
        db.set_setting("foo", "from-settings").unwrap();
        db.set_sync_state("foo", "from-sync-state").unwrap();
        assert_eq!(db.get_setting("foo", "x").unwrap(), "from-settings");
        assert_eq!(db.get_sync_state("foo", "x").unwrap(), "from-sync-state");
    }
}
