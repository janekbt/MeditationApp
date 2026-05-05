//! Guided-meditation file library + playback.
//!
//! Owns the three flows the Setup view's Guided mode needs:
//!
//! 1. **Open File** — `pick_file_for_open()` shows a `gtk::FileDialog`
//!    and returns a transient `GuidedFilePick` (path + duration +
//!    display name). The Setup view's Selected row reflects this until
//!    the user starts the session, picks a starred file from the list,
//!    or imports it via Import File.
//!
//! 2. **Import File** — `import_picked_file()` takes a transient pick,
//!    asks the user for a display name (with live-validated collision
//!    check against existing rows), transcodes the source file to OGG
//!    under `$XDG_DATA_HOME/meditate/guided/<uuid>.ogg`, and inserts
//!    a `guided_files` row. Auto-stars the new row (matches the
//!    Save Preset auto-star pattern).
//!
//! 3. **Manage Files** — `push_guided_files_chooser()` pushes a
//!    NavigationPage parallel to the label / bell-sound choosers. Per
//!    row: rename, delete, star toggle, tap-to-pick (returns through
//!    the on_picked callback for "play this starred file" UX). A
//!    synthetic "Create new guided file…" entry sits at the top to
//!    re-trigger the file picker → import pipeline.
//!
//! Phase 3 added `probe_duration_secs` (still here at the bottom).
//! Phases 5 + 6 build the timer Setup-view UI + playback engine on
//! top of these primitives.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use adw::prelude::*;

use crate::application::MeditateApplication;
use crate::db::GuidedFile;
use crate::i18n::gettext;

/// Single slot for the chooser's most-recently-shown undo toast.
/// Tapping a second mutating action (rename + rename, delete + delete,
/// rename + delete in either order) dismisses the prior toast so the
/// new one renders without queue delay. The Undo affordance on the
/// dismissed toast is lost, but that's the right trade — the user has
/// just done a NEW action, undoing the previous one would conflict.
type ToastSlot = Rc<RefCell<Option<adw::Toast>>>;

/// Transient selection from "Open File". Lives in the Setup view's
/// state until the session starts, the user picks something else, or
/// the user imports it via Import File. Not persisted across app
/// restarts (per the design-decision-table at phase plan time).
#[derive(Debug, Clone)]
pub struct GuidedFilePick {
    /// Suggested display name — derived from the file basename
    /// (without extension). The user can edit it in the import-name
    /// dialog before saving to the library.
    pub display_name: String,
    /// Absolute path to the picked file. Used to drive playback for
    /// the transient case AND as the source for the transcode worker
    /// when the user taps Import File.
    pub source_path: PathBuf,
    /// Duration in whole seconds, probed via gstreamer playbin.
    /// Drives the hero countdown the moment the row populates.
    pub duration_secs: u32,
}

// ── 1. Open File picker ──────────────────────────────────────────────

/// Show a `gtk::FileDialog` with audio-file filters, probe the
/// duration of the chosen file, and hand the resulting transient pick
/// to `on_picked`. Errors (user cancelled, probe failed, file moved
/// between pick and probe) surface as a window toast and skip the
/// callback — the Setup view's Selected row simply stays where it was.
pub fn pick_file_for_open(
    parent_window: &gtk::Window,
    on_picked: impl Fn(GuidedFilePick) + 'static,
) {
    let dialog = gtk::FileDialog::builder()
        .title(gettext("Choose Guided Meditation"))
        .modal(true)
        .build();

    // Filter to common audio formats. gstreamer can decode all of
    // these via decodebin; the import pipeline transcodes anything
    // not already OGG into OGG/Vorbis on the way in.
    let filter = gtk::FileFilter::new();
    filter.set_name(Some(&gettext("Audio files")));
    for ext in ["ogg", "mp3", "m4a", "aac", "wav", "flac", "opus"] {
        filter.add_pattern(&format!("*.{ext}"));
        filter.add_pattern(&format!("*.{}", ext.to_uppercase()));
    }
    let filters = gtk::gio::ListStore::new::<gtk::FileFilter>();
    filters.append(&filter);
    dialog.set_filters(Some(&filters));
    dialog.set_default_filter(Some(&filter));

    let parent_for_toast = parent_window.clone();
    let on_picked = Rc::new(on_picked);
    dialog.open(Some(parent_window), gtk::gio::Cancellable::NONE, move |result| {
        let file = match result {
            Ok(f) => f,
            Err(e) => {
                // User-cancellation arrives as a Cancelled error
                // variant — we swallow it silently. Other errors
                // (permission denied, etc.) get a toast.
                if !e.matches(gtk::DialogError::Dismissed) {
                    add_toast_to_window(
                        &parent_for_toast,
                        &format!("{}: {e}", gettext("File picker error")),
                    );
                }
                return;
            }
        };
        let Some(path) = file.path() else {
            add_toast_to_window(
                &parent_for_toast,
                &gettext("Picked file has no local path"),
            );
            return;
        };
        let display_name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Untitled")
            .to_string();
        match probe_duration_secs(&path) {
            Ok(duration_secs) => {
                on_picked(GuidedFilePick {
                    display_name,
                    source_path: path,
                    duration_secs,
                });
            }
            Err(e) => add_toast_to_window(
                &parent_for_toast,
                &format!("{}: {e}", gettext("Couldn't read audio file")),
            ),
        }
    });
}

// ── 2. Import File flow ──────────────────────────────────────────────

