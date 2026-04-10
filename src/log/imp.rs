use std::cell::{Cell, RefCell};
use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::{glib, CompositeTemplate};

use crate::db::{Label, Session, SessionData, SessionFilter, SessionMode};

// ── GObject impl ──────────────────────────────────────────────────────────────

#[derive(Debug, Default, CompositeTemplate)]
#[template(resource = "/io/github/janekbt/Meditate/ui/log_view.ui")]
pub struct LogView {
    #[template_child] pub view_stack:   TemplateChild<gtk::Stack>,
    #[template_child] pub list_box:     TemplateChild<gtk::ListBox>,
    #[template_child] pub add_first_btn: TemplateChild<gtk::Button>,

    // Cached DB data
    sessions:        RefCell<Vec<Session>>,
    labels:          RefCell<Vec<Label>>,

    // Filter state
    pub filter_notes_only: Cell<bool>,
    pub filter_label_id:   Cell<Option<i64>>,
}

#[glib::object_subclass]
impl ObjectSubclass for LogView {
    const NAME: &'static str = "LogView";
    type Type = super::LogView;
    type ParentType = gtk::Widget;

    fn class_init(klass: &mut Self::Class) {
        klass.bind_template();
        klass.set_layout_manager_type::<gtk::BinLayout>();
    }

    fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
        obj.init_template();
    }
}

impl ObjectImpl for LogView {
    fn constructed(&self) {
        self.parent_constructed();

        // "Add Session" button in the empty state
        self.add_first_btn.connect_clicked(glib::clone!(
            #[weak(rename_to = this)] obj = self.obj(),
            move |_| this.imp().show_add_dialog()
        ));

        // Row activation → edit dialog
        self.list_box.connect_row_activated(glib::clone!(
            #[weak(rename_to = this)] obj = self.obj(),
            move |_, row| {
                let session_id = row.widget_name().parse::<i64>().unwrap_or(-1);
                if session_id >= 0 {
                    this.imp().show_edit_dialog(session_id);
                }
            }
        ));
    }

    fn dispose(&self) {
        self.obj().first_child().map(|w| w.unparent());
    }
}

impl WidgetImpl for LogView {}

// ── Public API ────────────────────────────────────────────────────────────────

impl LogView {
    /// Reload sessions from the database and rebuild the list.
    pub fn refresh(&self) {
        let app = match self.get_app() {
            Some(a) => a,
            None => return,
        };

        // Fetch labels first (needed for row display)
        let labels = app
            .with_db(|db| db.list_labels())
            .and_then(|r| r.ok())
            .unwrap_or_default();
        *self.labels.borrow_mut() = labels;

        // Fetch sessions with current filter
        let filter = SessionFilter {
            label_id: self.filter_label_id.get(),
            only_with_notes: self.filter_notes_only.get(),
        };
        let sessions = app
            .with_db(|db| db.list_sessions(&filter))
            .and_then(|r| r.ok())
            .unwrap_or_default();

        let has_filter = self.filter_notes_only.get() || self.filter_label_id.get().is_some();

        if sessions.is_empty() {
            self.view_stack.set_visible_child_name(
                if has_filter { "filtered-empty" } else { "empty" },
            );
            self.sessions.borrow_mut().clear();
            return;
        }

        // Rebuild the list
        while let Some(child) = self.list_box.first_child() {
            self.list_box.remove(&child);
        }

        let labels_ref = self.labels.borrow();
        for session in &sessions {
            let row = self.build_row(session, &labels_ref);
            self.list_box.append(&row);
        }

        *self.sessions.borrow_mut() = sessions;
        self.view_stack.set_visible_child_name("list");
    }

    /// Populate the label combo in the filter popover.
    pub fn refresh_filter_labels(&self, combo: &adw::ComboRow) {
        let labels = self.labels.borrow();
        let names: Vec<&str> = std::iter::once("All labels")
            .chain(labels.iter().map(|l| l.name.as_str()))
            .collect();
        combo.set_model(Some(&gtk::StringList::new(&names)));
        combo.set_selected(0);
    }

    pub fn show_add_dialog(&self) {
        self.show_session_dialog(None);
    }
}

// ── Row building ──────────────────────────────────────────────────────────────

