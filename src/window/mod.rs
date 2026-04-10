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
}
