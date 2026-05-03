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

    let sounds = app
        .with_db(|db| db.list_bell_sounds())
        .and_then(|r| r.ok())
        .unwrap_or_default();

    let on_selected = Rc::new(on_selected);
    let nav_view_clone = nav_view.clone();
    for sound in sounds {
        let row = build_sound_row(
            &sound,
            current_uuid.as_deref(),
            &nav_view_clone,
            on_selected.clone(),
        );
        group.add(&row);
    }

    // Stop any in-flight preview when the user pops the page so a
    // bell doesn't keep ringing through the next setup screen.
    page.connect_hidden(move |_| crate::sound::stop_preview());

    nav_view.push(&page);
}

/// Build the "Sounds" preferences-tab page used to manage the
/// bell-sound library: rename rows, delete custom imports, preview
/// every entry. Bundled rows can't be deleted but can be renamed.
/// Same Play/Stop preview as the chooser (rows + buttons share
/// PREVIEW_MEDIA via mono playback).
pub fn build_sounds_management_page(app: &MeditateApplication) -> adw::PreferencesPage {
    let prefs_page = adw::PreferencesPage::builder()
        .title(gettext("Sounds"))
        .name("sounds")
        .icon_name("audio-x-generic-symbolic")
        .build();

    let group = adw::PreferencesGroup::new();
    let rows: Rc<std::cell::RefCell<Vec<adw::ActionRow>>> =
        Rc::new(std::cell::RefCell::new(Vec::new()));

    // Rebuild closure so rename / delete flows can re-render the
    // group from current DB state. Stored behind the same Rc<RefCell>
    // pattern bells.rs uses so the closures plumbed into per-row
    // handlers can fire it without us threading self-referential
    // generics.
    let rebuilder: Rc<std::cell::RefCell<Option<Box<dyn Fn()>>>> =
        Rc::new(std::cell::RefCell::new(None));

    let rebuilder_for_init = rebuilder.clone();
    let group_for_init = group.clone();
    let rows_for_init = rows.clone();
    let app_for_init = app.clone();
    *rebuilder.borrow_mut() = Some(Box::new(move || {
        rebuild_management_list(
            &group_for_init,
            &rows_for_init,
            &app_for_init,
            rebuilder_for_init.clone(),
        );
    }));

    if let Some(rb) = rebuilder.borrow().as_ref() {
        rb();
    }

    prefs_page.add(&group);
    prefs_page
}

fn rebuild_management_list(
    group: &adw::PreferencesGroup,
    rows: &Rc<std::cell::RefCell<Vec<adw::ActionRow>>>,
    app: &MeditateApplication,
    rebuilder: Rc<std::cell::RefCell<Option<Box<dyn Fn()>>>>,
) {
    for row in rows.borrow_mut().drain(..) {
        group.remove(&row);
    }
    let sounds = app
        .with_db(|db| db.list_bell_sounds())
        .and_then(|r| r.ok())
        .unwrap_or_default();
    for sound in sounds {
        let row = build_management_row(&sound, app, rebuilder.clone());
        group.add(&row);
        rows.borrow_mut().push(row);
    }
}

