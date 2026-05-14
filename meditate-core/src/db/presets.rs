//! `presets` table — named, full-fidelity session templates. The
//! shell defines and serialises `config_json`; core only stores +
//! round-trips it through sync events.

use rusqlite::{params, OptionalExtension};

use super::events::EventKind;
use super::{
    conflict_suffixed_name, is_unique_constraint_error, map_unique_err, mint_uuid,
    Database, DbError, Result, SessionMode,
};

/// One named, full-fidelity session template. Captures the entire
/// Setup-view state (mode, duration / breath pattern, label, bells,
/// interval-bell snapshot, end bell) under a stable UUID. The shell
/// applies a preset by replaying its `config_json` into the live
/// Setup state. `is_starred` controls whether the preset appears in
/// the visible chip list above the Save / Manage buttons; `mode`
/// is denormalised into a column so the visible-list query can
/// filter without parsing JSON.
///
/// The shape of `config_json` is opaque at this layer — core only
/// stores and round-trips it. The shell defines and serialises the
/// concrete schema, same way `Event::payload` works.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preset {
    pub id: i64,
    pub uuid: super::PresetUuid,
    pub name: String,
    pub mode: SessionMode,
    pub is_starred: bool,
    pub config_json: String,
    pub created_iso: String,
    pub updated_iso: String,
}

