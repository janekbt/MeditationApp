//! Preset chooser — the NavigationPage pushed when the user taps
//! "Save Settings" or "Manage Presets" in the Setup view.
//!
//! Modeled on `src/labels.rs` and `src/sounds.rs`. Two modes (Save /
//! Manage) share one rebuilder so the row layout is consistent
//! between them; each mode adds the affordances its UX needs.
//!
//! - **Save mode**: synthetic "Create new preset…" row at the top
//!   (opens a naming dialog); tapping an existing row pops a
//!   confirmation ("Override 'X' with current settings?") that
//!   writes the live snapshot into that preset and pops the page.
//! - **Manage mode**: no synthetic create row; tapping a row body is
//!   a no-op; rename + delete buttons sit as suffixes on each row.
//!
//! Star toggle (prefix on every row) works in both modes — it
//! mutates `is_starred` directly so the home-view chip list refreshes
//! through the caller's `on_changed` hook.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;

use crate::application::MeditateApplication;
use crate::db::{Preset, SessionMode};
use crate::i18n::gettext;
use crate::preset_config::PresetConfig;

/// Two-mode chooser parameter. The `Save` variant carries the live
/// Setup snapshot the caller wants to persist; the `Manage` variant
/// has no payload.
pub enum ChooserMode {
    Save { snapshot: PresetConfig },
    Manage,
}

/// Push the preset chooser onto the navigation view. `mode` filters
/// the listing strictly to one SessionMode (Timer vs BoxBreath); the
/// chooser never crosses modes — the user has to switch the Setup
/// view's mode toggle to see the other mode's presets. `on_changed`
/// fires after any DB write (create / rename / delete / star toggle
/// / overwrite) so the caller can refresh the home view's starred
/// list.
pub fn push_presets_chooser(
    nav_view: &adw::NavigationView,
    app: &MeditateApplication,
    mode: SessionMode,
    chooser_mode: ChooserMode,
    on_changed: impl Fn() + 'static,
) {
    let group = adw::PreferencesGroup::new();
    let prefs_page = adw::PreferencesPage::new();
    prefs_page.add(&group);

    let title = match chooser_mode {
        ChooserMode::Save { .. } => gettext("Save Preset"),
        ChooserMode::Manage      => gettext("Manage Presets"),
    };
    let header = adw::HeaderBar::builder().show_back_button(true).build();
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&prefs_page));

    let page = adw::NavigationPage::builder()
        .tag("presets-chooser")
        .title(title)
        .child(&toolbar)
        .build();

    let chooser_mode = Rc::new(chooser_mode);
    let on_changed: Rc<dyn Fn()> = Rc::new(on_changed);
    let nav_view_clone = nav_view.clone();
    let rows: Rc<RefCell<Vec<gtk::Widget>>> = Rc::new(RefCell::new(Vec::new()));

    let rebuilder: Rc<RefCell<Option<Box<dyn Fn()>>>> =
        Rc::new(RefCell::new(None));

    let group_for_rb = group.clone();
    let rows_for_rb = rows.clone();
    let app_for_rb = app.clone();
    let nav_view_for_rb = nav_view_clone.clone();
    let chooser_mode_for_rb = chooser_mode.clone();
    let on_changed_for_rb = on_changed.clone();
    let rebuilder_for_self = rebuilder.clone();
    *rebuilder.borrow_mut() = Some(Box::new(move || {
        rebuild_chooser_rows(
            &group_for_rb,
            &rows_for_rb,
            &app_for_rb,
            mode,
            &nav_view_for_rb,
            chooser_mode_for_rb.clone(),
            on_changed_for_rb.clone(),
            rebuilder_for_self.clone(),
        );
    }));

    if let Some(rb) = rebuilder.borrow().as_ref() {
        rb();
    }

    nav_view.push(&page);
}

