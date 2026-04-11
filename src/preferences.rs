use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gio, glib};

use crate::application::MeditateApplication;
use crate::window::MeditateWindow;

pub fn show_preferences(app: &MeditateApplication) {
    let app = app.clone();

    let dialog = adw::PreferencesDialog::builder()
        .title("Preferences")
        .search_enabled(false)
        .content_height(480)
        .build();

    // ── General page ──────────────────────────────────────────────────────────

    let general_page = adw::PreferencesPage::builder()
        .title("General")
        .icon_name("preferences-system-symbolic")
        .build();

    // ── Sound group ───────────────────────────────────────────────────────────

    let sound_group = adw::PreferencesGroup::builder()
        .title("Sound")
        .build();

    let sound_row = adw::ComboRow::builder()
        .title("End sound")
        .model(&gtk::StringList::new(&["None", "Singing Bowl", "Bell", "Gong", "Custom file…"]))
        .build();

    let preview_btn = gtk::Button::builder()
        .icon_name("media-playback-start-symbolic")
        .valign(gtk::Align::Center)
        .tooltip_text("Preview sound")
        .css_classes(["flat"])
        .build();
    sound_row.add_suffix(&preview_btn);

    let current_sound = app
        .with_db(|db| db.get_setting("end_sound", "none"))
        .and_then(|r| r.ok())
        .unwrap_or_else(|| "none".to_string());
    sound_row.set_selected(match current_sound.as_str() {
        "bowl"   => 1,
        "bell"   => 2,
        "gong"   => 3,
        "custom" => 4,
        _        => 0,
    });
    preview_btn.set_sensitive(current_sound != "none");

    // Tracks whether a preview is currently playing so the button toggles.
    let preview_playing: Rc<Cell<bool>> = Rc::new(Cell::new(false));

    // Custom file row — only visible when "Custom file…" is selected.
    let custom_sound_path: Rc<RefCell<String>> = Rc::new(RefCell::new(
        app.with_db(|db| db.get_setting("end_sound_path", ""))
            .and_then(|r| r.ok())
            .unwrap_or_default(),
    ));

    let custom_row = adw::ActionRow::builder()
        .title("Sound file")
        .visible(current_sound == "custom")
        .build();
    custom_row.set_subtitle(path_subtitle(&custom_sound_path.borrow()));

    let choose_btn = gtk::Button::builder()
        .label("Choose…")
        .valign(gtk::Align::Center)
        .css_classes(["flat"])
        .build();
    custom_row.add_suffix(&choose_btn);

    // Save selection + show/hide custom row whenever the combo changes.
    sound_row.connect_notify_local(
        Some("selected"),
        glib::clone!(
            #[weak] app,
            #[weak] custom_row,
            #[weak] preview_btn,
            #[strong] preview_playing,
            move |row, _| {
                let key = match row.selected() {
                    1 => "bowl",
                    2 => "bell",
                    3 => "gong",
                    4 => "custom",
                    _ => "none",
                };
                app.with_db(|db| db.set_setting("end_sound", key));
                custom_row.set_visible(key == "custom");
                preview_btn.set_sensitive(key != "none");
                // Stop any in-progress preview when the selection changes.
                if preview_playing.get() {
                    preview_playing.set(false);
                    preview_btn.set_icon_name("media-playback-start-symbolic");
                    crate::sound::stop_current();
                }
            }
        ),
    );

    // Toggle play/stop on the preview button.
    preview_btn.connect_clicked(glib::clone!(
        #[weak] sound_row,
        #[weak] preview_btn,
        #[strong] custom_sound_path,
        #[strong] preview_playing,
        move |_| {
            if preview_playing.get() {
                // Stop
                preview_playing.set(false);
                preview_btn.set_icon_name("media-playback-start-symbolic");
                crate::sound::stop_current();
                return;
            }

            // Start
            let media = match sound_row.selected() {
                1 => Some(crate::sound::play_bundled("bowl")),
                2 => Some(crate::sound::play_bundled("bell")),
                3 => Some(crate::sound::play_bundled("gong")),
                4 => {
                    let p = custom_sound_path.borrow().clone();
                    if p.is_empty() { None } else { Some(crate::sound::play_uri(&format!("file://{p}"))) }
                }
                _ => None,
            };
            if let Some(media) = media {
                preview_playing.set(true);
                preview_btn.set_icon_name("media-playback-stop-symbolic");

                // Reset the button icon when playback ends naturally.
                media.connect_notify_local(
                    Some("playing"),
                    glib::clone!(
                        #[strong] preview_playing,
                        #[weak] preview_btn,
                        move |m, _| {
                            if !m.is_playing() && preview_playing.get() {
                                preview_playing.set(false);
                                preview_btn.set_icon_name("media-playback-start-symbolic");
                            }
                        }
                    ),
                );
            }
        }
    ));

    // Open a file chooser to select a custom sound.
    choose_btn.connect_clicked(glib::clone!(
        #[weak] app,
        #[weak] custom_row,
        #[strong] custom_sound_path,
        move |_| {
            let file_dialog = gtk::FileDialog::builder()
                .title("Choose Sound File")
                .build();

            let filter = gtk::FileFilter::new();
            filter.set_name(Some("Audio files"));
            for ext in ["ogg", "wav", "flac", "mp3", "opus"] {
                filter.add_suffix(ext);
            }
            file_dialog.set_default_filter(Some(&filter));

            let path = custom_sound_path.borrow().clone();
            if !path.is_empty() {
                file_dialog.set_initial_file(Some(&gio::File::for_path(&path)));
            }

            let parent = app.active_window().and_downcast::<gtk::Window>();
            file_dialog.open(
                parent.as_ref(),
                None::<&gio::Cancellable>,
                glib::clone!(
                    #[weak] app,
                    #[weak] custom_row,
                    #[strong] custom_sound_path,
                    move |result| {
                        if let Ok(file) = result {
                            if let Some(p) = file.path() {
                                let path_str = p.to_string_lossy().to_string();
                                custom_row.set_subtitle(path_subtitle(&path_str));
                                *custom_sound_path.borrow_mut() = path_str.clone();
                                app.with_db(|db| db.set_setting("end_sound_path", &path_str));
                            }
                        }
                    }
                ),
            );
        }
    ));

    sound_group.add(&sound_row);
    sound_group.add(&custom_row);
    general_page.add(&sound_group);

    // ── Statistics group ──────────────────────────────────────────────────────

    let stats_group = adw::PreferencesGroup::builder()
        .title("Statistics")
        .build();

    let avg_row = adw::ComboRow::builder()
        .title("Running average period")
        .model(&gtk::StringList::new(&["7 days", "14 days", "30 days"]))
        .build();

    let current_avg = app
        .with_db(|db| db.get_setting("running_avg_days", "7"))
        .and_then(|r| r.ok())
        .unwrap_or_else(|| "7".to_string());
    avg_row.set_selected(match current_avg.as_str() {
        "14" => 1,
        "30" => 2,
        _ => 0,
    });

    avg_row.connect_notify_local(
        Some("selected"),
        glib::clone!(
            #[weak] app,
            move |row, _| {
                let val = match row.selected() {
                    1 => "14",
                    2 => "30",
                    _ => "7",
                };
                app.with_db(|db| db.set_setting("running_avg_days", val));
            }
        ),
    );

    stats_group.add(&avg_row);
    general_page.add(&stats_group);
    dialog.add(&general_page);

    // ── Labels page ───────────────────────────────────────────────────────────

    let labels_page = adw::PreferencesPage::builder()
        .title("Labels")
        .icon_name("user-bookmarks-symbolic")
        .build();

    let labels_group = adw::PreferencesGroup::builder()
        .title("Labels")
        .description("Organize sessions with custom labels")
        .build();

    let add_btn = gtk::Button::builder()
        .icon_name("list-add-symbolic")
        .tooltip_text("Add label")
        .valign(gtk::Align::Center)
        .css_classes(["flat"])
        .build();
    labels_group.set_header_suffix(Some(&add_btn));

    // All currently-tracked rows (used to re-order when a new label is added).
    let rows: Rc<RefCell<Vec<adw::EntryRow>>> = Rc::new(RefCell::new(Vec::new()));

    let labels = app
        .with_db(|db| db.list_labels())
        .and_then(|r| r.ok())
        .unwrap_or_default();

    for label in &labels {
        let row = make_label_row(label.id, &label.name, &labels_group, &app);
        labels_group.add(&row);
        rows.borrow_mut().push(row);
    }

    add_btn.connect_clicked(glib::clone!(
        #[weak] app,
        #[weak] labels_group,
        #[strong] rows,
        move |_| {
            let Some(label) = app
                .with_db(|db| db.create_label("New label"))
                .and_then(|r| r.ok())
            else {
                return;
            };

            let new_row = make_label_row(label.id, &label.name, &labels_group, &app);

            // Rows still attached to the group (excludes rows whose delete was
            // committed via a toast — those have already been removed).
            let active: Vec<adw::EntryRow> = rows
                .borrow()
                .iter()
                .filter(|r| r.parent().is_some())
                .cloned()
                .collect();

            // Detach every active row, then re-attach with new row at the front.
            for r in &active {
                labels_group.remove(r);
            }
            labels_group.add(&new_row);
            for r in &active {
                labels_group.add(r);
            }

            rows.borrow_mut().push(new_row.clone());
            new_row.grab_focus();
        }
    ));

    labels_page.add(&labels_group);
    dialog.add(&labels_page);

    // Stop preview when the user switches away from General or closes the dialog.
    general_page.connect_unmap(glib::clone!(
        #[strong] preview_playing,
        #[weak] preview_btn,
        move |_| {
            if preview_playing.get() {
                preview_playing.set(false);
                preview_btn.set_icon_name("media-playback-start-symbolic");
            }
            crate::sound::stop_current();
        }
    ));
    dialog.connect_closed(|_| {
        crate::sound::stop_current();
    });

    let parent = app.active_window();
    dialog.present(parent.as_ref());
}

