//! `bell_sounds` table — bundled + user-imported audio file library
//! shared across Starting / Interval / End bells (General category)
//! and Box-Breath per-phase cues (BoxBreath category).

use rusqlite::{params, OptionalExtension};

use super::events::EventKind;
use super::{Database, Result};

/// Bundled and user-imported audio files for cue playback. Categories
/// are mutually exclusive — no row sits in both — and the chooser
/// filters by the category its caller passes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BellSound {
    pub id: i64,
    pub uuid: super::BellSoundUuid,
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
        if let Some(existing) = self.existing_rowid_by_uuid("bell_sounds", uuid_str)? {
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
        self.emit_event(EventKind::BellSoundInsert, uuid_str, payload)?;
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
        self.emit_event(EventKind::BellSoundUpdate, uuid_str, payload)?;
        tx.commit()?;
        Ok(())
    }

    /// Remove a bell-sound row and emit a tombstone. Unknown uuids
    /// are silent no-ops AND emit no event. The UI gates by
    /// `is_bundled` to keep bundled rows from being deleted by mistake;
    /// no DB-level enforcement here.
    pub fn delete_bell_sound(&self, uuid_str: &str) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        if self.existing_rowid_by_uuid("bell_sounds", uuid_str)?.is_none() {
            return Ok(());
        }
        self.conn.execute(
            "DELETE FROM bell_sounds WHERE uuid = ?1",
            params![uuid_str],
        )?;
        let payload = serde_json::json!({ "uuid": uuid_str }).to_string();
        self.emit_event(EventKind::BellSoundDelete, uuid_str, payload)?;
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
        let Some(v) = self.winning_mutate(
            sound_uuid,
            [EventKind::BellSoundInsert, EventKind::BellSoundUpdate],
            EventKind::BellSoundDelete,
        )? else {
            self.conn.execute(
                "DELETE FROM bell_sounds WHERE uuid = ?1",
                params![sound_uuid],
            )?;
            return Ok(());
        };
        {
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
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Event;

    fn synth_bell_sound_insert(uuid: &str, lamport_ts: i64, device: &str, name: &str) -> Event {
        Event {
            event_uuid: format!("bs-insert-{uuid}-{lamport_ts}-{device}"),
            lamport_ts,
            device_id: device.to_string(),
            kind: "bell_sound_insert".to_string(),
            target_id: uuid.to_string(),
            payload: serde_json::json!({
                "uuid": uuid,
                "name": name,
                "file_path": format!("/path/{name}.wav"),
                "is_bundled": false,
                "mime_type": "audio/wav",
                "created_iso": "2026-05-03T00:00:00Z",
            }).to_string(),
        }
    }

    fn synth_bell_sound_delete(uuid: &str, lamport_ts: i64, device: &str) -> Event {
        Event {
            event_uuid: format!("bs-del-{uuid}-{lamport_ts}-{device}"),
            lamport_ts,
            device_id: device.to_string(),
            kind: "bell_sound_delete".to_string(),
            target_id: uuid.to_string(),
            payload: serde_json::json!({ "uuid": uuid }).to_string(),
        }
    }

    #[test]
    fn insert_bell_sound_inserts_a_row_with_uuid_and_returns_rowid() {
        let db = Database::open_in_memory().unwrap();
        let rowid = db
            .insert_bell_sound(
                "Tibetan Bowl",
                "/io/github/janekbt/Meditate/sounds/bowl.wav",
                true,
                "audio/wav",
                BellSoundCategory::General,
            )
            .unwrap();
        assert!(rowid > 0);
        let sounds = db.list_bell_sounds().unwrap();
        assert_eq!(sounds.len(), 1);
        let s = &sounds[0];
        assert_eq!(s.id, rowid);
        assert!(!s.uuid.is_empty());
        assert_eq!(s.name, "Tibetan Bowl");
        assert_eq!(s.file_path, "/io/github/janekbt/Meditate/sounds/bowl.wav");
        assert!(s.is_bundled);
        assert_eq!(s.mime_type, "audio/wav");
        assert_eq!(s.category, BellSoundCategory::General);
        assert!(!s.created_iso.is_empty());
    }

    #[test]
    fn insert_bell_sound_with_explicit_uuid_uses_it() {
        let db = Database::open_in_memory().unwrap();
        let fixed = "11111111-2222-3333-4444-555555555555";
        let rowid = db
            .insert_bell_sound_with_uuid(
                fixed,
                "Bundled bowl",
                "/io/github/janekbt/Meditate/sounds/bowl.wav",
                true,
                "audio/wav",
                BellSoundCategory::General,
            )
            .unwrap();
        assert!(rowid > 0);
        let s = &db.list_bell_sounds().unwrap()[0];
        assert_eq!(s.uuid, fixed);
    }

    #[test]
    fn insert_bell_sound_with_existing_uuid_is_silent_noop() {
        let db = Database::open_in_memory().unwrap();
        let fixed = "22222222-2222-3333-4444-555555555555";
        let r1 = db.insert_bell_sound_with_uuid(
            fixed, "Bowl", "/path/bowl.wav", true, "audio/wav",
            BellSoundCategory::General,
        ).unwrap();
        let r2 = db.insert_bell_sound_with_uuid(
            fixed, "Bowl", "/path/bowl.wav", true, "audio/wav",
            BellSoundCategory::General,
        ).unwrap();
        assert_eq!(r1, r2);
        assert_eq!(db.list_bell_sounds().unwrap().len(), 1);
        let inserts: Vec<_> = db.pending_events().unwrap()
            .into_iter()
            .filter(|(_, e)| e.kind == "bell_sound_insert")
            .collect();
        assert_eq!(inserts.len(), 1);
    }

    #[test]
    fn bell_sound_category_round_trips_through_db_str() {
        for c in [BellSoundCategory::General, BellSoundCategory::BoxBreath] {
            assert_eq!(BellSoundCategory::from_db_str(c.as_db_str()), Some(c));
        }
        assert_eq!(BellSoundCategory::from_db_str("typo"), None);
    }

    #[test]
    fn bell_sounds_category_check_constraint_rejects_unknown_value() {
        let db = Database::open_in_memory().unwrap();
        let bad = db.conn.execute(
            "INSERT INTO bell_sounds
                (uuid, name, file_path, is_bundled, mime_type, category, created_iso)
             VALUES ('u', 'n', '/p', 0, 'audio/wav', 'spiral', 'now')",
            [],
        );
        assert!(bad.is_err());
    }

    #[test]
    fn list_bell_sounds_for_category_filters_by_category() {
        let db = Database::open_in_memory().unwrap();
        db.insert_bell_sound("Bowl", "/p/bowl.wav", true, "audio/wav", BellSoundCategory::General).unwrap();
        db.insert_bell_sound("Inhale", "/p/inhale.ogg", false, "audio/ogg", BellSoundCategory::BoxBreath).unwrap();
        db.insert_bell_sound("Hold", "/p/hold.ogg", false, "audio/ogg", BellSoundCategory::BoxBreath).unwrap();

        let general = db.list_bell_sounds_for_category(BellSoundCategory::General).unwrap();
        assert_eq!(general.len(), 1);
        assert_eq!(general[0].name, "Bowl");

        let box_breath = db.list_bell_sounds_for_category(BellSoundCategory::BoxBreath).unwrap();
        assert_eq!(box_breath.len(), 2);
        let names: Vec<&str> = box_breath.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"Inhale"));
        assert!(names.contains(&"Hold"));
    }

    #[test]
    fn list_bell_sounds_unfiltered_returns_every_category() {
        let db = Database::open_in_memory().unwrap();
        db.insert_bell_sound("Bowl", "/p/bowl.wav", true, "audio/wav", BellSoundCategory::General).unwrap();
        db.insert_bell_sound("Inhale", "/p/inhale.ogg", false, "audio/ogg", BellSoundCategory::BoxBreath).unwrap();
        assert_eq!(db.list_bell_sounds().unwrap().len(), 2);
    }

    #[test]
    fn insert_bell_sound_emits_a_bell_sound_insert_event() {
        let db = Database::open_in_memory().unwrap();
        db.insert_bell_sound("Zen Bell", "/path/zen.wav", true, "audio/wav", BellSoundCategory::General).unwrap();
        let events = db.pending_events().unwrap();
        let inserts: Vec<_> = events
            .iter()
            .filter(|(_, e)| e.kind == "bell_sound_insert")
            .collect();
        assert_eq!(inserts.len(), 1);
        let payload: serde_json::Value = serde_json::from_str(&inserts[0].1.payload).unwrap();
        assert_eq!(payload["name"], "Zen Bell");
        assert_eq!(payload["file_path"], "/path/zen.wav");
        assert_eq!(payload["is_bundled"], true);
        assert_eq!(payload["mime_type"], "audio/wav");
        assert!(payload["uuid"].is_string());
        assert!(payload["created_iso"].is_string());
    }

    #[test]
    fn list_bell_sounds_returns_custom_rows_before_bundled() {
        let db = Database::open_in_memory().unwrap();
        db.insert_bell_sound("A", "/p/a.wav", true, "audio/wav", BellSoundCategory::General).unwrap();
        db.insert_bell_sound("B", "/p/b.wav", false, "audio/wav", BellSoundCategory::General).unwrap();
        db.insert_bell_sound("C", "/p/c.wav", true, "audio/wav", BellSoundCategory::General).unwrap();
        db.insert_bell_sound("D", "/p/d.wav", false, "audio/wav", BellSoundCategory::General).unwrap();
        let s = db.list_bell_sounds().unwrap();
        let names: Vec<_> = s.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, vec!["B", "D", "A", "C"]);
    }

    #[test]
    fn list_bell_sounds_returns_empty_when_none_inserted() {
        let db = Database::open_in_memory().unwrap();
        assert!(db.list_bell_sounds().unwrap().is_empty());
    }

    #[test]
    fn rename_bell_sound_changes_name_and_emits_update_event() {
        let db = Database::open_in_memory().unwrap();
        db.insert_bell_sound("Bowl", "/p/bowl.wav", true, "audio/wav", BellSoundCategory::General).unwrap();
        let uuid = db.list_bell_sounds().unwrap()[0].uuid.clone();
        db.rename_bell_sound(uuid.as_str(), "Singing Bowl").unwrap();
        assert_eq!(db.list_bell_sounds().unwrap()[0].name, "Singing Bowl");
        let updates: Vec<_> = db.pending_events().unwrap()
            .into_iter()
            .filter(|(_, e)| e.kind == "bell_sound_update")
            .collect();
        assert_eq!(updates.len(), 1);
        let payload: serde_json::Value = serde_json::from_str(&updates[0].1.payload).unwrap();
        assert_eq!(payload["name"], "Singing Bowl");
        assert_eq!(payload["uuid"], uuid.0);
        assert_eq!(payload["file_path"], "/p/bowl.wav");
        assert_eq!(payload["is_bundled"], true);
        assert_eq!(payload["mime_type"], "audio/wav");
    }

    #[test]
    fn rename_bell_sound_unknown_uuid_is_silent_noop() {
        let db = Database::open_in_memory().unwrap();
        db.rename_bell_sound("non-existent", "Bowl").unwrap();
        let updates: Vec<_> = db.pending_events().unwrap()
            .into_iter()
            .filter(|(_, e)| e.kind == "bell_sound_update")
            .collect();
        assert!(updates.is_empty());
    }

    #[test]
    fn delete_bell_sound_removes_the_row_and_emits_tombstone() {
        let db = Database::open_in_memory().unwrap();
        db.insert_bell_sound("Bowl", "/p/bowl.wav", false, "audio/wav", BellSoundCategory::General).unwrap();
        let uuid = db.list_bell_sounds().unwrap()[0].uuid.clone();
        db.delete_bell_sound(uuid.as_str()).unwrap();
        assert!(db.list_bell_sounds().unwrap().is_empty());
        let deletes: Vec<_> = db.pending_events().unwrap()
            .into_iter()
            .filter(|(_, e)| e.kind == "bell_sound_delete")
            .collect();
        assert_eq!(deletes.len(), 1);
        assert_eq!(deletes[0].1.target_id, uuid);
    }

    #[test]
    fn delete_bell_sound_unknown_uuid_is_silent_noop() {
        let db = Database::open_in_memory().unwrap();
        db.delete_bell_sound("non-existent").unwrap();
        let deletes: Vec<_> = db.pending_events().unwrap()
            .into_iter()
            .filter(|(_, e)| e.kind == "bell_sound_delete")
            .collect();
        assert!(deletes.is_empty());
    }

    #[test]
    fn apply_event_bell_sound_insert_creates_the_row() {
        let db = Database::open_in_memory().unwrap();
        db.apply_event(&synth_bell_sound_insert("bs-1", 5, "dev-A", "Bowl")).unwrap();
        let s = &db.list_bell_sounds().unwrap()[0];
        assert_eq!(s.uuid, "bs-1");
        assert_eq!(s.name, "Bowl");
        assert_eq!(s.file_path, "/path/Bowl.wav");
    }

    #[test]
    fn apply_event_bell_sound_delete_removes_the_row() {
        let db = Database::open_in_memory().unwrap();
        db.apply_event(&synth_bell_sound_insert("bs-1", 5, "dev-A", "Bowl")).unwrap();
        db.apply_event(&synth_bell_sound_delete("bs-1", 6, "dev-A")).unwrap();
        assert!(db.list_bell_sounds().unwrap().is_empty());
    }

    #[test]
    fn apply_event_bell_sound_tombstone_resists_lower_lamport_insert() {
        let db = Database::open_in_memory().unwrap();
        db.apply_event(&synth_bell_sound_delete("bs-1", 10, "dev-A")).unwrap();
        db.apply_event(&synth_bell_sound_insert("bs-1", 5, "dev-A", "Bowl")).unwrap();
        assert!(db.list_bell_sounds().unwrap().is_empty());
    }

    #[test]
    fn apply_event_bell_sound_replay_round_trip_across_peers() {
        let dev_a = Database::open_in_memory().unwrap();
        dev_a.insert_bell_sound("Bowl", "/p/bowl.wav", true, "audio/wav", BellSoundCategory::General).unwrap();
        let uuid = dev_a.list_bell_sounds().unwrap()[0].uuid.clone();
        dev_a.rename_bell_sound(uuid.as_str(), "Singing Bowl").unwrap();

        let events: Vec<Event> = dev_a.pending_events().unwrap()
            .into_iter()
            .filter(|(_, e)| e.kind.starts_with("bell_sound_"))
            .map(|(_, e)| e)
            .collect();

        let dev_b = Database::open_in_memory().unwrap();
        dev_b.replay_events(&events).unwrap();
        let sounds = dev_b.list_bell_sounds().unwrap();
        assert_eq!(sounds.len(), 1);
        assert_eq!(sounds[0].uuid, uuid);
        assert_eq!(sounds[0].name, "Singing Bowl");
        assert!(sounds[0].is_bundled);
        assert_eq!(sounds[0].file_path, "/p/bowl.wav");
    }
}
