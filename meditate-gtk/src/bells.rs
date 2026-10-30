//! Interval-bell library UI — the NavigationPage pushed when the user
//! taps the "Interval Bells" row in the timer setup. Lists every
//! configured bell (regardless of enabled state) with a per-row Switch
//! to toggle enabled. A synthetic "Create new interval bell…" entry
//! sits at the top of the list to add a new default bell, mirroring
//! the label and sound choosers. The edit page reachable by tapping
//! a bell row is defined further down in this file.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;

use crate::application::MeditateApplication;
use crate::db::{write_ok, Failed, IntervalBell, IntervalBellKind};
use meditate_core::bells::{bell_number_range, clamp_bell_time, BellTime, BellTimeField, TimeUnit};
use crate::i18n::gettext;

/// Push the bell-library page onto the window's nav view.
///
/// `on_changed` fires whenever the library's contents shift (an add,
/// a per-row enabled toggle, or — once B.3.3.B lands — an edit or
/// delete). The window uses it to re-read the count for the "Manage
/// Bells" subtitle on the timer setup page.
pub fn push_bells_page<F>(
    nav_view: &adw::NavigationView,
    app: &MeditateApplication,
    on_changed: F,
)
where F: Fn() + Clone + 'static,
{
    let group = adw::PreferencesGroup::new();
    let rows: Rc<RefCell<Vec<adw::ActionRow>>> = Rc::new(RefCell::new(Vec::new()));

    let prefs_page = adw::PreferencesPage::new();
    prefs_page.add(&group);

    let header = adw::HeaderBar::builder()
        .show_back_button(true)
        .build();

    // Its own overlay: a failed save must show on this page.
    let toast_overlay = adw::ToastOverlay::new();
    toast_overlay.set_child(Some(&prefs_page));
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&toast_overlay));

    let page = adw::NavigationPage::builder()
        .tag("interval-bells")
        .title(gettext("Interval Bells"))
        .child(&toolbar)
        .build();

    // Shared "rebuild + on_changed" closure stored behind a RefCell so
    // both the synthetic create-row and the edit-page callback can fire
    // it without us having to thread Clone through a self-referential
    // type. Set once below after the per-row handlers are built.
    let rebuilder: crate::Rebuilder =
        Rc::new(RefCell::new(None));

    let rebuilder_for_init = rebuilder.clone();
    let nav_view_clone = nav_view.clone();
    let app_clone = app.clone();
    let on_changed_clone = on_changed.clone();
    *rebuilder.borrow_mut() = Some(Box::new(move || {
        rebuild_list(
            &group,
            &rows,
            &app_clone,
            &nav_view_clone,
            rebuilder_for_init.clone(),
            on_changed_clone.clone(),
        );
        on_changed_clone();
    }));

    if let Some(rb) = rebuilder.borrow().as_ref() {
        rb();
    }
    // Back from a chooser or the edit page: a sound or pattern may
    // have been renamed or deleted there.
    let rebuilder_for_shown = rebuilder.clone();
    page.connect_shown(move |_| {
        if let Some(rb) = rebuilder_for_shown.borrow().as_ref() {
            rb();
        }
    });
    nav_view.push(&page);
}

/// Drop all rows from the group and re-add them from the current DB
/// state. Called on initial push and after an add. Per-row Switch
/// toggles call `set_interval_bell_enabled` directly so they don't
/// need a rebuild.
type Rebuilder = crate::Rebuilder;

fn rebuild_list(
    group: &adw::PreferencesGroup,
    rows: &Rc<RefCell<Vec<adw::ActionRow>>>,
    app: &MeditateApplication,
    nav_view: &adw::NavigationView,
    rebuilder: Rebuilder,
    on_changed: impl Fn() + Clone + 'static,
) {
    for row in rows.borrow_mut().drain(..) {
        group.remove(&row);
    }

    // Synthetic "Create new interval bell…" entry, always at the top —
    // matches the label and sound choosers' creation affordance and
    // keeps the action thumb-reachable on a phone instead of tucked
    // into the headerbar.
    let create_row = build_create_row(app, nav_view, rebuilder.clone(), on_changed.clone());
    group.add(&create_row);
    rows.borrow_mut().push(create_row);

    // Stopwatch mode disables fixed-from-end bells (no end to count
    // backwards from). The bell library is global, so we override the
    // visual switch state without writing to the DB — flipping
    // stopwatch off restores the user's persisted enabled flag.
    // Interval bells fire in Timer mode (the only mode that calls
    // build_session_bells with a real schedule), so the relevant
    // stopwatch toggle is Timer's.
    let stopwatch_key = meditate_core::settings_keys::stopwatch_key_for_mode(
        meditate_core::SessionMode::Timer,
    );
    let stopwatch_on = app
        .with_db(|db| meditate_core::read_bool(db.core(), stopwatch_key, false))
        .unwrap_or(false);

    let bells = app
        .with_db(super::db::Database::list_interval_bells)
        .and_then(std::result::Result::ok)
        .unwrap_or_default();

    if bells.is_empty() {
        let row = empty_state_row();
        group.add(&row);
        rows.borrow_mut().push(row);
        return;
    }

    for bell in bells {
        let row = build_bell_row(
            &bell,
            app,
            nav_view,
            rebuilder.clone(),
            on_changed.clone(),
            stopwatch_on,
        );
        group.add(&row);
        rows.borrow_mut().push(row);
    }
}