/// Show a name-dialog for `pick`, lock-and-spinner the Import button
/// while a worker thread transcodes (or copies, for OGG inputs) the
/// source file, then insert a `guided_files` row and call `on_done`.
/// `progress_btn` is the host-page button whose label gets swapped
/// for a "Converting…" spinner during the worker — typically the
/// Setup view's Import File button. Errors surface as a window toast
/// and leave the library unchanged.
pub fn import_picked_file(
    parent_window: &gtk::Window,
    app: &MeditateApplication,
    pick: GuidedFilePick,
    on_done: impl Fn(GuidedFile) + 'static,
) {
    // Live-validated entry — same pattern as the bell-sound import
    // dialog (sounds.rs::present_import_dialog).
    let entry = gtk::Entry::builder()
        .text(&pick.display_name)
        .placeholder_text(gettext("Track name"))
        .activates_default(false) // Import button drives the dialog
        .build();
    let collision_label = gtk::Label::builder()
        .label(gettext("A guided file with this name already exists."))
        .css_classes(["error", "caption"])
        .halign(gtk::Align::Start)
        .visible(false)
        .build();

    let import_btn = gtk::Button::builder()
        .label(gettext("Import"))
        .css_classes(["suggested-action"])
        .hexpand(true)
        .build();

    let form_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .build();
    form_box.append(&entry);
    form_box.append(&collision_label);

    let extra_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(18)
        .build();
    extra_box.append(&form_box);
    extra_box.append(&import_btn);

    let dialog = adw::AlertDialog::builder()
        .heading(gettext("Import Guided File"))
        .body(format!(
            "{} {} ({})",
            gettext("Importing:"),
            pick.display_name,
            format_duration_brief(pick.duration_secs),
        ))
        .extra_child(&extra_box)
        .close_response("cancel")
        .default_response("cancel")
        .build();
    dialog.add_response("cancel", &gettext("Cancel"));

    // Live validation: name not empty + no case-insensitive collision
    // with another row's name. Same shape as the bell-sound + label
    // import flows.
    let validate: Rc<dyn Fn()> = {
        let app = app.clone();
        let entry = entry.clone();
        let import_btn = import_btn.clone();
        let collision_label = collision_label.clone();
        Rc::new(move || {
            let text = entry.text();
            let trimmed = text.trim();
            let collision = !trimmed.is_empty()
                && app
                    .with_db(|db| db.is_guided_file_name_taken(trimmed, ""))
                    .and_then(|r| r.ok())
                    .unwrap_or(false);
            let valid = !trimmed.is_empty() && !collision;
            import_btn.set_sensitive(valid);
            collision_label.set_visible(!trimmed.is_empty() && collision);
        })
    };
    validate();
    let validate_for_change = validate.clone();
    entry.connect_changed(move |_| validate_for_change());

    let import_btn_for_enter = import_btn.clone();
    entry.connect_activate(move |_| {
        if import_btn_for_enter.is_sensitive() {
            import_btn_for_enter.emit_clicked();
        }
    });

    // The Import click drives the spawn_blocking worker. Cancel
    // remains enabled during the transcode — the user can abort a
    // long import (a 60-min stereo guide can take ~10 s on the
    // Librem) by tapping it. The Cancel response flips a shared
    // AtomicBool the worker checks every ~250 ms in its bus loop.
    let app_for_click = app.clone();
    let pick_for_click = pick.clone();
    let entry_for_click = entry.clone();
    let dialog_for_click = dialog.clone();
    let parent_for_click = parent_window.clone();
    let on_done = Rc::new(on_done);
    let on_done_for_click = on_done.clone();
    // Set up the cancel flag once at dialog scope so the response
    // handler (below) and the Import-button handler (here) share
    // the same slot. `Arc<AtomicBool>` because the worker thread
    // reads it across thread boundaries.
    let cancel_flag: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    let cancel_flag_for_dialog = cancel_flag.clone();
    dialog.connect_response(None, move |_, id| {
        if id == "cancel" {
            cancel_flag_for_dialog.store(true, Ordering::Relaxed);
        }
    });

    let cancel_flag_for_click = cancel_flag.clone();
    import_btn.connect_clicked(move |btn| {
        let trimmed = entry_for_click.text().trim().to_string();
        if trimmed.is_empty() {
            return;
        }
        // Freeze the form fields but leave Cancel responsive so the
        // user can abort. The dialog stays open until the worker
        // either completes (success / error) or honours cancel.
        entry_for_click.set_sensitive(false);
        btn.set_sensitive(false);

        let spinner = adw::Spinner::new();
        let label = gtk::Label::new(Some(&gettext("Converting…")));
        let busy_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .halign(gtk::Align::Center)
            .build();
        busy_box.append(&spinner);
        busy_box.append(&label);
        btn.set_child(Some(&busy_box));

        let app = app_for_click.clone();
        let dialog = dialog_for_click.clone();
        let parent = parent_for_click.clone();
        let pick = pick_for_click.clone();
        let trimmed_for_done = trimmed.clone();
        let on_done = on_done_for_click.clone();
        let cancel_flag = cancel_flag_for_click.clone();
        glib::MainContext::default().spawn_local(async move {
            let source = pick.source_path.clone();
            let cancel_for_worker = cancel_flag.clone();
            let import_result = gtk::gio::spawn_blocking(move || {
                do_import_io(&source, &cancel_for_worker)
            })
            .await;

            // Cancellation path: the worker has already cleaned up
            // its partial file; we just dismiss the dialog without
            // inserting a row or firing on_done.
            if cancel_flag.load(Ordering::Relaxed) {
                dialog.force_close();
                return;
            }

            match import_result {
                Ok(Ok((new_uuid, dest_path))) => {
                    let dest_str = dest_path.to_string_lossy().to_string();
                    let mut insert_err: Option<String> = None;
                    app.with_db_mut(|db| {
                        if let Err(e) = db.insert_guided_file_with_uuid(
                            &new_uuid,
                            &trimmed_for_done,
                            &dest_str,
                            pick.duration_secs,
                            true, // auto-star on import (mirrors Save Preset)
                        ) {
                            insert_err = Some(e.to_string());
                        }
                    });
                    if let Some(msg) = insert_err {
                        let _ = std::fs::remove_file(&dest_path);
                        add_toast_to_window(
                            &parent,
                            &format!("{}: {msg}", gettext("Import failed")),
                        );
                    } else if let Some(row) = app
                        .with_db(|db| db.find_guided_file_by_uuid(&new_uuid))
                        .and_then(|r| r.ok())
                        .flatten()
                    {
                        on_done(row);
                    }
                }
                Ok(Err(e)) => add_toast_to_window(
                    &parent,
                    &format!("{}: {e}", gettext("Import failed")),
                ),
                Err(_) => add_toast_to_window(
                    &parent,
                    &gettext("Import worker died"),
                ),
            }
            dialog.force_close();
        });
    });

    dialog.present(Some(parent_window));
    entry.grab_focus();
    entry.select_region(0, -1);
}

