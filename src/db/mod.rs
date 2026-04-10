use rusqlite::{Connection, Result, params};
use std::path::Path;

// ── Models ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionMode {
    Countdown,
    Stopwatch,
}

impl SessionMode {
    fn as_str(&self) -> &'static str {
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
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;

        let db = Self { conn };
        db.migrate()?;
        Ok(db)
    }

    // ── Migrations ────────────────────────────────────────────────────────────

    fn migrate(&self) -> Result<()> {
        self.conn.execute_batch("
            CREATE TABLE IF NOT EXISTS schema_migrations (
                version INTEGER PRIMARY KEY
            );

            CREATE TABLE IF NOT EXISTS labels (
                id   INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL UNIQUE
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
        ")?;
        Ok(())
    }

    // ── Labels ────────────────────────────────────────────────────────────────

    pub fn create_label(&self, name: &str) -> Result<Label> {
        self.conn.execute("INSERT INTO labels (name) VALUES (?1)", params![name])?;
        let id = self.conn.last_insert_rowid();
        Ok(Label { id, name: name.to_owned() })
    }

    pub fn list_labels(&self) -> Result<Vec<Label>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name FROM labels ORDER BY name COLLATE NOCASE"
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(Label { id: row.get(0)?, name: row.get(1)? })
        })?;
        rows.collect()
    }

    pub fn update_label(&self, id: i64, name: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE labels SET name = ?1 WHERE id = ?2",
            params![name, id],
        )?;
        Ok(())
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

    pub fn list_sessions(&self, filter: &SessionFilter) -> Result<Vec<Session>> {
        // Build the WHERE clause dynamically from the filter.
        let mut conditions = Vec::new();
        if filter.only_with_notes {
            conditions.push("note IS NOT NULL AND note != ''");
        }
        let label_clause;
        if filter.label_id.is_some() {
            label_clause = format!("label_id = {}", filter.label_id.unwrap());
            conditions.push(&label_clause);
        }
        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };

        let sql = format!(
            "SELECT id, start_time, duration_secs, mode, label_id, note
             FROM sessions
             {where_clause}
             ORDER BY start_time DESC"
        );

        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map([], |row| {
            Ok(Session {
                id:            row.get(0)?,
                start_time:    row.get(1)?,
                duration_secs: row.get(2)?,
                mode:          SessionMode::from_str(&row.get::<_, String>(3)?),
                label_id:      row.get(4)?,
                note:          row.get(5)?,
            })
        })?;
        rows.collect()
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

    // ── Stats queries ─────────────────────────────────────────────────────────

    /// Current streak: number of consecutive calendar days (ending today or
    /// yesterday) on which at least one session was completed.
    pub fn get_streak(&self) -> Result<u32> {
        // Fetch distinct dates (as Unix day numbers) in descending order.
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT CAST(start_time / 86400 AS INTEGER) AS day
             FROM sessions
             ORDER BY day DESC"
        )?;
        let days: Vec<i64> = stmt
            .query_map([], |row| row.get(0))?
            .collect::<Result<_>>()?;

        if days.is_empty() {
            return Ok(0);
        }

        let today = today_unix_day();
        // Streak must end today or yesterday (allow for not having meditated yet today).
        if days[0] < today - 1 {
            return Ok(0);
        }

        let mut streak = 1u32;
        for window in days.windows(2) {
            if window[0] - window[1] == 1 {
                streak += 1;
            } else {
                break;
            }
        }
        Ok(streak)
    }

    /// Longest consecutive-day streak ever recorded.
    pub fn get_best_streak(&self) -> Result<u32> {
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT CAST(start_time / 86400 AS INTEGER) AS day
             FROM sessions
             ORDER BY day ASC"
        )?;
        let days: Vec<i64> = stmt
            .query_map([], |row| row.get(0))?
            .collect::<Result<_>>()?;

        if days.is_empty() {
            return Ok(0);
        }

        let mut best = 1u32;
        let mut current = 1u32;
        for w in days.windows(2) {
            if w[1] - w[0] == 1 {
                current += 1;
                if current > best { best = current; }
            } else {
                current = 1;
            }
        }
        Ok(best)
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
        let since = (today_unix_day() - days as i64) * 86400;
        let total: i64 = self.conn.query_row(
            "SELECT COALESCE(SUM(duration_secs), 0)
             FROM sessions
             WHERE start_time >= ?1",
            params![since],
            |row| row.get(0),
        )?;
        Ok(total as f64 / days as f64)
    }

    /// Returns (day_unix_timestamp, total_duration_secs) for each day in the
    /// last `days` days that had at least one session. Used by the bar chart.
    pub fn get_daily_totals(&self, days: u32) -> Result<Vec<(i64, i64)>> {
        let since = (today_unix_day() - days as i64) * 86400;
        let mut stmt = self.conn.prepare(
            "SELECT CAST(start_time / 86400 AS INTEGER) * 86400 AS day,
                    SUM(duration_secs) AS total
             FROM sessions
             WHERE start_time >= ?1
             GROUP BY day
             ORDER BY day ASC"
        )?;
        let rows = stmt.query_map(params![since], |row| Ok((row.get(0)?, row.get(1)?)))?;
        rows.collect()
    }

    /// Returns distinct (year, month) pairs that have at least one session,
    /// in descending order. Used to populate the calendar month picker.
    pub fn get_active_months(&self) -> Result<Vec<(i32, u32)>> {
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT
                 CAST(strftime('%Y', start_time, 'unixepoch') AS INTEGER),
                 CAST(strftime('%m', start_time, 'unixepoch') AS INTEGER)
             FROM sessions
             ORDER BY 1 DESC, 2 DESC"
        )?;
        let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        rows.collect()
    }

    /// Returns the set of days (as Unix timestamps, midnight UTC) in the given
    /// year/month that had at least one session. Used to render the calendar.
    pub fn get_active_days_in_month(&self, year: i32, month: u32) -> Result<Vec<i64>> {
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT CAST(start_time / 86400 AS INTEGER) * 86400
             FROM sessions
             WHERE strftime('%Y', start_time, 'unixepoch') = ?1
               AND strftime('%m', start_time, 'unixepoch') = ?2
             ORDER BY 1"
        )?;
        let year_str = format!("{year:04}");
        let month_str = format!("{month:02}");
        let rows = stmt.query_map(params![year_str, month_str], |row| row.get(0))?;
        rows.collect()
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Current date as a Unix day number (seconds since epoch / 86400, floored).
fn today_unix_day() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
        / 86400
}
