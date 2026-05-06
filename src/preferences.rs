use adw::prelude::*;
use adw::subclass::prelude::ObjectSubclassIsExt;
use gtk::{gio, glib};

use crate::application::MeditateApplication;
use crate::i18n::gettext;

pub fn show_preferences(app: &MeditateApplication) {
    show_preferences_on_page(app, None);
}

/// Open the Preferences dialog with a specific page pre-selected.
/// Page name is matched against the `name` property on each
/// `AdwPreferencesPage` ("general", "data", "labels"). Used by the
/// headerbar sync indicator so a click lands directly on Data.
pub fn show_preferences_on_page(app: &MeditateApplication, initial_page: Option<&str>) {
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
        .name("general")
        .icon_name("preferences-system-symbolic")
        .build();

    // ── Sound group ───────────────────────────────────────────────────────────
    // The legacy "End sound" ComboRow + custom-file picker lived here
    // before B.4. Sound choice now lives on the timer setup's End Bell
    // row, which drills into the bell-sound chooser. Only the
    // vibrate-on-end toggle remains in Preferences for now; B.4.5
    // adds a "Sounds" tab for managing the bell-sound library.

    let sound_group = adw::PreferencesGroup::builder()
        .title(gettext("Sound"))
        .build();

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

    // ── Vibration group ───────────────────────────────────────────────────────
    // Always-present "Manage vibration patterns" entry — works on
    // laptop where per-bell vibration toggles are greyed (no haptic
    // motor), so the user can still author and sync patterns. Tapping
    // closes the Preferences dialog and pushes the chooser onto the
    // main window's nav view.
    let vibration_group = adw::PreferencesGroup::builder()
        .title(gettext("Vibration"))
        .build();
    let manage_patterns_row = adw::ActionRow::builder()
        .title(gettext("Manage vibration patterns"))
        .activatable(true)
        .build();
    let chevron = gtk::Image::from_icon_name("go-next-symbolic");
    chevron.add_css_class("dim-label");
    manage_patterns_row.add_suffix(&chevron);
    let app_for_row = app.clone();
    let dialog_for_row = dialog.clone();
    manage_patterns_row.connect_activated(move |_| {
        dialog_for_row.close();
        let Some(window) = app_for_row.active_window()
            .and_then(|w| w.downcast::<crate::window::MeditateWindow>().ok())
        else { return; };
        window.push_vibrations_chooser(&app_for_row, None, |_uuid| {
            // Manage-mode: nothing to do with the picked uuid
            // (the chooser pops itself; the list page above will
            // refresh from the rebuild fired by rename/delete).
        });
    });
    vibration_group.add(&manage_patterns_row);
    general_page.add(&vibration_group);

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
    dialog.add(&general_page);

    // ── Data page ─────────────────────────────────────────────────────────────

    let data_page = adw::PreferencesPage::builder()
        .title(gettext("Data"))
        .name("data")
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

    // Test connection — runs a single PROPFIND against the entered
    // URL+credentials, doesn't persist anything. Useful to verify
    // before saving (the more common case: an app-password got
    // revoked on the Nextcloud server and the user wants to confirm
    // the new one works). Reads from form fields; if the password
    // field is blank, falls back to the saved keychain entry so the
    // "URL and username are right but is the keyring still good?"
    // workflow doesn't require re-typing.
    let test_row = adw::ActionRow::builder()
        .title(gettext("Test connection"))
        .subtitle(gettext("Verify the URL and credentials reach your Nextcloud."))
        .activatable(true)
        .build();
    let test_btn = gtk::Button::builder()
        .label(gettext("_Test"))
        .use_underline(true)
        .valign(gtk::Align::Center)
        .css_classes(["flat"])
        .build();
    test_row.add_suffix(&test_btn);
    test_row.set_activatable_widget(Some(&test_btn));
    // Note: test_row is added to sync_group below, AFTER save_row, so
    // the save action sits closer to the input fields and Test reads
    // as a secondary verification step.

    test_btn.connect_clicked(glib::clone!(
        #[weak] dialog,
        #[weak] url_row,
        #[weak] username_row,
        #[weak] password_row,
        #[weak] test_btn,
        move |_| {
            let url = url_row.text().trim().to_string();
            let username = username_row.text().trim().to_string();
            if url.is_empty() || username.is_empty() {
                data_toast(&dialog, &gettext("Enter URL and username"));
                return;
            }
            // If the password field is blank, fall back to the saved
            // keychain entry so testing doesn't require re-typing for
            // the "URL+user are right, is the keyring still good?" case.
            let typed_pw = password_row.text().to_string();
            let password = if typed_pw.is_empty() {
                match crate::keychain::read_password(&url, &username) {
                    Ok(Some(p)) => p,
                    Ok(None) => {
                        data_toast(&dialog, &gettext("Enter a password"));
                        return;
                    }
                    Err(e) => {
                        // Full error to diag log; toast stays narrow.
                        crate::diag::log(&format!(
                            "test_connection: keychain read failed: {e:?}"));
                        data_toast(&dialog, &gettext("Keyring read failed"));
                        return;
                    }
                }
            } else {
                typed_pw
            };

            // Disable the button while the HTTP call is in flight.
            test_btn.set_sensitive(false);

            // Run the synchronous HTTP call on the GIO blocking pool
            // so it doesn't stall the GLib main loop, then await the
            // result on the main thread to update UI directly.
            // `spawn_local` keeps !Send GTK widgets in scope; only
            // `String`s and `TestConnectionResult` (also pure data)
            // cross into the worker, both Send.
            glib::MainContext::default().spawn_local(glib::clone!(
                #[weak] dialog,
                #[weak] test_btn,
                async move {
                    let result = gtk::gio::spawn_blocking(move || {
                        crate::sync_runner::test_connection(
                            &url, &username, &password)
                    }).await;
                    test_btn.set_sensitive(true);
                    // Toast = short Display; diag log = full detail().
                    // Keeps the phone toast inside the visible width
                    // while preserving the network-error specifics
                    // for post-hoc troubleshooting.
                    let (toast, detail) = match result {
                        Ok(r) => (r.to_string(), r.detail()),
                        Err(_) => {
                            let m = gettext("Test failed");
                            (m.clone(), "worker thread panicked".to_string())
                        }
                    };
                    crate::diag::log(&format!("test_connection: {detail}"));
                    dialog.add_toast(adw::Toast::builder()
                        .title(&toast).timeout(4).build());
                }
            ));
        }
    ));

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
    sync_group.add(&test_row);

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
                data_toast(&dialog, &gettext("Enter URL and username"));
                return;
            }

            // Order matters: store the password FIRST, then save the
            // account, then fire the sync trigger explicitly. The
            // alternative (account → with_db_mut auto-trigger →
            // password) races the worker thread against the keychain
            // write — the worker reads the keychain before the
            // password lands, sees PasswordMissing, bails. First
            // sync never happens until the user authors something
            // else, so the Meditate folder isn't created on Nextcloud.
            //
            // Empty password = "keep what's in the keychain" — don't
            // clobber the saved one with "".
            if !password.is_empty() {
                match crate::keychain::store_password(url_trimmed, username_trimmed, &password) {
                    Ok(()) => password_row.set_text(""),
                    Err(e) => {
                        // Log the full error to diagnostics — toast text
                        // gets cut off in narrow viewports (Librem 5,
                        // GNOME Shell), but the diagnostics file is
                        // uncapped and visible via About → Troubleshooting.
                        crate::diag::log(&format!("keychain store failed: {e:?}"));
                        data_toast(&dialog, &gettext("Keyring write failed"));
                        return;
                    }
                }
            }

            // Now save the account. Use `with_db` (not `with_db_mut`)
            // so we control when the trigger fires — explicitly,
            // below, after BOTH credentials are in place.
            let account_result = app.with_db(|db| {
                crate::sync_settings::set_nextcloud_account(db, url_trimmed, username_trimmed)
            });
            match account_result {
                Some(Ok(())) => {}
                Some(Err(e)) => {
                    crate::diag::log(&format!(
                        "sync settings save failed: {e:?}"));
                    data_toast(&dialog, &gettext("Save failed"));
                    return;
                }
                None => {
                    data_toast(&dialog, &gettext("Database unavailable"));
                    return;
                }
            }

            data_toast(&dialog, &gettext("Sync settings saved"));
            // Now both pieces are persisted — kick off the first sync.
            app.trigger_sync();
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

    // ── Labels page removed ───────────────────────────────────────────────────
    // Library management (create / rename / delete) lives inside the
    // label chooser pushed from the Setup + Done view label rows now.

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

    // If a caller asked us to open on a specific page, select it
    // before present(). AdwPreferencesDialog ignores unknown names
    // gracefully, so passing None or a typo is safe.
    if let Some(page_name) = initial_page {
        dialog.set_visible_page_name(page_name);
    }

    let parent = app.active_window();
    dialog.present(parent.as_ref());
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
