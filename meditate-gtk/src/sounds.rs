//! Bell-sound chooser — the NavigationPage pushed when the user taps
//! a bell-sound row in the timer setup (Starting Bell sound, per-
//! interval-bell sound, Completion Sound). Lists every row in the
//! `bell_sounds` library (bundled + custom) with a per-row Play
//! button preview. Tapping a row body picks that sound and pops the
//! page; the caller's `on_selected` callback receives the chosen
//! UUID.
//!
//! B.4.5 reuses the same module's row builder for the Preferences
//! tab in management mode (no selection, delete + rename).

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;

use crate::application::MeditateApplication;
use crate::db::{write_ok, Failed, BellSound, BellSoundCategory};
use crate::i18n::gettext;
use crate::name_dialog::{name_dialog, taken_in, NameRow};
use meditate_core::sound::AudioFileError;

/// Which sound is currently previewing + the play-button widget
/// showing the Stop icon. Mirrors the `PreviewState` in `vibrations.rs`
/// so both choosers share the same toggle / supersede / auto-revert
/// protocol. The portable state machine (which uuid is active, the
/// auto-revert generation counter) lives in
/// `meditate_core::sound::PreviewToggle`; this struct adds the
/// shell-only "which gtk::Button is the active Stop icon" handle so
/// we know which row to revert.
#[derive(Default)]
struct PreviewState {
    toggle: meditate_core::sound::PreviewToggle,
    active_btn: Option<gtk::Button>,
    /// The volume of the bell the chooser is open for: previews play
    /// at it.
    volume: meditate_core::bell_volume::BellVolume,
}

/// Push the bell-sound chooser onto the navigation view in selection
/// mode. `current_uuid` is the row to mark with a checkmark when
/// the page opens — pass `None` for "nothing selected yet". The
/// `on_selected` callback fires when the user taps a row body and
/// receives the chosen UUID; the page pops automatically right
/// after.
pub fn push_sounds_chooser(
    nav_view: &adw::NavigationView,
    app: &MeditateApplication,
    category: BellSoundCategory,
    current_uuid: Option<String>,
    volume: meditate_core::bell_volume::BellVolume,
    on_selected: impl Fn(String) + 'static,
) {
    let group = adw::PreferencesGroup::new();
    let prefs_page = adw::PreferencesPage::new();
    prefs_page.add(&group);

    let header = adw::HeaderBar::builder().show_back_button(true).build();
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    // Its own overlay: a failed save must show on this page.
    let toast_overlay = adw::ToastOverlay::new();
    toast_overlay.set_child(Some(&prefs_page));
    toolbar.set_content(Some(&toast_overlay));

    let page = adw::NavigationPage::builder()
        .tag("bell-sounds-chooser")
        .title(gettext("Choose Bell Sound"))
        .child(&toolbar)
        .build();

    let on_selected = Rc::new(on_selected);
    let nav_view_clone = nav_view.clone();

    // Track every row we add so a rebuild can drain them by ref.
    // Adw.PreferencesGroup wraps its rows inside an internal GtkBox,
    // so group.first_child() returns the wrapper — iterating that
    // would spin forever (and yell "tried to remove non-child" at
    // every loop). Holding row refs ourselves bypasses the wrapper
    // entirely.
    let rows: Rc<std::cell::RefCell<Vec<gtk::Widget>>> =
        Rc::new(std::cell::RefCell::new(Vec::new()));

    let rebuilder: crate::Rebuilder =
        Rc::new(std::cell::RefCell::new(None));

    let group_for_rb = group.clone();
    let rows_for_rb = rows.clone();
    let app_for_rb = app.clone();
    let nav_view_for_rb = nav_view_clone.clone();
    let current_uuid_for_rb = current_uuid.clone();
    let on_selected_for_rb = on_selected.clone();
    let rebuilder_for_self = rebuilder.clone();
    *rebuilder.borrow_mut() = Some(Box::new(move || {
        rebuild_chooser_rows(
            &group_for_rb,
            &rows_for_rb,
            &app_for_rb,
            category,
            current_uuid_for_rb.as_deref(),
            volume,
            &nav_view_for_rb,
            on_selected_for_rb.clone(),
            rebuilder_for_self.clone(),
        );
    }));

    if let Some(rb) = rebuilder.borrow().as_ref() {
        rb();
    }

    // Stop any in-flight preview when the user pops the page so a
    // bell doesn't keep ringing through the next setup screen.
    page.connect_hidden(move |_| crate::sound::stop_preview());

    nav_view.push(&page);
}

