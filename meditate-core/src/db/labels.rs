//! `labels` table — user-managed tag library. Every CRUD op emits a
//! sync event so the library round-trips across devices.

use rusqlite::{params, OptionalExtension};

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
        self.emit_event("label_delete", &uuid, payload)?;
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
                self.emit_event("label_rename", &label_uuid, payload)?;
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
                self.emit_event("label_insert", uuid_str, payload)?;
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
