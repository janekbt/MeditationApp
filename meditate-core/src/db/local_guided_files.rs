//! `local_guided_files` — guided audio in a folder the user manages
//! outside the app (GitHub #4). Device-local: no event is written and
//! Wipe Local leaves the table alone, because another device has no
//! such files and nothing could pull the rows back. Rows are never
//! deleted, so a star or a play count survives a file or a whole
//! folder going away for a while.

use super::{Database, Result};

/// One audio file under a chosen local folder, keyed by the folder
/// (`root`, as the platform names it) and its `/`-separated path
/// inside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalGuidedFile {
    pub id: i64,
    pub root: String,
    pub rel_path: String,
    pub size: u64,
    pub mtime: i64,
    /// None until the app has read the file's length.
    pub duration_secs: Option<u32>,
    pub is_starred: bool,
    pub play_count: u32,
    /// The last scan of its folder did not find it.
    pub missing: bool,
}

/// `sync_state` key: local-only, and kept by Wipe Local.
const LOCAL_FOLDER_KEY: &str = "local_guided_folder";

const COLUMNS: &str =
    "id, root, rel_path, size, mtime, duration_secs, is_starred, play_count, missing";

fn from_row(row: &rusqlite::Row) -> rusqlite::Result<LocalGuidedFile> {
    Ok(LocalGuidedFile {
        id: row.get(0)?,
        root: row.get(1)?,
        rel_path: row.get(2)?,
        size: row.get::<_, i64>(3)? as u64,
        mtime: row.get(4)?,
        duration_secs: row.get::<_, Option<i64>>(5)?.map(|s| s as u32),
        is_starred: row.get::<_, i64>(6)? != 0,
        play_count: row.get::<_, i64>(7)? as u32,
        missing: row.get::<_, i64>(8)? != 0,
    })
}

/// One file a folder walk found: its `/`-separated path under the
/// folder, its size in bytes and its modification time (seconds).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScannedFile {
    pub rel_path: String,
    pub size: u64,
    pub mtime: i64,
}

/// What a walk of the chosen folder gave.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FolderScan {
    Found(Vec<ScannedFile>),
    /// Moved, access lost, card removed: nothing is known about the files.
    Unreachable,
}

/// Walk `root` and every folder below it. Folder links are not
/// followed, so a link loop can't make the walk endless. Only the
/// root failing to open is an error; an unreadable subfolder is
/// skipped.
pub fn walk_folder(root: &std::path::Path) -> std::io::Result<Vec<ScannedFile>> {
    let mut found = Vec::new();
    let mut pending = vec![(root.to_path_buf(), String::new())];
    let mut first = true;
    while let Some((dir, prefix)) = pending.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(e) if first => return Err(e),
            Err(_) => continue,
        };
        first = false;
        for entry in entries.flatten() {
            let Ok(name) = entry.file_name().into_string() else { continue };
            // file_type() does not follow links, so a link to a folder
            // is never walked into; a link to a file counts as the file.
            let Ok(kind) = entry.file_type() else { continue };
            let rel_path = format!("{prefix}{name}");
            if kind.is_dir() {
                pending.push((entry.path(), format!("{rel_path}/")));
            } else {
                let Ok(meta) = std::fs::metadata(entry.path()) else { continue };
                if !meta.is_file() {
                    continue;
                }
                let mtime = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map_or(0, |d| d.as_secs() as i64);
                found.push(ScannedFile { rel_path, size: meta.len(), mtime });
            }
        }
    }
    Ok(found)
}

/// True for a file name the Guided picker accepts.
fn is_guided_audio(rel_path: &str) -> bool {
    rel_path.rsplit_once('.').is_some_and(|(_, ext)| {
        crate::sound::IMPORTABLE_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str())
    })
}

impl Database {
    /// Bring the rows of `root` in line with a walk of it, in one
    /// transaction. Returns false when the folder was unreachable,
    /// in which case nothing changes.
    pub fn apply_local_scan(&self, root: &str, scan: &FolderScan) -> Result<bool> {
        let FolderScan::Found(files) = scan else { return Ok(false) };
        let tx = self.conn.unchecked_transaction()?;
        self.conn.execute("UPDATE local_guided_files SET missing = 1 WHERE root = ?1", [root])?;
        let mut upsert = self.conn.prepare_cached(
            "INSERT INTO local_guided_files (root, rel_path, size, mtime) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (root, rel_path) DO UPDATE SET
                 missing = 0,
                 duration_secs = CASE WHEN size = excluded.size AND mtime = excluded.mtime
                                      THEN duration_secs END,
                 size = excluded.size,
                 mtime = excluded.mtime",
        )?;
        for f in files.iter().filter(|f| is_guided_audio(&f.rel_path)) {
            upsert.execute(rusqlite::params![root, f.rel_path, f.size as i64, f.mtime])?;
        }
        drop(upsert);
        tx.commit()?;
        Ok(true)
    }

