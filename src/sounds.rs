//! Bell-sound chooser — the NavigationPage pushed when the user taps
//! a bell-sound row in the timer setup (Starting Bell sound, per-
//! interval-bell sound, Completion Sound). Lists every row in the
//! `bell_sounds` library (bundled + custom) with a per-row Play
//! button preview. Tapping a row body picks that sound and pops the
//! page; the caller's `on_selected` callback receives the chosen
//! UUID.
//!
//! B.4.5 reuses the same module's row builder for the Preferences
//! tab in management mode (no selection, delete + rename).

use std::rc::Rc;

use adw::prelude::*;

use crate::application::MeditateApplication;
use crate::db::BellSound;
use crate::i18n::gettext;

/// Push the bell-sound chooser onto the navigation view in selection
/// mode. `current_uuid` is the row to mark with a checkmark when
/// the page opens — pass `None` for "nothing selected yet". The
/// `on_selected` callback fires when the user taps a row body and
/// receives the chosen UUID; the page pops automatically right
/// after.
pub fn push_sounds_chooser(
    nav_view: &adw::NavigationView,
    app: &MeditateApplication,
    current_uuid: Option<String>,
    on_selected: impl Fn(String) + 'static,
) {
    let group = adw::PreferencesGroup::new();
    let prefs_page = adw::PreferencesPage::new();
    prefs_page.add(&group);

    let header = adw::HeaderBar::builder().show_back_button(true).build();
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&prefs_page));

    let page = adw::NavigationPage::builder()
        .tag("bell-sounds-chooser")
        .title(gettext("Choose Bell Sound"))
        .child(&toolbar)
        .build();

    let sounds = app
        .with_db(|db| db.list_bell_sounds())
        .and_then(|r| r.ok())
        .unwrap_or_default();

    let on_selected = Rc::new(on_selected);
    let nav_view_clone = nav_view.clone();
    for sound in sounds {
        let row = build_sound_row(
            &sound,
            current_uuid.as_deref(),
            &nav_view_clone,
            on_selected.clone(),
        );
        group.add(&row);
    }

    // Stop any in-flight preview when the user pops the page so a
    // bell doesn't keep ringing through the next setup screen.
    page.connect_hidden(move |_| crate::sound::stop_preview());

    nav_view.push(&page);
}

fn build_sound_row(
    sound: &BellSound,
    current_uuid: Option<&str>,
    nav_view: &adw::NavigationView,
    on_selected: Rc<dyn Fn(String)>,
) -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .title(&sound.name)
        .activatable(true)
        .build();

    // Currently-selected row gets a discreet checkmark on the left
    // (suffix order in adw is left-to-right on the right side, so
    // adding the check first puts it before the play button).
    if current_uuid == Some(&sound.uuid) {
        let check = gtk::Image::from_icon_name("object-select-symbolic");
        check.add_css_class("dim-label");
        row.add_suffix(&check);
    }

    // Per-row preview button. Tapping plays through PREVIEW_MEDIA;
    // tapping a different row stops the previous so previews don't
    // stack while the user is scrubbing the list.
    let play_btn = gtk::Button::builder()
        .icon_name("media-playback-start-symbolic")
        .tooltip_text(gettext("Preview sound"))
        .css_classes(["flat", "circular"])
        .valign(gtk::Align::Center)
        .build();
    let path = sound.file_path.clone();
    let is_bundled = sound.is_bundled;
    play_btn.connect_clicked(move |_| {
        crate::sound::play_preview(&path, is_bundled);
    });
    row.add_suffix(&play_btn);

    // Tap row body → pick this sound and pop. Switch + play button
    // handle their own clicks so they don't trigger row activation.
    let uuid = sound.uuid.clone();
    let nav = nav_view.clone();
    row.connect_activated(move |_| {
        on_selected(uuid.clone());
        nav.pop();
    });
    row
}
