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

use std::cell::Cell;
use std::rc::Rc;

use adw::prelude::*;

use crate::application::MeditateApplication;
use crate::db::BellSound;
use crate::i18n::gettext;

/// Push the bell-sound chooser onto the navigation view in selection
/// mode. `current_uuid` is the row to mark with a checkmark when
/// the page opens — pass `None` for "nothing selected yet". The
/// `on_selected` callback fires when the user taps a row body and
/// receives the chosen UUID; the page pops automatically right
/// after.
pub fn push_sounds_chooser(
    nav_view: &adw::NavigationView,
    app: &MeditateApplication,
    current_uuid: Option<String>,
    on_selected: impl Fn(String) + 'static,
) {
    let group = adw::PreferencesGroup::new();
    let prefs_page = adw::PreferencesPage::new();
    prefs_page.add(&group);

    let header = adw::HeaderBar::builder().show_back_button(true).build();
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&prefs_page));

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

    let rebuilder: Rc<std::cell::RefCell<Option<Box<dyn Fn()>>>> =
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
            current_uuid_for_rb.as_deref(),
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
    current_uuid: Option<&str>,
    nav_view: &adw::NavigationView,
    on_selected: Rc<dyn Fn(String)>,
    rebuilder: Rc<std::cell::RefCell<Option<Box<dyn Fn()>>>>,
) {
    for row in rows.borrow_mut().drain(..) {
        group.remove(&row);
    }

    // "Choose your own…" — synthetic, always at the top, opens a
    // file picker. The closure borrows the same rebuilder so a
    // successful import re-renders the list with the new row.
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
        current_uuid: current_uuid.map(|s| s.to_string()),
        on_selected,
        nav_view: nav_view.clone(),
    };

    let sounds = app
        .with_db(|db| db.list_bell_sounds())
        .and_then(|r| r.ok())
        .unwrap_or_default();
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
}

/// Build a sound-library row for the chooser: tap-to-pick body
/// plus per-row Play/Stop preview, Rename, and (for non-bundled
/// rows) Delete.
fn build_sound_row(
    sound: &BellSound,
    app: &MeditateApplication,
    rebuilder: Rc<std::cell::RefCell<Option<Box<dyn Fn()>>>>,
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
        check.add_css_class("dim-label");
        row.add_suffix(&check);
    }

    add_play_button(&row, sound);
    add_rename_button(&row, sound, app, rebuilder.clone());
    if !sound.is_bundled {
        // Bundled rows stay permanent — the seed re-creates them on
        // every open anyway, and an accidental tombstone could
        // confuse a peer.
        add_delete_button(&row, sound, app, rebuilder);
    }

    let uuid = sound.uuid.clone();
    let on_selected = selection.on_selected.clone();
    let nav_view = selection.nav_view.clone();
    row.connect_activated(move |_| {
        on_selected(uuid.clone());
        nav_view.pop();
    });
    row
}

fn add_play_button(row: &adw::ActionRow, sound: &BellSound) {
    // Per-row preview button. Toggles between Play and Stop:
    //   - Tap while idle → start playback, icon flips to stop.
    //   - Tap while playing → stop, icon flips back to play.
    //   - Sound finishes naturally / a different row's Play takes
    //     over PREVIEW_MEDIA → notify::playing fires false on this
    //     MediaFile, the listener flips our icon back too.
    let play_btn = gtk::Button::builder()
        .icon_name("media-playback-start-symbolic")
        .tooltip_text(gettext("Preview sound"))
        .css_classes(["flat", "circular"])
        .valign(gtk::Align::Center)
        .build();
    let sound_clone = sound.clone();
    let playing = Rc::new(Cell::new(false));
    let play_btn_clone = play_btn.clone();
    play_btn.connect_clicked(move |_| {
        if playing.get() {
            crate::sound::stop_preview();
            playing.set(false);
            play_btn_clone.set_icon_name("media-playback-start-symbolic");
            return;
        }
        let media = crate::sound::play_preview(&sound_clone);
        playing.set(true);
        play_btn_clone.set_icon_name("media-playback-stop-symbolic");
        let playing_for_notify = playing.clone();
        let btn_for_notify = play_btn_clone.clone();
        media.connect_notify_local(Some("playing"), move |m, _| {
            if !m.is_playing() && playing_for_notify.get() {
                playing_for_notify.set(false);
                btn_for_notify.set_icon_name("media-playback-start-symbolic");
            }
        });
    });
    row.add_suffix(&play_btn);
}

