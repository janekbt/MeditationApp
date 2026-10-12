//! What the Guided Local Folder page shows (GitHub #4): the Folders
//! view one level at a time, the List view in the chosen order, the
//! folder line with the reading progress, and Starred Files with the
//! library and the local files together. The apps draw these rows
//! and translate their own text.

use crate::db::{Database, GuidedFile, LocalGuidedFile, Result};

/// One row of the Folders view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FolderRow<'a> {
    /// A subfolder with what is directly inside it.
    Folder { name: String, path: String, folders: u32, files: u32 },
    File(&'a LocalGuidedFile),
}

/// The rows of folder `dir` (`""` is the chosen folder itself, else a
/// `/`-separated path under it): subfolders first, then files, each
/// by name. Missing files are left out.
pub fn folder_view<'a>(files: &'a [LocalGuidedFile], dir: &str) -> Vec<FolderRow<'a>> {
    use std::collections::{BTreeSet, HashMap};
    let prefix = if dir.is_empty() { String::new() } else { format!("{dir}/") };
    // Subfolder name -> (its direct subfolders, its direct file count).
    let mut folders: HashMap<&str, (BTreeSet<&str>, u32)> = HashMap::new();
    let mut here = Vec::new();
    for file in files.iter().filter(|f| !f.missing) {
        let Some(rest) = file.rel_path.strip_prefix(&prefix) else { continue };
        match rest.split_once('/') {
            None => here.push(file),
            Some((sub, below)) => {
                let entry = folders.entry(sub).or_default();
                match below.split_once('/') {
                    None => entry.1 += 1,
                    Some((deeper, _)) => {
                        entry.0.insert(deeper);
                    }
                }
            }
        }
    }
    let mut folders: Vec<_> = folders.into_iter().collect();
    folders.sort_by_key(|(name, _)| name.to_lowercase());
    here.sort_by(|a, b| by_name(a, b));
    folders
        .into_iter()
        .map(|(name, (subs, count))| FolderRow::Folder {
            name: name.to_string(),
            path: format!("{prefix}{name}"),
            folders: subs.len() as u32,
            files: count,
        })
        .chain(here.into_iter().map(FolderRow::File))
        .collect()
}

fn by_name(a: &LocalGuidedFile, b: &LocalGuidedFile) -> std::cmp::Ordering {
    file_name(&a.rel_path)
        .to_lowercase()
        .cmp(&file_name(&b.rel_path).to_lowercase())
        .then_with(|| a.rel_path.cmp(&b.rel_path))
}

/// Page title and the path above it for folder `dir`:
/// `"Retreats/Autumn 2025/Day 3"` gives `("Day 3", Some("Retreats / Autumn 2025"))`.
pub fn folder_title(dir: &str) -> (String, Option<String>) {
    match dir.rsplit_once('/') {
        Some((parent, name)) => (name.to_string(), Some(parent.replace('/', " / "))),
        None => (dir.to_string(), None),
    }
}

/// A file's name without folder and extension.
pub fn file_name(rel_path: &str) -> &str {
    let name = rel_path.rsplit('/').next().unwrap_or(rel_path);
    match name.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => stem,
        _ => name,
    }
}

/// The folder a file is in, shown as `Retreats / Autumn 2025`; empty
/// for a file in the chosen folder itself.
pub fn folder_label(rel_path: &str) -> String {
    rel_path.rsplit_once('/').map(|(dir, _)| dir.replace('/', " / ")).unwrap_or_default()
}

/// The List view's order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListSort {
    Name,
    Length,
    MostPlayed,
    LeastPlayed,
}

/// Every present file once, in `sort` order, ties by name.
pub fn list_view(files: &[LocalGuidedFile], sort: ListSort) -> Vec<&LocalGuidedFile> {
    let mut rows: Vec<&LocalGuidedFile> = files.iter().filter(|f| !f.missing).collect();
    rows.sort_by(|a, b| {
        let first = match sort {
            ListSort::Name => std::cmp::Ordering::Equal,
            // An unread length (None) sorts after every read one.
            ListSort::Length => match (a.duration_secs, b.duration_secs) {
                (Some(x), Some(y)) => x.cmp(&y),
                (x, y) => x.is_none().cmp(&y.is_none()),
            },
            ListSort::MostPlayed => b.play_count.cmp(&a.play_count),
            ListSort::LeastPlayed => a.play_count.cmp(&b.play_count),
        };
        first.then_with(|| by_name(a, b))
    });
    rows
}

/// The folder line's count: the files still in the folder.
pub fn folder_line(files: &[LocalGuidedFile]) -> u32 {
    files.iter().filter(|f| !f.missing).count() as u32
}

