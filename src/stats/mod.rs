mod imp;

use gtk::glib;
use gtk::glib::subclass::prelude::ObjectSubclassIsExt;

glib::wrapper! {
    pub struct StatsView(ObjectSubclass<imp::StatsView>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl StatsView {
    pub fn refresh(&self) {
        self.imp().reload_all();
    }
}
