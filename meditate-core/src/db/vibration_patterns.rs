//! `vibration_patterns` table — user-managed vibration envelope
//! library. Each row holds N equally-spaced amplitude samples plus
//! a chart_kind (Line / Bar interpolation). Bundled rows ship with
//! the app under stable UUIDs.

use rusqlite::{params, OptionalExtension};

use super::events::EventKind;
use super::{
    conflict_suffixed_name, is_unique_constraint_error, Database, DbError, Result,
};

/// One vibration pattern in the user's library. The pattern itself is
/// an envelope of N equally-spaced amplitude samples played back over
/// `duration_ms`; `chart_kind` selects how playback interpolates
/// between samples — `Line` linear-interpolates (smooth ramp), `Bar`
/// holds each sample (sample-and-hold step). Bundled rows ship with
/// the app under stable UUIDs so a peer device that already has the
/// bundle doesn't end up with duplicate rows after a sync round-trip.
/// `updated_iso` advances on every edit so chooser sorts by recency.
#[derive(Debug, Clone, PartialEq)]
pub struct VibrationPattern {
    pub id: i64,
    pub uuid: String,
    pub name: String,
    pub duration_ms: u32,
    /// Equally-spaced amplitude samples in `[0.0, 1.0]`. Length is the
    /// pattern's "Points" count from the editor; spacing between
    /// samples is `duration_ms / (intensities.len() - 1)`.
    pub intensities: Vec<f32>,
    pub chart_kind: ChartKind,
    pub is_bundled: bool,
    pub created_iso: String,
    pub updated_iso: String,
}

/// How playback interpolates between adjacent envelope samples.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChartKind {
    /// Linear interpolation between adjacent samples — produces a
    /// continuous, smoothly-ramping intensity curve.
    Line,
    /// Sample-and-hold — each sample's value is held for its segment
    /// length, producing abrupt step transitions at sample boundaries.
    Bar,
}

impl ChartKind {
    pub fn as_db_str(self) -> &'static str {
        match self {
            ChartKind::Line => "line",
            ChartKind::Bar  => "bar",
        }
    }
    pub fn from_db_str(s: &str) -> Option<Self> {
        match s {
            "line" => Some(ChartKind::Line),
            "bar"  => Some(ChartKind::Bar),
            _      => None,
        }
    }
}

