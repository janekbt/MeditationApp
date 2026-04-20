use std::cell::{Cell, RefCell};
use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::{glib, CompositeTemplate};

use crate::db::{Label, Session, SessionData, SessionFilter, SessionMode};

// ── GObject impl ──────────────────────────────────────────────────────────────

#[derive(Debug, Default, CompositeTemplate)]
#[template(resource = "/io/github/janekbt/Meditate/ui/log_view.ui")]
pub struct LogView {
    #[template_child] pub view_stack:     TemplateChild<gtk::Stack>,
    #[template_child] pub feed_box:       TemplateChild<gtk::Box>,
    #[template_child] pub load_more_btn:  TemplateChild<gtk::Button>,
    #[template_child] pub add_first_btn:  TemplateChild<gtk::Button>,

    // Cached DB data
    sessions:        RefCell<Vec<Session>>,
    pub labels:      RefCell<Vec<Label>>,

    // Filter state
    pub filter_notes_only: Cell<bool>,
    pub filter_label_id:   Cell<Option<i64>>,

    // Pagination: how many rows are currently rendered. Rebuilding 2000+
    // rows upfront makes the whole app sluggish, so we load in pages and
    // append more on demand.
    loaded_count: Cell<usize>,

    /// Key (`YYYY-MM-DD`) of the section currently being filled — lets
    /// load_more extend the last-loaded day across a page boundary
    /// instead of starting a new header.
    current_section_key: RefCell<Option<String>>,

    /// All visible sections, keyed by their local-date string. Lets the
    /// delete-undo flow find the right section when a card is removed.
    sections_by_key: RefCell<std::collections::HashMap<String, DateSection>>,

    /// session_id → card widget. Needed for both in-place edit and
    /// in-place delete so neither has to rebuild the whole feed (which
    /// resets the scroll position).
    cards_by_id: RefCell<std::collections::HashMap<i64, gtk::Box>>,
}

/// One "Today" / "Yesterday" / "Apr 17" group in the feed.
#[derive(Debug, Clone)]
struct DateSection {
    /// Outer section Gtk.Box — used to remove the whole group when the
    /// last card in it gets deleted.
    outer: gtk::Box,
    /// Subtitle under the date header: "3 sessions · 1h 04m".
    caption: gtk::Label,
    /// Vertical Gtk.Box that holds the cards.
    cards_box: gtk::Box,
    /// Number of cards currently in the section. Interior mutability so
    /// the HashMap can store the section by value without &mut dances.
    count: Cell<u32>,
    total_secs: Cell<i64>,
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

        let obj = self.obj();

        // "Add Session" button in the empty state
        self.add_first_btn.connect_clicked(glib::clone!(
            #[weak(rename_to = this)] obj,
            move |_| this.imp().show_add_dialog()
        ));

        // "Load more" appends the next page of rows.
        self.load_more_btn.connect_clicked(glib::clone!(
            #[weak(rename_to = this)] obj,
            move |_| this.imp().load_more()
        ));
    }

    fn dispose(&self) {
        if let Some(w) = self.obj().first_child() { w.unparent() }
    }
}

impl WidgetImpl for LogView {}

// ── Public API ────────────────────────────────────────────────────────────────

impl LogView {
    /// Reload sessions from the database and rebuild the feed from scratch.
    /// Loads just the first page; additional pages come via `load_more`.
    pub fn refresh(&self) {
        let Some(app) = self.get_app() else { return; };

        // Fetch labels first (needed for card rendering)
        let labels = app
            .with_db(|db| db.list_labels())
            .and_then(|r| r.ok())
            .unwrap_or_default();
        *self.labels.borrow_mut() = labels;

        // Reset pagination + DOM + section tracking.
        self.loaded_count.set(0);
        self.sessions.borrow_mut().clear();
        self.cards_by_id.borrow_mut().clear();
        self.sections_by_key.borrow_mut().clear();
        *self.current_section_key.borrow_mut() = None;
        while let Some(child) = self.feed_box.first_child() {
            self.feed_box.remove(&child);
        }

        let loaded = self.load_page(&app);

        let has_filter = self.filter_notes_only.get() || self.filter_label_id.get().is_some();
        if loaded == 0 {
            self.view_stack.set_visible_child_name(
                if has_filter { "filtered-empty" } else { "empty" },
            );
            self.load_more_btn.set_visible(false);
            return;
        }
        self.view_stack.set_visible_child_name("list");
    }