    pub fn set_local_file_duration(&self, id: i64, secs: u32) -> Result<()> {
        self.conn.execute(
            "UPDATE local_guided_files SET duration_secs = ?2 WHERE id = ?1",
            rusqlite::params![id, secs],
        )?;
        Ok(())
    }

    /// Remember the chosen folder on this device only.
    pub fn set_local_folder(&self, root: &str) -> Result<()> {
        self.set_sync_state(LOCAL_FOLDER_KEY, root)
    }

    pub fn set_local_file_starred(&self, id: i64, starred: bool) -> Result<()> {
        self.conn.execute(
            "UPDATE local_guided_files SET is_starred = ?2 WHERE id = ?1",
            rusqlite::params![id, starred as i64],
        )?;
        Ok(())
    }
}

/// The chosen folder, if any.
pub fn local_folder_from_db(db: &Database) -> Result<Option<String>> {
    let root = db.get_sync_state(LOCAL_FOLDER_KEY, "")?;
    Ok((!root.is_empty()).then_some(root))
}

/// One file by its row id, whichever folder it belongs to.
pub fn find_local_file_from_db(db: &Database, id: i64) -> Result<Option<LocalGuidedFile>> {
    use rusqlite::OptionalExtension;
    Ok(db
        .conn
        .query_row(&format!("SELECT {COLUMNS} FROM local_guided_files WHERE id = ?1"), [id], from_row)
        .optional()?)
}

