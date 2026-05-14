//! `guided_files` table — user's imported guided-meditation tracks.
//! Same idempotent-insert + UNIQUE-NOCASE-name shape as bell_sounds
//! and presets. Imports flow through `_with_uuid` so the on-disk
//! filename can encode the same uuid the row uses.

use rusqlite::{params, OptionalExtension};

use super::events::EventKind;
use super::{
    conflict_suffixed_name, is_unique_constraint_error, map_unique_err,
    Database, DbError, Result,
};

/// One entry in the user's guided-meditation file library — an audio
/// track imported via the file picker, transcoded to OGG, and stored
/// under the app's data dir. Referenced by `sessions.guided_file_uuid`
/// for per-file aggregates. `is_starred` controls whether the row
/// shows up directly in the home-screen list (mirrors the preset
/// star flag); destarred files only appear inside the Manage Files
/// chooser. `file_path` is a relative path under the per-device data
/// dir (the binary itself doesn't sync — peers fetch it via WebDAV
/// the same way custom bell-sound binaries do).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuidedFile {
    pub id: i64,
    pub uuid: super::GuidedFileUuid,
    pub name: String,
    pub file_path: String,
    pub duration_secs: u32,
    pub is_starred: bool,
    pub created_iso: String,
    pub updated_iso: String,
}

