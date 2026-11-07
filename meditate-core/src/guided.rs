//! Which guided file Setup shows after the library changed.

/// What Setup does with its guided file after a rename, a delete, an
/// Undo or a sync.
#[derive(Debug, PartialEq, Eq)]
pub enum Follow {
    /// Nothing to do, or the library can't be read right now.
    Keep,
    /// The selected file is still there: re-read its name and length.
    Refresh,
    /// The selected file is gone: clear the selection and remember it.
    Clear,
    /// The remembered file is back (Undo): select it again.
    Reselect(String),
}

/// `selected`: the library file selected, if any. `other_pick`: an
/// Open File or Local Folder file is the current pick. `cleared`: the
/// file a delete cleared. `exists` looks a file up; `None` when the
/// library can't be read, which never clears anything.
pub fn follow_selection(
    selected: Option<&str>,
    other_pick: bool,
    cleared: Option<&str>,
    exists: impl Fn(&str) -> Option<bool>,
) -> Follow {
    match selected {
        Some(uuid) => match exists(uuid) {
            Some(true) => Follow::Refresh,
            Some(false) => Follow::Clear,
            None => Follow::Keep,
        },
        None if other_pick => Follow::Keep,
        None => match cleared {
            Some(uuid) if exists(uuid) == Some(true) => Follow::Reselect(uuid.to_string()),
            _ => Follow::Keep,
        },
    }
}

/// The Guided file chosen last, per device (`sync_state`, never
/// synced: a local file only exists here). Holds the Starred Files key.
const REMEMBERED_KEY: &str = "guided_chosen";

/// Remember the chosen file, or forget it (`None`: nothing chosen or a
/// one-off Open File pick).
pub fn remember_guided(db: &crate::db::Database, chosen: Option<&crate::local_folder::StarredGuidedId>) -> crate::db::Result<()> {
    let key = chosen.map(|c| c.key()).unwrap_or_default();
    // Both apps call this on every refresh of the shown file; only a change writes.
    if db.get_sync_state(REMEMBERED_KEY, "")? == key {
        return Ok(());
    }
    db.set_sync_state(REMEMBERED_KEY, &key)
}