fn add_rename_button(
    row: &adw::ActionRow,
    sound: &BellSound,
    app: &MeditateApplication,
    rebuilder: Rc<std::cell::RefCell<Option<Box<dyn Fn()>>>>,
) {
    let rename_btn = gtk::Button::builder()
        .icon_name("document-edit-symbolic")
        .tooltip_text(gettext("Rename"))
        .css_classes(["flat", "circular"])
        .valign(gtk::Align::Center)
        .build();
    let app = app.clone();
    let uuid = sound.uuid.clone();
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
    rebuilder: Rc<std::cell::RefCell<Option<Box<dyn Fn()>>>>,
) {
    let delete_btn = gtk::Button::builder()
        .icon_name("user-trash-symbolic")
        .tooltip_text(gettext("Delete sound"))
        .css_classes(["flat", "circular", "destructive-action"])
        .valign(gtk::Align::Center)
        .build();
    let app = app.clone();
    let uuid = sound.uuid.clone();
    delete_btn.connect_clicked(move |btn| {
        present_delete_dialog(btn, &app, &uuid, rebuilder.clone());
    });
    row.add_suffix(&delete_btn);
}

/// 10 MB cap matches the locked B.5 spec — same number is enforced
/// on the inbound sync side in B.6 so a peer can't push a file
/// bigger than what the local UI would accept.
const MAX_CUSTOM_BELL_BYTES: u64 = 10 * 1024 * 1024;

/// Open a file picker, validate the chosen file, and (on confirm)
/// import it into the bell-sound library. Calls `on_imported` after
/// a successful import so the caller can rebuild its list.
fn present_file_picker(
    anchor: &adw::ActionRow,
    app: &MeditateApplication,
    on_imported: Box<dyn Fn()>,
) {
    let file_dialog = gtk::FileDialog::builder()
        .title(gettext("Choose Sound File"))
        .build();

    let filter = gtk::FileFilter::new();
    filter.set_name(Some(&gettext("Audio files")));
    for ext in ["wav", "ogg", "mp3", "opus", "flac", "m4a"] {
        filter.add_suffix(ext);
    }
    file_dialog.set_default_filter(Some(&filter));

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
            let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            if size > MAX_CUSTOM_BELL_BYTES {
                present_size_toast(&anchor);
                return;
            }

            present_import_confirm_dialog(
                &anchor,
                &app,
                &path,
                on_imported.clone(),
            );
        },
    );
}

fn present_size_toast(anchor: &adw::ActionRow) {
    // Surface via the main window's toast overlay.
    if let Some(root) = anchor.root() {
        if let Ok(window) = root.downcast::<crate::window::MeditateWindow>() {
            window.add_toast(
                adw::Toast::builder()
                    .title(gettext("File is larger than 10 MB"))
                    .timeout(4)
                    .build(),
            );
        }
    }
}

