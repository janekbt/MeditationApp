mod imp;

use glib::prelude::IsA;
use gtk::{gio, glib};

glib::wrapper! {
    pub struct MeditateWindow(ObjectSubclass<imp::MeditateWindow>)
        @extends adw::ApplicationWindow, gtk::ApplicationWindow, gtk::Window, gtk::Widget,
        @implements gio::ActionGroup, gio::ActionMap, gtk::Accessible, gtk::Buildable,
                    gtk::ConstraintTarget, gtk::Native, gtk::Root, gtk::ShortcutManager;
}

impl MeditateWindow {
    pub fn new(app: &impl IsA<adw::Application>) -> Self {
        glib::Object::builder()
            .property("application", app)
            .build()
    }

    pub fn add_toast(&self, toast: adw::Toast) {
        use glib::subclass::prelude::ObjectSubclassIsExt;
        self.imp().add_toast(toast);
    }

    /// Push the interval-bell library page onto the navigation view.
    /// Triggered by the "Interval Bells" row in the timer setup; this
    /// wrapper keeps the timer module from having to know about the
    /// window's internal layout. The on_changed callback runs after
    /// any add / toggle inside the library so the timer's count
    /// subtitle stays in sync without the user having to leave + return.
    pub fn push_bells_page(&self, app: &crate::application::MeditateApplication) {
        use glib::subclass::prelude::ObjectSubclassIsExt;
        let timer_view = self.imp().timer_view.clone();
        crate::bells::push_bells_page(
            &self.imp().nav_view,
            app,
            move || timer_view.refresh_interval_bells_count(),
        );
    }

    /// Push the bell-sound chooser onto the navigation view.
    /// Triggered by every bell-sound row in the app — Starting Bell,
    /// per-bell row in the interval-bell edit page, End Bell. The
    /// caller's on_selected callback fires when the user taps a sound
    /// row, receives the chosen UUID, and the page pops automatically.
    pub fn push_sound_chooser(
        &self,
        app: &crate::application::MeditateApplication,
        current_uuid: Option<String>,
        on_selected: impl Fn(String) + 'static,
    ) {
        use glib::subclass::prelude::ObjectSubclassIsExt;
        crate::sounds::push_sounds_chooser(
            &self.imp().nav_view,
            app,
            current_uuid,
            on_selected,
        );
    }
}
