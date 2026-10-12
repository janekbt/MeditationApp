//! Guided → Local Folder (GitHub #4): a folder the user manages
//! outside the app, played in place. Core decides what the rows are
//! (`meditate_core::local_folder`); this module walks the folder and
//! reads lengths on worker threads, and draws the pages.

use std::cell::{Cell, RefCell};
use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use adw::prelude::*;
use meditate_core::db::{FolderScan, LocalGuidedFile};
use meditate_core::local_folder::{self as lf, FolderRow, ListSort};

use crate::application::MeditateApplication;
use crate::guided::GuidedFilePick;
use crate::i18n::{gettext, ngettext};

thread_local! {
    /// Called after every scan or length read; a listener returns
    /// false once its widget is gone and is then dropped.
    static LISTENERS: RefCell<Vec<Box<dyn Fn() -> bool>>> = RefCell::new(Vec::new());
    /// The last scan could not open the folder.
    static UNREACHABLE: Cell<bool> = const { Cell::new(false) };
    static SCANNING: Cell<bool> = const { Cell::new(false) };
    /// Files whose length is still to be read, front first.
    static QUEUE: RefCell<VecDeque<(i64, PathBuf)>> = const { RefCell::new(VecDeque::new()) };
    static READING: Cell<bool> = const { Cell::new(false) };
    /// Files whose length could not be read in this run of the app.
    static FAILED: RefCell<HashSet<i64>> = RefCell::new(HashSet::new());
}

/// Run `f` after every scan or length read while it returns true.
pub fn on_change(f: impl Fn() -> bool + 'static) {
    LISTENERS.with(|l| l.borrow_mut().push(Box::new(f)));
}

fn notify() {
    let listeners = LISTENERS.with(|l| std::mem::take(&mut *l.borrow_mut()));
    let kept: Vec<_> = listeners.into_iter().filter(|f| f()).collect();
    LISTENERS.with(|l| {
        let mut l = l.borrow_mut();
        let added = std::mem::take(&mut *l);
        *l = kept;
        l.extend(added);
    });
}

/// True when the length of `id` could not be read.
pub fn length_failed(id: i64) -> bool {
    FAILED.with(|f| f.borrow().contains(&id))
}

/// A Local Folder write. These write no event, so they don't start a
/// sync (`with_db`, not `with_db_mut`); a failure says so.
pub fn save<T, E>(app: &MeditateApplication, f: impl FnOnce(&meditate_core::Database) -> Result<T, E>) -> Option<T> {
    let done = app.with_db(|db| f(db.core())).and_then(Result::ok);
    if done.is_none() {
        if let Some(window) = app.active_window().and_downcast::<crate::window::MeditateWindow>() {
            window.add_toast(adw::Toast::new(&gettext("Save failed")));
        }
    }
    done
}

pub fn chosen_folder(app: &MeditateApplication) -> Option<String> {
    app.with_db(|db| meditate_core::db::local_folder_from_db(db.core()))
        .and_then(Result::ok)
        .flatten()
}

fn files_of(app: &MeditateApplication, root: &str) -> Vec<LocalGuidedFile> {
    app.with_db(|db| meditate_core::db::list_local_files_from_db(db.core(), root))
        .and_then(Result::ok)
        .unwrap_or_default()
}

/// Walk the chosen folder on a worker thread, bring the table in
/// line, then read the lengths still missing.
pub fn scan(app: &MeditateApplication) {
    let Some(root) = chosen_folder(app) else { return };
    if SCANNING.with(|s| s.replace(true)) {
        return;
    }
    let app = app.clone();
    glib::MainContext::default().spawn_local(async move {
        let walk_root = root.clone();
        let walked = gtk::gio::spawn_blocking(move || {
            meditate_core::db::walk_folder(Path::new(&walk_root))
        })
        .await;
        let walk = match walked {
            Ok(Ok(files)) => FolderScan::Found(files),
            _ => FolderScan::Unreachable,
        };
        // A failed write is not an unreachable folder: it says "Save failed".
        let reachable = save(&app, |db| db.apply_local_scan(&root, &walk)).unwrap_or(true);
        UNREACHABLE.with(|u| u.set(!reachable));
        SCANNING.with(|s| s.set(false));
        notify();
        // Another folder chosen during the walk: walk that one too.
        if chosen_folder(&app).as_deref() != Some(root.as_str()) {
            scan(&app);
            return;
        }
        let unread: Vec<_> = files_of(&app, &root)
            .into_iter()
            .filter(|f| !f.missing && f.duration_secs.is_none())
            .map(|f| (f.id, Path::new(&root).join(&f.rel_path)))
            .collect();
        QUEUE.with(|q| {
            let mut q = q.borrow_mut();
            for item in unread {
                if !q.iter().any(|(id, _)| *id == item.0) {
                    q.push_back(item);
                }
            }
        });
        read_lengths(&app);
    });
}

