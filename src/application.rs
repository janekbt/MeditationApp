mod imp {
    use adw::prelude::*;
    use adw::subclass::prelude::*;
    use gtk::{gdk, gio, glib};
    use std::cell::RefCell;

    use crate::config;
    use crate::db::Database;
    use crate::window::MeditateWindow;

    #[derive(Debug, Default)]
    pub struct MeditateApplication {
        pub db: RefCell<Option<Database>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for MeditateApplication {
        const NAME: &'static str = "MeditateApplication";
        type Type = super::MeditateApplication;
        type ParentType = adw::Application;
    }

    impl ObjectImpl for MeditateApplication {}

    impl ApplicationImpl for MeditateApplication {
        fn activate(&self) {
            self.parent_activate();
            let app = self.obj();

            if let Some(window) = app.active_window() {
                window.present();
                return;
            }

            MeditateWindow::new(&*app).present();
        }

        fn startup(&self) {
            self.parent_startup();

            // Open (or create) the SQLite database in the user data directory.
            let db_path = glib::user_data_dir()
                .join("meditate")
                .join("meditate.db");
            match Database::open(&db_path) {
                Ok(db) => *self.db.borrow_mut() = Some(db),
                Err(e) => eprintln!("Failed to open database: {e}"),
            }

            // Register the bundled app icon so the About dialog and GNOME Shell
            // can find it in development builds (installed builds use the
            // hicolor theme path; GResource acts as a fallback).
            gtk::IconTheme::for_display(&gdk::Display::default().expect("No display"))
                .add_resource_path("/io/github/janekbt/Meditate/icons");

            // Load application CSS (chart bar styles, etc.)
            let provider = gtk::CssProvider::new();
            provider.load_from_resource("/io/github/janekbt/Meditate/style.css");
            #[allow(deprecated)]
            gtk::style_context_add_provider_for_display(
                &gdk::Display::default().expect("No display"),
                &provider,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );

            self.setup_actions();
            self.setup_accels();
        }
    }

    impl GtkApplicationImpl for MeditateApplication {}
    impl AdwApplicationImpl for MeditateApplication {}

    impl MeditateApplication {
        fn setup_actions(&self) {
            let app = self.obj();

            // app.preferences — opens AdwPreferencesWindow (Phase 6)
            let preferences_action = gio::SimpleAction::new("preferences", None);
            preferences_action.connect_activate(glib::clone!(
                #[weak]
                app,
                move |_, _| {
                    crate::preferences::show_preferences(&app);
                }
            ));
            app.add_action(&preferences_action);

            // app.about
            let about_action = gio::SimpleAction::new("about", None);
            about_action.connect_activate(glib::clone!(
                #[weak]
                app,
                move |_, _| {
                    let dialog = adw::AboutDialog::builder()
                        .application_name("Meditate")
                        .application_icon(config::APP_ID)
                        .version(config::VERSION)
                        .developer_name("janekbt")
                        .website("https://github.com/janekbt/MeditationApp")
                        .issue_url("https://github.com/janekbt/MeditationApp/issues")
                        .license_type(gtk::License::Gpl30)
                        .release_notes_version(config::VERSION)
                        .release_notes("\
                            <p>Initial release.</p>\
                            <ul>\
                              <li>Countdown and stopwatch timer</li>\
                              <li>Session log with labels and notes</li>\
                              <li>Statistics: calendar, bar chart, streaks</li>\
                              <li>Completion sounds (bowl, bell, gong, or custom file)</li>\
                              <li>Adaptive layout for desktop and phone</li>\
                            </ul>")
                        .build();

                    dialog.present(app.active_window().as_ref());
                }
            ));
            app.add_action(&about_action);
        }

        fn setup_accels(&self) {
            let app = self.obj();
            app.set_accels_for_action("app.preferences", &["<Control>comma"]);
            app.set_accels_for_action("win.show-help-overlay", &["<Control>question"]);
            app.set_accels_for_action("app.quit", &["<Control>q", "<Control>w"]);
            app.set_accels_for_action("win.timer-toggle", &["space"]);
        }
    }
}

use gtk::glib;

glib::wrapper! {
    pub struct MeditateApplication(ObjectSubclass<imp::MeditateApplication>)
        @extends adw::Application, gtk::Application, gtk::gio::Application,
        @implements gtk::gio::ActionGroup, gtk::gio::ActionMap;
}

impl MeditateApplication {
    pub fn new() -> Self {
        glib::Object::builder()
            .property("application-id", crate::config::APP_ID)
            .property("flags", gtk::gio::ApplicationFlags::FLAGS_NONE)
            .build()
    }
}

impl Default for MeditateApplication {
    fn default() -> Self {
        Self::new()
    }
}

impl MeditateApplication {
    /// Run a closure with a reference to the open database.
    /// Returns `None` if the database failed to open at startup.
    pub fn with_db<F, R>(&self, f: F) -> Option<R>
    where
        F: FnOnce(&crate::db::Database) -> R,
    {
        use glib::subclass::prelude::ObjectSubclassIsExt;
        let borrow = self.imp().db.borrow();
        borrow.as_ref().map(f)
    }
}
