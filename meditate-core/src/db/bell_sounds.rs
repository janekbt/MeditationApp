//! `bell_sounds` table — bundled + user-imported audio file library
//! shared across Starting / Interval / End bells (General category)
//! and Box-Breath per-phase cues (BoxBreath category).

use rusqlite::{params, OptionalExtension};

use super::{Database, DbError, Result};

/// Bundled and user-imported audio files for cue playback. Categories
/// are mutually exclusive — no row sits in both — and the chooser
/// filters by the category its caller passes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BellSound {
    pub id: i64,
    pub uuid: String,
    pub name: String,
    pub file_path: String,
    pub is_bundled: bool,
    pub mime_type: String,
    pub category: BellSoundCategory,
    pub created_iso: String,
}

/// Usage-context partition for the `bell_sounds` library.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BellSoundCategory {
    /// Bells, gongs, chimes — Starting / Interval / End bells.
    General,
    /// Voice cues / soft phase markers — Box Breath per-phase chooser.
    BoxBreath,
}

impl BellSoundCategory {
    pub fn as_db_str(self) -> &'static str {
        match self {
            BellSoundCategory::General   => "general",
            BellSoundCategory::BoxBreath => "box_breath",
        }
    }
    pub fn from_db_str(s: &str) -> Option<Self> {
        match s {
            "general"    => Some(BellSoundCategory::General),
            "box_breath" => Some(BellSoundCategory::BoxBreath),
            _            => None,
        }
    }
}

impl BellSound {
    /// File extension corresponding to `mime_type`. Used by both the
    /// shell (canonical local-audio path) and the orchestrator
    /// (remote PUT/GET path). Falls back to "wav" for any mime not
    /// in the small known set — matches the import code's default.
    pub fn extension(&self) -> &'static str {
        match self.mime_type.as_str() {
            "audio/ogg" => "ogg",
            "audio/mpeg" => "mp3",
            "audio/opus" => "opus",
            "audio/flac" => "flac",
            "audio/mp4" => "m4a",
            _ => "wav",
        }
    }
}

impl Database {
    /// Insert a bell-sound row with a fresh UUID. Used by custom-file
    /// imports (B.5). Returns the AUTOINCREMENT rowid; emits a
    /// `bell_sound_insert` event.
    pub fn insert_bell_sound(
        &self,
        name: &str,
        file_path: &str,
        is_bundled: bool,
        mime_type: &str,
        category: BellSoundCategory,
    ) -> Result<i64> {
        self.insert_bell_sound_with_uuid(
            &uuid::Uuid::new_v4().to_string(),
            name,
            file_path,
            is_bundled,
            mime_type,
            category,
        )
    }

