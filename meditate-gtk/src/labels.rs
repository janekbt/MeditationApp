//! Label chooser — the NavigationPage pushed when the user taps a
//! label-selection row in the timer setup or on the Done page.
//! Lists every row in the `labels` table with a per-row tap-to-pick
//! body, plus inline rename + delete buttons. A synthetic
//! "Create new label…" entry sits at the top.
//!
//! Modeled after `src/sounds.rs::push_sounds_chooser`. The two are
//! intentionally similar — same row builder shape, same rebuilder
//! pattern, same accent-coloured selection checkmark.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;

use crate::application::{InvalidateScope, MeditateApplication};
use crate::db::{write_ok, Failed, Label};
use crate::i18n::{gettext, ngettext};
use meditate_core::labels::DeleteImpactKey;

/// Selection-mode parameters: the row tapped becomes the active
/// pick, the chooser pops, and `on_selected` fires with the chosen
/// `Label` so the caller can persist whichever fields it cares about
/// (id for session row, uuid for cross-device-stable settings).
pub struct SelectionContext {
    pub current_label_id: Option<i64>,
    pub on_selected: Rc<dyn Fn(Label)>,
    pub nav_view: adw::NavigationView,
}

/// Push the label chooser onto the navigation view. `on_selected`
/// fires when the user taps a row body and receives the chosen
/// `Label`; the page pops automatically right after. Same shape as
/// `push_sounds_chooser`.
pub fn push_labels_chooser(
    nav_view: &adw::NavigationView,
    app: &MeditateApplication,
    current_label_id: Option<i64>,
    on_selected: impl Fn(Label) + 'static,
) {
    let group = adw::PreferencesGroup::new();
    let prefs_page = adw::PreferencesPage::new();
    prefs_page.add(&group);

    let header = adw::HeaderBar::builder().show_back_button(true).build();
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    // Its own overlay: a failed save must show on this page.
    let toast_overlay = adw::ToastOverlay::new();
    toast_overlay.set_child(Some(&prefs_page));
    toolbar.set_content(Some(&toast_overlay));

    let page = adw::NavigationPage::builder()
        .tag("labels-chooser")
        .title(gettext("Choose Label"))
        .child(&toolbar)
        .build();

    let on_selected = Rc::new(on_selected) as Rc<dyn Fn(Label)>;
    let nav_view_clone = nav_view.clone();

    // Track every row we add so a rebuild can drain them by ref —
    // same Adw.PreferencesGroup wrapper-walking caveat as the sound
    // chooser (group.first_child returns the wrapper Box, not the
    // rows, so iterating it spins).
    let rows: Rc<RefCell<Vec<gtk::Widget>>> = Rc::new(RefCell::new(Vec::new()));

    let rebuilder: crate::Rebuilder =
        Rc::new(RefCell::new(None));

    let group_for_rb = group.clone();
    let rows_for_rb = rows.clone();
    let app_for_rb = app.clone();
    let nav_view_for_rb = nav_view_clone.clone();
    let on_selected_for_rb = on_selected.clone();
    let rebuilder_for_self = rebuilder.clone();
    *rebuilder.borrow_mut() = Some(Box::new(move || {
        rebuild_chooser_rows(
            &group_for_rb,
            &rows_for_rb,
            &app_for_rb,
            current_label_id,
            &nav_view_for_rb,
            on_selected_for_rb.clone(),
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
    current_label_id: Option<i64>,
    nav_view: &adw::NavigationView,
    on_selected: Rc<dyn Fn(Label)>,
    rebuilder: crate::Rebuilder,
) {
    for row in rows.borrow_mut().drain(..) {
        group.remove(&row);
    }

    // Synthetic "Create new label…" entry, always at the top.
    let create_row = adw::ActionRow::builder()
        .title(gettext("Create new label…"))
        .activatable(true)
        .build();
    let plus = gtk::Image::from_icon_name("list-add-symbolic");
    plus.add_css_class("dim-label");
    create_row.add_suffix(&plus);
    let app_for_create = app.clone();
    let rebuilder_for_create = rebuilder.clone();
    let on_selected_for_create = on_selected.clone();
    let nav_view_for_create = nav_view.clone();
    create_row.connect_activated(move |row| {
        let on_selected = on_selected_for_create.clone();
        let nav_view = nav_view_for_create.clone();
        let rebuilder = rebuilder_for_create.clone();
        present_create_label_dialog(
            row,
            &app_for_create,
            Box::new(move |label: Label| {
                // Treat creation as selection: commit the new label
                // through on_selected and pop the chooser. This matches
                // "you imported a sound and we picked it for you" UX
                // in the sound chooser.
                on_selected(label);
                nav_view.pop();
                if let Some(rb) = rebuilder.borrow().as_ref() {
                    rb();
                }
            }),
        );
    });
    group.add(&create_row);
    rows.borrow_mut().push(create_row.upcast());

    let labels = app
        .with_db(super::db::Database::list_labels)
        .and_then(std::result::Result::ok)
        .unwrap_or_default();
    let selection = SelectionContext {
        current_label_id,
        on_selected: on_selected.clone(),
        nav_view: nav_view.clone(),
    };
    for label in labels {
        let row = build_label_row(&label, app, rebuilder.clone(), &selection);
        group.add(&row);
        rows.borrow_mut().push(row.upcast());
    }
}

fn build_label_row(
    label: &Label,
    app: &MeditateApplication,
    rebuilder: crate::Rebuilder,
    selection: &SelectionContext,
) -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .title(&label.name)
        .activatable(true)
        .build();

    if selection.current_label_id == Some(label.id) {
        let check = gtk::Image::from_icon_name("object-select-symbolic");
        check.add_css_class("selected-check");
        row.add_suffix(&check);
    }

    add_rename_button(&row, label, app, rebuilder.clone());
    add_delete_button(&row, label, app, rebuilder);

    let label_clone = label.clone();
    let on_selected = selection.on_selected.clone();
    let nav_view = selection.nav_view.clone();
    row.connect_activated(move |_| {
        on_selected(label_clone.clone());
        nav_view.pop();
    });
    row
}

fn add_rename_button(
    row: &adw::ActionRow,
    label: &Label,
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
    let label_id = label.id;
    let row_clone = row.clone();
    rename_btn.connect_clicked(move |btn| {
        present_rename_label_dialog(
            btn, &app, label_id, &row_clone.title(), rebuilder.clone());
    });
    row.add_suffix(&rename_btn);
}

fn add_delete_button(
    row: &adw::ActionRow,
    label: &Label,
    app: &MeditateApplication,
    rebuilder: crate::Rebuilder,
) {
    let delete_btn = gtk::Button::builder()
        .icon_name("user-trash-symbolic")
        .tooltip_text(gettext("Delete label"))
        .css_classes(["flat", "circular", "destructive-action"])
        .valign(gtk::Align::Center)
        .build();
    let app = app.clone();
    let label_id = label.id;
    delete_btn.connect_clicked(move |btn| {
        present_delete_label_dialog(btn, &app, label_id, rebuilder.clone());
    });
    row.add_suffix(&delete_btn);
}

fn present_create_label_dialog(
    anchor: &adw::ActionRow,
    app: &MeditateApplication,
    on_created: Box<dyn Fn(Label)>,
) {
    let entry = gtk::Entry::builder()
        .placeholder_text(gettext("Label name"))
        .activates_default(true)
        .build();

    let dialog = adw::AlertDialog::builder()
        .heading(gettext("Create Label"))
        .extra_child(&entry)
        .close_response("cancel")
        .default_response("create")
        .build();
    dialog.add_response("cancel", &gettext("Cancel"));
    dialog.add_response("create", &gettext("Create"));
    dialog.set_response_appearance("create", adw::ResponseAppearance::Suggested);
    dialog.set_response_enabled("create", false);

    // Live validation — non-empty + no collision with existing names.
    let validate: Rc<dyn Fn()> = {
        let app = app.clone();
        let entry = entry.clone();
        let dialog = dialog.clone();
        Rc::new(move || {
            let text = entry.text();
            let trimmed = text.trim();
            let validity = meditate_core::validate(trimmed, |name| {
                app.with_db(|db| db.is_label_name_taken(name, 0))
                    .and_then(std::result::Result::ok)
                    .unwrap_or(false)
            });
            dialog.set_response_enabled("create", validity.is_savable());
        })
    };
    let validate_for_change = validate.clone();
    entry.connect_changed(move |_| validate_for_change());

    let on_created = Rc::new(on_created);
    let app = app.clone();
    let entry_for_response = entry.clone();
    let anchor_for_response = anchor.clone();
    dialog.connect_response(None, move |_, id| {
        if id != "create" { return; }
        let name = entry_for_response.text().trim().to_string();
        if name.is_empty() { return; }
        let outcome = app.with_db_mut(|db| db.create_label(&name));
        match outcome {
            Some(Ok(label)) => {
                // The Log's label filter lists it.
                app.invalidate(InvalidateScope::LOG);
                on_created(label);
            }
            // A name clash that raced past the live validation, or a
            // failed write: say so, so the action doesn't just vanish.
            failed => {
                write_ok(&anchor_for_response, "label.create", failed, Failed::Save);
            }
        }
    });

    if let Some(root) = anchor.root() {
        if let Ok(window) = root.downcast::<gtk::Window>() {
            dialog.present(Some(&window));
            entry.grab_focus();
        }
    }
}

fn present_rename_label_dialog(
    anchor: &gtk::Button,
    app: &MeditateApplication,
    label_id: i64,
    current_name: &str,
    rebuilder: crate::Rebuilder,
) {
    let entry = gtk::Entry::builder()
        .text(current_name)
        .activates_default(true)
        .build();

    let dialog = adw::AlertDialog::builder()
        .heading(gettext("Rename Label"))
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
        Rc::new(move || {
            let text = entry.text();
            let trimmed = text.trim();
            let validity = meditate_core::validate(trimmed, |name| {
                app.with_db(|db| db.is_label_name_taken(name, label_id))
                    .and_then(std::result::Result::ok)
                    .unwrap_or(false)
            });
            dialog.set_response_enabled("rename", validity.is_savable());
        })
    };
    validate();
    let validate_for_change = validate.clone();
    entry.connect_changed(move |_| validate_for_change());

    let app = app.clone();
    let entry_for_response = entry.clone();
    let anchor_for_response = anchor.clone();
    dialog.connect_response(None, move |_, id| {
        if id != "rename" { return; }
        let new_name = entry_for_response.text().trim().to_string();
        if new_name.is_empty() { return; }
        let renamed = app.with_db_mut(|db| db.update_label(label_id, &new_name));
        if !write_ok(&anchor_for_response, "label.rename", renamed, Failed::Save) {
            return;
        }
        // Log chips, its filter and the Stats rows show the name.
        app.invalidate(InvalidateScope::ALL);
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

fn present_delete_label_dialog(
    anchor: &gtk::Button,
    app: &MeditateApplication,
    label_id: i64,
    rebuilder: crate::Rebuilder,
) {
    // Show how many sessions still point at this label so the user
    // can decide knowingly. delete_label() un-labels each affected
    // session (label_id → NULL); the dialog body captures that. The
    // "is anything tagged?" decision lives in core (the dialog body
    // strings stay shell-side at the i18n boundary).
    let session_count = app
        .with_db(|db| db.label_session_count(label_id))
        .and_then(std::result::Result::ok)
        .unwrap_or(0);
    let body = match meditate_core::labels::delete_impact_key(session_count) {
        DeleteImpactKey::InUse(n) => format!(
            "{} {}.",
            ngettext(
                "Session tagged with this label will be un-labelled:",
                "Sessions tagged with this label will be un-labelled:",
                n as u32,
            ),
            n,
        ),
        DeleteImpactKey::Unused => gettext("This label is not used by any sessions."),
    };

    let dialog = adw::AlertDialog::builder()
        .heading(gettext("Delete Label?"))
        .body(body)
        .close_response("cancel")
        .default_response("cancel")
        .build();
    dialog.add_response("cancel", &gettext("Cancel"));
    dialog.add_response("delete", &gettext("Delete"));
    dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);

    let app = app.clone();
    let anchor_for_failure = anchor.clone();
    dialog.connect_response(None, move |_, id| {
        if id != "delete" { return; }
        let deleted = app.with_db_mut(|db| db.delete_label(label_id));
        if !write_ok(&anchor_for_failure, "label.delete", deleted, Failed::Delete) {
            return;
        }
        app.invalidate(InvalidateScope::ALL);
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

/// After a sync: two devices named a label alike before they synced,
/// and one copy carries the "(conflict-…)" suffix. Ask, like the phone
/// does, whether to merge it into the other (its sessions move over)
/// or keep both. Closing the dialog decides nothing: it asks again
/// after the next sync.
pub(crate) fn ask_about_label_conflict(app: &MeditateApplication, win: &crate::window::MeditateWindow) {
    use glib::subclass::prelude::ObjectSubclassIsExt;
    if win.visible_dialog().is_some() {
        return;
    }
    let Some(Ok(conflicts)) = app.with_db(|db| db.core().list_label_conflicts()) else { return; };
    let Some(conflict) = conflicts.into_iter().next() else { return; };
    let n = u32::try_from(conflict.suffixed_session_count).unwrap_or(u32::MAX);
    let body = format!(
        "{}\n\n{}",
        gettext("'{duplicate}' has the same name as '{label}'. It was created independently on another device.")
            .replace("{duplicate}", &conflict.suffixed_name)
            .replace("{label}", &conflict.base_name),
        ngettext(
            "Merging moves {n} session and deletes the duplicate.",
            "Merging moves {n} sessions and deletes the duplicate.",
            n,
        )
        .replace("{n}", &n.to_string()),
    );
    let dialog = adw::AlertDialog::builder()
        .heading(gettext("Duplicate Label"))
        .body(body)
        .close_response("later")
        .default_response("merge")
        .build();
    dialog.add_response("keep", &gettext("Keep Both"));
    dialog.add_response("merge", &gettext("Merge"));
    dialog.set_response_appearance("merge", adw::ResponseAppearance::Suggested);
    let decided = Rc::new(std::cell::Cell::new(false));
    let app_for_response = app.clone();
    let win_for_response = win.clone();
    let decided_for_response = decided.clone();
    dialog.connect_response(None, move |_, response| {
        let app = &app_for_response;
        match response {
            "merge" => {
                let merged = app.with_db_mut(|db| db.core().merge_labels(conflict.suffixed_id, conflict.base_id));
                if !matches!(merged, Some(Ok(_))) {
                    meditate_core::log("label.merge", &format!("failed: {merged:?}"));
                    win_for_response.add_toast(adw::Toast::new(&gettext("Save failed")));
                    return;
                }
                app.invalidate(InvalidateScope::ALL);
                win_for_response.imp().timer_view.label_merged(conflict.suffixed_id, conflict.base_id);
                win_for_response.imp().stats_view.refresh();
                win_for_response.imp().log_view.refresh();
            }
            "keep" => {
                if let Some(Err(e)) = app.with_db(|db| db.core().dismiss_label_conflict(&conflict.suffixed_uuid)) {
                    meditate_core::log("label.conflict", &format!("dismiss failed: {e:?}"));
                }
            }
            _ => return,
        }
        decided_for_response.set(true);
    });
    // Several can queue up: the next one once this dialog is gone.
    let app = app.clone();
    let win_for_next = win.clone();
    dialog.connect_closed(move |_| {
        if decided.get() {
            ask_about_label_conflict(&app, &win_for_next);
        }
    });
    dialog.present(Some(win));
}

#[cfg(test)]
mod tests {
    fn read(path: &str) -> String {
        std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(path)).unwrap()
    }

    fn body<'a>(src: &'a str, head: &str) -> &'a str {
        let start = src.find(head).unwrap();
        &src[start..start + src[start..].find("\n}\n").unwrap()]
    }

    /// Renaming or deleting a label (or renaming a guided file) left
    /// the old chips, filter entries and Stats rows until a sync.
    #[test]
    fn name_changes_refresh_log_and_stats() {
        let src = read("src/labels.rs");
        assert!(body(&src, "fn present_rename_label_dialog(").contains("app.invalidate(InvalidateScope::ALL)"));
        assert!(body(&src, "fn present_delete_label_dialog(").contains("app.invalidate(InvalidateScope::ALL)"));
        assert!(body(&src, "fn present_create_label_dialog(").contains("app.invalidate(InvalidateScope::LOG)"));
        let guided = read("src/guided.rs");
        let rename = body(&guided, "fn present_rename_dialog(");
        assert_eq!(rename.matches(".invalidate(crate::application::InvalidateScope::ALL)").count(), 2, "the rename and its Undo");
        // A chooser opened from the Log edit dialog: the Log shows again
        // when the dialog closes, with no tab switch to refresh it.
        assert!(read("src/log/imp.rs").contains("dialog.connect_closed("));
    }

    /// A label deleted in the chooser kept its name on the Done page
    /// and in the Log edit; the rows re-read it when they show again.
    #[test]
    fn label_rows_reread_their_label_when_shown_again() {
        let timer = read("src/timer/imp.rs");
        assert!(timer.contains("self.done_label_chooser_row.connect_map("));
        assert!(timer.contains("self.setup_label_chooser_row.connect_map("));
        assert!(read("src/log/imp.rs").contains("label_chooser_row.connect_map("));
    }

    /// Two devices that named a label alike before syncing kept a
    /// "(conflict)" copy the laptop couldn't merge; the phone asks.
    #[test]
    fn a_sync_asks_about_a_duplicate_label_like_the_phone() {
        assert!(read("src/application.rs").contains("crate::labels::ask_about_label_conflict(&app, &win);"));
        let src = read("src/labels.rs");
        let ask = body(&src, "pub(crate) fn ask_about_label_conflict(");
        assert!(ask.contains("visible_dialog().is_some()"));
        assert!(ask.contains("db.core().merge_labels(conflict.suffixed_id, conflict.base_id)"));
        assert!(ask.contains("db.core().dismiss_label_conflict(&conflict.suffixed_uuid)"));
        assert!(ask.contains("timer_view.label_merged(conflict.suffixed_id, conflict.base_id)"));
        assert!(ask.contains(".close_response(\"later\")"));
    }

    /// Undo after a delete put the item at the end of its list.
    #[test]
    fn undo_puts_a_deleted_item_back_in_its_place() {
        assert!(read("src/presets.rs").contains("db.core().restore_preset(&preset_undo)"));
        assert!(read("src/guided.rs").contains("db.core().restore_guided_file(&"));
        assert!(read("src/vibrations.rs").contains("db.core().restore_vibration_pattern(&snapshot)"));
    }
}
