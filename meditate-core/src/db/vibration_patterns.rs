//! `vibration_patterns` table — user-managed vibration envelope
//! library. Each row holds N equally-spaced amplitude samples plus
//! a chart_kind (Line / Bar interpolation). Bundled rows ship with
//! the app under stable UUIDs.

use rusqlite::{params, OptionalExtension};

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
        self.emit_event("vibration_pattern_insert", uuid_str, payload)?;
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
        self.emit_event("vibration_pattern_update", uuid_str, payload)?;
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
        self.emit_event("vibration_pattern_delete", uuid_str, payload)?;
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
