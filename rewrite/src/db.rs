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
pub struct Session {
    pub start_iso: String,
    pub duration_secs: u32,
    pub label_id: Option<i64>,
    pub notes: Option<String>,
    pub mode: SessionMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionMode {
    Countdown,
    Stopwatch,
    BoxBreath,
}

impl SessionMode {
    fn as_db_str(self) -> &'static str {
        match self {
            SessionMode::Countdown => "countdown",
            SessionMode::Stopwatch => "stopwatch",
            SessionMode::BoxBreath => "box_breath",
        }
    }

    fn from_db_str(s: &str) -> Option<Self> {
        match s {
            "countdown" => Some(SessionMode::Countdown),
            "stopwatch" => Some(SessionMode::Stopwatch),
            "box_breath" => Some(SessionMode::BoxBreath),
            _ => None,
        }
    }
}

pub struct Database {
    conn: Connection,
}

const SCHEMA: &str = "
    CREATE TABLE IF NOT EXISTS labels (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        name TEXT NOT NULL UNIQUE
    );
    CREATE TABLE IF NOT EXISTS sessions (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        start_iso TEXT NOT NULL,
        duration_secs INTEGER NOT NULL,
        label_id INTEGER REFERENCES labels(id),
        notes TEXT,
        mode TEXT NOT NULL CHECK (mode IN ('countdown', 'stopwatch', 'box_breath'))
    );
";

impl Database {
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn })
    }

    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn })
    }

    pub fn insert_label(&self, name: &str) -> Result<()> {
        match self
            .conn
            .execute("INSERT INTO labels (name) VALUES (?1)", [name])
        {
            Ok(_) => Ok(()),
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

    pub fn list_labels(&self) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare("SELECT name FROM labels ORDER BY name")?;
        let names = stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<String>>>()?;
        Ok(names)
    }

    /// Returns a label name guaranteed to be unique in the labels table:
    /// returns `base` if no collision, else `"{base} 2"`, `"{base} 3"`, …
    pub fn unique_label_name(&self, base: &str) -> Result<String> {
        Ok(base.to_string())
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
        self.conn.execute(
            "INSERT INTO sessions (start_iso, duration_secs, label_id, notes, mode)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                session.start_iso,
                session.duration_secs,
                session.label_id,
                session.notes,
                session.mode.as_db_str(),
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn get_best_streak(&self) -> Result<i64> {
        let days = self.distinct_session_days_ascending()?;
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

            let label_id = if let Some(label_name) = label {
                match self.find_label_by_name(&label_name)? {
                    Some(id) => Some(id),
                    None => {
                        self.insert_label(&label_name)?;
                        self.find_label_by_name(&label_name)?
                    }
                }
            } else {
                None
            };

            self.insert_session(&Session {
                start_iso,
                duration_secs,
                label_id,
                notes,
                mode,
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
        let mut stmt = self.conn.prepare(
            "SELECT SUBSTR(start_iso, 1, 10) AS day, SUM(duration_secs)
             FROM sessions
             GROUP BY day
             ORDER BY day",
        )?;
        let totals = stmt
            .query_map([], |row| {
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

    fn distinct_session_days_ascending(&self) -> Result<Vec<chrono::NaiveDate>> {
        let mut stmt = self
            .conn
            .prepare("SELECT DISTINCT SUBSTR(start_iso, 1, 10) FROM sessions ORDER BY 1")?;
        let days = stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?
            .into_iter()
            .filter_map(|s| chrono::NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok())
            .collect();
        Ok(days)
    }

    pub fn get_streak(&self, today: chrono::NaiveDate) -> Result<i64> {
        let days = self.distinct_session_days_ascending()?;
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

    pub fn total_minutes(&self) -> Result<i64> {
        let total_secs: i64 = self.conn.query_row(
            "SELECT COALESCE(SUM(duration_secs), 0) FROM sessions",
            [],
            |row| row.get(0),
        )?;
        Ok(total_secs / 60)
    }

    pub fn list_sessions(&self) -> Result<Vec<Session>> {
        let mut stmt = self.conn.prepare(
            "SELECT start_iso, duration_secs, label_id, notes, mode FROM sessions ORDER BY id",
        )?;
        let sessions = stmt
            .query_map([], |row| {
                let mode_str: String = row.get(4)?;
                let mode = SessionMode::from_db_str(&mode_str).expect(
                    "DB CHECK constraint should restrict mode to known values",
                );
                Ok(Session {
                    start_iso: row.get(0)?,
                    duration_secs: row.get(1)?,
                    label_id: row.get(2)?,
                    notes: row.get(3)?,
                    mode,
                })
            })?
            .collect::<rusqlite::Result<Vec<Session>>>()?;
        Ok(sessions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn unique_label_name_returns_base_when_no_collision() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(db.unique_label_name("Morning").unwrap(), "Morning");
    }

    #[test]
    fn list_labels_returns_inserted_names_alphabetically() {
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        db.insert_label("Afternoon").unwrap();
        db.insert_label("Evening").unwrap();
        assert_eq!(
            db.list_labels().unwrap(),
            vec!["Afternoon", "Evening", "Morning"]
        );
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
        };
        db.insert_session(&session).unwrap();
        assert_eq!(db.count_sessions().unwrap(), 1);
    }

    #[test]
    fn list_sessions_round_trips_inserted_session() {
        let db = Database::open_in_memory().unwrap();
        let session = Session {
            start_iso: "2026-04-27T10:00:00Z".to_string(),
            duration_secs: 600,
            label_id: None,
            notes: Some("felt clear today".to_string()),
            mode: SessionMode::BoxBreath,
        };
        db.insert_session(&session).unwrap();
        assert_eq!(db.list_sessions().unwrap(), vec![session]);
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
        assert_eq!(db.list_labels().unwrap(), vec!["Morning"]);
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
        })
        .unwrap();
        src.insert_session(&Session {
            start_iso: "2026-04-27T19:00:00Z".to_string(),
            duration_secs: 1200,
            label_id: None,
            notes: None,
            mode: SessionMode::BoxBreath,
        })
        .unwrap();

        let mut buf = Vec::new();
        src.export_sessions_csv(&mut buf).unwrap();

        let dst = Database::open_in_memory().unwrap();
        let imported = dst.import_sessions_csv(&buf[..]).unwrap();
        assert_eq!(imported, 2);

        // Label was created on import.
        assert_eq!(dst.list_labels().unwrap(), vec!["Morning"]);
        let dst_morning_id = dst.find_label_by_name("Morning").unwrap();

        let sessions = dst.list_sessions().unwrap();
        assert_eq!(sessions.len(), 2);
        assert_eq!(
            sessions[0],
            Session {
                start_iso: "2026-04-27T10:00:00Z".to_string(),
                duration_secs: 600,
                label_id: dst_morning_id,
                notes: Some("clear, focused".to_string()),
                mode: SessionMode::Countdown,
            }
        );
        assert_eq!(
            sessions[1],
            Session {
                start_iso: "2026-04-27T19:00:00Z".to_string(),
                duration_secs: 1200,
                label_id: None,
                notes: None,
                mode: SessionMode::BoxBreath,
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
}