/// Read `file`'s length before every other waiting file.
pub fn read_first(app: &MeditateApplication, id: i64, path: PathBuf) {
    QUEUE.with(|q| {
        let mut q = q.borrow_mut();
        q.retain(|(other, _)| *other != id);
        q.push_front((id, path));
    });
    read_lengths(app);
}

fn read_lengths(app: &MeditateApplication) {
    if READING.with(|r| r.replace(true)) {
        return;
    }
    let app = app.clone();
    glib::MainContext::default().spawn_local(async move {
        while let Some((id, path)) = QUEUE.with(|q| q.borrow_mut().pop_front()) {
            let probed = gtk::gio::spawn_blocking(move || crate::guided::probe_duration_secs(&path)).await;
            match probed {
                Ok(Ok(secs)) => {
                    save(&app, |db| db.set_local_file_duration(id, secs));
                }
                _ => {
                    FAILED.with(|f| f.borrow_mut().insert(id));
                }
            }
            notify();
        }
        READING.with(|r| r.set(false));
    });
}

/// The pick for a local file; a length not read yet is 0 and is read
/// next.
pub fn pick_for(app: &MeditateApplication, file: &LocalGuidedFile) -> GuidedFilePick {
    let path = Path::new(&file.root).join(&file.rel_path);
    if file.duration_secs.is_none() {
        FAILED.with(|f| f.borrow_mut().remove(&file.id));
        read_first(app, file.id, path.clone());
    }
    GuidedFilePick {
        display_name: lf::file_name(&file.rel_path).to_string(),
        source_path: path,
        duration_secs: file.duration_secs.unwrap_or(0),
        local_file_id: Some(file.id),
        folder: Some(lf::folder_label(&file.rel_path)),
    }
}

/// Length and plays, as a file row shows them.
pub fn length_text(secs: Option<u32>) -> String {
    match secs {
        Some(secs) => meditate_core::format::format_duration_brief(secs),
        None => gettext("length not read yet"),
    }
}

fn plays_text(plays: u32) -> String {
    match plays {
        0 => gettext("never played"),
        n => ngettext("played once", "played {n} times", n).replace("{n}", &n.to_string()),
    }
}

fn counts_text(folders: u32, files: u32) -> String {
    let folders_text = ngettext("1 folder", "{n} folders", folders).replace("{n}", &folders.to_string());
    let files_text = ngettext("1 file", "{n} files", files).replace("{n}", &files.to_string());
    match (folders, files) {
        (0, _) => files_text,
        (_, 0) => folders_text,
        _ => format!("{folders_text}, {files_text}"),
    }
}

/// Folder line: "~/Music/Meditations · 41 files" or, while reading,
/// "~/Music/Meditations · reading 29/41".
fn folder_line_text(root: &str, files: &[LocalGuidedFile]) -> String {
    let line = lf::folder_line(files);
    let shown = home_relative(root);
    // Reading stops at files it can't read; those keep no length.
    let busy = READING.with(Cell::get) || !QUEUE.with(|q| q.borrow().is_empty());
    let count = if busy && line.read < line.files {
        gettext("reading {read}/{total}")
            .replace("{read}", &line.read.to_string())
            .replace("{total}", &line.files.to_string())
    } else {
        counts_text(0, line.files)
    };
    format!("{shown} · {count}")
}

