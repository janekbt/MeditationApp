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
//! mutates `is_starred` directly; the write re-marks the home view's
//! starred list (`MeditateApplication::with_db`).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use adw::prelude::*;

use crate::application::MeditateApplication;
use crate::db::{write_ok, Failed, Preset, SessionMode};
use crate::i18n::gettext;
use crate::name_dialog::{name_dialog, taken_in};
use meditate_core::preset_config::PresetConfig;

/// Two-mode chooser parameter. The `Save` variant carries the live
/// Setup snapshot the caller wants to persist; the `Manage` variant
/// has no payload. The snapshot is boxed so the enum's footprint
/// doesn't pay the full PresetConfig size on every Manage-mode push.
pub enum ChooserMode {
    Save { snapshot: Box<PresetConfig> },
    Manage,
}

/// Back to Setup with its starred list already showing the write
/// (the write only queues the rebuild).
fn back_to_setup(app: &MeditateApplication, nav_view: &adw::NavigationView) {
    use gtk::subclass::prelude::ObjectSubclassIsExt;
    if let Some(win) = app.main_window() {
        win.imp().timer_view.rebuild_starred_presets_list();
    }
    nav_view.pop();
}

/// Push the preset chooser onto the navigation view. `mode` filters
/// the listing strictly to one SessionMode (Timer vs BoxBreath); the
/// chooser never crosses modes — the user has to switch the Setup
/// view's mode toggle to see the other mode's presets.
pub fn push_presets_chooser(
    nav_view: &adw::NavigationView,
    app: &MeditateApplication,
    mode: SessionMode,
    chooser_mode: ChooserMode,
) {
    let group = adw::PreferencesGroup::new();
    let prefs_page = adw::PreferencesPage::new();
    prefs_page.add(&group);

    let title = match chooser_mode {
        ChooserMode::Save { .. } => gettext("Save Preset"),
        ChooserMode::Manage      => gettext("Manage Presets"),
    };
    let header = adw::HeaderBar::builder().show_back_button(true).build();

    // Per-chooser ToastOverlay. The window's primary toast_overlay
    // lives inside the "main" NavigationPage, which is off-screen
    // while a chooser is pushed — toasts pushed to the window-level
    // overlay would appear on the hidden page and look like nothing
    // happened. The local overlay anchors toasts to the chooser's
    // own content allocation so they show up where the user is
    // actually looking.
    let toast_overlay = adw::ToastOverlay::new();
    toast_overlay.set_child(Some(&prefs_page));

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&toast_overlay));

    let page = adw::NavigationPage::builder()
        .tag("presets-chooser")
        .title(title)
        .child(&toolbar)
        .build();

    let chooser_mode = Rc::new(chooser_mode);
    let nav_view_clone = nav_view.clone();
    let rows: Rc<RefCell<Vec<gtk::Widget>>> = Rc::new(RefCell::new(Vec::new()));

    let rebuilder: crate::Rebuilder =
        Rc::new(RefCell::new(None));

    let group_for_rb = group.clone();
    let rows_for_rb = rows.clone();
    let app_for_rb = app.clone();
    let nav_view_for_rb = nav_view_clone.clone();
    let toast_overlay_for_rb = toast_overlay.clone();
    let chooser_mode_for_rb = chooser_mode.clone();
    let rebuilder_for_self = rebuilder.clone();
    *rebuilder.borrow_mut() = Some(Box::new(move || {
        rebuild_chooser_rows(
            &group_for_rb,
            &rows_for_rb,
            &app_for_rb,
            mode,
            &nav_view_for_rb,
            &toast_overlay_for_rb,
            chooser_mode_for_rb.clone(),
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
    toast_overlay: &adw::ToastOverlay,
    chooser_mode: Rc<ChooserMode>,
    rebuilder: crate::Rebuilder,
) {
    for row in rows.borrow_mut().drain(..) {
        group.remove(&row);
    }

    // Once per rebuild, so each row's subtitle lookup is O(1).
    let label_names: HashMap<String, String> = app
        .with_db(meditate_core::db::label_names_from_db)
        .and_then(std::result::Result::ok)
        .unwrap_or_default();

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
        create_row.connect_activated(move |row| {
            let snapshot = match &*chooser_mode_for_create {
                ChooserMode::Save { snapshot } => (**snapshot).clone(),
                ChooserMode::Manage => return,
            };
            let app = app_for_create.clone();
            let nav_view = nav_view_for_create.clone();
            // Core stars a new preset, as on Android.
            name_dialog(
                row,
                &gettext("Create Preset"),
                "",
                &gettext("Preset name"),
                &gettext("Create"),
                taken_in(&app, |db, name| meditate_core::db::is_preset_name_taken_from_db(db, name, "")),
                move |name| {
                    let json = snapshot.to_json();
                    let created = app.with_db(|db| db.insert_preset(name, mode, &json).map(drop));
                    if matches!(created, Some(Ok(()))) {
                        back_to_setup(&app, &nav_view);
                    }
                    created
                },
            );
        });
        group.add(&create_row);
        rows.borrow_mut().push(create_row.upcast());
    }

    let presets = app
        .with_db(|db| meditate_core::db::list_presets_for_mode_from_db(db, mode))
        .and_then(std::result::Result::ok)
        .unwrap_or_default();
    for (index, preset) in presets.iter().enumerate() {
        let row = build_preset_row(
            preset, index, app, &chooser_mode, nav_view, toast_overlay,
            &label_names, rows, rebuilder.clone(),
        );
        group.add(&row);
        rows.borrow_mut().push(row.upcast());
    }
}

fn build_preset_row(
    preset: &Preset,
    index: usize,
    app: &MeditateApplication,
    chooser_mode: &Rc<ChooserMode>,
    nav_view: &adw::NavigationView,
    toast_overlay: &adw::ToastOverlay,
    label_names: &HashMap<String, String>,
    rows: &Rc<RefCell<Vec<gtk::Widget>>>,
    rebuilder: crate::Rebuilder,
) -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .title(&preset.name)
        .subtitle(crate::preset_subtitle::preset_subtitle(preset, label_names))
        .activatable(matches!(**chooser_mode, ChooserMode::Save { .. }))
        .build();

    // Star prefix — accent-coloured filled star when on, dimmed
    // outline when off. Live in both modes: the chooser_mode only
    // affects how row activation behaves, not whether you can change
    // the pin state.
    let star_btn = build_star_button(
        preset, app, rebuilder.clone(),
    );
    row.add_prefix(&star_btn);
    if matches!(**chooser_mode, ChooserMode::Manage) {
        let (app, rows, rebuilder) = (app.clone(), rows.clone(), rebuilder.clone());
        let move_to: MoveTo = Rc::new(move |anchor, uuid, to| {
            let written = app.with_db(|db| db.move_preset(uuid, to));
            write_ok(anchor, "preset.move", written, Failed::Save);
            if let Some(rb) = rebuilder.borrow().as_ref() { rb(); }
            let rows = rows.borrow();
            if let Some(row) = rows.get(to).or(rows.last()) { row.grab_focus(); }
        });
        add_reordering(&row, preset.uuid.0.clone(), index, move_to);
    }

    // Manage-only suffixes: rename + delete buttons. In Save mode
    // taps on the row body trigger the override dialog instead.
    if matches!(**chooser_mode, ChooserMode::Manage) {
        add_rename_button(&row, preset, app, rebuilder.clone());
        add_delete_button(
            &row, preset, app, rebuilder, toast_overlay,
        );
    }

    if let ChooserMode::Save { snapshot } = &**chooser_mode {
        let preset_uuid = preset.uuid.0.clone();
        let preset_name = preset.name.clone();
        let snapshot = snapshot.clone();
        let prior_config_json = preset.config_json.clone();
        let app = app.clone();
        let nav_view = nav_view.clone();
        row.connect_activated(move |btn| {
            let preset_uuid = preset_uuid.clone();
            let preset_name = preset_name.clone();
            let snapshot = snapshot.clone();
            let prior_config_json = prior_config_json.clone();
            let app = app.clone();
            let nav_view = nav_view.clone();
            let dialog_name = preset_name.clone();
            // The Undo toast goes on the window: Override pops the
            // chooser and its overlay with it. Taken at tap time, when
            // the row is in the window.
            let window = btn.root().and_downcast::<crate::window::MeditateWindow>();
            let row_for_failure = btn.clone();
            present_override_dialog(
                btn,
                &dialog_name,
                Box::new(move || {
                    let json = snapshot.to_json();
                    let written = app.with_db(|db| db.update_preset_config(&preset_uuid, &json));
                    // Failed: stay on the page, so Override can be pressed again.
                    if !write_ok(&row_for_failure, "preset.override", written, Failed::Save) {
                        return;
                    }
                    back_to_setup(&app, &nav_view);

                    // Undo: restore the prior config_json. The
                    // forward write emitted preset_update; the undo
                    // emits another preset_update with newer ts so
                    // peers converge on the restored state.
                    let app_undo = app.clone();
                    let prior_undo = prior_config_json.clone();
                    let preset_uuid_undo = preset_uuid.clone();
                    if let Some(window) = window.as_ref() {
                        let title = gettext("'{name}' overridden").replace("{name}", &preset_name);
                        let window_undo = window.clone();
                        let toast = crate::db::undo_toast(title, move || {
                            let written = app_undo.with_db(|db| {
                                db.update_preset_config(&preset_uuid_undo, &prior_undo)
                            });
                            write_ok(&window_undo, "preset.override.undo", written, Failed::Save);
                        }, || {});
                        crate::db::post_toast(window, Some("presets"), toast);
                    }
                }),
            );
        });
    }
    row
}

/// The move a reorderable row runs: (row, its drag key, new place).
pub(crate) type MoveTo = Rc<dyn Fn(&gtk::Widget, &str, usize)>;

/// A drag handle (added after the other prefixes: `add_prefix` puts
/// each new prefix in front), the row as a drop target, and Alt+Up /
/// Alt+Down on the focused row. `key` travels with the drag; a drop or
/// a key press calls `move_to` with the place in this row's list.
pub(crate) fn add_reordering(row: &adw::ActionRow, key: String, index: usize, move_to: MoveTo) {
    let handle = gtk::Image::from_icon_name("list-drag-handle-symbolic");
    handle.add_css_class("dimmed");
    handle.set_tooltip_text(Some(&gettext("Drag to reorder")));
    handle.set_cursor_from_name(Some("grab"));
    row.add_prefix(&handle);

    let source = gtk::DragSource::builder().actions(gtk::gdk::DragAction::MOVE).build();
    let uuid = key.clone();
    let grab = Rc::new(std::cell::Cell::new((0.0, 0.0)));
    let (row_weak, grab_at) = (row.downgrade(), grab.clone());
    source.connect_prepare(move |src, x, y| {
        let point = src.widget().zip(row_weak.upgrade())
            .and_then(|(handle, row)| handle.compute_point(&row, &gtk::graphene::Point::new(x as f32, y as f32)));
        grab_at.set(point.map_or((x, y), |p| (f64::from(p.x()), f64::from(p.y()))));
        Some(gtk::gdk::ContentProvider::for_value(&uuid.to_value()))
    });
    let row_weak = row.downgrade();
    source.connect_drag_begin(move |src, _| {
        if let Some(row) = row_weak.upgrade() {
            let (x, y) = grab.get();
            src.set_icon(Some(&gtk::WidgetPaintable::new(Some(&row))), x as i32, y as i32);
        }
    });
    handle.add_controller(source);

    let target = gtk::DropTarget::new(String::static_type(), gtk::gdk::DragAction::MOVE);
    let move_on_drop = move_to.clone();
    target.connect_drop(move |target, value, _, _| {
        // Only a row of this list, never text dragged in from another app.
        let local = target.current_drop().is_some_and(|drop| drop.drag().is_some());
        let (true, Ok(uuid), Some(row)) = (local, value.get::<String>(), target.widget()) else { return false; };
        // After the drop has finished: the rebuild destroys the dragged row.
        let move_on_drop = move_on_drop.clone();
        gtk::glib::idle_add_local_once(move || move_on_drop(&row, &uuid, index));
        true
    });
    row.add_controller(target);

    let keys = gtk::EventControllerKey::new();
    let uuid = key;
    keys.connect_key_pressed(move |keys, key, _, state| {
        if !state.contains(gtk::gdk::ModifierType::ALT_MASK) { return gtk::glib::Propagation::Proceed; }
        let to = match key {
            // Past the end, core keeps the preset where it is.
            gtk::gdk::Key::Up if index > 0 => index - 1,
            gtk::gdk::Key::Down => index + 1,
            gtk::gdk::Key::Up => return gtk::glib::Propagation::Stop,
            _ => return gtk::glib::Propagation::Proceed,
        };
        if let Some(row) = keys.widget() { move_to(&row, &uuid, to); }
        gtk::glib::Propagation::Stop
    });
    row.add_controller(keys);
}

fn build_star_button(
    preset: &Preset,
    app: &MeditateApplication,
    rebuilder: crate::Rebuilder,
) -> gtk::Button {
    use meditate_core::preset_config::StarVisualState;
    let star_state = StarVisualState::from_is_starred(preset.is_starred);
    let icon_name = match star_state {
        StarVisualState::Starred => "starred-symbolic",
        StarVisualState::Unstarred => "non-starred-symbolic",
    };
    let icon = gtk::Image::from_icon_name(icon_name);
    icon.add_css_class(match star_state {
        StarVisualState::Starred => "preset-star-on",
        StarVisualState::Unstarred => "dimmed",
    });
    let tooltip = match star_state {
        StarVisualState::Starred => gettext("Remove from home list"),
        StarVisualState::Unstarred => gettext("Pin to home list"),
    };
    let btn = gtk::Button::builder()
        .child(&icon)
        .css_classes(["flat", "circular"])
        .valign(gtk::Align::Center)
        .tooltip_text(tooltip)
        .build();
    let app = app.clone();
    let preset_uuid = preset.uuid.0.clone();
    let new_starred = meditate_core::db::StarredState::from_flag(!preset.is_starred);
    btn.connect_clicked(move |btn| {
        let written = app.with_db(|db| db.update_preset_starred(&preset_uuid, new_starred));
        write_ok(btn, "preset.star", written, Failed::Save);
        if let Some(rb) = rebuilder.borrow().as_ref() { rb(); }
    });
    btn
}

fn add_rename_button(
    row: &adw::ActionRow,
    preset: &Preset,
    app: &MeditateApplication,
    rebuilder: crate::Rebuilder,
) {
    let rename_btn = gtk::Button::builder()
        .icon_name("document-edit-symbolic")
        .tooltip_text(gettext("Rename"))
        .css_classes(["flat", "circular"])
        .valign(gtk::Align::Center)
        .build();
    let app = app.clone();
    let preset_uuid = preset.uuid.0.clone();
    let preset_name = preset.name.clone();
    rename_btn.connect_clicked(move |btn| {
        present_rename_preset_dialog(
            btn, &app, &preset_uuid, &preset_name,
            rebuilder.clone(),
        );
    });
    row.add_suffix(&rename_btn);
}

fn add_delete_button(
    row: &adw::ActionRow,
    preset: &Preset,
    app: &MeditateApplication,
    rebuilder: crate::Rebuilder,
    toast_overlay: &adw::ToastOverlay,
) {
    let delete_btn = gtk::Button::builder()
        .icon_name("user-trash-symbolic")
        .tooltip_text(gettext("Delete preset"))
        .css_classes(["flat", "circular", "destructive-action"])
        .valign(gtk::Align::Center)
        .build();
    let app = app.clone();
    let preset_full = preset.clone();
    let toast_overlay = toast_overlay.clone();
    delete_btn.connect_clicked(move |btn| {
        present_delete_preset_dialog(
            btn, &app, &preset_full,
            rebuilder.clone(), &toast_overlay,
        );
    });
    row.add_suffix(&delete_btn);
}

fn present_rename_preset_dialog(
    anchor: &gtk::Button,
    app: &MeditateApplication,
    preset_uuid: &str,
    current_name: &str,
    rebuilder: crate::Rebuilder,
) {
    let uuid = preset_uuid.to_string();
    let taken = taken_in(app, move |db, name| meditate_core::db::is_preset_name_taken_from_db(db, name, &uuid));
    let app = app.clone();
    let preset_uuid = preset_uuid.to_string();
    name_dialog(anchor, &gettext("Rename Preset"), current_name, "", &gettext("Rename"), taken, move |name| {
        let renamed = app.with_db(|db| db.update_preset_name(&preset_uuid, name));
        if matches!(renamed, Some(Ok(()))) {
            if let Some(rb) = rebuilder.borrow().as_ref() { rb(); }
        }
        renamed
    });
}

fn present_delete_preset_dialog(
    anchor: &gtk::Button,
    app: &MeditateApplication,
    preset: &Preset,
    rebuilder: crate::Rebuilder,
    toast_overlay: &adw::ToastOverlay,
) {
    let body = gettext("'{name}' will be removed from this device and any synced peers.")
        .replace("{name}", &preset.name);
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
    let preset_full = preset.clone();
    let toast_overlay = toast_overlay.clone();
    dialog.connect_response(None, move |_, id| {
        if id != "delete" { return; }
        let deleted = app.with_db(|db| db.delete_preset(preset_full.uuid.as_str()));
        if !write_ok(&toast_overlay, "preset.delete", deleted, Failed::Delete) {
            return;
        }
        if let Some(rb) = rebuilder.borrow().as_ref() { rb(); }

        // Undo: re-insert with the same uuid + name + mode +
        // is_starred + config_json so the row resurrects identically.
        // The forward delete emitted preset_delete; the undo emits
        // preset_insert with newer ts so peers converge on the
        // restored row.
        let app_undo = app.clone();
        let preset_undo = preset_full.clone();
        let rebuilder_undo = rebuilder.clone();
        let title = gettext("'{name}' deleted").replace("{name}", &preset_full.name);
        let toast_overlay_undo = toast_overlay.clone();
        let toast = crate::db::undo_toast(title, move || {
            let restored = app_undo.with_db(|db| db.restore_preset(&preset_undo));
            write_ok(&toast_overlay_undo, "preset.restore", restored, Failed::Save);
            if let Some(rb) = rebuilder_undo.borrow().as_ref() { rb(); }
        }, || {});
        crate::db::post_toast(&toast_overlay, Some("presets"), toast);
    });

    dialog.present(Some(anchor));
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

    dialog.present(Some(anchor));
}