/// Build the synthetic top row that creates a new interval bell with
/// sensible defaults (every 5 min, no jitter, bundled bowl) and drills
/// straight into the edit page so the user can dial it in immediately.
fn build_create_row(
    app: &MeditateApplication,
    nav_view: &adw::NavigationView,
    rebuilder: Rebuilder,
    on_changed: impl Fn() + Clone + 'static,
) -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .title(gettext("Create new interval bell…"))
        .activatable(true)
        .build();
    let plus = gtk::Image::from_icon_name("list-add-symbolic");
    plus.add_css_class("dim-label");
    row.add_suffix(&plus);

    let app_for_create = app.clone();
    let nav_view_for_create = nav_view.clone();
    let rebuilder_for_create = rebuilder.clone();
    let on_changed_for_create = on_changed.clone();
    row.connect_activated(move |row| {
        let created = app_for_create.with_db_mut(|db| {
            db.insert_interval_bell(
                meditate_core::bells::DEFAULT_NEW_BELL_KIND,
                meditate_core::bells::DEFAULT_NEW_BELL_TIME,
                BellTime::ZERO,
                BellTime::ZERO,
                crate::db::BUNDLED_BOWL_UUID,
                crate::db::BUNDLED_PATTERN_PULSE_UUID,
                meditate_core::bells::DEFAULT_NEW_BELL_SIGNAL_MODE,
            )
        });
        let new_id = match created {
            Some(Ok(id)) => Some(id),
            failed => {
                write_ok(row, "bell.add", failed, Failed::Save);
                None
            }
        };

        if let Some(rb) = rebuilder_for_create.borrow().as_ref() {
            rb();
        }

        // Look up the just-inserted row's uuid (insert returns the
        // rowid, not the uuid) and drill into its edit page.
        if let Some(rowid) = new_id {
            let new_uuid = app_for_create
                .with_db(|db| db.find_interval_bell_by_id(rowid))
                .and_then(std::result::Result::ok)
                .flatten()
                .map(|b| b.uuid.0);
            if let Some(uuid) = new_uuid {
                push_edit_page(
                    &nav_view_for_create,
                    &app_for_create,
                    &uuid,
                    rebuilder_for_create.clone(),
                    on_changed_for_create.clone(),
                );
            }
        }
    });

    row
}

fn build_bell_row(
    bell: &IntervalBell,
    app: &MeditateApplication,
    nav_view: &adw::NavigationView,
    rebuilder: Rebuilder,
    on_changed: impl Fn() + Clone + 'static,
    stopwatch_on: bool,
) -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .title(bell_title(bell))
        .subtitle(sound_name(app, bell.sound_uuid.as_str()))
        .activatable(true)
        .build();

    // Fixed-from-end bells are inert in stopwatch mode (no end to
    // count backwards from). Switch shows OFF + insensitive without
    // touching the persisted enabled flag — the user's previous
    // intent comes back as soon as stopwatch flips off. The row
    // stays activatable so the user can still drill into Edit to
    // change kind or delete.
    let switch_state = meditate_core::bells::bell_row_switch_state(
        bell.enabled,
        bell.kind,
        meditate_core::bells::DisplayMode::from_stopwatch_flag(stopwatch_on),
    );
    if !switch_state.sensitive {
        row.add_css_class("dim-label");
    }
    let switch = gtk::Switch::builder()
        .active(switch_state.active)
        .sensitive(switch_state.sensitive)
        .valign(gtk::Align::Center)
        .build();

    // Toggle persists immediately. No list rebuild needed for an
    // enabled flip — the row's title/subtitle don't change, only the
    // count subtitle on the timer setup page.
    let bell_uuid_for_toggle = bell.uuid.0.clone();
    let app_for_toggle = app.clone();
    let on_changed_for_toggle = on_changed.clone();
    switch.connect_active_notify(move |s| {
        let written = app_for_toggle.with_db_mut(|db| {
            db.set_interval_bell_enabled(&bell_uuid_for_toggle, s.is_active())
        });
        write_ok(s, "bell.enable", written, Failed::Save);
        on_changed_for_toggle();
    });
    row.add_suffix(&switch);

    // Inline delete: small icon-only destructive button next to the
    // switch. Same confirmation dialog the edit page uses.
    // .destructive-action on a flat button tints just the symbolic
    // icon red via libadwaita's currentColor — no red background fill,
    // which would be too loud on a list row that's mostly chrome.
    let delete_btn = gtk::Button::builder()
        .icon_name("user-trash-symbolic")
        .tooltip_text(gettext("Delete bell"))
        .css_classes(["flat", "circular", "destructive-action"])
        .valign(gtk::Align::Center)
        .build();
    let bell_uuid_for_del = bell.uuid.0.clone();
    let app_for_del = app.clone();
    let rebuilder_for_del = rebuilder.clone();
    let on_changed_for_del = on_changed.clone();
    let row_for_del = row.clone();
    delete_btn.connect_clicked(move |_| {
        confirm_and_delete(
            &row_for_del,
            &app_for_del,
            &bell_uuid_for_del,
            rebuilder_for_del.clone(),
            on_changed_for_del.clone(),
        );
    });
    row.add_suffix(&delete_btn);
    // No set_activatable_widget — we want a tap on the row body to
    // emit `activated` and push the edit page; taps on the switch
    // and delete button handle themselves as their own widgets.

    let bell_uuid_for_edit = bell.uuid.0.clone();
    let app_for_edit = app.clone();
    let nav_view_for_edit = nav_view.clone();
    let rebuilder_for_edit = rebuilder.clone();
    let on_changed_for_edit = on_changed.clone();
    row.connect_activated(move |_| {
        push_edit_page(
            &nav_view_for_edit,
            &app_for_edit,
            &bell_uuid_for_edit,
            rebuilder_for_edit.clone(),
            on_changed_for_edit.clone(),
        );
    });
    row
}