    /// Append the next page to the existing feed without rebuilding anything.
    pub fn load_more(&self) {
        let Some(app) = self.get_app() else { return; };
        self.load_page(&app);
    }

    /// Query the next page of sessions and append them, grouping by date.
    /// Returns how many rows were appended; also toggles `load_more_btn`
    /// visibility based on whether the query returned a full page.
    fn load_page(&self, app: &crate::application::MeditateApplication) -> usize {
        const PAGE_SIZE: u32 = 200;

        let filter = SessionFilter {
            label_id:        self.filter_label_id.get(),
            only_with_notes: self.filter_notes_only.get(),
            limit:           Some(PAGE_SIZE),
            offset:          Some(self.loaded_count.get() as u32),
        };
        let page = app
            .with_db(|db| db.list_sessions(&filter))
            .and_then(|r| r.ok())
            .unwrap_or_default();

        let n = page.len();
        if n == 0 {
            self.load_more_btn.set_visible(false);
            return 0;
        }

        let labels_ref = self.labels.borrow();
        let label_map: std::collections::HashMap<i64, &str> =
            labels_ref.iter().map(|l| (l.id, l.name.as_str())).collect();
        for session in &page {
            self.append_session_to_feed(session, &label_map);
        }
        drop(labels_ref);

        self.loaded_count.set(self.loaded_count.get() + n);
        self.sessions.borrow_mut().extend(page);

        self.load_more_btn.set_visible(n == PAGE_SIZE as usize);
        n
    }