fn make_label_row(
    id: i64,
    name: &str,
    group: &adw::PreferencesGroup,
    app: &MeditateApplication,
) -> adw::EntryRow {
    let committed: Rc<RefCell<String>> = Rc::new(RefCell::new(name.to_string()));

    let row = adw::EntryRow::builder().build();
    row.set_text(name);

    // ── Suffix buttons: [discard] [apply] [delete] ────────────────────────────

    let discard_btn = gtk::Button::builder()
        .icon_name("edit-undo-symbolic")
        .valign(gtk::Align::Center)
        .tooltip_text("Discard changes")
        .css_classes(["flat"])
        .visible(false)
        .build();
    let apply_btn = gtk::Button::builder()
        .icon_name("object-select-symbolic")
        .valign(gtk::Align::Center)
        .tooltip_text("Save")
        .css_classes(["flat"])
        .visible(false)
        .build();
    let delete_btn = gtk::Button::builder()
        .icon_name("user-trash-symbolic")
        .valign(gtk::Align::Center)
        .tooltip_text("Delete label")
        .css_classes(["flat"])
        .build();

    row.add_suffix(&discard_btn);
    row.add_suffix(&apply_btn);
    row.add_suffix(&delete_btn);

    // Show/hide apply+discard buttons whenever the text changes.
    row.connect_changed(glib::clone!(
        #[weak] row,
        #[weak] apply_btn,
        #[weak] discard_btn,
        #[strong] committed,
        move |_| {
            let pending = row.text().as_str() != committed.borrow().as_str();
            apply_btn.set_visible(pending);
            discard_btn.set_visible(pending);
        }
    ));

    // Apply: save to DB, update committed baseline, then defer focus clear.
    apply_btn.connect_clicked(glib::clone!(
        #[weak] app,
        #[weak] row,
        #[weak] apply_btn,
        #[weak] discard_btn,
        #[strong] committed,
        move |_| {
            let new_name = row.text().to_string();
            if !new_name.is_empty() {
                app.with_db(|db| db.update_label(id, &new_name));
                *committed.borrow_mut() = new_name;
            }
            apply_btn.set_visible(false);
            discard_btn.set_visible(false);
            // Defer focus clear: hiding the buttons makes GTK search for a new
            // focus target (it lands back on the text input). Running on the
            // next idle cycle clears it after GTK finishes that restoration.
            let row_weak = row.downgrade();
            glib::idle_add_local(move || {
                if let Some(row) = row_weak.upgrade() {
                    if let Some(root) = row.root() {
                        root.set_focus(None::<&gtk::Widget>);
                    }
                }
                glib::ControlFlow::Break
            });
        }
    ));

    // Discard: restore committed text, then defer focus clear.
    discard_btn.connect_clicked(glib::clone!(
        #[weak] row,
        #[strong] committed,
        move |_| {
            row.set_text(&committed.borrow());
            let row_weak = row.downgrade();
            glib::idle_add_local(move || {
                if let Some(row) = row_weak.upgrade() {
                    if let Some(root) = row.root() {
                        root.set_focus(None::<&gtk::Widget>);
                    }
                }
                glib::ControlFlow::Break
            });
        }
    ));

    // Delete: hide row, show undo toast on the main window.
    delete_btn.connect_clicked(glib::clone!(
        #[weak] row,
        #[weak] group,
        #[weak] app,
        move |_| {
            row.set_visible(false);

            let toast = adw::Toast::builder()
                .title("Label deleted")
                .button_label("Undo")
                .timeout(5)
                .build();

            toast.connect_button_clicked(glib::clone!(
                #[weak] row,
                move |_| { row.set_visible(true); }
            ));

            toast.connect_dismissed(glib::clone!(
                #[weak] row,
                #[weak] group,
                #[weak] app,
                move |_| {
                    if !row.is_visible() {
                        app.with_db(|db| db.delete_label(id));
                        if row.parent().is_some() {
                            group.remove(&row);
                        }
                    }
                }
            ));

            if let Some(win) = app
                .active_window()
                .and_then(|w| w.downcast::<MeditateWindow>().ok())
            {
                win.add_toast(toast);
            }
        }
    ));

    row
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Subtitle text for the custom sound row: show just the filename, or a
/// placeholder when no file has been selected yet.
fn path_subtitle(path: &str) -> &str {
    if path.is_empty() {
        return "No file selected";
    }
    std::path::Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(path)
}