/// Shared deletion dialog used by the inline trash button on each
/// list row. Mirrors the edit-page delete flow but lives at the list
/// level so the confirmation can present against the list-page root.
fn confirm_and_delete(
    anchor: &adw::ActionRow,
    app: &MeditateApplication,
    bell_uuid: &str,
    rebuilder: Rebuilder,
    on_changed: impl Fn() + Clone + 'static,
) {
    let dialog = adw::AlertDialog::builder()
        .heading(gettext("Delete Bell?"))
        .body(gettext("This bell will no longer ring during sessions."))
        .close_response("cancel")
        .default_response("cancel")
        .build();
    dialog.add_response("cancel", &gettext("Cancel"));
    dialog.add_response("delete", &gettext("Delete"));
    dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);

    let app = app.clone();
    let uuid = bell_uuid.to_string();
    let rebuilder = rebuilder.clone();
    let on_changed = on_changed.clone();
    let anchor_for_failure = anchor.clone();
    dialog.connect_response(None, move |_, id| {
        if id != "delete" { return; }
        let deleted = app.with_db_mut(|db| db.delete_interval_bell(&uuid));
        if !write_ok(&anchor_for_failure, "bell.delete", deleted, Failed::Delete) {
            return;
        }
        if let Some(rb) = rebuilder.borrow().as_ref() { rb(); }
        on_changed();
    });

    if let Some(root) = anchor.root() {
        if let Ok(window) = root.downcast::<gtk::Window>() {
            dialog.present(Some(&window));
        }
    }
}

fn empty_state_row() -> adw::ActionRow {
    // Subdued helper text rendered as an unframed row with the same
    // .dim-label styling we use elsewhere for in-context hints.
    let row = adw::ActionRow::builder()
        .title(gettext("No interval bells configured"))
        .subtitle(gettext("Tap the row above to add one"))
        .activatable(false)
        .selectable(false)
        .build();
    row.add_css_class("dim-label");
    row
}

/// Concise human-readable summary of a bell — fits on a single
/// AdwActionRow title line on the Librem 5 portrait. Maps the
/// typed key from core to the user-facing translated string.
fn bell_title(bell: &IntervalBell) -> String {
    use crate::i18n::ngettext;
    use meditate_core::bells::BellTitleKey;
    use TimeUnit::{Min, Sec};
    let key = meditate_core::bells::bell_title_key(bell);
    let (BellTitleKey::Every { time, .. } | BellTitleKey::At { time } | BellTitleKey::BeforeEnd { time }) = key;
    let (n, count) = (time.number().to_string(), time.number());
    let mut title = match (key, time.unit) {
        (BellTitleKey::Every { .. }, Min) =>
            ngettext("Every 1 min", "Every {n} min", count).replace("{n}", &n),
        (BellTitleKey::Every { .. }, Sec) =>
            ngettext("Every 1 s", "Every {n} s", count).replace("{n}", &n),
        (BellTitleKey::At { .. }, Min) =>
            ngettext("At 1 min", "At {n} min", count).replace("{n}", &n),
        (BellTitleKey::At { .. }, Sec) =>
            ngettext("At 1 s", "At {n} s", count).replace("{n}", &n),
        (BellTitleKey::BeforeEnd { .. }, Min) =>
            ngettext("1 min before end", "{n} min before end", count).replace("{n}", &n),
        (BellTitleKey::BeforeEnd { .. }, Sec) =>
            ngettext("1 s before end", "{n} s before end", count).replace("{n}", &n),
    };
    if let BellTitleKey::Every { jitter, step, .. } = key {
        if let Some(j) = jitter {
            title.push_str(&format!(" ±{}", bell_time_text(j)));
        }
        if let Some(s) = step {
            title.push_str(&format!(", +{}", bell_time_text(s)));
        }
    }
    title
}

/// "5 min" or "30 s".
fn bell_time_text(t: BellTime) -> String {
    match t.unit {
        TimeUnit::Min => gettext("{n} min"),
        TimeUnit::Sec => gettext("{n} s"),
    }
    .replace("{n}", &t.number().to_string())
}

