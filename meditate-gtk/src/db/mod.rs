//! The Linux app's database: core's `Database`, plus what only Linux
//! needs: `open` with the bundled bells from GResource, the unix-time
//! `Session` shim, and the shared toast path.

use gtk::prelude::*;
use std::path::Path;

use meditate_core::db::{DbError, Result};
pub use meditate_core::Database;

use crate::i18n::gettext;

/// Which message a failed write shows.
#[derive(Clone, Copy)]
pub enum Failed {
    Save,
    Delete,
}

/// `true` when the write landed. Otherwise logs it under `tag` and
/// toasts "Save failed" or "Delete failed" (or "«X» is already taken"
/// for a name clash) on the toast overlay nearest to `anchor`. No database at
/// all counts as a failure.
pub fn write_ok<T, E: std::fmt::Debug + 'static>(
    anchor: &impl gtk::glib::object::IsA<gtk::Widget>,
    tag: &str,
    result: Option<std::result::Result<T, E>>,
    failed: Failed,
) -> bool {
    let err = match result {
        Some(Ok(_)) => return true,
        Some(Err(e)) => e,
        None => {
            meditate_core::log(tag, "failed: no database");
            show_write_failure(anchor, None, failed);
            return false;
        }
    };
    meditate_core::log(tag, &format!("failed: {err:?}"));
    show_write_failure(anchor, clash_text(&err), failed);
    false
}

/// "«X» is already taken" when the write failed on a name clash.
fn clash_text(err: &dyn std::any::Any) -> Option<String> {
    let name = err.downcast_ref::<DbError>()?.taken_name()?;
    Some(gettext("«{name}» is already taken").replace("{name}", name))
}

fn show_write_failure(anchor: &impl gtk::glib::object::IsA<gtk::Widget>, text: Option<String>, failed: Failed) {
    let text = text.unwrap_or_else(|| match failed {
        Failed::Save => gettext("Save failed"),
        Failed::Delete => gettext("Delete failed"),
    });
    show_toast(anchor, text);
}

/// A toast on `anchor`'s page (see `post_toast`). Only the same
/// message replaces it, so a slider step that fails again doesn't
/// queue, and one failure never hides another.
pub fn show_toast(anchor: &impl gtk::glib::object::IsA<gtk::Widget>, text: impl Into<gtk::glib::GString>) {
    let text = text.into();
    post_toast(anchor, Some(&format!("plain:{text}")), plain_toast(text));
}

/// A message with no page of its own (a bell error, a background save
/// or scan): on the page in view.
pub fn show_in_view(text: impl Into<gtk::glib::GString>) {
    let text = text.into();
    post_in_view(Some(&format!("plain:{text}")), plain_toast(text));
}

/// A toast whose title wraps: a plain title is one line,
/// cut with "…" on a narrow window.
pub fn plain_toast(text: impl Into<gtk::glib::GString>) -> adw::Toast {
    // WordChar: a name with no space breaks too, instead of widening the page.
    let label = gtk::Label::builder().label(text.into()).wrap(true).wrap_mode(gtk::pango::WrapMode::WordChar)
        // A little off the pill's round left edge, as a one-line title is.
        .xalign(0.0).margin_start(6).css_classes(["heading"]).build();
    adw::Toast::builder().custom_title(&label).build()
}

/// An Undo offer. `on_untaken` runs when it goes without Undo: timed
/// out, closed, or replaced by a newer offer.
pub fn undo_toast(
    text: impl Into<gtk::glib::GString>,
    on_undo: impl Fn() + 'static,
    on_untaken: impl Fn() + 'static,
) -> adw::Toast {
    let toast = plain_toast(text);
    // The title takes its one-line width first: capped, it wraps and
    // leaves Undo its room at phone width.
    if let Some(label) = toast.custom_title().and_downcast::<gtk::Label>() {
        label.set_max_width_chars(20);
    }
    toast.set_timeout(5);
    toast.set_button_label(Some(&gettext("Undo")));
    let undone = std::rc::Rc::new(std::cell::Cell::new(false));
    let undone_btn = undone.clone();
    toast.connect_button_clicked(move |_| {
        undone_btn.set(true);
        on_undo();
    });
    toast.connect_dismissed(move |_| {
        if !undone.get() { on_untaken(); }
    });
    toast
}

