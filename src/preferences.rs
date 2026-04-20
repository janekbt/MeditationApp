use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use adw::subclass::prelude::ObjectSubclassIsExt;
use gtk::{gio, glib};

use crate::application::MeditateApplication;

pub fn show_preferences(app: &MeditateApplication) {
    let app = app.clone();

    let dialog = adw::PreferencesDialog::builder()
        .title("Preferences")
        .search_enabled(false)
        // Large target: AdwDialog clamps to the available window height, so
        // this effectively asks "take the whole window" without locking a
        // hard minimum that would force scrolling inside a short window.
        .content_height(900)
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
        .model(&gtk::StringList::new(&["None", "Singing bowl", "Bell", "Gong", "Custom file…"]))
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
                crate::sound::preload_end_sound(&app);
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
                                crate::sound::preload_end_sound(&app);
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

    // Weekly meditation goal — drives the ring on the Stats tab.
    let current_goal_mins = app
        .with_db(|db| db.get_setting("weekly_goal_mins", "150"))
        .and_then(|r| r.ok())
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(150.0);
    let goal_row = adw::SpinRow::builder()
        .title("Weekly goal")
        .subtitle("Minutes per week — drives the ring on the Stats tab")
        .adjustment(&gtk::Adjustment::new(current_goal_mins, 30.0, 1000.0, 15.0, 60.0, 0.0))
        .climb_rate(15.0)
        .digits(0)
        .build();
    goal_row.connect_notify_local(
        Some("value"),
        glib::clone!(
            #[weak] app,
            move |row, _| {
                let val = row.value() as i64;
                app.with_db(|db| db.set_setting("weekly_goal_mins", &val.to_string()));
            }
        ),
    );
    stats_group.add(&goal_row);

    general_page.add(&stats_group);

    // ── Presets group ─────────────────────────────────────────────────────────

    let presets_group = adw::PreferencesGroup::builder()
        .title("Timer Presets")
        .description("Quick-select buttons shown in the countdown timer (1–5 presets, in minutes)")
        .build();

    let add_preset_btn = gtk::Button::builder()
        .icon_name("list-add-symbolic")
        .tooltip_text("Add Preset")
        .valign(gtk::Align::Center)
        .css_classes(["flat"])
        .build();
    presets_group.set_header_suffix(Some(&add_preset_btn));

    let current_presets = app
        .with_db(|db| db.get_presets())
        .and_then(|r| r.ok())
        .unwrap_or_else(|| vec![5, 10, 15, 20, 30]);

    // Track (SpinRow, delete_button) so we can update visibility and save.
    let preset_rows: Rc<RefCell<Vec<(adw::SpinRow, gtk::Button)>>> =
        Rc::new(RefCell::new(Vec::new()));

    // Save all current preset values to DB.
    let save_presets = Rc::new(glib::clone!(
        #[weak] app,
        #[strong] preset_rows,
        move || {
            let vals: Vec<u32> = preset_rows
                .borrow()
                .iter()
                .map(|(r, _)| r.value() as u32)
                .collect();
            app.with_db(|db| db.set_presets(&vals));
        }
    ));

    // Refresh add/delete button visibility based on current row count.
    let refresh_preset_btns = Rc::new(glib::clone!(
        #[weak] add_preset_btn,
        #[strong] preset_rows,
        move || {
            let count = preset_rows.borrow().len();
            add_preset_btn.set_visible(count < 5);
            for (_, del_btn) in preset_rows.borrow().iter() {
                del_btn.set_visible(count > 1);
            }
        }
    ));

    // Build one SpinRow for an existing or new preset value.
    let make_preset_row = {
        let preset_rows = preset_rows.clone();
        let save_presets = save_presets.clone();
        let refresh_preset_btns = refresh_preset_btns.clone();
        let presets_group = presets_group.clone();
        move |val: u32| -> (adw::SpinRow, gtk::Button) {
            let adj = gtk::Adjustment::new(val as f64, 1.0, 999.0, 1.0, 5.0, 0.0);
            let spin_row = adw::SpinRow::builder()
                .title("Minutes")
                .adjustment(&adj)
                .build();

            let del_btn = gtk::Button::builder()
                .icon_name("user-trash-symbolic")
                .valign(gtk::Align::Center)
                .tooltip_text("Remove Preset")
                .css_classes(["flat"])
                .build();
            spin_row.add_suffix(&del_btn);
            presets_group.add(&spin_row);

            // Save on value change.
            spin_row.connect_notify_local(
                Some("value"),
                glib::clone!(#[strong] save_presets, move |_, _| save_presets()),
            );

            // Delete: remove row from group and vec, save, refresh buttons.
            del_btn.connect_clicked(glib::clone!(
                #[weak] spin_row,
                #[weak] presets_group,
                #[strong] preset_rows,
                #[strong] save_presets,
                #[strong] refresh_preset_btns,
                move |_| {
                    presets_group.remove(&spin_row);
                    preset_rows.borrow_mut().retain(|(r, _)| r != &spin_row);
                    save_presets();
                    refresh_preset_btns();
                }
            ));

            (spin_row, del_btn)
        }
    };

    for &val in &current_presets {
        let pair = make_preset_row(val);
        preset_rows.borrow_mut().push(pair);
    }
    refresh_preset_btns();

    add_preset_btn.connect_clicked(glib::clone!(
        #[strong] preset_rows,
        #[strong] save_presets,
        #[strong] refresh_preset_btns,
        move |_| {
            let next_val = preset_rows
                .borrow()
                .last()
                .map(|(r, _)| (r.value() as u32).saturating_add(5).min(999))
                .unwrap_or(5);
            let pair = make_preset_row(next_val);
            preset_rows.borrow_mut().push(pair);
            save_presets();
            refresh_preset_btns();
        }
    ));

    general_page.add(&presets_group);
    dialog.add(&general_page);

    // ── Data page ─────────────────────────────────────────────────────────────

    let data_page = adw::PreferencesPage::builder()
        .title("Data")
        .icon_name("drive-harddisk-symbolic")
        .build();

    let backup_group = adw::PreferencesGroup::builder()
        .title("Backup")
        .description("Export your session log to a CSV file, or restore from one.")
        .build();

    let export_row = adw::ActionRow::builder()
        .title("Export session log")
        .subtitle("Save every session to a CSV file")
        .activatable(true)
        .build();
    let export_btn = gtk::Button::builder()
        .icon_name("document-save-symbolic")
        .valign(gtk::Align::Center)
        .tooltip_text("Export")
        .css_classes(["flat"])
        .build();
    export_row.add_suffix(&export_btn);
    export_row.set_activatable_widget(Some(&export_btn));
    backup_group.add(&export_row);

    let import_row = adw::ActionRow::builder()
        .title("Import from Meditate CSV")
        .subtitle("Restore sessions from a file exported above")
        .activatable(true)
        .build();
    let import_btn = gtk::Button::builder()
        .icon_name("document-open-symbolic")
        .valign(gtk::Align::Center)
        .tooltip_text("Import")
        .css_classes(["flat"])
        .build();
    import_row.add_suffix(&import_btn);
    import_row.set_activatable_widget(Some(&import_btn));
    backup_group.add(&import_row);

    data_page.add(&backup_group);

    let migrate_group = adw::PreferencesGroup::builder()
        .title("Migrate")
        .description("Import sessions from another meditation app.")
        .build();

    let it_row = adw::ActionRow::builder()
        .title("Import from Insight Timer")
        .subtitle("Upload an Insight Timer CSV export")
        .activatable(true)
        .build();
    let it_btn = gtk::Button::builder()
        .icon_name("document-open-symbolic")
        .valign(gtk::Align::Center)
        .tooltip_text("Import Insight Timer CSV")
        .css_classes(["flat"])
        .build();
    it_row.add_suffix(&it_btn);
    it_row.set_activatable_widget(Some(&it_btn));
    migrate_group.add(&it_row);

    data_page.add(&migrate_group);

    let danger_group = adw::PreferencesGroup::builder()
        .title("Danger Zone")
        .description("These actions cannot be undone.")
        .build();

    let delete_row = adw::ActionRow::builder()
        .title("Delete all sessions")
        .subtitle("Permanently remove every logged session")
        .activatable(true)
        .build();
    let delete_btn = gtk::Button::builder()
        .label("_Delete All")
        .use_underline(true)
        .valign(gtk::Align::Center)
        .css_classes(["destructive-action"])
        .build();
    delete_row.add_suffix(&delete_btn);
    delete_row.set_activatable_widget(Some(&delete_btn));
    danger_group.add(&delete_row);

    data_page.add(&danger_group);

    dialog.add(&data_page);

    // Wire the data-page actions.
    wire_data_actions(
        &app, &dialog,
        &export_btn, &import_btn, &it_btn, &delete_btn,
    );

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
        .tooltip_text("Add Label")
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
        let row = make_label_row(label.id, &label.name, &labels_group, &app, &dialog);
        labels_group.add(&row);
        rows.borrow_mut().push(row);
    }

    add_btn.connect_clicked(glib::clone!(
        #[weak] app,
        #[weak] labels_group,
        #[weak] dialog,
        #[strong] rows,
        move |_| {
            let Some(label) = app
                .with_db(|db| db.create_label("New label"))
                .and_then(|r| r.ok())
            else {
                return;
            };

            let new_row = make_label_row(label.id, &label.name, &labels_group, &app, &dialog);

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
    dialog.connect_closed(glib::clone!(
        #[weak] app,
        move |_| {
            crate::sound::stop_current();
            if let Some(win) = app
                .active_window()
                .and_then(|w| w.downcast::<crate::window::MeditateWindow>().ok())
            {
                // refresh_streak rebuilds presets, streak text, label
                // combo, and sound row — covers any pref change including
                // label add/delete/rename and preset edits.
                win.imp().timer_view.refresh_streak();
                // Stats view picks up running-average period + weekly goal.
                win.imp().stats_view.refresh();
            }
        }
    ));

    let parent = app.active_window();
    dialog.present(parent.as_ref());
}

fn do_delete(
    row: &adw::EntryRow,
    group: &adw::PreferencesGroup,
    app: &MeditateApplication,
    dialog: &adw::PreferencesDialog,
    committed: &Rc<RefCell<String>>,
    label_id: &Rc<std::cell::Cell<i64>>,
    allow_undo: bool,
) {
    app.with_db(|db| db.delete_label(label_id.get()));
    row.set_visible(false);

    // When sessions were affected the user already confirmed via AlertDialog,
    // so undo would be misleading: the label would be recreated but the
    // sessions would stay unlabeled.  Only offer undo for unused labels.
    let mut builder = adw::Toast::builder().title("Label deleted").timeout(4);
    if allow_undo {
        builder = builder.button_label("Undo");
    }
    let toast = builder.build();

    if allow_undo {
        let deleted_name = committed.borrow().clone();
        toast.connect_button_clicked(glib::clone!(
            #[weak] row,
            #[weak] app,
            #[strong] label_id,
            move |_| {
                if let Some(label) = app
                    .with_db(|db| db.create_label(&deleted_name))
                    .and_then(|r| r.ok())
                {
                    label_id.set(label.id);
                    row.set_visible(true);
                }
            }
        ));
    }

    toast.connect_dismissed(glib::clone!(
        #[weak] row,
        #[weak] group,
        move |_| {
            if !row.is_visible() && row.parent().is_some() {
                group.remove(&row);
            }
        }
    ));

    dialog.add_toast(toast);
}

fn make_label_row(
    id: i64,
    name: &str,
    group: &adw::PreferencesGroup,
    app: &MeditateApplication,
    dialog: &adw::PreferencesDialog,
) -> adw::EntryRow {
    use std::cell::Cell;

    // Wrapped so the undo handler can update it after recreating the label.
    let label_id: Rc<Cell<i64>> = Rc::new(Cell::new(id));

    let committed: Rc<RefCell<String>> = Rc::new(RefCell::new(name.to_string()));

    let row = adw::EntryRow::builder().build();
    row.set_text(name);

    // ── Suffix buttons: [discard] [apply] [delete] ────────────────────────────

    let discard_btn = gtk::Button::builder()
        .icon_name("edit-undo-symbolic")
        .valign(gtk::Align::Center)
        .tooltip_text("Discard Changes")
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
        .tooltip_text("Delete Label")
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
            row.remove_css_class("error");
        }
    ));

    // Apply: save to DB, update committed baseline, then defer focus clear.
    apply_btn.connect_clicked(glib::clone!(
        #[weak] app,
        #[weak] row,
        #[weak] apply_btn,
        #[weak] discard_btn,
        #[weak] dialog,
        #[strong] committed,
        #[strong] label_id,
        move |_| {
            let new_name = row.text().to_string();
            if new_name.is_empty() {
                return;
            }
            let taken = app
                .with_db(|db| db.is_label_name_taken(&new_name, label_id.get()))
                .and_then(|r| r.ok())
                .unwrap_or(false);
            if taken {
                row.add_css_class("error");
                dialog.add_toast(
                    adw::Toast::builder()
                        .title("A label with that name already exists")
                        .timeout(4)
                        .build(),
                );
                return;
            }
            app.with_db(|db| db.update_label(label_id.get(), &new_name));
            *committed.borrow_mut() = new_name;
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

    // Delete: if the label has been used in sessions, ask for confirmation first.
    delete_btn.connect_clicked(glib::clone!(
        #[weak] row,
        #[weak] group,
        #[weak] app,
        #[weak] dialog,
        #[strong] committed,
        #[strong] label_id,
        move |_| {
            let session_count = app
                .with_db(|db| db.label_session_count(label_id.get()))
                .and_then(|r| r.ok())
                .unwrap_or(0);

            if session_count > 0 {
                let body = if session_count == 1 {
                    "1 session uses this label and will become unlabeled.".to_string()
                } else {
                    format!("{session_count} sessions use this label and will become unlabeled.")
                };
                let alert = adw::AlertDialog::builder()
                    .heading("Delete Label?")
                    .body(body)
                    .default_response("cancel")
                    .close_response("cancel")
                    .build();
                alert.add_response("cancel", "Cancel");
                alert.add_response("delete", "Delete");
                alert.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
                alert.connect_response(
                    Some("delete"),
                    glib::clone!(
                        #[weak] row,
                        #[weak] group,
                        #[weak] app,
                        #[weak] dialog,
                        #[strong] committed,
                        #[strong] label_id,
                        move |_, _| do_delete(&row, &group, &app, &dialog, &committed, &label_id, false)
                    ),
                );
                alert.present(Some(&dialog));
            } else {
                do_delete(&row, &group, &app, &dialog, &committed, &label_id, true);
            }
        }
    ));

    row
}

// ── Data page actions ─────────────────────────────────────────────────────────

fn wire_data_actions(
    app: &MeditateApplication,
    dialog: &adw::PreferencesDialog,
    export_btn: &gtk::Button,
    import_btn: &gtk::Button,
    it_btn: &gtk::Button,
    delete_btn: &gtk::Button,
) {
    use crate::data_io;

    // ── Export to CSV ────────────────────────────────────────────────────
    export_btn.connect_clicked(glib::clone!(
        #[weak] app,
        #[weak] dialog,
        move |_| {
            let file_dialog = gtk::FileDialog::builder()
                .title("Export Session Log")
                .initial_name(data_io::suggested_export_filename())
                .build();
            let filter = gtk::FileFilter::new();
            filter.set_name(Some("CSV files"));
            filter.add_suffix("csv");
            file_dialog.set_default_filter(Some(&filter));

            let parent = app.active_window().and_downcast::<gtk::Window>();
            file_dialog.save(
                parent.as_ref(),
                None::<&gio::Cancellable>,
                glib::clone!(
                    #[weak] app,
                    #[weak] dialog,
                    move |result| {
                        let Ok(file) = result else { return; };
                        let Some(path) = file.path() else { return; };
                        match data_io::export_csv(&app, &path) {
                            Ok(n) => data_toast(&dialog, &format!(
                                "Exported {n} session{}", if n == 1 { "" } else { "s" }
                            )),
                            Err(e) => data_toast(&dialog, &format!("Export failed: {e}")),
                        }
                    }
                ),
            );
        }
    ));

    // ── Import from Meditate CSV ─────────────────────────────────────────
    import_btn.connect_clicked(glib::clone!(
        #[weak] app,
        #[weak] dialog,
        move |_| {
            open_import_dialog(&app, &dialog, "Import Session Log", |app, path| {
                data_io::import_csv(app, path)
            });
        }
    ));

    // ── Import from Insight Timer ────────────────────────────────────────
    it_btn.connect_clicked(glib::clone!(
        #[weak] app,
        #[weak] dialog,
        move |_| {
            open_import_dialog(&app, &dialog, "Import from Insight Timer", |app, path| {
                data_io::import_insighttimer(app, path)
            });
        }
    ));

    // ── Delete all (with confirmation) ───────────────────────────────────
    delete_btn.connect_clicked(glib::clone!(
        #[weak] app,
        #[weak] dialog,
        move |_| {
            let alert = adw::AlertDialog::builder()
                .heading("Delete Every Session?")
                .body("This permanently removes every session in your log. Export a backup first if you want to keep any history.")
                .default_response("cancel")
                .close_response("cancel")
                .build();
            alert.add_response("cancel", "Cancel");
            alert.add_response("delete", "Delete All");
            alert.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
            alert.connect_response(
                Some("delete"),
                glib::clone!(
                    #[weak] app,
                    #[weak] dialog,
                    move |_, _| {
                        match data_io::delete_all(&app) {
                            Ok(n) => {
                                data_toast(&dialog, &format!(
                                    "Deleted {n} session{}", if n == 1 { "" } else { "s" }
                                ));
                                refresh_main_window(&app);
                            }
                            Err(e) => data_toast(&dialog, &format!("Delete failed: {e}")),
                        }
                    }
                ),
            );
            alert.present(Some(&dialog));
        }
    ));
}

fn open_import_dialog<F>(
    app: &MeditateApplication,
    dialog: &adw::PreferencesDialog,
    title: &str,
    importer: F,
)
where F: FnOnce(&MeditateApplication, &std::path::Path) -> Result<usize, crate::data_io::DataIoError>
    + 'static,
{
    let file_dialog = gtk::FileDialog::builder()
        .title(title)
        .build();
    let filter = gtk::FileFilter::new();
    filter.set_name(Some("CSV files"));
    filter.add_suffix("csv");
    file_dialog.set_default_filter(Some(&filter));

    let parent = app.active_window().and_downcast::<gtk::Window>();
    let app = app.clone();
    let dialog = dialog.clone();
    file_dialog.open(
        parent.as_ref(),
        None::<&gio::Cancellable>,
        move |result| {
            let Ok(file) = result else { return; };
            let Some(path) = file.path() else { return; };
            match importer(&app, &path) {
                Ok(n) => {
                    data_toast(&dialog, &format!(
                        "Imported {n} session{}", if n == 1 { "" } else { "s" }
                    ));
                    refresh_main_window(&app);
                }
                Err(e) => data_toast(&dialog, &format!("Import failed: {e}")),
            }
        },
    );
}

fn data_toast(dialog: &adw::PreferencesDialog, title: &str) {
    let toast = adw::Toast::builder().title(title).timeout(4).build();
    dialog.add_toast(toast);
}

fn refresh_main_window(app: &MeditateApplication) {
    if let Some(win) = app.active_window()
        .and_then(|w| w.downcast::<crate::window::MeditateWindow>().ok())
    {
        win.imp().timer_view.refresh_streak();
        win.imp().stats_view.refresh();
        win.imp().log_view.refresh();
    }
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
