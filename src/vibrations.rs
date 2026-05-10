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

use meditate_core::vibration::patterns_equivalent;

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

    let rebuilder: crate::Rebuilder = Rc::new(RefCell::new(None));

    // Shared preview slot: each Play button replaces this, disarming
    // the previous handle so feedbackd's per-app supersede swaps the
    // pattern in-flight without the cancel-races-the-pattern bug.
    // Dropping the chooser drops the slot, which fires the empty-array
    // cancel for any in-flight preview.
    let play_slot: Rc<RefCell<Option<crate::vibration::PatternPlayback>>> =
        Rc::new(RefCell::new(None));
    let preview: Rc<RefCell<PreviewState>> = Rc::new(RefCell::new(PreviewState {
        active_uuid: None,
        active_btn: None,
    }));

    let group_for_rb = group.clone();
    let rows_for_rb = rows.clone();
    let app_for_rb = app.clone();
    let nav_view_for_rb = nav_view_clone.clone();
    let current_uuid_for_rb = current_uuid.clone();
    let on_selected_for_rb = on_selected.clone();
    let toast_overlay_for_rb = toast_overlay.clone();
    let rebuilder_for_self = rebuilder.clone();
    let play_slot_for_rb = play_slot.clone();
    let preview_for_rb = preview.clone();
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
            play_slot_for_rb.clone(),
            preview_for_rb.clone(),
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
    rebuilder: crate::Rebuilder,
    toast_overlay: &adw::ToastOverlay,
    play_slot: Rc<RefCell<Option<crate::vibration::PatternPlayback>>>,
    preview: Rc<RefCell<PreviewState>>,
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
        play_slot: play_slot.clone(),
        preview: preview.clone(),
        toast_overlay: toast_overlay.clone(),
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
/// checkmark. `play_slot` is shared across every row's Play button
/// so a fresh tap supersedes any previous preview cleanly via
/// disarm-on-replace, matching the bell-fire path. `preview` tracks
/// which row is currently showing the Stop icon so a click on a
/// different row (or on the same row again) can revert it.
struct SelectionContext {
    current_uuid: Option<String>,
    on_selected: Rc<dyn Fn(String)>,
    nav_view: adw::NavigationView,
    play_slot: Rc<RefCell<Option<crate::vibration::PatternPlayback>>>,
    preview: Rc<RefCell<PreviewState>>,
    toast_overlay: adw::ToastOverlay,
}

/// Which pattern is currently previewing + the play-button widget
/// showing the Stop icon. Used to flip the icon back to Play when
/// the preview ends — either because the user tapped the Stop
/// button, started a different pattern, or the natural duration
/// timeout fired.
struct PreviewState {
    active_uuid: Option<String>,
    active_btn: Option<gtk::Button>,
}