/// Render a core `ResolvedName` to subtitle text. `Resolved` passes
/// through; `Missing` becomes a localized affordance so a user (or
/// SR) sees that the row needs re-picking rather than just an empty
/// subtitle. Single rendering point so the `gettext` call lives in
/// one place — the Android shell will have its own equivalent using
/// `strings.xml`.
pub(crate) fn render_resolved_name(name: meditate_core::bells::ResolvedName) -> String {
    match name {
        meditate_core::bells::ResolvedName::Resolved(s) => s,
        meditate_core::bells::ResolvedName::Missing => gettext("Missing"),
    }
}

/// Look up a bell-sound's display name from the bell_sounds library.
/// Pulls the library from the DB and delegates the lookup to core,
/// then renders core's `ResolvedName` to subtitle text.
fn sound_name(app: &MeditateApplication, uuid: &str) -> String {
    app.with_db(|db| render_resolved_name(
        meditate_core::bells::resolve_sound_name(db.core(), uuid),
    ))
    .unwrap_or_default()
}

/// Same as sound_name but for vibration_patterns.
fn pattern_name(app: &MeditateApplication, uuid: &str) -> String {
    app.with_db(|db| render_resolved_name(
        meditate_core::bells::resolve_pattern_name(db.core(), uuid),
    ))
    .unwrap_or_default()
}

/// A bell time row: a number with an s/min switch beside it.
#[derive(Clone)]
struct TimeRow {
    row: adw::SpinRow,
    unit: adw::ToggleGroup,
    field: BellTimeField,
}

impl TimeRow {
    fn new(title: &str, subtitle: Option<&str>, field: BellTimeField, initial: BellTime) -> TimeRow {
        let (lo, hi) = bell_number_range(field, initial.unit);
        let row = adw::SpinRow::builder()
            .title(title)
            .adjustment(&gtk::Adjustment::new(
                f64::from(initial.number()), f64::from(lo), f64::from(hi), 1.0, 5.0, 0.0,
            ))
            .build();
        if let Some(subtitle) = subtitle {
            row.set_subtitle(subtitle);
        }
        let unit = adw::ToggleGroup::builder()
            .css_classes(["round"])
            .homogeneous(true)
            .valign(gtk::Align::Center)
            .build();
        unit.add(adw::Toggle::builder().name("s").label(gettext("s")).tooltip(gettext("Seconds")).build());
        unit.add(adw::Toggle::builder().name("min").label(gettext("min")).tooltip(gettext("Minutes")).build());
        unit.set_active_name(Some(initial.unit.as_db_str()));
        row.add_suffix(&unit);
        TimeRow { row, unit, field }
    }

    /// Show `time` without reporting it as a change; callers hold
    /// their populating guard around this.
    fn set(&self, time: BellTime) {
        let (lo, hi) = bell_number_range(self.field, time.unit);
        self.unit.set_active_name(Some(time.unit.as_db_str()));
        let adj = self.row.adjustment();
        adj.set_lower(f64::from(lo));
        adj.set_upper(f64::from(hi));
        self.row.set_value(f64::from(time.number()));
    }

    /// `f` gets the row and its time whenever the number or the unit
    /// changes. Flipping the unit keeps the number, clamped to the new
    /// unit's range.
    fn connect_changed(&self, f: impl Fn(&adw::SpinRow, BellTime) + 'static) {
        let unit_of = |group: &adw::ToggleGroup| {
            group.active_name().as_deref().and_then(TimeUnit::from_db_str)
        };
        let f = Rc::new(f);
        let flipping = Rc::new(std::cell::Cell::new(false));
        let field = self.field;
        let (f2, flipping2, group) = (f.clone(), flipping.clone(), self.unit.clone());
        self.row.connect_notify_local(Some("value"), move |row, _| {
            if flipping2.get() { return; }
            let Some(unit) = unit_of(&group) else { return };
            f2(row, clamp_bell_time(field, row.value().round() as u32, unit));
        });
        let row = self.row.clone();
        self.unit.connect_active_name_notify(move |group| {
            let Some(unit) = unit_of(group) else { return };
            let (lo, hi) = bell_number_range(field, unit);
            flipping.set(true);
            let adj = row.adjustment();
            adj.set_lower(f64::from(lo));
            adj.set_upper(f64::from(hi));
            row.set_value(row.value().clamp(f64::from(lo), f64::from(hi)));
            flipping.set(false);
            f(&row, clamp_bell_time(field, row.value().round() as u32, unit));
        });
    }
}