fn home_relative(root: &str) -> String {
    let home = glib::home_dir();
    match Path::new(root).strip_prefix(&home) {
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => root.to_string(),
    }
}

fn star_button(file: &LocalGuidedFile, app: &MeditateApplication) -> gtk::Button {
    let button = gtk::Button::builder()
        .icon_name(if file.is_starred { "starred-symbolic" } else { "non-starred-symbolic" })
        .tooltip_text(if file.is_starred { gettext("Unstar") } else { gettext("Star") })
        .valign(gtk::Align::Center)
        .css_classes(["flat"])
        .build();
    if file.is_starred {
        button.add_css_class("preset-star-on");
    }
    let (id, starred, app) = (file.id, file.is_starred, app.clone());
    button.connect_clicked(move |_| {
        save(&app, |db| db.set_local_file_starred(id, !starred));
        notify();
    });
    button
}

fn file_row(
    file: &LocalGuidedFile,
    with_folder: bool,
    app: &MeditateApplication,
    on_pick: &Rc<dyn Fn(GuidedFilePick)>,
    nav_view: &adw::NavigationView,
) -> adw::ActionRow {
    let mut parts = Vec::new();
    let folder = lf::folder_label(&file.rel_path);
    if with_folder && !folder.is_empty() {
        parts.push(folder);
    }
    parts.push(length_text(file.duration_secs));
    parts.push(plays_text(file.play_count));
    let row = adw::ActionRow::builder()
        .title(glib::markup_escape_text(lf::file_name(&file.rel_path)))
        .subtitle(glib::markup_escape_text(&parts.join(" · ")))
        .activatable(true)
        .build();
    row.add_suffix(&star_button(file, app));
    let (file, app, on_pick, nav_view) = (file.clone(), app.clone(), on_pick.clone(), nav_view.clone());
    row.connect_activated(move |_| {
        on_pick(pick_for(&app, &file));
        nav_view.pop_to_tag("main");
    });
    row
}

/// Push the Local Folder page. `on_pick` receives the file the user
/// taps.
pub fn push_page(
    nav_view: &adw::NavigationView,
    app: &MeditateApplication,
    on_pick: Rc<dyn Fn(GuidedFilePick)>,
) {
    nav_view.push(&build_page(nav_view, app, String::new(), on_pick));
    scan(app);
}

fn build_page(
    nav_view: &adw::NavigationView,
    app: &MeditateApplication,
    dir: String,
    on_pick: Rc<dyn Fn(GuidedFilePick)>,
) -> adw::NavigationPage {
    let top = dir.is_empty();
    let header = adw::HeaderBar::new();
    let (title, parent) = lf::folder_title(&dir);
    if top {
        header.set_title_widget(Some(&adw::WindowTitle::new(&gettext("Local Folder"), "")));
    } else {
        header.set_title_widget(Some(&adw::WindowTitle::new(&title, parent.as_deref().unwrap_or(""))));
    }
    let refresh = gtk::Button::builder()
        .icon_name("view-refresh-symbolic")
        .tooltip_text(gettext("Refresh"))
        .build();
    header.pack_end(&refresh);
    let banner = adw::Banner::builder()
        .title(gettext("Folder not reachable"))
        .button_label(gettext("Choose Again"))
        .build();
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.add_top_bar(&banner);
    // Kept across rebuilds so a length read doesn't scroll the list back up.
    let clamp = adw::Clamp::new();
    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(&clamp)
        .build();
    let parts = Page { toolbar: toolbar.clone(), banner: banner.clone(), scroller, clamp };
    let page = adw::NavigationPage::builder()
        .tag(if top { "local-folder".to_string() } else { format!("local-folder/{dir}") })
        .title(if top { gettext("Local Folder") } else { title })
        .child(&toolbar)
        .build();

    let alive = Rc::new(Cell::new(true));
    page.connect_destroy({
        let alive = alive.clone();
        move |_| alive.set(false)
    });

    let rebuild: crate::Rebuilder = Rc::new(RefCell::new(None));
    let rebuild_weak = Rc::downgrade(&rebuild);
    *rebuild.borrow_mut() = Some(Box::new({
        let (app, nav_view, on_pick) = (app.clone(), nav_view.clone(), on_pick.clone());
        let dir = dir.clone();
        move || {
            let rebuild_again = rebuild_weak.clone();
            let again = move || {
                if let Some(r) = rebuild_again.upgrade() {
                    if let Some(f) = r.borrow().as_ref() {
                        f();
                    }
                }
            };
            fill_page(&parts, &app, &nav_view, &dir, &on_pick, again);
        }
    }));

    let choose = {
        let (app, page) = (app.clone(), page.clone());
        move || choose_folder(&page, &app)
    };
    banner.connect_button_clicked({
        let choose = choose.clone();
        move |_| choose()
    });
    refresh.connect_clicked({
        let app = app.clone();
        move |_| scan(&app)
    });

    if let Some(f) = rebuild.borrow().as_ref() {
        f();
    }
    let rebuild_for_listener = rebuild.clone();
    on_change(move || {
        if !alive.get() {
            return false;
        }
        if let Some(f) = rebuild_for_listener.borrow().as_ref() {
            f();
        }
        true
    });
    page
}