/// Worker-thread half of the import: copies (OGG passthrough) or
/// transcodes the source into `$XDG_DATA_HOME/meditate/guided/
/// <uuid>.ogg` and returns the generated UUID + destination path.
/// Mirrors `sounds::do_import_io` but always lands as OGG and
/// preserves source channel layout (no `-ac 1` step). `cancel` is
/// checked at every coarse boundary (around the file copy + inside
/// the transcode pipeline's bus loop); a triggered cancel cleans up
/// the partial dest file before returning so the caller can ignore
/// the result without leaking on-disk state.
fn do_import_io(
    source: &Path,
    cancel: &AtomicBool,
) -> std::result::Result<(String, PathBuf), String> {
    let source_ext = source
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();

    let new_uuid = crate::db::mint_uuid();
    let dest_dir = gtk::glib::user_data_dir().join("meditate").join("guided");
    std::fs::create_dir_all(&dest_dir).map_err(|e| e.to_string())?;
    let dest_path = dest_dir.join(format!("{new_uuid}.ogg"));

    if source_ext == "ogg" {
        // OGG passthrough: a plain copy. fs::copy is a single
        // syscall — too quick to interrupt; we just check the flag
        // before and after so a cancel before copy starts skips
        // straight through.
        if cancel.load(Ordering::Relaxed) {
            return Err(CANCELLED.into());
        }
        std::fs::copy(source, &dest_path).map_err(|e| e.to_string())?;
    } else if let Err(e) = transcode_to_ogg_preserve_channels(source, &dest_path, cancel) {
        let _ = std::fs::remove_file(&dest_path);
        return Err(e);
    }
    if cancel.load(Ordering::Relaxed) {
        let _ = std::fs::remove_file(&dest_path);
        return Err(CANCELLED.into());
    }
    Ok((new_uuid, dest_path))
}

/// Sentinel error string used by the cancel path. The caller branches
/// on `cancel.load()` rather than the error string itself, but having
/// a stable message keeps logs readable when an unexpected `Err` does
/// surface.
const CANCELLED: &str = "cancelled";

/// Like `sounds::transcode_to_ogg`, but for guided meditations:
/// preserves source channel layout (no forced mono) and skips
/// `audioloudnorm` (preserves the artist's intentional voice-vs-music
/// mix). Vorbis quality 0.4 is plenty for spoken-word + ambient
/// content; a 30-min stereo guide lands ~25 MB.
///
/// `cancel` is polled inside the bus loop on a 250 ms timeout — when
/// it flips to true the pipeline transitions to Null and the function
/// returns Err with the CANCELLED sentinel.
fn transcode_to_ogg_preserve_channels(
    source: &Path,
    dest: &Path,
    cancel: &AtomicBool,
) -> std::result::Result<(), String> {
    use gst::prelude::*;
    use gstreamer as gst;

    gst::init().map_err(|e| format!("gst init failed: {e}"))?;

    let pipeline = gst::Pipeline::new();
    let make = |name: &str| -> Result<gst::Element, String> {
        gst::ElementFactory::make(name)
            .build()
            .map_err(|e| format!("create {name}: {e}"))
    };
    let filesrc = gst::ElementFactory::make("filesrc")
        .property("location", source.to_string_lossy().as_ref())
        .build()
        .map_err(|e| format!("create filesrc: {e}"))?;
    let decodebin = make("decodebin")?; // legacy decodebin, see sounds.rs note
    let audioconvert = make("audioconvert")?;
    let audioresample = make("audioresample")?;
    let vorbisenc = gst::ElementFactory::make("vorbisenc")
        .property("quality", 0.4f32)
        .build()
        .map_err(|e| format!("create vorbisenc: {e}"))?;
    let oggmux = make("oggmux")?;
    let filesink = gst::ElementFactory::make("filesink")
        .property("location", dest.to_string_lossy().as_ref())
        .build()
        .map_err(|e| format!("create filesink: {e}"))?;

    pipeline.add(&filesrc).map_err(|e| e.to_string())?;
    pipeline.add(&decodebin).map_err(|e| e.to_string())?;
    for el in [&audioconvert, &audioresample, &vorbisenc, &oggmux, &filesink] {
        pipeline.add(el).map_err(|e| e.to_string())?;
    }
    filesrc.link(&decodebin).map_err(|e| e.to_string())?;
    gst::Element::link_many([
        &audioconvert,
        &audioresample,
        &vorbisenc,
        &oggmux,
        &filesink,
    ])
    .map_err(|e| e.to_string())?;

    // decodebin produces its source pad lazily once typefind resolves
    // the input — link in pad-added.
    let audioconvert_sink = audioconvert
        .static_pad("sink")
        .ok_or_else(|| "audioconvert missing sink pad".to_string())?;
    decodebin.connect_pad_added(move |_, src_pad| {
        if !audioconvert_sink.is_linked() {
            let _ = src_pad.link(&audioconvert_sink);
        }
    });

    pipeline
        .set_state(gst::State::Playing)
        .map_err(|e| e.to_string())?;

    let bus = pipeline
        .bus()
        .ok_or_else(|| "pipeline missing bus".to_string())?;
    let mut transcode_err: Option<String> = None;
    // Bus poll loop with 250 ms timeout so the cancel flag is
    // observed on a sub-second cadence. `iter_timed(NONE)` would
    // block until the next message — fine for hands-off transcodes
    // but useless for honouring user cancellation. 250 ms is
    // imperceptible UX-side and barely measurable in throughput.
    let timeout = gst::ClockTime::from_mseconds(250);
    loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = pipeline.set_state(gst::State::Null);
            return Err(CANCELLED.into());
        }
        let Some(msg) = bus.timed_pop(timeout) else {
            // Timeout — re-check cancel flag and keep polling.
            continue;
        };
        use gst::MessageView::*;
        match msg.view() {
            Eos(..) => break,
            Error(err) => {
                transcode_err = Some(format!(
                    "{} ({})",
                    err.error(),
                    err.debug().unwrap_or_default()
                ));
                break;
            }
            _ => {}
        }
    }
    let _ = pipeline.set_state(gst::State::Null);
    transcode_err.map_or(Ok(()), Err)
}