/// Edit page for one bell — pushed when the user taps a row in the
/// list. Save-as-you-go: every field change persists immediately
/// Persist the current snapshot via the shell's `edit_interval_bell`
/// pass-through and fire the rebuild + on_changed pipeline. Hoisted
/// out of `push_edit_page` so each field-handler can call it from
/// its own captured clones without nesting an item inside a fn body.
fn write_back_bell(
    anchor: &impl IsA<gtk::Widget>,
    app: &MeditateApplication,
    snap: &Rc<RefCell<IntervalBell>>,
    rebuilder: &Rebuilder,
    on_changed: &(impl Fn() + Clone + 'static),
) {
    let s = snap.borrow().clone();
    let written = app.with_db_mut(|db| db.edit_interval_bell(&s));
    // A preset rings the old bell: the page edits the new one from now on.
    if let Some(Ok(meditate_core::preset_config::BellEdit::Copied(uuid))) = &written {
        snap.borrow_mut().uuid = uuid.clone();
        crate::db::show_toast(anchor, gettext("Saved as a new bell, your presets keep the old one"));
    }
    write_ok(anchor, "bell.update", written, Failed::Save);
    if let Some(rb) = rebuilder.borrow().as_ref() {
        rb();
    }
    on_changed();
}

/// (with a populating-style guard during the initial load) and fires
/// the same rebuilder/on_changed pipeline so the list page and the
/// timer setup's subtitle stay in sync. Delete asks for confirmation
/// via Adw.AlertDialog and pops the page on confirm.
fn push_edit_page(
    nav_view: &adw::NavigationView,
    app: &MeditateApplication,
    bell_uuid: &str,
    rebuilder: Rebuilder,
    on_changed: impl Fn() + Clone + 'static,
) {
    // Resolve the bell. If it's gone (raced with a delete from a peer
    // sync, say), bail silently — pushing an edit page for a row that
    // no longer exists would just confuse the user.
    let Some(bell) = lookup_bell(app, bell_uuid) else {
        return;
    };

    let prefs_page = adw::PreferencesPage::new();

    // ── Form group ─────────────────────────────────────────────────
    let form = adw::PreferencesGroup::new();

    // Kind combo — three entries matching IntervalBellKind. The user-
    // facing strings are gettext'd.
    let kind_choices = [
        gettext("Repeating"),
        gettext("At time from start"),
        gettext("Before end"),
    ];
    let kind_refs: Vec<&str> = kind_choices.iter().map(std::string::String::as_str).collect();
    let kind_row = adw::ComboRow::builder()
        .title(gettext("Kind"))
        .model(&gtk::StringList::new(&kind_refs))
        .selected(match bell.kind {
            IntervalBellKind::Interval => 0,
            IntervalBellKind::FixedFromStart => 1,
            IntervalBellKind::FixedFromEnd => 2,
        })
        .build();
    form.add(&kind_row);

    // Time — common to all kinds; its meaning shifts with kind.
    let time_row = TimeRow::new(&gettext("Time"), None, BellTimeField::Time, bell.time);
    form.add(&time_row.row);

    // Jitter and step only mean something for the Repeating kind.
    // Visibility gates on kind below.
    let jitter_row = TimeRow::new(
        &gettext("Jitter"),
        Some(&gettext("Random shift of each ring")),
        BellTimeField::Jitter,
        bell.jitter,
    );
    jitter_row.row.set_visible(bell.kind == IntervalBellKind::Interval);
    form.add(&jitter_row.row);
    let step_row = TimeRow::new(
        &gettext("Step"),
        Some(&gettext("Added to the gap after each ring")),
        BellTimeField::Step,
        bell.step,
    );
    step_row.row.set_visible(bell.kind == IntervalBellKind::Interval);
    form.add(&step_row.row);

    // Type — Sound / Vibration / Both AdwToggleGroup. Determines
    // which of Bell Sound / Pattern rows are shown below.
    let signal_mode_row = adw::ActionRow::builder()
        .title(gettext("Type"))
        .build();
    let signal_toggle_host = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .valign(gtk::Align::Center)
        .build();
    signal_mode_row.add_suffix(&signal_toggle_host);
    form.add(&signal_mode_row);

    let signal_toggle = adw::ToggleGroup::builder()
        .css_classes(["round"])
        .valign(gtk::Align::Center)
        .build();
    let toggle_sound = adw::Toggle::builder()
        .name("sound")
        .label(gettext("Sound"))
        .build();
    let toggle_vibration = adw::Toggle::builder()
        .name("vibration")
        .label(gettext("Vibration"))
        .build();
    let toggle_both = adw::Toggle::builder()
        .name("both")
        .label(gettext("Both"))
        .build();
    signal_toggle.add(toggle_sound);
    signal_toggle.add(toggle_vibration);
    signal_toggle.add(toggle_both);
    signal_toggle.set_active_name(Some(bell.signal_mode.as_db_str()));
    signal_toggle_host.append(&signal_toggle);
    crate::timer::show_no_vibration_note(&signal_toggle);

    // Sound row — taps push the bell-sound chooser. Subtitle shows the
    // currently-selected sound's name (looked up by uuid).
    let sound_row = adw::ActionRow::builder()
        .title(gettext("Sound"))
        .subtitle(sound_name(app, bell.sound_uuid.as_str()))
        .activatable(true)
        .build();
    {
        let chevron = gtk::Image::from_icon_name("go-next-symbolic");
        chevron.add_css_class("dim-label");
        sound_row.add_suffix(&chevron);
    }
    form.add(&sound_row);

    // Volume row (issue #1) — shown with the Sound row; wired below
    // once the snapshot exists.
    let volume_row = adw::PreferencesRow::new();
    form.add(&volume_row);

    // Pattern row — taps push the vibration-pattern chooser.
    let pattern_row = adw::ActionRow::builder()
        .title(gettext("Pattern"))
        .subtitle(pattern_name(app, bell.vibration_pattern_uuid.as_str()))
        .activatable(true)
        .build();
    {
        let chevron = gtk::Image::from_icon_name("go-next-symbolic");
        chevron.add_css_class("dim-label");
        pattern_row.add_suffix(&chevron);
    }
    form.add(&pattern_row);

    // Initial visibility based on the saved signal mode.
    sound_row.set_visible(bell.signal_mode.includes_sound());
    volume_row.set_visible(bell.signal_mode.includes_sound());
    pattern_row.set_visible(bell.signal_mode.includes_vibration());

    prefs_page.add(&form);

    // ── Delete group ───────────────────────────────────────────────
    let delete_group = adw::PreferencesGroup::new();
    let delete_btn = gtk::Button::builder()
        .label(gettext("Delete Bell"))
        .css_classes(["destructive-action", "pill"])
        .halign(gtk::Align::Center)
        .margin_top(12)
        .build();
    delete_group.add(&delete_btn);
    prefs_page.add(&delete_group);

    // ── Page chrome ────────────────────────────────────────────────
    let header = adw::HeaderBar::builder().show_back_button(true).build();
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    // Its own overlay: a failed save must show on this page.
    let toast_overlay = adw::ToastOverlay::new();
    toast_overlay.set_child(Some(&prefs_page));
    toolbar.set_content(Some(&toast_overlay));
    let page = adw::NavigationPage::builder()
        .tag("interval-bell-edit")
        .title(gettext("Edit Bell"))
        .child(&toolbar)
        .build();

    // ── Save-as-you-go wiring ──────────────────────────────────────
    // Snapshot of the current bell's state lives behind a RefCell so
    // each per-field handler reads the OTHER fields without re-querying
    // the DB. Updated whenever a handler writes back.
    let initial_volume = bell.volume;
    let snapshot = Rc::new(RefCell::new(bell));
    let populating = Rc::new(std::cell::Cell::new(false));

    // Back from a chooser: the sound or pattern may have been renamed
    // or deleted there.
    {
        let snap = snapshot.clone();
        let app = app.clone();
        let sound_row = sound_row.clone();
        let pattern_row = pattern_row.clone();
        page.connect_shown(move |_| {
            let s = snap.borrow();
            sound_row.set_subtitle(&sound_name(&app, s.sound_uuid.as_str()));
            pattern_row.set_subtitle(&pattern_name(&app, s.vibration_pattern_uuid.as_str()));
        });
    }

    // Volume: save-as-you-go like the other fields, then play the
    // bell at the new level. Leaving the page stops that preview.
    let volume = crate::volume_row::VolumeRow::install(&volume_row, || Some(()), {
        let snap = snapshot.clone();
        let app = app.clone();
        let rebuilder = rebuilder.clone();
        let on_changed = on_changed.clone();
        let volume_row = volume_row.clone();
        move |(), volume| {
            snap.borrow_mut().volume = volume;
            write_back_bell(&volume_row, &app, &snap, &rebuilder, &on_changed);
            Some(snap.borrow().sound_uuid.to_string())
        }
    });
    volume.set_volume(initial_volume);
    page.connect_hidden(|_| crate::sound::stop_preview());

    // Kind changes also flip jitter row visibility.
    let snap_for_kind = snapshot.clone();
    let app_for_kind = app.clone();
    let rebuilder_for_kind = rebuilder.clone();
    let on_changed_for_kind = on_changed.clone();
    let jitter_row_clone = jitter_row.clone();
    let step_row_clone = step_row.clone();
    let populating_for_kind = populating.clone();
    kind_row.connect_selected_notify(move |row| {
        if populating_for_kind.get() { return; }
        let new_kind = match row.selected() {
            1 => IntervalBellKind::FixedFromStart,
            2 => IntervalBellKind::FixedFromEnd,
            _ => IntervalBellKind::Interval,
        };
        {
            let mut s = snap_for_kind.borrow_mut();
            s.kind = new_kind;
            (s.jitter, s.step) = meditate_core::bells::bell_kind_offsets(new_kind, s.jitter, s.step);
            populating_for_kind.set(true);
            jitter_row_clone.set(s.jitter);
            step_row_clone.set(s.step);
            populating_for_kind.set(false);
        }
        jitter_row_clone.row.set_visible(new_kind == IntervalBellKind::Interval);
        step_row_clone.row.set_visible(new_kind == IntervalBellKind::Interval);
        write_back_bell(row, &app_for_kind, &snap_for_kind, &rebuilder_for_kind, &on_changed_for_kind);
    });

    for row in [&time_row, &jitter_row, &step_row] {
        let field = row.field;
        let snap = snapshot.clone();
        let app = app.clone();
        let rebuilder = rebuilder.clone();
        let on_changed = on_changed.clone();
        let populating = populating.clone();
        row.connect_changed(move |row, time| {
            if populating.get() { return; }
            {
                let mut s = snap.borrow_mut();
                match field {
                    BellTimeField::Time => s.time = time,
                    BellTimeField::Jitter => s.jitter = time,
                    BellTimeField::Step => s.step = time,
                }
            }
            write_back_bell(row, &app, &snap, &rebuilder, &on_changed);
        });
    }

    // Sound activation: walk to window, push chooser, on pick update
    // the bell + the row subtitle. The chooser handles its own preview
    // playback so we don't need to gate on a populating flag here.
    let snap_for_sound = snapshot.clone();
    let app_for_sound = app.clone();
    let rebuilder_for_sound = rebuilder.clone();
    let on_changed_for_sound = on_changed.clone();
    let sound_row_for_sub = sound_row.clone();
    sound_row.connect_activated(move |row| {
        let Some(window) = row.root()
            .and_then(|r| r.downcast::<crate::window::MeditateWindow>().ok())
        else { return; };
        let current = Some(snap_for_sound.borrow().sound_uuid.0.clone());
        let snap = snap_for_sound.clone();
        let app_outer = app_for_sound.clone();
        let app_inner = app_for_sound.clone();
        let rebuilder = rebuilder_for_sound.clone();
        let on_changed = on_changed_for_sound.clone();
        let sound_row = sound_row_for_sub.clone();
        let volume = snap.borrow().volume;
        window.push_sound_chooser(
            &app_outer,
            crate::db::BellSoundCategory::General,
            current,
            volume,
            move |uuid| {
                snap.borrow_mut().sound_uuid = uuid.clone().into();
                sound_row.set_subtitle(&sound_name(&app_inner, &uuid));
                write_back_bell(&sound_row, &app_inner, &snap, &rebuilder, &on_changed);
            },
        );
    });

    // Pattern row — taps push the vibration-pattern chooser.
    let snap_for_pat = snapshot.clone();
    let app_for_pat = app.clone();
    let rebuilder_for_pat = rebuilder.clone();
    let on_changed_for_pat = on_changed.clone();
    let pattern_row_for_sub = pattern_row.clone();
    pattern_row.connect_activated(move |row| {
        let Some(window) = row.root()
            .and_then(|r| r.downcast::<crate::window::MeditateWindow>().ok())
        else { return; };
        let current = Some(snap_for_pat.borrow().vibration_pattern_uuid.0.clone());
        let snap = snap_for_pat.clone();
        let app_outer = app_for_pat.clone();
        let app_inner = app_for_pat.clone();
        let rebuilder = rebuilder_for_pat.clone();
        let on_changed = on_changed_for_pat.clone();
        let pattern_row = pattern_row_for_sub.clone();
        window.push_vibrations_chooser(&app_outer, current, move |uuid| {
            snap.borrow_mut().vibration_pattern_uuid = uuid.clone().into();
            pattern_row.set_subtitle(&pattern_name(&app_inner, &uuid));
            write_back_bell(&pattern_row, &app_inner, &snap, &rebuilder, &on_changed);
        });
    });

    // Signal-mode toggle: persists into the snap + drives row
    // visibility. Sound / Both reveals Bell Sound; Vibration / Both
    // reveals Pattern. On no-haptic devices the Vibration / Both
    // segments are insensitive (set above) but the persisted value
    // is preserved so a sync to a phone restores intent.
    let snap_for_sig = snapshot.clone();
    let app_for_sig = app.clone();
    let rebuilder_for_sig = rebuilder.clone();
    let on_changed_for_sig = on_changed.clone();
    let sound_row_for_sig = sound_row.clone();
    let volume_row_for_sig = volume_row.clone();
    let pattern_row_for_sig = pattern_row.clone();
    let populating_for_sig = populating.clone();
    signal_toggle.connect_active_name_notify(move |tg| {
        if populating_for_sig.get() { return; }
        let mode = tg.active_name()
            .and_then(|n| crate::db::SignalMode::from_db_str(n.as_str()))
            .unwrap_or(crate::db::SignalMode::Sound);
        crate::timer::show_no_vibration_note(tg);
        snap_for_sig.borrow_mut().signal_mode = mode;
        sound_row_for_sig.set_visible(mode.includes_sound());
        volume_row_for_sig.set_visible(mode.includes_sound());
        pattern_row_for_sig.set_visible(mode.includes_vibration());
        write_back_bell(tg, &app_for_sig, &snap_for_sig, &rebuilder_for_sig, &on_changed_for_sig);
    });

    // ── Delete ────────────────────────────────────────────────────
    let app_for_delete = app.clone();
    let snap_for_delete = snapshot.clone();
    let nav_view_for_delete = nav_view.clone();
    let rebuilder_for_delete = rebuilder.clone();
    let on_changed_for_delete = on_changed.clone();
    let page_for_delete = page.clone();
    delete_btn.connect_clicked(move |_| {
        let dialog = adw::AlertDialog::builder()
            .heading(gettext("Delete Bell?"))
            .body(gettext("This bell will no longer ring during sessions."))
            .close_response("cancel")
            .default_response("cancel")
            .build();
        dialog.add_response("cancel", &gettext("Cancel"));
        dialog.add_response("delete", &gettext("Delete"));
        dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);

        let app = app_for_delete.clone();
        let snap = snap_for_delete.clone();
        let nav_view = nav_view_for_delete.clone();
        let rebuilder = rebuilder_for_delete.clone();
        let on_changed = on_changed_for_delete.clone();
        let prefs_for_failure = prefs_page.clone();
        dialog.connect_response(None, move |_, id| {
            if id != "delete" { return; }
            let uuid = snap.borrow().uuid.clone();
            let deleted = app.with_db_mut(|db| db.delete_interval_bell(uuid.as_str()));
            if !write_ok(&prefs_for_failure, "bell.delete", deleted, Failed::Delete) {
                return;
            }
            if let Some(rb) = rebuilder.borrow().as_ref() { rb(); }
            on_changed();
            nav_view.pop();
        });

        if let Some(root) = page_for_delete.root() {
            if let Ok(window) = root.downcast::<gtk::Window>() {
                dialog.present(Some(&window));
            }
        }
    });

    // No need for actual populating-guard flips here because we set the
    // initial values on widget construction (via builder()), which
    // doesn't fire notify::* signals — set_value does, set_selected
    // does, but not the builder. Guard left in place only as a safety
    // hook for future reset flows.
    let _ = populating;

    nav_view.push(&page);
}