/// Drain every previously-added row from the chooser's group, then
/// rebuild from the current bell_sounds library state. The synthetic
/// "Choose your own…" row goes back at the top.
fn rebuild_chooser_rows(
    group: &adw::PreferencesGroup,
    rows: &Rc<std::cell::RefCell<Vec<gtk::Widget>>>,
    app: &MeditateApplication,
    category: BellSoundCategory,
    current_uuid: Option<&str>,
    volume: meditate_core::bell_volume::BellVolume,
    nav_view: &adw::NavigationView,
    on_selected: Rc<dyn Fn(String)>,
    rebuilder: crate::Rebuilder,
) {
    for row in rows.borrow_mut().drain(..) {
        group.remove(&row);
    }

    // "Choose your own…" — synthetic, always at the top, opens a
    // file picker. The closure borrows the same rebuilder so a
    // successful import re-renders the list with the new row. The
    // chooser's category flows through to the insert call so an
    // import from a Box Breath phase chooser auto-tags BoxBreath.
    let import_row = adw::ActionRow::builder()
        .title(gettext("Choose your own…"))
        .activatable(true)
        .build();
    let chooser_arrow = gtk::Image::from_icon_name("document-open-symbolic");
    chooser_arrow.add_css_class("dim-label");
    import_row.add_suffix(&chooser_arrow);
    let app_for_import = app.clone();
    let rebuilder_for_import = rebuilder.clone();
    import_row.connect_activated(move |row| {
        let rebuilder = rebuilder_for_import.clone();
        present_file_picker(
            row,
            &app_for_import,
            category,
            Box::new(move || {
                if let Some(rb) = rebuilder.borrow().as_ref() {
                    rb();
                }
            }),
        );
    });
    group.add(&import_row);
    rows.borrow_mut().push(import_row.upcast());

    let selection = SelectionContext {
        current_uuid: current_uuid.map(std::string::ToString::to_string),
        on_selected,
        nav_view: nav_view.clone(),
        preview: Rc::new(RefCell::new(PreviewState { volume, ..PreviewState::default() })),
    };

    let sounds = app
        .with_db(|db| db.list_bell_sounds_for_category(category))
        .and_then(std::result::Result::ok)
        .unwrap_or_default();
    if sounds.is_empty() && category == BellSoundCategory::BoxBreath {
        // Phase chooser starts empty for new users until the
        // bundled voice cues land (TODO: "Source bundled Box
        // Breath voice-cue sounds"). Show a hint pointing at the
        // import affordance directly above.
        let empty = adw::ActionRow::builder()
            .title(gettext("No sounds for this category yet"))
            .subtitle(gettext("Tap \"Choose your own…\" above to import one"))
            .activatable(false)
            .selectable(false)
            .build();
        empty.add_css_class("dim-label");
        group.add(&empty);
        rows.borrow_mut().push(empty.upcast());
        return;
    }
    for sound in sounds {
        let row = build_sound_row(&sound, app, rebuilder.clone(), &selection);
        group.add(&row);
        rows.borrow_mut().push(row.upcast());
    }
}

/// Selection-mode parameters for the unified row builder. The
/// chooser tap-picks the row's uuid via `on_selected` and pops the
/// nav view; `current_uuid` is the entry to mark with a checkmark.
pub struct SelectionContext {
    pub current_uuid: Option<String>,
    pub on_selected: Rc<dyn Fn(String)>,
    pub nav_view: adw::NavigationView,
    /// Shared across all rows so tapping a different row's Play
    /// supersedes the previous (single playback slot, single Stop
    /// icon at any time).
    preview: Rc<RefCell<PreviewState>>,
}