thread_local! {
    // The last toast per slot and overlay. Weak: the overlay owns it, and
    // a popped page must not stay alive through it.
    static LAST: std::cell::RefCell<std::collections::HashMap<String, gtk::glib::WeakRef<adw::Toast>>> = std::cell::RefCell::default();
}

/// Show `toast` on `anchor`'s page, replacing the last toast posted
/// there to the same `slot` (`None` queues). That page shows it also
/// when the user only comes back to it, as when an editor pops. An
/// anchor already gone from the window goes to the page in view.
pub fn post_toast(anchor: &impl gtk::glib::object::IsA<gtk::Widget>, slot: Option<&str>, toast: adw::Toast) {
    let anchor = anchor.as_ref();
    // A popped page keeps its overlay, out of the window. A dialog's own
    // overlay (Preferences has one) sits below the dialog itself.
    let own = anchor.root().and_then(|_| {
        anchor.ancestor(adw::ToastOverlay::static_type()).and_downcast::<adw::ToastOverlay>()
            .or_else(|| first_overlay(&anchor.ancestor(adw::Dialog::static_type())?))
    });
    match own {
        Some(overlay) => show_on(&overlay, slot, toast),
        None => post_in_view(slot, toast),
    }
}

/// `post_toast` for a toast with no page of its own.
pub fn post_in_view(slot: Option<&str>, toast: adw::Toast) {
    let Some(window) = crate::application::MeditateApplication::get().and_then(|app| app.main_window()) else { return };
    let slot = slot.map(str::to_owned);
    // Picked once the running handler is done: Start pushes the running
    // page only after its own messages.
    gtk::glib::idle_add_local_once(move || show_on(&window.overlay_in_view(), slot.as_deref(), toast));
}

/// The first toast overlay inside `w`; of a navigation view, only in
/// the page it shows.
pub(crate) fn first_overlay(w: &gtk::Widget) -> Option<adw::ToastOverlay> {
    if let Some(nav) = w.downcast_ref::<adw::NavigationView>() {
        return nav.visible_page().and_then(|p| first_overlay(p.upcast_ref()));
    }
    std::iter::successors(w.first_child(), |c| c.next_sibling())
        .find_map(|c| c.clone().downcast().ok().or_else(|| first_overlay(&c)))
}

fn show_on(overlay: &adw::ToastOverlay, slot: Option<&str>, toast: adw::Toast) {
    let old = slot.and_then(|slot| {
        // Per overlay: the same message on another page is another toast.
        let key = format!("{:p}/{slot}", overlay.as_ptr());
        // Recorded first: the old one's dismiss handler may post a toast
        // of its own, which must not replace this one.
        let old = LAST.with(|l| l.borrow_mut().insert(key.clone(), toast.downgrade()));
        toast.connect_dismissed(move |t| LAST.with(|l| {
            let mut l = l.borrow_mut();
            if l.get(&key).and_then(|w| w.upgrade()).as_ref() == Some(t) { l.remove(&key); }
        }));
        old?.upgrade()
    });
    overlay.add_toast(toast);
    if let Some(old) = old { old.dismiss(); }
}

// ── Models ────────────────────────────────────────────────────────────────────

/// Core types the Linux code imports from `crate::db`.
pub use meditate_core::db::{
    mint_uuid, BellSound, BellSoundCategory, BoxBreathPhaseId, ChartKind, GuidedFile, IntervalBell, IntervalBellKind, Label, Preset, SessionMode, SignalMode, VibrationPattern,
};

/// The GResource path of a bundled bell this build ships, by uuid.
/// Bundled audio is always found here, never through the row's
/// stored `file_path` (sync can leave another platform's path there).
pub fn bundled_bell_resource(uuid: &str) -> Option<&'static str> {
    BUNDLED_BELL_SOUNDS
        .iter()
        .find(|(u, ..)| *u == uuid)
        .map(|(_, _, path, _)| *path)
}

