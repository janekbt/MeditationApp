//! `vibration_patterns` table — user-managed vibration envelope
//! library. Each row holds N equally-spaced amplitude samples plus
//! a chart_kind (Line / Bar interpolation). Bundled rows ship with
//! the app under stable UUIDs.

use rusqlite::params;

use super::events::EventKind;
use super::{Database, DbError, Result};
use crate::vibration::ChartKind;

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
    pub uuid: super::VibrationPatternUuid,
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


const COLUMNS: &str =
    "id, uuid, name, duration_ms, intensities_json, chart_kind, is_bundled, created_iso, updated_iso";

fn from_row(row: &rusqlite::Row) -> rusqlite::Result<VibrationPattern> {
    let intensities_json: String = row.get(4)?;
    let chart_kind: String = row.get(5)?;
    Ok(VibrationPattern {
        id: row.get(0)?,
        uuid: row.get(1)?,
        name: row.get(2)?,
        duration_ms: row.get::<_, i64>(3)? as u32,
        intensities: serde_json::from_str(&intensities_json).unwrap_or_default(),
        chart_kind: ChartKind::from_db_str(&chart_kind)
            .expect("vibration_patterns.chart_kind violates CHECK constraint"),
        is_bundled: row.get::<_, i64>(6)? != 0,
        created_iso: row.get(7)?,
        updated_iso: row.get(8)?,
    })
}

/// Custom rows first, then the bundled set, as `list_bell_sounds`
/// does: a new pattern sits at the top of the chooser.
pub fn list_vibration_patterns_from_db(db: &Database) -> Result<Vec<VibrationPattern>> {
    db.query_rows(
        &format!("SELECT {COLUMNS} FROM vibration_patterns ORDER BY is_bundled ASC, created_iso ASC, uuid ASC"),
        [],
        from_row,
    )
}

pub fn find_vibration_pattern_by_uuid_from_db(db: &Database, uuid_str: &str) -> Result<Option<VibrationPattern>> {
    db.query_one(&format!("SELECT {COLUMNS} FROM vibration_patterns WHERE uuid = ?1"), params![uuid_str], from_row)
}