impl LogView {
    fn build_row(&self, session: &Session, labels: &[Label]) -> adw::ActionRow {
        let label_name = session.label_id
            .and_then(|id| labels.iter().find(|l| l.id == id))
            .map(|l| l.name.as_str())
            .unwrap_or("");

        let duration = format_duration(session.duration_secs as u64);
        let date = format_date(session.start_time);
        let title = format!("{duration}  ·  {date}");

        let subtitle = match (label_name.is_empty(), session.note.as_deref()) {
            (false, Some(note)) if !note.is_empty() => format!("{label_name}  ·  {note}"),
            (false, _)  => label_name.to_owned(),
            (true, Some(note)) if !note.is_empty() => note.to_owned(),
            _ => String::new(),
        };

        let row = adw::ActionRow::builder()
            .title(&title)
            .activatable(true)
            // Store session id in widget name so row_activated can retrieve it.
            .name(&session.id.to_string())
            .build();

        if !subtitle.is_empty() {
            row.set_subtitle(&subtitle);
        }

        // Delete button
        let delete_btn = gtk::Button::builder()
            .icon_name("user-trash-symbolic")
            .valign(gtk::Align::Center)
            .tooltip_text("Delete session")
            .css_classes(["flat"])
            .build();

        let session_id = session.id;
        let obj = self.obj().clone();
        delete_btn.connect_clicked(move |btn| {
            obj.imp().on_delete_clicked(session_id, btn);
        });

        row.add_suffix(&delete_btn);
        row
    }
}

// ── Delete with undo toast ────────────────────────────────────────────────────

impl LogView {
    fn on_delete_clicked(&self, session_id: i64, delete_btn: &gtk::Button) {
        // Find and hide the row immediately (optimistic update)
        let row = delete_btn
            .parent()  // ActionRow
            .and_then(|r| r.parent()); // ListBoxRow wrapper (ActionRow IS a ListBoxRow)

        // ActionRow extends ListBoxRow, so the parent of the button is the ActionRow
        // which is itself a ListBoxRow — no extra wrapper needed.
        let action_row = delete_btn
            .parent()
            .and_downcast::<gtk::Widget>();

        if let Some(ref w) = action_row {
            w.set_visible(false);
        }

        let toast = adw::Toast::builder()
            .title("Session deleted")
            .button_label("Undo")
            .timeout(5)
            .build();

        // Undo: restore the row and don't delete from DB
        let widget_ref = action_row.clone();
        toast.connect_button_clicked(move |_| {
            if let Some(ref w) = widget_ref {
                w.set_visible(true);
            }
        });

        // When toast is dismissed without undo → actually delete from DB
        let obj = self.obj().clone();
        toast.connect_dismissed(move |_| {
            // Check if the row is still hidden (= undo was NOT pressed)
            let still_deleted = action_row
                .as_ref()
                .map(|w| !w.is_visible())
                .unwrap_or(true);

            if still_deleted {
                if let Some(app) = obj.imp().get_app() {
                    app.with_db(|db| db.delete_session(session_id));
                }
                obj.imp().refresh();
            }
        });

        self.add_toast(toast);
    }
}

// ── Add / Edit dialog ─────────────────────────────────────────────────────────

impl LogView {
    fn show_edit_dialog(&self, session_id: i64) {
        let session = self.sessions.borrow()
            .iter()
            .find(|s| s.id == session_id)
            .cloned();
        self.show_session_dialog(session.as_ref());
    }

    fn show_session_dialog(&self, session: Option<&Session>) {
        let is_edit = session.is_some();
        let labels = self.labels.borrow().clone();
        let session_id = session.map(|s| s.id);

        // ── Duration rows ──────────────────────────────────────────────
        let hours_row = adw::SpinRow::builder()
            .title("Hours")
            .adjustment(&gtk::Adjustment::new(0.0, 0.0, 23.0, 1.0, 5.0, 0.0))
            .build();
        let minutes_row = adw::SpinRow::builder()
            .title("Minutes")
            .adjustment(&gtk::Adjustment::new(0.0, 0.0, 59.0, 1.0, 5.0, 0.0))
            .build();

        if let Some(s) = session {
            hours_row.set_value((s.duration_secs / 3600) as f64);
            minutes_row.set_value(((s.duration_secs % 3600) / 60) as f64);
        }

        // ── Date row ───────────────────────────────────────────────────
        let date_row = adw::EntryRow::builder()
            .title("Date (YYYY-MM-DD)")
            .build();
        if let Some(s) = session {
            date_row.set_text(&format_date_iso(s.start_time));
        } else {
            date_row.set_text(&format_date_iso(unix_now()));
        }

        // ── Label row ──────────────────────────────────────────────────
        let label_names: Vec<&str> = std::iter::once("None")
            .chain(labels.iter().map(|l| l.name.as_str()))
            .collect();
        let label_row = adw::ComboRow::builder()
            .title("Label")
            .model(&gtk::StringList::new(&label_names))
            .build();

        if let Some(s) = session {
            let idx = s.label_id
                .and_then(|id| labels.iter().position(|l| l.id == id))
                .map(|i| (i + 1) as u32)
                .unwrap_or(0);
            label_row.set_selected(idx);
        }

        // ── Note row ───────────────────────────────────────────────────
        let note_row = adw::EntryRow::builder()
            .title("Note (optional)")
            .build();
        if let Some(s) = session {
            note_row.set_text(s.note.as_deref().unwrap_or(""));
        }

        // ── Assemble dialog ────────────────────────────────────────────
        let group = adw::PreferencesGroup::new();
        group.add(&hours_row);
        group.add(&minutes_row);
        group.add(&date_row);
        group.add(&label_row);
        group.add(&note_row);

        let scrolled = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&group)
            .build();

