//! Vibration-pattern chooser — the NavigationPage pushed when the
//! user taps a vibration-pattern row in a per-bell or per-phase
//! configuration screen, OR when they open "Manage vibration patterns"
//! from Preferences. Lists every row in the `vibration_patterns`
//! library (bundled + custom). Tapping a row body picks that pattern
//! and pops the page; the caller's `on_selected` callback receives
//! the chosen UUID.
//!
//! Mirrors `sounds.rs`'s shape: synthetic "Create custom pattern…"
//! top row that drills into the editor, per-row Edit (a copy for a
//! bundled pattern), per-row Delete (non-bundled only).

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;

use crate::application::MeditateApplication;
use crate::db::{write_ok, Failed, VibrationPattern};
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
    let preview: Rc<RefCell<PreviewState>> = Rc::new(RefCell::new(PreviewState::default()));

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
        current_uuid: current_uuid.map(std::string::ToString::to_string),
        on_selected,
        nav_view: nav_view.clone(),
        play_slot: play_slot.clone(),
        preview: preview.clone(),
        toast_overlay: toast_overlay.clone(),
    };

    let patterns = app
        .with_db(meditate_core::db::list_vibration_patterns_from_db)
        .and_then(std::result::Result::ok)
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
/// timeout fired. The portable state machine (which uuid is
/// active, the auto-revert generation counter) lives in
/// `meditate_core::vibration::PreviewToggle`; this struct adds the
/// shell-only "which gtk::Button is the active Stop icon" handle
/// so we know which row to revert.
#[derive(Default)]
struct PreviewState {
    toggle: meditate_core::vibration::PreviewToggle,
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
            move |_uuid, _changed| {
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

    // Play button comes before edit/delete so it's the
    // leftmost suffix — primary "test this on the motor" affordance.
    add_play_button(
        &row,
        pattern,
        app,
        selection.play_slot.clone(),
        selection.preview.clone(),
    );

    // Edit covers rename + curve + duration + chart kind. A bundled
    // row's Edit opens a copy (the editor decides), as on Android:
    // bundled rows stay permanent, since the seed re-creates them on
    // every open anyway and an accidental tombstone could confuse a
    // peer that hasn't seeded yet.
    add_edit_button(
        &row,
        pattern,
        app,
        &selection.nav_view,
        rebuilder.clone(),
        &selection.toast_overlay,
    );
    if !pattern.is_bundled {
        add_delete_button(&row, pattern, app, rebuilder, &selection.toast_overlay);
    }

    let uuid = pattern.uuid.0.clone();
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
        use meditate_core::vibration::PreviewAction;
        let action = preview_for_click.borrow_mut().toggle.request(pattern.uuid.as_str());
        // The previous Stop-icon button (if any) always reverts
        // first — either we're toggling it off, or switching to a
        // different row.
        if let Some(prev_btn) = preview_for_click.borrow_mut().active_btn.take() {
            prev_btn.set_icon_name("media-playback-start-symbolic");
            prev_btn.set_tooltip_text(Some(&gettext("Preview pattern")));
        }
        match action {
            PreviewAction::StopOnly => {
                // Toggle off — clear the slot, its Drop fires the
                // empty-array cancel at feedbackd.
                *play_slot.borrow_mut() = None;
            }
            PreviewAction::StopAndStart { generation, .. } => {
                // Start new preview. disarm-on-replace hands off
                // cleanly to feedbackd's per-app supersede; no
                // cancel race.
                let new_handle = crate::vibration::PatternPlayback::play(&app, &pattern);
                crate::vibration::replace_playback(&mut play_slot.borrow_mut(), Some(new_handle));
                btn_for_click.set_icon_name("media-playback-stop-symbolic");
                btn_for_click.set_tooltip_text(Some(&gettext("Stop preview")));
                preview_for_click.borrow_mut().active_btn = Some(btn_for_click.clone());

                // Auto-revert on natural completion. Core's
                // `timer_should_revert` no-ops if the user already
                // tapped Stop or switched rows in the meantime.
                let preview_for_timeout = preview_for_click.clone();
                glib::timeout_add_local_once(
                    std::time::Duration::from_millis(u64::from(pattern.duration_ms)),
                    move || {
                        let mut state = preview_for_timeout.borrow_mut();
                        if state.toggle.timer_should_revert(generation) {
                            if let Some(btn) = state.active_btn.take() {
                                btn.set_icon_name("media-playback-start-symbolic");
                                btn.set_tooltip_text(Some(&gettext("Preview pattern")));
                            }
                        }
                    },
                );
            }
            PreviewAction::NoOp => {}
        }
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
            move |_saved_uuid, changed| {
                if let Some(rb) = rebuilder_for_saved.borrow().as_ref() {
                    rb();
                }
                // A bundled pattern's copy is a new pattern: no Undo.
                if changed && !before.is_bundled {
                    show_undo_edit_toast(
                        &toast_for_saved,
                        &app_for_saved,
                        before.clone(),
                        rebuilder_for_saved.clone(),
                    );
                }
            },
        );
    });
    row.add_suffix(&edit_btn);
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
    let uuid = pattern.uuid.0.clone();
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
    // The name it has now: a rename says the new one.
    let name = app
        .with_db(|db| meditate_core::db::find_vibration_pattern_by_uuid_from_db(db, before.uuid.as_str()))
        .and_then(std::result::Result::ok)
        .flatten()
        .map_or_else(|| before.name.clone(), |p| p.name);
    let app = app.clone();
    let overlay_for_failure = overlay.clone();
    let toast = crate::db::undo_toast(gettext("'{name}' updated").replace("{name}", &name), move || {
        let restored = app.with_db(|db| {
            db.update_vibration_pattern(
                before.uuid.as_str(),
                &before.name,
                before.duration_ms,
                &before.intensities,
                before.chart_kind,
            )
        });
        write_ok(&overlay_for_failure, "pattern.edit.undo", restored, Failed::Save);
        if let Some(rb) = rebuilder.borrow().as_ref() {
            rb();
        }
    }, || {});
    // No slot: pattern offers queue, so each stays undoable.
    crate::db::post_toast(overlay, None, toast);
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
    let title = gettext("'{name}' deleted").replace("{name}", &snapshot.name);
    let app = app.clone();
    let overlay_for_failure = overlay.clone();
    let toast = crate::db::undo_toast(title, move || {
        let restored = app.with_db(|db| db.restore_vibration_pattern(&snapshot));
        write_ok(&overlay_for_failure, "pattern.restore", restored, Failed::Save);
        if let Some(rb) = rebuilder.borrow().as_ref() {
            rb();
        }
    }, || {});
    // No slot: pattern offers queue, so each stays undoable.
    crate::db::post_toast(overlay, None, toast);
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
            .with_db(|db| meditate_core::db::find_vibration_pattern_by_uuid_from_db(db, &uuid))
            .and_then(std::result::Result::ok)
            .flatten();
        let deleted = app.with_db(|db| db.delete_vibration_pattern(&uuid));
        if !write_ok(&toast_overlay, "pattern.delete", deleted, Failed::Delete) {
            return;
        }
        if let Some(rb) = rebuilder.borrow().as_ref() {
            rb();
        }
        if let Some(snapshot) = snapshot {
            show_undo_delete_toast(&toast_overlay, &app, snapshot, rebuilder.clone());
        }
    });

    dialog.present(Some(anchor));
}