    /// Add a session to either the current date-section (extending its
    /// counter + caption) or start a new section. Called once per row
    /// from `load_page`.
    fn append_session_to_feed(
        &self,
        session: &Session,
        label_map: &std::collections::HashMap<i64, &str>,
    ) {
        let key = date_group_key(session.start_time);

        // Create a new section if this date just rolled over.
        let need_new = self.current_section_key.borrow().as_deref() != Some(&key);
        if need_new {
            let (section_box, caption_label, cards_box) = build_section_frame(session.start_time);
            self.feed_box.append(&section_box);
            let section = DateSection {
                outer:      section_box,
                caption:    caption_label,
                cards_box,
                count:      Cell::new(0),
                total_secs: Cell::new(0),
            };
            self.sections_by_key.borrow_mut().insert(key.clone(), section);
            *self.current_section_key.borrow_mut() = Some(key.clone());
        }

        // Append the card into the (possibly just-created) section and
        // bump its counters.
        let sections = self.sections_by_key.borrow();
        let sec = sections.get(&key).expect("section populated above");
        let card = build_card(session, label_map);
        sec.cards_box.append(&card);
        self.cards_by_id.borrow_mut().insert(session.id, card);
        sec.count.set(sec.count.get() + 1);
        sec.total_secs.set(sec.total_secs.get() + session.duration_secs);
        sec.caption.set_label(
            &section_caption_text(sec.count.get(), sec.total_secs.get()),
        );
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

// ── Pure builders (no `self`) ─────────────────────────────────────────────────

/// Build the date-section scaffold: a vertical Gtk.Box holding the header
/// row (date + caption) plus an empty vertical `cards_box` for sessions
/// to be appended into. Returns `(outer_box, caption_label, cards_box)`.
fn build_section_frame(unix_secs: i64) -> (gtk::Box, gtk::Label, gtk::Box) {
    let outer = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .build();

    let header = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .margin_start(2)
        .build();
    let date_label = gtk::Label::builder()
        .label(date_group_display(unix_secs))
        .css_classes(["heading"])
        .halign(gtk::Align::Start)
        .build();
    let caption = gtk::Label::builder()
        .label("")
        .css_classes(["caption", "dimmed"])
        .halign(gtk::Align::Start)
        .build();
    header.append(&date_label);
    header.append(&caption);

    let cards_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .build();

    outer.append(&header);
    outer.append(&cards_box);
    (outer, caption, cards_box)
}

/// One session card. Uses bare Gtk widgets (not AdwActionRow) so we can
/// render a colored left stripe + hero duration + label chip + quoted
/// note in a single, cheap widget tree — critical for 2000+ session logs.
fn build_card(session: &Session, label_map: &std::collections::HashMap<i64, &str>) -> gtk::Box {
    let label_name = session.label_id
        .and_then(|id| label_map.get(&id).copied())
        .unwrap_or("");
    let color_cls = if label_name.is_empty() {
        "log-c-none"
    } else {
        label_color_class(label_name)
    };

    // Colored left stripe — 3 px wide, inset from the top + bottom so
    // the card's rounded corners stay free, matching the mockup.
    let stripe = gtk::Box::builder()
        .width_request(3)
        .margin_top(10)
        .margin_bottom(10)
        .margin_start(6)
        .css_classes(["log-stripe", color_cls])
        .build();

    // Left column: big duration, "MIN" unit, time-of-day.
    let mins = (session.duration_secs.max(0) as u64 + 30) / 60;
    let dur_label = gtk::Label::builder()
        .label(mins.max(1).to_string())
        .css_classes(["log-duration", "numeric"])
        .halign(gtk::Align::Start)
        .build();
    let unit_label = gtk::Label::builder()
        // Lowercase in source; the .log-unit CSS class renders it uppercase
        // (HIG forbids all-caps in source text).
        .label(crate::i18n::gettext("min"))
        .css_classes(["log-unit"])
        .halign(gtk::Align::Start)
        .build();
    let time_label = gtk::Label::builder()
        .label(format_time_of_day(session.start_time))
        .css_classes(["log-time", "numeric"])
        .halign(gtk::Align::Start)
        .build();
    let left_col = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .width_request(64)
        .build();
    left_col.append(&dur_label);
    left_col.append(&unit_label);
    left_col.append(&time_label);

    // Right column: label chip + note/placeholder.
    let right_col = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(7)
        .hexpand(true)
        .build();

    if !label_name.is_empty() {
        right_col.append(&build_label_chip(label_name, color_cls));
    }

    let note_text = session.note.as_deref().unwrap_or("").trim();
    if note_text.is_empty() {
        let placeholder = gtk::Label::builder()
            .label(crate::i18n::gettext("No note added"))
            .css_classes(["log-note-placeholder"])
            .halign(gtk::Align::Start)
            .xalign(0.0)
            .build();
        right_col.append(&placeholder);
    } else {
        let note_label = gtk::Label::builder()
            .label(note_text)
            .css_classes(["log-note"])
            .halign(gtk::Align::Fill)
            .xalign(0.0)
            .wrap(true)
            .wrap_mode(gtk::pango::WrapMode::WordChar)
            .lines(2)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build();
        right_col.append(&note_label);
    }

    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .margin_top(12)
        .margin_bottom(12)
        // Less leading margin here because the stripe already carries
        // its own margin-start (the gap between the card edge and the
        // accent strip) + 3 px of actual stripe width.
        .margin_start(6)
        .margin_end(4)
        .build();
    content.append(&left_col);
    content.append(&right_col);

    // Delete button — sits as a direct child of the card so we can
    // vertically centre it against the whole card height, not just the
    // right column's content.
    let delete_btn = gtk::Button::builder()
        .icon_name("user-trash-symbolic")
        .css_classes(["flat", "circular"])
        .valign(gtk::Align::Center)
        .margin_end(8)
        .tooltip_text(crate::i18n::gettext("Delete Session"))
        // Not Tab-focusable: AdwDialog's auto-focus-restore on close
        // would otherwise land on whichever trash button was last
        // hovered and the ScrolledWindow would scroll to it.
        .focusable(false)
        .build();
    let session_id = session.id;
    delete_btn.connect_clicked(move |btn| {
        if let Some(win) = btn.root()
            .and_then(|r| r.downcast::<crate::window::MeditateWindow>().ok())
        {
            win.imp().log_view.imp().on_delete_clicked(session_id);
        }
    });

    // Card is focusable so it's reachable via Tab — Enter opens the edit
    // dialog, Delete triggers the same undo-toast flow as the trash button.
    let card = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .css_classes(["log-card"])
        .focusable(true)
        .build();
    card.append(&stripe);
    card.append(&content);
    card.append(&delete_btn);

    // Tap / click anywhere on the card → edit.
    let click = gtk::GestureClick::new();
    let session_id = session.id;
    click.connect_released(glib::clone!(
        #[weak] card,
        move |_, _, _, _| {
            if let Some(win) = card.root().and_then(|r| r.downcast::<crate::window::MeditateWindow>().ok()) {
                win.imp().log_view.imp().show_edit_dialog(session_id);
            }
        }
    ));
    card.add_controller(click);

    // Keyboard: Enter / Space opens edit, Delete starts the undo-toast flow.
    let key = gtk::EventControllerKey::new();
    key.connect_key_pressed(glib::clone!(
        #[weak] card,
        #[upgrade_or] glib::Propagation::Proceed,
        move |_, keyval, _, _| {
            let Some(win) = card.root()
                .and_then(|r| r.downcast::<crate::window::MeditateWindow>().ok())
            else { return glib::Propagation::Proceed; };
            let imp = win.imp().log_view.imp();
            match keyval {
                gtk::gdk::Key::Return | gtk::gdk::Key::KP_Enter | gtk::gdk::Key::space => {
                    imp.show_edit_dialog(session_id);
                    glib::Propagation::Stop
                }
                gtk::gdk::Key::Delete | gtk::gdk::Key::KP_Delete => {
                    imp.on_delete_clicked(session_id);
                    glib::Propagation::Stop
                }
                _ => glib::Propagation::Proceed,
            }
        }
    ));
    card.add_controller(key);

    card
}

fn build_label_chip(name: &str, color_cls: &str) -> gtk::Box {
    let dot = gtk::Box::builder()
        .width_request(6)
        .height_request(6)
        .valign(gtk::Align::Center)
        .css_classes(["log-label-dot", color_cls])
        .build();
    let text = gtk::Label::builder()
        .label(name)
        .css_classes(["log-label-text"])
        .build();
    let chip = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(5)
        .halign(gtk::Align::Start)
        .css_classes(["log-label-chip", color_cls])
        .build();
    chip.append(&dot);
    chip.append(&text);
    chip
}

/// Stable-per-name color class. We cycle through 8 HIG palette accents
/// (defined in CSS as `.log-c0`..`.log-c7`). A DJB-ish string hash keeps
/// the mapping stable across restarts without needing a per-label column.
fn label_color_class(name: &str) -> &'static str {
    const CLASSES: &[&str] = &[
        "log-c0", "log-c1", "log-c2", "log-c3",
        "log-c4", "log-c5", "log-c6", "log-c7",
    ];
    let mut h: u32 = 5381;
    for b in name.bytes() {
        h = h.wrapping_mul(33).wrapping_add(b as u32);
    }
    CLASSES[(h as usize) % CLASSES.len()]
}

/// Local `YYYY-MM-DD` — used as a grouping key. Not shown to the user.
fn date_group_key(unix_secs: i64) -> String {
    glib::DateTime::from_unix_local(unix_secs)
        .ok()
        .and_then(|dt| dt.format("%Y-%m-%d").ok())
        .map(|g| g.to_string())
        .unwrap_or_default()
}

/// Human-readable section header: "Today", "Yesterday", or "Apr 17".
/// Year is elided for dates in the current calendar year, shown otherwise.
fn date_group_display(unix_secs: i64) -> String {
    let Some(dt) = glib::DateTime::from_unix_local(unix_secs).ok() else {
        return String::new();
    };
    let now = glib::DateTime::now_local().unwrap();
    let same_day = now.year() == dt.year() && now.day_of_year() == dt.day_of_year();
    if same_day { return "Today".to_string(); }
    if let Ok(yest) = now.add_days(-1) {
        if yest.year() == dt.year() && yest.day_of_year() == dt.day_of_year() {
            return "Yesterday".to_string();
        }
    }
    let fmt = if now.year() == dt.year() { "%b %-d" } else { "%b %-d, %Y" };
    dt.format(fmt).map(|g| g.to_string()).unwrap_or_default()
}

fn format_time_of_day(unix_secs: i64) -> String {
    glib::DateTime::from_unix_local(unix_secs)
        .ok()
        .and_then(|dt| dt.format("%H:%M").ok())
        .map(|g| g.to_string())
        .unwrap_or_default()
}

fn section_caption_text(count: u32, total_secs: i64) -> String {
    let noun = if count == 1 { "session" } else { "sessions" };
    format!("{count} {noun} · {}", format_total(total_secs))
}

/// Compact total for section header: "42m" / "1h 04m".
fn format_total(secs: i64) -> String {
    if secs <= 0 { return "0m".to_string(); }
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    if h > 0 { format!("{h}h {m:02}m") } else { format!("{m}m") }
}

// ── Delete with undo toast ────────────────────────────────────────────────────

impl LogView {
    /// Hide the card for `session_id` and show a "Session deleted · Undo"
    /// toast. The DB row only actually goes away when the toast is
    /// dismissed without pressing Undo. Keeps the feed's scroll position
    /// — no refresh() involved.
    pub fn on_delete_clicked(&self, session_id: i64) {
        let Some(card) = self.cards_by_id.borrow().get(&session_id).cloned() else { return; };
        let sess = self.sessions.borrow().iter().find(|s| s.id == session_id).cloned();
        let Some(session) = sess else { return; };

        card.set_visible(false);

        let toast = adw::Toast::builder()
            .title(crate::i18n::gettext("Session deleted"))
            .button_label("Undo")
            .timeout(5)
            .build();

        // Undo: just restore the card — nothing changed in the DB yet.
        let card_undo = card.clone();
        toast.connect_button_clicked(move |_| {
            card_undo.set_visible(true);
        });

        // Dismissed: commit the delete unless the user hit Undo.
        let obj = self.obj().clone();
        let card_commit = card.clone();
        toast.connect_dismissed(move |_| {
            if card_commit.is_visible() { return; }  // Undo was pressed.
            let imp = obj.imp();
            if let Some(app) = imp.get_app() {
                app.with_db(|db| db.delete_session(session_id));
            }
            imp.commit_delete_in_place(session_id, &session);
        });

        if let Some(win) = self.get_window() {
            win.add_toast(toast);
        }
    }