// ── 3. Manage Files chooser ──────────────────────────────────────────

/// Push the Manage Files page onto `nav_view`. `on_changed` fires
/// after every DB write (rename / delete / star toggle / import) so
/// the host (timer Setup view) can refresh its starred-list in place.
pub fn push_guided_files_chooser<F>(
    nav_view: &adw::NavigationView,
    app: &MeditateApplication,
    on_changed: F,
) where
    F: Fn() + Clone + 'static,
{
    let group = adw::PreferencesGroup::new();
    let rows: Rc<RefCell<Vec<adw::ActionRow>>> = Rc::new(RefCell::new(Vec::new()));

    let prefs_page = adw::PreferencesPage::new();
    prefs_page.add(&group);

    let toast_overlay = adw::ToastOverlay::new();
    toast_overlay.set_child(Some(&prefs_page));

    let header = adw::HeaderBar::builder().show_back_button(true).build();
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&toast_overlay));

    let page = adw::NavigationPage::builder()
        .tag("guided-files")
        .title(gettext("Manage Files"))
        .child(&toolbar)
        .build();

    // Self-referential rebuilder closure: per-row buttons + the
    // synthetic create-row need to fire it after a DB write. RefCell-
    // wrapped Box<dyn Fn()> matches the bells.rs / labels.rs idiom.
    let rebuilder: Rc<RefCell<Option<Box<dyn Fn()>>>> = Rc::new(RefCell::new(None));

    // One slot for the chooser's most-recently-shown undo toast.
    // Subsequent rename/delete actions dismiss whichever toast is in
    // the slot before showing their own — see ToastSlot's docs.
    let toast_slot: ToastSlot = Rc::new(RefCell::new(None));

    let group_for_init = group.clone();
    let rows_for_init = rows.clone();
    let app_for_init = app.clone();
    let nav_view_for_init = nav_view.clone();
    let on_changed_for_init = on_changed.clone();
    let rebuilder_for_init = rebuilder.clone();
    let toast_overlay_for_rb = toast_overlay.clone();
    let toast_slot_for_rb = toast_slot.clone();
    *rebuilder.borrow_mut() = Some(Box::new(move || {
        rebuild_chooser_rows(
            &group_for_init,
            &rows_for_init,
            &app_for_init,
            &nav_view_for_init,
            rebuilder_for_init.clone(),
            on_changed_for_init.clone(),
            &toast_overlay_for_rb,
            toast_slot_for_rb.clone(),
        );
        on_changed_for_init();
    }));

    if let Some(rb) = rebuilder.borrow().as_ref() {
        rb();
    }
    nav_view.push(&page);
}

fn rebuild_chooser_rows(
    group: &adw::PreferencesGroup,
    rows: &Rc<RefCell<Vec<adw::ActionRow>>>,
    app: &MeditateApplication,
    nav_view: &adw::NavigationView,
    rebuilder: Rc<RefCell<Option<Box<dyn Fn()>>>>,
    on_changed: impl Fn() + Clone + 'static,
    toast_overlay: &adw::ToastOverlay,
    toast_slot: ToastSlot,
) {
    for row in rows.borrow_mut().drain(..) {
        group.remove(&row);
    }

    // Synthetic create row — re-enters the file picker → import
    // pipeline. Mirrors how labels / interval bells handle creation.
    let create_row = build_create_row(app, nav_view, rebuilder.clone(), on_changed.clone());
    group.add(&create_row);
    rows.borrow_mut().push(create_row);

    let files = app
        .with_db(|db| db.list_guided_files())
        .and_then(|r| r.ok())
        .unwrap_or_default();

    if files.is_empty() {
        let row = empty_state_row();
        group.add(&row);
        rows.borrow_mut().push(row);
        return;
    }

    for file in files {
        let row = build_guided_file_row(
            &file, app, rebuilder.clone(), on_changed.clone(),
            toast_overlay, toast_slot.clone(),
        );
        group.add(&row);
        rows.borrow_mut().push(row);
    }
}

fn build_create_row(
    app: &MeditateApplication,
    nav_view: &adw::NavigationView,
    rebuilder: Rc<RefCell<Option<Box<dyn Fn()>>>>,
    on_changed: impl Fn() + Clone + 'static,
) -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .title(gettext("Create new guided file…"))
        .activatable(true)
        .build();
    let plus = gtk::Image::from_icon_name("list-add-symbolic");
    plus.add_css_class("dim-label");
    row.add_suffix(&plus);

    let app_for_create = app.clone();
    let _nav_view_for_create = nav_view.clone();
    let rebuilder_for_create = rebuilder.clone();
    let on_changed_for_create = on_changed.clone();
    row.connect_activated(move |row| {
        let Some(window) = window_from(row) else { return; };
        let win_for_pick = window.clone();
        let app_for_pick = app_for_create.clone();
        let rebuilder_for_pick = rebuilder_for_create.clone();
        let on_changed_for_pick = on_changed_for_create.clone();
        pick_file_for_open(window.upcast_ref::<gtk::Window>(), move |pick| {
            let app = app_for_pick.clone();
            let rebuilder = rebuilder_for_pick.clone();
            let on_changed = on_changed_for_pick.clone();
            import_picked_file(
                win_for_pick.upcast_ref::<gtk::Window>(),
                &app,
                pick,
                move |_row| {
                    if let Some(rb) = rebuilder.borrow().as_ref() {
                        rb();
                    }
                    on_changed();
                },
            );
        });
    });

    row
}