fn rebuild_chooser_rows(
    group: &adw::PreferencesGroup,
    rows: &Rc<RefCell<Vec<gtk::Widget>>>,
    app: &MeditateApplication,
    mode: SessionMode,
    nav_view: &adw::NavigationView,
    chooser_mode: Rc<ChooserMode>,
    on_changed: Rc<dyn Fn()>,
    rebuilder: Rc<RefCell<Option<Box<dyn Fn()>>>>,
) {
    for row in rows.borrow_mut().drain(..) {
        group.remove(&row);
    }

    // Synthetic "Create new preset…" entry — Save mode only. In
    // Manage mode taps shouldn't create new presets (we'd lack a
    // snapshot to save into them).
    if matches!(*chooser_mode, ChooserMode::Save { .. }) {
        let create_row = adw::ActionRow::builder()
            .title(gettext("Create new preset…"))
            .activatable(true)
            .build();
        let plus = gtk::Image::from_icon_name("list-add-symbolic");
        plus.add_css_class("dim-label");
        create_row.add_suffix(&plus);
        let app_for_create = app.clone();
        let chooser_mode_for_create = chooser_mode.clone();
        let nav_view_for_create = nav_view.clone();
        let on_changed_for_create = on_changed.clone();
        create_row.connect_activated(move |row| {
            let snapshot = match &*chooser_mode_for_create {
                ChooserMode::Save { snapshot } => snapshot.clone(),
                ChooserMode::Manage => return,
            };
            let app = app_for_create.clone();
            let nav_view = nav_view_for_create.clone();
            let on_changed = on_changed_for_create.clone();
            present_create_preset_dialog(
                row,
                &app_for_create,
                Box::new(move |name| {
                    // Create as starred so the new preset shows up in
                    // the home-view chip list immediately. The user
                    // can destar from Manage if they want it hidden.
                    let json = snapshot.to_json();
                    let result = app.with_db_mut(
                        |db| db.insert_preset(&name, mode, true, &json),
                    );
                    if matches!(result, Some(Ok(_))) {
                        on_changed();
                        nav_view.pop();
                    }
                }),
            );
        });
        group.add(&create_row);
        rows.borrow_mut().push(create_row.upcast());
    }

    let presets = app
        .with_db(|db| db.list_presets_for_mode(mode))
        .and_then(|r| r.ok())
        .unwrap_or_default();
    for preset in presets {
        let row = build_preset_row(
            &preset, app, &chooser_mode, nav_view,
            on_changed.clone(), rebuilder.clone(),
        );
        group.add(&row);
        rows.borrow_mut().push(row.upcast());
    }
}

fn build_preset_row(
    preset: &Preset,
    app: &MeditateApplication,
    chooser_mode: &Rc<ChooserMode>,
    nav_view: &adw::NavigationView,
    on_changed: Rc<dyn Fn()>,
    rebuilder: Rc<RefCell<Option<Box<dyn Fn()>>>>,
) -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .title(&preset.name)
        .subtitle(&subtitle_for(preset))
        .activatable(matches!(**chooser_mode, ChooserMode::Save { .. }))
        .build();

    // Star prefix — accent-coloured filled star when on, dimmed
    // outline when off. Live in both modes: the chooser_mode only
    // affects how row activation behaves, not whether you can change
    // the pin state.
    let star_btn = build_star_button(preset, app, on_changed.clone(), rebuilder.clone());
    row.add_prefix(&star_btn);

    // Manage-only suffixes: rename + delete buttons. In Save mode
    // taps on the row body trigger the override dialog instead.
    if matches!(**chooser_mode, ChooserMode::Manage) {
        add_rename_button(&row, preset, app, rebuilder.clone(), on_changed.clone());
        add_delete_button(&row, preset, app, rebuilder, on_changed.clone());
    }

    if let ChooserMode::Save { snapshot } = &**chooser_mode {
        let preset_uuid = preset.uuid.clone();
        let preset_name = preset.name.clone();
        let snapshot = snapshot.clone();
        let app = app.clone();
        let nav_view = nav_view.clone();
        let on_changed = on_changed.clone();
        row.connect_activated(move |btn| {
            let preset_uuid = preset_uuid.clone();
            let snapshot = snapshot.clone();
            let app = app.clone();
            let nav_view = nav_view.clone();
            let on_changed = on_changed.clone();
            present_override_dialog(
                btn,
                &preset_name,
                Box::new(move || {
                    let json = snapshot.to_json();
                    app.with_db_mut(|db| {
                        let _ = db.update_preset_config(&preset_uuid, &json);
                    });
                    on_changed();
                    nav_view.pop();
                }),
            );
        });
    }
    row
}

fn build_star_button(
    preset: &Preset,
    app: &MeditateApplication,
    on_changed: Rc<dyn Fn()>,
    rebuilder: Rc<RefCell<Option<Box<dyn Fn()>>>>,
) -> gtk::Button {
    let icon_name = if preset.is_starred {
        "starred-symbolic"
    } else {
        "non-starred-symbolic"
    };
    let icon = gtk::Image::from_icon_name(icon_name);
    if preset.is_starred {
        icon.add_css_class("preset-star-on");
    } else {
        icon.add_css_class("dimmed");
    }
    let btn = gtk::Button::builder()
        .child(&icon)
        .css_classes(["flat", "circular"])
        .valign(gtk::Align::Center)
        .tooltip_text(if preset.is_starred {
            gettext("Remove from home list")
        } else {
            gettext("Pin to home list")
        })
        .build();
    let app = app.clone();
    let preset_uuid = preset.uuid.clone();
    let new_starred = !preset.is_starred;
    btn.connect_clicked(move |_| {
        app.with_db_mut(|db| {
            let _ = db.update_preset_starred(&preset_uuid, new_starred);
        });
        on_changed();
        if let Some(rb) = rebuilder.borrow().as_ref() { rb(); }
    });
    btn
}