/// One refresh of the folder, as the refresh button shows it: the
/// lengths this run has to read and how many are done, and whether
/// the user tapped for it. Ids are kept, so a file queued twice
/// counts once.
#[derive(Debug, Default)]
pub struct RefreshProgress {
    tapped: bool,
    failed: bool,
    queued: std::collections::HashSet<i64>,
    done: std::collections::HashSet<i64>,
}

impl RefreshProgress {
    pub fn tap(&mut self) {
        self.tapped = true;
    }

    /// A file whose length this run will read.
    pub fn queued(&mut self, id: i64) {
        self.queued.insert(id);
    }

    /// A length read, or failed to read.
    pub fn finished(&mut self, id: i64) {
        self.done.insert(id);
    }

    /// A scan or length could not be saved or read; the app said so
    /// already. Counts only for a tapped refresh waiting for its
    /// message, so a failed pick can't silence a later refresh.
    pub fn write_failed(&mut self) {
        self.failed |= self.tapped;
    }

    /// Percent of this run's lengths done; None while there are none
    /// (the folder itself is still being walked).
    pub fn percent(&self) -> Option<u32> {
        let total = self.queued.len();
        let done = self.done.intersection(&self.queued).count();
        (total > 0).then(|| (done * 100 / total) as u32)
    }

    /// Call on every redraw. When nothing is read any more, starts the
    /// next run afresh and returns true once if the user tapped for
    /// this one, the folder could be read and everything was saved.
    pub fn settle(&mut self, busy: bool, unreachable: bool) -> bool {
        if busy {
            return false;
        }
        self.queued.clear();
        self.done.clear();
        let failed = std::mem::take(&mut self.failed);
        std::mem::take(&mut self.tapped) && !unreachable && !failed
    }
}

/// One row of Starred Files on the Guided screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StarredGuided {
    Library(GuidedFile),
    /// `found` is false when the file is missing or belongs to a
    /// folder that is no longer the chosen one.
    Local { file: LocalGuidedFile, found: bool },
}

/// Starred library files in their usual order, then starred local
/// files by name.
pub fn starred_guided_from_db(db: &Database) -> Result<Vec<StarredGuided>> {
    let mut rows: Vec<StarredGuided> = crate::db::list_guided_files_from_db(db)?
        .into_iter()
        .filter(|g| g.is_starred)
        .map(StarredGuided::Library)
        .collect();
    let current = crate::db::local_folder_from_db(db)?;
    let mut local = crate::db::list_starred_local_files_from_db(db)?;
    local.sort_by(by_name);
    rows.extend(local.into_iter().map(|file| {
        let found = !file.missing && current.as_deref() == Some(file.root.as_str());
        StarredGuided::Local { file, found }
    }));
    Ok(rows)
}

/// Synced `settings` keys, like the rest of the UI state.
const LIST_VIEW_KEY: &str = "guided_local_list_view";
const LIST_SORT_KEY: &str = "guided_local_sort";

/// True when the page shows the List view instead of Folders.
pub fn list_view_on_from_db(db: &Database) -> bool {
    crate::settings_keys::read_bool(db, LIST_VIEW_KEY, false)
}

pub fn set_list_view_on(db: &Database, on: bool) -> Result<()> {
    db.set_setting(LIST_VIEW_KEY, crate::settings_keys::format_bool(on))
}

pub fn list_sort_from_db(db: &Database) -> ListSort {
    match crate::settings_keys::read_str(db, LIST_SORT_KEY, "name").as_str() {
        "length" => ListSort::Length,
        "most_played" => ListSort::MostPlayed,
        "least_played" => ListSort::LeastPlayed,
        _ => ListSort::Name,
    }
}

