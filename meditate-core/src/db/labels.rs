//! `labels` table — user-managed tag library. Every CRUD op emits a
//! sync event so the library round-trips across devices.

use rusqlite::{params, OptionalExtension};

use super::events::EventKind;
use super::{
    conflict_suffixed_name, is_unique_constraint_error, Database, DbError, Result,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Label {
    pub id: i64,
    pub name: String,
    /// Stable cross-device identity, assigned by the DB at insert time.
    /// Same semantics as `Session::uuid` — populated on read.
    pub uuid: String,
}

impl Database {
    /// True iff some label OTHER THAN `except_id` already uses `name`
    /// (case-insensitive — the column is COLLATE NOCASE). UI-side
    /// pre-validation for renames: pass the row's own id as
    /// `except_id` so renaming-to-self isn't reported as a collision.
    /// Pass any non-existent id (e.g. 0) when validating a brand-new
    /// label.
    pub fn is_label_name_taken(&self, name: &str, except_id: i64) -> Result<bool> {
        Ok(self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM labels WHERE name = ?1 AND id != ?2)",
            params![name, except_id],
            |row| row.get(0),
        )?)
    }

    /// How many sessions reference the label with `id`. Returns 0 for
    /// unreferenced or non-existent labels (no error). Used by the UI's
    /// "delete N sessions?" confirmation before unlabel-on-delete.
    pub fn label_session_count(&self, id: i64) -> Result<i64> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM sessions WHERE label_id = ?1",
            params![id],
            |row| row.get(0),
        )?)
    }

    /// Remove the label with `id`. Sessions that referenced it survive
    /// with `label_id = None` (FK is `ON DELETE SET NULL`). Unknown ids
    /// are silently no-ops AND emit no event — peers would otherwise
    /// receive a tombstone for a label they never knew existed.
    pub fn delete_label(&self, id: i64) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        let row_uuid: Option<String> = self.conn.query_row(
            "SELECT uuid FROM labels WHERE id = ?1",
            params![id],
            |row| row.get::<_, String>(0),
        ).optional()?;
        let Some(uuid) = row_uuid else { return Ok(()); };
        self.conn.execute("DELETE FROM labels WHERE id = ?1", params![id])?;
        let payload = serde_json::json!({ "uuid": uuid }).to_string();
        self.emit_event(EventKind::LabelDelete, &uuid, payload)?;
        tx.commit()?;
        Ok(())
    }

    /// Rename the label with `id` to `name`. Unknown ids are silently
    /// no-ops AND emit no event. If `name` collides case-insensitively
    /// with another label, returns `DbError::DuplicateLabel` and the
    /// transaction rolls back (so no rename event leaks to peers).
    /// Renaming a row to its own current name (incl. a case variant
    /// of itself) succeeds, since SQLite's UNIQUE check excludes the
    /// row being updated.
    pub fn update_label(&self, id: i64, name: &str) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        let row_uuid: Option<String> = self.conn.query_row(
            "SELECT uuid FROM labels WHERE id = ?1",
            params![id],
            |row| row.get::<_, String>(0),
        ).optional()?;
        let Some(label_uuid) = row_uuid else { return Ok(()); };
        match self.conn.execute(
            "UPDATE labels SET name = ?1 WHERE id = ?2",
            params![name, id],
        ) {
            Ok(_) => {
                let payload = serde_json::json!({
                    "uuid": label_uuid,
                    "name": name,
                }).to_string();
                self.emit_event(EventKind::LabelRename, &label_uuid, payload)?;
                tx.commit()?;
                Ok(())
            }
            Err(rusqlite::Error::SqliteFailure(err, _))
                if err.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE =>
            {
                Err(DbError::DuplicateLabel(name.to_string()))
            }
            Err(e) => Err(DbError::Sqlite(e)),
        }
    }

    /// Insert a new label and return its AUTOINCREMENT rowid. Returns
    /// `DbError::DuplicateLabel` if `name` (case-insensitive) already
    /// exists — the column is `COLLATE NOCASE UNIQUE`. UIs that want to
    /// silently reuse an existing row (e.g. CSV import) should call
    /// `find_or_create_label` instead.
    pub fn insert_label(&self, name: &str) -> Result<i64> {
        let label_uuid = uuid::Uuid::new_v4().to_string();
        self.insert_label_with_uuid(&label_uuid, name)
    }

    /// Insert a label with a caller-supplied uuid. Idempotent on the
    /// uuid: if a row with that uuid already exists (regardless of
    /// its current name), returns its rowid without inserting or
    /// emitting. A duplicate *name* with a different uuid still
    /// surfaces `DuplicateLabel` so unrelated callers don't silently
    /// shadow each other's rows.
    pub fn insert_label_with_uuid(&self, uuid_str: &str, name: &str) -> Result<i64> {
        let tx = self.conn.unchecked_transaction()?;
        if let Some(existing) = self.conn.query_row(
            "SELECT id FROM labels WHERE uuid = ?1",
            params![uuid_str],
            |row| row.get::<_, i64>(0),
        ).optional()? {
            return Ok(existing);
        }
        match self.conn.execute(
            "INSERT INTO labels (name, uuid) VALUES (?1, ?2)",
            params![name, uuid_str],
        ) {
            Ok(_) => {
                let rowid = self.conn.last_insert_rowid();
                let payload = serde_json::json!({
                    "uuid": uuid_str,
                    "name": name,
                }).to_string();
                self.emit_event(EventKind::LabelInsert, uuid_str, payload)?;
                tx.commit()?;
                Ok(rowid)
            }
            Err(rusqlite::Error::SqliteFailure(err, _))
                if err.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE =>
            {
                Err(DbError::DuplicateLabel(name.to_string()))
            }
            Err(e) => Err(DbError::Sqlite(e)),
        }
    }

    pub fn count_labels(&self) -> Result<i64> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM labels", [], |row| row.get(0))?)
    }

    /// Every label as a `Label { id, name, uuid }`, alphabetic by name
    /// with the column's NOCASE collation so 'apple', 'Banana', 'cherry'
    /// come back in dictionary order regardless of casing.
    pub fn list_labels(&self) -> Result<Vec<Label>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, uuid FROM labels ORDER BY name",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok(Label { id: row.get(0)?, name: row.get(1)?, uuid: row.get(2)? })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Return a label id by name, creating the label if missing. Lookup
    /// is case-insensitive (column COLLATE NOCASE), so an import of
    /// "Meditation" finds an existing "meditation" instead of producing
    /// a duplicate row.
    pub fn find_or_create_label(&self, name: &str) -> Result<i64> {
        if let Some(id) = self.find_label_by_name(name)? {
            return Ok(id);
        }
        self.insert_label(name)
    }

    pub fn find_label_by_name(&self, name: &str) -> Result<Option<i64>> {
        let id = self
            .conn
            .query_row(
                "SELECT id FROM labels WHERE name = ?1",
                [name],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;
        Ok(id)
    }

    /// Recompute the `labels` row for `label_uuid` from the events table.
    /// Same precedence rules as sessions: tombstone wins on tie/precedence,
    /// else the highest-(lamport, device_id) mutate event drives the name.
    pub(super) fn recompute_label(&self, label_uuid: &str) -> Result<()> {
        let delete_ts: Option<i64> = self.conn.query_row(
            "SELECT MAX(lamport_ts) FROM events
             WHERE target_id = ?1 AND kind = 'label_delete'",
            params![label_uuid],
            |row| row.get::<_, Option<i64>>(0),
        )?;
        let mutate: Option<(i64, String)> = self.conn.query_row(
            "SELECT lamport_ts, payload FROM events
             WHERE target_id = ?1
               AND kind IN ('label_insert', 'label_rename')
             ORDER BY lamport_ts DESC, device_id DESC
             LIMIT 1",
            params![label_uuid],
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
                    format!("label event payload not valid JSON: {e}")))?;
            let name = v["name"].as_str().unwrap_or_default();
            // UPSERT keyed on uuid. The `name` column is UNIQUE
            // COLLATE NOCASE — two peers offline can both pick the
            // same label name independently. On collision, retry
            // with a uuid-suffixed name so both rows materialise and
            // sync keeps moving; without this retry the entire
            // replay_events transaction rolled back and sync got
            // hard-stuck on the poison event forever.
            let upsert = self.conn.execute(
                "INSERT INTO labels (uuid, name) VALUES (?1, ?2)
                 ON CONFLICT(uuid) DO UPDATE SET name = excluded.name",
                params![label_uuid, name],
            );
            match upsert {
                Ok(_) => {}
                Err(e) if is_unique_constraint_error(&e) => {
                    let suffixed = conflict_suffixed_name(name, label_uuid);
                    crate::diag::log(&format!(
                        "label_name_collision: uuid={label_uuid} \
                         original={name:?} resolved={suffixed:?}"
                    ));
                    self.conn.execute(
                        "INSERT INTO labels (uuid, name) VALUES (?1, ?2)
                         ON CONFLICT(uuid) DO UPDATE SET name = excluded.name",
                        params![label_uuid, suffixed],
                    )?;
                }
                Err(e) => return Err(DbError::Sqlite(e)),
            }

            // Re-link any sessions whose LATEST mutate event references
            // this label. Without this, an out-of-order delivery
            // (session_insert arrives in batch 1, label_insert in
            // batch 2) leaves the session permanently orphaned —
            // recompute_session ran before the label existed and set
            // label_id = None, but nothing recomputes the session when
            // the label finally arrives. Only "latest mutate references
            // this label" matches: a session that was inserted with L1
            // and later updated to L2 must NOT be re-stolen by L1.
            self.conn.execute(
                "UPDATE sessions
                 SET label_id = (SELECT id FROM labels WHERE uuid = ?1)
                 WHERE uuid IN (
                     SELECT e1.target_id FROM events e1
                     WHERE e1.kind IN ('session_insert', 'session_update')
                       AND json_extract(e1.payload, '$.label_uuid') = ?1
                       AND NOT EXISTS (
                           SELECT 1 FROM events e2
                           WHERE e2.target_id = e1.target_id
                             AND e2.kind IN ('session_insert', 'session_update')
                             AND (e2.lamport_ts > e1.lamport_ts
                                  OR (e2.lamport_ts = e1.lamport_ts
                                      AND e2.device_id > e1.device_id))
                       )
                 )",
                params![label_uuid],
            )?;
        } else {
            // Tombstoned. `ON DELETE SET NULL` on the FK clears
            // label_id on any cached sessions that referenced this row.
            self.conn.execute(
                "DELETE FROM labels WHERE uuid = ?1",
                params![label_uuid],
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{test_helpers::*, Event, Session, SessionMode};

    #[test]
    fn inserting_label_increases_count() {
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        assert_eq!(db.count_labels().unwrap(), 1);
    }

    #[test]
    fn inserting_two_distinct_labels_yields_count_of_two() {
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        db.insert_label("Evening").unwrap();
        assert_eq!(db.count_labels().unwrap(), 2);
    }

    #[test]
    fn inserting_duplicate_label_returns_err() {
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        let second = db.insert_label("Morning");
        assert!(second.is_err());
        assert_eq!(db.count_labels().unwrap(), 1);
    }

    #[test]
    fn is_label_name_taken_false_for_empty_db() {
        let db = Database::open_in_memory().unwrap();
        assert!(!db.is_label_name_taken("Morning", 0).unwrap());
    }

    #[test]
    fn is_label_name_taken_true_for_existing_other_label() {
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        let evening = db.insert_label("Evening").unwrap();
        assert!(db.is_label_name_taken("Morning", evening).unwrap());
    }

    #[test]
    fn is_label_name_taken_false_when_only_owner_is_excluded() {
        let db = Database::open_in_memory().unwrap();
        let morning = db.insert_label("Morning").unwrap();
        assert!(!db.is_label_name_taken("Morning", morning).unwrap());
    }

    #[test]
    fn is_label_name_taken_is_case_insensitive() {
        let db = Database::open_in_memory().unwrap();
        let morning = db.insert_label("Morning").unwrap();
        assert!(db.is_label_name_taken("morning", 0).unwrap());
        assert!(db.is_label_name_taken("MORNING", 0).unwrap());
        assert!(!db.is_label_name_taken("morning", morning).unwrap());
    }

    #[test]
    fn label_session_count_zero_for_unreferenced_label() {
        let db = Database::open_in_memory().unwrap();
        let id = db.insert_label("Morning").unwrap();
        assert_eq!(db.label_session_count(id).unwrap(), 0);
    }

    #[test]
    fn label_session_count_counts_referencing_sessions() {
        let db = Database::open_in_memory().unwrap();
        let morning = db.insert_label("Morning").unwrap();
        let evening = db.insert_label("Evening").unwrap();
        for i in 0..3 {
            db.insert_session(&Session {
                start_iso: format!("2026-04-2{i}T10:00:00Z"),
                duration_secs: 600,
                label_id: Some(morning),
                notes: None,
                mode: SessionMode::Timer,
                uuid: String::new(),
                guided_file_uuid: None,
            }).unwrap();
        }
        db.insert_session(&Session {
            start_iso: "2026-04-27T19:00:00Z".to_string(),
            duration_secs: 600,
            label_id: Some(evening),
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        for i in 0..2 {
            db.insert_session(&Session {
                start_iso: format!("2026-04-2{i}T20:00:00Z"),
                duration_secs: 300,
                label_id: None,
                notes: None,
                mode: SessionMode::Timer,
                uuid: String::new(),
                guided_file_uuid: None,
            }).unwrap();
        }
        assert_eq!(db.label_session_count(morning).unwrap(), 3);
        assert_eq!(db.label_session_count(evening).unwrap(), 1);
    }

    #[test]
    fn label_session_count_unknown_id_is_zero() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(db.label_session_count(9999).unwrap(), 0);
    }

    #[test]
    fn delete_label_removes_only_that_row() {
        let db = Database::open_in_memory().unwrap();
        let morning = db.insert_label("Morning").unwrap();
        let evening = db.insert_label("Evening").unwrap();
        db.delete_label(morning).unwrap();
        assert_eq!(db.find_label_by_name("Morning").unwrap(), None);
        assert_eq!(db.find_label_by_name("Evening").unwrap(), Some(evening));
        let names: Vec<String> =
            db.list_labels().unwrap().into_iter().map(|l| l.name).collect();
        assert_eq!(names, vec!["Evening"]);
        assert_eq!(db.count_labels().unwrap(), 1);
    }

    #[test]
    fn delete_label_unknown_id_is_noop() {
        let db = Database::open_in_memory().unwrap();
        let id = db.insert_label("Morning").unwrap();
        db.delete_label(id + 999).unwrap();
        assert_eq!(db.count_labels().unwrap(), 1);
        assert_eq!(db.find_label_by_name("Morning").unwrap(), Some(id));
    }

    #[test]
    fn delete_label_unlinks_sessions_via_set_null() {
        let db = Database::open_in_memory().unwrap();
        let morning = db.insert_label("Morning").unwrap();
        let labeled_id = db.insert_session(&Session {
            start_iso: "2026-04-27T10:00:00Z".to_string(),
            duration_secs: 600,
            label_id: Some(morning),
            notes: Some("first sit".to_string()),
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        let labeled_id2 = db.insert_session(&Session {
            start_iso: "2026-04-27T11:00:00Z".to_string(),
            duration_secs: 1200,
            label_id: Some(morning),
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        let unlabeled_id = db.insert_session(&Session {
            start_iso: "2026-04-27T12:00:00Z".to_string(),
            duration_secs: 300,
            label_id: None,
            notes: None,
            mode: SessionMode::BoxBreath,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();

        db.delete_label(morning).unwrap();

        let rows = db.list_sessions().unwrap();
        assert_eq!(rows.len(), 3);
        let by_id: std::collections::HashMap<i64, &Session> =
            rows.iter().map(|(i, s)| (*i, s)).collect();
        assert_eq!(by_id[&labeled_id].label_id, None);
        assert_eq!(by_id[&labeled_id2].label_id, None);
        assert_eq!(by_id[&unlabeled_id].label_id, None);
        assert_eq!(db.count_labels().unwrap(), 0);
    }

    #[test]
    fn delete_label_does_not_affect_unrelated_sessions() {
        let db = Database::open_in_memory().unwrap();
        let morning = db.insert_label("Morning").unwrap();
        let evening = db.insert_label("Evening").unwrap();
        let evening_id = db.insert_session(&Session {
            start_iso: "2026-04-27T19:00:00Z".to_string(),
            duration_secs: 600,
            label_id: Some(evening),
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        db.delete_label(morning).unwrap();
        let row = &db.list_sessions().unwrap()[0];
        assert_eq!(row.0, evening_id);
        assert_eq!(row.1.label_id, Some(evening));
    }

    #[test]
    fn update_label_renames_row() {
        let db = Database::open_in_memory().unwrap();
        let morning = db.insert_label("Morning").unwrap();
        let evening = db.insert_label("Evening").unwrap();
        db.update_label(morning, "Pre-coffee").unwrap();
        assert_eq!(db.find_label_by_name("Pre-coffee").unwrap(), Some(morning));
        assert_eq!(db.find_label_by_name("Morning").unwrap(), None);
        assert_eq!(db.find_label_by_name("Evening").unwrap(), Some(evening));
        assert_eq!(db.count_labels().unwrap(), 2);
    }

    #[test]
    fn update_label_to_same_name_is_idempotent() {
        let db = Database::open_in_memory().unwrap();
        let id = db.insert_label("Morning").unwrap();
        db.update_label(id, "Morning").unwrap();
        assert_eq!(db.count_labels().unwrap(), 1);
        assert_eq!(db.find_label_by_name("Morning").unwrap(), Some(id));
    }

    #[test]
    fn list_labels_returns_label_per_row_alphabetic_by_name() {
        let db = Database::open_in_memory().unwrap();
        let evening = db.insert_label("Evening").unwrap();
        let morning = db.insert_label("Morning").unwrap();
        let afternoon = db.insert_label("Afternoon").unwrap();
        let rows = db.list_labels().unwrap();
        assert_eq!(rows.len(), 3);
        let uuids: std::collections::HashSet<_> =
            rows.iter().map(|l| l.uuid.clone()).collect();
        assert_eq!(uuids.len(), 3);
        for label in &rows {
            assert!(looks_like_uuid_v4(&label.uuid));
        }
        let by_name: std::collections::HashMap<_, _> =
            rows.iter().map(|l| (l.name.clone(), l.uuid.clone())).collect();
        assert_eq!(rows, vec![
            Label { id: afternoon, name: "Afternoon".to_string(),
                uuid: by_name["Afternoon"].clone() },
            Label { id: evening,   name: "Evening".to_string(),
                uuid: by_name["Evening"].clone() },
            Label { id: morning,   name: "Morning".to_string(),
                uuid: by_name["Morning"].clone() },
        ]);
    }

    #[test]
    fn list_labels_returns_label_per_row_case_insensitive_sort() {
        let db = Database::open_in_memory().unwrap();
        let banana = db.insert_label("Banana").unwrap();
        let cherry = db.insert_label("cherry").unwrap();
        let apple = db.insert_label("apple").unwrap();
        let rows = db.list_labels().unwrap();
        let names: Vec<&str> = rows.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, vec!["apple", "Banana", "cherry"]);
        assert_eq!(rows[0].id, apple);
        assert_eq!(rows[1].id, banana);
        assert_eq!(rows[2].id, cherry);
    }

    #[test]
    fn update_label_to_case_variant_of_own_name_succeeds() {
        let db = Database::open_in_memory().unwrap();
        let id = db.insert_label("morning").unwrap();
        db.update_label(id, "Morning").unwrap();
        assert_eq!(db.find_label_by_name("morning").unwrap(), Some(id));
        assert_eq!(db.find_label_by_name("Morning").unwrap(), Some(id));
        let names: Vec<String> =
            db.list_labels().unwrap().into_iter().map(|l| l.name).collect();
        assert_eq!(names, vec!["Morning"]);
    }

    #[test]
    fn update_label_to_existing_other_name_returns_duplicate_error() {
        let db = Database::open_in_memory().unwrap();
        let morning = db.insert_label("Morning").unwrap();
        let _evening = db.insert_label("Evening").unwrap();
        let result = db.update_label(morning, "Evening");
        assert!(
            matches!(result, Err(DbError::DuplicateLabel(ref n)) if n == "Evening"),
            "expected DuplicateLabel(\"Evening\"), got {result:?}"
        );
        assert_eq!(db.find_label_by_name("Morning").unwrap(), Some(morning));
    }

    #[test]
    fn update_label_to_case_variant_of_other_name_returns_duplicate_error() {
        let db = Database::open_in_memory().unwrap();
        let morning = db.insert_label("Morning").unwrap();
        db.insert_label("Evening").unwrap();
        let result = db.update_label(morning, "evening");
        assert!(
            matches!(result, Err(DbError::DuplicateLabel(ref n)) if n == "evening"),
            "expected DuplicateLabel(\"evening\"), got {result:?}"
        );
    }

    #[test]
    fn update_label_unknown_id_is_noop() {
        let db = Database::open_in_memory().unwrap();
        let id = db.insert_label("Morning").unwrap();
        db.update_label(id + 999, "Phantom").unwrap();
        assert_eq!(db.find_label_by_name("Morning").unwrap(), Some(id));
        assert_eq!(db.find_label_by_name("Phantom").unwrap(), None);
        assert_eq!(db.count_labels().unwrap(), 1);
    }

    #[test]
    fn insert_label_returns_new_rowid() {
        let db = Database::open_in_memory().unwrap();
        let id1 = db.insert_label("Morning").unwrap();
        let id2 = db.insert_label("Evening").unwrap();
        let id3 = db.insert_label("Afternoon").unwrap();
        assert_eq!(id1, 1);
        assert_eq!(id2, 2);
        assert_eq!(id3, 3);
        assert_eq!(db.find_label_by_name("Morning").unwrap(), Some(id1));
        assert_eq!(db.find_label_by_name("Evening").unwrap(), Some(id2));
    }

    #[test]
    fn find_or_create_label_creates_when_missing() {
        let db = Database::open_in_memory().unwrap();
        let id = db.find_or_create_label("Morning").unwrap();
        assert_eq!(db.count_labels().unwrap(), 1);
        assert_eq!(db.find_label_by_name("Morning").unwrap(), Some(id));
    }

    #[test]
    fn find_or_create_label_returns_existing_id() {
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        let existing = db.find_label_by_name("Morning").unwrap().unwrap();
        let got = db.find_or_create_label("Morning").unwrap();
        assert_eq!(got, existing);
        assert_eq!(db.count_labels().unwrap(), 1);
    }

    #[test]
    fn find_or_create_label_is_case_insensitive() {
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        let existing = db.find_label_by_name("Morning").unwrap().unwrap();
        assert_eq!(db.find_or_create_label("morning").unwrap(), existing);
        assert_eq!(db.find_or_create_label("MORNING").unwrap(), existing);
        assert_eq!(db.count_labels().unwrap(), 1);
    }

    #[test]
    fn find_or_create_label_idempotent_across_calls() {
        let db = Database::open_in_memory().unwrap();
        let id1 = db.find_or_create_label("Evening").unwrap();
        let id2 = db.find_or_create_label("Evening").unwrap();
        let id3 = db.find_or_create_label("evening").unwrap();
        assert_eq!(id1, id2);
        assert_eq!(id1, id3);
        assert_eq!(db.count_labels().unwrap(), 1);
    }

    #[test]
    fn label_uniqueness_is_case_insensitive() {
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        let result = db.insert_label("morning");
        assert!(
            matches!(result, Err(DbError::DuplicateLabel(ref name)) if name == "morning")
        );
        assert!(matches!(db.insert_label("MORNING"), Err(DbError::DuplicateLabel(_))));
        assert!(matches!(db.insert_label("MoRnInG"), Err(DbError::DuplicateLabel(_))));
        assert_eq!(db.count_labels().unwrap(), 1);
    }

    #[test]
    fn find_label_by_name_is_case_insensitive() {
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        let canonical_id = db.find_label_by_name("Morning").unwrap();
        assert!(canonical_id.is_some());
        assert_eq!(db.find_label_by_name("morning").unwrap(), canonical_id);
        assert_eq!(db.find_label_by_name("MORNING").unwrap(), canonical_id);
        assert_eq!(db.find_label_by_name("MoRnInG").unwrap(), canonical_id);
    }

    #[test]
    fn duplicate_label_error_identifies_offending_name() {
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        let err = db.insert_label("Morning").unwrap_err();
        assert!(matches!(err, DbError::DuplicateLabel(ref name) if name == "Morning"));
    }

    #[test]
    fn list_labels_returns_inserted_names_alphabetically() {
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        db.insert_label("Afternoon").unwrap();
        db.insert_label("Evening").unwrap();
        let names: Vec<String> =
            db.list_labels().unwrap().into_iter().map(|l| l.name).collect();
        assert_eq!(names, vec!["Afternoon", "Evening", "Morning"]);
    }

    #[test]
    fn find_label_by_name_returns_some_id_when_present() {
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        assert!(db.find_label_by_name("Morning").unwrap().is_some());
    }

    #[test]
    fn find_label_by_name_returns_none_when_absent() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(db.find_label_by_name("Morning").unwrap(), None);
    }

    #[test]
    fn insert_label_appends_a_label_insert_event() {
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        let events = db.pending_events().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].1.kind, "label_insert");
    }

    #[test]
    fn label_insert_event_payload_carries_uuid_and_name() {
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        let row_uuid = db.list_labels().unwrap()[0].uuid.clone();
        let payload = event_payload(&db.pending_events().unwrap()[0].1);
        assert_eq!(payload["uuid"], serde_json::Value::String(row_uuid));
        assert_eq!(payload["name"], "Morning");
    }

    #[test]
    fn duplicate_insert_label_emits_no_event() {
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        drain_events(&db);
        let result = db.insert_label("Morning");
        assert!(result.is_err());
        assert!(db.pending_events().unwrap().is_empty());
    }

    #[test]
    fn update_label_appends_a_label_rename_event() {
        let db = Database::open_in_memory().unwrap();
        let id = db.insert_label("Morning").unwrap();
        let row_uuid = db.list_labels().unwrap()[0].uuid.clone();
        drain_events(&db);
        db.update_label(id, "Sunrise").unwrap();
        let events = db.pending_events().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].1.kind, "label_rename");
        let payload = event_payload(&events[0].1);
        assert_eq!(payload["uuid"], serde_json::Value::String(row_uuid));
        assert_eq!(payload["name"], "Sunrise");
    }

    #[test]
    fn update_label_unknown_id_emits_no_event() {
        let db = Database::open_in_memory().unwrap();
        drain_events(&db);
        db.update_label(9999, "Whatever").unwrap();
        assert!(db.pending_events().unwrap().is_empty());
    }

    #[test]
    fn update_label_to_duplicate_emits_no_event() {
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        let evening_id = db.insert_label("Evening").unwrap();
        drain_events(&db);
        let result = db.update_label(evening_id, "Morning");
        assert!(result.is_err());
        assert!(db.pending_events().unwrap().is_empty());
    }

    #[test]
    fn delete_label_appends_a_label_delete_event() {
        let db = Database::open_in_memory().unwrap();
        let id = db.insert_label("Morning").unwrap();
        let row_uuid = db.list_labels().unwrap()[0].uuid.clone();
        drain_events(&db);
        db.delete_label(id).unwrap();
        let events = db.pending_events().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].1.kind, "label_delete");
        let payload = event_payload(&events[0].1);
        assert_eq!(payload["uuid"], serde_json::Value::String(row_uuid));
    }

    #[test]
    fn delete_label_unknown_id_emits_no_event() {
        let db = Database::open_in_memory().unwrap();
        drain_events(&db);
        db.delete_label(9999).unwrap();
        assert!(db.pending_events().unwrap().is_empty());
    }

    #[test]
    fn apply_event_label_insert_creates_the_label() {
        let db = Database::open_in_memory().unwrap();
        db.apply_event(&synth_label_insert(LABEL_X, 5, DEVICE_A, "Morning")).unwrap();
        let labels = db.list_labels().unwrap();
        assert_eq!(labels.len(), 1);
        assert_eq!(labels[0].name, "Morning");
        assert_eq!(labels[0].uuid, LABEL_X);
    }

    #[test]
    fn apply_event_label_rename_updates_the_name() {
        let db = Database::open_in_memory().unwrap();
        db.apply_event(&synth_label_insert(LABEL_X, 5, DEVICE_A, "Morning")).unwrap();
        db.apply_event(&synth_label_rename(LABEL_X, 10, DEVICE_A, "Sunrise")).unwrap();
        let labels = db.list_labels().unwrap();
        assert_eq!(labels.len(), 1);
        assert_eq!(labels[0].name, "Sunrise");
        assert_eq!(labels[0].uuid, LABEL_X);
    }

    #[test]
    fn apply_event_label_delete_removes_the_label() {
        let db = Database::open_in_memory().unwrap();
        db.apply_event(&synth_label_insert(LABEL_X, 5, DEVICE_A, "Morning")).unwrap();
        db.apply_event(&synth_label_delete(LABEL_X, 10, DEVICE_A)).unwrap();
        assert!(db.list_labels().unwrap().is_empty());
    }

    #[test]
    fn apply_event_label_tombstone_resists_lower_lamport_insert() {
        let db = Database::open_in_memory().unwrap();
        db.apply_event(&synth_label_delete(LABEL_X, 10, DEVICE_A)).unwrap();
        db.apply_event(&synth_label_insert(LABEL_X, 5, DEVICE_A, "Morning")).unwrap();
        assert!(db.list_labels().unwrap().is_empty());
    }

    #[test]
    fn apply_event_label_concurrent_renames_break_ties_on_device_id() {
        let db = Database::open_in_memory().unwrap();
        db.apply_event(&synth_label_insert(LABEL_X, 1, DEVICE_A, "Morning")).unwrap();
        db.apply_event(&synth_label_rename(LABEL_X, 5, DEVICE_A, "From A")).unwrap();
        db.apply_event(&synth_label_rename(LABEL_X, 5, DEVICE_B, "From B")).unwrap();
        let labels = db.list_labels().unwrap();
        assert_eq!(labels[0].name, "From B");
    }

    #[test]
    fn apply_event_label_delete_clears_label_id_on_cached_sessions() {
        let db = Database::open_in_memory().unwrap();
        db.apply_event(&synth_label_insert(LABEL_X, 1, DEVICE_A, "Morning")).unwrap();
        db.apply_event(&synth_session_insert(
            SESSION_X, 2, DEVICE_A,
            "2026-04-30T10:00:00", 600,
            Some(LABEL_X), None, SessionMode::Timer,
        )).unwrap();
        assert!(db.list_sessions().unwrap()[0].1.label_id.is_some());
        db.apply_event(&synth_label_delete(LABEL_X, 10, DEVICE_A)).unwrap();
        assert!(db.list_labels().unwrap().is_empty());
        let s = &db.list_sessions().unwrap()[0].1;
        assert_eq!(s.label_id, None);
    }

    #[test]
    fn apply_event_label_is_idempotent() {
        let db = Database::open_in_memory().unwrap();
        let event = synth_label_insert(LABEL_X, 5, DEVICE_A, "Morning");
        db.apply_event(&event).unwrap();
        db.apply_event(&event).unwrap();
        let labels = db.list_labels().unwrap();
        assert_eq!(labels.len(), 1);
        assert_eq!(labels[0].name, "Morning");
    }

    // ── UUIDs on labels ──────────────────────────────────────────────────

    #[test]
    fn inserted_label_has_a_uuid() {
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        let labels = db.list_labels().unwrap();
        assert_eq!(labels.len(), 1);
        assert!(!labels[0].uuid.is_empty(), "uuid must be populated on read");
    }

    #[test]
    fn two_inserted_labels_get_distinct_uuids() {
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        db.insert_label("Evening").unwrap();
        let labels = db.list_labels().unwrap();
        assert_eq!(labels.len(), 2);
        assert_ne!(labels[0].uuid, labels[1].uuid);
    }

    #[test]
    fn inserted_label_uuid_is_v4_shaped() {
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        let uuid = &db.list_labels().unwrap()[0].uuid;
        assert!(looks_like_uuid_v4(uuid),
            "label uuid `{uuid}` doesn't match v4 shape");
    }
    // ── Name-collision suffix on sync merge ───────────────────────────

    #[test]
    fn label_name_collision_during_apply_renames_loser_with_uuid_suffix() {
        // Two peers offline both name a (different-uuid) label
        // "Morning". On sync merge, the UPSERT-on-uuid for the
        // second label hits UNIQUE-on-name. Without the retry,
        // recompute_label returns Err and replay_events rolls back
        // — sync hard-stuck on the poison event forever.
        // With the retry, the second row materialises under a
        // uuid-suffixed name and sync keeps moving.
        let db = Database::open_in_memory().unwrap();
        let l1 = "550e8400-e29b-41d4-a716-44665544aaaa";
        let l2 = "550e8400-e29b-41d4-a716-44665544bbbb";
        for (uuid, lamport, device) in [
            (l1, 5_i64, "dev-A"),
            (l2, 6_i64, "dev-B"),
        ] {
            db.apply_event(&Event {
                event_uuid: uuid::Uuid::new_v4().to_string(),
                lamport_ts: lamport,
                device_id: device.to_string(),
                kind: "label_insert".to_string(),
                target_id: uuid.to_string(),
                payload: serde_json::json!({
                    "uuid": uuid, "name": "Morning",
                }).to_string(),
            }).unwrap();
        }
        let labels = db.list_labels().unwrap();
        assert_eq!(labels.len(), 2,
            "both label rows must materialise — neither poisons the apply");
        let names: Vec<_> = labels.iter().map(|l| l.name.as_str()).collect();
        // The first-arriving label keeps the bare name; the second
        // gets the uuid-prefix suffix.
        assert!(names.contains(&"Morning"),
            "first label must keep its original name; got {names:?}");
        assert!(
            names.iter().any(|n| n.starts_with("Morning (conflict-")),
            "second label must carry the conflict suffix; got {names:?}"
        );
    }

    #[test]
    fn label_name_collision_is_idempotent_on_replay() {
        // Re-applying the same collision sequence must land on the
        // same suffixed name, not invent a different one each time
        // (the suffix is derived from the uuid prefix, which is
        // event-deterministic).
        let db1 = Database::open_in_memory().unwrap();
        let db2 = Database::open_in_memory().unwrap();
        let l1 = "550e8400-e29b-41d4-a716-44665544aaaa";
        let l2 = "550e8400-e29b-41d4-a716-44665544bbbb";
        let events: Vec<Event> = [(l1, 5_i64, "A"), (l2, 6_i64, "B")]
            .into_iter()
            .map(|(uuid, lamport, device)| Event {
                event_uuid: uuid::Uuid::new_v4().to_string(),
                lamport_ts: lamport,
                device_id: device.to_string(),
                kind: "label_insert".to_string(),
                target_id: uuid.to_string(),
                payload: serde_json::json!({
                    "uuid": uuid, "name": "Morning",
                }).to_string(),
            })
            .collect();
        for db in [&db1, &db2] {
            for e in &events { db.apply_event(e).unwrap(); }
        }
        let mut names1: Vec<_> = db1.list_labels().unwrap()
            .into_iter().map(|l| l.name).collect();
        let mut names2: Vec<_> = db2.list_labels().unwrap()
            .into_iter().map(|l| l.name).collect();
        names1.sort(); names2.sort();
        assert_eq!(names1, names2,
            "two devices applying the same events must converge on the same names");
    }

    // ── recompute_label re-links orphaned sessions ────────────────────

    #[test]
    fn session_arriving_before_label_gets_relinked_when_label_arrives() {
        // Out-of-order event delivery: a peer pushes label_insert and
        // session_insert in separate batches. Pull processes the
        // session batch first; recompute_session looks up the label
        // uuid and finds nothing (label_id = None — orphan). Later
        // the label batch arrives; recompute_label must re-link the
        // orphan sessions or they stay label-less forever even after
        // the label exists locally.
        let db = Database::open_in_memory().unwrap();
        let label_uuid = "550e8400-e29b-41d4-a716-446655440001";
        let session_uuid = "550e8400-e29b-41d4-a716-446655440002";

        // Step 1: session_insert arrives, label doesn't exist yet.
        let session_payload = serde_json::json!({
            "uuid": session_uuid,
            "start_iso": "2026-05-01T10:00:00",
            "duration_secs": 600,
            "label_uuid": label_uuid,
            "notes": null,
            "mode": "timer",
            "guided_file_uuid": null,
        }).to_string();
        db.apply_event(&Event {
            event_uuid: uuid::Uuid::new_v4().to_string(),
            lamport_ts: 5,
            device_id: "peer".to_string(),
            kind: "session_insert".to_string(),
            target_id: session_uuid.to_string(),
            payload: session_payload,
        }).unwrap();

        // Session exists but label_id is None — the orphan state.
        let sessions = db.list_sessions().unwrap();
        assert_eq!(sessions.len(), 1);
        assert!(
            sessions[0].1.label_id.is_none(),
            "session must be orphaned before label arrives"
        );

        // Step 2: label_insert arrives. Re-link must happen inside
        // recompute_label so the orphan resolves.
        let label_payload = serde_json::json!({
            "uuid": label_uuid,
            "name": "Focus",
        }).to_string();
        db.apply_event(&Event {
            event_uuid: uuid::Uuid::new_v4().to_string(),
            lamport_ts: 6,
            device_id: "peer".to_string(),
            kind: "label_insert".to_string(),
            target_id: label_uuid.to_string(),
            payload: label_payload,
        }).unwrap();

        let sessions = db.list_sessions().unwrap();
        let s = &sessions[0].1;
        assert!(s.label_id.is_some(), "session must be re-linked after label arrives");
        let labels = db.list_labels().unwrap();
        let focus_id = labels.iter().find(|l| l.name == "Focus").unwrap().id;
        assert_eq!(s.label_id, Some(focus_id));
    }

    #[test]
    fn label_relink_only_targets_sessions_whose_latest_event_references_this_label() {
        // A session that was inserted with label_uuid=L1 and later
        // updated to label_uuid=L2 must stay linked to L2 when L1
        // arrives — the re-link query must check the LATEST event's
        // label_uuid, not just any event in history.
        let db = Database::open_in_memory().unwrap();
        let l1 = "550e8400-e29b-41d4-a716-44665544aaaa";
        let l2 = "550e8400-e29b-41d4-a716-44665544bbbb";
        let s = "550e8400-e29b-41d4-a716-44665544cccc";

        // session_insert references L1 at lamport 5
        db.apply_event(&Event {
            event_uuid: uuid::Uuid::new_v4().to_string(),
            lamport_ts: 5, device_id: "peer".to_string(),
            kind: "session_insert".to_string(),
            target_id: s.to_string(),
            payload: serde_json::json!({
                "uuid": s, "start_iso": "2026-05-01T10:00:00",
                "duration_secs": 600, "label_uuid": l1,
                "notes": null, "mode": "timer", "guided_file_uuid": null,
            }).to_string(),
        }).unwrap();
        // session_update points to L2 at lamport 7 (the latest)
        db.apply_event(&Event {
            event_uuid: uuid::Uuid::new_v4().to_string(),
            lamport_ts: 7, device_id: "peer".to_string(),
            kind: "session_update".to_string(),
            target_id: s.to_string(),
            payload: serde_json::json!({
                "uuid": s, "start_iso": "2026-05-01T10:00:00",
                "duration_secs": 600, "label_uuid": l2,
                "notes": null, "mode": "timer", "guided_file_uuid": null,
            }).to_string(),
        }).unwrap();
        // Now L2 arrives — should link the session.
        db.apply_event(&Event {
            event_uuid: uuid::Uuid::new_v4().to_string(),
            lamport_ts: 8, device_id: "peer".to_string(),
            kind: "label_insert".to_string(),
            target_id: l2.to_string(),
            payload: serde_json::json!({"uuid": l2, "name": "L2"}).to_string(),
        }).unwrap();
        let labels = db.list_labels().unwrap();
        let l2_id = labels.iter().find(|l| l.name == "L2").unwrap().id;
        let session_label = db.list_sessions().unwrap()[0].1.label_id;
        assert_eq!(session_label, Some(l2_id), "linked to L2 (latest)");

        // L1 arrives — must NOT re-steal the session away from L2.
        db.apply_event(&Event {
            event_uuid: uuid::Uuid::new_v4().to_string(),
            lamport_ts: 9, device_id: "peer".to_string(),
            kind: "label_insert".to_string(),
            target_id: l1.to_string(),
            payload: serde_json::json!({"uuid": l1, "name": "L1"}).to_string(),
        }).unwrap();
        let session_label = db.list_sessions().unwrap()[0].1.label_id;
        assert_eq!(session_label, Some(l2_id),
            "session must remain on L2, not re-linked to L1 by the stale event");
    }
}