/// Bundled bell-sound seed: hardcoded (uuid, display name, GResource
/// path, MIME). UUIDs are STABLE across versions — never edit a row
/// here. New bundles get appended (never replace) so a peer that
/// already seeded the old set picks up the new ones via insert-or-
/// ignore. Adding a sound is a 1-tuple addition; no migration code.
const BUNDLED_BELL_SOUNDS: &[(&str, &str, &str, &str)] = &[
    (
        "f0c2e8a1-3a72-4d4f-9c8b-1b0e5d8c0001",
        "Singing Bowl",
        "/io/github/janekbt/Meditate/sounds/bowl.ogg",
        "audio/ogg",
    ),
    (
        "f0c2e8a1-3a72-4d4f-9c8b-1b0e5d8c0002",
        "Bell",
        "/io/github/janekbt/Meditate/sounds/bell.ogg",
        "audio/ogg",
    ),
    (
        "f0c2e8a1-3a72-4d4f-9c8b-1b0e5d8c0003",
        "Gong",
        "/io/github/janekbt/Meditate/sounds/gong.ogg",
        "audio/ogg",
    ),
    // ── B.* expansion: curated meditation-bell library. All OGG/Vorbis,
    // mono 48 kHz, EBU R128 loudness-normalised to -16 LUFS so the
    // 0.3 s woodblock click and the 30 s bonshō tail land at comparable
    // perceived volume. Sources + license attribution in
    // data/sounds/CREDITS.md.
    (
        "f0c2e8a1-3a72-4d4f-9c8b-1b0e5d8c0004",
        "Tibetan Singing Bowl",
        "/io/github/janekbt/Meditate/sounds/tibetan-bowl-medium.ogg",
        "audio/ogg",
    ),
    (
        "f0c2e8a1-3a72-4d4f-9c8b-1b0e5d8c0005",
        "Inkin",
        "/io/github/janekbt/Meditate/sounds/inkin.ogg",
        "audio/ogg",
    ),
    (
        "f0c2e8a1-3a72-4d4f-9c8b-1b0e5d8c0006",
        "Tingsha",
        "/io/github/janekbt/Meditate/sounds/tingsha.ogg",
        "audio/ogg",
    ),
    (
        meditate_core::seeds::BUNDLED_KANSHO_UUID,
        "Kansho",
        "/io/github/janekbt/Meditate/sounds/kansho.ogg",
        "audio/ogg",
    ),
    (
        "f0c2e8a1-3a72-4d4f-9c8b-1b0e5d8c0008",
        "Burmese Brass Bell",
        "/io/github/janekbt/Meditate/sounds/burmese-brass.ogg",
        "audio/ogg",
    ),
    (
        "f0c2e8a1-3a72-4d4f-9c8b-1b0e5d8c0009",
        "Chau Gong",
        "/io/github/janekbt/Meditate/sounds/chau-gong.ogg",
        "audio/ogg",
    ),
    (
        "f0c2e8a1-3a72-4d4f-9c8b-1b0e5d8c000a",
        "Crystal Bowl",
        "/io/github/janekbt/Meditate/sounds/crystal-bowl.ogg",
        "audio/ogg",
    ),
    (
        "f0c2e8a1-3a72-4d4f-9c8b-1b0e5d8c000b",
        "Woodblock",
        "/io/github/janekbt/Meditate/sounds/woodblock.ogg",
        "audio/ogg",
    ),
];

pub use meditate_core::seeds::{BUNDLED_BOWL_UUID, BUNDLED_PATTERN_PULSE_UUID, BUNDLED_WOODBLOCK_UUID};

#[derive(Debug, Clone)]
pub struct Session {
    pub id: i64,
    /// Cross-device id; Undo of a save deletes by it.
    pub uuid: String,
    /// Unix timestamp (seconds since epoch) of when the session started.
    pub start_time: i64,
    pub duration_secs: i64,
    pub mode: SessionMode,
    pub label_id: Option<i64>,
    pub note: Option<String>,
    /// Set on guided meditation rows that played a library-stored
    /// file. None for non-Guided modes and transient one-off plays.
    pub guided_file_uuid: Option<String>,
    /// The played file's title, for the Log card (any guided session).
    pub guided_file_name: Option<String>,
}