fn build_guided_file_row(
    file: &GuidedFile,
    app: &MeditateApplication,
    rebuilder: Rc<RefCell<Option<Box<dyn Fn()>>>>,
    on_changed: impl Fn() + Clone + 'static,
    toast_overlay: &adw::ToastOverlay,
    toast_slot: ToastSlot,
) -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .title(&file.name)
        .subtitle(format_duration_brief(file.duration_secs))
        .activatable(false)
        .build();

    // Star toggle (prefix). Accent-coloured when on, dim outline
    // when off — same visual language the preset chooser uses.
    let star_btn = gtk::Button::builder()
        .icon_name(if file.is_starred {
            "starred-symbolic"
        } else {
            "non-starred-symbolic"
        })
        .css_classes(if file.is_starred {
            vec!["flat", "circular", "preset-star-on"]
        } else {
            vec!["flat", "circular"]
        })
        .tooltip_text(if file.is_starred {
            gettext("Unstar — remove from home list")
        } else {
            gettext("Star — show in home list")
        })
        .valign(gtk::Align::Center)
        .build();
    let app_for_star = app.clone();
    let uuid_for_star = file.uuid.clone();
    let new_starred = !file.is_starred;
    let rebuilder_for_star = rebuilder.clone();
    let on_changed_for_star = on_changed.clone();
    star_btn.connect_clicked(move |_| {
        app_for_star.with_db_mut(|db| {
            db.set_guided_file_starred(&uuid_for_star, new_starred)
        });
        if let Some(rb) = rebuilder_for_star.borrow().as_ref() {
            rb();
        }
        on_changed_for_star();
    });
    row.add_prefix(&star_btn);

    // Rename suffix.
    let rename_btn = gtk::Button::builder()
        .icon_name("document-edit-symbolic")
        .tooltip_text(gettext("Rename"))
        .css_classes(["flat", "circular"])
        .valign(gtk::Align::Center)
        .build();
    {
        let app = app.clone();
        let uuid = file.uuid.clone();
        let current_name = file.name.clone();
        let rebuilder = rebuilder.clone();
        let on_changed = on_changed.clone();
        let toast_overlay = toast_overlay.clone();
        let toast_slot = toast_slot.clone();
        rename_btn.connect_clicked(move |btn| {
            present_rename_dialog(
                btn,
                &app,
                &uuid,
                &current_name,
                rebuilder.clone(),
                on_changed.clone(),
                &toast_overlay,
                toast_slot.clone(),
            );
        });
    }
    row.add_suffix(&rename_btn);

    // Delete suffix.
    let delete_btn = gtk::Button::builder()
        .icon_name("user-trash-symbolic")
        .tooltip_text(gettext("Delete file"))
        .css_classes(["flat", "circular", "destructive-action"])
        .valign(gtk::Align::Center)
        .build();
    {
        let app = app.clone();
        let uuid = file.uuid.clone();
        let display_name = file.name.clone();
        let rebuilder = rebuilder.clone();
        let on_changed = on_changed.clone();
        let toast_overlay = toast_overlay.clone();
        let toast_slot = toast_slot.clone();
        delete_btn.connect_clicked(move |btn| {
            present_delete_dialog(
                btn,
                &app,
                &uuid,
                &display_name,
                rebuilder.clone(),
                on_changed.clone(),
                &toast_overlay,
                toast_slot.clone(),
            );
        });
    }
    row.add_suffix(&delete_btn);

    row
}

fn empty_state_row() -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .title(gettext("No guided files imported"))
        .subtitle(gettext("Tap the row above to add one"))
        .activatable(false)
        .selectable(false)
        .build();
    row.add_css_class("dim-label");
    row
}

