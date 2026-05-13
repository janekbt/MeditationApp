//! `sessions` table — completed-meditation rows. CRUD plus the
//! query helpers that power stats (streak, daily totals, hour
//! buckets, label aggregates, longest session, etc.) and CSV
//! import/export.

use std::io::{Read, Write};

use rusqlite::{params, OptionalExtension};

use super::events::EventKind;
use super::{Database, DbError, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub start_iso: String,
    pub duration_secs: u32,
    pub label_id: Option<i64>,
    pub notes: Option<String>,
    pub mode: SessionMode,
    /// Stable cross-device identity, assigned by the DB at insert time.
    /// Callers may set this to `SessionUuid::new("")` before insert —
    /// the value is overwritten with a freshly generated v4 UUID.
    /// Always populated on read paths.
    pub uuid: super::SessionUuid,
    /// Set on guided meditation rows that played a library-stored file
    /// (i.e. an entry in `guided_files`). `None` for non-Guided modes
    /// AND for transient one-off guided sessions where the user played
    /// a file without importing it. Lets stats and the log surface
    /// per-file aggregates on top of the per-mode breakdown.
    pub guided_file_uuid: Option<super::GuidedFileUuid>,
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
        let (rowid, _uuid) = self.insert_session_tx_less(session)?;
        tx.commit()?;
        Ok(rowid)
    }

    /// Transaction-less core of `insert_session`. Inserts the row,
    /// emits the `session_insert` event, returns `(rowid,
    /// session_uuid)`. The caller is responsible for opening +
    /// committing the surrounding transaction. Split out so
    /// `finalize_session_in_progress` can atomically insert the
    /// session AND clear the in-progress row inside one outer
    /// transaction — without this, the gap between insert.commit
    /// and the subsequent clear would let a crash double-finalize
    /// the same in-flight session on the next launch.
    pub(super) fn insert_session_tx_less(&self, session: &Session) -> Result<(i64, String)> {
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
        self.emit_event(EventKind::SessionInsert, &session_uuid, payload)?;

        Ok((rowid, session_uuid))
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
            self.emit_event(EventKind::SessionInsert, &session_uuid, payload)?;
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
        self.emit_event(EventKind::SessionDelete, &uuid, payload)?;
        tx.commit()?;
        Ok(())
    }

    /// Delete a session row by its cross-device uuid. Used by the
    /// crash-recovery Undo flow — the toast carries the uuid that
    /// `finalize_session_in_progress` minted, not the local rowid.
    /// Emits one `session_delete` event so peers tombstone the row
    /// too. Unknown uuids are a silent no-op (the row may have been
    /// deleted via a peer-authored event between finalize and Undo).
    pub fn delete_session_by_uuid(&self, uuid: &str) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        let exists: Option<i64> = self.conn.query_row(
            "SELECT id FROM sessions WHERE uuid = ?1",
            params![uuid],
            |row| row.get::<_, i64>(0),
        ).optional()?;
        if exists.is_none() {
            return Ok(());
        }
        self.conn.execute("DELETE FROM sessions WHERE uuid = ?1", params![uuid])?;
        let payload = serde_json::json!({ "uuid": uuid }).to_string();
        self.emit_event(EventKind::SessionDelete, uuid, payload)?;
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
            self.emit_event(EventKind::SessionDelete, uuid, payload)?;
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
        self.emit_event(EventKind::SessionUpdate, &session_uuid, payload)?;
        tx.commit()?;
        Ok(())
    }

    pub fn get_best_streak(&self) -> Result<u32> {
        self.best_streak_filtered(None)
    }

    pub fn get_best_streak_for_label(&self, label_id: i64) -> Result<u32> {
        self.best_streak_filtered(Some(label_id))
    }

    fn best_streak_filtered(&self, label_filter: Option<i64>) -> Result<u32> {
        let days = self.distinct_session_days_ascending(label_filter)?;
        if days.is_empty() {
            return Ok(0);
        }
        let mut best = 1u32;
        let mut current = 1u32;
        for window in days.windows(2) {
            let Some(next_day) = window[0].succ_opt() else {
                return Err(DbError::DateOutOfRange);
            };
            if window[1] == next_day {
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
            let record = record.map_err(|e| DbError::Decode(e.to_string()))?;
            let start_iso = record
                .get(0)
                .ok_or_else(|| DbError::Decode("missing start_iso".to_string()))?
                .to_string();
            // Validate the timestamp before storing — `local_iso_to_unix`
            // returns 0 on parse failure / out-of-range year. Without
            // this gate a garbage row would persist; stats paths filter
            // it back out silently via `parse_from_str(...).ok()`, so
            // the row would vanish from totals while still counting
            // toward `count_sessions` and `get_longest_session`.
            if crate::time::local_iso_to_unix(&start_iso) == 0 {
                return Err(DbError::Decode(format!(
                    "bad start_iso: {start_iso:?}"
                )));
            }
            let duration_secs: u32 = record
                .get(1)
                .unwrap_or("")
                .parse()
                .map_err(|_| DbError::Decode("bad duration_secs".to_string()))?;
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
                .ok_or_else(|| DbError::Decode(format!("unknown mode: {mode_str}")))?;

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
                uuid: super::SessionUuid::new(""),
                guided_file_uuid: None,
            })?;
            count += 1;
        }
        Ok(count)
    }

    pub fn export_sessions_csv<W: Write>(&self, writer: W) -> Result<()> {
        let mut wtr = csv::Writer::from_writer(writer);
        wtr.write_record(["start_iso", "duration_secs", "label", "notes", "mode"])
            .map_err(|e| DbError::Decode(e.to_string()))?;

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
            .map_err(|e| DbError::Decode(e.to_string()))?;
        }
        wtr.flush().map_err(|e| DbError::Decode(e.to_string()))?;
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

    pub fn get_running_average_secs(&self, today: chrono::NaiveDate, days: u32) -> Result<f64> {
        if days == 0 {
            return Ok(0.0);
        }
        let cutoff = today - chrono::Duration::days((days - 1) as i64);
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

    /// Same shape as `get_daily_totals`, but only days on or after
    /// `since`. Pushes the date filter into the SQL `WHERE` instead
    /// of materialising every row in Rust and filtering after —
    /// matters once a heatmap user has a year+ of sessions; cheap
    /// even on tiny libraries because the comparison is on the
    /// `SUBSTR(start_iso, 1, 10)` GROUP key which sorts
    /// lexicographically identical to date order.
    pub fn get_daily_totals_since(
        &self,
        since: chrono::NaiveDate,
    ) -> Result<Vec<(chrono::NaiveDate, i64)>> {
        let since_str = since.format("%Y-%m-%d").to_string();
        let mut stmt = self.conn.prepare_cached(
            "SELECT SUBSTR(start_iso, 1, 10) AS day, SUM(duration_secs)
             FROM sessions
             WHERE SUBSTR(start_iso, 1, 10) >= ?1
             GROUP BY day
             ORDER BY day",
        )?;
        let totals = stmt
            .query_map(params![since_str], |row| {
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

    /// Sum of `duration_secs` grouped by start date (`SUBSTR(start_iso,
    /// 1, 10)`). A session that began at 23:55 and ran 30 minutes
    /// attributes the full 30 minutes to the start date — none of it
    /// is split into the next day. Matches the `hour_buckets`
    /// attribution rule (see its doc comment) and the peer-app
    /// convention. Intended behaviour.
    fn daily_totals_filtered(
        &self,
        label_filter: Option<i64>,
    ) -> Result<Vec<(chrono::NaiveDate, i64)>> {
        // `prepare_cached`: fires on every Stats / Insights / contrib
        // heatmap / streak refresh — among the hottest reads in the
        // crate. Caching the parse is free on the first call and
        // shaves a noticeable slice off every subsequent tab open.
        let mut stmt = self.conn.prepare_cached(
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
        // `prepare_cached`: streak refresh on every Setup view open
        // + every Stats tab open. Same hotness profile as
        // `daily_totals_filtered` above.
        let mut stmt = self.conn.prepare_cached(
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

    pub fn get_streak(&self, today: chrono::NaiveDate) -> Result<u32> {
        self.streak_filtered(today, None)
    }

    pub fn get_streak_for_label(&self, today: chrono::NaiveDate, label_id: i64) -> Result<u32> {
        self.streak_filtered(today, Some(label_id))
    }

    fn streak_filtered(
        &self,
        today: chrono::NaiveDate,
        label_filter: Option<i64>,
    ) -> Result<u32> {
        let days = self.distinct_session_days_ascending(label_filter)?;
        let Some(&most_recent) = days.last() else {
            return Ok(0);
        };
        // `pred_opt()` returns None only at `NaiveDate::MIN_DATE`
        // (chrono year -262144). Practically unreachable on real
        // session data, but the streak path runs on every Setup
        // view open — a panic here would be a sharp edge for any
        // future caller (CSV import of a synthetic test row, fuzz
        // input, etc.). Treat the boundary as "no prior day exists"
        // → no streak.
        let Some(yesterday) = today.pred_opt() else {
            return Ok(0);
        };
        let mut expected = if most_recent == today {
            today
        } else if most_recent == yesterday {
            yesterday
        } else {
            return Ok(0);
        };

        let mut count = 0u32;
        for day in days.iter().rev() {
            if *day == expected {
                count += 1;
                // Same boundary: walking past MIN_DATE means the
                // streak terminates here, not that we panic.
                let Some(prev) = expected.pred_opt() else { break };
                expected = prev;
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
    ///
    /// Attribution is by **start hour only** — a session that began at
    /// 23:55 and ran 30 minutes counts as one evening session, even
    /// though most of it occurred the next morning. This matches how
    /// peer meditation apps (Insight Timer et al.) report
    /// time-of-day, and avoids the CTE + date-arithmetic complexity
    /// a "split across midnight" SUM would require. Documented as
    /// intended behaviour, not a defect.
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

        // Build the WHERE clause from the filter. `prepare_cached`
        // dedupes by generated SQL text, so the four possible
        // (label_id, only_with_notes) combinations each end up with
        // their own cached statement just like the previous hand-
        // unrolled branches.
        let mut sql = String::from(
            "SELECT id, start_iso, duration_secs, label_id, notes, mode, uuid, guided_file_uuid \
             FROM sessions"
        );
        let mut clauses: Vec<&'static str> = Vec::new();
        if filter.label_id.is_some() {
            clauses.push("label_id = ?");
        }
        if filter.only_with_notes {
            clauses.push("notes IS NOT NULL AND notes != ''");
        }
        if !clauses.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&clauses.join(" AND "));
        }
        sql.push_str(" ORDER BY start_iso DESC LIMIT ? OFFSET ?");

        // Param order matches positional `?` placement: optional
        // label_id first (only when the clause is included), then
        // limit, then offset.
        let mut params: Vec<&dyn rusqlite::ToSql> = Vec::new();
        if let Some(lid) = filter.label_id.as_ref() {
            params.push(lid);
        }
        params.push(&limit_val);
        params.push(&offset_val);

        let mut s = self.conn.prepare_cached(&sql)?;
        let rows = s.query_map(rusqlite::params_from_iter(params), |row| {
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
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
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
        let Some(v) = self.winning_mutate(
            session_uuid,
            [EventKind::SessionInsert, EventKind::SessionUpdate],
            EventKind::SessionDelete,
        )? else {
            // Tombstoned (or no mutate event yet) → ensure absent.
            self.conn.execute(
                "DELETE FROM sessions WHERE uuid = ?1",
                params![session_uuid],
            )?;
            return Ok(());
        };
        {
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
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_helpers::*;

    // ── B1.1: apply_event for session events ─────────────────────────────────
    //
    // apply_event consumes a remote-authored event and updates the local
    // materialized cache. The model: record the event in `events`, then
    // recompute the cache row for its target_id from the events table —
    // tombstone wins on tie/precedence, otherwise the highest-lamport
    // mutate event drives the row's values. This makes apply_event
    // idempotent (re-applying same event_uuid is a no-op via INSERT OR
    // IGNORE) and order-independent (out-of-order delivery converges).

    #[test]
    fn apply_event_session_insert_creates_the_row() {
        // Apply a single insert event from a peer; the cache row appears
        // with all the event's values.
        let db = Database::open_in_memory().unwrap();
        let event = synth_session_insert(
            SESSION_X, 5, DEVICE_A,
            "2026-04-30T10:00:00", 600,
            None, Some("from peer"), SessionMode::BoxBreath,
        );
        db.apply_event(&event).unwrap();
        let rows = db.list_sessions().unwrap();
        assert_eq!(rows.len(), 1);
        let s = &rows[0].1;
        assert_eq!(s.uuid, SESSION_X);
        assert_eq!(s.start_iso, "2026-04-30T10:00:00");
        assert_eq!(s.duration_secs, 600);
        assert_eq!(s.notes.as_deref(), Some("from peer"));
        assert_eq!(s.mode, SessionMode::BoxBreath);
    }

    #[test]
    fn apply_event_session_insert_with_guided_file_uuid_round_trips() {
        // A guided session synced from a peer carries the file's uuid
        // in the event payload so per-file stats stay consistent across
        // devices. recompute_session must lift `guided_file_uuid` out
        // of the JSON payload and write it to the column.
        let db = Database::open_in_memory().unwrap();
        let file_uuid = "fffffff0-0000-4000-8000-cccccccccccc";
        let event = synth_event(
            "session_insert",
            SESSION_X,
            7,
            DEVICE_A,
            serde_json::json!({
                "uuid": SESSION_X,
                "start_iso": "2026-05-05T20:30:00",
                "duration_secs": 1200,
                "label_uuid": serde_json::Value::Null,
                "notes": serde_json::Value::Null,
                "mode": "guided",
                "guided_file_uuid": file_uuid,
            }),
        );
        db.apply_event(&event).unwrap();
        let rows = db.list_sessions().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].1.guided_file_uuid.as_ref().map(|u| u.as_str()), Some(file_uuid));
    }

    #[test]
    fn apply_event_session_insert_without_guided_file_uuid_leaves_column_null() {
        // Old-shape event payloads (no guided_file_uuid key) must
        // continue to work — recompute_session reads the field as
        // optional and writes NULL when missing.
        let db = Database::open_in_memory().unwrap();
        let event = synth_session_insert(
            SESSION_X, 5, DEVICE_A,
            "2026-04-30T10:00:00", 600,
            None, None, SessionMode::Timer,
        );
        db.apply_event(&event).unwrap();
        let rows = db.list_sessions().unwrap();
        assert_eq!(rows.len(), 1);
        assert!(rows[0].1.guided_file_uuid.is_none());
    }

    #[test]
    fn apply_event_is_idempotent_on_event_uuid() {
        // Applying the exact same Event twice must not double-insert
        // and must not error. The events table's UNIQUE(event_uuid)
        // is the dedup key.
        let db = Database::open_in_memory().unwrap();
        let event = synth_session_insert(
            SESSION_X, 5, DEVICE_A,
            "2026-04-30T10:00:00", 600,
            None, None, SessionMode::Timer,
        );
        db.apply_event(&event).unwrap();
        db.apply_event(&event).unwrap();
        assert_eq!(db.list_sessions().unwrap().len(), 1,
            "duplicate event_uuid must not create a second row");
    }

    #[test]
    fn apply_event_session_update_after_insert_updates_the_row() {
        let db = Database::open_in_memory().unwrap();
        db.apply_event(&synth_session_insert(
            SESSION_X, 5, DEVICE_A,
            "2026-04-30T10:00:00", 600,
            None, None, SessionMode::Timer,
        )).unwrap();
        db.apply_event(&synth_session_update(
            SESSION_X, 10, DEVICE_A,
            "2026-05-01T11:00:00", 1200,
            None, Some("revised"), SessionMode::Timer,
        )).unwrap();
        let s = &db.list_sessions().unwrap()[0].1;
        assert_eq!(s.start_iso, "2026-05-01T11:00:00");
        assert_eq!(s.duration_secs, 1200);
        assert_eq!(s.notes.as_deref(), Some("revised"));
        assert_eq!(s.mode, SessionMode::Timer);
    }

    #[test]
    fn apply_event_session_delete_removes_the_row() {
        let db = Database::open_in_memory().unwrap();
        db.apply_event(&synth_session_insert(
            SESSION_X, 5, DEVICE_A,
            "2026-04-30T10:00:00", 600,
            None, None, SessionMode::Timer,
        )).unwrap();
        db.apply_event(&synth_session_delete(SESSION_X, 10, DEVICE_A)).unwrap();
        assert!(db.list_sessions().unwrap().is_empty());
    }

    #[test]
    fn apply_event_tombstone_resists_later_applied_lower_lamport_insert() {
        // Out-of-order delivery: peer's delete arrives first (lamport=10),
        // then their insert at lamport=5 lands. The row must stay gone —
        // delete tombstones beat earlier inserts.
        let db = Database::open_in_memory().unwrap();
        db.apply_event(&synth_session_delete(SESSION_X, 10, DEVICE_A)).unwrap();
        db.apply_event(&synth_session_insert(
            SESSION_X, 5, DEVICE_A,
            "2026-04-30T10:00:00", 600,
            None, None, SessionMode::Timer,
        )).unwrap();
        assert!(db.list_sessions().unwrap().is_empty(),
            "tombstone with lamport 10 must beat insert at lamport 5");
    }

    #[test]
    fn apply_event_higher_lamport_update_supersedes_lower_one() {
        // Two updates from different devices on the same uuid; whichever
        // has the higher lamport_ts wins, regardless of arrival order.
        let db = Database::open_in_memory().unwrap();
        // Device A's update at lamport 10, Device B's at lamport 7 —
        // A wins. Apply B first (out of order), then A.
        db.apply_event(&synth_session_insert(
            SESSION_X, 1, DEVICE_A,
            "initial", 100, None, None, SessionMode::Timer,
        )).unwrap();
        db.apply_event(&synth_session_update(
            SESSION_X, 7, DEVICE_B,
            "B's edit", 700, None, Some("from B"), SessionMode::Timer,
        )).unwrap();
        db.apply_event(&synth_session_update(
            SESSION_X, 10, DEVICE_A,
            "A's edit", 1000, None, Some("from A"), SessionMode::BoxBreath,
        )).unwrap();
        let s = &db.list_sessions().unwrap()[0].1;
        assert_eq!(s.notes.as_deref(), Some("from A"),
            "A's lamport-10 update must win over B's lamport-7");
        assert_eq!(s.duration_secs, 1000);
    }

    #[test]
    fn apply_event_concurrent_updates_break_ties_on_device_id() {
        // Two updates with the SAME lamport_ts but different device_ids.
        // Lex-larger device_id wins (consistent across all peers per the
        // plan's tie-break rule).
        let db = Database::open_in_memory().unwrap();
        db.apply_event(&synth_session_insert(
            SESSION_X, 1, DEVICE_A,
            "initial", 100, None, None, SessionMode::Timer,
        )).unwrap();
        db.apply_event(&synth_session_update(
            SESSION_X, 5, DEVICE_A,
            "A wrote this", 500, None, Some("from A"), SessionMode::Timer,
        )).unwrap();
        db.apply_event(&synth_session_update(
            SESSION_X, 5, DEVICE_B,
            "B wrote this", 500, None, Some("from B"), SessionMode::Timer,
        )).unwrap();
        let s = &db.list_sessions().unwrap()[0].1;
        assert_eq!(s.notes.as_deref(), Some("from B"),
            "DEVICE_B is lex-larger than DEVICE_A; B's update wins on tie");
    }

    #[test]
    fn apply_event_records_the_event_in_the_log() {
        // After apply_event, the event must be in the events table so
        // future recomputes see it. Sync's push phase will pick it up
        // via pending_events (since `synced=0` by default).
        let db = Database::open_in_memory().unwrap();
        let event = synth_session_insert(
            SESSION_X, 5, DEVICE_A,
            "2026-04-30T10:00:00", 600,
            None, None, SessionMode::Timer,
        );
        let event_uuid = event.event_uuid.clone();
        db.apply_event(&event).unwrap();
        let pending = db.pending_events().unwrap();
        assert!(pending.iter().any(|(_, e)| e.event_uuid == event_uuid),
            "applied event must appear in events table");
    }

    #[test]
    fn apply_event_with_unknown_kind_is_a_silent_record_only() {
        // Forwards-compat: a future event kind we don't understand must
        // not panic or error. Record it — a future build can replay —
        // but don't try to mutate the cache from it.
        let db = Database::open_in_memory().unwrap();
        let weird = synth_event(
            "future_kind_not_yet_invented",
            SESSION_X, 5, DEVICE_A,
            serde_json::json!({"some": "future-data"}),
        );
        db.apply_event(&weird).unwrap();
        // Cache is empty (the event affected nothing it understood),
        // but the event was recorded.
        assert!(db.list_sessions().unwrap().is_empty());
        assert_eq!(db.pending_events().unwrap().len(), 1);
    }

    #[test]
    fn apply_event_session_insert_resolves_label_uuid_to_local_label_id() {
        // The peer's event references a label by label_uuid. If we have
        // a local label with that uuid, the materialized session must
        // link to it via local label_id. (Ensures cross-device
        // referential integrity survives the rowid-to-uuid translation.)
        let db = Database::open_in_memory().unwrap();
        let local_label_id = db.insert_label("Morning").unwrap();
        let label_uuid = db.list_labels().unwrap()[0].uuid.clone();
        drain_events(&db);

        db.apply_event(&synth_session_insert(
            SESSION_X, 5, DEVICE_A,
            "2026-04-30T10:00:00", 600,
            Some(label_uuid.as_str()), None, SessionMode::Timer,
        )).unwrap();
        let s = &db.list_sessions().unwrap()[0].1;
        assert_eq!(s.label_id, Some(local_label_id),
            "label_uuid must round-trip back to the local label_id");
    }

    // ── Event emission on mutations (A3) ─────────────────────────────────────
    //
    // Every state-changing operation appends a self-contained event to
    // `events` so peers can replay it. The local DB (`sessions`,
    // `labels`, `settings`) is the materialized cache derived from
    // those events; if the cache and the log disagree, the log wins on
    // every other device.


    // ── A3.1: insert_session emits a session_insert event ────────────────────

    #[test]
    fn insert_session_appends_exactly_one_session_insert_event() {
        let db = Database::open_in_memory().unwrap();
        db.insert_session(&Session {
            start_iso: "2026-04-30T10:00:00".to_string(),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
        }).unwrap();
        let events = db.pending_events().unwrap();
        assert_eq!(events.len(), 1, "one insert must produce exactly one event");
        assert_eq!(events[0].1.kind, "session_insert");
    }

    #[test]
    fn session_insert_event_payload_contains_the_rows_uuid() {
        // The event's session uuid must match the row's uuid — that's
        // how peers cross-reference events to materialized rows.
        let db = Database::open_in_memory().unwrap();
        db.insert_session(&Session {
            start_iso: "2026-04-30T10:00:00".to_string(),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
        }).unwrap();
        let row_uuid = db.list_sessions().unwrap()[0].1.uuid.clone();
        let events = db.pending_events().unwrap();
        let payload = event_payload(&events[0].1);
        assert_eq!(payload["uuid"], serde_json::Value::String(row_uuid.0));
    }

    #[test]
    fn session_insert_event_payload_carries_every_relevant_field() {
        // Every column that a peer needs to reconstruct the row must be
        // present in the payload — start_iso, duration_secs, notes, mode.
        // label_uuid is null here (label_id is None); covered separately
        // when the session does have a label.
        let db = Database::open_in_memory().unwrap();
        db.insert_session(&Session {
            start_iso: "2026-04-30T10:00:00".to_string(),
            duration_secs: 1234,
            label_id: None,
            notes: Some("note text".to_string()),
            mode: SessionMode::BoxBreath,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
        }).unwrap();
        let payload = event_payload(&db.pending_events().unwrap()[0].1);
        assert_eq!(payload["start_iso"], "2026-04-30T10:00:00");
        assert_eq!(payload["duration_secs"], 1234);
        assert_eq!(payload["notes"], "note text");
        assert_eq!(payload["mode"], "box_breath");
        assert_eq!(payload["label_uuid"], serde_json::Value::Null);
    }

    #[test]
    fn session_insert_event_payload_label_uuid_resolves_from_label_id() {
        // sessions reference labels by rowid locally, but the event must
        // carry the label's UUID — the cross-device identity. The
        // resolution `label_id → label_uuid` happens at event-emission
        // time so a peer can apply the event without needing this
        // device's rowid space.
        let db = Database::open_in_memory().unwrap();
        let label_id = db.insert_label("Morning").unwrap();
        let label_uuid = db.list_labels().unwrap()[0].uuid.clone();
        // insert_label also emits an event — drain it before the session
        // insert so we can assert on a single event below.
        for (id, _) in db.pending_events().unwrap() {
            db.mark_events_synced(&[id]).unwrap();
        }
        db.insert_session(&Session {
            start_iso: "2026-04-30T10:00:00".to_string(),
            duration_secs: 600,
            label_id: Some(label_id),
            notes: None,
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
        }).unwrap();
        let payload = event_payload(&db.pending_events().unwrap()[0].1);
        assert_eq!(payload["label_uuid"], serde_json::Value::String(label_uuid.0));
    }

    #[test]
    fn session_insert_event_payload_serializes_notes_null_when_absent() {
        // `notes: None` round-trips through the payload as JSON null —
        // not an empty string, which would lose the "no notes" vs "empty
        // notes" distinction on a peer that re-applies the event.
        let db = Database::open_in_memory().unwrap();
        db.insert_session(&Session {
            start_iso: "2026-04-30T10:00:00".to_string(),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
        }).unwrap();
        let payload = event_payload(&db.pending_events().unwrap()[0].1);
        assert_eq!(payload["notes"], serde_json::Value::Null);
    }

    #[test]
    fn session_insert_event_carries_this_devices_id() {
        let db = Database::open_in_memory().unwrap();
        let device_id = db.device_id().unwrap();
        db.insert_session(&Session {
            start_iso: "2026-04-30T10:00:00".to_string(),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
        }).unwrap();
        let events = db.pending_events().unwrap();
        assert_eq!(events[0].1.device_id, device_id,
            "event must be attributed to the authoring device");
    }

    #[test]
    fn session_insert_event_advances_the_lamport_clock() {
        // Bumping the clock on every authored event is what gives the
        // log a total order. After one insert, lamport must be ≥ 1; the
        // event's own ts must equal that bumped value.
        let db = Database::open_in_memory().unwrap();
        assert_eq!(db.lamport_clock().unwrap(), 0);
        db.insert_session(&Session {
            start_iso: "2026-04-30T10:00:00".to_string(),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
        }).unwrap();
        let lamport = db.lamport_clock().unwrap();
        assert!(lamport >= 1, "lamport must advance past zero");
        let events = db.pending_events().unwrap();
        assert_eq!(events[0].1.lamport_ts, lamport,
            "event ts must equal the post-bump clock value");
    }

    #[test]
    fn two_inserts_produce_two_distinct_events_in_lamport_order() {
        let db = Database::open_in_memory().unwrap();
        for i in 0..2 {
            db.insert_session(&Session {
                start_iso: format!("2026-04-3{}T10:00:00", i),
                duration_secs: 600,
                label_id: None,
                notes: None,
                mode: SessionMode::Timer,
                uuid: crate::db::SessionUuid::new(""),
                guided_file_uuid: None,
            }).unwrap();
        }
        let events = db.pending_events().unwrap();
        assert_eq!(events.len(), 2);
        assert!(events[0].1.lamport_ts < events[1].1.lamport_ts,
            "events must be sorted ASC by lamport_ts");
        assert_ne!(events[0].1.event_uuid, events[1].1.event_uuid);
    }


    // ── A3.2: update_session and delete_session emit events ──────────────────

    #[test]
    fn update_session_appends_a_session_update_event() {
        let db = Database::open_in_memory().unwrap();
        let id = db.insert_session(&Session {
            start_iso: "2026-04-30T10:00:00".to_string(),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
        }).unwrap();
        drain_events(&db);

        db.update_session(id, &Session {
            start_iso: "2026-05-01T11:00:00".to_string(),
            duration_secs: 1800,
            label_id: None,
            notes: Some("revised".to_string()),
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
        }).unwrap();
        let events = db.pending_events().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].1.kind, "session_update");
    }

    #[test]
    fn session_update_event_payload_carries_the_rows_uuid_unchanged() {
        // The session's uuid is stable — update changes every other field
        // but the cross-device identity of the session is fixed at insert
        // time. The event must reference that same uuid so peers can
        // locate the row to update.
        let db = Database::open_in_memory().unwrap();
        let id = db.insert_session(&Session {
            start_iso: "2026-04-30T10:00:00".to_string(),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
        }).unwrap();
        let original_uuid = db.list_sessions().unwrap()[0].1.uuid.clone();
        drain_events(&db);

        db.update_session(id, &Session {
            start_iso: "2026-05-01T11:00:00".to_string(),
            duration_secs: 1800,
            label_id: None,
            notes: Some("revised".to_string()),
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
        }).unwrap();
        let payload = event_payload(&db.pending_events().unwrap()[0].1);
        assert_eq!(payload["uuid"], serde_json::Value::String(original_uuid.0));
    }

    #[test]
    fn session_update_event_payload_reflects_the_new_field_values() {
        let db = Database::open_in_memory().unwrap();
        let id = db.insert_session(&Session {
            start_iso: "2026-04-30T10:00:00".to_string(),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
        }).unwrap();
        drain_events(&db);

        db.update_session(id, &Session {
            start_iso: "2026-05-01T11:00:00".to_string(),
            duration_secs: 1800,
            label_id: None,
            notes: Some("revised".to_string()),
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
        }).unwrap();
        let payload = event_payload(&db.pending_events().unwrap()[0].1);
        assert_eq!(payload["start_iso"], "2026-05-01T11:00:00");
        assert_eq!(payload["duration_secs"], 1800);
        assert_eq!(payload["notes"], "revised");
        assert_eq!(payload["mode"], "timer");
    }

    #[test]
    fn session_update_event_payload_label_uuid_resolves_from_new_label() {
        // Updates can change the label — the event payload must reflect
        // the *new* label's uuid, not the old one or the rowid.
        let db = Database::open_in_memory().unwrap();
        let label_id = db.insert_label("Evening").unwrap();
        let label_uuid = db.list_labels().unwrap()[0].uuid.clone();
        let id = db.insert_session(&Session {
            start_iso: "2026-04-30T10:00:00".to_string(),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
        }).unwrap();
        drain_events(&db);

        db.update_session(id, &Session {
            start_iso: "2026-04-30T10:00:00".to_string(),
            duration_secs: 600,
            label_id: Some(label_id),
            notes: None,
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
        }).unwrap();
        let payload = event_payload(&db.pending_events().unwrap()[0].1);
        assert_eq!(payload["label_uuid"], serde_json::Value::String(label_uuid.0));
    }

    #[test]
    fn update_session_unknown_id_emits_no_event() {
        // Defensive: an UPDATE that affects zero rows must NOT log a
        // ghost event referencing a uuid we don't know. Otherwise peers
        // would receive an update for a session they've never seen.
        let db = Database::open_in_memory().unwrap();
        drain_events(&db);
        db.update_session(9999, &Session {
            start_iso: "2026-05-01T11:00:00".to_string(),
            duration_secs: 1800,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
        }).unwrap();
        assert!(db.pending_events().unwrap().is_empty(),
            "no-match update must produce no event");
    }

    #[test]
    fn delete_session_appends_a_session_delete_event() {
        let db = Database::open_in_memory().unwrap();
        let id = db.insert_session(&Session {
            start_iso: "2026-04-30T10:00:00".to_string(),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
        }).unwrap();
        let row_uuid = db.list_sessions().unwrap()[0].1.uuid.clone();
        drain_events(&db);

        db.delete_session(id).unwrap();
        let events = db.pending_events().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].1.kind, "session_delete");

        // Payload is just the uuid — peers don't need any other field
        // since the tombstone semantics is "drop the row by this id".
        let payload = event_payload(&events[0].1);
        assert_eq!(payload["uuid"], serde_json::Value::String(row_uuid.0));
    }

    #[test]
    fn delete_session_unknown_id_emits_no_event() {
        let db = Database::open_in_memory().unwrap();
        drain_events(&db);
        db.delete_session(9999).unwrap();
        assert!(db.pending_events().unwrap().is_empty(),
            "no-match delete must produce no event");
    }

    // ── A3.3: bulk operations emit one event per row ─────────────────────────

    #[test]
    fn bulk_insert_sessions_emits_one_event_per_row() {
        // Each row crosses the network as its own SessionInserted event —
        // the cross-device replay model has no concept of "bulk insert",
        // every row is independent. So N inputs must yield N events.
        let db = Database::open_in_memory().unwrap();
        let to_insert: Vec<Session> = (0..3).map(|i| Session {
            start_iso: format!("2026-04-3{i}T10:00:00"),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
        }).collect();
        db.bulk_insert_sessions(&to_insert).unwrap();
        let events = db.pending_events().unwrap();
        assert_eq!(events.len(), 3,
            "three input rows must yield three events");
        for (_, e) in &events {
            assert_eq!(e.kind, "session_insert");
        }
    }

    #[test]
    fn bulk_insert_sessions_event_uuids_match_inserted_rows() {
        // Each event's session uuid must correspond to a stored row's
        // uuid — the set must be equal. Otherwise a peer would receive
        // events for rows we don't have, or skip rows we do.
        let db = Database::open_in_memory().unwrap();
        let to_insert: Vec<Session> = (0..3).map(|i| Session {
            start_iso: format!("2026-04-3{i}T10:00:00"),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
        }).collect();
        db.bulk_insert_sessions(&to_insert).unwrap();
        let row_uuids: std::collections::HashSet<String> = db.list_sessions()
            .unwrap()
            .iter().map(|(_, s)| s.uuid.0.clone()).collect();
        let event_uuids: std::collections::HashSet<String> = db
            .pending_events()
            .unwrap()
            .iter()
            .map(|(_, e)| event_payload(e)["uuid"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(row_uuids, event_uuids,
            "every stored row must have a matching event, and vice versa");
    }

    #[test]
    fn bulk_insert_sessions_with_empty_slice_emits_no_events() {
        let db = Database::open_in_memory().unwrap();
        drain_events(&db);
        db.bulk_insert_sessions(&[]).unwrap();
        assert!(db.pending_events().unwrap().is_empty());
    }

    #[test]
    fn bulk_insert_session_events_have_strictly_increasing_lamport_ts() {
        // Replay order is determined by lamport_ts. Even within a bulk
        // op, each row gets its own ts so peers can apply them in a
        // consistent order across devices.
        let db = Database::open_in_memory().unwrap();
        let to_insert: Vec<Session> = (0..3).map(|i| Session {
            start_iso: format!("2026-04-3{i}T10:00:00"),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
        }).collect();
        db.bulk_insert_sessions(&to_insert).unwrap();
        let timestamps: Vec<i64> = db.pending_events().unwrap()
            .iter().map(|(_, e)| e.lamport_ts).collect();
        let mut sorted = timestamps.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(timestamps.len(), sorted.len(),
            "every bulk-inserted event must have a unique lamport_ts: {timestamps:?}");
        assert_eq!(timestamps, sorted,
            "events must be returned in ascending lamport_ts order");
    }

    #[test]
    fn delete_all_sessions_emits_one_delete_event_per_existing_row() {
        let db = Database::open_in_memory().unwrap();
        for i in 0..3 {
            db.insert_session(&Session {
                start_iso: format!("2026-04-3{i}T10:00:00"),
                duration_secs: 600,
                label_id: None,
                notes: None,
                mode: SessionMode::Timer,
                uuid: crate::db::SessionUuid::new(""),
                guided_file_uuid: None,
            }).unwrap();
        }
        let row_uuids: std::collections::HashSet<String> = db.list_sessions()
            .unwrap()
            .iter().map(|(_, s)| s.uuid.0.clone()).collect();
        drain_events(&db);

        let removed = db.delete_all_sessions().unwrap();
        assert_eq!(removed, 3);

        let events = db.pending_events().unwrap();
        assert_eq!(events.len(), 3,
            "delete_all must emit one delete event per row that was present");
        for (_, e) in &events {
            assert_eq!(e.kind, "session_delete");
        }
        let event_uuids: std::collections::HashSet<String> = events.iter()
            .map(|(_, e)| event_payload(e)["uuid"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(row_uuids, event_uuids,
            "every previously-present row must show up in a tombstone event");
    }

    #[test]
    fn delete_all_sessions_on_empty_database_emits_no_events() {
        let db = Database::open_in_memory().unwrap();
        drain_events(&db);
        let removed = db.delete_all_sessions().unwrap();
        assert_eq!(removed, 0);
        assert!(db.pending_events().unwrap().is_empty());
    }

    // ── SessionMode serialization ─────────────────────────────────────────────

    #[test]
    fn session_mode_as_db_str_returns_canonical_strings() {
        // These are the values that go into the sessions.mode column AND
        // the CSV mode column — pinning them so a refactor that quietly
        // changes one (e.g. 'box_breath' → 'breath') gets caught.
        assert_eq!(SessionMode::Timer.as_db_str(), "timer");
        assert_eq!(SessionMode::BoxBreath.as_db_str(), "box_breath");
        assert_eq!(SessionMode::Guided.as_db_str(), "guided");
    }

    #[test]
    fn session_mode_from_db_str_parses_canonical_strings() {
        assert_eq!(SessionMode::from_db_str("timer"), Some(SessionMode::Timer));
        assert_eq!(SessionMode::from_db_str("box_breath"), Some(SessionMode::BoxBreath));
        assert_eq!(SessionMode::from_db_str("guided"), Some(SessionMode::Guided));
    }

    #[test]
    fn session_mode_from_db_str_returns_none_for_unknown() {
        // No legacy fallback — "countdown" and "stopwatch" deliberately
        // map to None. Callers decide what to do (existing data_io /
        // log paths default to Timer via unwrap_or, which makes legacy
        // rows readable without us adding a compat shim).
        assert_eq!(SessionMode::from_db_str(""), None);
        assert_eq!(SessionMode::from_db_str("countdown"), None);
        assert_eq!(SessionMode::from_db_str("stopwatch"), None);
        assert_eq!(SessionMode::from_db_str("TIMER"), None);  // case-sensitive
        assert_eq!(SessionMode::from_db_str("breathing"), None);  // old name
        assert_eq!(SessionMode::from_db_str("box-breath"), None); // dash, not underscore
        assert_eq!(SessionMode::from_db_str("Guided"), None);     // case-sensitive
        assert_eq!(SessionMode::from_db_str("garbage"), None);
    }

    #[test]
    fn session_mode_db_str_round_trip() {
        for &mode in &[SessionMode::Timer, SessionMode::BoxBreath, SessionMode::Guided] {
            assert_eq!(SessionMode::from_db_str(mode.as_db_str()), Some(mode));
        }
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
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
        }).unwrap();
        db.insert_session(&Session {
            start_iso: "2026-04-28T07:00:00".to_string(),
            duration_secs: 300, label_id: Some(morning), notes: None,
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
        }).unwrap();
        // Evening: 1 session, 1200s total — larger total, should sort first.
        db.insert_session(&Session {
            start_iso: "2026-04-27T20:00:00".to_string(),
            duration_secs: 1200, label_id: Some(evening), notes: None,
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
        }).unwrap();
        // Unlabeled session — must NOT appear.
        db.insert_session(&Session {
            start_iso: "2026-04-27T12:00:00".to_string(),
            duration_secs: 500, label_id: None, notes: None,
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
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
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
        }).unwrap();
        db.insert_session(&Session {
            start_iso: "2026-04-28T12:00:00".to_string(),
            duration_secs: 600, label_id: Some(alpha), notes: None,
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
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
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
        }).unwrap();
        db.insert_session(&Session {
            start_iso: "2026-04-28T10:00:00".to_string(),
            duration_secs: 45, label_id: Some(lid), notes: None,
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
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
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
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
                mode: SessionMode::Timer,
                uuid: crate::db::SessionUuid::new(""),
                guided_file_uuid: None,
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
                mode: SessionMode::Timer,
                uuid: crate::db::SessionUuid::new(""),
                guided_file_uuid: None,
            }).unwrap();
        }
        for d in 5..=6 {
            db.insert_session(&Session {
                start_iso: format!("2026-03-{d:02}T10:00:00"),
                duration_secs: 600, label_id: None, notes: None,
                mode: SessionMode::Timer,
                uuid: crate::db::SessionUuid::new(""),
                guided_file_uuid: None,
            }).unwrap();
        }
        db.insert_session(&Session {
            start_iso: "2025-12-25T10:00:00".to_string(),
            duration_secs: 600, label_id: None, notes: None,
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
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
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
        }).unwrap();
        db.insert_session(&Session {
            start_iso: "2025-12-15T10:00:00".to_string(),
            duration_secs: 600, label_id: None, notes: None,
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
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
                mode: SessionMode::Timer,
                uuid: crate::db::SessionUuid::new(""),
                guided_file_uuid: None,
            }).unwrap();
        }
        db.insert_session(&Session {
            start_iso: "2026-04-12T10:00:00".to_string(),
            duration_secs: 600, label_id: None, notes: None,
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
        }).unwrap();
        db.insert_session(&Session {
            start_iso: "2026-04-28T10:00:00".to_string(),
            duration_secs: 600, label_id: None, notes: None,
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
        }).unwrap();
        // A session in March — must NOT appear in April's days.
        db.insert_session(&Session {
            start_iso: "2026-03-15T10:00:00".to_string(),
            duration_secs: 600, label_id: None, notes: None,
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
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
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
        }).unwrap();
        // Jan 1 next year — must NOT contribute.
        db.insert_session(&Session {
            start_iso: "2027-01-01T00:30:00".to_string(),
            duration_secs: 600, label_id: None, notes: None,
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
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
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
        }).unwrap();
        // April 1, midnight — INCLUDED in April.
        db.insert_session(&Session {
            start_iso: "2026-04-01T00:00:00".to_string(),
            duration_secs: 600, label_id: None, notes: None,
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
        }).unwrap();
        // April 30, late evening — INCLUDED.
        db.insert_session(&Session {
            start_iso: "2026-04-30T23:59:59".to_string(),
            duration_secs: 1200, label_id: None, notes: None,
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
        }).unwrap();
        // May 1, midnight — EXCLUDED.
        db.insert_session(&Session {
            start_iso: "2026-05-01T00:00:00".to_string(),
            duration_secs: 8888, label_id: None, notes: None,
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
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
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
        }).unwrap();
        // Jan 1, 2027 — must NOT count toward Dec 2026.
        db.insert_session(&Session {
            start_iso: "2027-01-01T00:00:00".to_string(),
            duration_secs: 9999, label_id: None, notes: None,
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
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
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
        }).unwrap();
        // Later that day.
        db.insert_session(&Session {
            start_iso: "2026-04-27T18:00:00".to_string(),
            duration_secs: 1200, label_id: None, notes: None,
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
        }).unwrap();
        // Following day.
        db.insert_session(&Session {
            start_iso: "2026-04-28T10:00:00".to_string(),
            duration_secs: 300, label_id: None, notes: None,
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
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
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
        }).unwrap();
        // On / after cut-off — counted.
        db.insert_session(&Session {
            start_iso: "2026-04-27T00:00:00".to_string(),
            duration_secs: 600, label_id: None, notes: None,
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
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
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
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
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
        };
        let id = db.insert_session(&session).unwrap();
        let (got_id, got) = db.get_longest_session().unwrap().unwrap();
        assert!(looks_like_uuid_v4(got.uuid.as_str()),
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
                mode: SessionMode::Timer,
                uuid: crate::db::SessionUuid::new(""),
                guided_file_uuid: None,
            }).unwrap();
        }
        let mut longest_session = Session {
            start_iso: "2026-04-30T20:00:00Z".to_string(),
            duration_secs: 3600,
            label_id: None,
            notes: Some("the long one".to_string()),
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
        };
        let longest_id = db.insert_session(&longest_session).unwrap();
        // Add one more shorter after — the order of insertion must not
        // affect which row wins.
        db.insert_session(&Session {
            start_iso: "2026-05-01T10:00:00Z".to_string(),
            duration_secs: 700,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
        }).unwrap();

        let (got_id, got) = db.get_longest_session().unwrap().unwrap();
        assert!(looks_like_uuid_v4(got.uuid.as_str()));
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
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
        }).unwrap();
        db.insert_session(&Session {
            start_iso: "2026-04-27T11:00:00Z".to_string(),
            duration_secs: 1245, label_id: None, notes: None,
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
        }).unwrap();
        // Sub-minute remainder must NOT be lost — the whole point of
        // having a seconds aggregate alongside total_minutes.
        db.insert_session(&Session {
            start_iso: "2026-04-27T12:00:00Z".to_string(),
            duration_secs: 17, label_id: None, notes: None,
            mode: SessionMode::BoxBreath,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
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
                mode: SessionMode::Timer,
                uuid: crate::db::SessionUuid::new(""),
                guided_file_uuid: None,
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
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
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
                mode: SessionMode::Timer,
                uuid: crate::db::SessionUuid::new(""),
                guided_file_uuid: None,
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
                mode: SessionMode::Timer,
                uuid: crate::db::SessionUuid::new(""),
                guided_file_uuid: None,
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
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
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
            notes: None, mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
        }).unwrap();
        db.insert_session(&Session {
            start_iso: "2026-04-27T11:00:00Z".to_string(),
            duration_secs: 600, label_id: Some(morning),
            notes: None, mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
        }).unwrap();
        db.insert_session(&Session {
            start_iso: "2026-04-27T19:00:00Z".to_string(),
            duration_secs: 600, label_id: Some(evening),
            notes: None, mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
        }).unwrap();
        db.insert_session(&Session {
            start_iso: "2026-04-27T20:00:00Z".to_string(),
            duration_secs: 600, label_id: None,
            notes: None, mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
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
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
        }).unwrap();
        // Without note (None).
        db.insert_session(&Session {
            start_iso: "2026-04-27T11:00:00Z".to_string(),
            duration_secs: 600, label_id: None,
            notes: None, mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
        }).unwrap();
        // Empty-string note — also excluded.
        db.insert_session(&Session {
            start_iso: "2026-04-27T12:00:00Z".to_string(),
            duration_secs: 600, label_id: None,
            notes: Some("".to_string()),
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
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
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
        }).unwrap();
        // Morning, no note → dropped (notes filter).
        db.insert_session(&Session {
            start_iso: "2026-04-27T11:00:00Z".to_string(),
            duration_secs: 600, label_id: Some(morning),
            notes: None, mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
        }).unwrap();
        // No label, with note → dropped (label filter).
        db.insert_session(&Session {
            start_iso: "2026-04-27T12:00:00Z".to_string(),
            duration_secs: 600, label_id: None,
            notes: Some("orphan".to_string()),
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
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
                notes: None, mode: SessionMode::Timer,
                uuid: crate::db::SessionUuid::new(""),
                guided_file_uuid: None,
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
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
        };
        db.insert_session(&session).unwrap();
        assert_eq!(db.count_sessions().unwrap(), 1);
    }

    #[test]
    fn insert_session_with_mode_guided_is_accepted_by_check_constraint() {
        // Sessions saved at the end of a guided meditation carry
        // mode='guided'. The schema's CHECK clause must accept it
        // alongside 'timer' and 'box_breath' or insert fails.
        let db = Database::open_in_memory().unwrap();
        let session = Session {
            start_iso: "2026-05-05T20:30:00Z".to_string(),
            duration_secs: 1200,
            label_id: None,
            notes: None,
            mode: SessionMode::Guided,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
        };
        db.insert_session(&session).unwrap();
        assert_eq!(db.count_sessions().unwrap(), 1);
    }

    #[test]
    fn insert_session_with_guided_file_uuid_round_trips() {
        // A guided session that played a starred imported file carries
        // the file's uuid so the log / stats can show per-file aggregates
        // later. Verifies the column is actually persisted + read back.
        let db = Database::open_in_memory().unwrap();
        let file_uuid = "deadbeef-1234-5678-9abc-def012345678";
        let session = Session {
            start_iso: "2026-05-05T20:30:00Z".to_string(),
            duration_secs: 1200,
            label_id: None,
            notes: None,
            mode: SessionMode::Guided,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: Some(file_uuid.into()),
        };
        db.insert_session(&session).unwrap();
        let rows = db.query_sessions(&SessionFilter::default()).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].1.guided_file_uuid.as_ref().map(|u| u.as_str()), Some(file_uuid));
    }

    #[test]
    fn insert_session_without_guided_file_uuid_round_trips_as_none() {
        // Transient one-off guided sessions don't reference a
        // library-stored file; the column must accept NULL.
        let db = Database::open_in_memory().unwrap();
        let session = Session {
            start_iso: "2026-05-05T21:00:00Z".to_string(),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Guided,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
        };
        db.insert_session(&session).unwrap();
        let rows = db.query_sessions(&SessionFilter::default()).unwrap();
        assert_eq!(rows.len(), 1);
        assert!(rows[0].1.guided_file_uuid.is_none());
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
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
        };
        let unlabeled = Session {
            start_iso: "2026-04-27T19:00:00Z".to_string(),
            duration_secs: 300,
            label_id: None,
            notes: None,
            mode: SessionMode::BoxBreath,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
        };
        let labeled_id = db.insert_session(&labeled).unwrap();
        db.insert_session(&unlabeled).unwrap();
        let rows = db.list_sessions_for_label(morning).unwrap();
        assert_eq!(rows.len(), 1, "only the labeled session must be returned");
        assert!(looks_like_uuid_v4(rows[0].1.uuid.as_str()));
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
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
        };
        let id = db.insert_session(&session).unwrap();
        let rows = db.list_sessions().unwrap();
        assert_eq!(rows.len(), 1);
        assert!(looks_like_uuid_v4(rows[0].1.uuid.as_str()),
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
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
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
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
        };
        let id = db.insert_session(&original).unwrap();

        // Insert a sibling that must remain untouched.
        let other_id = db.insert_session(&Session {
            start_iso: "2026-04-27T11:00:00Z".to_string(),
            duration_secs: 300,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
        }).unwrap();

        db.insert_label("Evening").unwrap();
        let evening = db.find_label_by_name("Evening").unwrap().unwrap();
        let mut updated = Session {
            start_iso: "2026-04-28T19:00:00Z".to_string(),
            duration_secs: 1500,
            label_id: Some(evening),
            notes: Some("after dinner".to_string()),
            mode: SessionMode::BoxBreath,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
        };
        db.update_session(id, &updated).unwrap();

        let rows = db.list_sessions().unwrap();
        assert_eq!(rows.len(), 2);
        // Updated row reflects every new field. Its uuid is whatever the
        // DB assigned at insert time and must survive an update unchanged
        // — bind it into `updated.uuid` for the full struct comparison.
        let updated_row = rows.iter().find(|(rid, _)| *rid == id).unwrap();
        assert!(looks_like_uuid_v4(updated_row.1.uuid.as_str()));
        updated.uuid = updated_row.1.uuid.clone();
        assert_eq!(updated_row.1, updated);
        // Sibling row untouched.
        let other_row = rows.iter().find(|(rid, _)| *rid == other_id).unwrap();
        assert_eq!(other_row.1.start_iso, "2026-04-27T11:00:00Z");
        assert_eq!(other_row.1.duration_secs, 300);
        assert_eq!(other_row.1.mode, SessionMode::Timer);
        // Each row must carry its own distinct uuid.
        assert!(looks_like_uuid_v4(other_row.1.uuid.as_str()));
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
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
        }).unwrap();
        db.update_session(id, &Session {
            start_iso: "2026-04-27T10:00:00Z".to_string(),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
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
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
        }).unwrap();
        db.update_session(id + 999, &Session {
            start_iso: "2099-01-01T00:00:00Z".to_string(),
            duration_secs: 9999,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
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
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
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
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
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
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
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
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
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
                mode: SessionMode::Timer,
                uuid: crate::db::SessionUuid::new(""),
                guided_file_uuid: None,
            },
            Session {
                start_iso: "2026-04-27T11:00:00Z".to_string(),
                duration_secs: 1200,
                label_id: None,
                notes: None,
                mode: SessionMode::Timer,
                uuid: crate::db::SessionUuid::new(""),
                guided_file_uuid: None,
            },
            Session {
                start_iso: "2026-04-27T12:00:00Z".to_string(),
                duration_secs: 300,
                label_id: Some(morning),
                notes: None,
                mode: SessionMode::BoxBreath,
                uuid: crate::db::SessionUuid::new(""),
                guided_file_uuid: None,
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
            assert!(looks_like_uuid_v4(got.uuid.as_str()),
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
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
        }).unwrap();
        assert_eq!(db.count_sessions().unwrap(), 1);

        let bad_label = 9999i64; // No label has this id.
        let batch = vec![
            Session {
                start_iso: "2026-04-27T10:00:00Z".to_string(),
                duration_secs: 600,
                label_id: None, // OK
                notes: None,
                mode: SessionMode::Timer,
                uuid: crate::db::SessionUuid::new(""),
                guided_file_uuid: None,
            },
            Session {
                start_iso: "2026-04-27T11:00:00Z".to_string(),
                duration_secs: 600,
                label_id: Some(bad_label), // FK violation
                notes: None,
                mode: SessionMode::Timer,
                uuid: crate::db::SessionUuid::new(""),
                guided_file_uuid: None,
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
                mode: SessionMode::Timer,
                uuid: crate::db::SessionUuid::new(""),
                guided_file_uuid: None,
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
                mode: SessionMode::Timer,
                uuid: crate::db::SessionUuid::new(""),
                guided_file_uuid: None,
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
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
        };
        let id = db.insert_session(&labeled).unwrap();
        // Insert a second, unlabeled session — must not appear.
        db.insert_session(&Session {
            start_iso: "2026-04-27T11:00:00Z".to_string(),
            duration_secs: 300,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
        }).unwrap();
        let rows = db.list_sessions_for_label(morning).unwrap();
        assert_eq!(rows.len(), 1, "only the labeled session must be returned");
        assert!(looks_like_uuid_v4(rows[0].1.uuid.as_str()));
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
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
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
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
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
    fn streak_at_naive_date_min_does_not_panic() {
        // chrono's NaiveDate::MIN (year -262144) has no pred_opt;
        // the streak walk used to .expect() on it. Practically
        // unreachable on real data, but the Setup view hits the
        // streak path on every open — a panic there for any caller
        // synthesising a date (CSV import, fuzz, future tests)
        // would be a sharp edge. Verify the saturation path
        // returns 0 rather than panicking.
        let db = Database::open_in_memory().unwrap();
        // Without any sessions, returns 0 trivially.
        assert_eq!(db.get_streak(chrono::NaiveDate::MIN).unwrap(), 0);
        // Insert a session at MIN too — exercises the loop's
        // saturating pred path.
        db.insert_session(&session_on("-262144-01-01")).unwrap();
        let n = db.get_streak(chrono::NaiveDate::MIN).unwrap();
        assert!(n >= 0, "no panic, returns a valid count: got {n}");
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
    fn daily_totals_since_excludes_days_before_cutoff() {
        let db = Database::open_in_memory().unwrap();
        db.insert_session(&Session {
            duration_secs: 600,
            ..session_on("2026-04-24")
        }).unwrap();
        db.insert_session(&Session {
            duration_secs: 300,
            ..session_on("2026-04-26")
        }).unwrap();
        db.insert_session(&Session {
            duration_secs: 1200,
            ..session_on("2026-04-27")
        }).unwrap();
        assert_eq!(
            db.get_daily_totals_since(date(2026, 4, 26)).unwrap(),
            vec![(date(2026, 4, 26), 300), (date(2026, 4, 27), 1200)],
        );
    }

    #[test]
    fn daily_totals_since_includes_the_cutoff_day_itself() {
        let db = Database::open_in_memory().unwrap();
        db.insert_session(&Session {
            duration_secs: 600,
            ..session_on("2026-04-26")
        }).unwrap();
        assert_eq!(
            db.get_daily_totals_since(date(2026, 4, 26)).unwrap(),
            vec![(date(2026, 4, 26), 600)],
        );
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
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
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
    fn median_duration_is_none_for_empty_db() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(db.get_median_duration_secs().unwrap(), None);
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
        assert_eq!(db.get_median_duration_secs().unwrap(), Some(900));
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
        assert_eq!(db.get_median_duration_secs().unwrap(), Some(600));
    }

    #[test]
    fn csv_round_trips_sessions_with_labels() {
        let src = Database::open_in_memory().unwrap();
        src.insert_label("Morning").unwrap();
        let morning_id = src.find_label_by_name("Morning").unwrap();
        // Canonical naive-local ISO shape (no `Z`) — `unix_to_local_iso`
        // never emits one, and `import_sessions_csv` now rejects them.
        src.insert_session(&Session {
            start_iso: "2026-04-27T10:00:00".to_string(),
            duration_secs: 600,
            label_id: morning_id,
            notes: Some("clear, focused".to_string()), // comma forces CSV quoting
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
        })
        .unwrap();
        src.insert_session(&Session {
            start_iso: "2026-04-27T19:00:00".to_string(),
            duration_secs: 1200,
            label_id: None,
            notes: None,
            mode: SessionMode::BoxBreath,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
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
        assert!(looks_like_uuid_v4(sessions[0].1.uuid.as_str()));
        assert!(looks_like_uuid_v4(sessions[1].1.uuid.as_str()));
        assert_ne!(sessions[0].1.uuid, sessions[1].1.uuid);
        assert_eq!(
            sessions[0].1,
            Session {
                start_iso: "2026-04-27T10:00:00".to_string(),
                duration_secs: 600,
                label_id: dst_morning_id,
                notes: Some("clear, focused".to_string()),
                mode: SessionMode::Timer,
                uuid: sessions[0].1.uuid.clone(),
                guided_file_uuid: None,
            }
        );
        assert_eq!(
            sessions[1].1,
            Session {
                start_iso: "2026-04-27T19:00:00".to_string(),
                duration_secs: 1200,
                label_id: None,
                notes: None,
                mode: SessionMode::BoxBreath,
                uuid: sessions[1].1.uuid.clone(),
                guided_file_uuid: None,
            }
        );
    }

    #[test]
    fn import_sessions_csv_rejects_unparseable_start_iso() {
        // Without the validation gate, a row with `start_iso = "garbage"`
        // would persist; `daily_totals_filtered` would then silently
        // filter it out via `parse_from_str(...).ok()`, but
        // `count_sessions` would still see it — invisible drift between
        // stat surfaces.
        let db = Database::open_in_memory().unwrap();
        let csv = "start_iso,duration_secs,label,notes,mode\n\
                   not-a-date,600,,,timer\n";
        let err = db.import_sessions_csv(csv.as_bytes()).unwrap_err();
        assert!(
            matches!(&err, DbError::Decode(s) if s.contains("bad start_iso")),
            "expected DbError::Decode(\"bad start_iso: …\"), got {err:?}",
        );
        assert_eq!(db.list_sessions().unwrap().len(), 0,
            "rejected row must not have landed in the DB");
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
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
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
        assert!(csv.contains("timer"));
    }

    // ── UUIDs on sessions and labels (Nextcloud-Sync phase A1) ───────────────
    //
    // Every session and label row must carry a stable cross-device UUID.
    // The DB generates it at insert time — the value the caller puts in
    // the struct's `uuid` field is ignored. Reads round-trip the stored
    // UUID into the returned struct so the rest of the app (including
    // the future event log) can address rows by it.

    #[test]
    fn inserted_session_has_a_uuid_in_query_results() {
        let db = Database::open_in_memory().unwrap();
        db.insert_session(&Session {
            start_iso: "2026-04-27T10:00:00".to_string(),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
                        uuid: crate::db::SessionUuid::new(""),  // ignored — DB assigns
                        guided_file_uuid: None,
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
                mode: SessionMode::Timer,
                                uuid: crate::db::SessionUuid::new(""),
                                guided_file_uuid: None,
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
            mode: SessionMode::Timer,
                        uuid: crate::db::SessionUuid::new(""),
                        guided_file_uuid: None,
        })
        .unwrap();
        let uuid = &db.list_sessions().unwrap()[0].1.uuid;
        assert!(looks_like_uuid_v4(uuid.as_str()),
            "session uuid `{uuid}` doesn't match v4 shape");
    }

    #[test]
    fn caller_supplied_session_uuid_is_ignored_in_favour_of_a_fresh_one() {
        // Documents that uuid is DB-assigned, not caller-controlled.
        // Belt-and-braces: if a caller accidentally reuses a uuid string
        // the DB still produces fresh, unique values — no collision risk.
        let db = Database::open_in_memory().unwrap();
        let bogus: crate::db::SessionUuid = "00000000-0000-4000-8000-000000000000".into();
        for i in 0..2 {
            db.insert_session(&Session {
                start_iso: format!("2026-04-2{}T10:00:00", 7 + i),
                duration_secs: 600,
                label_id: None,
                notes: None,
                mode: SessionMode::Timer,
                                uuid: bogus.clone(),
                                guided_file_uuid: None,
            })
            .unwrap();
        }
        let rows = db.list_sessions().unwrap();
        assert_ne!(rows[0].1.uuid, bogus, "DB must override caller's uuid");
        assert_ne!(rows[1].1.uuid, bogus, "DB must override caller's uuid");
        assert_ne!(rows[0].1.uuid, rows[1].1.uuid);
    }
}