fn add_rename_button(
    row: &adw::ActionRow,
    preset: &Preset,
    app: &MeditateApplication,
    rebuilder: Rc<RefCell<Option<Box<dyn Fn()>>>>,
    on_changed: Rc<dyn Fn()>,
) {
    let rename_btn = gtk::Button::builder()
        .icon_name("document-edit-symbolic")
        .tooltip_text(gettext("Rename"))
        .css_classes(["flat", "circular"])
        .valign(gtk::Align::Center)
        .build();
    let app = app.clone();
    let preset_uuid = preset.uuid.clone();
    let preset_name = preset.name.clone();
    rename_btn.connect_clicked(move |btn| {
        present_rename_preset_dialog(
            btn, &app, &preset_uuid, &preset_name,
            rebuilder.clone(), on_changed.clone(),
        );
    });
    row.add_suffix(&rename_btn);
}

fn add_delete_button(
    row: &adw::ActionRow,
    preset: &Preset,
    app: &MeditateApplication,
    rebuilder: Rc<RefCell<Option<Box<dyn Fn()>>>>,
    on_changed: Rc<dyn Fn()>,
) {
    let delete_btn = gtk::Button::builder()
        .icon_name("user-trash-symbolic")
        .tooltip_text(gettext("Delete preset"))
        .css_classes(["flat", "circular", "destructive-action"])
        .valign(gtk::Align::Center)
        .build();
    let app = app.clone();
    let preset_uuid = preset.uuid.clone();
    let preset_name = preset.name.clone();
    delete_btn.connect_clicked(move |btn| {
        present_delete_preset_dialog(
            btn, &app, &preset_uuid, &preset_name,
            rebuilder.clone(), on_changed.clone(),
        );
    });
    row.add_suffix(&delete_btn);
}

fn present_create_preset_dialog(
    anchor: &adw::ActionRow,
    app: &MeditateApplication,
    on_created: Box<dyn Fn(String)>,
) {
    let entry = gtk::Entry::builder()
        .placeholder_text(gettext("Preset name"))
        .activates_default(true)
        .build();

    let dialog = adw::AlertDialog::builder()
        .heading(gettext("Create Preset"))
        .extra_child(&entry)
        .close_response("cancel")
        .default_response("create")
        .build();
    dialog.add_response("cancel", &gettext("Cancel"));
    dialog.add_response("create", &gettext("Create"));
    dialog.set_response_appearance("create", adw::ResponseAppearance::Suggested);
    dialog.set_response_enabled("create", false);

    // Live validation: non-empty + no name collision against any
    // existing preset (case-insensitive, matches the COLLATE NOCASE
    // UNIQUE on the column).
    let validate: Rc<dyn Fn()> = {
        let app = app.clone();
        let entry = entry.clone();
        let dialog = dialog.clone();
        Rc::new(move || {
            let text = entry.text();
            let trimmed = text.trim();
            let collision = if trimmed.is_empty() {
                false
            } else {
                app.with_db(|db| db.is_preset_name_taken(trimmed, ""))
                    .and_then(|r| r.ok())
                    .unwrap_or(false)
            };
            dialog.set_response_enabled("create", !trimmed.is_empty() && !collision);
        })
    };
    let validate_for_change = validate.clone();
    entry.connect_changed(move |_| validate_for_change());

    let on_created = Rc::new(on_created);
    let entry_for_response = entry.clone();
    dialog.connect_response(None, move |_, id| {
        if id != "create" { return; }
        let name = entry_for_response.text().trim().to_string();
        if name.is_empty() { return; }
        on_created(name);
    });

    if let Some(root) = anchor.root() {
        if let Ok(window) = root.downcast::<gtk::Window>() {
            dialog.present(Some(&window));
            entry.grab_focus();
        }
    }
}