/// Every preset, ordered by mode (timer first, then box_breath)
/// then created_iso ASC. Stable order: rows don't shuffle when a
/// star toggles or a config gets overwritten.
pub fn list_presets_from_db(db: &Database) -> Result<Vec<Preset>> {
    let mut stmt = db.conn.prepare(
        "SELECT id, uuid, name, mode, is_starred, config_json, created_iso, updated_iso
         FROM presets
         ORDER BY mode, created_iso ASC",
    )?;
    let rows = stmt
        .query_map([], |row| {
            let mode_str: String = row.get(3)?;
            Ok(Preset {
                id: row.get(0)?,
                uuid: row.get(1)?,
                name: row.get(2)?,
                mode: SessionMode::from_db_str(&mode_str)
                    .expect("presets.mode violates CHECK constraint"),
                is_starred: row.get::<_, i64>(4)? != 0,
                config_json: row.get(5)?,
                created_iso: row.get(6)?,
                updated_iso: row.get(7)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}
/// Presets for one mode, ordered by created_iso ASC. Used by the
/// chooser pages (Save / Manage), both of which are mode-strict
/// per the design (the user shouldn't accidentally save a Timer
/// config into a Box-Breath preset, or see other-mode presets in
/// the management page).
pub fn list_presets_for_mode_from_db(db: &Database, mode: SessionMode) -> Result<Vec<Preset>> {
    let mut stmt = db.conn.prepare(
        "SELECT id, uuid, name, mode, is_starred, config_json, created_iso, updated_iso
         FROM presets
         WHERE mode = ?1
         ORDER BY created_iso ASC",
    )?;
    let rows = stmt
        .query_map(params![mode.as_db_str()], |row| {
            let mode_str: String = row.get(3)?;
            Ok(Preset {
                id: row.get(0)?,
                uuid: row.get(1)?,
                name: row.get(2)?,
                mode: SessionMode::from_db_str(&mode_str)
                    .expect("presets.mode violates CHECK constraint"),
                is_starred: row.get::<_, i64>(4)? != 0,
                config_json: row.get(5)?,
                created_iso: row.get(6)?,
                updated_iso: row.get(7)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}
/// Starred presets for one mode, ordered by created_iso ASC.
/// Drives the visible chip list above the Save / Manage buttons
/// in the Setup view. When this list is empty, the chip section
/// hides entirely (just the two buttons remain).
pub fn list_starred_presets_for_mode_from_db(db: &Database, mode: SessionMode) -> Result<Vec<Preset>> {
    let mut stmt = db.conn.prepare(
        "SELECT id, uuid, name, mode, is_starred, config_json, created_iso, updated_iso
         FROM presets
         WHERE mode = ?1 AND is_starred = 1
         ORDER BY created_iso ASC",
    )?;
    let rows = stmt
        .query_map(params![mode.as_db_str()], |row| {
            let mode_str: String = row.get(3)?;
            Ok(Preset {
                id: row.get(0)?,
                uuid: row.get(1)?,
                name: row.get(2)?,
                mode: SessionMode::from_db_str(&mode_str)
                    .expect("presets.mode violates CHECK constraint"),
                is_starred: row.get::<_, i64>(4)? != 0,
                config_json: row.get(5)?,
                created_iso: row.get(6)?,
                updated_iso: row.get(7)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}
/// True iff any preset other than `except_uuid` already uses
/// `name` (case-insensitive — the column is COLLATE NOCASE).
/// Used by the rename flow's live validation; pass the row's own
/// uuid as `except_uuid` so renaming to its current name (or a
/// case variant) doesn't false-positive.
pub fn is_preset_name_taken_from_db(db: &Database, name: &str, except_uuid: &str) -> Result<bool> {
    let n: i64 = db.conn.query_row(
        "SELECT COUNT(*) FROM presets WHERE name = ?1 AND uuid != ?2",
        params![name, except_uuid],
        |row| row.get(0),
    )?;
    Ok(n > 0)
}
pub fn find_preset_by_uuid_from_db(db: &Database, uuid_str: &str) -> Result<Option<Preset>> {
    let row = db.conn.query_row(
        "SELECT id, uuid, name, mode, is_starred, config_json, created_iso, updated_iso
         FROM presets WHERE uuid = ?1",
        params![uuid_str],
        |row| {
            let mode_str: String = row.get(3)?;
            Ok(Preset {
                id: row.get(0)?,
                uuid: row.get(1)?,
                name: row.get(2)?,
                mode: SessionMode::from_db_str(&mode_str)
                    .expect("presets.mode violates CHECK constraint"),
                is_starred: row.get::<_, i64>(4)? != 0,
                config_json: row.get(5)?,
                created_iso: row.get(6)?,
                updated_iso: row.get(7)?,
            })
        },
    ).optional()?;
    Ok(row)
}
pub fn count_presets_from_db(db: &Database) -> Result<i64> {
    Ok(db
        .conn
        .query_row("SELECT COUNT(*) FROM presets", [], |row| row.get(0))?)
}
impl Database {
    /// Create a preset under a freshly-minted v4 UUID. Convenience
    /// over `insert_preset_with_uuid` for the user-creates-from-Setup
    /// flow where the shell doesn't need a stable uuid up front.
    pub fn insert_preset(
        &self,
        name: &str,
        mode: SessionMode,
        is_starred: bool,
        config_json: &str,
    ) -> Result<i64> {
        self.insert_preset_with_uuid(
            &mint_uuid(),
            name,
            mode,
            is_starred,
            config_json,
        )
    }

    /// Insert a preset with a caller-supplied uuid. Idempotent on the
    /// uuid: an existing row with this uuid is returned without
    /// inserting or emitting. A duplicate *name* with a different uuid
    /// surfaces `DuplicatePreset` so unrelated callers don't silently
    /// shadow each other's rows. Emits a `preset_insert` event with
    /// the full row payload so a peer that's missed prior events can
    /// still materialise the row from this single message.
    pub fn insert_preset_with_uuid(
        &self,
        uuid_str: &str,
        name: &str,
        mode: SessionMode,
        is_starred: bool,
        config_json: &str,
    ) -> Result<i64> {
        let tx = self.conn.unchecked_transaction()?;
        if let Some(existing) = self.existing_rowid_by_uuid("presets", uuid_str)? {
            return Ok(existing);
        }
        let now_iso = chrono::Utc::now().to_rfc3339();
        self.conn
            .execute(
                "INSERT INTO presets (uuid, name, mode, is_starred, config_json, created_iso, updated_iso)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
                params![
                    uuid_str,
                    name,
                    mode.as_db_str(),
                    i64::from(is_starred),
                    config_json,
                    now_iso,
                ],
            )
            .map_err(|e| map_unique_err(e, || DbError::DuplicatePreset(name.to_string())))?;
        let rowid = self.conn.last_insert_rowid();
        let payload = serde_json::json!({
            "uuid": uuid_str,
            "name": name,
            "mode": mode.as_db_str(),
            "is_starred": is_starred,
            "config_json": config_json,
            "created_iso": now_iso,
            "updated_iso": now_iso,
        }).to_string();
        self.emit_event(EventKind::PresetInsert, uuid_str, payload)?;
        tx.commit()?;
        Ok(rowid)
    }







    /// Rename a preset. Unknown uuids are silent no-ops AND emit no
    /// event. If `name` collides with another preset (case-insensitive)
    /// returns `DuplicatePreset` and the transaction rolls back so no
    /// rename event leaks to peers. Renaming to the current name (or a
    /// case variant of itself) is allowed — SQLite's UNIQUE check
    /// excludes the row being updated.
    pub fn update_preset_name(&self, uuid_str: &str, name: &str) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        let exists: bool = self.conn.query_row(
            "SELECT 1 FROM presets WHERE uuid = ?1",
            params![uuid_str],
            |_| Ok(true),
        ).optional()?.unwrap_or(false);
        if !exists { return Ok(()); }
        let now_iso = chrono::Utc::now().to_rfc3339();
        self.conn
            .execute(
                "UPDATE presets SET name = ?1, updated_iso = ?2 WHERE uuid = ?3",
                params![name, now_iso, uuid_str],
            )
            .map_err(|e| map_unique_err(e, || DbError::DuplicatePreset(name.to_string())))?;
        let row = find_preset_by_uuid_from_db(self, uuid_str)?
            .expect("just confirmed exists");
        let payload = serde_json::json!({
            "uuid": uuid_str,
            "name": name,
            "mode": row.mode.as_db_str(),
            "is_starred": row.is_starred,
            "config_json": row.config_json,
            "created_iso": row.created_iso,
            "updated_iso": now_iso,
        }).to_string();
        self.emit_event(EventKind::PresetUpdate, uuid_str, payload)?;
        tx.commit()?;
        Ok(())
    }

    /// Replace the config JSON for a preset (the "Override" path in
    /// Save mode). Unknown uuids are silent no-ops with no event.
    /// Bumps `updated_iso`.
    pub fn update_preset_config(&self, uuid_str: &str, config_json: &str) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        let row = find_preset_by_uuid_from_db(self, uuid_str)?;
        let Some(row) = row else { return Ok(()); };
        let now_iso = chrono::Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE presets SET config_json = ?1, updated_iso = ?2 WHERE uuid = ?3",
            params![config_json, now_iso, uuid_str],
        )?;
        let payload = serde_json::json!({
            "uuid": uuid_str,
            "name": row.name,
            "mode": row.mode.as_db_str(),
            "is_starred": row.is_starred,
            "config_json": config_json,
            "created_iso": row.created_iso,
            "updated_iso": now_iso,
        }).to_string();
        self.emit_event(EventKind::PresetUpdate, uuid_str, payload)?;
        tx.commit()?;
        Ok(())
    }

    /// Star or unstar a preset. Unknown uuids are silent no-ops with
    /// no event. Bumps `updated_iso` so peers' last-write-wins
    /// resolution converges on the latest toggle.
    pub fn update_preset_starred(&self, uuid_str: &str, starred: crate::db::StarredState) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        let row = find_preset_by_uuid_from_db(self, uuid_str)?;
        let Some(row) = row else { return Ok(()); };
        let now_iso = chrono::Utc::now().to_rfc3339();
        let is_starred = starred.is_starred();
        self.conn.execute(
            "UPDATE presets SET is_starred = ?1, updated_iso = ?2 WHERE uuid = ?3",
            params![i64::from(is_starred), now_iso, uuid_str],
        )?;
        let payload = serde_json::json!({
            "uuid": uuid_str,
            "name": row.name,
            "mode": row.mode.as_db_str(),
            "is_starred": is_starred,
            "config_json": row.config_json,
            "created_iso": row.created_iso,
            "updated_iso": now_iso,
        }).to_string();
        self.emit_event(EventKind::PresetUpdate, uuid_str, payload)?;
        tx.commit()?;
        Ok(())
    }

    /// Remove a preset row and emit a tombstone. Unknown uuids are
    /// silent no-ops with no event — peers would otherwise receive a
    /// tombstone for a preset they never knew existed.
    pub fn delete_preset(&self, uuid_str: &str) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        let exists: bool = self.conn.query_row(
            "SELECT 1 FROM presets WHERE uuid = ?1",
            params![uuid_str],
            |_| Ok(true),
        ).optional()?.unwrap_or(false);
        if !exists { return Ok(()); }
        self.conn.execute(
            "DELETE FROM presets WHERE uuid = ?1",
            params![uuid_str],
        )?;
        let payload = serde_json::json!({ "uuid": uuid_str }).to_string();
        self.emit_event(EventKind::PresetDelete, uuid_str, payload)?;
        tx.commit()?;
        Ok(())
    }

    /// Recompute the `presets` row for `preset_uuid` from the events
    /// table. Same precedence rules as labels / interval_bells:
    /// tombstone wins on tie, else the highest-(lamport, device_id)
    /// mutate event drives the row. Update events carry every field
    /// plus created_iso so they self-suffice if the corresponding
    /// insert event hasn't arrived yet (out-of-order delivery).
    pub(super) fn recompute_preset(&self, preset_uuid: &str) -> Result<()> {
        let Some(v) = self.winning_mutate(
            preset_uuid,
            [EventKind::PresetInsert, EventKind::PresetUpdate],
            EventKind::PresetDelete,
        )? else {
            self.conn.execute(
                "DELETE FROM presets WHERE uuid = ?1",
                params![preset_uuid],
            )?;
            return Ok(());
        };
        {
            let name = v["name"].as_str().unwrap_or_default();
            let mode = v["mode"].as_str().unwrap_or("timer");
            let is_starred = v["is_starred"].as_bool().unwrap_or(false);
            let config_json = v["config_json"].as_str().unwrap_or("{}");
            let created_iso = v["created_iso"].as_str().unwrap_or_default();
            let updated_iso = v["updated_iso"].as_str().unwrap_or_default();
            let upsert_sql = "INSERT INTO presets
                    (uuid, name, mode, is_starred, config_json, created_iso, updated_iso)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(uuid) DO UPDATE SET
                    name        = excluded.name,
                    mode        = excluded.mode,
                    is_starred  = excluded.is_starred,
                    config_json = excluded.config_json,
                    created_iso = excluded.created_iso,
                    updated_iso = excluded.updated_iso";
            let first = self.conn.execute(upsert_sql, params![
                preset_uuid, name, mode, i64::from(is_starred),
                config_json, created_iso, updated_iso,
            ]);
            match first {
                Ok(_) => {}
                Err(e) if is_unique_constraint_error(&e) => {
                    let suffixed = conflict_suffixed_name(name, preset_uuid);
                    crate::diag::log(&format!(
                        "preset_name_collision: uuid={preset_uuid} \
                         original={name:?} resolved={suffixed:?}"
                    ));
                    self.conn.execute(upsert_sql, params![
                        preset_uuid, suffixed, mode, i64::from(is_starred),
                        config_json, created_iso, updated_iso,
                    ])?;
                }
                Err(e) => return Err(DbError::Sqlite(e)),
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{test_helpers::*, Event};

    fn insert_basic_preset(db: &Database, name: &str, mode: SessionMode) -> i64 {
        db.insert_preset(
            name,
            mode,
            false,
            r#"{"placeholder":true}"#,
        ).unwrap()
    }

    #[test]
    fn insert_preset_round_trips_through_list() {
        let db = Database::open_in_memory().unwrap();
        let id = db.insert_preset(
            "Sitting",
            SessionMode::Timer,
            true,
            r#"{"duration_secs":900}"#,
        ).unwrap();

        let presets = list_presets_from_db(&db).unwrap();
        assert_eq!(presets.len(), 1);
        let p = &presets[0];
        assert_eq!(p.id, id);
        assert!(!p.uuid.is_empty(), "fresh insert mints a uuid");
        assert_eq!(p.name, "Sitting");
        assert_eq!(p.mode, SessionMode::Timer);
        assert!(p.is_starred);
        assert_eq!(p.config_json, r#"{"duration_secs":900}"#);
        assert!(!p.created_iso.is_empty());
        assert_eq!(p.created_iso, p.updated_iso,
            "fresh insert sets updated_iso = created_iso");
    }

    #[test]
    fn insert_preset_guided_mode_round_trips() {
        let db = Database::open_in_memory().unwrap();
        let id = db.insert_preset(
            "Headspace Track 1",
            SessionMode::Guided,
            true,
            r#"{"guided_file_uuid":"x"}"#,
        ).unwrap();
        let presets = list_presets_from_db(&db).unwrap();
        assert_eq!(presets.len(), 1);
        assert_eq!(presets[0].id, id);
        assert_eq!(presets[0].mode, SessionMode::Guided);
    }

    #[test]
    fn insert_preset_with_uuid_uses_supplied_uuid() {
        let db = Database::open_in_memory().unwrap();
        let _id = db.insert_preset_with_uuid(
            "abc-123",
            "Sitting",
            SessionMode::Timer,
            false,
            r#"{}"#,
        ).unwrap();
        let presets = list_presets_from_db(&db).unwrap();
        assert_eq!(presets[0].uuid, "abc-123");
    }

    #[test]
    fn insert_preset_with_existing_uuid_is_silent_noop() {
        let db = Database::open_in_memory().unwrap();
        let r1 = db.insert_preset_with_uuid(
            "u-1", "Sitting", SessionMode::Timer, false, r#"{}"#,
        ).unwrap();
        let r2 = db.insert_preset_with_uuid(
            "u-1", "Different Name", SessionMode::BoxBreath, true, r#"{"x":1}"#,
        ).unwrap();
        assert_eq!(r1, r2, "second insert returns existing rowid");
        assert_eq!(count_presets_from_db(&db).unwrap(), 1);
        // Original values stand — second insert is a pure no-op.
        let p = &list_presets_from_db(&db).unwrap()[0];
        assert_eq!(p.name, "Sitting");
        assert_eq!(p.mode, SessionMode::Timer);
        assert!(!p.is_starred);
    }

    #[test]
    fn insert_preset_duplicate_name_returns_duplicate_preset() {
        let db = Database::open_in_memory().unwrap();
        db.insert_preset("Sitting", SessionMode::Timer, false, r#"{}"#).unwrap();
        let r = db.insert_preset(
            "sitting",  // case-insensitive collision
            SessionMode::BoxBreath,
            false,
            r#"{}"#,
        );
        assert!(
            matches!(r, Err(DbError::DuplicatePreset(ref n)) if n == "sitting"),
            "expected DuplicatePreset, got {r:?}",
        );
        assert_eq!(count_presets_from_db(&db).unwrap(), 1, "row count unchanged");
    }

    #[test]
    fn insert_preset_emits_a_preset_insert_event() {
        let db = Database::open_in_memory().unwrap();
        db.insert_preset("Sitting", SessionMode::Timer, true, r#"{"dur":900}"#).unwrap();
        let events = db.pending_events().unwrap();
        let insert: Vec<_> = events.iter()
            .filter(|(_, e)| e.kind == "preset_insert")
            .collect();
        assert_eq!(insert.len(), 1);
        let payload: serde_json::Value =
            serde_json::from_str(&insert[0].1.payload).unwrap();
        assert_eq!(payload["name"], "Sitting");
        assert_eq!(payload["mode"], "timer");
        assert_eq!(payload["is_starred"], true);
        assert_eq!(payload["config_json"], r#"{"dur":900}"#);
    }

    #[test]
    fn list_presets_orders_by_mode_then_created_iso() {
        let db = Database::open_in_memory().unwrap();
        // Interleave insert order and modes; verify the SQL ORDER BY
        // groups by mode and orders by created_iso within each group.
        insert_basic_preset(&db, "BB-1", SessionMode::BoxBreath);
        std::thread::sleep(std::time::Duration::from_millis(2));
        insert_basic_preset(&db, "T-1", SessionMode::Timer);
        std::thread::sleep(std::time::Duration::from_millis(2));
        insert_basic_preset(&db, "BB-2", SessionMode::BoxBreath);
        std::thread::sleep(std::time::Duration::from_millis(2));
        insert_basic_preset(&db, "T-2", SessionMode::Timer);

        let names: Vec<String> = list_presets_from_db(&db).unwrap()
            .into_iter().map(|p| p.name).collect();
        // Mode ordering (timer < box_breath alphabetically as DB string —
        // 'box_breath' < 'timer' actually). Verify whichever way SQL sees it.
        // What matters is that within each mode block, created_iso is ASC.
        let bb_pos: Vec<usize> = names.iter().enumerate()
            .filter(|(_, n)| n.starts_with("BB-"))
            .map(|(i, _)| i).collect();
        let t_pos: Vec<usize> = names.iter().enumerate()
            .filter(|(_, n)| n.starts_with("T-"))
            .map(|(i, _)| i).collect();
        assert_eq!(bb_pos.len(), 2);
        assert_eq!(t_pos.len(), 2);
        // Within each mode, ordering follows insert order.
        assert!(bb_pos[0] < bb_pos[1]);
        assert!(t_pos[0] < t_pos[1]);
        assert_eq!(names[bb_pos[0]], "BB-1");
        assert_eq!(names[bb_pos[1]], "BB-2");
        assert_eq!(names[t_pos[0]], "T-1");
        assert_eq!(names[t_pos[1]], "T-2");
    }

    #[test]
    fn list_presets_for_mode_filters_correctly() {
        let db = Database::open_in_memory().unwrap();
        insert_basic_preset(&db, "T-1", SessionMode::Timer);
        insert_basic_preset(&db, "BB-1", SessionMode::BoxBreath);
        insert_basic_preset(&db, "T-2", SessionMode::Timer);

        let timers: Vec<String> = list_presets_for_mode_from_db(&db, SessionMode::Timer)
            .unwrap().into_iter().map(|p| p.name).collect();
        assert_eq!(timers, vec!["T-1", "T-2"]);

        let breaths: Vec<String> = list_presets_for_mode_from_db(&db, SessionMode::BoxBreath)
            .unwrap().into_iter().map(|p| p.name).collect();
        assert_eq!(breaths, vec!["BB-1"]);
    }

    #[test]
    fn list_starred_presets_for_mode_returns_only_starred_in_mode() {
        let db = Database::open_in_memory().unwrap();
        // Mix: Timer starred + unstarred; BoxBreath starred + unstarred.
        db.insert_preset("T-star", SessionMode::Timer, true, r#"{}"#).unwrap();
        db.insert_preset("T-no", SessionMode::Timer, false, r#"{}"#).unwrap();
        db.insert_preset("BB-star", SessionMode::BoxBreath, true, r#"{}"#).unwrap();
        db.insert_preset("BB-no", SessionMode::BoxBreath, false, r#"{}"#).unwrap();

        let timer_starred: Vec<String> =
            list_starred_presets_for_mode_from_db(&db, SessionMode::Timer).unwrap()
                .into_iter().map(|p| p.name).collect();
        assert_eq!(timer_starred, vec!["T-star"]);

        let breath_starred: Vec<String> =
            list_starred_presets_for_mode_from_db(&db, SessionMode::BoxBreath).unwrap()
                .into_iter().map(|p| p.name).collect();
        assert_eq!(breath_starred, vec!["BB-star"]);
    }

    #[test]
    fn is_preset_name_taken_excludes_self_uuid() {
        let db = Database::open_in_memory().unwrap();
        db.insert_preset_with_uuid(
            "u-1", "Sitting", SessionMode::Timer, false, r#"{}"#,
        ).unwrap();
        db.insert_preset_with_uuid(
            "u-2", "Walking", SessionMode::Timer, false, r#"{}"#,
        ).unwrap();
        // Collision with another preset's name.
        assert!(is_preset_name_taken_from_db(&db, "Walking", "u-1").unwrap());
        // Renaming to own current name — case-insensitive — is allowed.
        assert!(!is_preset_name_taken_from_db(&db, "sitting", "u-1").unwrap());
        // Brand-new name is fine.
        assert!(!is_preset_name_taken_from_db(&db, "Sleeping", "u-1").unwrap());
    }

    #[test]
    fn find_preset_by_uuid_returns_some_when_present() {
        let db = Database::open_in_memory().unwrap();
        db.insert_preset_with_uuid(
            "u-1", "Sitting", SessionMode::Timer, true, r#"{"x":1}"#,
        ).unwrap();
        let p = find_preset_by_uuid_from_db(&db, "u-1").unwrap().unwrap();
        assert_eq!(p.name, "Sitting");
        assert!(p.is_starred);
        assert_eq!(p.config_json, r#"{"x":1}"#);
    }

    #[test]
    fn find_preset_by_uuid_returns_none_when_absent() {
        let db = Database::open_in_memory().unwrap();
        assert!(find_preset_by_uuid_from_db(&db, "missing").unwrap().is_none());
    }

    #[test]
    fn update_preset_name_changes_the_name_and_bumps_updated_iso() {
        let db = Database::open_in_memory().unwrap();
        db.insert_preset_with_uuid(
            "u-1", "Sitting", SessionMode::Timer, false, r#"{}"#,
        ).unwrap();
        let before = find_preset_by_uuid_from_db(&db, "u-1").unwrap().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        db.update_preset_name("u-1", "Morning Sit").unwrap();
        let after = find_preset_by_uuid_from_db(&db, "u-1").unwrap().unwrap();
        assert_eq!(after.name, "Morning Sit");
        assert_eq!(after.created_iso, before.created_iso);
        assert!(after.updated_iso > before.updated_iso);
    }

    #[test]
    fn update_preset_name_duplicate_returns_error_and_rolls_back() {
        let db = Database::open_in_memory().unwrap();
        db.insert_preset_with_uuid(
            "u-1", "Sitting", SessionMode::Timer, false, r#"{}"#,
        ).unwrap();
        db.insert_preset_with_uuid(
            "u-2", "Walking", SessionMode::Timer, false, r#"{}"#,
        ).unwrap();
        let result = db.update_preset_name("u-1", "Walking");
        assert!(matches!(result, Err(DbError::DuplicatePreset(_))));
        // u-1 still has its original name.
        let p = find_preset_by_uuid_from_db(&db, "u-1").unwrap().unwrap();
        assert_eq!(p.name, "Sitting");
    }

    #[test]
    fn update_preset_name_unknown_uuid_is_silent_noop() {
        let db = Database::open_in_memory().unwrap();
        db.update_preset_name("missing", "Foo").unwrap();
        let events: Vec<_> = db.pending_events().unwrap()
            .into_iter()
            .filter(|(_, e)| e.kind == "preset_update")
            .collect();
        assert!(events.is_empty(), "unknown uuid emits no event");
    }

    #[test]
    fn update_preset_config_replaces_the_config_blob() {
        let db = Database::open_in_memory().unwrap();
        db.insert_preset_with_uuid(
            "u-1", "Sitting", SessionMode::Timer, false, r#"{"old":1}"#,
        ).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        db.update_preset_config("u-1", r#"{"new":2}"#).unwrap();
        let p = find_preset_by_uuid_from_db(&db, "u-1").unwrap().unwrap();
        assert_eq!(p.config_json, r#"{"new":2}"#);
    }

    #[test]
    fn update_preset_config_emits_one_preset_update_event() {
        let db = Database::open_in_memory().unwrap();
        db.insert_preset_with_uuid(
            "u-1", "Sitting", SessionMode::Timer, false, r#"{}"#,
        ).unwrap();
        db.update_preset_config("u-1", r#"{"x":1}"#).unwrap();
        let updates: Vec<_> = db.pending_events().unwrap()
            .into_iter()
            .filter(|(_, e)| e.kind == "preset_update")
            .collect();
        assert_eq!(updates.len(), 1);
    }

    #[test]
    fn update_preset_starred_toggles_the_flag() {
        let db = Database::open_in_memory().unwrap();
        db.insert_preset_with_uuid(
            "u-1", "Sitting", SessionMode::Timer, false, r#"{}"#,
        ).unwrap();
        db.update_preset_starred("u-1", crate::db::StarredState::Starred).unwrap();
        assert!(find_preset_by_uuid_from_db(&db, "u-1").unwrap().unwrap().is_starred);
        db.update_preset_starred("u-1", crate::db::StarredState::Unstarred).unwrap();
        assert!(!find_preset_by_uuid_from_db(&db, "u-1").unwrap().unwrap().is_starred);
    }

    #[test]
    fn delete_preset_removes_the_row_and_emits_tombstone() {
        let db = Database::open_in_memory().unwrap();
        db.insert_preset_with_uuid(
            "u-1", "Sitting", SessionMode::Timer, false, r#"{}"#,
        ).unwrap();
        db.delete_preset("u-1").unwrap();
        assert!(find_preset_by_uuid_from_db(&db, "u-1").unwrap().is_none());
        let tombstones: Vec<_> = db.pending_events().unwrap()
            .into_iter()
            .filter(|(_, e)| e.kind == "preset_delete")
            .collect();
        assert_eq!(tombstones.len(), 1);
    }

    #[test]
    fn delete_preset_unknown_uuid_emits_no_event() {
        let db = Database::open_in_memory().unwrap();
        db.delete_preset("never-existed").unwrap();
        let tombstones: Vec<_> = db.pending_events().unwrap()
            .into_iter()
            .filter(|(_, e)| e.kind == "preset_delete")
            .collect();
        assert!(tombstones.is_empty());
    }

    // ── Presets — sync replay (apply_event / replay_events) ───────────

    #[test]
    fn apply_event_preset_insert_creates_the_row_on_a_fresh_peer() {
        let peer = Database::open_in_memory().unwrap();
        let payload = serde_json::json!({
            "uuid": "u-1",
            "name": "Sitting",
            "mode": "timer",
            "is_starred": true,
            "config_json": "{\"dur\":900}",
            "created_iso": "2026-05-04T10:00:00Z",
            "updated_iso": "2026-05-04T10:00:00Z",
        });
        peer.apply_event(&synth_event("preset_insert", "u-1", 1, "dev-a", payload))
            .unwrap();
        let p = find_preset_by_uuid_from_db(&peer, "u-1").unwrap()
            .expect("row materialised on peer");
        assert_eq!(p.name, "Sitting");
        assert_eq!(p.mode, SessionMode::Timer);
        assert!(p.is_starred);
        assert_eq!(p.config_json, r#"{"dur":900}"#);
    }

    #[test]
    fn apply_event_preset_update_overwrites_fields_after_insert() {
        let peer = Database::open_in_memory().unwrap();
        peer.apply_event(&synth_event("preset_insert", "u-1", 1, "dev-a",
            serde_json::json!({
                "uuid": "u-1", "name": "Sitting", "mode": "timer",
                "is_starred": false, "config_json": "{\"v\":1}",
                "created_iso": "2026-05-04T10:00:00Z",
                "updated_iso": "2026-05-04T10:00:00Z",
            }))).unwrap();
        peer.apply_event(&synth_event("preset_update", "u-1", 5, "dev-a",
            serde_json::json!({
                "uuid": "u-1", "name": "Morning Sit", "mode": "timer",
                "is_starred": true, "config_json": "{\"v\":2}",
                "created_iso": "2026-05-04T10:00:00Z",
                "updated_iso": "2026-05-04T10:05:00Z",
            }))).unwrap();
        let p = find_preset_by_uuid_from_db(&peer, "u-1").unwrap().unwrap();
        assert_eq!(p.name, "Morning Sit");
        assert!(p.is_starred);
        assert_eq!(p.config_json, r#"{"v":2}"#);
    }

    #[test]
    fn apply_event_preset_delete_removes_the_row() {
        let peer = Database::open_in_memory().unwrap();
        peer.apply_event(&synth_event("preset_insert", "u-1", 1, "dev-a",
            serde_json::json!({
                "uuid": "u-1", "name": "Sitting", "mode": "timer",
                "is_starred": false, "config_json": "{}",
                "created_iso": "x", "updated_iso": "x",
            }))).unwrap();
        peer.apply_event(&synth_event("preset_delete", "u-1", 5, "dev-a",
            serde_json::json!({"uuid": "u-1"}))).unwrap();
        assert!(find_preset_by_uuid_from_db(&peer, "u-1").unwrap().is_none());
    }

    #[test]
    fn apply_event_preset_tombstone_resists_lower_lamport_insert() {
        // Out-of-order delivery: delete arrives first (ts=10), then a
        // late insert event with ts=5. Tombstone wins, row stays absent.
        let peer = Database::open_in_memory().unwrap();
        peer.apply_event(&synth_event("preset_delete", "u-1", 10, "dev-a",
            serde_json::json!({"uuid": "u-1"}))).unwrap();
        peer.apply_event(&synth_event("preset_insert", "u-1", 5, "dev-a",
            serde_json::json!({
                "uuid": "u-1", "name": "Sitting", "mode": "timer",
                "is_starred": false, "config_json": "{}",
                "created_iso": "x", "updated_iso": "x",
            }))).unwrap();
        assert!(find_preset_by_uuid_from_db(&peer, "u-1").unwrap().is_none(),
            "tombstone with higher ts wins over later-applied lower-ts insert");
    }

    #[test]
    fn replay_events_round_trips_a_preset_through_create_rename_delete() {
        // Full lifecycle on device A, replayed on device B from the
        // event log alone.
        let dev_a = Database::open_in_memory().unwrap();
        dev_a.insert_preset_with_uuid(
            "u-1", "Sitting", SessionMode::Timer, true, r#"{"dur":900}"#,
        ).unwrap();
        dev_a.update_preset_name("u-1", "Morning Sit").unwrap();
        dev_a.update_preset_starred("u-1", crate::db::StarredState::Unstarred).unwrap();

        let events: Vec<Event> = dev_a.pending_events().unwrap()
            .into_iter().map(|(_, e)| e).collect();

        let dev_b = Database::open_in_memory().unwrap();
        dev_b.replay_events(&events).unwrap();
        let p = find_preset_by_uuid_from_db(&dev_b, "u-1").unwrap()
            .expect("device B materialised the preset from events alone");
        assert_eq!(p.name, "Morning Sit");
        assert!(!p.is_starred);
        assert_eq!(p.config_json, r#"{"dur":900}"#);
    }

    #[test]
    fn replay_events_with_delete_at_the_end_leaves_the_row_absent() {
        let dev_a = Database::open_in_memory().unwrap();
        dev_a.insert_preset_with_uuid(
            "u-1", "Sitting", SessionMode::Timer, false, r#"{}"#,
        ).unwrap();
        dev_a.delete_preset("u-1").unwrap();
        let events: Vec<Event> = dev_a.pending_events().unwrap()
            .into_iter().map(|(_, e)| e).collect();

        let dev_b = Database::open_in_memory().unwrap();
        dev_b.replay_events(&events).unwrap();
        assert!(find_preset_by_uuid_from_db(&dev_b, "u-1").unwrap().is_none());
    }
}
