mod imp;

use gtk::glib;
use gtk::glib::prelude::*;
use gtk::glib::subclass::prelude::ObjectSubclassIsExt;

glib::wrapper! {
    pub struct LogView(ObjectSubclass<imp::LogView>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl LogView {
    pub fn refresh(&self) {
        self.imp().refresh();
    }

    pub fn show_add_dialog(&self) {
        self.imp().show_add_dialog();
    }

    pub fn set_filter_notes_only(&self, value: bool) {
        self.imp().filter_notes_only.set(value);
    }

    pub fn set_filter_label_id(&self, id: Option<i64>) {
        self.imp().filter_label_id.set(id);
    }

    /// Populate the filter popover's label combo with current DB labels.
    pub fn refresh_filter_labels(&self, combo: &adw::ComboRow) {
        self.imp().refresh_filter_labels(combo);
    }
}
