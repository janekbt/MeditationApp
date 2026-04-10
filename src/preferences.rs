use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;

use crate::application::MeditateApplication;
use crate::window::MeditateWindow;

pub fn show_preferences(app: &MeditateApplication) {
    let app = app.clone();

    let dialog = adw::PreferencesDialog::builder()
        .title("Preferences")
        .search_enabled(false)
        .content_height(480)
        .build();

    // ── General page ──────────────────────────────────────────────────────────

    let general_page = adw::PreferencesPage::builder()
        .title("General")
        .icon_name("preferences-system-symbolic")
        .build();

    let stats_group = adw::PreferencesGroup::builder()
        .title("Statistics")
        .build();

    let avg_row = adw::ComboRow::builder()
        .title("Running average period")
        .model(&gtk::StringList::new(&["7 days", "14 days", "30 days"]))
        .build();

    let current_avg = app
        .with_db(|db| db.get_setting("running_avg_days", "7"))
        .and_then(|r| r.ok())
        .unwrap_or_else(|| "7".to_string());
    avg_row.set_selected(match current_avg.as_str() {
        "14" => 1,
        "30" => 2,
        _ => 0,
    });

    avg_row.connect_notify_local(
        Some("selected"),
        glib::clone!(
            #[weak] app,
            move |row, _| {
                let val = match row.selected() {
                    1 => "14",
                    2 => "30",
                    _ => "7",
                };
                app.with_db(|db| db.set_setting("running_avg_days", val));
            }
        ),
    );

    stats_group.add(&avg_row);
    general_page.add(&stats_group);
    dialog.add(&general_page);

    // ── Labels page ───────────────────────────────────────────────────────────

    let labels_page = adw::PreferencesPage::builder()
        .title("Labels")
        .icon_name("tag-symbolic")
        .build();

    let labels_group = adw::PreferencesGroup::builder()
        .title("Labels")
        .description("Organize sessions with custom labels")
        .build();

    let add_btn = gtk::Button::builder()
        .icon_name("list-add-symbolic")
        .tooltip_text("Add label")
        .valign(gtk::Align::Center)
        .css_classes(["flat"])
        .build();
    labels_group.set_header_suffix(Some(&add_btn));

    let labels = app
        .with_db(|db| db.list_labels())
        .and_then(|r| r.ok())
        .unwrap_or_default();

    for label in &labels {
        let row = make_label_row(label.id, &label.name, &labels_group, &app);
        labels_group.add(&row);
    }

    add_btn.connect_clicked(glib::clone!(
        #[weak] app,
        #[weak] labels_group,
        move |_| {
            if let Some(label) = app
                .with_db(|db| db.create_label("New label"))
                .and_then(|r| r.ok())
            {
                let row = make_label_row(label.id, &label.name, &labels_group, &app);
                labels_group.add(&row);
                row.grab_focus();
            }
        }
    ));

    labels_page.add(&labels_group);
    dialog.add(&labels_page);

    let parent = app.active_window();
    dialog.present(parent.as_ref());
}

fn make_label_row(
    id: i64,
    name: &str,
    group: &adw::PreferencesGroup,
    app: &MeditateApplication,
) -> adw::EntryRow {
    // Tracks the last saved name so we know when there are pending changes.
    let committed: Rc<RefCell<String>> = Rc::new(RefCell::new(name.to_string()));

    let row = adw::EntryRow::builder().build();
    row.set_text(name);

    // ── Suffix buttons: [discard] [apply] [delete] ────────────────────────────

    let discard_btn = gtk::Button::builder()
        .icon_name("edit-undo-symbolic")
        .valign(gtk::Align::Center)
        .tooltip_text("Discard changes")
        .css_classes(["flat"])
        .visible(false)
        .build();
    let apply_btn = gtk::Button::builder()
        .icon_name("emblem-ok-symbolic")
        .valign(gtk::Align::Center)
        .tooltip_text("Save")
        .css_classes(["flat"])
        .visible(false)
        .build();
    let delete_btn = gtk::Button::builder()
        .icon_name("user-trash-symbolic")
        .valign(gtk::Align::Center)
        .tooltip_text("Delete label")
        .css_classes(["flat"])
        .build();

    row.add_suffix(&discard_btn);
    row.add_suffix(&apply_btn);
    row.add_suffix(&delete_btn);

    // Show/hide apply+discard whenever the text changes.
    row.connect_changed(glib::clone!(
        #[weak] row,
        #[weak] apply_btn,
        #[weak] discard_btn,
        #[strong] committed,
        move |_| {
            let pending = row.text().as_str() != committed.borrow().as_str();
            apply_btn.set_visible(pending);
            discard_btn.set_visible(pending);
        }
    ));

    // Apply: save new name to DB and clear pending state.
    apply_btn.connect_clicked(glib::clone!(
        #[weak] app,
        #[weak] row,
        #[weak] apply_btn,
        #[weak] discard_btn,
        #[strong] committed,
        move |_| {
            let new_name = row.text().to_string();
            if !new_name.is_empty() {
                app.with_db(|db| db.update_label(id, &new_name));
                *committed.borrow_mut() = new_name;
            }
            apply_btn.set_visible(false);
            discard_btn.set_visible(false);
        }
    ));

    // Discard: restore committed text (triggers connect_changed, which hides the buttons).
    discard_btn.connect_clicked(glib::clone!(
        #[weak] row,
        #[strong] committed,
        move |_| {
            row.set_text(&committed.borrow());
        }
    ));

    // Delete with 5-second undo toast on the main window.
    delete_btn.connect_clicked(glib::clone!(
        #[weak] row,
        #[weak] group,
        #[weak] app,
        move |_| {
            row.set_visible(false);

            let toast = adw::Toast::builder()
                .title("Label deleted")
                .button_label("Undo")
                .timeout(5)
                .build();

            toast.connect_button_clicked(glib::clone!(
                #[weak] row,
                move |_| { row.set_visible(true); }
            ));

            toast.connect_dismissed(glib::clone!(
                #[weak] row,
                #[weak] group,
                #[weak] app,
                move |_| {
                    if !row.is_visible() {
                        app.with_db(|db| db.delete_label(id));
                        group.remove(&row);
                    }
                }
            ));

            if let Some(win) = app
                .active_window()
                .and_then(|w| w.downcast::<MeditateWindow>().ok())
            {
                win.add_toast(toast);
            }
        }
    ));

    row
}