fn lookup_bell(app: &MeditateApplication, uuid: &str) -> Option<IntervalBell> {
    app.with_db(super::db::Database::list_interval_bells)
        .and_then(std::result::Result::ok)
        .unwrap_or_default()
        .into_iter()
        .find(|b| b.uuid == uuid)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(kind: IntervalBellKind, time: BellTime, jitter: BellTime, step: BellTime) -> IntervalBell {
        IntervalBell {
            id: 0,
            uuid: "u".into(),
            kind,
            time,
            jitter,
            step,
            sound_uuid: "bowl".into(),
            vibration_pattern_uuid: crate::db::BUNDLED_PATTERN_PULSE_UUID.into(),
            signal_mode: crate::db::SignalMode::Sound,
            enabled: true,
            created_iso: "2026-05-03T00:00:00Z".into(),
            volume: Default::default(),
        }
    }

    fn s(n: u32) -> BellTime { BellTime::new(n, TimeUnit::Sec) }
    fn m(n: u32) -> BellTime { BellTime::new(n, TimeUnit::Min) }
    const NONE: BellTime = BellTime::ZERO;

    #[test]
    fn bell_title_formats_interval_without_jitter_as_every_n_min() {
        assert_eq!(bell_title(&b(IntervalBellKind::Interval, m(5), NONE, NONE)), "Every 5 min");
    }

    #[test]
    fn bell_title_names_time_jitter_and_step_with_their_units() {
        assert_eq!(
            bell_title(&b(IntervalBellKind::Interval, s(120), m(1), s(5))),
            "Every 120 s ±1 min, +5 s"
        );
        assert_eq!(bell_title(&b(IntervalBellKind::Interval, s(45), NONE, NONE)), "Every 45 s");
        assert_eq!(bell_title(&b(IntervalBellKind::Interval, m(9), s(30), NONE)), "Every 9 min ±30 s");
    }

    #[test]
    fn bell_title_formats_fixed_from_start_as_at_n_min() {
        assert_eq!(bell_title(&b(IntervalBellKind::FixedFromStart, m(10), NONE, NONE)), "At 10 min");
        assert_eq!(bell_title(&b(IntervalBellKind::FixedFromStart, s(90), NONE, NONE)), "At 90 s");
    }

    #[test]
    fn bell_title_formats_fixed_from_end_as_n_min_before_end() {
        assert_eq!(bell_title(&b(IntervalBellKind::FixedFromEnd, m(5), NONE, NONE)), "5 min before end");
        assert_eq!(bell_title(&b(IntervalBellKind::FixedFromEnd, s(30), NONE, NONE)), "30 s before end");
    }

    fn production_fn(sig: &str) -> String {
        let src = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bells.rs")).unwrap();
        let code = src.split("#[cfg(test)]\nmod tests").next().unwrap().to_string();
        let at = code.find(sig).unwrap_or_else(|| panic!("{sig} not found"));
        let end = code[at..].find("\n}\n").unwrap();
        code[at..at + end].to_string()
    }

    #[test]
    fn bell_pages_reread_names_when_shown() {
        // #84: a sound or pattern renamed or deleted in a chooser
        // shows its new name (or "Missing") on coming back.
        let list = production_fn("pub fn push_bells_page<");
        let shown = list.find("page.connect_shown(").expect("the list re-reads when shown");
        assert!(list[shown..].contains("rb()"), "by rebuilding its rows");
        let edit = production_fn("fn push_edit_page(");
        let shown = edit.find("page.connect_shown(").expect("the edit page re-reads when shown");
        let after = &edit[shown..];
        assert!(after.contains("sound_name(") && after.contains("pattern_name("));
        // A failure on either page must show on that page.
        assert!(list.contains("adw::ToastOverlay::new()") && edit.contains("adw::ToastOverlay::new()"));
    }
}