/// Parameters for creating or updating a session.
pub struct SessionData {
    pub start_time: i64,
    pub duration_secs: i64,
    pub mode: SessionMode,
    pub label_id: Option<i64>,
    pub note: Option<String>,
    pub guided_file_uuid: Option<String>,
    pub guided_file_name: Option<String>,
}

pub use meditate_core::db::SessionFilter;

// ── Translation: GTK-side ↔ meditate_core::db ─────────────────────────────────

/// Build core's `Session` from this shell's `SessionData`. The
/// scalar→ISO + clamp logic lives on `meditate_core::db::Session`
/// via `from_unix` so the Android shell can share the same canonical
/// translation; this helper is just a thin field-name renamer
/// (`note` → `notes`).
fn session_data_to_core(s: &SessionData) -> meditate_core::db::Session {
    meditate_core::db::Session::from_unix(
        s.start_time,
        s.duration_secs,
        s.label_id,
        s.note.clone(),
        s.mode,
        s.guided_file_uuid.clone().map(meditate_core::db::GuidedFileUuid::new),
        s.guided_file_name.clone(),
    )
}

/// Inverse of `session_data_to_core` for retrievals: takes core's
/// `(id, Session)` shape and produces the GTK-side `Session` with
/// embedded id and i64-unix `start_time`. Reads `start_unix()` off
/// core so the ISO-parse logic is shared with future shells.
fn session_from_core(id: i64, core: &meditate_core::db::Session) -> Session {
    Session {
        id,
        uuid: core.uuid.to_string(),
        start_time: core.start_unix(),
        duration_secs: i64::from(core.duration_secs),
        mode: core.mode,
        label_id: core.label_id,
        note: core.notes.clone(),
        guided_file_uuid: core.guided_file_uuid.as_ref().map(|u| u.0.clone()),
        guided_file_name: core.guided_file_name.clone(),
    }
}

/// Open (or create) the database at `path` and seed it. Core's own
/// `Database::open` would skip the bundled bells.
pub fn open(path: &Path) -> Result<Database> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| DbError::Decode(format!("mkdir {}: {e}", parent.display())))?;
    }
    let db = Database::open(path)?;
    // Bell sounds stay shell-side: the seed list carries gresource
    // paths that don't exist on Android.
    db.seed_bell_sounds_with_paths(BUNDLED_BELL_SOUNDS)?;
    db.seed_all_non_audio()?;
    Ok(db)
}

pub fn create_session(db: &Database, data: &SessionData) -> Result<Session> {
    let core = session_data_to_core(data);
    let id = db.insert_session(&core)?;
    Ok(session_from_core(id, &core))
}

/// Save a just-ended session and drop its recovery snapshot in
/// one write, counting the play of its Local Folder file (see
/// core's `insert_session_counting_local_play`).
pub fn save_ended_session(db: &Database, data: &SessionData, local_file_id: Option<i64>) -> Result<Session> {
    let (id, stored) = db.insert_session_counting_local_play(&session_data_to_core(data), local_file_id)?;
    Ok(session_from_core(id, &stored))
}

pub fn list_sessions(db: &Database, filter: &SessionFilter) -> Result<Vec<Session>> {
    let rows = meditate_core::db::query_sessions_from_db(db, filter)?;
    Ok(rows.into_iter().map(|(id, c)| session_from_core(id, &c)).collect())
}

