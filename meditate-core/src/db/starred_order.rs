//! The order of Starred Files: library files and local files in one
//! list, sorted by one key space (`order_key.rs`). A library file's key
//! is its synced `created_iso`; a local file's is its device-local
//! `star_key`. Only a change in the library files' own order writes a
//! sync event; local files move with local writes only.

use rusqlite::params;

use super::order_key::key_between;
use super::{Database, GuidedFile, Result};
use crate::local_folder::{starred_guided_from_db, StarredGuided, StarredGuidedId};

impl StarredGuided {
    pub fn id(&self) -> StarredGuidedId {
        match self {
            StarredGuided::Library(g) => StarredGuidedId::Library(g.uuid.0.clone()),
            StarredGuided::Local { file, .. } => StarredGuidedId::Local(file.id),
        }
    }

    /// Its order key; None for a local file from before keys.
    pub(crate) fn key(&self) -> Option<&str> {
        match self {
            StarredGuided::Library(g) => Some(&g.created_iso),
            StarredGuided::Local { file, .. } => file.star_key.as_deref(),
        }
    }

    fn set_key(&mut self, key: String) {
        match self {
            StarredGuided::Library(g) => g.created_iso = key,
            StarredGuided::Local { file, .. } => file.star_key = Some(key),
        }
    }
}

/// A local file's key tail: its row id as 8 hex digits.
pub(super) fn local_tail(id: i64) -> String {
    format!("{id:08x}")
}