fn build_create_row(
    app: &MeditateApplication,
    nav_view: &adw::NavigationView,
    rebuilder: crate::Rebuilder,
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
    rebuilder: crate::Rebuilder,
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

    // Play button comes before edit/rename/delete so it's the
    // leftmost suffix — primary "test this on the motor" affordance.
    add_play_button(
        &row,
        pattern,
        app,
        selection.play_slot.clone(),
        selection.preview.clone(),
    );

    if pattern.is_bundled {
        // Bundled rows stay permanent — the seed re-creates them on
        // every open anyway, and an accidental tombstone could
        // confuse a peer that hasn't seeded yet. Rename is the only
        // mutation we let through; the curve, duration, and kind are
        // the seed's identity.
        add_rename_button(&row, pattern, app, rebuilder, &selection.toast_overlay);
    } else {
        // Edit covers rename + curve + duration + chart kind, so we
        // skip the standalone rename button here to avoid two
        // overlapping affordances.
        add_edit_button(
            &row,
            pattern,
            app,
            &selection.nav_view,
            rebuilder.clone(),
            &selection.toast_overlay,
        );
        add_delete_button(&row, pattern, app, rebuilder, &selection.toast_overlay);
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

fn add_play_button(
    row: &adw::ActionRow,
    pattern: &VibrationPattern,
    app: &MeditateApplication,
    play_slot: Rc<RefCell<Option<crate::vibration::PatternPlayback>>>,
    preview: Rc<RefCell<PreviewState>>,
) {
    let play_btn = gtk::Button::builder()
        .icon_name("media-playback-start-symbolic")
        .tooltip_text(gettext("Preview pattern"))
        .css_classes(["flat", "circular"])
        .valign(gtk::Align::Center)
        .build();
    let app = app.clone();
    let pattern = pattern.clone();
    let preview_for_click = preview.clone();
    let btn_for_click = play_btn.clone();
    play_btn.connect_clicked(move |_| {
        let already_playing_this = preview_for_click
            .borrow()
            .active_uuid
            .as_deref()
            == Some(pattern.uuid.as_str());

        // Always revert whichever button currently shows Stop —
        // either we're toggling it off here, or we're switching to
        // a different pattern.
        if let Some(prev_btn) = preview_for_click.borrow_mut().active_btn.take() {
            prev_btn.set_icon_name("media-playback-start-symbolic");
            prev_btn.set_tooltip_text(Some(&gettext("Preview pattern")));
        }
        preview_for_click.borrow_mut().active_uuid = None;

        if already_playing_this {
            // Toggle off — clear the slot, its Drop fires the
            // empty-array cancel at feedbackd.
            *play_slot.borrow_mut() = None;
            return;
        }

        // Start new preview. disarm-on-replace hands off cleanly to
        // feedbackd's per-app supersede; no cancel race.
        let new_handle = crate::vibration::PatternPlayback::play(&app, &pattern);
        {
            let mut slot = play_slot.borrow_mut();
            if let Some(mut old) = slot.take() {
                old.disarm();
            }
            *slot = Some(new_handle);
        }

        btn_for_click.set_icon_name("media-playback-stop-symbolic");
        btn_for_click.set_tooltip_text(Some(&gettext("Stop preview")));
        {
            let mut state = preview_for_click.borrow_mut();
            state.active_uuid = Some(pattern.uuid.clone());
            state.active_btn = Some(btn_for_click.clone());
        }

        // Auto-revert on natural completion. The timeout fires after
        // the pattern's full duration — if the same pattern is still
        // active by then (no other Play tap in between), flip the
        // icon back to Play. If another pattern took over in the
        // meantime, this no-ops because active_uuid won't match.
        let preview_for_timeout = preview_for_click.clone();
        let uuid_for_timeout = pattern.uuid.clone();
        glib::timeout_add_local_once(
            std::time::Duration::from_millis(pattern.duration_ms as u64),
            move || {
                let mut state = preview_for_timeout.borrow_mut();
                if state.active_uuid.as_deref() == Some(uuid_for_timeout.as_str()) {
                    if let Some(btn) = state.active_btn.take() {
                        btn.set_icon_name("media-playback-start-symbolic");
                        btn.set_tooltip_text(Some(&gettext("Preview pattern")));
                    }
                    state.active_uuid = None;
                }
            },
        );
    });
    row.add_suffix(&play_btn);
}

fn add_edit_button(
    row: &adw::ActionRow,
    pattern: &VibrationPattern,
    app: &MeditateApplication,
    nav_view: &adw::NavigationView,
    rebuilder: crate::Rebuilder,
    toast_overlay: &adw::ToastOverlay,
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
    let toast_overlay = toast_overlay.clone();
    edit_btn.connect_clicked(move |_| {
        let rebuilder = rebuilder.clone();
        // Snapshot the pre-edit state — passed into the editor as
        // `initial`, recovered here for the Undo toast.
        let before = pattern.clone();
        let app_for_saved = app.clone();
        let toast_for_saved = toast_overlay.clone();
        let rebuilder_for_saved = rebuilder.clone();
        crate::vibration_editor::push_pattern_editor(
            &nav_view,
            &app,
            Some(before.clone()),
            move |saved_uuid| {
                if let Some(rb) = rebuilder_for_saved.borrow().as_ref() {
                    rb();
                }
                // Skip the Undo toast when nothing actually changed
                // (the editor's Save button stays sensitive even if
                // the user just opened/closed) — comparing the field
                // tuple is enough; we don't have a "dirty" flag.
                let after = app_for_saved
                    .with_db(|db| db.find_vibration_pattern_by_uuid(&saved_uuid))
                    .and_then(|r| r.ok())
                    .flatten();
                if let Some(after) = after {
                    if !patterns_equivalent(&before, &after) {
                        show_undo_edit_toast(
                            &toast_for_saved,
                            &app_for_saved,
                            before.clone(),
                            rebuilder_for_saved.clone(),
                        );
                    }
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
    rebuilder: crate::Rebuilder,
    toast_overlay: &adw::ToastOverlay,
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
    let toast_overlay = toast_overlay.clone();
    rename_btn.connect_clicked(move |btn| {
        present_rename_dialog(
            btn,
            &app,
            &uuid,
            &row_clone.title(),
            rebuilder.clone(),
            &toast_overlay,
        );
    });
    row.add_suffix(&rename_btn);
}

fn add_delete_button(
    row: &adw::ActionRow,
    pattern: &VibrationPattern,
    app: &MeditateApplication,
    rebuilder: crate::Rebuilder,
    toast_overlay: &adw::ToastOverlay,
) {
    let delete_btn = gtk::Button::builder()
        .icon_name("user-trash-symbolic")
        .tooltip_text(gettext("Delete pattern"))
        .css_classes(["flat", "circular", "destructive-action"])
        .valign(gtk::Align::Center)
        .build();
    let app = app.clone();
    let uuid = pattern.uuid.clone();
    let toast_overlay = toast_overlay.clone();
    delete_btn.connect_clicked(move |btn| {
        present_delete_dialog(btn, &app, &uuid, rebuilder.clone(), &toast_overlay);
    });
    row.add_suffix(&delete_btn);
}

/// Show an Undo toast for an edit/rename. Clicking Undo restores
/// the pre-edit name + duration + curve + chart kind by routing
/// through the same update_vibration_pattern call.
fn show_undo_edit_toast(
    overlay: &adw::ToastOverlay,
    app: &MeditateApplication,
    before: VibrationPattern,
    rebuilder: crate::Rebuilder,
) {
    let toast = adw::Toast::builder()
        .title(format!("{} {}", gettext("Updated"), &before.name))
        .button_label(gettext("Undo"))
        .timeout(5)
        .build();
    let app = app.clone();
    toast.connect_button_clicked(move |t| {
        let _ = app.with_db_mut(|db| {
            db.update_vibration_pattern(
                &before.uuid,
                &before.name,
                before.duration_ms,
                &before.intensities,
                before.chart_kind,
            )
        });
        if let Some(rb) = rebuilder.borrow().as_ref() {
            rb();
        }
        t.dismiss();
    });
    overlay.add_toast(toast);
}

/// Show an Undo toast for a delete. Clicking Undo re-inserts the
/// pattern with its original UUID — bells / phases that referenced
/// it (and started rendering as the bundled Pulse fallback the
/// moment we deleted) resolve back to it on the next refresh.
fn show_undo_delete_toast(
    overlay: &adw::ToastOverlay,
    app: &MeditateApplication,
    snapshot: VibrationPattern,
    rebuilder: crate::Rebuilder,
) {
    let toast = adw::Toast::builder()
        .title(format!("{} {}", gettext("Deleted"), &snapshot.name))
        .button_label(gettext("Undo"))
        .timeout(5)
        .build();
    let app = app.clone();
    toast.connect_button_clicked(move |t| {
        let _ = app.with_db_mut(|db| {
            db.insert_vibration_pattern_with_uuid(
                &snapshot.uuid,
                &snapshot.name,
                snapshot.duration_ms,
                &snapshot.intensities,
                snapshot.chart_kind,
                snapshot.is_bundled,
            )
        });
        if let Some(rb) = rebuilder.borrow().as_ref() {
            rb();
        }
        t.dismiss();
    });
    overlay.add_toast(toast);
}

fn present_rename_dialog(
    anchor: &gtk::Button,
    app: &MeditateApplication,
    uuid: &str,
    current_name: &str,
    rebuilder: crate::Rebuilder,
    toast_overlay: &adw::ToastOverlay,
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
    let toast_overlay = toast_overlay.clone();
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
        let before = app
            .with_db(|db| db.find_vibration_pattern_by_uuid(&uuid))
            .and_then(|r| r.ok())
            .flatten();
        if let Some(ref p) = before {
            if p.name == trimmed {
                return; // No-op rename — skip the toast.
            }
            app.with_db_mut(|db| {
                db.update_vibration_pattern(
                    &uuid, trimmed, p.duration_ms, &p.intensities, p.chart_kind,
                )
            });
        }
        if let Some(rb) = rebuilder.borrow().as_ref() {
            rb();
        }
        if let Some(before) = before {
            show_undo_edit_toast(&toast_overlay, &app, before, rebuilder.clone());
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
    rebuilder: crate::Rebuilder,
    toast_overlay: &adw::ToastOverlay,
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
    let toast_overlay = toast_overlay.clone();
    dialog.connect_response(None, move |_, id| {
        if id != "delete" {
            return;
        }
        // Snapshot before deleting so the Undo toast can re-insert
        // with the same UUID.
        let snapshot = app
            .with_db(|db| db.find_vibration_pattern_by_uuid(&uuid))
            .and_then(|r| r.ok())
            .flatten();
        app.with_db_mut(|db| db.delete_vibration_pattern(&uuid));
        if let Some(rb) = rebuilder.borrow().as_ref() {
            rb();
        }
        if let Some(snapshot) = snapshot {
            show_undo_delete_toast(&toast_overlay, &app, snapshot, rebuilder.clone());
        }
    });

    if let Some(root) = anchor.root() {
        if let Ok(window) = root.downcast::<gtk::Window>() {
            dialog.present(Some(&window));
        }
    }
}