fn present_import_confirm_dialog(
    anchor: &adw::ActionRow,
    app: &MeditateApplication,
    source_path: &std::path::Path,
    on_imported: Rc<Box<dyn Fn()>>,
) {
    let stem = source_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Custom sound")
        .to_string();
    let filename = source_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();

    let entry = gtk::Entry::builder()
        .text(&stem)
        .build();

    // Inline form-error label hidden by default. Shown when the
    // user types a name that collides with an existing bell-sound,
    // so the greyed-out Import button has a visible reason next
    // to it (tooltips don't reach touch users on the Librem).
    let collision_label = gtk::Label::builder()
        .label(gettext("This name is already in use"))
        .css_classes(["error", "caption"])
        .halign(gtk::Align::Start)
        .visible(false)
        .build();

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

    // Two-level box: tight gap (4px) between entry and inline
    // error label so they read as one form field; wider gap (18px)
    // between the form and Import so import_btn↔Cancel and
    // entry↔import_btn feel symmetric (Cancel sits in AdwAlertDialog's
    // own response row below extra_child, separated by the dialog's
    // internal padding which is in the same range).
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
        .heading(gettext("Import Sound"))
        .body(format!("{} {}", gettext("Importing:"), filename))
        .extra_child(&extra_box)
        .close_response("cancel")
        .default_response("cancel")
        .build();
    dialog.add_response("cancel", &gettext("Cancel"));

    // Live validation — name not empty, no collision with existing
    // bell-sound names (case-insensitive). Same shape the rename
    // dialog uses, plus the collision_label appears next to a
    // greyed Import so the user sees *why* it isn't clickable.
    let validate: Rc<dyn Fn()> = {
        let app = app.clone();
        let entry = entry.clone();
        let import_btn = import_btn.clone();
        let collision_label = collision_label.clone();
        Rc::new(move || {
            let text = entry.text();
            let trimmed = text.trim();
            let lower = trimmed.to_lowercase();
            let collision = app
                .with_db(|db| db.list_bell_sounds())
                .and_then(|r| r.ok())
                .unwrap_or_default()
                .into_iter()
                .any(|s| s.name.to_lowercase() == lower);
            let valid = !trimmed.is_empty() && !collision;
            import_btn.set_sensitive(valid);
            // Only show the collision message when the user has
            // actually typed something. For an empty entry the
            // greyed button is intuitive on its own — flagging
            // "name in use" against a blank field would mislead.
            collision_label.set_visible(!trimmed.is_empty() && collision);
        })
    };
    validate();
    let validate_for_change = validate.clone();
    entry.connect_changed(move |_| validate_for_change());

    // Enter on the entry triggers Import (replaces the
    // activates_default behavior we lost when Import left the
    // response row).
    let import_btn_for_enter = import_btn.clone();
    entry.connect_activate(move |_| {
        if import_btn_for_enter.is_sensitive() {
            import_btn_for_enter.emit_clicked();
        }
    });

    let app_for_click = app.clone();
    let source_for_click = source_path.to_path_buf();
    let entry_for_click = entry.clone();
    let dialog_for_click = dialog.clone();
    let anchor_for_click = anchor.clone();
    let on_imported_for_click = on_imported.clone();
    import_btn.connect_clicked(move |btn| {
        let name = entry_for_click.text().to_string();
        let trimmed = name.trim().to_string();
        if trimmed.is_empty() {
            return;
        }

        // Lock the dialog while the worker runs — Cancel disabled,
        // entry frozen, button greyed and showing a spinner.
        // can_close=false suppresses Escape too. The only exit
        // path is the spawn_local closure's force_close().
        dialog_for_click.set_can_close(false);
        dialog_for_click.set_response_enabled("cancel", false);
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

        // Off-thread: copy or transcode the file. !Send GTK widgets
        // (app handle, dialog, anchor, on_imported callback) stay
        // on the main thread inside the spawn_local closure; only
        // the source PathBuf crosses into spawn_blocking.
        let app = app_for_click.clone();
        let source = source_for_click.clone();
        let dialog = dialog_for_click.clone();
        let anchor = anchor_for_click.clone();
        let on_imported = on_imported_for_click.clone();
        let trimmed_for_done = trimmed.clone();
        glib::MainContext::default().spawn_local(async move {
            let import_result = gtk::gio::spawn_blocking(move || {
                do_import_io(&source)
            }).await;

            match import_result {
                Ok(Ok((new_uuid, dest_path, mime))) => {
                    let dest_str = dest_path.to_string_lossy().to_string();
                    let mut insert_err: Option<String> = None;
                    app.with_db_mut(|db| {
                        if let Err(e) = db.insert_bell_sound_with_uuid(
                            &new_uuid, &trimmed_for_done, &dest_str, false, mime,
                        ) {
                            insert_err = Some(e.to_string());
                        }
                    });
                    if let Some(msg) = insert_err {
                        let _ = std::fs::remove_file(&dest_path);
                        present_import_error_toast(&anchor, &msg);
                    } else {
                        on_imported();
                    }
                }
                Ok(Err(e)) => present_import_error_toast(&anchor, &e),
                Err(_) => present_import_error_toast(
                    &anchor,
                    &gettext("import worker died"),
                ),
            }
            dialog.force_close();
        });
    });

    if let Some(root) = anchor.root() {
        if let Ok(window) = root.downcast::<gtk::Window>() {
            dialog.present(Some(&window));
            entry.grab_focus();
        }
    }
}

