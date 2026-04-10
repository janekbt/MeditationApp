mod imp {
    use adw::prelude::*;
    use adw::subclass::prelude::*;
    use gtk::{gio, glib};

    use crate::config;
    use crate::window::MeditateWindow;

    #[derive(Debug, Default)]
    pub struct MeditateApplication {}

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
                    // TODO Phase 6
                    let _ = app;
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
            app.set_accels_for_action("app.quit", &["<Control>q"]);
        }
    }
}

use adw::subclass::prelude::*;
use gtk::glib;
use gtk::prelude::*;

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
