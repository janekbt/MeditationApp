use rusqlite::{Connection, OptionalExtension, Result, params};
use std::path::Path;

// ── Models ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionMode {
    Countdown,
    Stopwatch,
}

impl SessionMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            SessionMode::Countdown => "countdown",
            SessionMode::Stopwatch => "stopwatch",
        }
    }

    fn from_str(s: &str) -> Self {
        match s {
            "stopwatch" => SessionMode::Stopwatch,
            _ => SessionMode::Countdown,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Label {
    pub id: i64,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct Session {
    pub id: i64,
    /// Unix timestamp (seconds since epoch) of when the session started.
    pub start_time: i64,
    pub duration_secs: i64,
    pub mode: SessionMode,
    pub label_id: Option<i64>,
    pub note: Option<String>,
}

/// Parameters for creating or updating a session.
pub struct SessionData {
    pub start_time: i64,
    pub duration_secs: i64,
    pub mode: SessionMode,
    pub label_id: Option<i64>,
    pub note: Option<String>,
}

/// Filter options for listing sessions.
#[derive(Default)]
pub struct SessionFilter {
    /// If set, only return sessions with this label.
    pub label_id: Option<i64>,
    /// If true, only return sessions that have a non-empty note.
    pub only_with_notes: bool,
    /// Pagination — fetch at most this many rows (None = no limit).
    pub limit: Option<u32>,
    /// Pagination — skip this many rows (None = start at 0).
    pub offset: Option<u32>,
}

// ── Database ──────────────────────────────────────────────────────────────────

pub struct Database {
    conn: Connection,
}

impl std::fmt::Debug for Database {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Database").finish_non_exhaustive()
    }
}

impl Database {
    /// Open (or create) the database at `path`, running any pending migrations.
    pub fn open(path: &Path) -> Result<Self> {
        // Create parent directories if they don't exist.
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        }

        let conn = Connection::open(path)?;

        // Enable WAL mode for better concurrent read performance.
        // Enable foreign key enforcement so ON DELETE SET NULL cascades work.
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;

        let db = Self { conn };
        db.migrate()?;
        Ok(db)
    }

    // ── Migrations ────────────────────────────────────────────────────────────

    fn migrate(&self) -> Result<()> {
        // Base schema — idempotent, runs on every startup.
        // labels.name has NO UNIQUE constraint; uniqueness at the DB level
        // was too restrictive (silent failures on "Add label" after renaming).
        self.conn.execute_batch("
            CREATE TABLE IF NOT EXISTS schema_migrations (
                version INTEGER PRIMARY KEY
            );

            CREATE TABLE IF NOT EXISTS labels (
                id   INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS sessions (
                id            INTEGER PRIMARY KEY AUTOINCREMENT,
                start_time    INTEGER NOT NULL,
                duration_secs INTEGER NOT NULL,
                mode          TEXT    NOT NULL DEFAULT 'countdown',
                label_id      INTEGER REFERENCES labels(id) ON DELETE SET NULL,
                note          TEXT
            );

            CREATE INDEX IF NOT EXISTS idx_sessions_start_time
                ON sessions (start_time);

            CREATE TABLE IF NOT EXISTS settings (
                key   TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
        ")?;

        // Migration 1: drop the UNIQUE constraint on labels.name that the
        // initial schema included.  SQLite requires recreating the table.
        let already_done: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version = 1)",
            [],
            |row| row.get(0),
        )?;
        if !already_done {
            self.conn.execute_batch("
                BEGIN;
                CREATE TABLE labels_new (
                    id   INTEGER PRIMARY KEY AUTOINCREMENT,
                    name TEXT NOT NULL
                );
                INSERT INTO labels_new SELECT id, name FROM labels;
                DROP TABLE labels;
                ALTER TABLE labels_new RENAME TO labels;
                INSERT INTO schema_migrations (version) VALUES (1);
                COMMIT;
            ")?;
        }

        // Migration 2: index on sessions.label_id for filtered log queries.
        let already_done: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version = 2)",
            [],
            |row| row.get(0),
        )?;
        if !already_done {
            self.conn.execute_batch("
                BEGIN;
                CREATE INDEX IF NOT EXISTS idx_sessions_label_id
                    ON sessions (label_id);
                INSERT INTO schema_migrations (version) VALUES (2);
                COMMIT;
            ")?;
        }

        Ok(())
    }

    // ── Labels ────────────────────────────────────────────────────────────────

    pub fn create_label(&self, base_name: &str) -> Result<Label> {
        // Find a name that isn't already in use: "New label", "New label 2", …
        let name = self.unique_label_name(base_name)?;
        self.conn.execute("INSERT INTO labels (name) VALUES (?1)", params![name])?;
        let id = self.conn.last_insert_rowid();
        Ok(Label { id, name })
    }

    fn unique_label_name(&self, base: &str) -> Result<String> {
        let exists = |n: &str| -> Result<bool> {
            self.conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM labels WHERE name = ?1)",
                params![n],
                |row| row.get(0),
            )
        };
        if !exists(base)? {
            return Ok(base.to_owned());
        }
        let mut i = 2u32;
        loop {
            let candidate = format!("{base} {i}");
            if !exists(&candidate)? {
                return Ok(candidate);
            }
            i += 1;
        }
    }

    pub fn list_labels(&self) -> Result<Vec<Label>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, name FROM labels ORDER BY name COLLATE NOCASE"
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(Label { id: row.get(0)?, name: row.get(1)? })
        })?;
        rows.collect()
    }

    /// Returns true if any label other than `except_id` already uses `name`.
    pub fn is_label_name_taken(&self, name: &str, except_id: i64) -> Result<bool> {
        self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM labels WHERE name = ?1 AND id != ?2)",
            params![name, except_id],
            |row| row.get(0),
        )
    }

    pub fn update_label(&self, id: i64, name: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE labels SET name = ?1 WHERE id = ?2",
            params![name, id],
        )?;
        Ok(())
    }

    pub fn label_session_count(&self, id: i64) -> Result<i64> {
        self.conn.query_row(
            "SELECT COUNT(*) FROM sessions WHERE label_id = ?1",
            params![id],
            |row| row.get(0),
        )
    }

    pub fn delete_label(&self, id: i64) -> Result<()> {
        // Sessions referencing this label will have label_id set to NULL (ON DELETE SET NULL).
        self.conn.execute("DELETE FROM labels WHERE id = ?1", params![id])?;
        Ok(())
    }

    // ── Sessions ──────────────────────────────────────────────────────────────

    pub fn create_session(&self, data: &SessionData) -> Result<Session> {
        self.conn.execute(
            "INSERT INTO sessions (start_time, duration_secs, mode, label_id, note)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                data.start_time,
                data.duration_secs,
                data.mode.as_str(),
                data.label_id,
                data.note,
            ],
        )?;
        let id = self.conn.last_insert_rowid();
        Ok(Session {
            id,
            start_time:    data.start_time,
            duration_secs: data.duration_secs,
            mode:          data.mode.clone(),
            label_id:      data.label_id,
            note:          data.note.clone(),
        })
    }

    /// Insert many sessions inside a single transaction — orders of magnitude
    /// faster than calling `create_session` in a loop. Returns the number of
    /// rows inserted. Transaction is rolled back on error.
    ///
    /// Uses `unchecked_transaction` so the caller can hold a shared `&Database`
    /// (matching `with_db`'s closure signature); safe here because the app is
    /// single-threaded and we don't nest transactions.
    pub fn bulk_insert_sessions(&self, sessions: &[SessionData]) -> Result<usize> {
        let tx = self.conn.unchecked_transaction()?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO sessions (start_time, duration_secs, mode, label_id, note)
                 VALUES (?1, ?2, ?3, ?4, ?5)"
            )?;
            for data in sessions {
                stmt.execute(params![
                    data.start_time,
                    data.duration_secs,
                    data.mode.as_str(),
                    data.label_id,
                    data.note,
                ])?;
            }
        }
        tx.commit()?;
        Ok(sessions.len())
    }

    /// Delete every row from `sessions`. Returns the number of rows deleted.
    pub fn delete_all_sessions(&self) -> Result<usize> {
        let n = self.conn.execute("DELETE FROM sessions", [])?;
        Ok(n)
    }

    /// Stream every session row in a stable order (start_time ASC) — used
    /// for CSV export. Calls `row_cb` once per session; the callback may
    /// return Err to abort.
    pub fn for_each_session<F: FnMut(&Session) -> Result<()>>(&self, mut row_cb: F) -> Result<()> {
        let mut stmt = self.conn.prepare(
            "SELECT id, start_time, duration_secs, mode, label_id, note
             FROM sessions ORDER BY start_time ASC"
        )?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let mode_str: String = row.get(3)?;
            let mode = match mode_str.as_str() {
                "stopwatch" => SessionMode::Stopwatch,
                _ => SessionMode::Countdown,
            };
            let sess = Session {
                id:            row.get(0)?,
                start_time:    row.get(1)?,
                duration_secs: row.get(2)?,
                mode,
                label_id:      row.get(4)?,
                note:          row.get(5)?,
            };
            row_cb(&sess)?;
        }
        Ok(())
    }

    /// Return a label id by name, creating it if missing. Matches
    /// case-insensitively so an import of "Meditation" finds an existing
    /// "meditation" instead of producing a duplicate row.
    pub fn find_or_create_label(&self, name: &str) -> Result<i64> {
        if let Some(id) = self.conn.query_row(
            "SELECT id FROM labels WHERE name = ?1 COLLATE NOCASE",
            params![name],
            |r| r.get::<_, i64>(0),
        ).optional()? {
            return Ok(id);
        }
        // `create_label` auto-suffixes on collision, so reuse that path when
        // the simple lookup missed (race-safe enough for a single-user app).
        Ok(self.create_label(name)?.id)
    }

    pub fn list_sessions(&self, filter: &SessionFilter) -> Result<Vec<Session>> {
        // Four fixed SQL variants (notes×label) so every statement is a static
        // string that prepare_cached can cache permanently, and label_id is
        // always a bound parameter rather than interpolated into the SQL.
        // ORDER BY start_time DESC uses the idx_sessions_start_time index.
        macro_rules! map_row {
            ($row:expr) => {
                Session {
                    id:            $row.get(0)?,
                    start_time:    $row.get(1)?,
                    duration_secs: $row.get(2)?,
                    mode:          SessionMode::from_str(&$row.get::<_, String>(3)?),
                    label_id:      $row.get(4)?,
                    note:          $row.get(5)?,
                }
            };
        }
        // LIMIT -1 means "no limit" in SQLite; OFFSET 0 means "no offset".
        // That lets us keep one static query per variant even when the caller
        // hasn't paginated.
        let limit_val: i64 = filter.limit.map(|n| n as i64).unwrap_or(-1);
        let offset_val: i64 = filter.offset.map(|n| n as i64).unwrap_or(0);
        match (filter.only_with_notes, filter.label_id) {
            (false, None) => {
                let mut s = self.conn.prepare_cached(
                    "SELECT id, start_time, duration_secs, mode, label_id, note
                     FROM sessions ORDER BY start_time DESC
                     LIMIT ?1 OFFSET ?2")?;
                let rows: Result<Vec<_>> =
                    s.query_map(params![limit_val, offset_val], |r| Ok(map_row!(r)))?.collect();
                rows
            }
            (true, None) => {
                let mut s = self.conn.prepare_cached(
                    "SELECT id, start_time, duration_secs, mode, label_id, note
                     FROM sessions WHERE note IS NOT NULL AND note != ''
                     ORDER BY start_time DESC
                     LIMIT ?1 OFFSET ?2")?;
                let rows: Result<Vec<_>> =
                    s.query_map(params![limit_val, offset_val], |r| Ok(map_row!(r)))?.collect();
                rows
            }
            (false, Some(lid)) => {
                let mut s = self.conn.prepare_cached(
                    "SELECT id, start_time, duration_secs, mode, label_id, note
                     FROM sessions WHERE label_id = ?1
                     ORDER BY start_time DESC
                     LIMIT ?2 OFFSET ?3")?;
                let rows: Result<Vec<_>> =
                    s.query_map(params![lid, limit_val, offset_val], |r| Ok(map_row!(r)))?.collect();
                rows
            }
            (true, Some(lid)) => {
                let mut s = self.conn.prepare_cached(
                    "SELECT id, start_time, duration_secs, mode, label_id, note
                     FROM sessions WHERE label_id = ?1 AND note IS NOT NULL AND note != ''
                     ORDER BY start_time DESC
                     LIMIT ?2 OFFSET ?3")?;
                let rows: Result<Vec<_>> =
                    s.query_map(params![lid, limit_val, offset_val], |r| Ok(map_row!(r)))?.collect();
                rows
            }
        }
    }

    pub fn update_session(&self, id: i64, data: &SessionData) -> Result<()> {
        self.conn.execute(
            "UPDATE sessions
             SET start_time = ?1, duration_secs = ?2, mode = ?3,
                 label_id = ?4, note = ?5
             WHERE id = ?6",
            params![
                data.start_time,
                data.duration_secs,
                data.mode.as_str(),
                data.label_id,
                data.note,
                id,
            ],
        )?;
        Ok(())
    }

    pub fn delete_session(&self, id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM sessions WHERE id = ?1", params![id])?;
        Ok(())
    }

    // ── Settings ──────────────────────────────────────────────────────────────

    pub fn get_presets(&self) -> Result<Vec<u32>> {
        let s = self.get_setting("timer_presets", "5,10,15,20,30")?;
        let vals: Vec<u32> = s.split(',')
            .filter_map(|v| v.trim().parse::<u32>().ok())
            .filter(|&v| v > 0)
            .collect();
        if vals.is_empty() { Ok(vec![5, 10, 15, 20, 30]) } else { Ok(vals) }
    }

    pub fn set_presets(&self, presets: &[u32]) -> Result<()> {
        let s = presets.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(",");
        self.set_setting("timer_presets", &s)
    }

    pub fn get_setting(&self, key: &str, default: &str) -> Result<String> {
        match self.conn.query_row(
            "SELECT value FROM settings WHERE key = ?1",
            params![key],
            |row| row.get::<_, String>(0),
        ) {
            Ok(val) => Ok(val),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(default.to_owned()),
            Err(e) => Err(e),
        }
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    // ── Stats queries ─────────────────────────────────────────────────────────

    /// Current streak: number of consecutive calendar days (ending today or
    /// yesterday) on which at least one session was completed.
    ///
    /// Uses a gap-and-island window query so no rows are loaded into Rust.
    pub fn get_streak(&self) -> Result<u32> {
        // CAST(julianday(day) AS INTEGER) gives an integer Julian day number
        // that increments by exactly 1 per calendar day regardless of DST.
        // jday - ROW_NUMBER() is constant within a consecutive run (island).
        self.conn.query_row(
            "WITH active_days AS (
                 SELECT DISTINCT strftime('%Y-%m-%d', start_time, 'unixepoch', 'localtime') AS day
                 FROM sessions
             ),
             numbered AS (
                 SELECT day,
                        CAST(julianday(day) AS INTEGER) - ROW_NUMBER() OVER (ORDER BY day) AS grp
                 FROM active_days
             ),
             last_grp AS (
                 SELECT grp FROM numbered ORDER BY day DESC LIMIT 1
             ),
             latest_day AS (
                 SELECT day FROM active_days ORDER BY day DESC LIMIT 1
             )
             SELECT CASE
                 WHEN (SELECT day FROM latest_day) >=
                      strftime('%Y-%m-%d', 'now', '-1 day', 'localtime')
                 THEN (SELECT COUNT(*) FROM numbered WHERE grp = (SELECT grp FROM last_grp))
                 ELSE 0
             END",
            [],
            |row| row.get::<_, u32>(0),
        )
    }

    /// Longest consecutive-day streak ever recorded.
    ///
    /// Uses a gap-and-island window query so no rows are loaded into Rust.
    pub fn get_best_streak(&self) -> Result<u32> {
        self.conn.query_row(
            "WITH active_days AS (
                 SELECT DISTINCT strftime('%Y-%m-%d', start_time, 'unixepoch', 'localtime') AS day
                 FROM sessions
             ),
             numbered AS (
                 SELECT CAST(julianday(day) AS INTEGER) - ROW_NUMBER() OVER (ORDER BY day) AS grp
                 FROM active_days
             )
             SELECT COALESCE(MAX(cnt), 0)
             FROM (SELECT COUNT(*) AS cnt FROM numbered GROUP BY grp)",
            [],
            |row| row.get::<_, u32>(0),
        )
    }

    /// Total meditation time across all sessions, in seconds.
    pub fn get_total_duration_secs(&self) -> Result<i64> {
        self.conn.query_row(
            "SELECT COALESCE(SUM(duration_secs), 0) FROM sessions",
            [],
            |row| row.get(0),
        )
    }

    /// Average daily meditation time (in seconds) over the last `days` days.
    /// Days with no sessions count as zero.
    pub fn get_running_average_secs(&self, days: u32) -> Result<f64> {
        // Compute the since-date in local time via SQLite so the boundary is
        // local midnight, not UTC midnight. We then hand the string to
        // strftime('%s', …, 'utc') inside the SUM query to turn it into a
        // unix timestamp, which makes the WHERE sargable against
        // idx_sessions_start_time.
        let since: String = self.conn.query_row(
            "SELECT strftime('%Y-%m-%d', 'now', ?1, 'localtime')",
            params![format!("-{} days", days - 1)],
            |r| r.get(0),
        )?;
        let total: i64 = self.conn.query_row(
            "SELECT COALESCE(SUM(duration_secs), 0)
             FROM sessions
             WHERE start_time >= strftime('%s', ?1, 'utc')",
            params![since],
            |row| row.get(0),
        )?;
        Ok(total as f64 / days as f64)
    }

    /// Returns `(local-date-string "YYYY-MM-DD", total_secs)` for each day
    /// on or after `since_date` that had at least one session. WHERE uses
    /// the unix boundary so the index drives the scan; only the narrowed
    /// subset pays for the strftime that produces the GROUP key.
    pub fn get_daily_totals(&self, since_date: &str) -> Result<Vec<(String, i64)>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT strftime('%Y-%m-%d', start_time, 'unixepoch', 'localtime') AS day,
                    SUM(duration_secs) AS total
             FROM sessions
             WHERE start_time >= strftime('%s', ?1, 'utc')
             GROUP BY day
             ORDER BY day ASC"
        )?;
        let rows = stmt.query_map(params![since_date], |row| Ok((row.get(0)?, row.get(1)?)))?;
        rows.collect()
    }

    /// Sum of `duration_secs` for every session whose local-time start date
    /// is on or after `since_date` (YYYY-MM-DD). Used for the weekly-goal
    /// ring, where `since_date` is the locale's current-week start.
    pub fn get_total_secs_since(&self, since_date: &str) -> Result<i64> {
        let total: i64 = self.conn.query_row(
            "SELECT COALESCE(SUM(duration_secs), 0)
             FROM sessions
             WHERE start_time >= strftime('%s', ?1, 'utc')",
            params![since_date],
            |row| row.get(0),
        )?;
        Ok(total)
    }

    /// Returns distinct (year, month) pairs that have at least one session,
    /// in descending order. Used to populate the calendar month picker.
    pub fn get_active_months(&self) -> Result<Vec<(i32, u32)>> {
        // 'localtime' matches every other date bucket in the DB layer; without
        // it, sessions started just before local midnight file into the wrong
        // month in the picker.
        let mut stmt = self.conn.prepare_cached(
            "SELECT DISTINCT
                 CAST(strftime('%Y', start_time, 'unixepoch', 'localtime') AS INTEGER),
                 CAST(strftime('%m', start_time, 'unixepoch', 'localtime') AS INTEGER)
             FROM sessions
             ORDER BY 1 DESC, 2 DESC"
        )?;
        let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        rows.collect()
    }

    /// Returns the set of day-of-month numbers in the given year/month that had
    /// at least one session. Uses `start_time BETWEEN` so the index is used.
    pub fn get_active_days_in_month(&self, year: i32, month: u32) -> Result<Vec<u32>> {
        // Compute the local-midnight boundaries as date strings; the 'utc'
        // modifier in SQLite converts them to the correct UTC epoch so the
        // idx_sessions_start_time index is usable.
        let start_str = format!("{year:04}-{month:02}-01");
        let (next_year, next_month) = if month == 12 { (year + 1, 1) } else { (year, month + 1) };
        let end_str = format!("{next_year:04}-{next_month:02}-01");

        let mut stmt = self.conn.prepare_cached(
            "SELECT DISTINCT
                 CAST(strftime('%d', start_time, 'unixepoch', 'localtime') AS INTEGER)
             FROM sessions
             WHERE start_time >= strftime('%s', ?1, 'utc')
               AND start_time <  strftime('%s', ?2, 'utc')
             ORDER BY 1"
        )?;
        let rows = stmt.query_map(params![start_str, end_str], |row| row.get::<_, u32>(0))?;
        rows.collect()
    }

    /// Total number of sessions ever recorded.
    pub fn get_session_count(&self) -> Result<i64> {
        self.conn.query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))
    }

    /// Longest single session, as (duration_secs, start_time_unix). None if
    /// the database is empty.
    pub fn get_longest_session(&self) -> Result<Option<(i64, i64)>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT duration_secs, start_time FROM sessions
             ORDER BY duration_secs DESC LIMIT 1"
        )?;
        let mut rows = stmt.query([])?;
        match rows.next()? {
            Some(row) => Ok(Some((row.get(0)?, row.get(1)?))),
            None => Ok(None),
        }
    }

    /// Median session duration (seconds). Returns the lower median on even
    /// counts. None if the database is empty.
    pub fn get_median_duration_secs(&self) -> Result<Option<i64>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT duration_secs FROM sessions
             ORDER BY duration_secs
             LIMIT 1 OFFSET (SELECT MAX(0, (COUNT(*) - 1) / 2) FROM sessions)"
        )?;
        let mut rows = stmt.query([])?;
        match rows.next()? {
            Some(row) => Ok(Some(row.get(0)?)),
            None => Ok(None),
        }
    }

    /// Session counts bucketed by local time-of-day:
    /// (morning <12, afternoon 12–17, evening ≥18).
    pub fn get_hour_buckets(&self) -> Result<(i64, i64, i64)> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT
               COALESCE(SUM(CASE WHEN h < 12 THEN 1 ELSE 0 END), 0),
               COALESCE(SUM(CASE WHEN h >= 12 AND h < 18 THEN 1 ELSE 0 END), 0),
               COALESCE(SUM(CASE WHEN h >= 18 THEN 1 ELSE 0 END), 0)
             FROM (
               SELECT CAST(strftime('%H', start_time, 'unixepoch', 'localtime') AS INTEGER) AS h
               FROM sessions
             )"
        )?;
        stmt.query_row([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
    }

    /// Sum of session durations (seconds) in a given local-time calendar month.
    pub fn get_month_total_secs(&self, year: i32, month: u32) -> Result<i64> {
        let start_str = format!("{year:04}-{month:02}-01");
        let (next_year, next_month) = if month == 12 { (year + 1, 1) } else { (year, month + 1) };
        let end_str = format!("{next_year:04}-{next_month:02}-01");
        self.conn.query_row(
            "SELECT COALESCE(SUM(duration_secs), 0)
             FROM sessions
             WHERE start_time >= strftime('%s', ?1, 'utc')
               AND start_time <  strftime('%s', ?2, 'utc')",
            params![start_str, end_str],
            |row| row.get(0),
        )
    }
}

