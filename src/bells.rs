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
use crate::db::{IntervalBell, IntervalBellKind};
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

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&prefs_page));

    let page = adw::NavigationPage::builder()
        .tag("interval-bells")
        .title(gettext("Interval Bells"))
        .child(&toolbar)
        .build();

    // Shared "rebuild + on_changed" closure stored behind a RefCell so
    // both the synthetic create-row and the edit-page callback can fire
    // it without us having to thread Clone through a self-referential
    // type. Set once below after the per-row handlers are built.
    let rebuilder: Rc<RefCell<Option<Box<dyn Fn()>>>> =
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
    nav_view.push(&page);
}

/// Drop all rows from the group and re-add them from the current DB
/// state. Called on initial push and after an add. Per-row Switch
/// toggles call `set_interval_bell_enabled` directly so they don't
/// need a rebuild.
type Rebuilder = Rc<RefCell<Option<Box<dyn Fn()>>>>;

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
    let stopwatch_on = app
        .with_db(|db| {
            db.get_setting("stopwatch_mode_active", "false")
                .map(|v| v == "true")
                .unwrap_or(false)
        })
        .unwrap_or(false);

    let bells = app
        .with_db(|db| db.list_interval_bells())
        .and_then(|r| r.ok())
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
    row.connect_activated(move |_| {
        let new_id = app_for_create.with_db_mut(|db| {
            db.insert_interval_bell(
                IntervalBellKind::Interval,
                5,
                0,
                crate::db::BUNDLED_BOWL_UUID,
                crate::db::BUNDLED_PATTERN_PULSE_UUID,
                crate::db::SignalMode::Sound,
            )
        }).and_then(|r| r.ok());

        if let Some(rb) = rebuilder_for_create.borrow().as_ref() {
            rb();
        }

        // Look up the just-inserted row's uuid (insert returns the
        // rowid, not the uuid) and drill into its edit page.
        if let Some(rowid) = new_id {
            let new_uuid = app_for_create
                .with_db(|db| db.list_interval_bells())
                .and_then(|r| r.ok())
                .unwrap_or_default()
                .into_iter()
                .find(|b| b.id == rowid)
                .map(|b| b.uuid);
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
        .subtitle(sound_label(app, &bell.sound))
        .activatable(true)
        .build();

    // Fixed-from-end bells are inert in stopwatch mode (no end to
    // count backwards from). Switch shows OFF + insensitive without
    // touching the persisted enabled flag — the user's previous
    // intent comes back as soon as stopwatch flips off.
    let inert = stopwatch_on && bell.kind == IntervalBellKind::FixedFromEnd;
    let switch = gtk::Switch::builder()
        .active(bell.enabled && !inert)
        .sensitive(!inert)
        .valign(gtk::Align::Center)
        .build();

    // Toggle persists immediately. No list rebuild needed for an
    // enabled flip — the row's title/subtitle don't change, only the
    // count subtitle on the timer setup page.
    let bell_uuid_for_toggle = bell.uuid.clone();
    let app_for_toggle = app.clone();
    let on_changed_for_toggle = on_changed.clone();
    switch.connect_active_notify(move |s| {
        app_for_toggle.with_db_mut(|db| {
            db.set_interval_bell_enabled(&bell_uuid_for_toggle, s.is_active())
        });
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
    let bell_uuid_for_del = bell.uuid.clone();
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

    let bell_uuid_for_edit = bell.uuid.clone();
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
    dialog.connect_response(None, move |_, id| {
        if id != "delete" { return; }
        app.with_db_mut(|db| db.delete_interval_bell(&uuid));
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
/// AdwActionRow title line on the Librem 5 portrait. Subtitle carries
/// the sound name; we don't pack everything into the title.
fn bell_title(bell: &IntervalBell) -> String {
    match bell.kind {
        IntervalBellKind::Interval => {
            if bell.jitter_pct == 0 {
                format!("Every {} min", bell.minutes)
            } else {
                format!("Every {} min ±{}%", bell.minutes, bell.jitter_pct)
            }
        }
        IntervalBellKind::FixedFromStart => format!("At {} min", bell.minutes),
        IntervalBellKind::FixedFromEnd => format!("{} min before end", bell.minutes),
    }
}

/// Look up a bell-sound's display name from the bell_sounds library.
/// Empty string if the uuid is empty or stale (post-wipe legacy
/// values point at no row); the row will just have no subtitle until
/// the user re-picks via the chooser.
fn sound_label(app: &MeditateApplication, uuid: &str) -> String {
    if uuid.is_empty() {
        return String::new();
    }
    app.with_db(|db| db.list_bell_sounds())
        .and_then(|r| r.ok())
        .unwrap_or_default()
        .into_iter()
        .find(|s| s.uuid == uuid)
        .map(|s| s.name)
        .unwrap_or_default()
}

/// Same as sound_label but for vibration_patterns.
fn pattern_label(app: &MeditateApplication, uuid: &str) -> String {
    if uuid.is_empty() {
        return String::new();
    }
    app.with_db(|db| db.find_vibration_pattern_by_uuid(uuid))
        .and_then(|r| r.ok())
        .flatten()
        .map(|p| p.name)
        .unwrap_or_default()
}

/// Edit page for one bell — pushed when the user taps a row in the
/// list. Save-as-you-go: every field change persists immediately
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
        gettext("Every N minutes"),
        gettext("At time from start"),
        gettext("Before end"),
    ];
    let kind_refs: Vec<&str> = kind_choices.iter().map(|s| s.as_str()).collect();
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

    // Minutes spinner — common to all kinds; semantic shifts with kind.
    let minutes_row = adw::SpinRow::builder()
        .title(gettext("Minutes"))
        .adjustment(&gtk::Adjustment::new(
            bell.minutes as f64, 1.0, 120.0, 1.0, 5.0, 0.0,
        ))
        .build();
    form.add(&minutes_row);

    // Jitter spinner — only meaningful for the Interval kind. Visible
    // gates on kind below.
    let jitter_row = adw::SpinRow::builder()
        .title(gettext("Jitter"))
        .subtitle(gettext("Percent — randomises the next ring"))
        .adjustment(&gtk::Adjustment::new(
            bell.jitter_pct as f64, 0.0, 50.0, 5.0, 10.0, 0.0,
        ))
        .visible(bell.kind == IntervalBellKind::Interval)
        .build();
    form.add(&jitter_row);

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
    if !app.has_haptic() {
        toggle_vibration.set_enabled(false);
        toggle_both.set_enabled(false);
    }
    signal_toggle.add(toggle_sound);
    signal_toggle.add(toggle_vibration);
    signal_toggle.add(toggle_both);
    let initial_mode = if !app.has_haptic() {
        crate::db::SignalMode::Sound
    } else {
        bell.signal_mode
    };
    signal_toggle.set_active_name(Some(match initial_mode {
        crate::db::SignalMode::Sound     => "sound",
        crate::db::SignalMode::Vibration => "vibration",
        crate::db::SignalMode::Both      => "both",
    }));
    signal_toggle_host.append(&signal_toggle);

    // Sound row — taps push the bell-sound chooser. Subtitle shows the
    // currently-selected sound's name (looked up by uuid).
    let sound_row = adw::ActionRow::builder()
        .title(gettext("Sound"))
        .subtitle(sound_label(app, &bell.sound))
        .activatable(true)
        .build();
    {
        let chevron = gtk::Image::from_icon_name("go-next-symbolic");
        chevron.add_css_class("dim-label");
        sound_row.add_suffix(&chevron);
    }
    form.add(&sound_row);

    // Pattern row — taps push the vibration-pattern chooser.
    let pattern_row = adw::ActionRow::builder()
        .title(gettext("Pattern"))
        .subtitle(pattern_label(app, &bell.vibration_pattern_uuid))
        .activatable(true)
        .build();
    {
        let chevron = gtk::Image::from_icon_name("go-next-symbolic");
        chevron.add_css_class("dim-label");
        pattern_row.add_suffix(&chevron);
    }
    form.add(&pattern_row);

    // Initial visibility based on the saved signal mode.
    sound_row.set_visible(matches!(
        initial_mode,
        crate::db::SignalMode::Sound | crate::db::SignalMode::Both,
    ));
    pattern_row.set_visible(matches!(
        initial_mode,
        crate::db::SignalMode::Vibration | crate::db::SignalMode::Both,
    ));

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
    toolbar.set_content(Some(&prefs_page));
    let page = adw::NavigationPage::builder()
        .tag("interval-bell-edit")
        .title(gettext("Edit Bell"))
        .child(&toolbar)
        .build();

    // ── Save-as-you-go wiring ──────────────────────────────────────
    // Snapshot of the current bell's state lives behind a RefCell so
    // each per-field handler reads the OTHER fields without re-querying
    // the DB. Updated whenever a handler writes back.
    let snapshot = Rc::new(RefCell::new(bell));
    let populating = Rc::new(std::cell::Cell::new(false));

    fn write_back(
        app: &MeditateApplication,
        snap: &Rc<RefCell<IntervalBell>>,
        rebuilder: &Rebuilder,
        on_changed: &(impl Fn() + Clone + 'static),
    ) {
        let s = snap.borrow();
        app.with_db_mut(|db| {
            db.update_interval_bell(
                &s.uuid,
                s.kind,
                s.minutes,
                s.jitter_pct,
                &s.sound,
                &s.vibration_pattern_uuid,
                s.signal_mode,
                s.enabled,
            )
        });
        if let Some(rb) = rebuilder.borrow().as_ref() {
            rb();
        }
        on_changed();
    }

    // Kind changes also flip jitter row visibility.
    let snap_for_kind = snapshot.clone();
    let app_for_kind = app.clone();
    let rebuilder_for_kind = rebuilder.clone();
    let on_changed_for_kind = on_changed.clone();
    let jitter_row_clone = jitter_row.clone();
    let populating_for_kind = populating.clone();
    kind_row.connect_selected_notify(move |row| {
        if populating_for_kind.get() { return; }
        let new_kind = match row.selected() {
            1 => IntervalBellKind::FixedFromStart,
            2 => IntervalBellKind::FixedFromEnd,
            _ => IntervalBellKind::Interval,
        };
        snap_for_kind.borrow_mut().kind = new_kind;
        jitter_row_clone.set_visible(new_kind == IntervalBellKind::Interval);
        write_back(&app_for_kind, &snap_for_kind, &rebuilder_for_kind, &on_changed_for_kind);
    });

    let snap_for_min = snapshot.clone();
    let app_for_min = app.clone();
    let rebuilder_for_min = rebuilder.clone();
    let on_changed_for_min = on_changed.clone();
    let populating_for_min = populating.clone();
    minutes_row.connect_notify_local(Some("value"), move |row, _| {
        if populating_for_min.get() { return; }
        snap_for_min.borrow_mut().minutes = row.value().round().max(1.0) as u32;
        write_back(&app_for_min, &snap_for_min, &rebuilder_for_min, &on_changed_for_min);
    });

    let snap_for_jitter = snapshot.clone();
    let app_for_jitter = app.clone();
    let rebuilder_for_jitter = rebuilder.clone();
    let on_changed_for_jitter = on_changed.clone();
    let populating_for_jitter = populating.clone();
    jitter_row.connect_notify_local(Some("value"), move |row, _| {
        if populating_for_jitter.get() { return; }
        snap_for_jitter.borrow_mut().jitter_pct = row.value().round().clamp(0.0, 50.0) as u32;
        write_back(&app_for_jitter, &snap_for_jitter, &rebuilder_for_jitter, &on_changed_for_jitter);
    });

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
        let current = Some(snap_for_sound.borrow().sound.clone());
        let snap = snap_for_sound.clone();
        let app_outer = app_for_sound.clone();
        let app_inner = app_for_sound.clone();
        let rebuilder = rebuilder_for_sound.clone();
        let on_changed = on_changed_for_sound.clone();
        let sound_row = sound_row_for_sub.clone();
        window.push_sound_chooser(
            &app_outer,
            crate::db::BellSoundCategory::General,
            current,
            move |uuid| {
                snap.borrow_mut().sound = uuid.clone();
                sound_row.set_subtitle(&sound_label(&app_inner, &uuid));
                write_back(&app_inner, &snap, &rebuilder, &on_changed);
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
        let current = Some(snap_for_pat.borrow().vibration_pattern_uuid.clone());
        let snap = snap_for_pat.clone();
        let app_outer = app_for_pat.clone();
        let app_inner = app_for_pat.clone();
        let rebuilder = rebuilder_for_pat.clone();
        let on_changed = on_changed_for_pat.clone();
        let pattern_row = pattern_row_for_sub.clone();
        window.push_vibrations_chooser(&app_outer, current, move |uuid| {
            snap.borrow_mut().vibration_pattern_uuid = uuid.clone();
            pattern_row.set_subtitle(&pattern_label(&app_inner, &uuid));
            write_back(&app_inner, &snap, &rebuilder, &on_changed);
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
    let pattern_row_for_sig = pattern_row.clone();
    let populating_for_sig = populating.clone();
    signal_toggle.connect_active_name_notify(move |tg| {
        if populating_for_sig.get() { return; }
        let mode = match tg.active_name().as_deref() {
            Some("vibration") => crate::db::SignalMode::Vibration,
            Some("both")      => crate::db::SignalMode::Both,
            _                 => crate::db::SignalMode::Sound,
        };
        snap_for_sig.borrow_mut().signal_mode = mode;
        sound_row_for_sig.set_visible(matches!(
            mode,
            crate::db::SignalMode::Sound | crate::db::SignalMode::Both,
        ));
        pattern_row_for_sig.set_visible(matches!(
            mode,
            crate::db::SignalMode::Vibration | crate::db::SignalMode::Both,
        ));
        write_back(&app_for_sig, &snap_for_sig, &rebuilder_for_sig, &on_changed_for_sig);
    });

    // ── Delete ────────────────────────────────────────────────────
    let app_for_delete = app.clone();
    let bell_uuid = bell_uuid.to_string();
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
        let uuid = bell_uuid.clone();
        let nav_view = nav_view_for_delete.clone();
        let rebuilder = rebuilder_for_delete.clone();
        let on_changed = on_changed_for_delete.clone();
        let _page_ref = page_for_delete.clone();
        dialog.connect_response(None, move |_, id| {
            if id != "delete" { return; }
            app.with_db_mut(|db| db.delete_interval_bell(&uuid));
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
    app.with_db(|db| db.list_interval_bells())
        .and_then(|r| r.ok())
        .unwrap_or_default()
        .into_iter()
        .find(|b| b.uuid == uuid)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(kind: IntervalBellKind, minutes: u32, jitter_pct: u32, sound: &str) -> IntervalBell {
        IntervalBell {
            id: 0,
            uuid: "u".into(),
            kind,
            minutes,
            jitter_pct,
            sound: sound.into(),
            vibration_pattern_uuid: crate::db::BUNDLED_PATTERN_PULSE_UUID.into(),
            signal_mode: crate::db::SignalMode::Sound,
            enabled: true,
            created_iso: "2026-05-03T00:00:00Z".into(),
        }
    }

    #[test]
    fn bell_title_formats_interval_without_jitter_as_every_n_min() {
        assert_eq!(
            bell_title(&b(IntervalBellKind::Interval, 5, 0, "bowl")),
            "Every 5 min"
        );
    }

    #[test]
    fn bell_title_formats_interval_with_jitter_as_every_n_min_plus_minus_pct() {
        assert_eq!(
            bell_title(&b(IntervalBellKind::Interval, 9, 30, "bell")),
            "Every 9 min ±30%"
        );
    }

    #[test]
    fn bell_title_formats_fixed_from_start_as_at_n_min() {
        assert_eq!(
            bell_title(&b(IntervalBellKind::FixedFromStart, 10, 0, "bowl")),
            "At 10 min"
        );
    }

    #[test]
    fn bell_title_formats_fixed_from_end_as_n_min_before_end() {
        assert_eq!(
            bell_title(&b(IntervalBellKind::FixedFromEnd, 5, 0, "gong")),
            "5 min before end"
        );
    }
}