/// Every starred file of every folder ever chosen.
pub fn list_starred_local_files_from_db(db: &Database) -> Result<Vec<LocalGuidedFile>> {
    let mut stmt = db.conn.prepare(&format!(
        "SELECT {COLUMNS} FROM local_guided_files WHERE is_starred = 1 ORDER BY rel_path"
    ))?;
    let rows = stmt.query_map([], from_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Every file ever seen under `root`, missing ones included.
pub fn list_local_files_from_db(db: &Database, root: &str) -> Result<Vec<LocalGuidedFile>> {
    let mut stmt = db.conn.prepare(&format!(
        "SELECT {COLUMNS} FROM local_guided_files WHERE root = ?1 ORDER BY rel_path"
    ))?;
    let rows = stmt.query_map([root], from_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    /// A database as v26.10.1 left it, with one session and one
    /// library file.
    fn release_v26_10_1_database() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(include_str!("fixtures/schema_v26_10_1.sql")).unwrap();
        conn.execute_batch(
            "PRAGMA user_version = 1;
             INSERT INTO sync_state (key, value) VALUES ('cache_schema_version', '2');
             INSERT INTO sessions (start_iso, duration_secs, mode, uuid)
                 VALUES ('2026-10-01T07:00:00', 1200, 'guided', 's1');
             INSERT INTO guided_files (uuid, name, file_path, duration_secs, created_iso, updated_iso)
                 VALUES ('g1', 'Body scan', 'guided/g1.ogg', 1380, '2026-10-01T00:00:00Z', '2026-10-01T00:00:00Z');",
        )
        .unwrap();
        conn
    }

    #[test]
    fn a_database_from_the_last_release_gains_the_local_folder_table() {
        let db = Database::init(release_v26_10_1_database()).unwrap();
        let has_table: i64 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'local_guided_files'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(has_table, 1);
        assert!(list_local_files_from_db(&db, "/music").unwrap().is_empty());
        let sessions: i64 = db.conn.query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0)).unwrap();
        assert_eq!(sessions, 1);
        let files = super::super::list_guided_files_from_db(&db).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].name, "Body scan");
        assert_eq!(files[0].duration_secs, 1380);
        let v: u32 = db.conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(v, 1);
    }

    const ROOT: &str = "/music";

    fn scanned(rel_path: &str, size: u64, mtime: i64) -> ScannedFile {
        ScannedFile { rel_path: rel_path.into(), size, mtime }
    }

    fn found(files: &[ScannedFile]) -> FolderScan {
        FolderScan::Found(files.to_vec())
    }

    fn file(db: &Database, rel_path: &str) -> LocalGuidedFile {
        list_local_files_from_db(db, ROOT)
            .unwrap()
            .into_iter()
            .find(|f| f.rel_path == rel_path)
            .unwrap_or_else(|| panic!("{rel_path} not in the table"))
    }

    #[test]
    fn a_scan_keeps_audio_files_at_every_depth() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.mp3"), b"x").unwrap();
        std::fs::write(dir.path().join("notes.txt"), b"x").unwrap();
        std::fs::create_dir_all(dir.path().join("Sleep/Short")).unwrap();
        std::fs::write(dir.path().join("Sleep/Short/b.OGG"), b"xy").unwrap();
        let db = Database::open_in_memory().unwrap();
        let root = dir.path().to_str().unwrap();
        assert!(db.apply_local_scan(root, &FolderScan::Found(walk_folder(dir.path()).unwrap())).unwrap());
        let paths: Vec<String> =
            list_local_files_from_db(&db, root).unwrap().into_iter().map(|f| f.rel_path).collect();
        assert_eq!(paths, vec!["Sleep/Short/b.OGG".to_string(), "a.mp3".to_string()]);
    }

    #[cfg(unix)]
    #[test]
    fn the_walk_does_not_follow_folder_links() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("Sleep")).unwrap();
        std::fs::write(dir.path().join("Sleep/b.ogg"), b"x").unwrap();
        std::os::unix::fs::symlink(dir.path(), dir.path().join("Sleep/loop")).unwrap();
        let files = walk_folder(dir.path()).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].rel_path, "Sleep/b.ogg");
    }

    #[cfg(unix)]
    #[test]
    fn the_walk_keeps_links_to_files() {
        let dir = tempfile::tempdir().unwrap();
        let talks = tempfile::tempdir().unwrap();
        std::fs::write(talks.path().join("talk.ogg"), b"xyz").unwrap();
        std::os::unix::fs::symlink(talks.path().join("talk.ogg"), dir.path().join("talk.ogg")).unwrap();
        let files = walk_folder(dir.path()).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].rel_path, "talk.ogg");
        assert_eq!(files[0].size, 3);
    }

    #[test]
    fn a_file_is_found_by_its_id_in_any_folder() {
        let db = Database::open_in_memory().unwrap();
        db.apply_local_scan("/a", &found(&[scanned("x.ogg", 1, 1)])).unwrap();
        let id = list_local_files_from_db(&db, "/a").unwrap()[0].id;
        db.set_local_folder("/b").unwrap();
        assert_eq!(find_local_file_from_db(&db, id).unwrap().map(|f| f.root), Some("/a".to_string()));
        assert_eq!(find_local_file_from_db(&db, id + 99).unwrap(), None);
    }

    #[test]
    fn a_walk_of_a_missing_folder_fails() {
        assert!(walk_folder(std::path::Path::new("/no/such/meditate/folder")).is_err());
    }

    #[test]
    fn a_rescan_keeps_the_length_of_an_unchanged_file() {
        let db = Database::open_in_memory().unwrap();
        db.apply_local_scan(ROOT, &found(&[scanned("a.ogg", 10, 100)])).unwrap();
        db.set_local_file_duration(file(&db, "a.ogg").id, 600).unwrap();
        db.apply_local_scan(ROOT, &found(&[scanned("a.ogg", 10, 100)])).unwrap();
        assert_eq!(file(&db, "a.ogg").duration_secs, Some(600));
    }

    #[test]
    fn a_changed_file_needs_its_length_read_again() {
        let db = Database::open_in_memory().unwrap();
        db.apply_local_scan(ROOT, &found(&[scanned("a.ogg", 10, 100), scanned("b.ogg", 10, 100)])).unwrap();
        db.set_local_file_duration(file(&db, "a.ogg").id, 600).unwrap();
        db.set_local_file_duration(file(&db, "b.ogg").id, 700).unwrap();
        // a.ogg replaced by a bigger file, b.ogg touched.
        db.apply_local_scan(ROOT, &found(&[scanned("a.ogg", 11, 100), scanned("b.ogg", 10, 101)])).unwrap();
        assert_eq!(file(&db, "a.ogg").duration_secs, None);
        assert_eq!(file(&db, "b.ogg").duration_secs, None);
    }

    #[test]
    fn a_starred_file_gone_from_the_folder_stays_marked_missing() {
        let db = Database::open_in_memory().unwrap();
        db.apply_local_scan(ROOT, &found(&[scanned("a.ogg", 10, 100), scanned("b.ogg", 10, 100)])).unwrap();
        db.set_local_file_starred(file(&db, "a.ogg").id, true).unwrap();
        db.apply_local_scan(ROOT, &found(&[scanned("b.ogg", 10, 100)])).unwrap();
        let a = file(&db, "a.ogg");
        assert!(a.missing);
        assert!(a.is_starred);
        assert!(!file(&db, "b.ogg").missing);
    }

    #[test]
    fn a_file_back_in_the_folder_keeps_its_star_and_plays() {
        let db = Database::open_in_memory().unwrap();
        db.apply_local_scan(ROOT, &found(&[scanned("a.ogg", 10, 100)])).unwrap();
        let id = file(&db, "a.ogg").id;
        db.set_local_file_starred(id, true).unwrap();
        db.conn.execute("UPDATE local_guided_files SET play_count = 4 WHERE id = ?1", [id]).unwrap();
        db.apply_local_scan(ROOT, &found(&[])).unwrap();
        assert!(file(&db, "a.ogg").missing);
        db.apply_local_scan(ROOT, &found(&[scanned("a.ogg", 10, 100)])).unwrap();
        let a = file(&db, "a.ogg");
        assert!(!a.missing);
        assert!(a.is_starred);
        assert_eq!(a.play_count, 4);
    }

    fn event_count(db: &Database) -> i64 {
        db.conn.query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0)).unwrap()
    }

    fn guided_session() -> crate::db::Session {
        crate::db::Session {
            start_iso: "2026-10-12T07:00:00".into(),
            duration_secs: 1380,
            label_id: None,
            notes: None,
            mode: crate::db::SessionMode::Guided,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
        }
    }

    #[test]
    fn changing_the_folder_keeps_the_old_folders_stars_and_plays() {
        let db = Database::open_in_memory().unwrap();
        db.set_local_folder("/a").unwrap();
        db.apply_local_scan("/a", &found(&[scanned("x.ogg", 1, 1)])).unwrap();
        let x = list_local_files_from_db(&db, "/a").unwrap()[0].id;
        db.set_local_file_starred(x, true).unwrap();
        db.insert_session_counting_local_play(&guided_session(), Some(x)).unwrap();
        db.set_local_folder("/b").unwrap();
        db.apply_local_scan("/b", &found(&[scanned("x.ogg", 1, 1)])).unwrap();
        assert_eq!(local_folder_from_db(&db).unwrap().as_deref(), Some("/b"));
        assert!(!list_local_files_from_db(&db, "/b").unwrap()[0].is_starred);
        db.set_local_folder("/a").unwrap();
        db.apply_local_scan("/a", &found(&[scanned("x.ogg", 1, 1)])).unwrap();
        assert_eq!(local_folder_from_db(&db).unwrap().as_deref(), Some("/a"));
        let back = &list_local_files_from_db(&db, "/a").unwrap()[0];
        assert!(back.is_starred);
        assert_eq!(back.play_count, 1);
    }

    #[test]
    fn local_folder_changes_write_no_sync_event() {
        let db = Database::open_in_memory().unwrap();
        let before = event_count(&db);
        db.set_local_folder(ROOT).unwrap();
        db.apply_local_scan(ROOT, &found(&[scanned("a.ogg", 10, 100)])).unwrap();
        let id = file(&db, "a.ogg").id;
        db.set_local_file_starred(id, true).unwrap();
        db.set_local_file_duration(id, 600).unwrap();
        assert_eq!(event_count(&db), before);
        db.insert_session_counting_local_play(&guided_session(), Some(id)).unwrap();
        assert_eq!(event_count(&db), before + 1, "only the session's own event");
        assert_eq!(local_folder_from_db(&db).unwrap().as_deref(), Some(ROOT));
        let setting: i64 = db
            .conn
            .query_row("SELECT COUNT(*) FROM settings WHERE value = ?1", [ROOT], |r| r.get(0))
            .unwrap();
        assert_eq!(setting, 0, "the folder is not a synced setting");
    }

    #[test]
    fn saving_a_session_counts_one_play_of_its_local_file() {
        let db = Database::open_in_memory().unwrap();
        db.apply_local_scan(ROOT, &found(&[scanned("Sleep/b.ogg", 10, 100)])).unwrap();
        let id = file(&db, "Sleep/b.ogg").id;
        db.conn.execute("UPDATE local_guided_files SET play_count = 2 WHERE id = ?1", [id]).unwrap();
        db.insert_session_counting_local_play(&guided_session(), Some(id)).unwrap();
        assert_eq!(file(&db, "Sleep/b.ogg").play_count, 3);
        assert_eq!(crate::db::list_sessions_from_db(&db).unwrap().len(), 1);
    }

    #[test]
    fn a_session_that_fails_to_save_counts_no_play() {
        let db = Database::open_in_memory().unwrap();
        db.apply_local_scan(ROOT, &found(&[scanned("b.ogg", 10, 100)])).unwrap();
        let id = file(&db, "b.ogg").id;
        db.conn
            .execute_batch(
                "CREATE TRIGGER no_sessions BEFORE INSERT ON sessions BEGIN SELECT RAISE(ABORT, 'full'); END;",
            )
            .unwrap();
        assert!(db.insert_session_counting_local_play(&guided_session(), Some(id)).is_err());
        assert_eq!(file(&db, "b.ogg").play_count, 0);
    }

    #[test]
    fn a_failed_scan_changes_no_file() {
        let db = Database::open_in_memory().unwrap();
        db.apply_local_scan(ROOT, &found(&[scanned("a.ogg", 10, 100)])).unwrap();
        assert!(!db.apply_local_scan(ROOT, &FolderScan::Unreachable).unwrap());
        let a = file(&db, "a.ogg");
        assert!(!a.missing);
    }
}
