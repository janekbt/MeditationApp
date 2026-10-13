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
    /// Delete the rows still waiting for Undo, for a window close: the
    /// toast's own dismiss then finds no app and would drop them.
    pub fn commit_all_pending(&self) {
        self.imp().commit_all_pending();
    }

    pub fn prepend_session(&self, session: crate::db::Session) {
        // If log is dirty, the next public refresh() will re-query the DB
        // and our prepend would be discarded. Skip it.
        if let Some(app) = self.imp().get_app() {
            if app.log_dirty() { return; }
            // Whether the session matches a filter is the query's call.
            if self.imp().filter_notes_only.get() || self.imp().filter_label_id.get().is_some() {
                app.invalidate(crate::application::InvalidateScope::LOG);
                return;
            }
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

#[cfg(test)]
mod tests {
    //! The Log needs a display, which the test host lacks, so these
    //! read the production source.
    fn production(file: &str) -> String {
        let src = std::fs::read_to_string(format!("{}/src/{file}", env!("CARGO_MANIFEST_DIR"))).unwrap();
        src[..src.find("#[cfg(test)]").unwrap_or(src.len())].to_string()
    }

    fn body<'a>(src: &'a str, sig: &str) -> &'a str {
        let at = src.find(sig).unwrap_or_else(|| panic!("{sig} not found"));
        let end = src[at + 1..].find("\n    pub fn ").into_iter()
            .chain(src[at + 1..].find("\n    fn "))
            .chain(src[at + 1..].find("\n    pub(super) fn "))
            .min().map_or(src.len(), |e| at + 1 + e);
        &src[at..end]
    }

    /// Filtered to "Yoga", a saved "Zen" session showed up and Load
    /// more skipped a Yoga session.
    #[test]
    fn a_save_under_a_filter_reloads_instead_of_adding() {
        let src = production("log/mod.rs");
        let b = body(&src, "pub fn prepend_session(");
        let filter = b.find("filter_label_id.get().is_some()").expect("checks the label filter");
        assert!(b[..filter].contains("filter_notes_only.get()"), "and the notes filter");
        let invalidate = b.find("InvalidateScope::LOG").expect("marks the Log stale");
        let add = b.find("self.imp().prepend_session(").unwrap();
        assert!(filter < invalidate && invalidate < add);
        assert!(b[invalidate..add].contains("return;"), "no card is added");
    }

    /// The dropdown came back at "All labels" while still filtering,
    /// and after a rename picked the wrong label.
    #[test]
    fn the_filter_shows_and_maps_the_labels_it_was_built_from() {
        let imp = production("log/imp.rs");
        let b = body(&imp, "pub fn refresh_filter_labels(");
        assert!(b.contains("*self.filter_label_ids.borrow_mut() ="), "remembers what it shows");
        assert!(b.contains("combo.set_selected(filter_position(&ids, self.filter_label_id.get()))"), "selects the active filter");
        let on = b.find("self.building_filter.set(true)").expect("guards its own rebuild");
        let model = b.find("combo.set_model(").unwrap();
        let off = b.find("self.building_filter.set(false)").unwrap();
        assert!(on < model && model < off);
        assert!(b.contains("list_labels"), "current names, not the ones of the last reload");

        let win = production("window/imp.rs");
        let h = &win[win.find("self.filter_label_row.connect_notify_local(").unwrap()..];
        let h = &h[..h.find("\n        );").unwrap()];
        assert!(h.contains("filter_label_ids.borrow().get(selected - 1)"), "maps through the shown ids");
        assert!(!h.contains("labels.borrow()"));
        assert!(h.contains("building_filter.get()"));
        assert!(h.contains("label_id == log.filter_label_id.get()"), "no reload when nothing changed");
        assert!(win.contains("popover.connect_show("), "rebuilt each time the filter opens");
    }

    /// Android clears a filter whose label is gone; GTK kept filtering
    /// by an id no entry named.
    #[test]
    fn a_deleted_filter_label_clears_the_filter() {
        let imp = production("log/imp.rs");
        let b = body(&imp, "pub fn refresh_filter_labels(");
        let gone = b.find("!labels.iter().any(|l| l.id == id)").expect("checks the label still exists");
        let clear = b.find("self.obj().set_filter_label_id(None)").expect("clears the filter");
        let reload = b.find("self.obj().refresh()").expect("reloads the Log");
        assert!(gone < clear && clear < reload);
        // A sync that deletes the label reloads the Log on screen.
        let r = body(&imp, "pub fn refresh(&self)");
        let gone = r.find("!labels.iter().any(|l| l.id == id)").expect("every reload checks the label");
        assert!(r[..gone].contains("if let (Some(labels), Some(id)) = (&read, self.filter_label_id.get())"),
            "a failed read is no deleted label");
        let cleared = r.find("self.filter_label_id.set(None)").unwrap();
        assert!(gone < cleared && cleared < r.find("self.load_page(&app)").unwrap());
    }

    #[test]
    fn the_dropdown_selects_the_active_label() {
        let ids = [7, 3, 9];
        assert_eq!(super::imp::filter_position(&ids, None), 0, "All labels");
        assert_eq!(super::imp::filter_position(&ids, Some(3)), 2, "after All labels");
        assert_eq!(super::imp::filter_position(&ids, Some(4)), 0, "unknown id");
    }

    /// A note-only edit moved 07:12:45 to 07:12:00 and synced it.
    #[test]
    fn an_edit_keeps_the_start_seconds() {
        let imp = production("log/imp.rs");
        let b = body(&imp, "fn show_session_dialog(");
        assert!(b.contains("let original_second = session"), "taken from the session being edited");
        let start = &b[b.find("let start_time =").unwrap()..];
        let start = &start[..start.find(";\n").unwrap()];
        assert!(start.contains("original_second as u32)"), "used for the start");
        assert!(start.contains("meditate_core::time::local_naive_to_unix"), "converted like Android");
        assert!(!start.contains("0.0"), "no longer built at second 0");
    }

    /// Cards add rounded minutes; the delete took raw seconds off.
    #[test]
    fn a_delete_subtracts_what_the_card_added() {
        let imp = production("log/imp.rs");
        let b = body(&imp, "fn commit_delete_in_place(");
        assert!(b.contains("sec.total_secs.get() - meditate_core::format::log_card_total_secs(session.duration_secs)"));
    }

    /// A rebuild during Undo brought the card back and a second delete
    /// counted it twice.
    #[test]
    fn a_pending_delete_stays_hidden_and_counts_once() {
        let imp = production("log/imp.rs");
        let b = body(&imp, "fn commit_delete_in_place(");
        let found = b.find("let Some(card) = self.cards_by_id.borrow_mut().remove(&session_id) else { return; };")
            .expect("only a card on screen is taken off");
        assert!(found < b.find("sec.count.set(").unwrap(), "before any count changes");
        for sig in ["fn append_session_to_feed(", "pub fn prepend_session("] {
            assert!(body(&imp, sig).contains("card.set_visible(!self.is_pending_delete(session.id));"), "{sig}");
        }
    }
}
