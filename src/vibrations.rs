//! Vibration-pattern chooser — the NavigationPage pushed when the
//! user taps a vibration-pattern row in a per-bell or per-phase
//! configuration screen, OR when they open "Manage vibration patterns"
//! from Preferences. Lists every row in the `vibration_patterns`
//! library (bundled + custom). Tapping a row body picks that pattern
//! and pops the page; the caller's `on_selected` callback receives
//! the chosen UUID.
//!
//! Mirrors `sounds.rs`'s shape: synthetic "Create custom pattern…"
//! top row that drills into the editor, per-row Rename, per-row
//! Delete (non-bundled only). The editor itself lands in the next
//! phasing step — for now the create row presents a toast pointing
//! at the prototype.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;

use crate::application::MeditateApplication;
use crate::db::VibrationPattern;
use crate::i18n::gettext;

/// Push the vibration-pattern chooser onto `nav_view` in selection
/// mode. `current_uuid` is the row to mark with a checkmark when the
/// page opens — pass `None` for "nothing selected yet". The
/// `on_selected` callback fires when the user taps a row body and
/// receives the chosen UUID; the page pops automatically afterward.
pub fn push_vibrations_chooser(
    nav_view: &adw::NavigationView,
    app: &MeditateApplication,
    current_uuid: Option<String>,
    on_selected: impl Fn(String) + 'static,
) {
    let group = adw::PreferencesGroup::new();
    let prefs_page = adw::PreferencesPage::new();
    prefs_page.add(&group);

    let toast_overlay = adw::ToastOverlay::new();
    toast_overlay.set_child(Some(&prefs_page));

    let header = adw::HeaderBar::builder().show_back_button(true).build();
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&toast_overlay));

    let page = adw::NavigationPage::builder()
        .tag("vibration-patterns-chooser")
        .title(gettext("Choose Vibration Pattern"))
        .child(&toolbar)
        .build();

    let on_selected = Rc::new(on_selected);
    let nav_view_clone = nav_view.clone();

    // Hold row refs so a rebuild can drain them — Adw.PreferencesGroup
    // wraps its children in an internal GtkBox so iterating the group
    // wouldn't return the rows we added.
    let rows: Rc<RefCell<Vec<gtk::Widget>>> = Rc::new(RefCell::new(Vec::new()));

    let rebuilder: Rc<RefCell<Option<Box<dyn Fn()>>>> = Rc::new(RefCell::new(None));

    let group_for_rb = group.clone();
    let rows_for_rb = rows.clone();
    let app_for_rb = app.clone();
    let nav_view_for_rb = nav_view_clone.clone();
    let current_uuid_for_rb = current_uuid.clone();
    let on_selected_for_rb = on_selected.clone();
    let toast_overlay_for_rb = toast_overlay.clone();
    let rebuilder_for_self = rebuilder.clone();
    *rebuilder.borrow_mut() = Some(Box::new(move || {
        rebuild_chooser_rows(
            &group_for_rb,
            &rows_for_rb,
            &app_for_rb,
            current_uuid_for_rb.as_deref(),
            &nav_view_for_rb,
            on_selected_for_rb.clone(),
            rebuilder_for_self.clone(),
            &toast_overlay_for_rb,
        );
    }));

    if let Some(rb) = rebuilder.borrow().as_ref() {
        rb();
    }

    nav_view.push(&page);
}

/// Drain every previously-added row, then rebuild from the current
/// `vibration_patterns` library state. The synthetic "Create custom
/// pattern…" row goes back at the top.
fn rebuild_chooser_rows(
    group: &adw::PreferencesGroup,
    rows: &Rc<RefCell<Vec<gtk::Widget>>>,
    app: &MeditateApplication,
    current_uuid: Option<&str>,
    nav_view: &adw::NavigationView,
    on_selected: Rc<dyn Fn(String)>,
    rebuilder: Rc<RefCell<Option<Box<dyn Fn()>>>>,
    _toast_overlay: &adw::ToastOverlay,
) {
    for row in rows.borrow_mut().drain(..) {
        group.remove(&row);
    }

    // "Create custom pattern…" — synthetic, always at the top.
    // Pushes the editor in create-new mode; on_saved triggers a
    // chooser rebuild so the new row appears immediately.
    let create_row = build_create_row(app, nav_view, rebuilder.clone());
    group.add(&create_row);
    rows.borrow_mut().push(create_row.upcast());

    let selection = SelectionContext {
        current_uuid: current_uuid.map(|s| s.to_string()),
        on_selected,
        nav_view: nav_view.clone(),
    };

    let patterns = app
        .with_db(|db| db.list_vibration_patterns())
        .and_then(|r| r.ok())
        .unwrap_or_default();
    for pattern in patterns {
        let row = build_pattern_row(&pattern, app, rebuilder.clone(), &selection);
        group.add(&row);
        rows.borrow_mut().push(row.upcast());
    }
}