fn present_import_error_toast(anchor: &adw::ActionRow, msg: &str) {
    if let Some(root) = anchor.root() {
        if let Ok(window) = root.downcast::<crate::window::MeditateWindow>() {
            window.add_toast(
                adw::Toast::builder()
                    .title(format!("{} {}", gettext("Import failed:"), msg))
                    .timeout(4)
                    .build(),
            );
        }
    }
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
) -> std::result::Result<(String, std::path::PathBuf, &'static str), String> {
    let source_ext = source
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_else(|| "wav".to_string());

    // wav and ogg pass through gtk::MediaFile cleanly on every runtime
    // we ship to. Everything else (mp3, m4a, opus, flac, …) becomes
    // OGG/Vorbis on the way in. Vorbis at quality 0.4 (~128 kbps) is
    // far below the 10 MB cap for any reasonable bell-length input
    // and is plenty for short transient sounds.
    let (dest_ext, mime): (&str, &'static str) = match source_ext.as_str() {
        "wav" => ("wav", "audio/wav"),
        "ogg" => ("ogg", "audio/ogg"),
        _ => ("ogg", "audio/ogg"),
    };

    let new_uuid = crate::db::mint_uuid();
    let dest_dir = gtk::glib::user_data_dir()
        .join("meditate")
        .join("sounds");
    std::fs::create_dir_all(&dest_dir).map_err(|e| e.to_string())?;
    let dest_path = dest_dir.join(format!("{new_uuid}.{dest_ext}"));

    if dest_ext == source_ext.as_str() {
        std::fs::copy(source, &dest_path).map_err(|e| e.to_string())?;
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
) -> std::result::Result<(), String> {
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

    pipeline
        .add_many([
            &filesrc,
            &decodebin,
            &audioconvert,
            &audioresample,
            &vorbisenc,
            &oggmux,
            &filesink,
        ])
        .map_err(|e| e.to_string())?;
    filesrc.link(&decodebin).map_err(|e| e.to_string())?;
    gst::Element::link_many([
        &audioconvert,
        &audioresample,
        &vorbisenc,
        &oggmux,
        &filesink,
    ])
    .map_err(|e| e.to_string())?;

    // decodebin produces its source pad lazily once the input is
    // typefind-ed, so we link it to audioconvert's sink in pad-added.
    let audioconvert_sink = audioconvert
        .static_pad("sink")
        .ok_or("audioconvert missing sink pad".to_string())?;
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
        .ok_or("pipeline missing bus".to_string())?;
    let mut transcode_err: Option<String> = None;
    for msg in bus.iter_timed(gst::ClockTime::NONE) {
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

fn present_rename_dialog(
    anchor: &gtk::Button,
    app: &MeditateApplication,
    uuid: &str,
    current_name: &str,
    rebuilder: Rc<std::cell::RefCell<Option<Box<dyn Fn()>>>>,
) {
    let entry = gtk::Entry::builder()
        .text(current_name)
        .activates_default(true)
        .build();

    let dialog = adw::AlertDialog::builder()
        .heading(gettext("Rename Sound"))
        .extra_child(&entry)
        .close_response("cancel")
        .default_response("rename")
        .build();
    dialog.add_response("cancel", &gettext("Cancel"));
    dialog.add_response("rename", &gettext("Rename"));
    dialog.set_response_appearance("rename", adw::ResponseAppearance::Suggested);

    // Live validation — gate the Rename button on:
    //   1. non-empty trimmed name
    //   2. no other bell-sound row holds the same name (case-insensitive)
    // The user's own current name is allowed (renaming-to-self is a no-op).
    let validate: Rc<dyn Fn()> = {
        let app = app.clone();
        let uuid = uuid.to_string();
        let entry = entry.clone();
        let dialog = dialog.clone();
        Rc::new(move || {
            let text = entry.text();
            let trimmed = text.trim();
            let lower = trimmed.to_lowercase();
            let collision = app
                .with_db(|db| db.list_bell_sounds())
                .and_then(|r| r.ok())
                .unwrap_or_default()
                .into_iter()
                .any(|s| s.uuid != uuid && s.name.to_lowercase() == lower);
            let valid = !trimmed.is_empty() && !collision;
            dialog.set_response_enabled("rename", valid);
        })
    };
    validate();
    let validate_for_change = validate.clone();
    entry.connect_changed(move |_| validate_for_change());

    let app = app.clone();
    let uuid = uuid.to_string();
    let entry_for_response = entry.clone();
    dialog.connect_response(None, move |_, id| {
        if id != "rename" { return; }
        let new_name = entry_for_response.text().to_string();
        let trimmed = new_name.trim();
        if trimmed.is_empty() {
            return;
        }
        app.with_db_mut(|db| db.rename_bell_sound(&uuid, trimmed));
        if let Some(rb) = rebuilder.borrow().as_ref() {
            rb();
        }
    });

    if let Some(root) = anchor.root() {
        if let Ok(window) = root.downcast::<gtk::Window>() {
            dialog.present(Some(&window));
            entry.grab_focus();
        }
    }
}

fn present_delete_dialog(
    anchor: &gtk::Button,
    app: &MeditateApplication,
    uuid: &str,
    rebuilder: Rc<std::cell::RefCell<Option<Box<dyn Fn()>>>>,
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
    dialog.connect_response(None, move |_, id| {
        if id != "delete" { return; }
        app.with_db_mut(|db| db.delete_bell_sound(&uuid));
        if let Some(rb) = rebuilder.borrow().as_ref() {
            rb();
        }
    });

    if let Some(root) = anchor.root() {
        if let Ok(window) = root.downcast::<gtk::Window>() {
            dialog.present(Some(&window));
        }
    }
}