fn present_rename_dialog(
    anchor: &gtk::Button,
    app: &MeditateApplication,
    uuid: &str,
    current_name: &str,
    rebuilder: Rc<RefCell<Option<Box<dyn Fn()>>>>,
    on_changed: impl Fn() + Clone + 'static,
    toast_overlay: &adw::ToastOverlay,
    toast_slot: ToastSlot,
) {
    let entry = gtk::Entry::builder()
        .text(current_name)
        .activates_default(true)
        .build();
    let collision_label = gtk::Label::builder()
        .label(gettext("A guided file with this name already exists."))
        .css_classes(["error", "caption"])
        .halign(gtk::Align::Start)
        .visible(false)
        .build();
    let form = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .build();
    form.append(&entry);
    form.append(&collision_label);

    let dialog = adw::AlertDialog::builder()
        .heading(gettext("Rename Guided File"))
        .extra_child(&form)
        .close_response("cancel")
        .default_response("rename")
        .build();
    dialog.add_response("cancel", &gettext("Cancel"));
    dialog.add_response("rename", &gettext("Rename"));
    dialog.set_response_appearance("rename", adw::ResponseAppearance::Suggested);
    dialog.set_response_enabled("rename", false);

    let validate: Rc<dyn Fn()> = {
        let app = app.clone();
        let entry = entry.clone();
        let dialog = dialog.clone();
        let collision_label = collision_label.clone();
        let uuid_for_validate = uuid.to_string();
        Rc::new(move || {
            let text = entry.text();
            let trimmed = text.trim();
            let collision = !trimmed.is_empty()
                && app
                    .with_db(|db| db.is_guided_file_name_taken(trimmed, &uuid_for_validate))
                    .and_then(|r| r.ok())
                    .unwrap_or(false);
            let valid = !trimmed.is_empty() && !collision;
            dialog.set_response_enabled("rename", valid);
            collision_label.set_visible(!trimmed.is_empty() && collision);
        })
    };
    validate();
    let validate_for_change = validate.clone();
    entry.connect_changed(move |_| validate_for_change());

    let app_for_response = app.clone();
    let uuid_for_response = uuid.to_string();
    let old_name = current_name.to_string();
    let entry_for_response = entry.clone();
    let toast_overlay_for_response = toast_overlay.clone();
    let toast_slot_for_response = toast_slot.clone();
    dialog.connect_response(None, move |_, id| {
        if id != "rename" {
            return;
        }
        let new_name = entry_for_response.text().trim().to_string();
        if new_name.is_empty() || new_name == old_name {
            return;
        }
        app_for_response.with_db_mut(|db| {
            db.rename_guided_file(&uuid_for_response, &new_name)
        });
        if let Some(rb) = rebuilder.borrow().as_ref() {
            rb();
        }
        on_changed.clone()();

        // Undo: rename back to old_name. Same DB call, fires another
        // guided_file_update event with the old name as the new value
        // — peers replay it correctly because the event log resolves
        // by lamport_ts (undo's event is later, so it wins).
        let app_for_undo = app_for_response.clone();
        let uuid_for_undo = uuid_for_response.clone();
        let old_name_for_undo = old_name.clone();
        let on_changed_for_undo = on_changed.clone();
        let rebuilder_for_undo = rebuilder.clone();
        push_undo_toast(
            &toast_overlay_for_response,
            &toast_slot_for_response,
            &gettext("File renamed"),
            move || {
                app_for_undo.with_db_mut(|db| {
                    db.rename_guided_file(&uuid_for_undo, &old_name_for_undo)
                });
                if let Some(rb) = rebuilder_for_undo.borrow().as_ref() {
                    rb();
                }
                on_changed_for_undo();
            },
            // Rename undo has no on-natural-dismiss work — no on-disk
            // file to clean up like the delete case does.
            || {},
        );
    });

    if let Some(root) = anchor.root() {
        if let Ok(window) = root.downcast::<gtk::Window>() {
            dialog.present(Some(&window));
            entry.grab_focus();
            entry.select_region(0, -1);
        }
    }
}

fn present_delete_dialog(
    anchor: &gtk::Button,
    app: &MeditateApplication,
    uuid: &str,
    display_name: &str,
    rebuilder: Rc<RefCell<Option<Box<dyn Fn()>>>>,
    on_changed: impl Fn() + Clone + 'static,
    toast_overlay: &adw::ToastOverlay,
    toast_slot: ToastSlot,
) {
    let dialog = adw::AlertDialog::builder()
        .heading(gettext("Delete Guided File?"))
        .body(format!(
            "{} \"{}\". {}",
            gettext("This will permanently remove"),
            display_name,
            gettext("Past sessions logged with this file stay in the log."),
        ))
        .close_response("cancel")
        .default_response("cancel")
        .build();
    dialog.add_response("cancel", &gettext("Cancel"));
    dialog.add_response("delete", &gettext("Delete"));
    dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);

    let app = app.clone();
    let uuid = uuid.to_string();
    let display_name_for_response = display_name.to_string();
    let toast_overlay_for_response = toast_overlay.clone();
    let toast_slot_for_response = toast_slot.clone();
    dialog.connect_response(None, move |_, id| {
        if id != "delete" {
            return;
        }
        // Capture the full row state BEFORE the DB delete so the Undo
        // path can re-insert it. Bail out if the row's already gone
        // (sync race between two devices, say).
        let Some(file) = app
            .with_db(|db| db.find_guided_file_by_uuid(&uuid))
            .and_then(|r| r.ok())
            .flatten()
        else {
            return;
        };

        app.with_db_mut(|db| db.delete_guided_file(&uuid));
        // The on-disk file is NOT removed here — that's deferred to
        // the toast's natural dismissal so Undo can restore the row
        // without needing to re-transcode. If the toast times out
        // or gets dismissed by a subsequent action, the on-dismiss
        // handler below cleans up.
        if let Some(rb) = rebuilder.borrow().as_ref() {
            rb();
        }
        on_changed.clone()();

        // Undo: re-insert with the same uuid + the captured fields.
        // emits a fresh guided_file_insert event whose lamport_ts is
        // strictly later than the prior delete event, so peers replay
        // both and recompute_guided_file resolves the row as live.
        let app_for_undo = app.clone();
        let file_for_undo = file.clone();
        let rebuilder_for_undo = rebuilder.clone();
        let on_changed_for_undo = on_changed.clone();

        // On natural dismiss (timeout or replacement, NOT undo): now
        // safe to delete the on-disk file. Best-effort — failures
        // (file already gone, permissions) get swallowed; a stale
        // row in past sessions resolves to a missing file, which
        // playback will toast about if the user tries to play it.
        let file_path_for_dismiss = file.file_path.clone();
        push_undo_toast(
            &toast_overlay_for_response,
            &toast_slot_for_response,
            &format!(
                "{} {}",
                gettext("Deleted"),
                ellipsize(&display_name_for_response, 28),
            ),
            move || {
                app_for_undo.with_db_mut(|db| {
                    db.insert_guided_file_with_uuid(
                        &file_for_undo.uuid,
                        &file_for_undo.name,
                        &file_for_undo.file_path,
                        file_for_undo.duration_secs,
                        file_for_undo.is_starred,
                    )
                });
                if let Some(rb) = rebuilder_for_undo.borrow().as_ref() {
                    rb();
                }
                on_changed_for_undo();
            },
            move || {
                let _ = std::fs::remove_file(&file_path_for_dismiss);
            },
        );
    });

    if let Some(root) = anchor.root() {
        if let Ok(window) = root.downcast::<gtk::Window>() {
            dialog.present(Some(&window));
        }
    }
}

