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

    /// The date section currently being filled. Persists across load_more
    /// calls so a single calendar day never gets split into two headers
    /// just because it straddled a page boundary.
    current_section: RefCell<Option<DateSection>>,

    /// session_id → card widget. Lets edits update a single card in place
    /// instead of rebuilding the whole feed (which resets the scroll
    /// position — closing the edit dialog used to jump to the bottom).
    cards_by_id: RefCell<std::collections::HashMap<i64, gtk::Box>>,
}

/// One "Today" / "Yesterday" / "Apr 17" group in the feed.
#[derive(Debug)]
struct DateSection {
    /// Sort key — YYYY-MM-DD local date. Same key means same section.
    key: String,
    /// Subtitle under the date header: "3 sessions · 1h 04m".
    caption: gtk::Label,
    /// Vertical Gtk.Box that holds the cards.
    cards_box: gtk::Box,
    count: u32,
    total_secs: i64,
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
        *self.current_section.borrow_mut() = None;
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
        let mut current = self.current_section.borrow_mut();

        // Do we need a new section?
        let need_new = !matches!(current.as_ref(), Some(sec) if sec.key == key);
        if need_new {
            let (section_box, caption_label, cards_box) = build_section_frame(session.start_time);
            self.feed_box.append(&section_box);
            *current = Some(DateSection {
                key: key.clone(),
                caption: caption_label,
                cards_box,
                count: 0,
                total_secs: 0,
            });
        }