/// Build a sound-library row for the chooser: tap-to-pick body
/// plus per-row Play/Stop preview, Rename, and (for non-bundled
/// rows) Delete.
fn build_sound_row(
    sound: &BellSound,
    app: &MeditateApplication,
    rebuilder: crate::Rebuilder,
    selection: &SelectionContext,
) -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .title(&sound.name)
        .subtitle(if sound.is_bundled {
            gettext("Bundled")
        } else {
            gettext("Custom")
        })
        .activatable(true)
        .build();

    // Checkmark for the currently-selected row. Lives left of the
    // buttons because suffix order is left→right on the right side,
    // so adding it first lands it adjacent to the title block.
    if selection.current_uuid.as_deref() == Some(sound.uuid.as_str()) {
        let check = gtk::Image::from_icon_name("object-select-symbolic");
        check.add_css_class("selected-check");
        row.add_suffix(&check);
    }

    add_play_button(&row, sound, selection.preview.clone());
    add_rename_button(&row, sound, app, rebuilder.clone());
    if !sound.is_bundled {
        // Bundled rows stay permanent — the seed re-creates them on
        // every open anyway, and an accidental tombstone could
        // confuse a peer.
        add_delete_button(&row, sound, app, rebuilder);
    }

    let uuid = sound.uuid.0.clone();
    let on_selected = selection.on_selected.clone();
    let nav_view = selection.nav_view.clone();
    row.connect_activated(move |_| {
        on_selected(uuid.clone());
        nav_view.pop();
    });
    row
}

fn add_play_button(
    row: &adw::ActionRow,
    sound: &BellSound,
    preview: Rc<RefCell<PreviewState>>,
) {
    // Per-row preview button. Toggle / supersede / auto-revert
    // protocol via the shared `meditate_core::sound::PreviewToggle`:
    //   - Tap while idle → start playback, this row's icon flips to
    //     Stop, recorded as `active_btn`.
    //   - Tap while playing → toggle off, this row's icon flips
    //     back to Play.
    //   - Tap a different row's button while one is playing →
    //     `active_btn` (the previous row's button) reverts first;
    //     this row's Play starts and becomes the new `active_btn`.
    //   - MediaFile transitions to not-playing (natural end / user
    //     stop / supersede) → `timer_should_revert(gen)` only
    //     returns true for the natural-end case (no other action
    //     has bumped the generation), and the icon flips back.
    let play_btn = gtk::Button::builder()
        .icon_name("media-playback-start-symbolic")
        .tooltip_text(gettext("Preview sound"))
        .css_classes(["flat", "circular"])
        .valign(gtk::Align::Center)
        .build();
    let sound_clone = sound.clone();
    let preview_for_click = preview.clone();
    let btn_for_click = play_btn.clone();
    play_btn.connect_clicked(move |_| {
        use meditate_core::sound::PreviewAction;
        let action = preview_for_click
            .borrow_mut()
            .toggle
            .request(sound_clone.uuid.as_str());
        // The previous Stop-icon button (if any) always reverts
        // first — either we're toggling it off, or switching to a
        // different row.
        if let Some(prev_btn) = preview_for_click.borrow_mut().active_btn.take() {
            prev_btn.set_icon_name("media-playback-start-symbolic");
            prev_btn.set_tooltip_text(Some(&gettext("Preview sound")));
        }
        match action {
            PreviewAction::StopOnly => {
                crate::sound::stop_preview();
            }
            PreviewAction::StopAndStart { generation, .. } => {
                let volume = preview_for_click.borrow().volume;
                let media = crate::sound::play_preview(&sound_clone, volume);
                btn_for_click.set_icon_name("media-playback-stop-symbolic");
                btn_for_click.set_tooltip_text(Some(&gettext("Stop preview")));
                preview_for_click.borrow_mut().active_btn = Some(btn_for_click.clone());

                // Auto-revert when this play's MediaFile reaches
                // not-playing. `timer_should_revert` no-ops if the
                // user already tapped Stop / a different row in the
                // meantime — generation will have advanced past
                // `generation` and the call returns false.
                let preview_for_notify = preview_for_click.clone();
                media.connect_notify_local(Some("playing"), move |m, _| {
                    if m.is_playing() {
                        return;
                    }
                    let mut state = preview_for_notify.borrow_mut();
                    if state.toggle.timer_should_revert(generation) {
                        if let Some(btn) = state.active_btn.take() {
                            btn.set_icon_name("media-playback-start-symbolic");
                            btn.set_tooltip_text(Some(&gettext("Preview sound")));
                        }
                    }
                });
            }
            PreviewAction::NoOp => {}
        }
    });
    row.add_suffix(&play_btn);
}

