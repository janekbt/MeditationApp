//! The one name dialog (#358, #374): every create and rename of a
//! preset, label, guided file or bell sound. The import dialogs keep
//! their own frame and use its `NameRow`.

use std::rc::Rc;

use adw::prelude::*;
use meditate_core::db::DbError;
use meditate_core::naming::{NameField, NameNote};
use std::cell::RefCell;

use crate::i18n::gettext;

/// The line under a name field for core's note.
pub fn note_text(note: NameNote) -> Option<String> {
    match note {
        NameNote::None => None,
        NameNote::Taken => Some(gettext("This name is already in use")),
        NameNote::SaveFailed => Some(gettext("Save failed")),
    }
}

/// The name field and the line under it, run by core's `NameField`:
/// whether Save may run, and why not. Whether the name is free is
/// only a hint while typing; the write decides (`saved`).
pub struct NameRow {
    pub entry: gtk::Entry,
    hint: gtk::Label,
    /// The field and its hint, for the dialog's body.
    pub widget: gtk::Box,
    field: RefCell<NameField>,
    is_taken: Box<dyn Fn(&str) -> bool>,
    on_savable: Box<dyn Fn(bool)>,
}

impl NameRow {
    /// A create or import: `initial` (empty, or the picked file's
    /// name) is checked like any typed name. `on_savable` hears whether
    /// Save may run, now and on every change.
    pub fn new(initial: &str, is_taken: impl Fn(&str) -> bool + 'static, on_savable: impl Fn(bool) + 'static) -> Rc<Self> {
        Self::build(NameField::new(initial, &is_taken), is_taken, on_savable)
    }

    /// A rename: the item's own name `old` is never taken.
    pub fn renaming(old: &str, is_taken: impl Fn(&str) -> bool + 'static, on_savable: impl Fn(bool) + 'static) -> Rc<Self> {
        Self::build(NameField::renaming(old, &is_taken), is_taken, on_savable)
    }

    fn build(field: NameField, is_taken: impl Fn(&str) -> bool + 'static, on_savable: impl Fn(bool) + 'static) -> Rc<Self> {
        let initial = field.text.clone();
        let entry = gtk::Entry::builder().text(&initial).build();
        let hint = gtk::Label::builder()
            .css_classes(["error", "caption"])
            .halign(gtk::Align::Start)
            .wrap(true)
            .visible(false)
            .build();
        let widget = gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(4).build();
        widget.append(&entry);
        widget.append(&hint);
        let field = RefCell::new(field);
        let row = Rc::new(Self { entry, hint, widget, field, is_taken: Box::new(is_taken), on_savable: Box::new(on_savable) });
        row.show();
        let weak = Rc::downgrade(&row);
        row.entry.connect_changed(move |entry| {
            if let Some(row) = weak.upgrade() {
                row.field.borrow_mut().changed(&entry.text(), |n| (row.is_taken)(n));
                row.show();
            }
        });
        row
    }

    /// The typed name, trimmed.
    pub fn name(&self) -> String {
        self.entry.text().trim().to_string()
    }

    fn show(&self) {
        let field = self.field.borrow();
        (self.on_savable)(field.can_save());
        let text = note_text(field.note);
        self.hint.set_visible(text.is_some());
        self.hint.set_label(&text.unwrap_or_default());
    }

    /// A converted import's store failed: the field and Import come
    /// back for a retry, and the line under the field says why (a
    /// taken name turns Import off again).
    pub fn import_failed(&self, import_btn: &gtk::Button, tag: &str, result: Option<Result<(), DbError>>) {
        self.entry.set_sensitive(true);
        import_btn.set_label(&gettext("Import"));
        import_btn.set_sensitive(true);
        self.saved(tag, result);
    }

    /// The write's result (`None`: no database): `true` when it
    /// landed. Otherwise the line under the field says why, and the
    /// name stays; a name taken meanwhile (a sync) turns Save off,
    /// any other failure leaves it on for a retry.
    pub fn saved(&self, tag: &str, result: Option<Result<(), DbError>>) -> bool {
        let landed = self.field.borrow_mut().confirm(tag, result);
        self.show();
        landed
    }
}

/// A converted import whose store failed, with its file: the next
/// Import stores it again, and Cancel removes the file.
pub type PendingImport<T> = Rc<RefCell<Option<(T, std::path::PathBuf)>>>;

/// An empty `PendingImport` whose file goes when `dialog` is cancelled.
pub fn pending_import<T: 'static>(dialog: &adw::AlertDialog) -> PendingImport<T> {
    let pending: PendingImport<T> = Rc::default();
    let on_cancel = pending.clone();
    dialog.connect_response(Some("cancel"), move |_, _| {
        if let Some((_, path)) = on_cancel.borrow_mut().take() {
            let _ = std::fs::remove_file(path);
        }
    });
    pending
}

