//! Per-device trackers for the WebDAV sync layer:
//! - `known_remote_files` records ingested-or-pushed batch_uuids so
//!   the puller can skip GET on files it already replayed and the
//!   pusher doesn't re-fetch its own uploads.
//! - `known_remote_sounds` does the same job at per-bell-sound
//!   granularity for the custom-audio sync.

use rusqlite::params;

use super::{Database, Result};

/// One of the trackers: what this device already pushed or pulled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Known {
    /// Event batch files: the puller skips their GET, the pusher
    /// doesn't fetch its own uploads back.
    Files,
    /// Custom bell-sound audio files.
    Sounds,
    /// Guided-file audio files.
    GuidedFiles,
}

impl Known {
    pub(crate) const ALL: [Known; 3] = [Known::Files, Known::Sounds, Known::GuidedFiles];

    fn table_and_column(self) -> (&'static str, &'static str) {
        match self {
            Known::Files => ("known_remote_files", "file_uuid"),
            Known::Sounds => ("known_remote_sounds", "bell_uuid"),
            Known::GuidedFiles => ("known_remote_guided_files", "guided_uuid"),
        }
    }
}

impl Database {
    /// Every uuid recorded in tracker `known`.
    pub(crate) fn known_remote(&self, known: Known) -> Result<std::collections::HashSet<String>> {
        let (table, column) = known.table_and_column();
        self.query_rows(&format!("SELECT {column} FROM {table}"), [], |row| row.get(0))
    }

    /// Record `uuid` in tracker `known`. Idempotent, so a retry after a
    /// half-completed PUT can call it again.
    pub(crate) fn record_known_remote(&self, known: Known, uuid: &str) -> Result<()> {
        let (table, column) = known.table_and_column();
        self.conn.execute(&format!("INSERT OR IGNORE INTO {table} ({column}) VALUES (?1)"), params![uuid])?;
        Ok(())
    }

    /// Clear tracker `known`: on an account swap (the old uuids belong
    /// to another store) and before pushing local data again after
    /// "remote data lost".
    pub(crate) fn wipe_known_remote(&self, known: Known) -> Result<()> {
        let (table, _) = known.table_and_column();
        self.conn.execute(&format!("DELETE FROM {table}"), [])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_helpers::*;

    #[test]
    fn each_tracker_records_once_lists_and_wipes_only_itself() {
        for known in Known::ALL {
            let db = Database::open_in_memory().unwrap();
            assert!(db.known_remote(known).unwrap().is_empty(), "{known:?} starts empty");
            db.wipe_known_remote(known).unwrap();
            db.record_known_remote(known, "a").unwrap();
            db.record_known_remote(known, "a").unwrap();
            db.record_known_remote(known, "b").unwrap();
            assert_eq!(db.known_remote(known).unwrap(), ["a", "b"].map(String::from).into_iter().collect(), "{known:?}");
            for other in Known::ALL.into_iter().filter(|&o| o != known) {
                assert!(db.known_remote(other).unwrap().is_empty(), "{known:?} leaked into {other:?}");
            }
            db.wipe_known_remote(known).unwrap();
            assert!(db.known_remote(known).unwrap().is_empty(), "{known:?} wiped");
        }
    }

    #[test]
    fn known_remote_trackers_persist_across_database_reopens() {
        // The dedup tracker MUST survive process restart — otherwise a
        // user who closes the app between sync attempts re-GETs every
        // remote file on the next pull, defeating the optimisation.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.db");
        {
            let db = Database::open(&path).unwrap();
            db.record_known_remote(Known::Files, "persistent-batch").unwrap();
        }
        let db2 = Database::open(&path).unwrap();
        assert!(db2.known_remote(Known::Files).unwrap().contains("persistent-batch"));
    }

    #[test]
    fn wiping_a_tracker_does_not_touch_other_tables() {
        // Defensive: the wipe is scoped to the dedup tracker. Sessions,
        // labels, events, and settings must all survive untouched —
        // otherwise an account swap would silently destroy local state.
        let db = Database::open_in_memory().unwrap();
        let _ = db.append_event(&sample_event(1)).unwrap();
        let label_id = db.insert_label("focus").unwrap().id;
        db.record_known_remote(Known::Files, "a").unwrap();
        db.set_setting("k", "v").unwrap();
        let labels_before = crate::db::list_labels_from_db(&db).unwrap().len();
        let events_before = db.pending_events().unwrap().len();

        db.wipe_known_remote(Known::Files).unwrap();

        assert_eq!(crate::db::list_labels_from_db(&db).unwrap().len(), labels_before);
        assert!(crate::db::list_labels_from_db(&db).unwrap().iter().any(|l| l.id == label_id));
        assert_eq!(db.pending_events().unwrap().len(), events_before);
        assert_eq!(db.get_setting("k", "default").unwrap(), "v");
    }
}