    /// Actually remove the already-hidden card from its section, update
    /// the section caption, and drop the section entirely if it's now
    /// empty. Called only after the user lets the undo toast expire.
    fn commit_delete_in_place(&self, session_id: i64, session: &Session) {
        let key = date_group_key(session.start_time);

        // Remove widget from its section + update the section's counter.
        // Cloned out of the borrow so we can mutate the map below.
        let section = self.sections_by_key.borrow().get(&key).cloned();
        let section_empty = if let Some(sec) = section.as_ref() {
            if let Some(card) = self.cards_by_id.borrow().get(&session_id).cloned() {
                sec.cards_box.remove(&card);
            }
            sec.count.set(sec.count.get().saturating_sub(1));
            sec.total_secs.set((sec.total_secs.get() - session.duration_secs).max(0));
            sec.caption.set_label(
                &section_caption_text(sec.count.get(), sec.total_secs.get()),
            );
            sec.cards_box.first_child().is_none()
        } else {
            false
        };

        if let Some(sec) = section {
            if section_empty {
                self.feed_box.remove(&sec.outer);
                self.sections_by_key.borrow_mut().remove(&key);
                if self.current_section_key.borrow().as_deref() == Some(&key) {
                    *self.current_section_key.borrow_mut() = None;
                }
            }
        }

        self.cards_by_id.borrow_mut().remove(&session_id);
        self.sessions.borrow_mut().retain(|s| s.id != session_id);
        self.loaded_count.set(self.loaded_count.get().saturating_sub(1));

        // If the feed became entirely empty, flip to the empty state.
        if self.feed_box.first_child().is_none() {
            let has_filter = self.filter_notes_only.get()
                || self.filter_label_id.get().is_some();
            self.view_stack.set_visible_child_name(
                if has_filter { "filtered-empty" } else { "empty" },
            );
        }
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

        // ── Duration (hours + minutes as AdwSpinRows) ─────────────────
        let hours_spin = adw::SpinRow::builder()
            .title(crate::i18n::gettext("Hours"))
            .adjustment(&gtk::Adjustment::new(0.0, 0.0, 23.0, 1.0, 5.0, 0.0))
            .digits(0)
            .build();
        let minutes_spin = adw::SpinRow::builder()
            .title(crate::i18n::gettext("Minutes"))
            .adjustment(&gtk::Adjustment::new(0.0, 0.0, 59.0, 1.0, 5.0, 0.0))
            .digits(0)
            .build();

        if let Some(s) = session {
            hours_spin.set_value((s.duration_secs / 3600) as f64);
            minutes_spin.set_value(((s.duration_secs % 3600) / 60) as f64);
        }

        // ── Date row with calendar picker ──────────────────────────────
        let init_time = session.map(|s| s.start_time).unwrap_or_else(unix_now);
        let init_dt = glib::DateTime::from_unix_local(init_time).ok();
        let init_hour   = init_dt.as_ref().map(|d| d.hour()).unwrap_or(0);
        let init_minute = init_dt.as_ref().map(|d| d.minute()).unwrap_or(0);

        let date_row = adw::ActionRow::builder()
            .title(crate::i18n::gettext("Date"))
            .subtitle(format_date(init_time))
            .build();

        let calendar = gtk::Calendar::new();
        if let Ok(dt) = glib::DateTime::from_unix_local(init_time) {
            calendar.select_day(&dt);
        }

        let cal_popover = gtk::Popover::builder()
            .child(&calendar)
            .build();

        // MenuButton manages the popover lifecycle correctly (no manual set_parent/unparent).
        let cal_btn = gtk::MenuButton::builder()
            .icon_name("office-calendar-symbolic")
            .valign(gtk::Align::Center)
            .tooltip_text(crate::i18n::gettext("Pick a Date"))
            .css_classes(["flat"])
            .popover(&cal_popover)
            .always_show_arrow(false)
            .build();
        date_row.add_suffix(&cal_btn);

        calendar.connect_day_selected(glib::clone!(
            #[weak] date_row,
            #[weak] cal_popover,
            move |cal| {
                let dt = cal.date();
                if let Ok(local) = glib::DateTime::new(
                    &glib::TimeZone::local(),
                    dt.year(), dt.month(), dt.day_of_month(),
                    0, 0, 0.0,
                ) {
                    date_row.set_subtitle(&format_date(local.to_unix()));
                }
                cal_popover.popdown();
            }
        ));

        // ── Start time (hour + minute as AdwSpinRows) ─────────────────
        let time_hours_spin = adw::SpinRow::builder()
            .title(crate::i18n::gettext("Hour"))
            .adjustment(&gtk::Adjustment::new(init_hour as f64, 0.0, 23.0, 1.0, 5.0, 0.0))
            .digits(0)
            .build();
        let time_minutes_spin = adw::SpinRow::builder()
            .title(crate::i18n::gettext("Minute"))
            .adjustment(&gtk::Adjustment::new(init_minute as f64, 0.0, 59.0, 1.0, 5.0, 0.0))
            .digits(0)
            .build();

        // ── Label row ──────────────────────────────────────────────────
        let label_names: Vec<&str> = std::iter::once("None")
            .chain(labels.iter().map(|l| l.name.as_str()))
            .collect();
        let label_row = adw::ComboRow::builder()
            .title(crate::i18n::gettext("Label"))
            .model(&gtk::StringList::new(&label_names))
            .build();

        if let Some(s) = session {
            let idx = s.label_id
                .and_then(|id| labels.iter().position(|l| l.id == id))
                .map(|i| (i + 1) as u32)
                .unwrap_or(0);
            label_row.set_selected(idx);
        }

        // ── Note (multiline) ───────────────────────────────────────────
        let note_buffer = gtk::TextBuffer::new(None);
        if let Some(s) = session {
            note_buffer.set_text(s.note.as_deref().unwrap_or(""));
        }
        let note_view = gtk::TextView::builder()
            .buffer(&note_buffer)
            .wrap_mode(gtk::WrapMode::WordChar)
            .top_margin(8)
            .bottom_margin(8)
            .left_margin(12)
            .right_margin(12)
            .build();
        let note_scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            // Fixed taller starting size — GtkTextView reports a small
            // natural height (one line-ish) when empty, so
            // propagate_natural_height wasn't actually letting it grow.
            // Just force the height via height_request.
            .height_request(240)
            .child(&note_view)
            .css_classes(["log-note-editor"])
            .build();
        let note_caption = gtk::Label::builder()
            .label(crate::i18n::gettext("Note (optional)"))
            .halign(gtk::Align::Start)
            .margin_start(12)
            .css_classes(["caption", "dimmed"])
            .build();
        // Programmatically associate the caption with the text view so
        // screen readers announce "Note (optional) text entry" when the
        // user tabs into the editor.
        note_view.update_relation(&[gtk::accessible::Relation::LabelledBy(
            &[note_caption.upcast_ref::<gtk::Accessible>()],
        )]);
        let note_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(6)
            .build();
        note_box.append(&note_caption);
        note_box.append(&note_scroll);

        // ── Assemble dialog ────────────────────────────────────────────
        let duration_group = adw::PreferencesGroup::builder()
            .title(crate::i18n::gettext("Duration"))
            .build();
        duration_group.add(&hours_spin);
        duration_group.add(&minutes_spin);

        let time_group = adw::PreferencesGroup::builder()
            .title(crate::i18n::gettext("Start time"))
            .build();
        time_group.add(&date_row);
        time_group.add(&time_hours_spin);
        time_group.add(&time_minutes_spin);

        let label_group = adw::PreferencesGroup::new();
        label_group.add(&label_row);

        let content_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(18)
            .margin_start(12)
            .margin_end(12)
            .margin_top(12)
            .margin_bottom(12)
            .build();
        content_box.append(&duration_group);
        content_box.append(&time_group);
        content_box.append(&label_group);
        content_box.append(&note_box);

        let scrolled = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .propagate_natural_height(true)
            .child(&content_box)
            .build();

        let cancel_btn = gtk::Button::builder()
            .label(crate::i18n::gettext("_Cancel"))
            .use_underline(true)
            .build();
        let save_btn = gtk::Button::builder()
            .label(if is_edit { crate::i18n::gettext("_Save") } else { crate::i18n::gettext("_Add") })
            .use_underline(true)
            .css_classes(["suggested-action"])
            .build();

        let header = adw::HeaderBar::new();
        header.pack_start(&cancel_btn);
        header.pack_end(&save_btn);

        let toolbar_view = adw::ToolbarView::new();
        toolbar_view.add_top_bar(&header);
        toolbar_view.set_content(Some(&scrolled));

        let dialog = adw::Dialog::builder()
            .title(if is_edit { crate::i18n::gettext("Edit Session") } else { crate::i18n::gettext("Add Session") })
            .content_width(360)
            .child(&toolbar_view)
            .build();

        // Cancel
        cancel_btn.connect_clicked(glib::clone!(
            #[weak] dialog,
            move |_| { dialog.close(); }
        ));

        // Save
        let obj = self.obj().clone();
        save_btn.connect_clicked(glib::clone!(
            #[weak] dialog,
            #[weak] hours_spin,
            #[weak] minutes_spin,
            #[weak] time_hours_spin,
            #[weak] time_minutes_spin,
            #[weak] calendar,
            #[weak] label_row,
            #[weak] note_buffer,
            move |_| {
                let imp = obj.imp();
                let duration = hours_spin.value() as i64 * 3600
                    + minutes_spin.value() as i64 * 60;
                let cal_date = calendar.date();
                let start_time = glib::DateTime::new(
                    &glib::TimeZone::local(),
                    cal_date.year(), cal_date.month(), cal_date.day_of_month(),
                    time_hours_spin.value() as i32,
                    time_minutes_spin.value() as i32,
                    0.0,
                ).ok().map(|d| d.to_unix()).unwrap_or_else(unix_now);
                let selected = label_row.selected() as usize;
                let label_id = if selected == 0 {
                    None
                } else {
                    imp.labels.borrow().get(selected - 1).map(|l| l.id)
                };
                let note_text = note_buffer.text(
                    &note_buffer.start_iter(),
                    &note_buffer.end_iter(),
                    false,
                );
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

fn format_date(unix_secs: i64) -> String {
    glib::DateTime::from_unix_local(unix_secs)
        .ok()
        .and_then(|dt| dt.format("%b %d, %Y").ok())
        .map(|gs| gs.to_string())
        .unwrap_or_default()
}


fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

