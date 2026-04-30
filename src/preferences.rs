use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use adw::subclass::prelude::ObjectSubclassIsExt;
use gtk::{gio, glib};

use crate::application::MeditateApplication;
use crate::i18n::gettext;

pub fn show_preferences(app: &MeditateApplication) {
    let app = app.clone();

    let dialog = adw::PreferencesDialog::builder()
        .title(gettext("Preferences"))
        .search_enabled(false)
        // Large target: AdwDialog clamps to the available window height, so
        // this effectively asks "take the whole window" without locking a
        // hard minimum that would force scrolling inside a short window.
        .content_height(900)
        .build();

    // ── General page ──────────────────────────────────────────────────────────

    let general_page = adw::PreferencesPage::builder()
        .title(gettext("General"))
        .icon_name("preferences-system-symbolic")
        .build();

    // ── Sound group ───────────────────────────────────────────────────────────

    let sound_group = adw::PreferencesGroup::builder()
        .title(gettext("Sound"))
        .build();

    let sound_choices = [
        gettext("None"),
        gettext("Singing bowl"),
        gettext("Bell"),
        gettext("Gong"),
        gettext("Custom file…"),
    ];
    let sound_choice_refs: Vec<&str> = sound_choices.iter().map(|s| s.as_str()).collect();
    let sound_row = adw::ComboRow::builder()
        .title(gettext("End sound"))
        .model(&gtk::StringList::new(&sound_choice_refs))
        .build();

    let preview_btn = gtk::Button::builder()
        .icon_name("media-playback-start-symbolic")
        .valign(gtk::Align::Center)
        .tooltip_text(gettext("Preview sound"))
        .css_classes(["flat"])
        .build();
    sound_row.add_suffix(&preview_btn);

    let current_sound = app
        .with_db(|db| db.get_setting("end_sound", "bowl"))
        .and_then(|r| r.ok())
        .unwrap_or_else(|| "bowl".to_string());
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
        .title(gettext("Sound file"))
        .visible(current_sound == "custom")
        .build();
    custom_row.set_subtitle(&path_subtitle(&custom_sound_path.borrow()));

    let choose_btn = gtk::Button::builder()
        .label(gettext("Choose…"))
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
                app.with_db_mut(|db| db.set_setting("end_sound", key));
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
                .title(gettext("Choose Sound File"))
                .build();

            let filter = gtk::FileFilter::new();
            filter.set_name(Some(&gettext("Audio files")));
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
                                custom_row.set_subtitle(&path_subtitle(&path_str));
                                *custom_sound_path.borrow_mut() = path_str.clone();
                                app.with_db_mut(|db| db.set_setting("end_sound_path", &path_str));
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

    // Vibrate-on-end toggle. Routed through feedbackd on mobile; a no-op on
    // systems without it, so desktop users with this accidentally enabled
    // just notice nothing extra.
    let vibrate_on = app
        .with_db(|db| db.get_setting("vibrate_on_end", "false"))
        .and_then(|r| r.ok())
        .map(|s| s == "true")
        .unwrap_or(false);
    let vibrate_row = adw::SwitchRow::builder()
        .title(gettext("Vibrate on session end"))
        .subtitle(gettext("Haptic feedback when the timer finishes (mobile only)"))
        .active(vibrate_on)
        .build();
    vibrate_row.connect_active_notify(glib::clone!(
        #[weak] app,
        move |row| {
            let v = if row.is_active() { "true" } else { "false" };
            app.with_db_mut(|db| db.set_setting("vibrate_on_end", v));
        }
    ));
    sound_group.add(&vibrate_row);

    general_page.add(&sound_group);

    // ── Statistics group ──────────────────────────────────────────────────────

    let stats_group = adw::PreferencesGroup::builder()
        .title(gettext("Statistics"))
        .build();

    // Weekly meditation goal — drives the ring on the Stats tab.
    let current_goal_mins = app
        .with_db(|db| db.get_setting("weekly_goal_mins", "150"))
        .and_then(|r| r.ok())
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(150.0);
    let goal_row = adw::SpinRow::builder()
        .title(gettext("Weekly goal"))
        .subtitle(gettext("Minutes per week — drives the ring on the Stats tab"))
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
                app.with_db_mut(|db| db.set_setting("weekly_goal_mins", &val.to_string()));
                app.invalidate(crate::application::InvalidateScope::STATS);
            }
        ),
    );
    stats_group.add(&goal_row);

    general_page.add(&stats_group);

    // ── Presets group ─────────────────────────────────────────────────────────

    let presets_group = adw::PreferencesGroup::builder()
        .title(gettext("Timer Presets"))
        .description(gettext("Quick-select buttons shown in the countdown timer (1–5 presets, in minutes)"))
        .build();

    let add_preset_btn = gtk::Button::builder()
        .icon_name("list-add-symbolic")
        .tooltip_text(gettext("Add Preset"))
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
            app.with_db_mut(|db| db.set_presets(&vals));
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
                .title(gettext("Minutes"))
                .adjustment(&adj)
                .build();

            let del_btn = gtk::Button::builder()
                .icon_name("user-trash-symbolic")
                .valign(gtk::Align::Center)
                .tooltip_text(gettext("Remove Preset"))
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
        .title(gettext("Data"))
        .icon_name("drive-harddisk-symbolic")
        .build();

    let backup_group = adw::PreferencesGroup::builder()
        .title(gettext("Backup"))
        .description(gettext("Export your session log to a CSV file, or restore from one."))
        .build();

    let export_row = adw::ActionRow::builder()
        .title(gettext("Export session log"))
        .subtitle(gettext("Save every session to a CSV file"))
        .activatable(true)
        .build();
    let export_btn = gtk::Button::builder()
        .icon_name("document-save-symbolic")
        .valign(gtk::Align::Center)
        .tooltip_text(gettext("Export"))
        .css_classes(["flat"])
        .build();
    export_row.add_suffix(&export_btn);
    export_row.set_activatable_widget(Some(&export_btn));
    backup_group.add(&export_row);

    let import_row = adw::ActionRow::builder()
        .title(gettext("Import from Meditate CSV"))
        .subtitle(gettext("Restore sessions from a file exported above"))
        .activatable(true)
        .build();
    let import_btn = gtk::Button::builder()
        .icon_name("document-open-symbolic")
        .valign(gtk::Align::Center)
        .tooltip_text(gettext("Import"))
        .css_classes(["flat"])
        .build();
    import_row.add_suffix(&import_btn);
    import_row.set_activatable_widget(Some(&import_btn));
    backup_group.add(&import_row);

    data_page.add(&backup_group);

    let migrate_group = adw::PreferencesGroup::builder()
        .title(gettext("Migrate"))
        .description(gettext("Import sessions from another meditation app."))
        .build();

    let it_row = adw::ActionRow::builder()
        .title(gettext("Import from Insight Timer"))
        .subtitle(gettext("Upload an Insight Timer CSV export"))
        .activatable(true)
        .build();
    let it_btn = gtk::Button::builder()
        .icon_name("document-open-symbolic")
        .valign(gtk::Align::Center)
        .tooltip_text(gettext("Import Insight Timer CSV"))
        .css_classes(["flat"])
        .build();
    it_row.add_suffix(&it_btn);
    it_row.set_activatable_widget(Some(&it_btn));
    migrate_group.add(&it_row);

    data_page.add(&migrate_group);

    // ── Nextcloud sync group ──────────────────────────────────────────────
    //
    // Opt-in: the rows are blank on a fresh install. Saving here writes
    // URL+username to the sync_state KV (`sync_settings`) and the
    // password to libsecret (`keychain`). The password row stays blank
    // on dialog reopen — leaving it blank on Save means "keep the
    // currently-stored password".

    let sync_group = adw::PreferencesGroup::builder()
        .title(gettext("Nextcloud Sync"))
        .description(gettext(
            "Sync sessions, labels, and preferences between your devices via your own Nextcloud server.",
        ))
        .build();

    let url_row = adw::EntryRow::builder()
        .title(gettext("Server URL"))
        .input_purpose(gtk::InputPurpose::Url)
        .build();
    let username_row = adw::EntryRow::builder()
        .title(gettext("Username"))
        .build();
    let password_row = adw::PasswordEntryRow::builder()
        .title(gettext("App password"))
        .build();
    // Helps the user understand that empty password is non-destructive.
    password_row.add_css_class("monospace");

    // Pre-fill from previously-saved values. Password stays blank by
    // design — we don't echo what's in the keychain.
    if let Some(account) = app
        .with_db(|db| crate::sync_settings::get_nextcloud_account(db))
        .and_then(|r| r.ok())
        .flatten()
    {
        url_row.set_text(&account.url);
        username_row.set_text(&account.username);
    }

    sync_group.add(&url_row);
    sync_group.add(&username_row);
    sync_group.add(&password_row);

    // Save button as a row suffix — clicking it commits the form.
    let save_row = adw::ActionRow::builder()
        .title(gettext("Save sync settings"))
        .subtitle(gettext("Stores URL and username locally; password goes to your keyring."))
        .activatable(true)
        .build();
    let save_btn = gtk::Button::builder()
        .label(gettext("_Save"))
        .use_underline(true)
        .valign(gtk::Align::Center)
        .css_classes(["suggested-action"])
        .build();
    save_row.add_suffix(&save_btn);
    save_row.set_activatable_widget(Some(&save_btn));
    sync_group.add(&save_row);

    save_btn.connect_clicked(glib::clone!(
        #[strong] app,
        #[weak] dialog,
        #[weak] url_row,
        #[weak] username_row,
        #[weak] password_row,
        move |_| {
            let url = url_row.text().to_string();
            let username = username_row.text().to_string();
            let password = password_row.text().to_string();

            // Trim leading/trailing whitespace on URL and username only.
            // Password is taken verbatim — Nextcloud app-passwords are
            // hex blobs that don't need trimming.
            let url_trimmed = url.trim();
            let username_trimmed = username.trim();

            if url_trimmed.is_empty() || username_trimmed.is_empty() {
                data_toast(&dialog, &gettext(
                    "URL and username are required."));
                return;
            }

            // Persist the account first; if the keychain step fails the
            // URL/username are still saved (the user can retry the
            // password). Order chosen so a half-success leaves a usable
            // state rather than a broken one.
            // `with_db_mut` so saving the account (which is what
            // unlocks sync from "unconfigured" to "go") immediately
            // fires the first sync attempt. Without that the user
            // would have to click something else to trigger it.
            let account_result = app.with_db_mut(|db| {
                crate::sync_settings::set_nextcloud_account(db, url_trimmed, username_trimmed)
            });
            match account_result {
                Some(Ok(())) => {}
                Some(Err(e)) => {
                    data_toast(&dialog, &format!(
                        "{}: {e}", gettext("Couldn't save sync settings")));
                    return;
                }
                None => {
                    // No DB available — shouldn't happen at runtime.
                    data_toast(&dialog, &gettext("Database unavailable; sync settings not saved."));
                    return;
                }
            }

            // Empty password = "keep what's in the keychain". Storing
            // an empty string would clobber the existing one which is
            // almost never what the user means.
            if !password.is_empty() {
                match crate::keychain::store_password(url_trimmed, username_trimmed, &password) {
                    Ok(()) => {
                        password_row.set_text("");
                        data_toast(&dialog, &gettext("Sync settings saved."));
                    }
                    Err(e) => {
                        data_toast(&dialog, &format!(
                            "{}: {e}",
                            gettext("URL/username saved, but the password couldn't be stored"),
                        ));
                    }
                }
            } else {
                data_toast(&dialog, &gettext("Sync settings saved."));
            }
        }
    ));

    data_page.add(&sync_group);

    let danger_group = adw::PreferencesGroup::builder()
        .title(gettext("Danger Zone"))
        .description(gettext("These actions cannot be undone."))
        .build();

    let delete_row = adw::ActionRow::builder()
        .title(gettext("Delete all sessions"))
        .subtitle(gettext("Permanently remove every logged session"))
        .activatable(true)
        .build();
    let delete_btn = gtk::Button::builder()
        .label(gettext("_Delete All"))
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
        .title(gettext("Labels"))
        .icon_name("user-bookmarks-symbolic")
        .build();

    let labels_group = adw::PreferencesGroup::builder()
        .title(gettext("Labels"))
        .description(gettext("Organize sessions with custom labels"))
        .build();

    let add_btn = gtk::Button::builder()
        .icon_name("list-add-symbolic")
        .tooltip_text(gettext("Add Label"))
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
                .with_db_mut(|db| db.create_label(&gettext("New label")))
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
                // Stats view picks up weekly goal changes.
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
    app.with_db_mut(|db| db.delete_label(label_id.get()));
    // Label text shows on every log card; deleting it affects rendering.
    // Stats also indirectly (label-filter dropdowns etc.).
    app.invalidate(crate::application::InvalidateScope::ALL);
    // Force a refresh of the main window's tabs — invalidate() only sets
    // dirty flags, which the visible-child handler consumes on tab
    // switch. Without this, a user who deletes a label while sitting on
    // the log tab sees the old (now-stale) chips until they switch tabs.
    refresh_main_window(app);
    row.set_visible(false);

    // When sessions were affected the user already confirmed via AlertDialog,
    // so undo would be misleading: the label would be recreated but the
    // sessions would stay unlabeled.  Only offer undo for unused labels.
    let mut builder = adw::Toast::builder().title(gettext("Label deleted")).timeout(4);
    if allow_undo {
        builder = builder.button_label(gettext("Undo"));
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
                    .with_db_mut(|db| db.create_label(&deleted_name))
                    .and_then(|r| r.ok())
                {
                    label_id.set(label.id);
                    row.set_visible(true);
                    app.invalidate(crate::application::InvalidateScope::ALL);
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
        .tooltip_text(gettext("Discard Changes"))
        .css_classes(["flat"])
        .visible(false)
        .build();
    let apply_btn = gtk::Button::builder()
        .icon_name("object-select-symbolic")
        .valign(gtk::Align::Center)
        .tooltip_text(gettext("Save"))
        .css_classes(["flat"])
        .visible(false)
        .build();
    let delete_btn = gtk::Button::builder()
        .icon_name("user-trash-symbolic")
        .valign(gtk::Align::Center)
        .tooltip_text(gettext("Delete Label"))
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
                        .title(gettext("A label with that name already exists"))
                        .timeout(4)
                        .build(),
                );
                return;
            }
            app.with_db_mut(|db| db.update_label(label_id.get(), &new_name));
            // Renamed labels need to reflect on log cards + label filter.
            app.invalidate(crate::application::InvalidateScope::ALL);
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
                    gettext("1 session uses this label and will become unlabeled.")
                } else {
                    gettext("{n} sessions use this label and will become unlabeled.")
                        .replace("{n}", &session_count.to_string())
                };
                let alert = adw::AlertDialog::builder()
                    .heading(gettext("Delete Label?"))
                    .body(body)
                    .default_response("cancel")
                    .close_response("cancel")
                    .build();
                alert.add_response("cancel", &gettext("Cancel"));
                alert.add_response("delete", &gettext("Delete"));
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
                .title(gettext("Export Session Log"))
                .initial_name(data_io::suggested_export_filename())
                .build();
            let filter = gtk::FileFilter::new();
            filter.set_name(Some(&gettext("CSV files")));
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
                            Ok(n) => data_toast(&dialog, &pluralize_sessions(
                                &gettext("Exported 1 session"),
                                &gettext("Exported {n} sessions"),
                                n,
                            )),
                            Err(e) => data_toast(&dialog, &gettext("Export failed: {error}")
                                .replace("{error}", &e.to_string())),
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
            open_import_dialog(&app, &dialog, &gettext("Import Session Log"), |app, path| {
                data_io::import_csv(app, path)
            });
        }
    ));

    // ── Import from Insight Timer ────────────────────────────────────────
    it_btn.connect_clicked(glib::clone!(
        #[weak] app,
        #[weak] dialog,
        move |_| {
            open_import_dialog(&app, &dialog, &gettext("Import from Insight Timer"), |app, path| {
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
                .heading(gettext("Delete Every Session?"))
                .body(gettext("This permanently removes every session in your log. Export a backup first if you want to keep any history."))
                .default_response("cancel")
                .close_response("cancel")
                .build();
            alert.add_response("cancel", &gettext("Cancel"));
            alert.add_response("delete", &gettext("Delete All"));
            alert.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
            alert.connect_response(
                Some("delete"),
                glib::clone!(
                    #[weak] app,
                    #[weak] dialog,
                    move |_, _| {
                        match data_io::delete_all(&app) {
                            Ok(n) => {
                                data_toast(&dialog, &pluralize_sessions(
                                    &gettext("Deleted 1 session"),
                                    &gettext("Deleted {n} sessions"),
                                    n,
                                ));
                                app.invalidate(crate::application::InvalidateScope::ALL);
                                refresh_main_window(&app);
                            }
                            Err(e) => data_toast(&dialog, &gettext("Delete failed: {error}")
                                .replace("{error}", &e.to_string())),
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
                    data_toast(&dialog, &pluralize_sessions(
                        &gettext("Imported 1 session"),
                        &gettext("Imported {n} sessions"),
                        n,
                    ));
                    app.invalidate(crate::application::InvalidateScope::ALL);
                    refresh_main_window(&app);
                }
                Err(e) => data_toast(&dialog, &gettext("Import failed: {error}")
                    .replace("{error}", &e.to_string())),
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
fn path_subtitle(path: &str) -> String {
    if path.is_empty() {
        return gettext("No file selected");
    }
    std::path::Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(path)
        .to_string()
}

/// Two-form pluralization for session counts. Uses the shipped `_one` /
/// `_other` msgids directly — we don't need full ngettext support because
/// English plurals are trivial and the catalogs cover enough locales that
/// a 1 / ≥2 split is a reasonable approximation.
fn pluralize_sessions(singular: &str, plural: &str, n: usize) -> String {
    if n == 1 {
        singular.to_string()
    } else {
        plural.replace("{n}", &n.to_string())
    }
}