fn add_rename_button(
    row: &adw::ActionRow,
    sound: &BellSound,
    app: &MeditateApplication,
    rebuilder: crate::Rebuilder,
) {
    let rename_btn = gtk::Button::builder()
        .icon_name("document-edit-symbolic")
        .tooltip_text(gettext("Rename"))
        .css_classes(["flat", "circular"])
        .valign(gtk::Align::Center)
        .build();
    let app = app.clone();
    let uuid = sound.uuid.0.clone();
    let row_clone = row.clone();
    rename_btn.connect_clicked(move |btn| {
        present_rename_dialog(btn, &app, &uuid, &row_clone.title(), rebuilder.clone());
    });
    row.add_suffix(&rename_btn);
}

fn add_delete_button(
    row: &adw::ActionRow,
    sound: &BellSound,
    app: &MeditateApplication,
    rebuilder: crate::Rebuilder,
) {
    let delete_btn = gtk::Button::builder()
        .icon_name("user-trash-symbolic")
        .tooltip_text(gettext("Delete sound"))
        .css_classes(["flat", "circular", "destructive-action"])
        .valign(gtk::Align::Center)
        .build();
    let app = app.clone();
    let uuid = sound.uuid.0.clone();
    delete_btn.connect_clicked(move |btn| {
        present_delete_dialog(btn, &app, &uuid, rebuilder.clone());
    });
    row.add_suffix(&delete_btn);
}

// 10 MB cap matches the locked B.5 spec — same number is enforced
// on the inbound sync side in B.6 so a peer can't push a file bigger
// than what the local UI would accept. Source of truth lives in
// `meditate_core::sound::MAX_CUSTOM_BELL_BYTES` so the Android shell
// + the sync gate read the same constant.

/// Open a file picker, validate the chosen file, and (on confirm)
/// import it into the bell-sound library. Calls `on_imported` after
/// a successful import so the caller can rebuild its list.
fn present_file_picker(
    anchor: &adw::ActionRow,
    app: &MeditateApplication,
    category: BellSoundCategory,
    on_imported: Box<dyn Fn()>,
) {
    let file_dialog = gtk::FileDialog::builder()
        .title(gettext("Choose Sound File"))
        .build();

    file_dialog.set_default_filter(Some(&audio_file_filter()));

    let parent = anchor
        .root()
        .and_then(|r| r.downcast::<gtk::Window>().ok());
    let on_imported = Rc::new(on_imported);
    let app = app.clone();
    let anchor = anchor.clone();
    file_dialog.open(
        parent.as_ref(),
        None::<&gtk::gio::Cancellable>,
        move |result| {
            let Ok(file) = result else { return; };
            let Some(path) = file.path() else { return; };

            // Size cap.
            let size = std::fs::metadata(&path).map_or(0, |m| m.len());
            if !meditate_core::sound::is_within_size_limit(size) {
                crate::db::show_toast(&anchor, gettext("File is larger than 10 MB"));
                return;
            }
            // Refuse a file without an audio stream (e.g. a video-only
            // mp4) before the name dialog. Any other probe failure is
            // left for the import itself to report, as before.
            if crate::guided::probe_duration_secs(&path)
                == Err(AudioFileError::NoAudioTrack)
            {
                crate::db::show_toast(&anchor, gettext("This file has no audio track"));
                return;
            }

            present_import_confirm_dialog(
                &anchor,
                &app,
                category,
                &path,
                on_imported.clone(),
            );
        },
    );
}

/// File-dialog filter for every importable audio format. Suffix
/// rules are always case-insensitive (`.MP4`, `.Mp4` …), including
/// through the file-chooser portal. Shared by the bell and guided
/// pickers.
pub(crate) fn audio_file_filter() -> gtk::FileFilter {
    let filter = gtk::FileFilter::new();
    filter.set_name(Some(&gettext("Audio files")));
    for ext in meditate_core::sound::IMPORTABLE_EXTENSIONS {
        filter.add_suffix(ext);
    }
    filter
}