pub fn update_session(db: &Database, id: i64, data: &SessionData) -> Result<()> {
    db.update_session(id, &session_data_to_core(data))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #52, #191: a failed write must reach the user. `#[must_use]` on
    /// `with_db` catches a bare statement; this catches the rest.
    #[test]
    fn no_write_result_is_dropped() {
        let setting_writes = ["set_setting(", "bell_volume::write(", "set_box_breath_phase(", "edit_box_breath_phase(", "settings_keys::set_", "labels::set_", "labels::persist_"];
        let mut found = Vec::new();
        let mut dirs = vec![std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src")];
        while let Some(dir) = dirs.pop() {
            for path in std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()) {
                if path.is_dir() { dirs.push(path); continue; }
                if path.extension().is_none_or(|e| e != "rs") { continue; }
                let full = std::fs::read_to_string(&path).unwrap();
                let src = full.split("#[cfg(test)]\nmod").next().unwrap();
                for (n, line) in src.lines().enumerate() {
                    let dropped = line.contains("let _ =") && (line.contains("with_db") || line.contains("db."));
                    // #191: a setting write whose error only reaches the log.
                    let logged = line.trim_start().starts_with("if let ") && line.contains("Err(")
                        && setting_writes.iter().any(|w| line.contains(w));
                    if dropped || logged {
                        found.push(format!("{}:{}: {}", path.display(), n + 1, line.trim()));
                    }
                }
            }
        }
        assert!(found.is_empty(), "dropped write results:\n{}", found.join("\n"));
    }

    #[test]
    fn a_core_name_clash_keeps_its_text() {
        // Every write returns core's error; a name clash names the name, not "Save failed".
        use meditate_core::db::DbError as E;
        for err in [
            E::DuplicateLabel("Zen".into()),
            E::DuplicatePreset("Zen".into()),
            E::DuplicateGuidedFile("Zen".into()),
            E::DuplicateVibrationPattern("Zen".into()),
            E::DuplicateBellSound("Zen".into()),
        ] {
            assert!(clash_text(&err).is_some_and(|t| t.contains("Zen")), "{err:?}");
        }
        assert_eq!(clash_text(&E::DateOutOfRange), None);
    }

    /// The bundled bells are seeded only by `db::open`; core's own opener skips them.
    #[test]
    fn the_app_opens_its_database_with_the_bundled_bells() {
        let sources = production_sources();
        let callers: Vec<_> = sources.iter()
            .filter(|(p, s)| !p.ends_with("db/mod.rs") && s.contains("Database::open("))
            .map(|(p, _)| p.clone())
            .collect();
        assert!(callers.is_empty(), "core's opener used directly: {callers:?}");
        assert!(file(&sources, "application.rs").contains("db::open(&db_path)"));
    }

    /// Every production file under `src/`, cut at its test module.
    fn production_sources() -> Vec<(String, String)> {
        fn walk(dir: &std::path::Path, out: &mut Vec<(String, String)>) {
            for e in std::fs::read_dir(dir).unwrap() {
                let p = e.unwrap().path();
                if p.is_dir() {
                    walk(&p, out);
                } else if p.extension().is_some_and(|x| x == "rs") {
                    let src = std::fs::read_to_string(&p).unwrap();
                    // The test module ends at the first `}` in column 0;
                    // some files keep production code after it.
                    let cut = match src.find("#[cfg(test)]\nmod tests {") {
                        Some(at) => {
                            let end = src[at..].find("\n}\n").map_or(src.len(), |e| at + e + 3);
                            format!("{}{}", &src[..at], &src[end..])
                        }
                        None => src,
                    };
                    let name = p.strip_prefix(concat!(env!("CARGO_MANIFEST_DIR"), "/src/")).unwrap();
                    out.push((name.to_string_lossy().into_owned(), cut));
                }
            }
        }
        let mut out = Vec::new();
        walk(std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/src")), &mut out);
        out
    }

    /// A toast built or posted past the shared path was cut with "…"
    /// or landed on an overlay a pushed page hid.
    #[test]
    fn every_toast_goes_through_the_shared_path() {
        let sources = production_sources();
        let mut stray = Vec::new();
        for (file, src) in &sources {
            if file == "db/mod.rs" { continue; }
            for pat in ["Toast::new(", "Toast::builder(", "add_toast("] {
                if src.contains(pat) { stray.push(format!("{file}: {pat}")); }
            }
        }
        assert_eq!(stray, Vec::<String>::new());
        // ...and that path wraps its title instead of cutting it.
        let db = file(&sources, "db/mod.rs");
        let plain = &db[db.find("pub fn plain_toast(").unwrap()..];
        let plain = &plain[..plain.find("\n}\n").unwrap()];
        assert!(plain.contains(".wrap(true)") && plain.contains(".custom_title(&label)"));
        // A name with no space broke no line and widened the page, and a
        // one-line title left the Undo button "Rüc…" (laptop, 2026-11-11).
        assert!(plain.contains("wrap_mode(gtk::pango::WrapMode::WordChar)"), "breaks inside a long word");
        let undo = &db[db.find("pub fn undo_toast(").unwrap()..];
        assert!(undo[..undo.find("\n}\n").unwrap()].contains("set_max_width_chars("), "wraps before it takes Undo's room");
        // The running and Local Folder pages, where a failure also comes
        // while they are in view, have an overlay of their own.
        for name in ["window/imp.rs", "local_folder.rs"] {
            let src = file(&sources, name);
            assert!(src.contains("let overlay = adw::ToastOverlay::new();\n") && src.contains("overlay.set_child(Some(&toolbar"), "{name}");
            assert!(src.contains(".child(&overlay)"), "{name}: the page shows the overlay");
        }
    }

    /// The root-downcast guard skipped a dialog without a word.
    #[test]
    fn dialogs_open_on_their_anchor() {
        let sources = production_sources();
        let guard: Vec<_> = sources.iter()
            .filter(|(_, src)| [".present(Some(&win))", ".present(Some(&window))"].iter().any(|p| src.contains(p)))
            .map(|(f, _)| f.clone()).collect();
        assert_eq!(guard, Vec::<String>::new());
        let bells = file(&sources, "bells.rs");
        assert_eq!(bells.matches("\"Delete Bell?\"").count(), 1, "one delete dialog");
        let edit = &bells[bells.find("fn push_edit_page(").unwrap()..];
        assert!(edit.contains("confirm_and_delete(btn, "), "the edit page uses it too");
    }

    /// "Deleted Body scan with long pa…": a cut name, glued to a word
    /// translators could not move.
    #[test]
    fn no_message_cuts_or_glues_a_name() {
        let sources = production_sources();
        let mut found = Vec::new();
        for (file, src) in &sources {
            for pat in [" ellipsize(", "format!(\"{} {}\", gettext(", "format!(\"{}: {", "format!(\"{} {} ({})\""] {
                if src.contains(pat) { found.push(format!("{file}: {pat}")); }
            }
        }
        assert_eq!(found, Vec::<String>::new());
        assert!(file(&sources, "vibrations.rs").contains("gettext(\"'{name}' updated\")"));
        assert!(file(&sources, "guided.rs").contains("gettext(\"'{name}' deleted\")"));
    }

    /// A message with no page of its own (the window as anchor) went
    /// under the running page Start pushes next, under a nested page,
    /// or replaced the same text on another page; a lost bell error
    /// never came back.
    #[test]
    fn messages_with_no_page_go_to_the_page_in_view() {
        let sources = production_sources();
        let db = file(&sources, "db/mod.rs");
        let post = &db[db.find("pub fn post_toast(").unwrap()..];
        assert!(post.contains("None => post_in_view(slot, toast)"), "no page of its own");
        assert!(db.contains("idle_add_local_once(move || show_on(&window.overlay_in_view()"), "picked after the handler");
        assert!(db.contains("format!(\"{:p}/{slot}\", overlay.as_ptr())"), "slots per overlay");
        let first = &db[db.find("fn first_overlay(").unwrap()..];
        assert!(first[..first.find("\n}\n").unwrap()].contains("nav.visible_page()"), "only the shown page");
        assert!(file(&sources, "window/mod.rs").contains("self.dialogs()"), "every open dialog, top first");
        let timer = file(&sources, "timer/imp.rs");
        let start = &timer[timer.find("fn on_start(&self)").unwrap()..];
        assert!(start[..start.find("emit_by_name::<()>(\"timer-started\"").unwrap()].contains("crate::sound::rearm_audio_error_toast();"));
        let log = file(&sources, "log/imp.rs");
        let commit = &log[log.find("fn commit_all_pending(").unwrap()..];
        assert!(commit.contains("self.obj().root()") && commit.contains("write_ok(&anchor, \"log.delete\""));
    }

    fn file<'a>(sources: &'a [(String, String)], name: &str) -> &'a str {
        &sources.iter().find(|(f, _)| f == name).unwrap_or_else(|| panic!("{name}")).1
    }
}