        // Append the card into the current section and update its counter.
        let sec = current.as_mut().expect("current_section populated above");
        let card = build_card(session, label_map);
        sec.cards_box.append(&card);
        self.cards_by_id.borrow_mut().insert(session.id, card);
        sec.count       += 1;
        sec.total_secs  += session.duration_secs;
        sec.caption.set_label(&section_caption_text(sec.count, sec.total_secs));
    }

    /// Replace the card for `session_id` with a freshly-built one, keeping
    /// the same slot in its date section. Avoids the full refresh() that
    /// would reset the scroll position on dialog close.
    pub fn replace_card_in_place(&self, session: &Session) {
        let old_card = self.cards_by_id.borrow().get(&session.id).cloned();
        let Some(old) = old_card else { return; };
        let Some(parent) = old.parent().and_then(|p| p.downcast::<gtk::Box>().ok()) else {
            return;
        };

        // Build the replacement card before we touch the DOM so the rest
        // of the update is a simple remove + insert.
        let labels_ref = self.labels.borrow();
        let label_map: std::collections::HashMap<i64, &str> =
            labels_ref.iter().map(|l| (l.id, l.name.as_str())).collect();
        let new_card = build_card(session, &label_map);

        // `insert_child_after` with the previous sibling keeps the card at
        // its existing index in the cards_box. `prev_sibling` = None means
        // the card was first, and GTK interprets that as prepend.
        let prev = old.prev_sibling();
        parent.remove(&old);
        parent.insert_child_after(&new_card, prev.as_ref());

        self.cards_by_id.borrow_mut().insert(session.id, new_card);

        // Keep the cached Vec<Session> consistent so future pagination /
        // edit dialog lookups see the updated values too.
        if let Some(s) = self.sessions.borrow_mut().iter_mut().find(|s| s.id == session.id) {
            *s = session.clone();
        }
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
        .css_classes(["caption", "dim-label"])
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

    // Colored left stripe — 3 px wide, full card height.
    let stripe = gtk::Box::builder()
        .width_request(3)
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
        .label("MIN")
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

    // Right column: label chip + (note or placeholder).
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
            .label("No note added")
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
        .margin_start(10)
        .margin_end(12)
        .build();
    content.append(&left_col);
    content.append(&right_col);

    let card = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .css_classes(["log-card"])
        .build();
    card.append(&stripe);
    card.append(&content);

    // Tap anywhere on the card to open the edit dialog; per-card
    // delete sits as a small flat button in the top-right of the right col.
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

        // ── Duration row ───────────────────────────────────────────────
        let hours_spin = gtk::SpinButton::with_range(0.0, 23.0, 1.0);
        hours_spin.set_valign(gtk::Align::Center);
        hours_spin.set_width_chars(3);
        let minutes_spin = gtk::SpinButton::with_range(0.0, 59.0, 1.0);
        minutes_spin.set_valign(gtk::Align::Center);
        minutes_spin.set_width_chars(3);

        if let Some(s) = session {
            hours_spin.set_value((s.duration_secs / 3600) as f64);
            minutes_spin.set_value(((s.duration_secs % 3600) / 60) as f64);
        }

        let h_label = gtk::Label::builder().label("h").margin_start(4).margin_end(8).build();
        let m_label = gtk::Label::builder().label("min").margin_start(4).build();
        let spin_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(0)
            .valign(gtk::Align::Center)
            .build();
        spin_box.append(&hours_spin);
        spin_box.append(&h_label);
        spin_box.append(&minutes_spin);
        spin_box.append(&m_label);

        let duration_row = adw::ActionRow::builder()
            .title("Duration")
            .activatable_widget(&hours_spin)
            .build();
        duration_row.add_suffix(&spin_box);

        // ── Date row with calendar picker ──────────────────────────────
        let init_time = session.map(|s| s.start_time).unwrap_or_else(unix_now);
        let init_dt = glib::DateTime::from_unix_local(init_time).ok();
        let init_hour   = init_dt.as_ref().map(|d| d.hour()).unwrap_or(0);
        let init_minute = init_dt.as_ref().map(|d| d.minute()).unwrap_or(0);

        let date_row = adw::ActionRow::builder()
            .title("Date")
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
            .tooltip_text("Pick a date")
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

        // ── Time row ───────────────────────────────────────────────────
        let time_hours_spin = gtk::SpinButton::with_range(0.0, 23.0, 1.0);
        time_hours_spin.set_valign(gtk::Align::Center);
        time_hours_spin.set_width_chars(3);
        time_hours_spin.set_value(init_hour as f64);
        let time_minutes_spin = gtk::SpinButton::with_range(0.0, 59.0, 1.0);
        time_minutes_spin.set_valign(gtk::Align::Center);
        time_minutes_spin.set_width_chars(3);
        time_minutes_spin.set_value(init_minute as f64);

        let th_label = gtk::Label::builder().label("h").margin_start(4).margin_end(8).build();
        let tm_label = gtk::Label::builder().label("min").margin_start(4).build();
        let time_spin_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(0)
            .valign(gtk::Align::Center)
            .build();
        time_spin_box.append(&time_hours_spin);
        time_spin_box.append(&th_label);
        time_spin_box.append(&time_minutes_spin);
        time_spin_box.append(&tm_label);

        let time_row = adw::ActionRow::builder()
            .title("Time")
            .activatable_widget(&time_hours_spin)
            .build();
        time_row.add_suffix(&time_spin_box);

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
            .min_content_height(80)
            .max_content_height(160)
            .propagate_natural_height(true)
            .child(&note_view)
            .css_classes(["card"])
            .build();
        let note_caption = gtk::Label::builder()
            .label("Note (optional)")
            .halign(gtk::Align::Start)
            .margin_start(12)
            .css_classes(["caption", "dim-label"])
            .build();
        let note_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(6)
            .build();
        note_box.append(&note_caption);
        note_box.append(&note_scroll);

        // ── Assemble dialog ────────────────────────────────────────────
        let group = adw::PreferencesGroup::new();
        group.add(&duration_row);
        group.add(&date_row);
        group.add(&time_row);
        group.add(&label_row);

        let content_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(18)
            .margin_start(12)
            .margin_end(12)
            .margin_top(12)
            .margin_bottom(12)
            .build();
        content_box.append(&group);
        content_box.append(&note_box);

        let scrolled = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .propagate_natural_height(true)
            .child(&content_box)
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