impl Database {
    /// Move a Starred Files row to place `to` in that list (clamped to
    /// the end). Unknown rows and a drop on the same place write nothing.
    pub fn move_starred_guided(&self, id: &StarredGuidedId, to: usize) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        let mut rows = starred_guided_from_db(self)?;
        let Some(from) = rows.iter().position(|r| r.id() == *id) else { return Ok(()); };
        let to = to.min(rows.len() - 1);
        if from == to { return Ok(()); }
        self.key_unkeyed_locals(&mut rows)?;
        let library_order = |rows: &[StarredGuided]| -> Vec<StarredGuidedId> {
            rows.iter().filter(|r| matches!(r, StarredGuided::Library(_))).map(StarredGuided::id).collect()
        };
        let before = library_order(&rows);
        let moved = rows.remove(from);
        rows.insert(to, moved);
        // A library file that kept its order among the library files
        // keeps its key; the local rows it passed move instead (below).
        let tail = match &rows[to] {
            StarredGuided::Library(_) if library_order(&rows) == before => None,
            StarredGuided::Library(g) => Some(g.uuid.0.clone()),
            StarredGuided::Local { file, .. } => Some(local_tail(file.id)),
        };
        if let Some(tail) = tail {
            let prev = to.checked_sub(1).and_then(|i| rows[i].key());
            let next = rows.get(to + 1).and_then(StarredGuided::key);
            match key_between(prev, next, &tail) {
                Some(key) => {
                    match &rows[to] {
                        StarredGuided::Library(g) => self.write_library_key(&tx, g, &key)?,
                        StarredGuided::Local { file, .. } => self.write_star_key(file.id, &key)?,
                    }
                    rows[to].set_key(key);
                }
                None => self.retime_starred_library(&tx, &mut rows)?,
            }
        }
        if !self.repair_local_runs(&mut rows)? {
            self.retime_starred_library(&tx, &mut rows)?;
            self.repair_local_runs(&mut rows)?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Move a library file to place `to` among all library files
    /// (Manage Imports), clamped to the end: only the moved file gets
    /// a new key, between its new neighbours there, so Starred Files
    /// follows by key. Unknown uuids and a drop on the same place write
    /// nothing.
    pub fn move_guided_file(&self, uuid_str: &str, to: usize) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        let mut list = super::list_guided_files_from_db(self)?;
        let Some(from) = list.iter().position(|g| g.uuid.0 == uuid_str) else { return Ok(()); };
        let to = to.min(list.len() - 1);
        if from == to { return Ok(()); }
        // The starred order as it reads before the move, for the fallback.
        let rows = starred_guided_from_db(self)?;
        let moved = list.remove(from);
        list.insert(to, moved);
        let prev = to.checked_sub(1).map(|i| list[i].created_iso.as_str());
        let next = list.get(to + 1).map(|g| g.created_iso.as_str());
        match key_between(prev, next, uuid_str) {
            Some(key) => self.write_library_key(&tx, &list[to], &key)?,
            None => {
                // No room: every library file gets a fresh time in the
                // new order, as `move_preset` does, and the local files
                // keep the places they had among the starred rows.
                let mut rows = rows;
                self.retime_library(&tx, &mut list, &mut rows)?;
                self.key_unkeyed_locals(&mut rows)?;
                self.repair_local_runs(&mut rows)?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Starred local rows without a key (from before keys, in their
    /// by-name order at the end) get keys after the highest key.
    pub(super) fn key_unkeyed_locals(&self, rows: &mut [StarredGuided]) -> Result<()> {
        let mut highest: Option<String> = rows.iter().filter_map(|r| r.key().map(str::to_string)).max();
        for row in rows.iter_mut() {
            let StarredGuided::Local { file, .. } = row else { continue };
            if file.star_key.is_some() { continue; }
            let Some(key) = key_between(highest.as_deref(), None, &local_tail(file.id)) else { continue };
            self.write_star_key(file.id, &key)?;
            file.star_key = Some(key.clone());
            highest = Some(key);
        }
        Ok(())
    }

    /// Every run of local rows whose keys don't rise strictly between
    /// the keys of the library rows around it (none at the top or the
    /// bottom) gets new keys in its order. Local writes only. False
    /// when two library rows around a run leave no room.
    fn repair_local_runs(&self, rows: &mut [StarredGuided]) -> Result<bool> {
        let mut start = 0;
        while start < rows.len() {
            if matches!(rows[start], StarredGuided::Library(_)) { start += 1; continue; }
            let end = (start..rows.len()).find(|&i| matches!(rows[i], StarredGuided::Library(_))).unwrap_or(rows.len());
            let lower = start.checked_sub(1).and_then(|i| rows[i].key()).map(str::to_string);
            let upper = rows.get(end).and_then(StarredGuided::key).map(str::to_string);
            let mut last = lower.clone();
            let fits = rows[start..end].iter().all(|r| {
                let ok = r.key().is_some_and(|k| last.as_deref().map_or(true, |l| l < k) && upper.as_deref().map_or(true, |u| k < u));
                last = r.key().map(str::to_string);
                ok
            });
            if !fits {
                let mut last = lower;
                for row in &mut rows[start..end] {
                    let StarredGuided::Local { file, .. } = row else { unreachable!("a run holds local rows") };
                    let Some(key) = key_between(last.as_deref(), upper.as_deref(), &local_tail(file.id)) else { return Ok(false) };
                    self.write_star_key(file.id, &key)?;
                    file.star_key = Some(key.clone());
                    last = Some(key);
                }
            }
            start = end;
        }
        Ok(true)
    }

    /// No room between equal library keys: every library file gets a
    /// fresh time 1 ms apart, the unstarred ones where they stand, the
    /// starred ones in their order in `rows`.
    fn retime_starred_library(&self, tx: &rusqlite::Transaction, rows: &mut [StarredGuided]) -> Result<()> {
        let mut starred = rows.iter().filter_map(|r| match r {
            StarredGuided::Library(g) => Some(g.clone()),
            StarredGuided::Local { .. } => None,
        });
        let mut list: Vec<GuidedFile> = super::list_guided_files_from_db(self)?
            .into_iter()
            .map(|g| if g.is_starred { starred.next().unwrap_or(g) } else { g })
            .collect();
        self.retime_library(tx, &mut list, rows)
    }

    /// Fresh times 1 ms apart for the library files in `list` order;
    /// the starred ones fill the library rows of `rows` in that order.
    fn retime_library(&self, tx: &rusqlite::Transaction, list: &mut [GuidedFile], rows: &mut [StarredGuided]) -> Result<()> {
        let keys = super::order_key::fresh_keys(list.len());
        for (g, key) in list.iter_mut().zip(keys) {
            self.write_library_key(tx, g, &key)?;
            g.created_iso = key;
        }
        let mut starred = list.iter().filter(|g| g.is_starred);
        for row in rows.iter_mut() {
            if let StarredGuided::Library(slot) = row {
                if let Some(g) = starred.next() { *slot = g.clone(); }
            }
        }
        Ok(())
    }

    /// A library file's new place: its `created_iso`, in one synced
    /// update carrying every field, as a rename's.
    fn write_library_key(&self, tx: &rusqlite::Transaction, g: &GuidedFile, key: &str) -> Result<()> {
        self.edit_guided_file_in(tx, g.uuid.as_str(), |f| f.created_iso = key.to_string())
    }

    fn write_star_key(&self, id: i64, key: &str) -> Result<()> {
        self.conn.execute("UPDATE local_guided_files SET star_key = ?2 WHERE id = ?1", params![id, key])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::db::{list_guided_files_from_db, Database, FolderScan, ScannedFile};
    use crate::local_folder::{starred_guided_from_db, StarredGuided, StarredGuidedId};

    fn starred(db: &Database) -> Vec<String> {
        starred_guided_from_db(db).unwrap().iter().map(|r| match r {
            StarredGuided::Library(g) => g.name.clone(),
            StarredGuided::Local { file, .. } => file.rel_path.clone(),
        }).collect()
    }

    #[test]
    fn a_move_with_no_room_keeps_the_manage_imports_order() {
        let db = Database::open_in_memory().unwrap();
        db.insert_guided_file_with_uuid("u", "U", "guided/u.ogg", 60, false).unwrap();
        db.insert_guided_file_with_uuid("a", "A", "guided/a.ogg", 60, true).unwrap();
        db.insert_guided_file_with_uuid("b", "B", "guided/b.ogg", 60, true).unwrap();
        // Two imports in the same instant: no key fits between them.
        db.conn.execute("UPDATE guided_files SET created_iso = '2026-10-02T00:00:00Z' WHERE uuid IN ('a', 'b')", []).unwrap();
        db.conn.execute("UPDATE guided_files SET created_iso = '2026-10-03T00:00:00Z' WHERE uuid = 'u'", []).unwrap();
        db.set_local_folder("/m").unwrap();
        db.apply_local_scan("/m", &FolderScan::Found(vec![ScannedFile { rel_path: "x.ogg".into(), size: 1, mtime: 1 }])).unwrap();
        let x = crate::db::list_local_files_from_db(&db, "/m").unwrap()[0].id;
        db.set_local_file_starred(x, true).unwrap();
        assert_eq!(starred(&db), ["A", "B", "x.ogg"]);
        db.move_starred_guided(&StarredGuidedId::Local(x), 1).unwrap();
        assert_eq!(starred(&db), ["A", "x.ogg", "B"]);
        let all: Vec<String> = list_guided_files_from_db(&db).unwrap().into_iter().map(|g| g.name).collect();
        assert_eq!(all, ["A", "B", "U"], "the unstarred file stays where it was");
    }

    #[test]
    fn a_manage_imports_move_with_no_room_keeps_local_places() {
        let db = Database::open_in_memory().unwrap();
        db.insert_guided_file_with_uuid("a", "A", "guided/a.ogg", 60, true).unwrap();
        db.insert_guided_file_with_uuid("b", "B", "guided/b.ogg", 60, true).unwrap();
        db.insert_guided_file_with_uuid("c", "C", "guided/c.ogg", 60, false).unwrap();
        db.conn.execute("UPDATE guided_files SET created_iso = '2026-10-02T00:00:00Z' WHERE uuid IN ('a', 'b')", []).unwrap();
        db.conn.execute("UPDATE guided_files SET created_iso = '2026-10-03T00:00:00Z' WHERE uuid = 'c'", []).unwrap();
        db.set_local_folder("/m").unwrap();
        db.apply_local_scan("/m", &FolderScan::Found(vec![ScannedFile { rel_path: "x.ogg".into(), size: 1, mtime: 1 }])).unwrap();
        let x = crate::db::list_local_files_from_db(&db, "/m").unwrap()[0].id;
        db.set_local_file_starred(x, true).unwrap();
        db.conn.execute("UPDATE local_guided_files SET star_key = '2026-10-02T12:00:00Z'", []).unwrap();
        assert_eq!(starred(&db), ["A", "B", "x.ogg"]);
        db.move_guided_file("c", 1).unwrap();
        let all: Vec<String> = list_guided_files_from_db(&db).unwrap().into_iter().map(|g| g.name).collect();
        assert_eq!(all, ["A", "C", "B"]);
        assert_eq!(starred(&db), ["A", "B", "x.ogg"]);
    }
}