        let cancel_btn = gtk::Button::builder()
            .label("Cancel")
            .build();
        let save_btn = gtk::Button::builder()
            .label(if is_edit { "Save" } else { "Add" })
            .css_classes(["suggested-action"])
            .build();

        let header = adw::HeaderBar::new();
        header.pack_start(&cancel_btn);
        header.pack_end(&save_btn);

        let toolbar_view = adw::ToolbarView::new();
        toolbar_view.add_top_bar(&header);
        toolbar_view.set_content(Some(&scrolled));

        let dialog = adw::Dialog::builder()
            .title(if is_edit { "Edit Session" } else { "Add Session" })
            .content_width(360)
            .child(&toolbar_view)
            .build();

        // Cancel
        cancel_btn.connect_clicked(glib::clone!(
            #[weak] dialog,
            move |_| dialog.close()
        ));

        // Save
        let obj = self.obj().clone();
        save_btn.connect_clicked(glib::clone!(
            #[weak] dialog,
            #[weak] hours_row,
            #[weak] minutes_row,
            #[weak] date_row,
            #[weak] label_row,
            #[weak] note_row,
            move |_| {
                let imp = obj.imp();
                let duration = hours_row.value() as i64 * 3600
                    + minutes_row.value() as i64 * 60;
                let start_time = parse_date_iso(&date_row.text())
                    .unwrap_or_else(unix_now);
                let selected = label_row.selected() as usize;
                let label_id = if selected == 0 {
                    None
                } else {
                    imp.labels.borrow().get(selected - 1).map(|l| l.id)
                };
                let note_text = note_row.text();
                let note = if note_text.is_empty() { None } else { Some(note_text.to_string()) };

                let data = SessionData {
                    start_time,
                    duration_secs: duration.max(0),
                    mode: SessionMode::Countdown,
                    label_id,
                    note,
                };

                if let Some(app) = imp.get_app() {
                    if let Some(id) = session_id {
                        app.with_db(|db| db.update_session(id, &data));
                    } else {
                        app.with_db(|db| db.create_session(&data));
                    }
                }

                dialog.close();
                imp.refresh();
            }
        ));

        if let Some(win) = self.get_window() {
            dialog.present(Some(&win));
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

impl LogView {
    fn add_toast(&self, toast: adw::Toast) {
        if let Some(win) = self.get_window() {
            win.add_toast(toast);
        }
    }

    fn get_app(&self) -> Option<crate::application::MeditateApplication> {
        self.obj()
            .root()
            .and_then(|r| r.downcast::<gtk::Window>().ok())
            .and_then(|w| w.application())
            .and_then(|a| a.downcast::<crate::application::MeditateApplication>().ok())
    }

    fn get_window(&self) -> Option<crate::window::MeditateWindow> {
        self.obj()
            .root()
            .and_then(|r| r.downcast::<crate::window::MeditateWindow>().ok())
    }
}

// ── Formatting ────────────────────────────────────────────────────────────────

pub fn format_duration(secs: u64) -> String {
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m:02}:{s:02}")
    }
}

fn format_date(unix_secs: i64) -> String {
    glib::DateTime::from_unix_local(unix_secs)
        .ok()
        .and_then(|dt| dt.format("%b %d, %Y").ok())
        .map(|gs| gs.to_string())
        .unwrap_or_default()
}

fn format_date_iso(unix_secs: i64) -> String {
    glib::DateTime::from_unix_local(unix_secs)
        .ok()
        .and_then(|dt| dt.format("%Y-%m-%d").ok())
        .map(|gs| gs.to_string())
        .unwrap_or_default()
}

/// Parse "YYYY-MM-DD" into a unix timestamp (midnight local time).
fn parse_date_iso(s: &str) -> Option<i64> {
    let parts: Vec<&str> = s.trim().split('-').collect();
    if parts.len() != 3 {
        return None;
    }
    let y: i32 = parts[0].parse().ok()?;
    let m: i32 = parts[1].parse().ok()?;
    let d: i32 = parts[2].parse().ok()?;
    glib::DateTime::new_local(y, m, d, 0, 0, 0.0)
        .ok()
        .map(|dt| dt.to_unix())
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
