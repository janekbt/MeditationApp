//! Interval-bell library UI — the NavigationPage pushed when the user
//! taps the "Interval Bells" row in the timer setup. Lists every
//! configured bell (regardless of enabled state) with a per-row Switch
//! to toggle enabled and a "+" headerbar button to add a new default
//! bell. The edit page reachable by tapping a row lands in B.3.3.B.

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

    let add_btn = gtk::Button::builder()
        .icon_name("list-add-symbolic")
        .tooltip_text(gettext("Add interval bell"))
        .build();
    header.pack_end(&add_btn);

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&prefs_page));

    let page = adw::NavigationPage::builder()
        .tag("interval-bells")
        .title(gettext("Interval Bells"))
        .child(&toolbar)
        .build();

    let rebuild = {
        let group = group.clone();
        let app = app.clone();
        let rows = rows.clone();
        let on_changed = on_changed.clone();
        move || rebuild_list(&group, &rows, &app, on_changed.clone())
    };

    let rebuild_for_add = rebuild.clone();
    let app_for_add = app.clone();
    let on_changed_for_add = on_changed.clone();
    add_btn.connect_clicked(move |_| {
        // Insert a sensible default — every 5 min, no jitter, bowl. The
        // user dials it in via the edit page (lands in B.3.3.B).
        app_for_add.with_db_mut(|db| {
            db.insert_interval_bell(IntervalBellKind::Interval, 5, 0, "bowl")
        });
        rebuild_for_add();
        on_changed_for_add();
    });

    rebuild();
    nav_view.push(&page);
}

/// Drop all rows from the group and re-add them from the current DB
/// state. Called on initial push and after an add. Per-row Switch
/// toggles call `set_interval_bell_enabled` directly so they don't
/// need a rebuild.
fn rebuild_list<F>(
    group: &adw::PreferencesGroup,
    rows: &Rc<RefCell<Vec<adw::ActionRow>>>,
    app: &MeditateApplication,
    on_changed: F,
)
where F: Fn() + Clone + 'static,
{
    for row in rows.borrow_mut().drain(..) {
        group.remove(&row);
    }

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
        let row = build_bell_row(&bell, app, on_changed.clone());
        group.add(&row);
        rows.borrow_mut().push(row);
    }
}

fn build_bell_row<F>(
    bell: &IntervalBell,
    app: &MeditateApplication,
    on_changed: F,
) -> adw::ActionRow
where F: Fn() + Clone + 'static,
{
    let row = adw::ActionRow::builder()
        .title(bell_title(bell))
        .subtitle(sound_label(&bell.sound))
        .build();

    let switch = gtk::Switch::builder()
        .active(bell.enabled)
        .valign(gtk::Align::Center)
        .build();

    // Toggle persists immediately. The bells_loading-style guards
    // aren't needed here because the rebuild path doesn't drive these
    // switches — it tears down the rows entirely and rebuilds.
    let bell_uuid = bell.uuid.clone();
    let app_for_toggle = app.clone();
    let on_changed_for_toggle = on_changed.clone();
    switch.connect_active_notify(move |s| {
        app_for_toggle.with_db_mut(|db| {
            db.set_interval_bell_enabled(&bell_uuid, s.is_active())
        });
        on_changed_for_toggle();
    });

    row.add_suffix(&switch);
    row.set_activatable_widget(Some(&switch));
    row
}

fn empty_state_row() -> adw::ActionRow {
    // Subdued helper text rendered as an unframed row with the same
    // .dim-label styling we use elsewhere for in-context hints.
    let row = adw::ActionRow::builder()
        .title(gettext("No interval bells configured"))
        .subtitle(gettext("Tap + to add one"))
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

fn sound_label(sound: &str) -> String {
    match sound {
        "bowl" => gettext("Singing bowl"),
        "bell" => gettext("Bell"),
        "gong" => gettext("Gong"),
        other => other.to_string(),
    }
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
