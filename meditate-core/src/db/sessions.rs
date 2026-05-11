//! `sessions` table — completed-meditation rows. CRUD plus the
//! query helpers that power stats (streak, daily totals, hour
//! buckets, label aggregates, longest session, etc.) and CSV
//! import/export.

use std::io::{Read, Write};

use rusqlite::{params, OptionalExtension};

use super::{Database, DbError, Result};

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
    /// Set on guided meditation rows that played a library-stored file
    /// (i.e. an entry in `guided_files`). `None` for non-Guided modes
    /// AND for transient one-off guided sessions where the user played
    /// a file without importing it. Lets stats and the log surface
    /// per-file aggregates on top of the per-mode breakdown.
    pub guided_file_uuid: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionMode {
    /// Generic timer session — covers both targeted countdowns and
    /// open-ended (stopwatch) runs. The distinction lives at the UI
    /// level (`current_target_secs: Option<u32>`) and isn't persisted:
    /// stats and the log already key off the recorded duration alone.
    Timer,
    BoxBreath,
    /// Guided meditation — the user picks an audio file (transient
    /// "Open File" or imported into the library); the session length
    /// is the file's natural duration. Pause / Stop / Add overtime
    /// mirror the Timer countdown's running view.
    Guided,
}

impl SessionMode {
    /// On-disk and CSV string representation. Exposed so callers
    /// (CSV import/export, debug logging) don't need to re-implement
    /// this match against the enum.
    pub fn as_db_str(self) -> &'static str {
        match self {
            SessionMode::Timer => "timer",
            SessionMode::BoxBreath => "box_breath",
            SessionMode::Guided => "guided",
        }
    }

    /// Inverse of `as_db_str`. Returns `None` for unknown / typo'd
    /// values; callers decide whether to hard-error or treat the row
    /// as corrupt. The DB column carries a CHECK constraint matching
    /// these strings, so reads off the local DB cannot legitimately
    /// hit the None branch — only out-of-band data (sync wire format,
    /// CSV import, hand-edited rows) needs to think about it.
    pub fn from_db_str(s: &str) -> Option<Self> {
        match s {
            "timer" => Some(SessionMode::Timer),
            "box_breath" => Some(SessionMode::BoxBreath),
            "guided" => Some(SessionMode::Guided),
            _ => None,
        }
    }
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

impl Database {
    /// Look up a label's cross-device UUID by its local rowid. Used at
    /// event-emission time to translate from the cache key (rowid) to
    /// the cross-device identity. Errors when the rowid is unknown —
    /// callers should already have validated this via the FK constraint
    /// or by reading the row's `label_id` from a known-good source.
    pub(super) fn label_uuid_by_id(&self, id: i64) -> Result<String> {
        Ok(self.conn.query_row(
            "SELECT uuid FROM labels WHERE id = ?1",
            params![id],
            |row| row.get::<_, String>(0),
        )?)
    }