fn choose_folder(anchor: &impl IsA<gtk::Widget>, app: &MeditateApplication) {
    let Some(window) = anchor.root().and_downcast::<gtk::Window>() else { return };
    let dialog = gtk::FileDialog::builder().title(gettext("Choose Folder")).modal(true).build();
    let app = app.clone();
    dialog.select_folder(Some(&window), gtk::gio::Cancellable::NONE, move |result| {
        let Ok(folder) = result else { return };
        let Some(path) = folder.path() else { return };
        let root = path.to_string_lossy().to_string();
        if save(&app, |db| db.set_local_folder(&root)).is_none() {
            return;
        }
        UNREACHABLE.with(|u| u.set(false));
        notify();
        scan(&app);
    });
}

/// The parts of a Local Folder page that a rebuild fills.
struct Page {
    toolbar: adw::ToolbarView,
    banner: adw::Banner,
    scroller: gtk::ScrolledWindow,
    clamp: adw::Clamp,
}

fn fill_page(
    page: &Page,
    app: &MeditateApplication,
    nav_view: &adw::NavigationView,
    dir: &str,
    on_pick: &Rc<dyn Fn(GuidedFilePick)>,
    rebuild: impl Fn() + Clone + 'static,
) {
    let Page { toolbar, banner, scroller, clamp } = page;
    let Some(root) = chosen_folder(app) else {
        banner.set_revealed(false);
        let button = gtk::Button::builder()
            .label(gettext("Choose Folder"))
            .halign(gtk::Align::Center)
            .css_classes(["pill", "suggested-action"])
            .build();
        let anchor = button.clone();
        let app_for_choose = app.clone();
        button.connect_clicked(move |_| choose_folder(&anchor, &app_for_choose));
        let status = adw::StatusPage::builder()
            .icon_name("folder-symbolic")
            .title(gettext("No Local Folder"))
            .description(gettext(
                "Choose a folder of guided meditations. Meditate plays the files where they are and doesn't copy them. They stay on this device and don't sync.",
            ))
            .child(&button)
            .build();
        toolbar.set_content(Some(&status));
        return;
    };
    banner.set_revealed(UNREACHABLE.with(Cell::get));
    let files = files_of(app, &root);
    let prefs = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(24)
        .margin_top(24)
        .margin_bottom(24)
        .margin_start(12)
        .margin_end(12)
        .build();
    let show = |prefs: &gtk::Box| {
        clamp.set_child(Some(prefs));
        if toolbar.content().as_ref() != Some(scroller.upcast_ref()) {
            toolbar.set_content(Some(scroller));
        }
    };

    if dir.is_empty() {
        let folder_group = adw::PreferencesGroup::new();
        let folder_row = adw::ActionRow::builder()
            .title(gettext("Folder"))
            .subtitle(glib::markup_escape_text(&folder_line_text(&root, &files)))
            .build();
        let change = gtk::Button::builder()
            .label(gettext("Change"))
            .valign(gtk::Align::Center)
            .build();
        let app_for_change = app.clone();
        change.connect_clicked(move |b| choose_folder(b, &app_for_change));
        folder_row.add_suffix(&change);
        folder_group.add(&folder_row);
        prefs.append(&folder_group);

        let list_on = app
            .with_db(|db| lf::list_view_on_from_db(db.core()))
            .unwrap_or(false);
        let toggle = adw::ToggleGroup::builder().css_classes(["round"]).build();
        toggle.add(adw::Toggle::builder().name("folders").label(gettext("Folders")).build());
        toggle.add(adw::Toggle::builder().name("list").label(gettext("List")).build());
        toggle.set_active_name(Some(if list_on { "list" } else { "folders" }));
        let toggle_group = adw::PreferencesGroup::new();
        toggle_group.add(&toggle);
        prefs.append(&toggle_group);
        {
            let (app, rebuild) = (app.clone(), rebuild.clone());
            toggle.connect_active_name_notify(move |t| {
                let on = t.active_name().as_deref() == Some("list");
                save(&app, |db| lf::set_list_view_on(db, on));
                // Rebuild after the toggle's own signal returns.
                let rebuild = rebuild.clone();
                glib::idle_add_local_once(rebuild);
            });
        }

        if list_on {
            let sorts = [ListSort::Name, ListSort::Length, ListSort::MostPlayed, ListSort::LeastPlayed];
            let labels = [gettext("Name"), gettext("Length"), gettext("Most played"), gettext("Least played")];
            let sort = app.with_db(|db| lf::list_sort_from_db(db.core())).unwrap_or(ListSort::Name);
            let model = gtk::StringList::new(&labels.iter().map(String::as_str).collect::<Vec<_>>());
            let sort_row = adw::ComboRow::builder().title(gettext("Sort by")).model(&model).build();
            sort_row.set_selected(sorts.iter().position(|s| *s == sort).unwrap_or(0) as u32);
            {
                let (app, rebuild) = (app.clone(), rebuild.clone());
                sort_row.connect_selected_notify(move |row| {
                    let chosen = sorts[(row.selected() as usize).min(sorts.len() - 1)];
                    save(&app, |db| lf::set_list_sort(db, chosen));
                    let rebuild = rebuild.clone();
                    glib::idle_add_local_once(rebuild);
                });
            }
            let sort_group = adw::PreferencesGroup::new();
            sort_group.add(&sort_row);
            prefs.append(&sort_group);

            let list_group = adw::PreferencesGroup::new();
            let rows = lf::list_view(&files, sort);
            for file in &rows {
                list_group.add(&file_row(file, true, app, on_pick, nav_view));
            }
            if rows.is_empty() {
                list_group.add(&empty_row());
            }
            prefs.append(&list_group);
            show(&prefs);
            return;
        }
    }

    let group = adw::PreferencesGroup::new();
    let rows = lf::folder_view(&files, dir);
    for row in &rows {
        match row {
            FolderRow::Folder { name, path, folders, files } => {
                let folder_row = adw::ActionRow::builder()
                    .title(glib::markup_escape_text(name))
                    .subtitle(counts_text(*folders, *files))
                    .activatable(true)
                    .build();
                folder_row.add_prefix(&gtk::Image::from_icon_name("folder-symbolic"));
                folder_row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
                let (nav_view, app, path, on_pick) =
                    (nav_view.clone(), app.clone(), path.clone(), on_pick.clone());
                folder_row.connect_activated(move |_| {
                    nav_view.push(&build_page(&nav_view, &app, path.clone(), on_pick.clone()));
                });
                group.add(&folder_row);
            }
            FolderRow::File(file) => group.add(&file_row(file, false, app, on_pick, nav_view)),
        }
    }
    if rows.is_empty() {
        group.add(&empty_row());
    }
    prefs.append(&group);
    show(&prefs);
}

fn empty_row() -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .title(gettext("No audio files in this folder"))
        .activatable(false)
        .build();
    row.add_css_class("dim-label");
    row
}