/// The remembered file to choose at start: a library file that still
/// exists, or a local file found in the current folder with its length
/// read. Anything else chooses nothing.
pub fn remembered_guided_from_db(db: &crate::db::Database) -> Option<crate::local_folder::StarredGuidedId> {
    use crate::local_folder::StarredGuidedId;
    let key = db.get_sync_state(REMEMBERED_KEY, "").ok().filter(|k| !k.is_empty())?;
    let id = StarredGuidedId::from_key(&key);
    let usable = match &id {
        StarredGuidedId::Library(uuid) => crate::db::find_guided_file_by_uuid_from_db(db, uuid).ok()?.is_some(),
        StarredGuidedId::Local(local) => {
            let file = crate::db::find_local_file_from_db(db, *local).ok()??;
            let root = crate::db::local_folder_from_db(db).ok()??;
            !file.missing && file.root == root && file.duration_secs.is_some()
        }
    };
    usable.then_some(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{Database, FolderScan, ScannedFile};
    use crate::local_folder::StarredGuidedId;

    fn with_local(db: &Database, files: &[&str]) -> Vec<i64> {
        db.set_local_folder("/music").unwrap();
        let scan: Vec<ScannedFile> = files.iter().map(|f| ScannedFile { rel_path: f.to_string(), size: 1, mtime: 1 }).collect();
        db.apply_local_scan("/music", &FolderScan::Found(scan)).unwrap();
        let all = crate::db::list_local_files_from_db(db, "/music").unwrap();
        files.iter().map(|f| all.iter().find(|x| x.rel_path == *f).unwrap().id).collect()
    }

    #[test]
    fn the_chosen_file_comes_back_after_a_restart() {
        let db = Database::open_in_memory().unwrap();
        db.insert_guided_file_with_uuid("gf-1", "Body Scan", "guided/gf-1.ogg", 1200, true).unwrap();
        db.insert_guided_file_with_uuid("gf-2", "Metta", "guided/gf-2.ogg", 900, false).unwrap();
        assert_eq!(remembered_guided_from_db(&db), None);
        remember_guided(&db, Some(&StarredGuidedId::Library("gf-2".into()))).unwrap();
        assert_eq!(remembered_guided_from_db(&db), Some(StarredGuidedId::Library("gf-2".into())));
    }

    #[test]
    fn a_chosen_local_file_comes_back_after_a_restart() {
        let db = Database::open_in_memory().unwrap();
        let ids = with_local(&db, &["a.ogg", "b.ogg"]);
        db.set_local_file_duration(ids[1], 600).unwrap();
        remember_guided(&db, Some(&StarredGuidedId::Local(ids[1]))).unwrap();
        assert_eq!(remembered_guided_from_db(&db), Some(StarredGuidedId::Local(ids[1])));
    }

    #[test]
    fn a_remembered_file_that_is_gone_stays_unchosen() {
        let db = Database::open_in_memory().unwrap();
        db.insert_guided_file_with_uuid("gf-1", "Body Scan", "guided/gf-1.ogg", 1200, true).unwrap();
        remember_guided(&db, Some(&StarredGuidedId::Library("gf-1".into()))).unwrap();
        db.delete_guided_file("gf-1").unwrap();
        assert_eq!(remembered_guided_from_db(&db), None);
        remember_guided(&db, Some(&StarredGuidedId::Local(4711))).unwrap();
        assert_eq!(remembered_guided_from_db(&db), None, "a local row that never existed");
    }

    #[test]
    fn a_remembered_local_file_that_is_not_found_stays_unchosen() {
        let db = Database::open_in_memory().unwrap();
        let ids = with_local(&db, &["a.ogg", "b.ogg"]);
        db.set_local_file_duration(ids[0], 600).unwrap();
        remember_guided(&db, Some(&StarredGuidedId::Local(ids[0]))).unwrap();
        // The next scan no longer finds a.ogg.
        db.apply_local_scan("/music", &FolderScan::Found(vec![ScannedFile { rel_path: "b.ogg".into(), size: 1, mtime: 1 }])).unwrap();
        assert_eq!(remembered_guided_from_db(&db), None, "missing");
        let db = Database::open_in_memory().unwrap();
        let ids = with_local(&db, &["a.ogg"]);
        db.set_local_file_duration(ids[0], 600).unwrap();
        remember_guided(&db, Some(&StarredGuidedId::Local(ids[0]))).unwrap();
        db.set_local_folder("/other").unwrap();
        assert_eq!(remembered_guided_from_db(&db), None, "another folder");
    }

    #[test]
    fn a_remembered_local_file_without_a_length_stays_unchosen() {
        let db = Database::open_in_memory().unwrap();
        let ids = with_local(&db, &["a.ogg"]);
        remember_guided(&db, Some(&StarredGuidedId::Local(ids[0]))).unwrap();
        assert_eq!(remembered_guided_from_db(&db), None);
    }

    #[test]
    fn a_one_off_pick_forgets_the_chosen_file() {
        let db = Database::open_in_memory().unwrap();
        db.insert_guided_file_with_uuid("gf-1", "Body Scan", "guided/gf-1.ogg", 1200, true).unwrap();
        remember_guided(&db, Some(&StarredGuidedId::Library("gf-1".into()))).unwrap();
        remember_guided(&db, None).unwrap();
        assert_eq!(remembered_guided_from_db(&db), None);
    }

    #[test]
    fn the_chosen_file_stays_on_its_device() {
        let db = Database::open_in_memory().unwrap();
        db.insert_guided_file_with_uuid("gf-1", "Body Scan", "guided/gf-1.ogg", 1200, true).unwrap();
        let before = db.pending_events().unwrap().len();
        remember_guided(&db, Some(&StarredGuidedId::Library("gf-1".into()))).unwrap();
        assert_eq!(remembered_guided_from_db(&db), Some(StarredGuidedId::Library("gf-1".into())));
        assert_eq!(db.pending_events().unwrap().len(), before, "nothing to sync");
    }

    #[test]
    fn the_selected_file_follows_renames_and_deletes() {
        assert_eq!(follow_selection(Some("a"), false, None, |_| Some(true)), Follow::Refresh);
        assert_eq!(follow_selection(Some("a"), false, None, |_| Some(false)), Follow::Clear);
        // A busy or missing database is not a delete.
        assert_eq!(follow_selection(Some("a"), false, None, |_| None), Follow::Keep);
    }

    #[test]
    fn undo_of_a_delete_selects_the_file_again() {
        assert_eq!(follow_selection(None, false, Some("a"), |_| Some(true)), Follow::Reselect("a".into()));
        // Still deleted, or the user picked something else meanwhile.
        assert_eq!(follow_selection(None, false, Some("a"), |_| Some(false)), Follow::Keep);
        assert_eq!(follow_selection(None, true, Some("a"), |_| Some(true)), Follow::Keep);
        assert_eq!(follow_selection(None, false, None, |_| Some(true)), Follow::Keep);
    }
}
