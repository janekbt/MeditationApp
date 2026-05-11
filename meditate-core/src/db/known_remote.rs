//! Per-device trackers for the WebDAV sync layer:
//! - `known_remote_files` records ingested-or-pushed batch_uuids so
//!   the puller can skip GET on files it already replayed and the
//!   pusher doesn't re-fetch its own uploads.
//! - `known_remote_sounds` does the same job at per-bell-sound
//!   granularity for the custom-audio sync.

use rusqlite::params;

use super::{Database, Result};

impl Database {
    /// Return every remote file_uuid that this device has already
    /// ingested or pushed. The puller queries this BEFORE issuing a GET
    /// on each remote file, so it can skip files it already pulled.
    /// The pusher inserts its own batch_uuid into this table on
    /// successful PUT.
    pub fn known_remote_file_uuids(&self) -> Result<std::collections::HashSet<String>> {
        let mut stmt = self.conn.prepare("SELECT file_uuid FROM known_remote_files")?;
        let ids = stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<std::collections::HashSet<_>>>()?;
        Ok(ids)
    }

    /// Record a single batch_uuid as ingested. Idempotent (uses
    /// INSERT OR IGNORE) so callers don't have to check membership
    /// first.
    pub fn record_known_remote_file(&self, file_uuid: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO known_remote_files (file_uuid) VALUES (?1)",
            params![file_uuid],
        )?;
        Ok(())
    }

    /// Clear every recorded remote file_uuid. Two callers:
    /// - Account swap: when the user changes URL or username, the
    ///   previously-known remote files belong to a different store
    ///   entirely; clearing prevents a phantom "remote data lost"
    ///   trigger against the new account.
    /// - Push-local-after-wipe: after the user resolves a "remote data
    ///   lost" prompt by re-uploading, we wipe + re-anchor against
    ///   the now-empty remote.
    pub fn wipe_known_remote_files(&self) -> Result<()> {
        self.conn.execute("DELETE FROM known_remote_files", [])?;
        Ok(())
    }

    /// Per-bell-sound version of `known_remote_file_uuids` for the
    /// B.6 audio-file sync layer. Returns every bell uuid this device
    /// has either pushed or pulled; the orchestrator's push side
    /// skips files in this set and the pull side dittos.
    pub fn known_remote_sound_uuids(&self) -> Result<std::collections::HashSet<String>> {
        let mut stmt = self.conn.prepare("SELECT bell_uuid FROM known_remote_sounds")?;
        let ids = stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<std::collections::HashSet<_>>>()?;
        Ok(ids)
    }

    /// INSERT-OR-IGNORE on the known-sound tracker. Idempotent so a
    /// retry after a half-completed PUT can re-call without fuss.
    pub fn record_known_remote_sound(&self, bell_uuid: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO known_remote_sounds (bell_uuid) VALUES (?1)",
            params![bell_uuid],
        )?;
        Ok(())
    }

    /// Clear the known-sound tracker. Same callers as
    /// wipe_known_remote_files: account swap, push-after-wipe.
    pub fn wipe_known_remote_sounds(&self) -> Result<()> {
        self.conn.execute("DELETE FROM known_remote_sounds", [])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_helpers::*;

    #[test]
    fn known_remote_file_uuids_starts_empty() {
        let db = Database::open_in_memory().unwrap();
        assert!(db.known_remote_file_uuids().unwrap().is_empty());
    }

    #[test]
    fn record_known_remote_file_then_known_remote_file_uuids_returns_it() {
        let db = Database::open_in_memory().unwrap();
        db.record_known_remote_file("aaa-batch-uuid").unwrap();
        let known = db.known_remote_file_uuids().unwrap();
        assert!(known.contains("aaa-batch-uuid"));
    }

    #[test]
    fn record_known_remote_file_is_idempotent() {
        let db = Database::open_in_memory().unwrap();
        db.record_known_remote_file("xyz").unwrap();
        db.record_known_remote_file("xyz").unwrap();
        assert_eq!(db.known_remote_file_uuids().unwrap().len(), 1);
    }

    #[test]
    fn known_remote_files_persist_across_database_reopens() {
        // The dedup tracker MUST survive process restart — otherwise a
        // user who closes the app between sync attempts re-GETs every
        // remote file on the next pull, defeating the optimisation.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.db");
        {
            let db = Database::open(&path).unwrap();
            db.record_known_remote_file("persistent-batch").unwrap();
        }
        let db2 = Database::open(&path).unwrap();
        assert!(db2.known_remote_file_uuids().unwrap().contains("persistent-batch"));
    }

    #[test]
    fn wipe_known_remote_files_clears_every_recorded_uuid() {
        let db = Database::open_in_memory().unwrap();
        db.record_known_remote_file("a").unwrap();
        db.record_known_remote_file("b").unwrap();
        db.record_known_remote_file("c").unwrap();
        assert_eq!(db.known_remote_file_uuids().unwrap().len(), 3);
        db.wipe_known_remote_files().unwrap();
        assert!(db.known_remote_file_uuids().unwrap().is_empty());
    }

    #[test]
    fn wipe_known_remote_files_on_an_empty_table_is_a_silent_no_op() {
        let db = Database::open_in_memory().unwrap();
        db.wipe_known_remote_files().unwrap();
        assert!(db.known_remote_file_uuids().unwrap().is_empty());
    }

    #[test]
    fn wipe_known_remote_files_does_not_touch_other_tables() {
        // Defensive: the wipe is scoped to the dedup tracker. Sessions,
        // labels, events, and settings must all survive untouched —
        // otherwise an account swap would silently destroy local state.
        let db = Database::open_in_memory().unwrap();
        let _ = db.append_event(&sample_event(1)).unwrap();
        let label_id = db.insert_label("focus").unwrap();
        db.record_known_remote_file("a").unwrap();
        db.set_setting("k", "v").unwrap();
        let labels_before = db.list_labels().unwrap().len();
        let events_before = db.pending_events().unwrap().len();

        db.wipe_known_remote_files().unwrap();

        assert_eq!(db.list_labels().unwrap().len(), labels_before);
        assert!(db.list_labels().unwrap().iter().any(|l| l.id == label_id));
        assert_eq!(db.pending_events().unwrap().len(), events_before);
        assert_eq!(db.get_setting("k", "default").unwrap(), "v");
    }

    #[test]
    fn known_remote_sound_uuids_starts_empty() {
        let db = Database::open_in_memory().unwrap();
        assert!(db.known_remote_sound_uuids().unwrap().is_empty());
    }

    #[test]
    fn record_known_remote_sound_adds_to_membership_set() {
        let db = Database::open_in_memory().unwrap();
        db.record_known_remote_sound("bs-1").unwrap();
        db.record_known_remote_sound("bs-2").unwrap();
        let known = db.known_remote_sound_uuids().unwrap();
        assert_eq!(known.len(), 2);
        assert!(known.contains("bs-1"));
        assert!(known.contains("bs-2"));
    }

    #[test]
    fn record_known_remote_sound_is_idempotent_on_repeat() {
        let db = Database::open_in_memory().unwrap();
        db.record_known_remote_sound("bs-1").unwrap();
        db.record_known_remote_sound("bs-1").unwrap();
        assert_eq!(db.known_remote_sound_uuids().unwrap().len(), 1);
    }

    #[test]
    fn wipe_known_remote_sounds_clears_the_set() {
        let db = Database::open_in_memory().unwrap();
        db.record_known_remote_sound("bs-1").unwrap();
        db.record_known_remote_sound("bs-2").unwrap();
        db.wipe_known_remote_sounds().unwrap();
        assert!(db.known_remote_sound_uuids().unwrap().is_empty());
    }
}