fn present_rename_preset_dialog(
    anchor: &gtk::Button,
    app: &MeditateApplication,
    preset_uuid: &str,
    current_name: &str,
    rebuilder: Rc<RefCell<Option<Box<dyn Fn()>>>>,
    on_changed: Rc<dyn Fn()>,
) {
    let entry = gtk::Entry::builder()
        .text(current_name)
        .activates_default(true)
        .build();

    let dialog = adw::AlertDialog::builder()
        .heading(gettext("Rename Preset"))
        .extra_child(&entry)
        .close_response("cancel")
        .default_response("rename")
        .build();
    dialog.add_response("cancel", &gettext("Cancel"));
    dialog.add_response("rename", &gettext("Rename"));
    dialog.set_response_appearance("rename", adw::ResponseAppearance::Suggested);

    let validate: Rc<dyn Fn()> = {
        let app = app.clone();
        let entry = entry.clone();
        let dialog = dialog.clone();
        let preset_uuid = preset_uuid.to_string();
        Rc::new(move || {
            let text = entry.text();
            let trimmed = text.trim();
            let collision = app
                .with_db(|db| db.is_preset_name_taken(trimmed, &preset_uuid))
                .and_then(|r| r.ok())
                .unwrap_or(false);
            dialog.set_response_enabled("rename", !trimmed.is_empty() && !collision);
        })
    };
    validate();
    let validate_for_change = validate.clone();
    entry.connect_changed(move |_| validate_for_change());

    let app = app.clone();
    let preset_uuid = preset_uuid.to_string();
    let entry_for_response = entry.clone();
    dialog.connect_response(None, move |_, id| {
        if id != "rename" { return; }
        let new_name = entry_for_response.text().trim().to_string();
        if new_name.is_empty() { return; }
        app.with_db_mut(|db| { let _ = db.update_preset_name(&preset_uuid, &new_name); });
        on_changed();
        if let Some(rb) = rebuilder.borrow().as_ref() { rb(); }
    });

    if let Some(root) = anchor.root() {
        if let Ok(window) = root.downcast::<gtk::Window>() {
            dialog.present(Some(&window));
            entry.grab_focus();
        }
    }
}

fn present_delete_preset_dialog(
    anchor: &gtk::Button,
    app: &MeditateApplication,
    preset_uuid: &str,
    preset_name: &str,
    rebuilder: Rc<RefCell<Option<Box<dyn Fn()>>>>,
    on_changed: Rc<dyn Fn()>,
) {
    let body = gettext("'{name}' will be removed from this device and any synced peers.")
        .replace("{name}", preset_name);
    let dialog = adw::AlertDialog::builder()
        .heading(gettext("Delete Preset?"))
        .body(body)
        .close_response("cancel")
        .default_response("cancel")
        .build();
    dialog.add_response("cancel", &gettext("Cancel"));
    dialog.add_response("delete", &gettext("Delete"));
    dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);

    let app = app.clone();
    let preset_uuid = preset_uuid.to_string();
    dialog.connect_response(None, move |_, id| {
        if id != "delete" { return; }
        app.with_db_mut(|db| { let _ = db.delete_preset(&preset_uuid); });
        on_changed();
        if let Some(rb) = rebuilder.borrow().as_ref() { rb(); }
    });

    if let Some(root) = anchor.root() {
        if let Ok(window) = root.downcast::<gtk::Window>() {
            dialog.present(Some(&window));
        }
    }
}

fn present_override_dialog(
    anchor: &adw::ActionRow,
    preset_name: &str,
    on_confirmed: Box<dyn Fn()>,
) {
    let body = gettext("Replace '{name}'s saved configuration with the current settings?")
        .replace("{name}", preset_name);
    let dialog = adw::AlertDialog::builder()
        .heading(gettext("Override Preset?"))
        .body(body)
        .close_response("cancel")
        .default_response("override")
        .build();
    dialog.add_response("cancel", &gettext("Cancel"));
    dialog.add_response("override", &gettext("Override"));
    dialog.set_response_appearance("override", adw::ResponseAppearance::Suggested);

    let on_confirmed = Rc::new(on_confirmed);
    dialog.connect_response(None, move |_, id| {
        if id != "override" { return; }
        on_confirmed();
    });

    if let Some(root) = anchor.root() {
        if let Ok(window) = root.downcast::<gtk::Window>() {
            dialog.present(Some(&window));
        }
    }
}

/// One-line subtitle on a chooser row. Matches the home-view chip
/// list's subtitle format so a preset reads the same in both places.
fn subtitle_for(p: &Preset) -> String {
    use crate::preset_config::PresetTiming;
    let cfg = match PresetConfig::from_json(&p.config_json) {
        Ok(c) => c,
        Err(_) => return String::new(),
    };
    match cfg.timing {
        PresetTiming::Timer { stopwatch: true, .. } => gettext("Stopwatch"),
        PresetTiming::Timer { stopwatch: false, duration_secs } => {
            let mins = duration_secs / 60;
            gettext("{n} min").replace("{n}", &mins.to_string())
        }
        PresetTiming::BoxBreath {
            inhale_secs, hold_full_secs, exhale_secs, hold_empty_secs,
            duration_minutes,
        } => format!(
            "{}-{}-{}-{} · {}",
            inhale_secs, hold_full_secs, exhale_secs, hold_empty_secs,
            gettext("{n} min").replace("{n}", &duration_minutes.to_string()),
        ),
    }
}