/// `is_taken` from one of core's `is_*_name_taken_from_db` checks. A
/// failed read, or no database, answers "free": the write decides.
pub fn taken_in(
    app: &crate::application::MeditateApplication,
    check: impl Fn(&crate::db::Database, &str) -> meditate_core::db::Result<bool> + 'static,
) -> impl Fn(&str) -> bool + 'static {
    let app = app.clone();
    move |name| app.with_db(|db| check(db, name)).and_then(Result::ok).unwrap_or(false)
}

/// Open the name dialog over `anchor`. Save (`confirm_label`) is off
/// while the trimmed name is empty or `is_taken`; `save` gets the
/// trimmed name (its `None`: no database), and the dialog closes only
/// once it landed. `placeholder` shows in an empty field.
pub fn name_dialog(
    anchor: &impl IsA<gtk::Widget>,
    heading: &str,
    initial: &str,
    placeholder: &str,
    confirm_label: &str,
    is_taken: impl Fn(&str) -> bool + 'static,
    save: impl Fn(&str) -> Option<Result<(), DbError>> + 'static,
) -> adw::AlertDialog {
    let dialog = adw::AlertDialog::builder()
        .heading(heading)
        .close_response("cancel")
        .default_response("save")
        // A response doesn't close it: a failed save keeps the name.
        .can_close(false)
        .build();
    dialog.add_response("cancel", &gettext("Cancel"));
    dialog.add_response("save", confirm_label);
    dialog.set_response_appearance("save", adw::ResponseAppearance::Suggested);
    let weak = dialog.downgrade();
    // A create opens empty, so treating it as a rename changes nothing.
    let row = NameRow::renaming(initial, is_taken, move |savable| {
        if let Some(dialog) = weak.upgrade() {
            dialog.set_response_enabled("save", savable);
        }
    });
    row.entry.set_activates_default(true);
    row.entry.set_placeholder_text(Some(placeholder));
    dialog.set_extra_child(Some(&row.widget));
    let entry = row.entry.clone();
    // A response button also attempts a close, before its response:
    // only an attempt without one (Escape) closes, once both have run.
    // Closing may report the close response once more; `closing`
    // makes every later response a no-op.
    let answered = Rc::new(std::cell::Cell::new(false));
    let closing = Rc::new(std::cell::Cell::new(false));
    let (answered_by_button, closing_by_button) = (answered.clone(), closing.clone());
    // The diagnostics log names the dialog the failure came from.
    let tag = format!("name.save: {heading}");
    dialog.connect_response(None, move |dialog, id| {
        if closing_by_button.get() {
            return;
        }
        answered_by_button.set(true);
        // Its close attempt's idle (if any) runs first and reads the
        // mark; a response without one must not leave it set, or the
        // next Escape would be swallowed.
        let answered = answered_by_button.clone();
        gtk::glib::idle_add_local_once(move || answered.set(false));
        if id != "save" || row.saved(&tag, save(&row.name())) {
            closing_by_button.set(true);
            dialog.force_close();
        }
    });
    dialog.connect_close_attempt(move |dialog| {
        let dialog = dialog.downgrade();
        let (answered, closing) = (answered.clone(), closing.clone());
        gtk::glib::idle_add_local_once(move || {
            if !answered.replace(false) && !closing.get() {
                if let Some(dialog) = dialog.upgrade() {
                    closing.set(true);
                    dialog.force_close();
                }
            }
        });
    });
    dialog.present(Some(anchor));
    // Selected, so typing replaces it.
    entry.grab_focus();
    entry.select_region(0, -1);
    dialog
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    fn descendants(root: &gtk::Widget) -> Vec<gtk::Widget> {
        let mut all = vec![root.clone()];
        let mut i = 0;
        while i < all.len() {
            let mut child = all[i].first_child();
            while let Some(c) = child {
                child = c.next_sibling();
                all.push(c);
            }
            i += 1;
        }
        all
    }

    fn settle() {
        while gtk::glib::MainContext::default().iteration(false) {}
    }

    /// A dialog shown in a window, its entry, its hint line, and
    /// whether it has closed.
    struct Shown {
        dialog: adw::AlertDialog,
        entry: gtk::Entry,
        closed: Rc<Cell<bool>>,
        _window: adw::Window,
    }

    impl Shown {
        fn new(
            initial: &str,
            is_taken: impl Fn(&str) -> bool + 'static,
            save: impl Fn(&str) -> Result<(), DbError> + 'static,
        ) -> Self {
            let save = move |name: &str| Some(save(name));
            let window = adw::Window::new();
            window.present();
            let dialog = name_dialog(&window, "Rename Sound", initial, "", "Rename", is_taken, save);
            let closed = Rc::new(Cell::new(false));
            let flag = closed.clone();
            dialog.connect_closed(move |_| flag.set(true));
            settle();
            let entry = descendants(dialog.upcast_ref()).into_iter().find_map(|w| w.downcast::<gtk::Entry>().ok()).unwrap();
            Self { dialog, entry, closed, _window: window }
        }

        fn type_name(&self, text: &str) {
            self.entry.set_text(text);
            settle();
        }

        fn can_save(&self) -> bool {
            self.dialog.is_response_enabled("save")
        }

        /// The line under the field, when shown.
        fn hint(&self) -> Option<String> {
            descendants(self.dialog.upcast_ref())
                .into_iter()
                .filter_map(|w| w.downcast::<gtk::Label>().ok())
                .find(|l| l.has_css_class("error"))
                .filter(|l| l.is_visible())
                .map(|l| l.label().to_string())
        }

        /// Click a response button, as the user does.
        fn click(&self, label: &str) {
            let button = descendants(self.dialog.upcast_ref())
                .into_iter()
                .filter_map(|w| w.downcast::<gtk::Button>().ok())
                .find(|b| b.label().as_deref() == Some(label))
                .unwrap_or_else(|| panic!("no {label} button"));
            button.emit_clicked();
            settle();
        }
    }

    #[test]
    fn a_taken_name_turns_save_off_and_says_why() {
        crate::gtk_test::run(a_taken_name);
    }

    fn a_taken_name() {
        let shown = Shown::new("Gong", |n| n.eq_ignore_ascii_case("Pulse"), |_| Ok(()));
        assert!(shown.can_save(), "a rename opens with its own name");
        assert_eq!(shown.hint(), None);
        shown.type_name("pulse ");
        assert!(!shown.can_save());
        assert_eq!(shown.hint().as_deref(), Some("This name is already in use"));
        shown.type_name("Heartbeat");
        assert!(shown.can_save());
        assert_eq!(shown.hint(), None);
        shown.type_name("   ");
        assert!(!shown.can_save(), "empty: Save off");
        assert_eq!(shown.hint(), None, "and no hint");
        shown.click("Cancel");
        assert!(shown.closed.get(), "Cancel closes");
        // Escape asks the dialog to close.
        let shown = Shown::new("Gong", |_| false, |_| Ok(()));
        shown.dialog.close();
        settle();
        assert!(shown.closed.get(), "Escape closes");
    }

    /// An import opens with the picked file's name, and it is checked
    /// at once, before the conversion runs.
    #[test]
    fn an_import_checks_the_picked_files_name() {
        crate::gtk_test::run(|| {
            let savable = Rc::new(Cell::new(true));
            let seen = savable.clone();
            let row = NameRow::new("Pulse", |n| n.eq_ignore_ascii_case("pulse"), move |s| seen.set(s));
            assert!(!savable.get());
            let hint = descendants(row.widget.upcast_ref()).into_iter().filter_map(|w| w.downcast::<gtk::Label>().ok()).find(|l| l.is_visible());
            assert_eq!(hint.map(|l| l.label().to_string()).as_deref(), Some("This name is already in use"));
        });
    }

    #[test]
    fn a_failed_save_keeps_the_name_dialog_open() {
        crate::gtk_test::run(a_failed_save);
    }

    fn a_failed_save() {
        let results = Rc::new(RefCell::new(vec![
            Ok(()),
            Err(DbError::DuplicateLabel("Walk".into())),
            Err(DbError::Decode("database is locked".into())),
        ]));
        let saved = Rc::new(RefCell::new(Vec::new()));
        let shown = {
            let saved = saved.clone();
            Shown::new("", |_| false, move |name| {
                saved.borrow_mut().push(name.to_string());
                results.borrow_mut().pop().unwrap()
            })
        };
        shown.type_name(" Walk ");
        // The database can't be written: open, the name kept, Save on.
        shown.click("Rename");
        assert!(!shown.closed.get());
        assert_eq!(shown.entry.text(), " Walk ");
        assert_eq!(shown.hint().as_deref(), Some("Save failed"));
        assert!(shown.can_save());
        // Taken meanwhile (a sync): the hint, Save off.
        shown.click("Rename");
        assert!(!shown.closed.get());
        assert_eq!(shown.hint().as_deref(), Some("This name is already in use"));
        assert!(!shown.can_save());
        // Another name, and the retry works.
        shown.type_name("Stroll");
        shown.click("Rename");
        assert!(shown.closed.get());
        assert_eq!(*saved.borrow(), ["Walk", "Walk", "Stroll"]);
        // After a failed save, one Escape still closes.
        let shown = Shown::new("", |_| false, |_| Err(DbError::Decode("database is locked".into())));
        shown.type_name("Walk");
        // A response with no close attempt before it (sent by code).
        shown.dialog.emit_by_name::<()>("response", &[&"save"]);
        settle();
        assert!(!shown.closed.get());
        shown.dialog.close();
        settle();
        assert!(shown.closed.get(), "Escape after a failed save closes");
    }
}
