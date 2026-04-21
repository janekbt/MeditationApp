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
        if let Some(app) = self.imp().get_app() {
            if !app.stats_dirty() { return; }
            self.imp().reload_all();
            app.clear_stats_dirty();
        } else {
            // No app handle yet (early startup) — still populate once.
            self.imp().reload_all();
        }
    }
}