    /// Insert with a caller-supplied UUID. Idempotent on uuid: a re-
    /// run with the same id skips the insert AND emits no event so a
    /// peer doesn't get a redundant duplicate-insert. Returns the
    /// existing rowid in that case. Used by the bundled-seed path
    /// where every device must end up with the same UUID per file.
    pub fn insert_bell_sound_with_uuid(
        &self,
        uuid_str: &str,
        name: &str,
        file_path: &str,
        is_bundled: bool,
        mime_type: &str,
        category: BellSoundCategory,
    ) -> Result<i64> {
        let tx = self.conn.unchecked_transaction()?;
        // Pre-check for an existing row with this uuid — return its
        // rowid without inserting or emitting an event.
        if let Some(existing) = self.conn.query_row(
            "SELECT id FROM bell_sounds WHERE uuid = ?1",
            params![uuid_str],
            |row| row.get::<_, i64>(0),
        ).optional()? {
            return Ok(existing);
        }
        let created_iso = chrono::Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO bell_sounds
                (uuid, name, file_path, is_bundled, mime_type, category, created_iso)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                uuid_str,
                name,
                file_path,
                is_bundled as i64,
                mime_type,
                category.as_db_str(),
                created_iso,
            ],
        )?;
        let rowid = self.conn.last_insert_rowid();
        let payload = serde_json::json!({
            "uuid": uuid_str,
            "name": name,
            "file_path": file_path,
            "is_bundled": is_bundled,
            "mime_type": mime_type,
            "category": category.as_db_str(),
            "created_iso": created_iso,
        }).to_string();
        self.emit_event("bell_sound_insert", uuid_str, payload)?;
        tx.commit()?;
        Ok(rowid)
    }

    /// Rename a bell sound. The only mutable property is `name`;
    /// file_path / is_bundled / mime_type are fixed at insert time.
    /// Unknown uuids are silent no-ops AND emit no event (mirrors
    /// the labels rename pattern). The event payload carries every
    /// field of the row so a peer that's missed the insert can still
    /// materialise from this rename alone.
    pub fn rename_bell_sound(&self, uuid_str: &str, name: &str) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        let row: Option<(String, i64, String, String, String)> = self.conn.query_row(
            "SELECT file_path, is_bundled, mime_type, category, created_iso
               FROM bell_sounds WHERE uuid = ?1",
            params![uuid_str],
            |row| Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            )),
        ).optional()?;
        let Some((file_path, is_bundled, mime_type, category, created_iso)) = row else {
            return Ok(());
        };
        self.conn.execute(
            "UPDATE bell_sounds SET name = ?1 WHERE uuid = ?2",
            params![name, uuid_str],
        )?;
        let payload = serde_json::json!({
            "uuid": uuid_str,
            "name": name,
            "file_path": file_path,
            "is_bundled": is_bundled != 0,
            "mime_type": mime_type,
            "category": category,
            "created_iso": created_iso,
        }).to_string();
        self.emit_event("bell_sound_update", uuid_str, payload)?;
        tx.commit()?;
        Ok(())
    }

    /// Remove a bell-sound row and emit a tombstone. Unknown uuids
    /// are silent no-ops AND emit no event. The UI gates by
    /// `is_bundled` to keep bundled rows from being deleted by mistake;
    /// no DB-level enforcement here.
    pub fn delete_bell_sound(&self, uuid_str: &str) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        let exists: Option<i64> = self.conn.query_row(
            "SELECT id FROM bell_sounds WHERE uuid = ?1",
            params![uuid_str],
            |row| row.get::<_, i64>(0),
        ).optional()?;
        if exists.is_none() {
            return Ok(());
        }
        self.conn.execute(
            "DELETE FROM bell_sounds WHERE uuid = ?1",
            params![uuid_str],
        )?;
        let payload = serde_json::json!({ "uuid": uuid_str }).to_string();
        self.emit_event("bell_sound_delete", uuid_str, payload)?;
        tx.commit()?;
        Ok(())
    }

    /// Every bell sound in insert order. The B.4.3 chooser renders
    /// this directly. id ASC keeps bundled rows (which get inserted
    /// first via the seed) at the top of the list.
    pub fn list_bell_sounds(&self) -> Result<Vec<BellSound>> {
        // Custom imports first (is_bundled = 0), then the curated
        // bundled set. The chooser places "Choose your own…" at the
        // very top, then this list — so the user's own imports sit
        // immediately under the import affordance instead of being
        // pushed to the bottom of a long bundled list.
        let mut stmt = self.conn.prepare(
            "SELECT id, uuid, name, file_path, is_bundled, mime_type, category, created_iso
             FROM bell_sounds
             ORDER BY is_bundled ASC, id ASC",
        )?;
        let rows = stmt
            .query_map([], |row| {
                let cat_str: String = row.get(6)?;
                Ok(BellSound {
                    id: row.get(0)?,
                    uuid: row.get(1)?,
                    name: row.get(2)?,
                    file_path: row.get(3)?,
                    is_bundled: row.get::<_, i64>(4)? != 0,
                    mime_type: row.get(5)?,
                    category: BellSoundCategory::from_db_str(&cat_str)
                        .unwrap_or(BellSoundCategory::General),
                    created_iso: row.get(7)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Same as `list_bell_sounds`, but filters to a single category.
    /// The chooser passes the category implied by its caller — bell
    /// rows pass General, Box Breath phase rows pass BoxBreath — so
    /// the user only sees sounds tailored to the slot they're filling.
    pub fn list_bell_sounds_for_category(
        &self,
        category: BellSoundCategory,
    ) -> Result<Vec<BellSound>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, uuid, name, file_path, is_bundled, mime_type, category, created_iso
             FROM bell_sounds
             WHERE category = ?1
             ORDER BY is_bundled ASC, id ASC",
        )?;
        let rows = stmt
            .query_map(params![category.as_db_str()], |row| {
                let cat_str: String = row.get(6)?;
                Ok(BellSound {
                    id: row.get(0)?,
                    uuid: row.get(1)?,
                    name: row.get(2)?,
                    file_path: row.get(3)?,
                    is_bundled: row.get::<_, i64>(4)? != 0,
                    mime_type: row.get(5)?,
                    category: BellSoundCategory::from_db_str(&cat_str)
                        .unwrap_or(BellSoundCategory::General),
                    created_iso: row.get(7)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Recompute the `bell_sounds` row for `sound_uuid` from the events
    /// table. Same precedence rules as labels / interval_bells:
    /// tombstone wins on tie, else the highest-(lamport, device_id)
    /// mutate event drives the row's values. Update events carry every
    /// field plus created_iso so they self-suffice if the corresponding
    /// insert event hasn't arrived yet.
    pub(super) fn recompute_bell_sound(&self, sound_uuid: &str) -> Result<()> {
        let delete_ts: Option<i64> = self.conn.query_row(
            "SELECT MAX(lamport_ts) FROM events
             WHERE target_id = ?1 AND kind = 'bell_sound_delete'",
            params![sound_uuid],
            |row| row.get::<_, Option<i64>>(0),
        )?;
        let mutate: Option<(i64, String)> = self.conn.query_row(
            "SELECT lamport_ts, payload FROM events
             WHERE target_id = ?1
               AND kind IN ('bell_sound_insert', 'bell_sound_update')
             ORDER BY lamport_ts DESC, device_id DESC
             LIMIT 1",
            params![sound_uuid],
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
                    format!("bell_sound event payload not valid JSON: {e}")))?;
            let name = v["name"].as_str().unwrap_or_default();
            let file_path = v["file_path"].as_str().unwrap_or_default();
            let is_bundled = v["is_bundled"].as_bool().unwrap_or(false);
            let mime_type = v["mime_type"].as_str().unwrap_or("audio/wav");
            // Old payloads (pre-categories) won't carry this field;
            // default to 'general' so peer rows materialise into the
            // bell context they came from.
            let category = v["category"].as_str().unwrap_or("general");
            let created_iso = v["created_iso"].as_str().unwrap_or_default();
            self.conn.execute(
                "INSERT INTO bell_sounds
                    (uuid, name, file_path, is_bundled, mime_type, category, created_iso)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(uuid) DO UPDATE SET
                    name        = excluded.name,
                    file_path   = excluded.file_path,
                    is_bundled  = excluded.is_bundled,
                    mime_type   = excluded.mime_type,
                    category    = excluded.category,
                    created_iso = excluded.created_iso",
                params![
                    sound_uuid,
                    name,
                    file_path,
                    is_bundled as i64,
                    mime_type,
                    category,
                    created_iso,
                ],
            )?;
        } else {
            self.conn.execute(
                "DELETE FROM bell_sounds WHERE uuid = ?1",
                params![sound_uuid],
            )?;
        }
        Ok(())
    }
}