pub fn set_list_sort(db: &Database, sort: ListSort) -> Result<()> {
    let value = match sort {
        ListSort::Name => "name",
        ListSort::Length => "length",
        ListSort::MostPlayed => "most_played",
        ListSort::LeastPlayed => "least_played",
    };
    db.set_setting(LIST_SORT_KEY, value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{FolderScan, ScannedFile};

    fn f(rel_path: &str, secs: Option<u32>, plays: u32) -> LocalGuidedFile {
        LocalGuidedFile {
            id: 0,
            root: "/m".into(),
            rel_path: rel_path.into(),
            size: 1,
            mtime: 1,
            duration_secs: secs,
            is_starred: false,
            play_count: plays,
            missing: false,
        }
    }

    fn names(rows: &[FolderRow]) -> Vec<String> {
        rows.iter()
            .map(|r| match r {
                FolderRow::Folder { name, .. } => name.clone(),
                FolderRow::File(file) => file_name(&file.rel_path).to_string(),
            })
            .collect()
    }

    #[test]
    fn folders_view_lists_subfolders_then_files_by_name() {
        let files = [
            f("b.ogg", Some(60), 0),
            f("Sleep/x.ogg", Some(60), 0),
            f("a.ogg", Some(60), 0),
            f("Morning/y.ogg", Some(60), 0),
            f("gone.ogg", Some(60), 0),
        ];
        let mut files = files.to_vec();
        files[4].missing = true;
        assert_eq!(names(&folder_view(&files, "")), vec!["Morning", "Sleep", "a", "b"]);
    }

    #[test]
    fn a_folder_row_counts_its_own_folders_and_files() {
        let files: Vec<_> = [
            "Sleep/A/1.ogg", "Sleep/A/2.ogg", "Sleep/B/C/3.ogg",
            "Sleep/1.ogg", "Sleep/2.ogg", "Sleep/3.ogg", "Sleep/4.ogg", "Sleep/5.ogg",
        ]
        .iter()
        .map(|p| f(p, None, 0))
        .collect();
        let rows = folder_view(&files, "");
        assert_eq!(
            rows,
            vec![FolderRow::Folder { name: "Sleep".into(), path: "Sleep".into(), folders: 2, files: 5 }]
        );
    }

    #[test]
    fn folders_view_opens_any_depth() {
        let files = [
            f("Retreats/Autumn 2025/Day 3/Evening talks/t.ogg", None, 0),
            f("Retreats/Autumn 2025/Day 3/Body scan, long.ogg", None, 0),
            f("Retreats/Autumn 2025/Day 2/other.ogg", None, 0),
            f("Day 3/elsewhere.ogg", None, 0),
        ];
        let rows = folder_view(&files, "Retreats/Autumn 2025/Day 3");
        assert_eq!(names(&rows), vec!["Evening talks", "Body scan, long"]);
        assert_eq!(
            rows[0],
            FolderRow::Folder {
                name: "Evening talks".into(),
                path: "Retreats/Autumn 2025/Day 3/Evening talks".into(),
                folders: 0,
                files: 1,
            }
        );
        assert_eq!(
            folder_title("Retreats/Autumn 2025/Day 3"),
            ("Day 3".to_string(), Some("Retreats / Autumn 2025".to_string()))
        );
        assert_eq!(folder_title("Sleep"), ("Sleep".to_string(), None));
        assert_eq!(folder_label("Retreats/Autumn 2025/Day 3/x.ogg"), "Retreats / Autumn 2025 / Day 3");
        assert_eq!(folder_label("x.ogg"), "");
    }

    fn list_names(files: &[LocalGuidedFile], sort: ListSort) -> Vec<&str> {
        list_view(files, sort).into_iter().map(|f| file_name(&f.rel_path)).collect()
    }

    #[test]
    fn list_view_sorts_by_each_order_with_ties_by_name() {
        let mut files = vec![
            f("Sleep/c.ogg", Some(300), 0),
            f("b.ogg", Some(100), 3),
            f("Morning/a.ogg", Some(200), 0),
            f("d.ogg", Some(100), 9),
        ];
        files.push(LocalGuidedFile { missing: true, ..f("gone.ogg", Some(1), 0) });
        assert_eq!(list_names(&files, ListSort::Name), vec!["a", "b", "c", "d"]);
        assert_eq!(list_names(&files, ListSort::Length), vec!["b", "d", "a", "c"]);
        assert_eq!(list_names(&files, ListSort::MostPlayed), vec!["d", "b", "a", "c"]);
        assert_eq!(list_names(&files, ListSort::LeastPlayed), vec!["a", "c", "b", "d"]);
    }

    #[test]
    fn an_unread_length_sorts_last() {
        let files = vec![f("z.ogg", Some(500), 0), f("a.ogg", None, 0), f("m.ogg", Some(100), 0)];
        assert_eq!(list_names(&files, ListSort::Length), vec!["m", "z", "a"]);
    }

    #[test]
    fn folder_line_counts_files_still_there() {
        let mut files: Vec<_> = (0..41).map(|i| f(&format!("{i}.ogg"), (i < 29).then_some(60), 0)).collect();
        files.push(LocalGuidedFile { missing: true, ..f("gone.ogg", None, 0) });
        assert_eq!(folder_line(&files), 41);
    }

    #[test]
    fn a_tapped_refresh_reports_once_when_done() {
        let mut p = RefreshProgress::default();
        // A refresh the page started itself: no report.
        p.queued(1);
        p.finished(1);
        assert!(!p.settle(false, false));
        p.tap();
        assert!(!p.settle(true, false), "not while it runs");
        assert!(p.settle(false, false));
        assert!(!p.settle(false, false), "once");
    }

    #[test]
    fn an_unreachable_folder_reports_no_refresh() {
        let mut p = RefreshProgress::default();
        p.tap();
        assert!(!p.settle(false, true));
        assert!(!p.settle(false, false), "the tap was used up");
    }

    #[test]
    fn a_failed_write_reports_no_refresh() {
        let mut p = RefreshProgress::default();
        p.tap();
        p.write_failed();
        assert!(!p.settle(false, false));
        p.tap();
        assert!(p.settle(false, false), "the next run starts clean");
    }

    #[test]
    fn a_failure_with_no_tapped_refresh_is_forgotten() {
        let mut p = RefreshProgress::default();
        // A picked file's length couldn't be read: no refresh waits.
        p.queued(7);
        p.write_failed();
        p.tap();
        assert!(p.settle(false, false), "the later refresh still reports");
    }

    #[test]
    fn the_refresh_percentage_counts_this_runs_files() {
        let mut p = RefreshProgress::default();
        assert_eq!(p.percent(), None, "walking: no number");
        for id in 1..=4 {
            p.queued(id);
        }
        p.queued(2); // picked: read first, still one file
        assert_eq!(p.percent(), Some(0));
        p.finished(2);
        assert_eq!(p.percent(), Some(25));
        p.finished(1);
        p.finished(3);
        assert_eq!(p.percent(), Some(75));
        p.finished(4);
        p.settle(false, false);
        assert_eq!(p.percent(), None, "the next run starts afresh");
    }

    #[test]
    fn starred_files_list_library_and_local_files() {
        let db = Database::open_in_memory().unwrap();
        db.insert_guided_file_with_uuid("g1", "Loving kindness", "g.ogg", 1080, true).unwrap();
        db.insert_guided_file_with_uuid("g2", "Not starred", "h.ogg", 60, false).unwrap();
        db.set_local_folder("/m").unwrap();
        let scan = |names: &[&str]| {
            FolderScan::Found(
                names.iter().map(|n| ScannedFile { rel_path: (*n).into(), size: 1, mtime: 1 }).collect(),
            )
        };
        db.apply_local_scan("/m", &scan(&["Sleep/Body scan.ogg", "Walking.ogg", "x.ogg"])).unwrap();
        for file in crate::db::list_local_files_from_db(&db, "/m").unwrap() {
            if file.rel_path != "x.ogg" {
                db.set_local_file_starred(file.id, true).unwrap();
            }
        }
        db.apply_local_scan("/m", &scan(&["Sleep/Body scan.ogg", "x.ogg"])).unwrap();
        let body_scan = crate::db::list_local_files_from_db(&db, "/m").unwrap()[0].id;
        db.set_local_file_duration(body_scan, 1380).unwrap();
        let rows = starred_guided_from_db(&db).unwrap();
        assert!(
            rows.iter().any(|r| matches!(r, StarredGuided::Local { file, found: true } if file.duration_secs == Some(1380))),
            "the local entry carries its length"
        );
        let summary: Vec<(String, Option<bool>)> = rows
            .iter()
            .map(|r| match r {
                StarredGuided::Library(g) => (g.name.clone(), None),
                StarredGuided::Local { file, found } => (file_name(&file.rel_path).to_string(), Some(*found)),
            })
            .collect();
        assert_eq!(
            summary,
            vec![
                ("Loving kindness".to_string(), None),
                ("Body scan".to_string(), Some(true)),
                ("Walking".to_string(), Some(false)),
            ]
        );
        // Another folder chosen: the old folder's stars are not found.
        db.set_local_folder("/other").unwrap();
        let rows = starred_guided_from_db(&db).unwrap();
        assert!(rows.iter().all(|r| !matches!(r, StarredGuided::Local { found: true, .. })));
    }

    #[test]
    fn view_and_sort_are_remembered() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(list_sort_from_db(&db), ListSort::Name);
        assert!(!list_view_on_from_db(&db));
        set_list_sort(&db, ListSort::LeastPlayed).unwrap();
        set_list_view_on(&db, true).unwrap();
        assert_eq!(list_sort_from_db(&db), ListSort::LeastPlayed);
        assert!(list_view_on_from_db(&db));
        db.set_setting(LIST_SORT_KEY, "nonsense").unwrap();
        assert_eq!(list_sort_from_db(&db), ListSort::Name);
    }
}
