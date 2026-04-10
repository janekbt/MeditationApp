mod imp;

use adw::subclass::prelude::*;
use gtk::{gio, glib};

glib::wrapper! {
    pub struct MeditateWindow(ObjectSubclass<imp::MeditateWindow>)
        @extends adw::ApplicationWindow, gtk::ApplicationWindow, gtk::Window, gtk::Widget,
        @implements gio::ActionGroup, gio::ActionMap, gtk::Accessible, gtk::Buildable,
                    gtk::ConstraintTarget, gtk::Native, gtk::Root, gtk::ShortcutManager;
}

impl MeditateWindow {
    pub fn new(app: &impl glib::IsA<adw::Application>) -> Self {
        glib::Object::builder()
            .property("application", app)
            .build()
    }
}
