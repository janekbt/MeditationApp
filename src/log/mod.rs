mod imp;

use gtk::glib;
use gtk::glib::subclass::prelude::ObjectSubclassIsExt;

glib::wrapper! {
    pub struct LogView(ObjectSubclass<imp::LogView>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl LogView {
    /// Rebuild the session feed, but only when the app-level log flag is
    /// set. Tab switches trigger this on every visibility change; the flag
    /// prevents re-issuing the `list_sessions` query each time.
    pub fn refresh(&self) {
        if let Some(app) = self.imp().get_app() {
            if !app.log_dirty() { return; }
            self.imp().refresh();
            app.clear_log_dirty();
        } else {
            self.imp().refresh();
        }
    }

    /// Incremental append of a just-saved session to the top of the feed —
    /// skips the full rebuild that plain `refresh()` does. Called after a
    /// timer-view session save; keeps the log view in sync without tearing
    /// down and re-querying 15 cards. Skips work if the view hasn't been
    /// populated yet (first log-tab entry will pull fresh from DB).
    pub fn prepend_session(&self, session: crate::db::Session) {
        // If log is dirty, the next public refresh() will re-query the DB
        // and our prepend would be discarded. Skip it.
        if let Some(app) = self.imp().get_app() {
            if app.log_dirty() { return; }
        }
        self.imp().prepend_session(session);
    }

    pub fn show_add_dialog(&self) {
        self.imp().show_add_dialog();
    }

    pub fn set_filter_notes_only(&self, value: bool) {
        self.imp().filter_notes_only.set(value);
        self.invalidate_for_filter();
    }

    pub fn set_filter_label_id(&self, id: Option<i64>) {
        self.imp().filter_label_id.set(id);
        self.invalidate_for_filter();
    }

    /// A filter change doesn't touch the DB but DOES invalidate the rows
    /// currently shown — without this, the caller's follow-up `refresh()`
    /// would no-op because the dirty flag stays false.
    fn invalidate_for_filter(&self) {
        if let Some(app) = self.imp().get_app() {
            app.invalidate(crate::application::InvalidateScope::LOG);
        }
    }

    /// Populate the filter popover's label combo with current DB labels.
    pub fn refresh_filter_labels(&self, combo: &adw::ComboRow) {
        self.imp().refresh_filter_labels(combo);
    }
}