fn build_management_row(
    sound: &BellSound,
    app: &MeditateApplication,
    rebuilder: Rc<std::cell::RefCell<Option<Box<dyn Fn()>>>>,
) -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .title(&sound.name)
        .subtitle(if sound.is_bundled {
            gettext("Bundled")
        } else {
            gettext("Custom")
        })
        .build();

    // Per-row preview button — same Play/Stop toggle as the chooser.
    let play_btn = gtk::Button::builder()
        .icon_name("media-playback-start-symbolic")
        .tooltip_text(gettext("Preview sound"))
        .css_classes(["flat", "circular"])
        .valign(gtk::Align::Center)
        .build();
    let path = sound.file_path.clone();
    let is_bundled = sound.is_bundled;
    let playing = Rc::new(Cell::new(false));
    {
        let playing = playing.clone();
        let play_btn_clone = play_btn.clone();
        play_btn.connect_clicked(move |_| {
            if playing.get() {
                crate::sound::stop_preview();
                playing.set(false);
                play_btn_clone.set_icon_name("media-playback-start-symbolic");
                return;
            }
            let media = crate::sound::play_preview(&path, is_bundled);
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
    }
    row.add_suffix(&play_btn);

    // Rename button — opens an AlertDialog with a text entry.
    let rename_btn = gtk::Button::builder()
        .icon_name("document-edit-symbolic")
        .tooltip_text(gettext("Rename"))
        .css_classes(["flat", "circular"])
        .valign(gtk::Align::Center)
        .build();
    {
        let app = app.clone();
        let uuid = sound.uuid.clone();
        let row_clone = row.clone();
        let rebuilder = rebuilder.clone();
        rename_btn.connect_clicked(move |btn| {
            present_rename_dialog(btn, &app, &uuid, &row_clone.title(), rebuilder.clone());
        });
    }
    row.add_suffix(&rename_btn);

    // Delete button — only for non-bundled rows. Bundled stay
    // permanent; the chooser would just re-seed them on next open
    // anyway, and an accidental tombstone could confuse a peer.
    if !sound.is_bundled {
        let delete_btn = gtk::Button::builder()
            .icon_name("user-trash-symbolic")
            .tooltip_text(gettext("Delete sound"))
            .css_classes(["flat", "circular", "destructive-action"])
            .valign(gtk::Align::Center)
            .build();
        let app = app.clone();
        let uuid = sound.uuid.clone();
        let rebuilder = rebuilder.clone();
        delete_btn.connect_clicked(move |btn| {
            present_delete_dialog(btn, &app, &uuid, rebuilder.clone());
        });
        row.add_suffix(&delete_btn);
    }

    row
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

fn build_sound_row(
    sound: &BellSound,
    current_uuid: Option<&str>,
    nav_view: &adw::NavigationView,
    on_selected: Rc<dyn Fn(String)>,
) -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .title(&sound.name)
        .activatable(true)
        .build();

    // Currently-selected row gets a discreet checkmark on the left
    // (suffix order in adw is left-to-right on the right side, so
    // adding the check first puts it before the play button).
    if current_uuid == Some(&sound.uuid) {
        let check = gtk::Image::from_icon_name("object-select-symbolic");
        check.add_css_class("dim-label");
        row.add_suffix(&check);
    }

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
    let path = sound.file_path.clone();
    let is_bundled = sound.is_bundled;
    let playing = Rc::new(Cell::new(false));
    {
        let playing = playing.clone();
        let play_btn_clone = play_btn.clone();
        play_btn.connect_clicked(move |_| {
            if playing.get() {
                crate::sound::stop_preview();
                playing.set(false);
                play_btn_clone.set_icon_name("media-playback-start-symbolic");
                return;
            }
            let media = crate::sound::play_preview(&path, is_bundled);
            playing.set(true);
            play_btn_clone.set_icon_name("media-playback-stop-symbolic");
            // Revert icon when playback ends — natural end-of-file,
            // user stop on the same button, or another row's Play
            // taking over the PREVIEW_MEDIA slot (which sets the old
            // MediaFile to playing=false before swapping).
            let playing_for_notify = playing.clone();
            let btn_for_notify = play_btn_clone.clone();
            media.connect_notify_local(Some("playing"), move |m, _| {
                if !m.is_playing() && playing_for_notify.get() {
                    playing_for_notify.set(false);
                    btn_for_notify.set_icon_name("media-playback-start-symbolic");
                }
            });
        });
    }
    row.add_suffix(&play_btn);

    // Tap row body → pick this sound and pop. Switch + play button
    // handle their own clicks so they don't trigger row activation.
    let uuid = sound.uuid.clone();
    let nav = nav_view.clone();
    row.connect_activated(move |_| {
        on_selected(uuid.clone());
        nav.pop();
    });
    row
}