fn present_import_confirm_dialog(
    anchor: &adw::ActionRow,
    app: &MeditateApplication,
    category: BellSoundCategory,
    source_path: &std::path::Path,
    on_imported: Rc<Box<dyn Fn()>>,
) {
    let stem = meditate_core::sound::display_name_from_path(source_path);
    let filename = source_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();

    // Import is a custom button (not an AdwAlertDialog response) so
    // the dialog stays open while the worker thread transcodes —
    // responses auto-dismiss on dispatch and can't host a "loading"
    // state. With Import as a regular gtk::Button we can swap its
    // child for a spinner while the file IO is in flight, then
    // force_close the dialog when the worker reports back.
    let import_btn = gtk::Button::builder()
        .label(gettext("Import"))
        .css_classes(["suggested-action"])
        .hexpand(true)
        .build();

    // The shared name row: its hint says why Import is off, next to
    // the greyed button (tooltips don't reach touch users on the
    // Librem). One name set over both categories.
    let btn_for_row = import_btn.downgrade();
    let row = NameRow::new(
        &stem,
        taken_in(app, |db, name| meditate_core::db::is_bell_sound_name_taken_from_db(db, name, "")),
        move |savable| {
            if let Some(btn) = btn_for_row.upgrade() {
                btn.set_sensitive(savable);
            }
        },
    );

    // Wider gap (18px) between the form and Import so
    // import_btn↔Cancel and entry↔import_btn feel symmetric (Cancel
    // sits in AdwAlertDialog's own response row below extra_child).
    let extra_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(18)
        .build();
    extra_box.append(&row.widget);
    extra_box.append(&import_btn);

    let dialog = adw::AlertDialog::builder()
        .heading(gettext("Import Sound"))
        .body(gettext("Importing: {name}").replace("{name}", &filename))
        .extra_child(&extra_box)
        .close_response("cancel")
        .default_response("cancel")
        .build();
    dialog.add_response("cancel", &gettext("Cancel"));

    // Enter on the entry triggers Import (replaces the
    // activates_default behavior we lost when Import left the
    // response row).
    let import_btn_for_enter = import_btn.clone();
    row.entry.connect_activate(move |_| {
        if import_btn_for_enter.is_sensitive() {
            import_btn_for_enter.emit_clicked();
        }
    });

    // (uuid, mime) of a converted file whose store failed.
    let converted: crate::name_dialog::PendingImport<(String, &'static str)> = crate::name_dialog::pending_import(&dialog);

    // Store the converted file under the typed name: the dialog closes
    // with it, or stays with the name and says why (#428).
    let store: Rc<dyn Fn(String, std::path::PathBuf, &'static str)> = {
        let app = app.clone();
        let dialog = dialog.downgrade();
        let row = row.clone();
        let import_btn = import_btn.clone();
        let converted = converted.clone();
        Rc::new(move |uuid, path, mime| {
            let Some(dialog) = dialog.upgrade() else { return };
            let stored = app.with_db(|db| {
                db.import_bell_sound(&uuid, &row.name(), &path.to_string_lossy(), mime, category).map(drop)
            });
            if matches!(stored, Some(Ok(()))) {
                dialog.force_close();
                on_imported();
                return;
            }
            dialog.set_can_close(true);
            dialog.set_response_enabled("cancel", true);
            row.import_failed(&import_btn, "sound.import", stored);
            *converted.borrow_mut() = Some(((uuid, mime), path));
        })
    };

    let source_for_click = source_path.to_path_buf();
    let row_for_click = row.clone();
    let dialog_for_click = dialog.downgrade();
    let anchor_for_click = anchor.clone();
    import_btn.connect_clicked(move |btn| {
        let Some(dialog) = dialog_for_click.upgrade() else { return };
        // Converted already, its store failed: store it again.
        let pending = converted.borrow_mut().take();
        if let Some(((uuid, mime), path)) = pending {
            store(uuid, path, mime);
            return;
        }

        // Lock the dialog while the worker runs — Cancel disabled,
        // entry frozen, button greyed and showing a spinner.
        // can_close=false suppresses Escape too. The only exit
        // path is the spawn_local closure's force_close().
        dialog.set_can_close(false);
        dialog.set_response_enabled("cancel", false);
        row_for_click.entry.set_sensitive(false);
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

        // Off-thread: copy or transcode the file. !Send GTK widgets
        // stay on the main thread inside the spawn_local closure;
        // only the source PathBuf crosses into spawn_blocking.
        let source = source_for_click.clone();
        let anchor = anchor_for_click.clone();
        let store = store.clone();
        glib::MainContext::default().spawn_local(async move {
            let import_result = gtk::gio::spawn_blocking(move || {
                do_import_io(&source)
            }).await;

            match import_result {
                Ok(Ok((new_uuid, dest_path, mime))) => return store(new_uuid, dest_path, mime),
                Ok(Err(AudioFileError::NoAudioTrack)) => {
                    crate::db::show_toast(&anchor, gettext("This file has no audio track"));
                }
                Ok(Err(AudioFileError::TooLargeToSync)) => crate::db::show_toast(&anchor, crate::guided::too_large_to_sync()),
                Ok(Err(AudioFileError::Other(e))) => crate::db::show_toast(&anchor, gettext("Import failed: {error}").replace("{error}", &e)),
                Err(_) => crate::db::show_toast(&anchor, gettext("Import worker died")),
            }
            // A failed conversion used up its pick: a retry starts anew.
            dialog.force_close();
        });
    });

    dialog.present(Some(anchor));
    row.entry.grab_focus();
}

/// Worker-thread half of the custom-sound import. Copies or
/// transcodes the source into $XDG_DATA_HOME/meditate/sounds/
/// <uuid>.<ext> and returns the generated UUID, dest path, and
/// mime type so the caller can insert the bell_sounds row on the
/// main thread (Database is !Send). Anything that isn't already
/// WAV or OGG gets transcoded to OGG/Vorbis to dodge the gst 1.26.x
/// decodebin3 assertion-fail (see `transcode_to_ogg`).
fn do_import_io(
    source: &std::path::Path,
) -> std::result::Result<(String, std::path::PathBuf, &'static str), AudioFileError> {
    let source_ext = source
        .extension()
        .and_then(|s| s.to_str()).map_or_else(|| "wav".to_string(), str::to_ascii_lowercase);

    // wav and ogg pass through; everything else transcodes to
    // OGG/Vorbis on import. Mapping lives in core so the Android
    // shell's importer shares it.
    let (dest_ext, mime) = meditate_core::sound::target_extension_and_mime(&source_ext);

    let new_uuid = crate::db::mint_uuid();
    let dest_dir = gtk::glib::user_data_dir()
        .join("meditate")
        .join("sounds");
    std::fs::create_dir_all(&dest_dir).map_err(|e| e.to_string())?;
    let dest_path = dest_dir.join(format!("{new_uuid}.{dest_ext}"));

    if dest_ext == source_ext.as_str() {
        // Don't use std::fs::copy — it follows symlinks at the
        // destination, which would let a pre-planted link at
        // dest_path silently overwrite an arbitrary file. The core
        // helper uses O_CREAT|O_EXCL plus O_NOFOLLOW to refuse a
        // pre-existing or symlinked destination.
        meditate_core::sound::safe_copy_no_follow(source, &dest_path)
            .map_err(|e| e.to_string())?;
    } else if let Err(e) = transcode_to_ogg(source, &dest_path) {
        let _ = std::fs::remove_file(&dest_path);
        return Err(e);
    }

    Ok((new_uuid, dest_path, mime))
}

/// Build a one-shot gst pipeline that decodes `source` (any format
/// gst can read), re-encodes it to OGG/Vorbis at quality 0.4, and
/// writes the result to `dest`. Runs synchronously on the calling
/// thread — for ~10 MB inputs this is a few seconds even on the
/// Librem 5, which is acceptable for a one-time import flow.
///
/// Pipeline:
///   filesrc ! decodebin ! audioconvert ! audioresample !
///   vorbisenc quality=0.4 ! oggmux ! filesink
///
/// Crucially uses `decodebin` (legacy), not `decodebin3`. The newer
/// element has a known race in `mq_slot_handle_stream_start` that
/// aborts the process on certain MP3 streams (gst 1.26.x). Since
/// `vorbisenc` + `oggmux` produce a clean single-stream OGG, the
/// transcoded file then plays back through gtk::MediaFile without
/// touching the buggy code path.
fn transcode_to_ogg(
    source: &std::path::Path,
    dest: &std::path::Path,
) -> std::result::Result<(), AudioFileError> {
    use gstreamer as gst;
    use gst::prelude::*;

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
    let decodebin = make("decodebin")?;
    let audioconvert = make("audioconvert")?;
    // EBU R128 loudness normalisation — same -16 LUFS / 11 LU range /
    // -1.5 dBTP target the bundled set is encoded at, so an imported
    // metal singing bowl can't end up twice as loud as a bundled
    // woodblock click. Optional: the element ships in gst-plugins-bad
    // and is present in the freedesktop-sdk flatpak runtime we deploy
    // to, but Debian Trixie's 1.26.2 packages plugins-bad without it.
    // Falling through silently keeps `cargo run` working on the dev
    // laptop; the production import path on the phone gets the
    // normalisation.
    let audioloudnorm = gst::ElementFactory::make("audioloudnorm")
        .property("loudness-target", -16.0_f64)
        .property("loudness-range-target", 11.0_f64)
        .property("true-peak-target", -1.5_f64)
        .build()
        .ok();
    if audioloudnorm.is_none() {
        meditate_core::log(
            "transcode.to_ogg",
            "audioloudnorm element not registered — skipping loudness-normalisation step",
        );
    }
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

    // Build the post-decodebin chain dynamically so the pipeline
    // is identical whether or not audioloudnorm is in the registry.
    let mut chain: Vec<&gst::Element> = vec![&audioconvert];
    if let Some(ln) = audioloudnorm.as_ref() {
        chain.push(ln);
    }
    chain.extend([&audioresample, &vorbisenc, &oggmux, &filesink]);

    pipeline
        .add(&filesrc).map_err(|e| e.to_string())?;
    pipeline
        .add(&decodebin).map_err(|e| e.to_string())?;
    for el in &chain {
        pipeline.add(*el).map_err(|e| e.to_string())?;
    }
    filesrc.link(&decodebin).map_err(|e| e.to_string())?;
    gst::Element::link_many(&chain).map_err(|e| e.to_string())?;

    // decodebin produces its source pad lazily once the input is
    // typefind-ed, so we link it to audioconvert's sink in pad-added.
    connect_audio_pads(&decodebin, &audioconvert)?;

    pipeline
        .set_state(gst::State::Playing)
        .map_err(|e| e.to_string())?;

    let bus = pipeline
        .bus()
        .ok_or("pipeline missing bus".to_string())?;
    let mut transcode_err: Option<AudioFileError> = None;
    for msg in bus.iter_timed(gst::ClockTime::NONE) {
        use gst::MessageView::*;
        if is_no_audio_message(&msg) {
            transcode_err = Some(AudioFileError::NoAudioTrack);
            break;
        }
        match msg.view() {
            Eos(..) => break,
            Error(err) => {
                transcode_err = Some(AudioFileError::Other(format!(
                    "{} ({})",
                    err.error(),
                    err.debug().unwrap_or_default()
                )));
                break;
            }
            _ => {}
        }
    }
    let _ = pipeline.set_state(gst::State::Null);
    transcode_err.map_or(Ok(()), Err)
}

/// Structure name of the application message `connect_audio_pads`
/// posts when decodebin has exposed every stream and none is audio.
const NO_AUDIO_MESSAGE: &str = "meditate-no-audio-track";

/// Link decodebin's first **audio** pad to `audioconvert` (video or
/// subtitle pads are left unlinked), and post `NO_AUDIO_MESSAGE` on
/// the bus if decodebin finishes exposing pads without an audio one.
/// Without that, a video-only file hung the transcode forever
/// (gst 1.26.2: no EOS, no error once nothing links).
pub(crate) fn connect_audio_pads(
    decodebin: &gstreamer::Element,
    audioconvert: &gstreamer::Element,
) -> Result<(), AudioFileError> {
    use gstreamer as gst;
    use gst::prelude::*;

    let sink = audioconvert
        .static_pad("sink")
        .ok_or("audioconvert missing sink pad".to_string())?;
    let sink_for_pads = sink.clone();
    decodebin.connect_pad_added(move |_, src_pad| {
        let is_audio = src_pad
            .current_caps()
            .unwrap_or_else(|| src_pad.query_caps(None))
            .structure(0)
            .is_some_and(|s| s.name().starts_with("audio/"));
        if is_audio && !sink_for_pads.is_linked() {
            let _ = src_pad.link(&sink_for_pads);
        }
    });
    decodebin.connect_no_more_pads(move |decodebin| {
        if !sink.is_linked() {
            let msg = gst::message::Application::builder(
                gst::Structure::new_empty(NO_AUDIO_MESSAGE),
            )
            .src(decodebin)
            .build();
            let _ = decodebin.post_message(msg);
        }
    });
    Ok(())
}

/// True for the message `connect_audio_pads` posts on a file
/// without an audio stream.
pub(crate) fn is_no_audio_message(msg: &gstreamer::Message) -> bool {
    match msg.view() {
        gstreamer::MessageView::Application(app) => app
            .structure()
            .is_some_and(|s| s.name() == NO_AUDIO_MESSAGE),
        _ => false,
    }
}

fn present_rename_dialog(
    anchor: &gtk::Button,
    app: &MeditateApplication,
    uuid: &str,
    current_name: &str,
    rebuilder: crate::Rebuilder,
) {
    // One name set over both categories; its own name is no clash.
    let except = uuid.to_string();
    let taken = taken_in(app, move |db, name| meditate_core::db::is_bell_sound_name_taken_from_db(db, name, &except));
    let app = app.clone();
    let uuid = uuid.to_string();
    name_dialog(anchor, &gettext("Rename Sound"), current_name, "", &gettext("Rename"), taken, move |name| {
        let renamed = app.with_db(|db| db.rename_bell_sound(&uuid, name));
        if matches!(renamed, Some(Ok(()))) {
            if let Some(rb) = rebuilder.borrow().as_ref() {
                rb();
            }
        }
        renamed
    });
}

fn present_delete_dialog(
    anchor: &gtk::Button,
    app: &MeditateApplication,
    uuid: &str,
    rebuilder: crate::Rebuilder,
) {
    let dialog = adw::AlertDialog::builder()
        .heading(gettext("Delete Sound?"))
        .body(gettext("Bells that reference this sound will lose their audio."))
        .close_response("cancel")
        .default_response("cancel")
        .build();
    dialog.add_response("cancel", &gettext("Cancel"));
    dialog.add_response("delete", &gettext("Delete"));
    dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);

    let app = app.clone();
    let uuid = uuid.to_string();
    let anchor_for_failure = anchor.clone();
    dialog.connect_response(None, move |_, id| {
        if id != "delete" { return; }
        let deleted = app.with_db(|db| db.delete_bell_sound(&uuid));
        if write_ok(&anchor_for_failure, "sound.delete", deleted, Failed::Delete) {
            meditate_core::audio_files::remove_sound_files(&crate::sync_runner::local_sounds_dir(), &uuid);
        }
        if let Some(rb) = rebuilder.borrow().as_ref() {
            rb();
        }
    });

    dialog.present(Some(anchor));
}

#[cfg(test)]
mod tests {
    use super::*;

    // mp4 import (issue #2): the bell transcode path (mono +
    // loudness-normalised) on the same fixtures as guided.rs.
    fn bell_transcode(name: &str) -> Result<u32, AudioFileError> {
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name);
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("out.ogg");
        transcode_to_ogg(&src, &dest)?;
        crate::guided::probe_duration_secs(&dest)
    }

    #[test]
    fn bell_transcode_accepts_m4a() {
        assert_eq!(bell_transcode("audio_only.m4a"), Ok(1));
    }

    #[test]
    fn bell_transcode_accepts_audio_only_mp4() {
        assert_eq!(bell_transcode("audio_only.mp4"), Ok(1));
    }

    #[test]
    fn bell_transcode_accepts_mp4_with_video_track() {
        assert_eq!(bell_transcode("video_and_audio.mp4"), Ok(1));
    }

    #[test]
    fn bell_transcode_of_video_only_mp4_is_no_audio_track() {
        // Used to hang forever — see the guided.rs twin.
        assert_eq!(
            bell_transcode("video_only.mp4"),
            Err(AudioFileError::NoAudioTrack),
        );
    }
}