/// True iff a row other than `except_uuid` already holds `name`
/// (case-insensitive). The editor and chooser use this for live
/// validation; `except_uuid` is the row currently being renamed
/// (or "" for fresh inserts) so the user's own case-only renames
/// don't false-positive.
pub fn is_vibration_pattern_name_taken_from_db(
    db: &Database,
    name: &str,
    except_uuid: &str,
) -> Result<bool> {
    let count: i64 = db.conn.query_row(
        "SELECT COUNT(*) FROM vibration_patterns
          WHERE name = ?1 COLLATE NOCASE AND uuid != ?2",
        params![name, except_uuid],
        |row| row.get(0),
    )?;
    Ok(count > 0)
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
        let now_iso = chrono::Utc::now().to_rfc3339();
        self.insert_vibration_pattern_at(&VibrationPattern {
            id: 0,
            uuid: super::VibrationPatternUuid::new(uuid_str),
            name: name.to_string(),
            duration_ms,
            intensities: intensities.to_vec(),
            chart_kind,
            is_bundled,
            created_iso: now_iso.clone(),
            updated_iso: now_iso,
        })
    }

    /// Undo of a delete: the pattern comes back with its uuid and its
    /// creation time, so it takes its old place in the list.
    pub fn restore_vibration_pattern(&self, pattern: &VibrationPattern) -> Result<i64> {
        self.insert_vibration_pattern_at(pattern)
    }

    /// Insert `pattern` under its uuid and creation time (its id is
    /// ignored).
    fn insert_vibration_pattern_at(&self, pattern: &VibrationPattern) -> Result<i64> {
        let uuid_str = pattern.uuid.as_str();
        let tx = self.tx()?;
        let rowid = self.insert_new_in("vibration_patterns", uuid_str, || {
            // Checked first: the rebuild would suffix a taken name instead.
            if is_vibration_pattern_name_taken_from_db(self, &pattern.name, uuid_str)? {
                return Err(DbError::DuplicateVibrationPattern(pattern.name.clone()));
            }
            self.write_vibration_pattern_in(&tx, EventKind::VibrationPatternInsert, pattern)
        })?;
        tx.commit()?;
        Ok(rowid)
    }

    /// Record `pattern` as `kind` and store it the way a peer stores it.
    fn write_vibration_pattern_in(
        &self,
        tx: &super::Tx<'_>,
        kind: EventKind,
        pattern: &VibrationPattern,
    ) -> Result<()> {
        let uuid = pattern.uuid.as_str();
        let intensities_json = serde_json::to_string(&pattern.intensities)
            .map_err(|e| DbError::Decode(format!("serialise intensities: {e}")))?;
        self.emit_and_rebuild_in(tx, kind, uuid, serde_json::json!({
            "uuid": uuid,
            "name": pattern.name,
            "duration_ms": pattern.duration_ms,
            "intensities_json": intensities_json,
            "chart_kind": pattern.chart_kind.as_db_str(),
            "is_bundled": pattern.is_bundled,
            "created_iso": pattern.created_iso,
            "updated_iso": pattern.updated_iso,
        }))
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
        let uuid_str = super::mint_uuid();
        self.insert_vibration_pattern_with_uuid(
            &uuid_str, name, duration_ms, intensities, chart_kind, is_bundled,
        )?;
        Ok(uuid_str)
    }


    /// Rename a pattern. `Ok(false)` when the row doesn't exist or
    /// already has that name (the shell skips its undo toast then);
    /// `DuplicateVibrationPattern` if another row holds the name.
    pub fn rename_vibration_pattern(&self, uuid_str: &str, new_name: &str) -> Result<bool> {
        self.edit_vibration_pattern(uuid_str, |p| p.name = new_name.to_string())
    }

    /// Update the four mutable fields on an existing pattern. Unknown
    /// uuids, and the values the pattern already has (points within
    /// 1e-6, as the editor resamples), write nothing and return
    /// `false`: a fresh event would undo an edit another device made
    /// in the meantime. Returns `DuplicateVibrationPattern(name)` if
    /// another row already holds the new name (case-insensitive).
    pub fn update_vibration_pattern(
        &self,
        uuid_str: &str,
        name: &str,
        duration_ms: u32,
        intensities: &[f32],
        chart_kind: ChartKind,
    ) -> Result<bool> {
        self.edit_vibration_pattern(uuid_str, |p| {
            p.name = name.to_string();
            p.duration_ms = duration_ms;
            p.intensities = intensities.to_vec();
            p.chart_kind = chart_kind;
        })
    }

    /// Apply `edit` to the stored pattern and write it, unless that
    /// changes nothing (see `update_vibration_pattern`).
    fn edit_vibration_pattern(&self, uuid_str: &str, edit: impl FnOnce(&mut VibrationPattern)) -> Result<bool> {
        let Some(stored) = find_vibration_pattern_by_uuid_from_db(self, uuid_str)? else {
            return Ok(false);
        };
        let mut pattern = stored.clone();
        edit(&mut pattern);
        if crate::vibration::patterns_equivalent(&pattern, &stored) {
            return Ok(false);
        }
        if pattern.name != stored.name && is_vibration_pattern_name_taken_from_db(self, &pattern.name, uuid_str)? {
            return Err(DbError::DuplicateVibrationPattern(pattern.name));
        }
        pattern.updated_iso = chrono::Utc::now().to_rfc3339();
        let tx = self.tx()?;
        self.write_vibration_pattern_in(&tx, EventKind::VibrationPatternUpdate, &pattern)?;
        tx.commit()?;
        Ok(true)
    }

    /// See `delete_synced_row`.
    pub fn delete_vibration_pattern(&self, uuid_str: &str) -> Result<()> {
        self.delete_synced_row("vibration_patterns", uuid_str, EventKind::VibrationPatternDelete).map(drop)
    }


    /// Recompute the `vibration_patterns` row for `pattern_uuid` from
    /// the events table. Same precedence rules as guided_files /
    /// presets / bell_sounds: tombstone wins on tie, else the highest-
    /// (lamport, device_id) mutate event drives the row. Update events
    /// carry every field so they self-suffice on out-of-order delivery.
    pub(super) fn recompute_vibration_pattern(&self, pattern_uuid: &str) -> Result<Option<String>> {
        let Some(v) = self.winning_mutate(
            pattern_uuid,
            [EventKind::VibrationPatternInsert, EventKind::VibrationPatternUpdate],
            EventKind::VibrationPatternDelete,
        )? else {
            self.delete_row("vibration_patterns", pattern_uuid)?;
            return Ok(None);
        };
        {
            let name = v["name"].as_str().unwrap_or_default();
            let duration_ms = super::events::peer_u32(&v["duration_ms"]);
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
            self.claim_name("vibration_patterns", pattern_uuid, name, |name| self.conn.execute(upsert_sql, params![
                pattern_uuid, name, duration_ms, intensities_json,
                chart_kind, i64::from(is_bundled), created_iso, updated_iso,
            ]))
            .map(Some)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{test_helpers::*, Event};
    use crate::test_macros::assert_matches;

    const PAT: &str = "c0000000-0000-4000-8000-00000000000c";

    #[test]
    fn an_unchanged_pattern_save_keeps_a_peers_edit() {
        let laptop = Database::open_in_memory().unwrap();
        laptop.insert_vibration_pattern_with_uuid(PAT, "Wave", 1000, &[0.3, 0.6], ChartKind::Line, false).unwrap();
        let phone = Database::open_in_memory().unwrap();
        exchange(&laptop, &phone);
        phone.update_vibration_pattern(PAT, "Wave", 2000, &[0.3, 0.9], ChartKind::Bar).unwrap();
        let before = sent(&laptop);
        laptop.update_vibration_pattern(PAT, "Wave", 1000, &[0.3, 0.6], ChartKind::Line).unwrap();
        assert_eq!(sent(&laptop), before, "an unchanged save sends nothing");
        exchange(&laptop, &phone);
        for db in [&laptop, &phone] {
            let p = find_vibration_pattern_by_uuid_from_db(db, PAT).unwrap().unwrap();
            assert_eq!((p.duration_ms, p.chart_kind), (2000, ChartKind::Bar));
        }
    }

    #[test]
    fn a_peers_damaged_pattern_length_is_clamped() {
        let db = Database::open_in_memory().unwrap();
        db.apply_event(&synth_event("vibration_pattern_insert", PAT, 1, DEVICE_A, serde_json::json!({
            "uuid": PAT, "name": "Wave", "duration_ms": 1200.0, "intensities_json": "[0.5,1.0]",
            "chart_kind": "line", "is_bundled": false, "created_iso": "2026-10-01T00:00:00Z",
        }))).unwrap();
        assert_eq!(find_vibration_pattern_by_uuid_from_db(&db, PAT).unwrap().unwrap().duration_ms, 1200);
    }

    #[test]
    fn renaming_a_pattern_to_a_taken_name_sends_nothing() {
        let db = Database::open_in_memory().unwrap();
        db.insert_vibration_pattern_with_uuid(PAT, "Wave", 1000, &[0.3, 0.6], ChartKind::Line, false).unwrap();
        let other = db.insert_vibration_pattern("Tide", 1000, &[0.3, 0.6], ChartKind::Line, false).unwrap();
        let before = sent(&db);
        assert_matches!(db.rename_vibration_pattern(&other, "WAVE"), Err(DbError::DuplicateVibrationPattern(_)));
        assert_eq!(sent(&db), before);
    }

    #[test]
    fn saving_an_unchanged_pattern_reports_no_change() {
        let db = Database::open_in_memory().unwrap();
        db.insert_vibration_pattern_with_uuid(PAT, "Wave", 1000, &[0.3, 0.6], ChartKind::Line, false).unwrap();
        let before = sent(&db);
        // The editor resamples: a point may come back a hair off.
        assert!(!db.update_vibration_pattern(PAT, "Wave", 1000, &[0.300_000_1, 0.6], ChartKind::Line).unwrap());
        assert_eq!(sent(&db), before);
        assert!(db.update_vibration_pattern(PAT, "Wave", 1000, &[0.4, 0.6], ChartKind::Line).unwrap());
        assert_eq!(sent(&db), before + 1);
    }

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
        assert!(list_vibration_patterns_from_db(&db).unwrap().is_empty());
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
        let rows = list_vibration_patterns_from_db(&db).unwrap();
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
        let rows = list_vibration_patterns_from_db(&db).unwrap();
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
        let rows = list_vibration_patterns_from_db(&db).unwrap();
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
        assert_matches!(
            db.insert_vibration_pattern_with_uuid(
                "vp-2", "PULSE", 400, &[0.0, 1.0, 0.0],
                ChartKind::Line, false,
            ),
            Err(DbError::DuplicateVibrationPattern(name)) => assert_eq!(name, "PULSE"),
        );
    }

    #[test]
    fn insert_vibration_pattern_returns_a_fresh_uuid() {
        let db = Database::open_in_memory().unwrap();
        let uuid_str = db.insert_vibration_pattern(
            "Custom Wave", 1000, &[0.0, 0.5, 1.0, 0.5, 0.0],
            ChartKind::Line, false,
        ).unwrap();
        assert!(uuid::Uuid::parse_str(&uuid_str).is_ok());
        let row = find_vibration_pattern_by_uuid_from_db(&db, &uuid_str).unwrap()
            .expect("row should exist under the returned uuid");
        assert_eq!(row.name, "Custom Wave");
    }

    #[test]
    fn find_vibration_pattern_by_uuid_returns_none_for_unknown_uuid() {
        let db = Database::open_in_memory().unwrap();
        assert!(find_vibration_pattern_by_uuid_from_db(&db, "nope").unwrap().is_none());
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
        let row = find_vibration_pattern_by_uuid_from_db(&db, &uuid_str).unwrap().unwrap();
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
        let before = find_vibration_pattern_by_uuid_from_db(&db, &uuid_str).unwrap().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        db.update_vibration_pattern(
            &uuid_str, "Wave", 2500, &[0.0, 1.0, 0.0], ChartKind::Line,
        ).unwrap();
        let after = find_vibration_pattern_by_uuid_from_db(&db, &uuid_str).unwrap().unwrap();
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
        assert_matches!(
            db.update_vibration_pattern(
                &other, "PULSE", 1000, &[0.0, 1.0, 0.0], ChartKind::Line,
            ),
            Err(DbError::DuplicateVibrationPattern(name)) => assert_eq!(name, "PULSE"),
        );
    }

    #[test]
    fn delete_vibration_pattern_removes_row_and_emits_event() {
        let db = Database::open_in_memory().unwrap();
        let uuid_str = db.insert_vibration_pattern(
            "Wave", 2000, &[0.0, 1.0, 0.0], ChartKind::Line, false,
        ).unwrap();
        db.delete_vibration_pattern(&uuid_str).unwrap();
        assert!(list_vibration_patterns_from_db(&db).unwrap().is_empty());
        assert!(find_vibration_pattern_by_uuid_from_db(&db, &uuid_str).unwrap().is_none());
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
        assert!(is_vibration_pattern_name_taken_from_db(&db, "Pulse", "").unwrap());
        assert!(is_vibration_pattern_name_taken_from_db(&db, "PULSE", "").unwrap());
        assert!(is_vibration_pattern_name_taken_from_db(&db, "pulse", "").unwrap());
    }

    #[test]
    fn is_vibration_pattern_name_taken_returns_false_for_missing_name() {
        let db = Database::open_in_memory().unwrap();
        assert!(!is_vibration_pattern_name_taken_from_db(&db, "Anything", "").unwrap());
    }

    #[test]
    fn is_vibration_pattern_name_taken_excludes_self_via_except_uuid() {
        let db = Database::open_in_memory().unwrap();
        db.insert_vibration_pattern_with_uuid(
            "vp-1", "Pulse", 400, &[0.0, 1.0, 0.0],
            ChartKind::Line, false,
        ).unwrap();
        assert!(!is_vibration_pattern_name_taken_from_db(&db, "Pulse", "vp-1").unwrap());
        assert!(is_vibration_pattern_name_taken_from_db(&db, "Pulse", "vp-2").unwrap());
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
        let rows = list_vibration_patterns_from_db(&peer).unwrap();
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
        let rows = list_vibration_patterns_from_db(&peer).unwrap();
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
        assert!(list_vibration_patterns_from_db(&peer).unwrap().is_empty());
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
        assert!(list_vibration_patterns_from_db(&peer).unwrap().is_empty());
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
        let rows = list_vibration_patterns_from_db(&dev_b).unwrap();
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

    const SMALL: &str = "11111111-0000-4000-8000-000000000001";
    const LARGE: &str = "99999999-0000-4000-8000-000000000009";

    #[test]
    fn every_device_suffixes_the_same_pattern_of_a_same_name_pair() {
        for (mine, theirs) in [(SMALL, LARGE), (LARGE, SMALL)] {
            let peer = Database::open_in_memory().unwrap();
            peer.insert_vibration_pattern_with_uuid(theirs, "Wave", 1000, &[0.0, 1.0], ChartKind::Line, false).unwrap();
            let events: Vec<Event> = peer.pending_events().unwrap().into_iter()
                .map(|(_, e)| e).filter(|e| e.target_id == theirs).collect();
            let db = Database::open_in_memory().unwrap();
            db.insert_vibration_pattern_with_uuid(mine, "Wave", 1000, &[0.0, 1.0], ChartKind::Line, false).unwrap();
            db.replay_events(&events).unwrap();
            let name = |u: &str| list_vibration_patterns_from_db(&db).unwrap().into_iter()
                .find(|p| p.uuid.as_str() == u).unwrap().name;
            assert_eq!(name(SMALL), "Wave");
            assert_eq!(name(LARGE), "Wave (conflict-99999999)");
        }
    }

    /// Undo after a delete put the pattern at the end of the list.
    #[test]
    fn an_undone_delete_puts_the_pattern_back_in_its_place() {
        let db = Database::open_in_memory().unwrap();
        for name in ["Wave", "Pulse Two", "Heartbeat Two"] {
            db.insert_vibration_pattern(name, 1000, &[0.0, 1.0], ChartKind::Line, false).unwrap();
        }
        let names = |db: &Database| -> Vec<String> {
            list_vibration_patterns_from_db(db).unwrap().into_iter().map(|p| p.name).collect()
        };
        let before = names(&db);
        let wave = list_vibration_patterns_from_db(&db).unwrap().into_iter().find(|p| p.name == "Wave").unwrap();
        db.delete_vibration_pattern(wave.uuid.as_str()).unwrap();
        db.restore_vibration_pattern(&wave).unwrap();
        assert_eq!(names(&db), before);
    }
}