// ── Helpers ──────────────────────────────────────────────────────────

/// Push an undo toast onto the chooser-local overlay. `on_undo` fires
/// when the user taps the Undo button. `on_dismiss_natural` fires on
/// any other dismissal (timeout, programmatic replacement by a newer
/// toast, manual close) — used by the delete path to clean up the
/// on-disk file once it's clear the user isn't going to undo.
fn push_undo_toast(
    toast_overlay: &adw::ToastOverlay,
    toast_slot: &ToastSlot,
    title: &str,
    on_undo: impl Fn() + 'static,
    on_dismiss_natural: impl Fn() + 'static,
) {
    let toast = build_undo_toast(toast_slot, title, on_undo, on_dismiss_natural);
    toast_overlay.add_toast(toast);
}

fn build_undo_toast(
    toast_slot: &ToastSlot,
    title: &str,
    on_undo: impl Fn() + 'static,
    on_dismiss_natural: impl Fn() + 'static,
) -> adw::Toast {
    // Replace any in-flight toast — its dismiss handler picks up the
    // change-of-slot and clears itself. The two-step (replace, then
    // dismiss) ordering avoids re-entering the dismiss callback
    // before the new toast has been installed.
    let prev = toast_slot.replace(None);
    if let Some(prev) = prev {
        prev.dismiss();
    }

    let toast = adw::Toast::builder()
        .title(title)
        .button_label(gettext("Undo"))
        .build();

    // Track whether Undo was invoked so connect_dismissed can decide
    // whether to fire the on-dismiss-natural cleanup. Without this
    // flag, dismissing the toast post-undo would re-run the cleanup
    // path (e.g. delete the file we just restored).
    let undone: Rc<Cell<bool>> = Rc::new(Cell::new(false));
    let undone_for_btn = undone.clone();
    let on_undo = Rc::new(on_undo);
    toast.connect_button_clicked(move |_| {
        undone_for_btn.set(true);
        on_undo();
    });

    let toast_slot_dismiss = toast_slot.clone();
    let on_dismiss_natural = Rc::new(on_dismiss_natural);
    toast.connect_dismissed(move |t| {
        if !undone.get() {
            on_dismiss_natural();
        }
        // Clear the slot iff the toast that's dismissing IS the one
        // currently in the slot. A newer toast that replaced this
        // one has already taken the slot — don't clobber it.
        let should_clear = toast_slot_dismiss
            .borrow()
            .as_ref()
            .map(|cur| cur == t)
            .unwrap_or(false);
        if should_clear {
            toast_slot_dismiss.replace(None);
        }
    });
    toast_slot.replace(Some(toast.clone()));
    toast
}