pub fn list_guided_files_from_db(db: &Database) -> Result<Vec<GuidedFile>> {
    let mut stmt = db.conn.prepare(
        "SELECT id, uuid, name, file_path, duration_secs, is_starred, created_iso, updated_iso
         FROM guided_files
         ORDER BY created_iso ASC, id ASC",
    )?;
    let rows = stmt
        .query_map([], |row| {
            Ok(GuidedFile {
                id: row.get(0)?,
                uuid: row.get(1)?,
                name: row.get(2)?,
                file_path: row.get(3)?,
                duration_secs: row.get::<_, i64>(4)? as u32,
                is_starred: row.get::<_, i64>(5)? != 0,
                created_iso: row.get(6)?,
                updated_iso: row.get(7)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}
/// Look up a guided-file row by its cross-device uuid. Returns
/// None if no row matches. Used by the chooser sub-row, the
/// session-save path (resolving the home-list selection's
/// uuid → row → file_path / duration), and the import flow's
/// "did this UUID land?" check.
pub fn find_guided_file_by_uuid_from_db(db: &Database, uuid_str: &str) -> Result<Option<GuidedFile>> {
    let row = db.conn.query_row(
        "SELECT id, uuid, name, file_path, duration_secs, is_starred, created_iso, updated_iso
         FROM guided_files WHERE uuid = ?1",
        params![uuid_str],
        |row| Ok(GuidedFile {
            id: row.get(0)?,
            uuid: row.get(1)?,
            name: row.get(2)?,
            file_path: row.get(3)?,
            duration_secs: row.get::<_, i64>(4)? as u32,
            is_starred: row.get::<_, i64>(5)? != 0,
            created_iso: row.get(6)?,
            updated_iso: row.get(7)?,
        }),
    ).optional()?;
    Ok(row)
}
/// True iff a row other than `except_uuid` already holds `name`
/// (case-insensitive). The import / rename dialogs use this to
/// live-validate input — `except_uuid` must be the row currently
/// being renamed (or "" for fresh imports) so the user's own
/// case-only renames don't false-positive.
pub fn is_guided_file_name_taken_from_db(db: &Database, name: &str, except_uuid: &str) -> Result<bool> {
    let count: i64 = db.conn.query_row(
        "SELECT COUNT(*) FROM guided_files
          WHERE name = ?1 COLLATE NOCASE AND uuid != ?2",
        params![name, except_uuid],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}
impl Database {
    /// Insert a row keyed on `uuid_str`. Idempotent — a second call
    /// with the same uuid returns the existing rowid without touching
    /// the row or emitting another event (mirrors bell_sounds /
    /// presets). Returns `DuplicateGuidedFile(name)` if a row with
    /// the same case-insensitive `name` already exists under a
    /// different uuid; the shell surfaces that as a "name already
    /// taken" toast on the import / rename dialog.
    pub fn insert_guided_file_with_uuid(
        &self,
        uuid_str: &str,
        name: &str,
        file_path: &str,
        duration_secs: u32,
        is_starred: bool,
    ) -> Result<i64> {
        let tx = self.conn.unchecked_transaction()?;
        if let Some(existing) = self.existing_rowid_by_uuid("guided_files", uuid_str)? {
            return Ok(existing);
        }
        let now_iso = chrono::Utc::now().to_rfc3339();
        self.conn
            .execute(
                "INSERT INTO guided_files
                    (uuid, name, file_path, duration_secs, is_starred, created_iso, updated_iso)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
                params![
                    uuid_str,
                    name,
                    file_path,
                    duration_secs,
                    i64::from(is_starred),
                    now_iso,
                ],
            )
            .map_err(|e| map_unique_err(e, || DbError::DuplicateGuidedFile(name.to_string())))?;
        let rowid = self.conn.last_insert_rowid();
        let payload = serde_json::json!({
            "uuid": uuid_str,
            "name": name,
            "file_path": file_path,
            "duration_secs": duration_secs,
            "is_starred": is_starred,
            "created_iso": now_iso,
            "updated_iso": now_iso,
        }).to_string();
        self.emit_event(EventKind::GuidedFileInsert, uuid_str, payload)?;
        tx.commit()?;
        Ok(rowid)
    }


    /// Rename a guided-file row. Bumps `updated_iso` so the recompute
    /// helper can resolve concurrent renames by lamport timestamp.
    /// Unknown uuids are silent no-ops AND emit no event (mirrors the
    /// bell-sound / preset rename pattern). Returns
    /// `DuplicateGuidedFile(name)` if another row already holds the
    /// new name (case-insensitive).
    pub fn rename_guided_file(&self, uuid_str: &str, name: &str) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        let row: Option<(String, i64, String)> = self.conn.query_row(
            "SELECT file_path, duration_secs, created_iso
               FROM guided_files WHERE uuid = ?1",
            params![uuid_str],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        ).optional()?;
        let Some((file_path, duration_secs, created_iso)) = row else {
            return Ok(());
        };
        let now_iso = chrono::Utc::now().to_rfc3339();
        self.conn
            .execute(
                "UPDATE guided_files SET name = ?1, updated_iso = ?2 WHERE uuid = ?3",
                params![name, now_iso, uuid_str],
            )
            .map_err(|e| map_unique_err(e, || DbError::DuplicateGuidedFile(name.to_string())))?;
        // Read back is_starred so the event payload carries every
        // field — peers that missed the insert can still materialise
        // from this single update event alone.
        let is_starred: bool = self.conn.query_row(
            "SELECT is_starred FROM guided_files WHERE uuid = ?1",
            params![uuid_str],
            |row| row.get::<_, i64>(0),
        )? != 0;
        let payload = serde_json::json!({
            "uuid": uuid_str,
            "name": name,
            "file_path": file_path,
            "duration_secs": duration_secs,
            "is_starred": is_starred,
            "created_iso": created_iso,
            "updated_iso": now_iso,
        }).to_string();
        self.emit_event(EventKind::GuidedFileUpdate, uuid_str, payload)?;
        tx.commit()?;
        Ok(())
    }

    /// Toggle the home-screen-pin flag. Same payload shape as rename
    /// so peers can replay either change with the same recompute path.
    pub fn set_guided_file_starred(&self, uuid_str: &str, starred: crate::db::StarredState) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        let row: Option<(String, String, i64, String)> = self.conn.query_row(
            "SELECT name, file_path, duration_secs, created_iso
               FROM guided_files WHERE uuid = ?1",
            params![uuid_str],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        ).optional()?;
        let Some((name, file_path, duration_secs, created_iso)) = row else {
            return Ok(());
        };
        let now_iso = chrono::Utc::now().to_rfc3339();
        let is_starred = starred.is_starred();
        self.conn.execute(
            "UPDATE guided_files SET is_starred = ?1, updated_iso = ?2 WHERE uuid = ?3",
            params![i64::from(is_starred), now_iso, uuid_str],
        )?;
        let payload = serde_json::json!({
            "uuid": uuid_str,
            "name": name,
            "file_path": file_path,
            "duration_secs": duration_secs,
            "is_starred": is_starred,
            "created_iso": created_iso,
            "updated_iso": now_iso,
        }).to_string();
        self.emit_event(EventKind::GuidedFileUpdate, uuid_str, payload)?;
        tx.commit()?;
        Ok(())
    }



    /// Drop a guided-file row. Unknown uuids are silent no-ops AND
    /// emit no event — peers would otherwise see a tombstone for a
    /// row they never knew existed. Mirrors bell_sounds / presets.
    pub fn delete_guided_file(&self, uuid_str: &str) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        let exists: bool = self.conn.query_row(
            "SELECT 1 FROM guided_files WHERE uuid = ?1",
            params![uuid_str],
            |_| Ok(true),
        ).optional()?.unwrap_or(false);
        if !exists { return Ok(()); }
        self.conn.execute(
            "DELETE FROM guided_files WHERE uuid = ?1",
            params![uuid_str],
        )?;
        let payload = serde_json::json!({ "uuid": uuid_str }).to_string();
        self.emit_event(EventKind::GuidedFileDelete, uuid_str, payload)?;
        tx.commit()?;
        Ok(())
    }

    /// Recompute the `guided_files` row for `file_uuid` from the events
    /// table. Same precedence rules as labels / interval_bells / presets:
    /// tombstone wins on tie, else the highest-(lamport, device_id)
    /// mutate event drives the row. Update events carry every field
    /// (name, file_path, duration, is_starred, both timestamps) so they
    /// self-suffice on out-of-order delivery.
    pub(super) fn recompute_guided_file(&self, file_uuid: &str) -> Result<()> {
        let Some(v) = self.winning_mutate(
            file_uuid,
            [EventKind::GuidedFileInsert, EventKind::GuidedFileUpdate],
            EventKind::GuidedFileDelete,
        )? else {
            self.conn.execute(
                "DELETE FROM guided_files WHERE uuid = ?1",
                params![file_uuid],
            )?;
            return Ok(());
        };
        {
            let name = v["name"].as_str().unwrap_or_default();
            let file_path = v["file_path"].as_str().unwrap_or_default();
            let duration_secs = v["duration_secs"].as_u64().unwrap_or(0) as u32;
            let is_starred = v["is_starred"].as_bool().unwrap_or(false);
            let created_iso = v["created_iso"].as_str().unwrap_or_default();
            let updated_iso = v["updated_iso"].as_str().unwrap_or(created_iso);
            let upsert_sql = "INSERT INTO guided_files
                    (uuid, name, file_path, duration_secs, is_starred, created_iso, updated_iso)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(uuid) DO UPDATE SET
                    name          = excluded.name,
                    file_path     = excluded.file_path,
                    duration_secs = excluded.duration_secs,
                    is_starred    = excluded.is_starred,
                    created_iso   = excluded.created_iso,
                    updated_iso   = excluded.updated_iso";
            let first = self.conn.execute(upsert_sql, params![
                file_uuid, name, file_path, duration_secs,
                i64::from(is_starred), created_iso, updated_iso,
            ]);
            match first {
                Ok(_) => {}
                Err(e) if is_unique_constraint_error(&e) => {
                    let suffixed = conflict_suffixed_name(name, file_uuid);
                    crate::diag::log(
                        "guided_file.name_collision",
                        &format!(
                            "uuid={file_uuid} original={name:?} \
                             resolved={suffixed:?}"
                        ),
                    );
                    self.conn.execute(upsert_sql, params![
                        file_uuid, suffixed, file_path, duration_secs,
                        i64::from(is_starred), created_iso, updated_iso,
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
    use crate::test_macros::assert_matches;

    #[test]
    fn list_guided_files_is_empty_on_a_fresh_database() {
        let db = Database::open_in_memory().unwrap();
        assert!(list_guided_files_from_db(&db).unwrap().is_empty());
    }

    #[test]
    fn insert_guided_file_with_uuid_round_trips_through_list() {
        let db = Database::open_in_memory().unwrap();
        db.insert_guided_file_with_uuid(
            "gf-1", "Body Scan", "guided/gf-1.ogg", 1200, true,
        ).unwrap();
        let rows = list_guided_files_from_db(&db).unwrap();
        assert_eq!(rows.len(), 1);
        let r = &rows[0];
        assert_eq!(r.uuid, "gf-1");
        assert_eq!(r.name, "Body Scan");
        assert_eq!(r.file_path, "guided/gf-1.ogg");
        assert_eq!(r.duration_secs, 1200);
        assert!(r.is_starred);
        assert!(!r.created_iso.is_empty());
        assert_eq!(r.created_iso, r.updated_iso);
    }

    #[test]
    fn insert_guided_file_with_existing_uuid_is_silent_noop() {
        let db = Database::open_in_memory().unwrap();
        let id1 = db.insert_guided_file_with_uuid(
            "gf-1", "Body Scan", "guided/gf-1.ogg", 1200, false,
        ).unwrap();
        let id2 = db.insert_guided_file_with_uuid(
            "gf-1", "Different Name", "guided/different.ogg", 999, true,
        ).unwrap();
        assert_eq!(id1, id2);
        let rows = list_guided_files_from_db(&db).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "Body Scan");
        assert_eq!(rows[0].duration_secs, 1200);
    }

    #[test]
    fn insert_guided_file_with_duplicate_name_returns_duplicate_error() {
        let db = Database::open_in_memory().unwrap();
        db.insert_guided_file_with_uuid(
            "gf-1", "Body Scan", "guided/gf-1.ogg", 1200, false,
        ).unwrap();
        assert_matches!(
            db.insert_guided_file_with_uuid(
                "gf-2", "BODY SCAN", "guided/gf-2.ogg", 800, false,
            ),
            Err(DbError::DuplicateGuidedFile(name)) => assert_eq!(name, "BODY SCAN"),
        );
    }

    #[test]
    fn list_guided_files_orders_by_created_iso() {
        let db = Database::open_in_memory().unwrap();
        db.insert_guided_file_with_uuid(
            "gf-1", "First", "guided/1.ogg", 600, true,
        ).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        db.insert_guided_file_with_uuid(
            "gf-2", "Second", "guided/2.ogg", 1200, true,
        ).unwrap();
        let rows = list_guided_files_from_db(&db).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].name, "First");
        assert_eq!(rows[1].name, "Second");
    }

    #[test]
    fn delete_guided_file_removes_the_row() {
        let db = Database::open_in_memory().unwrap();
        db.insert_guided_file_with_uuid(
            "gf-1", "Body Scan", "guided/gf-1.ogg", 1200, true,
        ).unwrap();
        db.delete_guided_file("gf-1").unwrap();
        assert!(list_guided_files_from_db(&db).unwrap().is_empty());
    }

    #[test]
    fn delete_guided_file_unknown_uuid_is_silent_noop() {
        let db = Database::open_in_memory().unwrap();
        db.delete_guided_file("never-existed").unwrap();
        assert!(list_guided_files_from_db(&db).unwrap().is_empty());
    }

    #[test]
    fn rename_guided_file_changes_name_and_bumps_updated_iso() {
        let db = Database::open_in_memory().unwrap();
        db.insert_guided_file_with_uuid(
            "gf-1", "Old Name", "guided/gf-1.ogg", 600, false,
        ).unwrap();
        let before = list_guided_files_from_db(&db).unwrap()[0].clone();
        std::thread::sleep(std::time::Duration::from_millis(10));
        db.rename_guided_file("gf-1", "New Name").unwrap();
        let after = list_guided_files_from_db(&db).unwrap()[0].clone();
        assert_eq!(after.name, "New Name");
        assert_eq!(after.created_iso, before.created_iso);
        assert!(after.updated_iso > before.updated_iso);
    }

    #[test]
    fn rename_guided_file_unknown_uuid_is_silent_noop() {
        let db = Database::open_in_memory().unwrap();
        db.rename_guided_file("never-existed", "anything").unwrap();
        let renames: Vec<_> = db.pending_events().unwrap()
            .into_iter()
            .filter(|(_, e)| e.kind == "guided_file_update")
            .collect();
        assert!(renames.is_empty());
    }

    #[test]
    fn rename_guided_file_to_existing_name_returns_duplicate_error() {
        let db = Database::open_in_memory().unwrap();
        db.insert_guided_file_with_uuid(
            "gf-1", "Body Scan", "guided/gf-1.ogg", 600, false,
        ).unwrap();
        db.insert_guided_file_with_uuid(
            "gf-2", "Loving Kindness", "guided/gf-2.ogg", 900, false,
        ).unwrap();
        assert_matches!(
            db.rename_guided_file("gf-2", "Body Scan"),
            Err(DbError::DuplicateGuidedFile(name)) => assert_eq!(name, "Body Scan"),
        );
    }

    #[test]
    fn rename_guided_file_to_same_name_with_different_case_is_accepted() {
        let db = Database::open_in_memory().unwrap();
        db.insert_guided_file_with_uuid(
            "gf-1", "body scan", "guided/gf-1.ogg", 600, false,
        ).unwrap();
        db.rename_guided_file("gf-1", "Body Scan").unwrap();
        assert_eq!(list_guided_files_from_db(&db).unwrap()[0].name, "Body Scan");
    }

    #[test]
    fn set_guided_file_starred_toggles_and_emits_event() {
        let db = Database::open_in_memory().unwrap();
        db.insert_guided_file_with_uuid(
            "gf-1", "Body Scan", "guided/gf-1.ogg", 600, false,
        ).unwrap();
        db.set_guided_file_starred("gf-1", crate::db::StarredState::Starred).unwrap();
        assert!(list_guided_files_from_db(&db).unwrap()[0].is_starred);
        db.set_guided_file_starred("gf-1", crate::db::StarredState::Unstarred).unwrap();
        assert!(!list_guided_files_from_db(&db).unwrap()[0].is_starred);
        let updates: Vec<_> = db.pending_events().unwrap()
            .into_iter()
            .filter(|(_, e)| e.kind == "guided_file_update")
            .collect();
        assert_eq!(updates.len(), 2);
    }

    #[test]
    fn set_guided_file_starred_unknown_uuid_is_silent_noop() {
        let db = Database::open_in_memory().unwrap();
        db.set_guided_file_starred("never-existed", crate::db::StarredState::Starred).unwrap();
        let events: Vec<_> = db.pending_events().unwrap()
            .into_iter()
            .filter(|(_, e)| e.kind.starts_with("guided_file"))
            .collect();
        assert!(events.is_empty());
    }

    #[test]
    fn find_guided_file_by_uuid_returns_some_when_present() {
        let db = Database::open_in_memory().unwrap();
        db.insert_guided_file_with_uuid(
            "gf-1", "Body Scan", "guided/gf-1.ogg", 600, true,
        ).unwrap();
        let got = find_guided_file_by_uuid_from_db(&db, "gf-1").unwrap();
        assert!(got.is_some());
        let r = got.unwrap();
        assert_eq!(r.name, "Body Scan");
        assert_eq!(r.duration_secs, 600);
        assert!(r.is_starred);
    }

    #[test]
    fn find_guided_file_by_uuid_returns_none_when_missing() {
        let db = Database::open_in_memory().unwrap();
        assert!(find_guided_file_by_uuid_from_db(&db, "never-existed").unwrap().is_none());
    }

    #[test]
    fn is_guided_file_name_taken_matches_case_insensitively() {
        let db = Database::open_in_memory().unwrap();
        db.insert_guided_file_with_uuid(
            "gf-1", "Body Scan", "guided/gf-1.ogg", 600, false,
        ).unwrap();
        assert!(is_guided_file_name_taken_from_db(&db, "Body Scan", "").unwrap());
        assert!(is_guided_file_name_taken_from_db(&db, "body scan", "").unwrap());
        assert!(is_guided_file_name_taken_from_db(&db, "BODY SCAN", "").unwrap());
        assert!(!is_guided_file_name_taken_from_db(&db, "Different Name", "").unwrap());
    }

    #[test]
    fn is_guided_file_name_taken_excludes_the_row_being_renamed() {
        let db = Database::open_in_memory().unwrap();
        db.insert_guided_file_with_uuid(
            "gf-1", "Body Scan", "guided/gf-1.ogg", 600, false,
        ).unwrap();
        assert!(!is_guided_file_name_taken_from_db(&db, "Body Scan", "gf-1").unwrap());
        assert!(!is_guided_file_name_taken_from_db(&db, "body scan", "gf-1").unwrap());
        assert!(is_guided_file_name_taken_from_db(&db, "Body Scan", "gf-2").unwrap());
    }

    #[test]
    fn apply_event_guided_file_insert_creates_the_row_on_a_fresh_peer() {
        let peer = Database::open_in_memory().unwrap();
        let payload = serde_json::json!({
            "uuid": "gf-1",
            "name": "Body Scan",
            "file_path": "guided/gf-1.ogg",
            "duration_secs": 1200,
            "is_starred": true,
            "created_iso": "2026-05-05T20:00:00Z",
            "updated_iso": "2026-05-05T20:00:00Z",
        });
        let event = synth_event(
            "guided_file_insert", "gf-1", 5, DEVICE_A, payload,
        );
        peer.apply_event(&event).unwrap();
        let rows = list_guided_files_from_db(&peer).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "Body Scan");
        assert_eq!(rows[0].duration_secs, 1200);
        assert!(rows[0].is_starred);
    }

    #[test]
    fn apply_event_guided_file_update_overwrites_earlier_state() {
        let peer = Database::open_in_memory().unwrap();
        let insert_payload = serde_json::json!({
            "uuid": "gf-1",
            "name": "Old Name",
            "file_path": "guided/gf-1.ogg",
            "duration_secs": 1200,
            "is_starred": false,
            "created_iso": "2026-05-05T20:00:00Z",
            "updated_iso": "2026-05-05T20:00:00Z",
        });
        peer.apply_event(&synth_event(
            "guided_file_insert", "gf-1", 5, DEVICE_A, insert_payload,
        )).unwrap();
        let update_payload = serde_json::json!({
            "uuid": "gf-1",
            "name": "New Name",
            "file_path": "guided/gf-1.ogg",
            "duration_secs": 1200,
            "is_starred": true,
            "created_iso": "2026-05-05T20:00:00Z",
            "updated_iso": "2026-05-05T20:05:00Z",
        });
        peer.apply_event(&synth_event(
            "guided_file_update", "gf-1", 7, DEVICE_A, update_payload,
        )).unwrap();
        let rows = list_guided_files_from_db(&peer).unwrap();
        assert_eq!(rows[0].name, "New Name");
        assert!(rows[0].is_starred);
    }

    #[test]
    fn apply_event_guided_file_delete_removes_the_row() {
        let peer = Database::open_in_memory().unwrap();
        let insert_payload = serde_json::json!({
            "uuid": "gf-1",
            "name": "Body Scan",
            "file_path": "guided/gf-1.ogg",
            "duration_secs": 1200,
            "is_starred": false,
            "created_iso": "2026-05-05T20:00:00Z",
            "updated_iso": "2026-05-05T20:00:00Z",
        });
        peer.apply_event(&synth_event(
            "guided_file_insert", "gf-1", 5, DEVICE_A, insert_payload,
        )).unwrap();
        peer.apply_event(&synth_event(
            "guided_file_delete", "gf-1", 7, DEVICE_A,
            serde_json::json!({ "uuid": "gf-1" }),
        )).unwrap();
        assert!(list_guided_files_from_db(&peer).unwrap().is_empty());
    }

    #[test]
    fn apply_event_guided_file_tombstone_resists_lower_lamport_insert() {
        let peer = Database::open_in_memory().unwrap();
        peer.apply_event(&synth_event(
            "guided_file_delete", "gf-1", 10, DEVICE_A,
            serde_json::json!({ "uuid": "gf-1" }),
        )).unwrap();
        let insert_payload = serde_json::json!({
            "uuid": "gf-1",
            "name": "Body Scan",
            "file_path": "guided/gf-1.ogg",
            "duration_secs": 1200,
            "is_starred": false,
            "created_iso": "2026-05-05T20:00:00Z",
            "updated_iso": "2026-05-05T20:00:00Z",
        });
        peer.apply_event(&synth_event(
            "guided_file_insert", "gf-1", 5, DEVICE_A, insert_payload,
        )).unwrap();
        assert!(list_guided_files_from_db(&peer).unwrap().is_empty());
    }

    #[test]
    fn replay_guided_file_events_round_trips_to_a_fresh_peer() {
        let dev_a = Database::open_in_memory().unwrap();
        dev_a.insert_guided_file_with_uuid(
            "gf-1", "Body Scan", "guided/gf-1.ogg", 1200, true,
        ).unwrap();
        dev_a.set_guided_file_starred("gf-1", crate::db::StarredState::Unstarred).unwrap();
        let events: Vec<Event> = dev_a.pending_events().unwrap()
            .into_iter().map(|(_, e)| e).collect();

        let dev_b = Database::open_in_memory().unwrap();
        dev_b.replay_events(&events).unwrap();
        let rows = list_guided_files_from_db(&dev_b).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "Body Scan");
        assert!(!rows[0].is_starred);
    }
}
