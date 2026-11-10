mod imp;

use gtk::glib;
use gtk::glib::subclass::prelude::ObjectSubclassIsExt;

glib::wrapper! {
    pub struct StatsView(ObjectSubclass<imp::StatsView>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl StatsView {
    /// Re-run the aggregations, but only if the app-level stats flag says
    /// something changed. Tab switches fire this on every visibility change;
    /// the flag prevents that from re-issuing 10+ DB queries every time.
    pub fn refresh(&self) {
        // No app handle yet (early startup): still populate once.
        if self.imp().get_app().map_or(true, |app| app.take_stats_dirty()) {
            self.imp().reload_all();
        }
    }
}