impl Database {
    /// Insert a row keyed on `uuid_str`. Idempotent — a second call
    /// with the same uuid returns the existing rowid without touching
    /// the row or emitting another event. Returns
    /// `DuplicateVibrationPattern(name)` if a row with the same
    /// case-insensitive `name` already exists under a different uuid.
    pub fn insert_vibration_pattern_with_uuid(
        &self,
        uuid_str: &str,
        name: &str,
        duration_ms: u32,
        intensities: &[f32],
        chart_kind: ChartKind,
        is_bundled: bool,
    ) -> Result<i64> {
        let tx = self.conn.unchecked_transaction()?;
        if let Some(existing) = self.conn.query_row(
            "SELECT id FROM vibration_patterns WHERE uuid = ?1",
            params![uuid_str],
            |row| row.get::<_, i64>(0),
        ).optional()? {
            return Ok(existing);
        }
        let intensities_json = serde_json::to_string(intensities)
            .map_err(|e| DbError::Csv(format!("serialise intensities: {e}")))?;
        let now_iso = chrono::Utc::now().to_rfc3339();
        let result = self.conn.execute(
            "INSERT INTO vibration_patterns
                (uuid, name, duration_ms, intensities_json, chart_kind,
                 is_bundled, created_iso, updated_iso)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
            params![
                uuid_str,
                name,
                duration_ms,
                intensities_json,
                chart_kind.as_db_str(),
                is_bundled as i64,
                now_iso,
            ],
        );
        match result {
            Ok(_) => {}
            Err(rusqlite::Error::SqliteFailure(err, _))
                if err.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                return Err(DbError::DuplicateVibrationPattern(name.to_string()));
            }
            Err(e) => return Err(DbError::Sqlite(e)),
        }
        let rowid = self.conn.last_insert_rowid();
        let payload = serde_json::json!({
            "uuid": uuid_str,
            "name": name,
            "duration_ms": duration_ms,
            "intensities_json": intensities_json,
            "chart_kind": chart_kind.as_db_str(),
            "is_bundled": is_bundled,
            "created_iso": now_iso,
            "updated_iso": now_iso,
        }).to_string();
        self.emit_event(EventKind::VibrationPatternInsert, uuid_str, payload)?;
        tx.commit()?;
        Ok(rowid)
    }

    /// Insert a row with a freshly-minted UUID. Returns the new uuid
    /// so the caller (typically the editor's Save handler) can use it
    /// to drive selection in the chooser. Mirrors bell_sound's two-
    /// variant pattern, but returns the uuid (more useful for the
    /// editor flow) rather than the rowid.
    pub fn insert_vibration_pattern(
        &self,
        name: &str,
        duration_ms: u32,
        intensities: &[f32],
        chart_kind: ChartKind,
        is_bundled: bool,
    ) -> Result<String> {
        let uuid_str = uuid::Uuid::new_v4().to_string();
        self.insert_vibration_pattern_with_uuid(
            &uuid_str, name, duration_ms, intensities, chart_kind, is_bundled,
        )?;
        Ok(uuid_str)
    }

    pub fn find_vibration_pattern_by_uuid(
        &self,
        uuid_str: &str,
    ) -> Result<Option<VibrationPattern>> {
        let row = self.conn.query_row(
            "SELECT id, uuid, name, duration_ms, intensities_json,
                    chart_kind, is_bundled, created_iso, updated_iso
             FROM vibration_patterns WHERE uuid = ?1",
            params![uuid_str],
            |row| {
                let intensities_json: String = row.get(4)?;
                let chart_str: String = row.get(5)?;
                Ok(VibrationPattern {
                    id: row.get(0)?,
                    uuid: row.get(1)?,
                    name: row.get(2)?,
                    duration_ms: row.get::<_, i64>(3)? as u32,
                    intensities: serde_json::from_str(&intensities_json)
                        .unwrap_or_default(),
                    chart_kind: ChartKind::from_db_str(&chart_str)
                        .unwrap_or(ChartKind::Line),
                    is_bundled: row.get::<_, i64>(6)? != 0,
                    created_iso: row.get(7)?,
                    updated_iso: row.get(8)?,
                })
            },
        ).optional()?;
        Ok(row)
    }

    /// Focused rename: re-uses `update_vibration_pattern` internally
    /// but lets the shell skip the read-modify-write of all four
    /// mutable fields when only the name changes. Returns `Ok(false)`
    /// when the row doesn't exist OR `new_name == existing.name`
    /// (a no-op rename — the shell uses this to skip the undo toast).
    /// Returns `DuplicateVibrationPattern` if another row already
    /// holds the new name.
    pub fn rename_vibration_pattern(&self, uuid_str: &str, new_name: &str) -> Result<bool> {
        let existing: Option<(String, u32, String, String)> = self.conn.query_row(
            "SELECT name, duration_ms, intensities_json, chart_kind
               FROM vibration_patterns WHERE uuid = ?1",
            params![uuid_str],
            |row| Ok((
                row.get::<_, String>(0)?,
                row.get::<_, u32>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            )),
        ).optional()?;
        let Some((current_name, duration_ms, intensities_json, chart_kind_str)) = existing else {
            return Ok(false);
        };
        if current_name == new_name {
            return Ok(false);
        }
        let intensities: Vec<f32> = serde_json::from_str(&intensities_json)
            .map_err(|e| DbError::Csv(format!("deserialise intensities: {e}")))?;
        let chart_kind = ChartKind::from_db_str(&chart_kind_str)
            .ok_or_else(|| DbError::Csv(format!("bad chart_kind: {chart_kind_str}")))?;
        self.update_vibration_pattern(
            uuid_str, new_name, duration_ms, &intensities, chart_kind,
        )?;
        Ok(true)
    }

    /// Update the four mutable fields on an existing pattern. Bumps
    /// `updated_iso` so the recompute helper can resolve concurrent
    /// edits by lamport timestamp. Unknown uuids are silent no-ops AND
    /// emit no event (mirrors guided_file rename / bell_sound rename).
    /// Returns `DuplicateVibrationPattern(name)` if another row already
    /// holds the new name (case-insensitive).
    pub fn update_vibration_pattern(
        &self,
        uuid_str: &str,
        name: &str,
        duration_ms: u32,
        intensities: &[f32],
        chart_kind: ChartKind,
    ) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        let row: Option<(String, i64)> = self.conn.query_row(
            "SELECT created_iso, is_bundled FROM vibration_patterns WHERE uuid = ?1",
            params![uuid_str],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).optional()?;
        let Some((created_iso, is_bundled_int)) = row else {
            return Ok(());
        };
        let is_bundled = is_bundled_int != 0;
        let intensities_json = serde_json::to_string(intensities)
            .map_err(|e| DbError::Csv(format!("serialise intensities: {e}")))?;
        let now_iso = chrono::Utc::now().to_rfc3339();
        let result = self.conn.execute(
            "UPDATE vibration_patterns
             SET name = ?1, duration_ms = ?2, intensities_json = ?3,
                 chart_kind = ?4, updated_iso = ?5
             WHERE uuid = ?6",
            params![
                name, duration_ms, intensities_json,
                chart_kind.as_db_str(), now_iso, uuid_str,
            ],
        );
        match result {
            Ok(_) => {}
            Err(rusqlite::Error::SqliteFailure(err, _))
                if err.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                return Err(DbError::DuplicateVibrationPattern(name.to_string()));
            }
            Err(e) => return Err(DbError::Sqlite(e)),
        }
        let payload = serde_json::json!({
            "uuid": uuid_str,
            "name": name,
            "duration_ms": duration_ms,
            "intensities_json": intensities_json,
            "chart_kind": chart_kind.as_db_str(),
            "is_bundled": is_bundled,
            "created_iso": created_iso,
            "updated_iso": now_iso,
        }).to_string();
        self.emit_event(EventKind::VibrationPatternUpdate, uuid_str, payload)?;
        tx.commit()?;
        Ok(())
    }

    /// True iff a row other than `except_uuid` already holds `name`
    /// (case-insensitive). The editor and chooser use this for live
    /// validation; `except_uuid` is the row currently being renamed
    /// (or "" for fresh inserts) so the user's own case-only renames
    /// don't false-positive.
    pub fn is_vibration_pattern_name_taken(
        &self,
        name: &str,
        except_uuid: &str,
    ) -> Result<bool> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM vibration_patterns
              WHERE name = ?1 COLLATE NOCASE AND uuid != ?2",
            params![name, except_uuid],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    /// Drop a vibration-pattern row. Unknown uuids are silent no-ops
    /// AND emit no event — peers would otherwise see a tombstone for
    /// a row they never knew existed. Mirrors guided_file delete.
    pub fn delete_vibration_pattern(&self, uuid_str: &str) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        let exists: bool = self.conn.query_row(
            "SELECT 1 FROM vibration_patterns WHERE uuid = ?1",
            params![uuid_str],
            |_| Ok(true),
        ).optional()?.unwrap_or(false);
        if !exists { return Ok(()); }
        self.conn.execute(
            "DELETE FROM vibration_patterns WHERE uuid = ?1",
            params![uuid_str],
        )?;
        let payload = serde_json::json!({ "uuid": uuid_str }).to_string();
        self.emit_event(EventKind::VibrationPatternDelete, uuid_str, payload)?;
        tx.commit()?;
        Ok(())
    }

    pub fn list_vibration_patterns(&self) -> Result<Vec<VibrationPattern>> {
        // Custom rows first (is_bundled = 0), then the bundled seed
        // set. Mirrors list_bell_sounds — a freshly authored custom
        // pattern lives at the top of the chooser instead of being
        // pushed to the bottom of the bundled list.
        let mut stmt = self.conn.prepare(
            "SELECT id, uuid, name, duration_ms, intensities_json,
                    chart_kind, is_bundled, created_iso, updated_iso
             FROM vibration_patterns
             ORDER BY is_bundled ASC, id ASC",
        )?;
        let rows = stmt
            .query_map([], |row| {
                let intensities_json: String = row.get(4)?;
                let chart_str: String = row.get(5)?;
                Ok(VibrationPattern {
                    id: row.get(0)?,
                    uuid: row.get(1)?,
                    name: row.get(2)?,
                    duration_ms: row.get::<_, i64>(3)? as u32,
                    intensities: serde_json::from_str(&intensities_json)
                        .unwrap_or_default(),
                    chart_kind: ChartKind::from_db_str(&chart_str)
                        .unwrap_or(ChartKind::Line),
                    is_bundled: row.get::<_, i64>(6)? != 0,
                    created_iso: row.get(7)?,
                    updated_iso: row.get(8)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Recompute the `vibration_patterns` row for `pattern_uuid` from
    /// the events table. Same precedence rules as guided_files /
    /// presets / bell_sounds: tombstone wins on tie, else the highest-
    /// (lamport, device_id) mutate event drives the row. Update events
    /// carry every field so they self-suffice on out-of-order delivery.
    pub(super) fn recompute_vibration_pattern(&self, pattern_uuid: &str) -> Result<()> {
        let delete_ts: Option<i64> = self.conn.query_row(
            "SELECT MAX(lamport_ts) FROM events
             WHERE target_id = ?1 AND kind = 'vibration_pattern_delete'",
            params![pattern_uuid],
            |row| row.get::<_, Option<i64>>(0),
        )?;
        let mutate: Option<(i64, String)> = self.conn.query_row(
            "SELECT lamport_ts, payload FROM events
             WHERE target_id = ?1
               AND kind IN ('vibration_pattern_insert', 'vibration_pattern_update')
             ORDER BY lamport_ts DESC, device_id DESC
             LIMIT 1",
            params![pattern_uuid],
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
                    format!("vibration_pattern event payload not valid JSON: {e}")))?;
            let name = v["name"].as_str().unwrap_or_default();
            let duration_ms = v["duration_ms"].as_u64().unwrap_or(0) as u32;
            let intensities_json = v["intensities_json"].as_str().unwrap_or("[]");
            let chart_kind = v["chart_kind"].as_str().unwrap_or("line");
            let is_bundled = v["is_bundled"].as_bool().unwrap_or(false);
            let created_iso = v["created_iso"].as_str().unwrap_or_default();
            let updated_iso = v["updated_iso"].as_str().unwrap_or(created_iso);
            let upsert_sql = "INSERT INTO vibration_patterns
                    (uuid, name, duration_ms, intensities_json, chart_kind,
                     is_bundled, created_iso, updated_iso)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT(uuid) DO UPDATE SET
                    name             = excluded.name,
                    duration_ms      = excluded.duration_ms,
                    intensities_json = excluded.intensities_json,
                    chart_kind       = excluded.chart_kind,
                    is_bundled       = excluded.is_bundled,
                    created_iso      = excluded.created_iso,
                    updated_iso      = excluded.updated_iso";
            let first = self.conn.execute(upsert_sql, params![
                pattern_uuid, name, duration_ms, intensities_json,
                chart_kind, is_bundled as i64, created_iso, updated_iso,
            ]);
            match first {
                Ok(_) => {}
                Err(e) if is_unique_constraint_error(&e) => {
                    let suffixed = conflict_suffixed_name(name, pattern_uuid);
                    crate::diag::log(&format!(
                        "vibration_pattern_name_collision: uuid={pattern_uuid} \
                         original={name:?} resolved={suffixed:?}"
                    ));
                    self.conn.execute(upsert_sql, params![
                        pattern_uuid, suffixed, duration_ms, intensities_json,
                        chart_kind, is_bundled as i64, created_iso, updated_iso,
                    ])?;
                }
                Err(e) => return Err(DbError::Sqlite(e)),
            }
        } else {
            self.conn.execute(
                "DELETE FROM vibration_patterns WHERE uuid = ?1",
                params![pattern_uuid],
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{test_helpers::*, Event};

    #[test]
    fn chart_kind_round_trips_through_db_str() {
        for k in [ChartKind::Line, ChartKind::Bar] {
            assert_eq!(ChartKind::from_db_str(k.as_db_str()), Some(k));
        }
        assert_eq!(ChartKind::from_db_str("typo"), None);
    }

    #[test]
    fn list_vibration_patterns_is_empty_on_a_fresh_database() {
        let db = Database::open_in_memory().unwrap();
        assert!(db.list_vibration_patterns().unwrap().is_empty());
    }

    #[test]
    fn list_vibration_patterns_returns_custom_rows_before_bundled() {
        let db = Database::open_in_memory().unwrap();
        db.insert_vibration_pattern_with_uuid(
            "vp-bundled", "Pulse", 400, &[0.0, 1.0, 0.0],
            ChartKind::Line, true,
        ).unwrap();
        db.insert_vibration_pattern_with_uuid(
            "vp-custom", "My Wave", 1000, &[0.0, 0.5, 0.0],
            ChartKind::Line, false,
        ).unwrap();
        let rows = db.list_vibration_patterns().unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].uuid, "vp-custom");
        assert_eq!(rows[1].uuid, "vp-bundled");
    }

    #[test]
    fn insert_vibration_pattern_with_uuid_round_trips_through_list() {
        let db = Database::open_in_memory().unwrap();
        db.insert_vibration_pattern_with_uuid(
            "vp-1", "Heartbeat", 1500, &[0.0, 0.6, 0.0, 0.0, 1.0, 0.0],
            ChartKind::Line, false,
        ).unwrap();
        let rows = db.list_vibration_patterns().unwrap();
        assert_eq!(rows.len(), 1);
        let r = &rows[0];
        assert_eq!(r.uuid, "vp-1");
        assert_eq!(r.name, "Heartbeat");
        assert_eq!(r.duration_ms, 1500);
        assert_eq!(r.intensities, vec![0.0, 0.6, 0.0, 0.0, 1.0, 0.0]);
        assert_eq!(r.chart_kind, ChartKind::Line);
        assert!(!r.is_bundled);
        assert!(!r.created_iso.is_empty());
        assert_eq!(r.created_iso, r.updated_iso);
    }

    #[test]
    fn insert_vibration_pattern_with_existing_uuid_is_silent_noop() {
        let db = Database::open_in_memory().unwrap();
        let id1 = db.insert_vibration_pattern_with_uuid(
            "vp-1", "Pulse", 400, &[0.0, 1.0, 0.0],
            ChartKind::Line, true,
        ).unwrap();
        let id2 = db.insert_vibration_pattern_with_uuid(
            "vp-1", "Different Name", 999, &[0.5],
            ChartKind::Bar, false,
        ).unwrap();
        assert_eq!(id1, id2);
        let rows = db.list_vibration_patterns().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "Pulse");
        assert_eq!(rows[0].duration_ms, 400);
    }

    #[test]
    fn insert_vibration_pattern_with_duplicate_name_returns_duplicate_error() {
        let db = Database::open_in_memory().unwrap();
        db.insert_vibration_pattern_with_uuid(
            "vp-1", "Pulse", 400, &[0.0, 1.0, 0.0],
            ChartKind::Line, false,
        ).unwrap();
        match db.insert_vibration_pattern_with_uuid(
            "vp-2", "PULSE", 400, &[0.0, 1.0, 0.0],
            ChartKind::Line, false,
        ) {
            Err(DbError::DuplicateVibrationPattern(name)) => assert_eq!(name, "PULSE"),
            other => panic!("expected DuplicateVibrationPattern, got {other:?}"),
        }
    }

    #[test]
    fn insert_vibration_pattern_returns_a_fresh_uuid() {
        let db = Database::open_in_memory().unwrap();
        let uuid_str = db.insert_vibration_pattern(
            "Custom Wave", 1000, &[0.0, 0.5, 1.0, 0.5, 0.0],
            ChartKind::Line, false,
        ).unwrap();
        assert!(uuid::Uuid::parse_str(&uuid_str).is_ok());
        let row = db.find_vibration_pattern_by_uuid(&uuid_str).unwrap()
            .expect("row should exist under the returned uuid");
        assert_eq!(row.name, "Custom Wave");
    }

    #[test]
    fn find_vibration_pattern_by_uuid_returns_none_for_unknown_uuid() {
        let db = Database::open_in_memory().unwrap();
        assert!(db.find_vibration_pattern_by_uuid("nope").unwrap().is_none());
    }

    #[test]
    fn update_vibration_pattern_round_trips_changes() {
        let db = Database::open_in_memory().unwrap();
        let uuid_str = db.insert_vibration_pattern(
            "Wave", 2000, &[0.0, 0.5, 1.0, 0.5, 0.0],
            ChartKind::Line, false,
        ).unwrap();
        db.update_vibration_pattern(
            &uuid_str, "Slow Wave", 3000, &[0.0, 0.3, 0.6, 0.3, 0.0, 0.0, 0.0],
            ChartKind::Bar,
        ).unwrap();
        let row = db.find_vibration_pattern_by_uuid(&uuid_str).unwrap().unwrap();
        assert_eq!(row.name, "Slow Wave");
        assert_eq!(row.duration_ms, 3000);
        assert_eq!(row.intensities, vec![0.0, 0.3, 0.6, 0.3, 0.0, 0.0, 0.0]);
        assert_eq!(row.chart_kind, ChartKind::Bar);
    }

    #[test]
    fn update_vibration_pattern_bumps_updated_iso() {
        let db = Database::open_in_memory().unwrap();
        let uuid_str = db.insert_vibration_pattern(
            "Wave", 2000, &[0.0, 1.0, 0.0], ChartKind::Line, false,
        ).unwrap();
        let before = db.find_vibration_pattern_by_uuid(&uuid_str).unwrap().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        db.update_vibration_pattern(
            &uuid_str, "Wave", 2500, &[0.0, 1.0, 0.0], ChartKind::Line,
        ).unwrap();
        let after = db.find_vibration_pattern_by_uuid(&uuid_str).unwrap().unwrap();
        assert_eq!(after.created_iso, before.created_iso);
        assert!(after.updated_iso > before.updated_iso);
    }

    #[test]
    fn update_vibration_pattern_unknown_uuid_is_silent_noop() {
        let db = Database::open_in_memory().unwrap();
        db.update_vibration_pattern(
            "never-existed", "anything", 1000, &[0.0, 1.0], ChartKind::Line,
        ).unwrap();
        let updates: Vec<_> = db.pending_events().unwrap()
            .into_iter()
            .filter(|(_, e)| e.kind == "vibration_pattern_update")
            .collect();
        assert!(updates.is_empty());
    }

    #[test]
    fn update_vibration_pattern_to_existing_name_returns_duplicate_error() {
        let db = Database::open_in_memory().unwrap();
        db.insert_vibration_pattern_with_uuid(
            "vp-1", "Pulse", 400, &[0.0, 1.0, 0.0],
            ChartKind::Line, false,
        ).unwrap();
        let other = db.insert_vibration_pattern(
            "Wave", 1000, &[0.0, 1.0, 0.0], ChartKind::Line, false,
        ).unwrap();
        match db.update_vibration_pattern(
            &other, "PULSE", 1000, &[0.0, 1.0, 0.0], ChartKind::Line,
        ) {
            Err(DbError::DuplicateVibrationPattern(name)) => assert_eq!(name, "PULSE"),
            other => panic!("expected DuplicateVibrationPattern, got {other:?}"),
        }
    }

    #[test]
    fn delete_vibration_pattern_removes_row_and_emits_event() {
        let db = Database::open_in_memory().unwrap();
        let uuid_str = db.insert_vibration_pattern(
            "Wave", 2000, &[0.0, 1.0, 0.0], ChartKind::Line, false,
        ).unwrap();
        db.delete_vibration_pattern(&uuid_str).unwrap();
        assert!(db.list_vibration_patterns().unwrap().is_empty());
        assert!(db.find_vibration_pattern_by_uuid(&uuid_str).unwrap().is_none());
        let deletes: Vec<_> = db.pending_events().unwrap()
            .into_iter()
            .filter(|(_, e)| e.kind == "vibration_pattern_delete")
            .collect();
        assert_eq!(deletes.len(), 1);
    }

    #[test]
    fn delete_vibration_pattern_unknown_uuid_is_silent_noop() {
        let db = Database::open_in_memory().unwrap();
        db.delete_vibration_pattern("never-existed").unwrap();
        let deletes: Vec<_> = db.pending_events().unwrap()
            .into_iter()
            .filter(|(_, e)| e.kind == "vibration_pattern_delete")
            .collect();
        assert!(deletes.is_empty());
    }

    #[test]
    fn is_vibration_pattern_name_taken_returns_true_for_existing_other_row() {
        let db = Database::open_in_memory().unwrap();
        db.insert_vibration_pattern_with_uuid(
            "vp-1", "Pulse", 400, &[0.0, 1.0, 0.0],
            ChartKind::Line, false,
        ).unwrap();
        assert!(db.is_vibration_pattern_name_taken("Pulse", "").unwrap());
        assert!(db.is_vibration_pattern_name_taken("PULSE", "").unwrap());
        assert!(db.is_vibration_pattern_name_taken("pulse", "").unwrap());
    }

    #[test]
    fn is_vibration_pattern_name_taken_returns_false_for_missing_name() {
        let db = Database::open_in_memory().unwrap();
        assert!(!db.is_vibration_pattern_name_taken("Anything", "").unwrap());
    }

    #[test]
    fn is_vibration_pattern_name_taken_excludes_self_via_except_uuid() {
        let db = Database::open_in_memory().unwrap();
        db.insert_vibration_pattern_with_uuid(
            "vp-1", "Pulse", 400, &[0.0, 1.0, 0.0],
            ChartKind::Line, false,
        ).unwrap();
        assert!(!db.is_vibration_pattern_name_taken("Pulse", "vp-1").unwrap());
        assert!(db.is_vibration_pattern_name_taken("Pulse", "vp-2").unwrap());
    }

    #[test]
    fn apply_event_vibration_pattern_insert_creates_the_row_on_a_fresh_peer() {
        let peer = Database::open_in_memory().unwrap();
        let payload = serde_json::json!({
            "uuid": "vp-1",
            "name": "Wave",
            "duration_ms": 2000,
            "intensities_json": "[0.0,0.5,1.0,0.5,0.0]",
            "chart_kind": "line",
            "is_bundled": false,
            "created_iso": "2026-05-06T20:00:00Z",
            "updated_iso": "2026-05-06T20:00:00Z",
        });
        peer.apply_event(&synth_event(
            "vibration_pattern_insert", "vp-1", 5, DEVICE_A, payload,
        )).unwrap();
        let rows = peer.list_vibration_patterns().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "Wave");
        assert_eq!(rows[0].duration_ms, 2000);
        assert_eq!(rows[0].intensities, vec![0.0, 0.5, 1.0, 0.5, 0.0]);
        assert_eq!(rows[0].chart_kind, ChartKind::Line);
    }

    #[test]
    fn apply_event_vibration_pattern_update_overwrites_earlier_state() {
        let peer = Database::open_in_memory().unwrap();
        peer.apply_event(&synth_event(
            "vibration_pattern_insert", "vp-1", 5, DEVICE_A,
            serde_json::json!({
                "uuid": "vp-1", "name": "Wave",
                "duration_ms": 2000,
                "intensities_json": "[0.0,1.0,0.0]",
                "chart_kind": "line", "is_bundled": false,
                "created_iso": "2026-05-06T20:00:00Z",
                "updated_iso": "2026-05-06T20:00:00Z",
            }),
        )).unwrap();
        peer.apply_event(&synth_event(
            "vibration_pattern_update", "vp-1", 7, DEVICE_A,
            serde_json::json!({
                "uuid": "vp-1", "name": "Slow Wave",
                "duration_ms": 3000,
                "intensities_json": "[0.0,0.5,0.0]",
                "chart_kind": "bar", "is_bundled": false,
                "created_iso": "2026-05-06T20:00:00Z",
                "updated_iso": "2026-05-06T20:05:00Z",
            }),
        )).unwrap();
        let rows = peer.list_vibration_patterns().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "Slow Wave");
        assert_eq!(rows[0].duration_ms, 3000);
        assert_eq!(rows[0].chart_kind, ChartKind::Bar);
    }

    #[test]
    fn apply_event_vibration_pattern_delete_removes_the_row() {
        let peer = Database::open_in_memory().unwrap();
        peer.apply_event(&synth_event(
            "vibration_pattern_insert", "vp-1", 5, DEVICE_A,
            serde_json::json!({
                "uuid": "vp-1", "name": "Wave",
                "duration_ms": 2000,
                "intensities_json": "[0.0,1.0,0.0]",
                "chart_kind": "line", "is_bundled": false,
                "created_iso": "2026-05-06T20:00:00Z",
                "updated_iso": "2026-05-06T20:00:00Z",
            }),
        )).unwrap();
        peer.apply_event(&synth_event(
            "vibration_pattern_delete", "vp-1", 7, DEVICE_A,
            serde_json::json!({ "uuid": "vp-1" }),
        )).unwrap();
        assert!(peer.list_vibration_patterns().unwrap().is_empty());
    }

    #[test]
    fn apply_event_vibration_pattern_tombstone_resists_lower_lamport_insert() {
        let peer = Database::open_in_memory().unwrap();
        peer.apply_event(&synth_event(
            "vibration_pattern_delete", "vp-1", 10, DEVICE_A,
            serde_json::json!({ "uuid": "vp-1" }),
        )).unwrap();
        peer.apply_event(&synth_event(
            "vibration_pattern_insert", "vp-1", 5, DEVICE_A,
            serde_json::json!({
                "uuid": "vp-1", "name": "Wave",
                "duration_ms": 2000,
                "intensities_json": "[0.0,1.0,0.0]",
                "chart_kind": "line", "is_bundled": false,
                "created_iso": "2026-05-06T20:00:00Z",
                "updated_iso": "2026-05-06T20:00:00Z",
            }),
        )).unwrap();
        assert!(peer.list_vibration_patterns().unwrap().is_empty());
    }

    #[test]
    fn replay_vibration_pattern_events_round_trips_to_a_fresh_peer() {
        let dev_a = Database::open_in_memory().unwrap();
        dev_a.insert_vibration_pattern_with_uuid(
            "vp-1", "Wave", 2000, &[0.0, 0.5, 1.0, 0.5, 0.0],
            ChartKind::Line, false,
        ).unwrap();
        dev_a.update_vibration_pattern(
            "vp-1", "Slow Wave", 3000, &[0.0, 0.3, 0.6, 0.3, 0.0],
            ChartKind::Bar,
        ).unwrap();
        let events: Vec<Event> = dev_a.pending_events().unwrap()
            .into_iter().map(|(_, e)| e).collect();

        let dev_b = Database::open_in_memory().unwrap();
        dev_b.replay_events(&events).unwrap();
        let rows = dev_b.list_vibration_patterns().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "Slow Wave");
        assert_eq!(rows[0].chart_kind, ChartKind::Bar);
    }

    #[test]
    fn vibration_patterns_table_is_created_on_fresh_open() {
        let db = Database::open_in_memory().unwrap();
        let count: i64 = db.conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master \
             WHERE type = 'table' AND name = 'vibration_patterns'",
            [], |r| r.get(0),
        ).unwrap();
        assert_eq!(count, 1);

        let bad = db.conn.execute(
            "INSERT INTO vibration_patterns
                (uuid, name, duration_ms, intensities_json, chart_kind,
                 is_bundled, created_iso, updated_iso)
             VALUES ('u', 'n', 100, '[]', 'spiral', 0, 'now', 'now')",
            [],
        );
        assert!(bad.is_err());
    }
}
