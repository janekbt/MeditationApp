//! A bell's Volume row (issue #1): every bell has one, under its Bell
//! Sound row. libadwaita has no slider row, so it's a plain
//! `AdwPreferencesRow` with the title on top and the `GtkScale` below
//! at the row's full width — a suffix slider would be a few
//! centimetres on a phone, too short to set precisely (GNOME
//! Settings' volume rows use the same layout).
//!
//! Moving the slider changes a ringing preview at once. Once it rests,
//! the level is saved and the bell plays at it, with a Stop button
//! while it rings — a long bell (a bonshō rings ~30 s) shouldn't have
//! to play out. The level is relative to the system volume
//! (`meditate_core::bell_volume`).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;
use meditate_core::bell_volume::{self, BellVolume};

use crate::application::MeditateApplication;
use crate::i18n::gettext;

/// How long the slider must rest before its level is saved and played.
const SETTLE: std::time::Duration = std::time::Duration::from_millis(400);

/// A Volume row's slider, to set its level.
#[derive(Clone, Debug)]
pub struct VolumeRow {
    scale: gtk::Scale,
    /// Set while the level is set from stored state, so it isn't saved
    /// back or played.
    loading: Rc<Cell<bool>>,
}

impl VolumeRow {
    /// Build the slider into `row`. `save` stores a level once the
    /// slider rests and returns the bell's sound to preview it with.
    pub fn install(
        row: &adw::PreferencesRow,
        save: impl Fn(BellVolume) -> Option<String> + 'static,
    ) -> Self {
        let scale = gtk::Scale::with_range(
            gtk::Orientation::Horizontal,
            bell_volume::MIN.into(),
            bell_volume::MAX.into(),
            bell_volume::STEP.into(),
        );
        scale.set_value(BellVolume::default().percent().into());
        scale.set_draw_value(false);
        scale.set_hexpand(true);
        scale.update_property(&[gtk::accessible::Property::Label(&gettext("Volume"))]);
        let title = gtk::Label::builder()
            .label(gettext("Volume"))
            .xalign(0.0)
            .hexpand(true)
            .build();
        let stop = gtk::Button::builder()
            .icon_name("media-playback-stop-symbolic")
            .tooltip_text(gettext("Stop preview"))
            .css_classes(["flat", "circular"])
            .valign(gtk::Align::Center)
            .visible(false)
            .build();
        stop.connect_clicked(|btn| {
            crate::sound::stop_preview();
            btn.set_visible(false);
        });
        let header = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        header.append(&title);
        header.append(&stop);
        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(6)
            .margin_top(12)
            .margin_bottom(6)
            .margin_start(12)
            .margin_end(12)
            .build();
        content.append(&header);
        content.append(&scale);
        row.set_title(&gettext("Volume"));
        row.set_activatable(false);
        row.set_child(Some(&content));

        let this = Self { scale, loading: Rc::new(Cell::new(false)) };
        let settle: Rc<RefCell<Option<glib::SourceId>>> = Rc::default();
        let save = Rc::new(save);
        let loading = this.loading.clone();
        this.scale.connect_value_changed(glib::clone!(
            #[weak] stop,
            move |scale| {
                let volume = BellVolume::from_percent(scale.value());
                if scale.value() != f64::from(volume.percent()) {
                    scale.set_value(volume.percent().into());
                    return;
                }
                if loading.get() {
                    return;
                }
                crate::sound::set_preview_volume(volume);
                if let Some(pending) = settle.take() {
                    pending.remove();
                }
                let settle_done = settle.clone();
                let save = save.clone();
                let id = glib::timeout_add_local_once(
                    SETTLE,
                    glib::clone!(
                        #[weak] stop,
                        move || {
                            settle_done.replace(None);
                            let Some(sound) = save(volume) else { return };
                            let Some(app) = gtk::gio::Application::default()
                                .and_then(|a| a.downcast::<MeditateApplication>().ok())
                            else {
                                return;
                            };
                            let Some(media) = crate::sound::play_bell_preview(&app, &sound, volume)
                            else {
                                return;
                            };
                            stop.set_visible(true);
                            media.connect_notify_local(Some("playing"), move |m, _| {
                                if !m.is_playing() {
                                    stop.set_visible(false);
                                }
                            });
                        }
                    ),
                );
                settle.replace(Some(id));
            }
        ));
        this
    }

    /// Show a stored level without saving or playing it.
    pub fn set_volume(&self, volume: BellVolume) {
        self.loading.set(true);
        self.scale.set_value(volume.percent().into());
        self.loading.set(false);
    }
}