/// Selection-mode parameters: tap-pick fires `on_selected` then pops
/// the nav view; `current_uuid` decorates the matching row with a
/// checkmark.
struct SelectionContext {
    current_uuid: Option<String>,
    on_selected: Rc<dyn Fn(String)>,
    nav_view: adw::NavigationView,
}

fn build_create_row(
    app: &MeditateApplication,
    nav_view: &adw::NavigationView,
    rebuilder: Rc<RefCell<Option<Box<dyn Fn()>>>>,
) -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .title(gettext("Create custom pattern…"))
        .activatable(true)
        .build();
    let plus = gtk::Image::from_icon_name("list-add-symbolic");
    plus.add_css_class("dim-label");
    row.add_suffix(&plus);

    let app = app.clone();
    let nav_view = nav_view.clone();
    row.connect_activated(move |_| {
        let rebuilder = rebuilder.clone();
        crate::vibration_editor::push_pattern_editor(
            &nav_view,
            &app,
            None,
            move |_uuid| {
                if let Some(rb) = rebuilder.borrow().as_ref() {
                    rb();
                }
            },
        );
    });
    row
}

fn build_pattern_row(
    pattern: &VibrationPattern,
    app: &MeditateApplication,
    rebuilder: Rc<RefCell<Option<Box<dyn Fn()>>>>,
    selection: &SelectionContext,
) -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .title(&pattern.name)
        .subtitle(if pattern.is_bundled {
            gettext("Bundled")
        } else {
            gettext("Custom")
        })
        .activatable(true)
        .build();

    if selection.current_uuid.as_deref() == Some(pattern.uuid.as_str()) {
        let check = gtk::Image::from_icon_name("object-select-symbolic");
        check.add_css_class("selected-check");
        row.add_suffix(&check);
    }

    if pattern.is_bundled {
        // Bundled rows stay permanent — the seed re-creates them on
        // every open anyway, and an accidental tombstone could
        // confuse a peer that hasn't seeded yet. Rename is the only
        // mutation we let through; the curve, duration, and kind are
        // the seed's identity.
        add_rename_button(&row, pattern, app, rebuilder);
    } else {
        // Edit covers rename + curve + duration + chart kind, so we
        // skip the standalone rename button here to avoid two
        // overlapping affordances.
        add_edit_button(&row, pattern, app, &selection.nav_view, rebuilder.clone());
        add_delete_button(&row, pattern, app, rebuilder);
    }

    let uuid = pattern.uuid.clone();
    let on_selected = selection.on_selected.clone();
    let nav_view = selection.nav_view.clone();
    row.connect_activated(move |_| {
        on_selected(uuid.clone());
        nav_view.pop();
    });
    row
}

fn add_edit_button(
    row: &adw::ActionRow,
    pattern: &VibrationPattern,
    app: &MeditateApplication,
    nav_view: &adw::NavigationView,
    rebuilder: Rc<RefCell<Option<Box<dyn Fn()>>>>,
) {
    let edit_btn = gtk::Button::builder()
        .icon_name("document-edit-symbolic")
        .tooltip_text(gettext("Edit pattern"))
        .css_classes(["flat", "circular"])
        .valign(gtk::Align::Center)
        .build();
    let app = app.clone();
    let nav_view = nav_view.clone();
    let pattern = pattern.clone();
    edit_btn.connect_clicked(move |_| {
        let rebuilder = rebuilder.clone();
        crate::vibration_editor::push_pattern_editor(
            &nav_view,
            &app,
            Some(pattern.clone()),
            move |_uuid| {
                if let Some(rb) = rebuilder.borrow().as_ref() {
                    rb();
                }
            },
        );
    });
    row.add_suffix(&edit_btn);
}

fn add_rename_button(
    row: &adw::ActionRow,
    pattern: &VibrationPattern,
    app: &MeditateApplication,
    rebuilder: Rc<RefCell<Option<Box<dyn Fn()>>>>,
) {
    let rename_btn = gtk::Button::builder()
        .icon_name("document-edit-symbolic")
        .tooltip_text(gettext("Rename"))
        .css_classes(["flat", "circular"])
        .valign(gtk::Align::Center)
        .build();
    let app = app.clone();
    let uuid = pattern.uuid.clone();
    let row_clone = row.clone();
    rename_btn.connect_clicked(move |btn| {
        present_rename_dialog(btn, &app, &uuid, &row_clone.title(), rebuilder.clone());
    });
    row.add_suffix(&rename_btn);
}

