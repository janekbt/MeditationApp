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

use crate::application::MeditateApplication;
use crate::db::Label;
use crate::i18n::gettext;

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
    toolbar.set_content(Some(&prefs_page));

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

    let rebuilder: Rc<RefCell<Option<Box<dyn Fn()>>>> =
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
    rebuilder: Rc<RefCell<Option<Box<dyn Fn()>>>>,
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
        .with_db(|db| db.list_labels())
        .and_then(|r| r.ok())
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
    rebuilder: Rc<RefCell<Option<Box<dyn Fn()>>>>,
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
    rebuilder: Rc<RefCell<Option<Box<dyn Fn()>>>>,
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
    rebuilder: Rc<RefCell<Option<Box<dyn Fn()>>>>,
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
            let lower = trimmed.to_lowercase();
            let collision = app
                .with_db(|db| db.list_labels())
                .and_then(|r| r.ok())
                .unwrap_or_default()
                .into_iter()
                .any(|l| l.name.to_lowercase() == lower);
            dialog.set_response_enabled("create", !trimmed.is_empty() && !collision);
        })
    };
    let validate_for_change = validate.clone();
    entry.connect_changed(move |_| validate_for_change());

    let on_created = Rc::new(on_created);
    let app = app.clone();
    let entry_for_response = entry.clone();
    dialog.connect_response(None, move |_, id| {
        if id != "create" { return; }
        let name = entry_for_response.text().trim().to_string();
        if name.is_empty() { return; }
        let new_label: Option<Label> = app
            .with_db_mut(|db| db.create_label(&name))
            .and_then(|r| r.ok());
        if let Some(label) = new_label {
            on_created(label);
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
    rebuilder: Rc<RefCell<Option<Box<dyn Fn()>>>>,
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
            let collision = app
                .with_db(|db| db.is_label_name_taken(trimmed, label_id))
                .and_then(|r| r.ok())
                .unwrap_or(false);
            dialog.set_response_enabled("rename", !trimmed.is_empty() && !collision);
        })
    };
    validate();
    let validate_for_change = validate.clone();
    entry.connect_changed(move |_| validate_for_change());

    let app = app.clone();
    let entry_for_response = entry.clone();
    dialog.connect_response(None, move |_, id| {
        if id != "rename" { return; }
        let new_name = entry_for_response.text().trim().to_string();
        if new_name.is_empty() { return; }
        app.with_db_mut(|db| { let _ = db.update_label(label_id, &new_name); });
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
    rebuilder: Rc<RefCell<Option<Box<dyn Fn()>>>>,
) {
    // Show how many sessions still point at this label so the user
    // can decide knowingly. delete_label() un-labels each affected
    // session (label_id → NULL); the dialog body captures that.
    let session_count = app
        .with_db(|db| db.label_session_count(label_id))
        .and_then(|r| r.ok())
        .unwrap_or(0);
    let body = if session_count > 0 {
        format!(
            "{} {}.",
            gettext("Sessions tagged with this label will be un-labelled:"),
            session_count,
        )
    } else {
        gettext("This label is not used by any sessions.")
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
    dialog.connect_response(None, move |_, id| {
        if id != "delete" { return; }
        app.with_db_mut(|db| { let _ = db.delete_label(label_id); });
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
