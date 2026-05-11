//! `presets` table — named, full-fidelity session templates. The
//! shell defines and serialises `config_json`; core only stores +
//! round-trips it through sync events.

use rusqlite::{params, OptionalExtension};

use super::{
    conflict_suffixed_name, is_unique_constraint_error, mint_uuid, Database, DbError,
    Result, SessionMode,
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
    pub uuid: String,
    pub name: String,
    pub mode: SessionMode,
    pub is_starred: bool,
    pub config_json: String,
    pub created_iso: String,
    pub updated_iso: String,
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
        if let Some(existing) = self.conn.query_row(
            "SELECT id FROM presets WHERE uuid = ?1",
            params![uuid_str],
            |row| row.get::<_, i64>(0),
        ).optional()? {
            return Ok(existing);
        }
        let now_iso = chrono::Utc::now().to_rfc3339();
        match self.conn.execute(
            "INSERT INTO presets (uuid, name, mode, is_starred, config_json, created_iso, updated_iso)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
            params![
                uuid_str,
                name,
                mode.as_db_str(),
                is_starred as i64,
                config_json,
                now_iso,
            ],
        ) {
            Ok(_) => {
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
                self.emit_event("preset_insert", uuid_str, payload)?;
                tx.commit()?;
                Ok(rowid)
            }
            Err(rusqlite::Error::SqliteFailure(err, _))
                if err.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE =>
            {
                Err(DbError::DuplicatePreset(name.to_string()))
            }
            Err(e) => Err(DbError::Sqlite(e)),
        }
    }

    /// Every preset, ordered by mode (timer first, then box_breath)
    /// then created_iso ASC. Stable order: rows don't shuffle when a
    /// star toggles or a config gets overwritten.
    pub fn list_presets(&self) -> Result<Vec<Preset>> {
        let mut stmt = self.conn.prepare(
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
                        .unwrap_or(SessionMode::Timer),
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
    pub fn list_presets_for_mode(&self, mode: SessionMode) -> Result<Vec<Preset>> {
        let mut stmt = self.conn.prepare(
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
                        .unwrap_or(SessionMode::Timer),
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
    pub fn list_starred_presets_for_mode(&self, mode: SessionMode) -> Result<Vec<Preset>> {
        let mut stmt = self.conn.prepare(
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
                        .unwrap_or(SessionMode::Timer),
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
    pub fn is_preset_name_taken(&self, name: &str, except_uuid: &str) -> Result<bool> {
        let n: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM presets WHERE name = ?1 AND uuid != ?2",
            params![name, except_uuid],
            |row| row.get(0),
        )?;
        Ok(n > 0)
    }

    pub fn find_preset_by_uuid(&self, uuid_str: &str) -> Result<Option<Preset>> {
        let row = self.conn.query_row(
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
                        .unwrap_or(SessionMode::Timer),
                    is_starred: row.get::<_, i64>(4)? != 0,
                    config_json: row.get(5)?,
                    created_iso: row.get(6)?,
                    updated_iso: row.get(7)?,
                })
            },
        ).optional()?;
        Ok(row)
    }

    pub fn count_presets(&self) -> Result<i64> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM presets", [], |row| row.get(0))?)
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
        match self.conn.execute(
            "UPDATE presets SET name = ?1, updated_iso = ?2 WHERE uuid = ?3",
            params![name, now_iso, uuid_str],
        ) {
            Ok(_) => {}
            Err(rusqlite::Error::SqliteFailure(err, _))
                if err.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE =>
            {
                return Err(DbError::DuplicatePreset(name.to_string()));
            }
            Err(e) => return Err(DbError::Sqlite(e)),
        }
        let row = self.find_preset_by_uuid(uuid_str)?
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
        self.emit_event("preset_update", uuid_str, payload)?;
        tx.commit()?;
        Ok(())
    }

    /// Replace the config JSON for a preset (the "Override" path in
    /// Save mode). Unknown uuids are silent no-ops with no event.
    /// Bumps `updated_iso`.
    pub fn update_preset_config(&self, uuid_str: &str, config_json: &str) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        let row = self.find_preset_by_uuid(uuid_str)?;
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
        self.emit_event("preset_update", uuid_str, payload)?;
        tx.commit()?;
        Ok(())
    }

    /// Star or unstar a preset. Unknown uuids are silent no-ops with
    /// no event. Bumps `updated_iso` so peers' last-write-wins
    /// resolution converges on the latest toggle.
    pub fn update_preset_starred(&self, uuid_str: &str, is_starred: bool) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        let row = self.find_preset_by_uuid(uuid_str)?;
        let Some(row) = row else { return Ok(()); };
        let now_iso = chrono::Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE presets SET is_starred = ?1, updated_iso = ?2 WHERE uuid = ?3",
            params![is_starred as i64, now_iso, uuid_str],
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
        self.emit_event("preset_update", uuid_str, payload)?;
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
        self.emit_event("preset_delete", uuid_str, payload)?;
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
        let delete_ts: Option<i64> = self.conn.query_row(
            "SELECT MAX(lamport_ts) FROM events
             WHERE target_id = ?1 AND kind = 'preset_delete'",
            params![preset_uuid],
            |row| row.get::<_, Option<i64>>(0),
        )?;
        let mutate: Option<(i64, String)> = self.conn.query_row(
            "SELECT lamport_ts, payload FROM events
             WHERE target_id = ?1
               AND kind IN ('preset_insert', 'preset_update')
             ORDER BY lamport_ts DESC, device_id DESC
             LIMIT 1",
            params![preset_uuid],
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
                    format!("preset event payload not valid JSON: {e}")))?;
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
                preset_uuid, name, mode, is_starred as i64,
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
                        preset_uuid, suffixed, mode, is_starred as i64,
                        config_json, created_iso, updated_iso,
                    ])?;
                }
                Err(e) => return Err(DbError::Sqlite(e)),
            }
        } else {
            self.conn.execute(
                "DELETE FROM presets WHERE uuid = ?1",
                params![preset_uuid],
            )?;
        }
        Ok(())
    }
}