    pub fn count_sessions(&self) -> Result<i64> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))?)
    }

    pub fn insert_session(&self, session: &Session) -> Result<i64> {
        let tx = self.conn.unchecked_transaction()?;
        let session_uuid = uuid::Uuid::new_v4().to_string();
        self.conn.execute(
            "INSERT INTO sessions (start_iso, duration_secs, label_id, notes, mode, uuid, guided_file_uuid)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                session.start_iso,
                session.duration_secs,
                session.label_id,
                session.notes,
                session.mode.as_db_str(),
                session_uuid,
                session.guided_file_uuid,
            ],
        )?;
        let rowid = self.conn.last_insert_rowid();

        // Translate label_id (local rowid) → label_uuid (cross-device).
        // The peer applying this event has a different rowid space.
        let label_uuid = match session.label_id {
            Some(id) => Some(self.label_uuid_by_id(id)?),
            None => None,
        };
        let payload = serde_json::json!({
            "uuid": session_uuid,
            "start_iso": session.start_iso,
            "duration_secs": session.duration_secs,
            "label_uuid": label_uuid,
            "notes": session.notes,
            "mode": session.mode.as_db_str(),
            "guided_file_uuid": session.guided_file_uuid,
        }).to_string();
        self.emit_event("session_insert", &session_uuid, payload)?;

        tx.commit()?;
        Ok(rowid)
    }

    /// Insert many sessions inside a single transaction — orders of
    /// magnitude faster than calling `insert_session` in a loop. Atomic:
    /// if any row fails a constraint, the whole batch is rolled back and
    /// the caller never sees a partially-imported DB. Each row also
    /// emits its own `session_insert` event — peers replay them
    /// independently, there is no "bulk" event kind.
    pub fn bulk_insert_sessions(&self, sessions: &[Session]) -> Result<usize> {
        let tx = self.conn.unchecked_transaction()?;
        let mut session_uuids: Vec<String> = Vec::with_capacity(sessions.len());
        {
            let mut stmt = tx.prepare(
                "INSERT INTO sessions (start_iso, duration_secs, label_id, notes, mode, uuid, guided_file_uuid)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
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
                    s.guided_file_uuid,
                ])?;
                session_uuids.push(uuid);
            }
        }
        for (s, session_uuid) in sessions.iter().zip(session_uuids) {
            let label_uuid = match s.label_id {
                Some(id) => Some(self.label_uuid_by_id(id)?),
                None => None,
            };
            let payload = serde_json::json!({
                "uuid": session_uuid,
                "start_iso": s.start_iso,
                "duration_secs": s.duration_secs,
                "label_uuid": label_uuid,
                "notes": s.notes,
                "mode": s.mode.as_db_str(),
                "guided_file_uuid": s.guided_file_uuid,
            }).to_string();
            self.emit_event("session_insert", &session_uuid, payload)?;
        }
        tx.commit()?;
        Ok(sessions.len())
    }

    /// Remove the row with `id`. Unknown ids are silently no-ops AND
    /// emit no event — otherwise peers would see a tombstone for a
    /// session they never knew existed.
    pub fn delete_session(&self, id: i64) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        let row_uuid: Option<String> = self.conn.query_row(
            "SELECT uuid FROM sessions WHERE id = ?1",
            params![id],
            |row| row.get::<_, String>(0),
        ).optional()?;
        let Some(uuid) = row_uuid else {
            return Ok(());
        };
        self.conn.execute("DELETE FROM sessions WHERE id = ?1", params![id])?;
        let payload = serde_json::json!({ "uuid": uuid }).to_string();
        self.emit_event("session_delete", &uuid, payload)?;
        tx.commit()?;
        Ok(())
    }

    /// Remove every session row. Returns how many rows were deleted.
    /// Labels and settings are untouched. Emits one `session_delete`
    /// event per row that was actually present, so peers tombstone the
    /// same set we cleared locally.
    pub fn delete_all_sessions(&self) -> Result<usize> {
        let tx = self.conn.unchecked_transaction()?;
        let row_uuids: Vec<String> = {
            let mut stmt = self.conn.prepare("SELECT uuid FROM sessions")?;
            let rows = stmt
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            rows
        };
        let n = self.conn.execute("DELETE FROM sessions", [])?;
        for uuid in &row_uuids {
            let payload = serde_json::json!({ "uuid": uuid }).to_string();
            self.emit_event("session_delete", uuid, payload)?;
        }
        tx.commit()?;
        Ok(n)
    }

    /// Replace every field of the row with `id`. Unknown ids are silently
    /// no-ops AND emit no event — peers would otherwise receive an update
    /// referencing a uuid we don't have.
    pub fn update_session(&self, id: i64, session: &Session) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        let row_uuid: Option<String> = self.conn.query_row(
            "SELECT uuid FROM sessions WHERE id = ?1",
            params![id],
            |row| row.get::<_, String>(0),
        ).optional()?;
        let Some(session_uuid) = row_uuid else {
            return Ok(());
        };
        self.conn.execute(
            "UPDATE sessions
             SET start_iso = ?1, duration_secs = ?2, label_id = ?3,
                 notes = ?4, mode = ?5, guided_file_uuid = ?6
             WHERE id = ?7",
            params![
                session.start_iso,
                session.duration_secs,
                session.label_id,
                session.notes,
                session.mode.as_db_str(),
                session.guided_file_uuid,
                id,
            ],
        )?;
        let label_uuid = match session.label_id {
            Some(id) => Some(self.label_uuid_by_id(id)?),
            None => None,
        };
        let payload = serde_json::json!({
            "uuid": session_uuid,
            "start_iso": session.start_iso,
            "duration_secs": session.duration_secs,
            "label_uuid": label_uuid,
            "notes": session.notes,
            "mode": session.mode.as_db_str(),
            "guided_file_uuid": session.guided_file_uuid,
        }).to_string();
        self.emit_event("session_update", &session_uuid, payload)?;
        tx.commit()?;
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
            let mode_str = record.get(4).unwrap_or("timer");
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
                guided_file_uuid: None,
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

    /// Lower-median session duration in seconds, or `None` when the
    /// sessions table is empty. The `Option` distinguishes "no data
    /// to report" from a real zero — useful for any shell rendering
    /// "n/a" vs "0s".
    pub fn get_median_duration_secs(&self) -> Result<Option<u32>> {
        let mut stmt = self
            .conn
            .prepare("SELECT duration_secs FROM sessions ORDER BY duration_secs")?;
        let durations: Vec<u32> = stmt
            .query_map([], |row| row.get(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        if durations.is_empty() {
            return Ok(None);
        }
        Ok(Some(durations[(durations.len() - 1) / 2]))
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
            "SELECT id, start_iso, duration_secs, label_id, notes, mode, uuid, guided_file_uuid
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
                        guided_file_uuid: row.get(7)?,
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
                    guided_file_uuid: row.get(7)?,
                },
            ))
        };

        let rows: rusqlite::Result<Vec<(i64, Session)>> = match (filter.only_with_notes, filter.label_id) {
            (false, None) => {
                let mut s = self.conn.prepare_cached(
                    "SELECT id, start_iso, duration_secs, label_id, notes, mode, uuid, guided_file_uuid
                     FROM sessions
                     ORDER BY start_iso DESC
                     LIMIT ?1 OFFSET ?2",
                )?;
                let it = s.query_map(params![limit_val, offset_val], map_row)?;
                it.collect()
            }
            (true, None) => {
                let mut s = self.conn.prepare_cached(
                    "SELECT id, start_iso, duration_secs, label_id, notes, mode, uuid, guided_file_uuid
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
                    "SELECT id, start_iso, duration_secs, label_id, notes, mode, uuid, guided_file_uuid
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
                    "SELECT id, start_iso, duration_secs, label_id, notes, mode, uuid, guided_file_uuid
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
            "SELECT id, start_iso, duration_secs, label_id, notes, mode, uuid, guided_file_uuid FROM sessions
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
                        guided_file_uuid: row.get(7)?,
                    },
                ))
            })?
            .collect::<rusqlite::Result<Vec<(i64, Session)>>>()?;
        Ok(sessions)
    }

    /// Recompute the `sessions` row for `session_uuid` from the events
    /// table. Same precedence rules as labels: tombstone wins on
    /// tie/precedence, else the highest-(lamport, device_id) mutate event
    /// drives the row's values. Update events carry every mutable field
    /// so they self-suffice if the corresponding insert event hasn't
    /// arrived yet.
    pub(super) fn recompute_session(&self, session_uuid: &str) -> Result<()> {
        let delete_ts: Option<i64> = self.conn.query_row(
            "SELECT MAX(lamport_ts) FROM events
             WHERE target_id = ?1 AND kind = 'session_delete'",
            params![session_uuid],
            |row| row.get::<_, Option<i64>>(0),
        )?;

        let mutate: Option<(i64, String)> = self.conn.query_row(
            "SELECT lamport_ts, payload FROM events
             WHERE target_id = ?1
               AND kind IN ('session_insert', 'session_update')
             ORDER BY lamport_ts DESC, device_id DESC
             LIMIT 1",
            params![session_uuid],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
        ).optional()?;

        let row_should_exist = match (mutate.as_ref(), delete_ts) {
            (Some(_), None) => true,
            (None, _) => false,
            (Some((m_ts, _)), Some(d_ts)) => *m_ts > d_ts,
        };

        if let Some((_, payload)) = mutate.filter(|_| row_should_exist) {
            let v: serde_json::Value = serde_json::from_str(&payload)
                .map_err(|e| DbError::Csv(
                    format!("session event payload not valid JSON: {e}")))?;
            let start_iso = v["start_iso"].as_str().unwrap_or_default();
            let duration_secs = v["duration_secs"].as_u64().unwrap_or(0) as u32;
            let label_uuid = v["label_uuid"].as_str();
            let label_id: Option<i64> = match label_uuid {
                Some(luuid) => self.conn.query_row(
                    "SELECT id FROM labels WHERE uuid = ?1",
                    params![luuid],
                    |row| row.get::<_, i64>(0),
                ).optional()?,
                None => None,
            };
            let notes = v["notes"].as_str();
            let mode = v["mode"].as_str().unwrap_or("timer");
            let guided_file_uuid = v["guided_file_uuid"].as_str();

            // UPSERT — first time materialising creates the row, later
            // recomputes overwrite every field with the winning event's
            // values. The local rowid stays stable across recomputes
            // because the UNIQUE column we conflict on is `uuid`.
            self.conn.execute(
                "INSERT INTO sessions (uuid, start_iso, duration_secs, label_id, notes, mode, guided_file_uuid)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(uuid) DO UPDATE SET
                    start_iso        = excluded.start_iso,
                    duration_secs    = excluded.duration_secs,
                    label_id         = excluded.label_id,
                    notes            = excluded.notes,
                    mode             = excluded.mode,
                    guided_file_uuid = excluded.guided_file_uuid",
                params![session_uuid, start_iso, duration_secs, label_id, notes, mode, guided_file_uuid],
            )?;
        } else {
            // Tombstoned (or no mutate event yet) → ensure absent.
            self.conn.execute(
                "DELETE FROM sessions WHERE uuid = ?1",
                params![session_uuid],
            )?;
        }
        Ok(())
    }
}