/// Truncate a string to `max_chars` Unicode scalar values, appending
/// "…" if truncation happened. Used by the toast titles so a long
/// guided-file name doesn't push the Undo button off-screen on phone.
fn ellipsize(s: &str, max_chars: usize) -> String {
    let count = s.chars().count();
    if count <= max_chars {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max_chars.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

pub fn format_duration_brief(secs: u32) -> String {
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

fn window_from(widget: &impl glib::object::IsA<gtk::Widget>) -> Option<crate::window::MeditateWindow> {
    widget
        .root()
        .and_then(|r| r.downcast::<crate::window::MeditateWindow>().ok())
}

fn add_toast_to_window(window: &gtk::Window, msg: &str) {
    if let Ok(mw) = window.clone().downcast::<crate::window::MeditateWindow>() {
        mw.add_toast(adw::Toast::builder().title(msg).timeout(4).build());
    }
}

// ── Playback (phase 6) ───────────────────────────────────────────────

/// A live gst playbin instance + its bus watch guard. The timer view
/// holds one of these in a RefCell across the running session and
/// tears it down on stop / save / discard / reset. Drop runs the
/// pipeline's set_state(Null) AND the watch guard's drop (which
/// removes the bus watch).
#[derive(Debug)]
pub struct GuidedPlayback {
    pipeline: gstreamer::Pipeline,
    /// Holds the bus watch alive — drops on the same teardown path.
    /// Stored in an Option so we never need to read it back; named
    /// underscore-prefix to silence the unused-field warning.
    _watch: gstreamer::bus::BusWatchGuard,
}

impl GuidedPlayback {
    /// Build a fresh playbin pointed at `path` and set its state to
    /// Playing. Returns Err with a human-readable string for any gst
    /// failure (init, element creation, file URI bad, state change).
    /// `on_eos` fires on the GTK main thread when the playback hits
    /// end-of-stream — used by the timer view to slide into Overtime
    /// in case the audio happens to end slightly earlier than the
    /// duration we probed at import time.
    pub fn start(
        path: &Path,
        on_eos: impl Fn() + 'static,
    ) -> Result<Self, String> {
        use gst::prelude::*;
        use gstreamer as gst;

        gst::init().map_err(|e| format!("gst init failed: {e}"))?;

        let abs = path
            .canonicalize()
            .map_err(|e| format!("canonicalize {}: {e}", path.display()))?;
        let uri = format!("file://{}", abs.to_string_lossy());

        // playbin handles full audio decode + render through the
        // platform's autoaudiosink. Same element used in the duration
        // probe — same compatibility envelope.
        let playbin = gst::ElementFactory::make("playbin")
            .property("uri", &uri)
            .build()
            .map_err(|e| format!("create playbin: {e}"))?;

        // Wrap in a Pipeline so the bus is reachable and start/stop
        // semantics are explicit. playbin IS already a pipeline-like
        // bin internally, but exposing it through gst::Pipeline gives
        // a cleaner state-management surface.
        let pipeline = gst::Pipeline::new();
        pipeline
            .add(&playbin)
            .map_err(|e| format!("add playbin to pipeline: {e}"))?;

        // `add_watch_local` (vs `add_watch` / `connect_message`) takes
        // a !Send closure and dispatches on the thread-local main
        // context — the right shape when the closure body touches
        // GTK objects (which are themselves !Send). The returned
        // BusWatchGuard removes the watch on Drop.
        let bus = pipeline
            .bus()
            .ok_or_else(|| "pipeline missing bus".to_string())?;
        let on_eos = Rc::new(on_eos);
        let watch = bus
            .add_watch_local(move |_, msg| {
                use gst::MessageView;
                match msg.view() {
                    MessageView::Eos(_) => on_eos(),
                    MessageView::Error(err) => {
                        crate::diag::log(&format!(
                            "guided playback error: {} ({})",
                            err.error(),
                            err.debug().unwrap_or_default()
                        ));
                    }
                    _ => {}
                }
                glib::ControlFlow::Continue
            })
            .map_err(|e| format!("add_watch_local: {e}"))?;

        pipeline
            .set_state(gst::State::Playing)
            .map_err(|e| format!("set state Playing: {e}"))?;

        Ok(Self {
            pipeline,
            _watch: watch,
        })
    }

    /// Pause playback in place. Position freezes; resume() picks up
    /// at the same offset. No-op if the pipeline already isn't in a
    /// playing state.
    pub fn pause(&self) {
        use gst::prelude::*;
        use gstreamer as gst;
        let _ = self.pipeline.set_state(gst::State::Paused);
    }

    /// Resume playback from the position where pause() left off.
    pub fn resume(&self) {
        use gst::prelude::*;
        use gstreamer as gst;
        let _ = self.pipeline.set_state(gst::State::Playing);
    }
}

impl Drop for GuidedPlayback {
    fn drop(&mut self) {
        use gst::prelude::*;
        use gstreamer as gst;
        let _ = self.pipeline.set_state(gst::State::Null);
        // The BusWatchGuard's own Drop removes the bus watch.
    }
}

// ── Duration probe (phase 3) ─────────────────────────────────────────

/// Probe the duration of an audio file in seconds, using a paused
/// gstreamer `playbin` pipeline. Synchronous — for typical guided-
/// meditation files this returns within a few hundred milliseconds.
///
/// Why playbin instead of pbutils' Discoverer: pbutils requires the
/// `gstreamer-plugins-base-dev` system package to build against,
/// which isn't part of Debian's `libgstreamer-plugins-base1.0-0`
/// runtime metadata package. Using only the core `gstreamer` crate
/// avoids the extra dev-package dependency.
pub fn probe_duration_secs(path: &Path) -> Result<u32, String> {
    use gst::prelude::*;
    use gstreamer as gst;

    gst::init().map_err(|e| format!("gst init failed: {e}"))?;

    let abs = path
        .canonicalize()
        .map_err(|e| format!("canonicalize {}: {e}", path.display()))?;
    let uri = format!("file://{}", abs.to_string_lossy());

    let pipeline = gst::ElementFactory::make("playbin")
        .property("uri", &uri)
        .build()
        .map_err(|e| format!("create playbin: {e}"))?;
    let audio_sink = gst::ElementFactory::make("fakesink")
        .property("sync", false)
        .build()
        .map_err(|e| format!("create audio fakesink: {e}"))?;
    let video_sink = gst::ElementFactory::make("fakesink")
        .property("sync", false)
        .build()
        .map_err(|e| format!("create video fakesink: {e}"))?;
    pipeline.set_property("audio-sink", &audio_sink);
    pipeline.set_property("video-sink", &video_sink);

    pipeline
        .set_state(gst::State::Paused)
        .map_err(|e| format!("set state Paused: {e}"))?;

    let timeout = gst::ClockTime::from_seconds(5);
    let (state_change, _, _) = pipeline.state(timeout);
    state_change.map_err(|e| format!("waiting for Paused: {e}"))?;

    if let Some(bus) = pipeline.bus() {
        while let Some(msg) = bus.pop_filtered(&[gst::MessageType::Error]) {
            use gst::MessageView::Error;
            if let Error(err) = msg.view() {
                let _ = pipeline.set_state(gst::State::Null);
                return Err(format!(
                    "{} ({})",
                    err.error(),
                    err.debug().unwrap_or_default()
                ));
            }
        }
    }

    let duration: Option<gst::ClockTime> = pipeline.query_duration();
    let _ = pipeline.set_state(gst::State::Null);

    let nanos = duration
        .ok_or_else(|| format!("duration unknown for {}", path.display()))?
        .nseconds();
    Ok((nanos.div_ceil(1_000_000_000)) as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_duration_secs_returns_close_to_known_bell_length() {
        let p = Path::new("data/sounds/bell.ogg");
        if !p.exists() {
            eprintln!("skipping: data/sounds/bell.ogg not present");
            return;
        }
        let secs = probe_duration_secs(p).expect("probe should succeed");
        assert!(
            (9..=11).contains(&secs),
            "bell.ogg ≈ 9.83 s — probe returned {secs}",
        );
    }

    #[test]
    fn format_duration_brief_under_one_hour_is_m_ss() {
        assert_eq!(format_duration_brief(0), "0:00");
        assert_eq!(format_duration_brief(7), "0:07");
        assert_eq!(format_duration_brief(60), "1:00");
        assert_eq!(format_duration_brief(150), "2:30");
        assert_eq!(format_duration_brief(59 * 60 + 59), "59:59");
    }

    #[test]
    fn format_duration_brief_over_one_hour_is_h_mm_ss() {
        assert_eq!(format_duration_brief(3600), "1:00:00");
        assert_eq!(format_duration_brief(3661), "1:01:01");
        assert_eq!(format_duration_brief(2 * 3600 + 5 * 60 + 9), "2:05:09");
    }
}