fn add_delete_button(
    row: &adw::ActionRow,
    pattern: &VibrationPattern,
    app: &MeditateApplication,
    rebuilder: Rc<RefCell<Option<Box<dyn Fn()>>>>,
) {
    let delete_btn = gtk::Button::builder()
        .icon_name("user-trash-symbolic")
        .tooltip_text(gettext("Delete pattern"))
        .css_classes(["flat", "circular", "destructive-action"])
        .valign(gtk::Align::Center)
        .build();
    let app = app.clone();
    let uuid = pattern.uuid.clone();
    delete_btn.connect_clicked(move |btn| {
        present_delete_dialog(btn, &app, &uuid, rebuilder.clone());
    });
    row.add_suffix(&delete_btn);
}

fn present_rename_dialog(
    anchor: &gtk::Button,
    app: &MeditateApplication,
    uuid: &str,
    current_name: &str,
    rebuilder: Rc<RefCell<Option<Box<dyn Fn()>>>>,
) {
    let entry = gtk::Entry::builder()
        .text(current_name)
        .activates_default(true)
        .build();

    let dialog = adw::AlertDialog::builder()
        .heading(gettext("Rename Pattern"))
        .extra_child(&entry)
        .close_response("cancel")
        .default_response("rename")
        .build();
    dialog.add_response("cancel", &gettext("Cancel"));
    dialog.add_response("rename", &gettext("Rename"));
    dialog.set_response_appearance("rename", adw::ResponseAppearance::Suggested);

    // Live validation — Rename is gated on a non-empty trimmed name
    // and no collision with another row's case-insensitive name.
    // Renaming-to-self (same uuid, same name modulo case) is allowed
    // so the user can normalise capitalisation without a false
    // collision.
    let validate: Rc<dyn Fn()> = {
        let app = app.clone();
        let uuid = uuid.to_string();
        let entry = entry.clone();
        let dialog = dialog.clone();
        Rc::new(move || {
            let text = entry.text();
            let trimmed = text.trim();
            let collision = app
                .with_db(|db| db.is_vibration_pattern_name_taken(trimmed, &uuid))
                .and_then(|r| r.ok())
                .unwrap_or(false);
            let valid = !trimmed.is_empty() && !collision;
            dialog.set_response_enabled("rename", valid);
        })
    };
    validate();
    let validate_for_change = validate.clone();
    entry.connect_changed(move |_| validate_for_change());

    let app = app.clone();
    let uuid = uuid.to_string();
    let entry_for_response = entry.clone();
    dialog.connect_response(None, move |_, id| {
        if id != "rename" {
            return;
        }
        let new_name = entry_for_response.text().to_string();
        let trimmed = new_name.trim();
        if trimmed.is_empty() {
            return;
        }
        // Read the current row to round-trip duration / intensities /
        // chart_kind through the update — those don't change on a
        // rename, but update_vibration_pattern wants every field.
        let snapshot = app
            .with_db(|db| db.find_vibration_pattern_by_uuid(&uuid))
            .and_then(|r| r.ok())
            .flatten();
        if let Some(p) = snapshot {
            app.with_db_mut(|db| {
                db.update_vibration_pattern(
                    &uuid, trimmed, p.duration_ms, &p.intensities, p.chart_kind,
                )
            });
        }
        if let Some(rb) = rebuilder.borrow().as_ref() {
            rb();
        }
    });

    if let Some(root) = anchor.root() {
        if let Ok(window) = root.downcast::<gtk::Window>() {
            dialog.present(Some(&window));
            entry.grab_focus();
        }
    }
}

fn present_delete_dialog(
    anchor: &gtk::Button,
    app: &MeditateApplication,
    uuid: &str,
    rebuilder: Rc<RefCell<Option<Box<dyn Fn()>>>>,
) {
    let dialog = adw::AlertDialog::builder()
        .heading(gettext("Delete Pattern?"))
        .body(gettext(
            "Bells and Box Breath phases that reference this pattern will lose their vibration.",
        ))
        .close_response("cancel")
        .default_response("cancel")
        .build();
    dialog.add_response("cancel", &gettext("Cancel"));
    dialog.add_response("delete", &gettext("Delete"));
    dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);

    let app = app.clone();
    let uuid = uuid.to_string();
    dialog.connect_response(None, move |_, id| {
        if id != "delete" {
            return;
        }
        app.with_db_mut(|db| db.delete_vibration_pattern(&uuid));
        if let Some(rb) = rebuilder.borrow().as_ref() {
            rb();
        }
    });

    if let Some(root) = anchor.root() {
        if let Ok(window) = root.downcast::<gtk::Window>() {
            dialog.present(Some(&window));
        }
    }
}
