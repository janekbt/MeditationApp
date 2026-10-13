//! A bell's Volume row (issue #1): every bell has one, under its Bell
//! Sound row. libadwaita has no slider row, so it's a plain
//! `AdwPreferencesRow` with the title on top and the `GtkScale` below
//! at the row's full width — a suffix slider would be a few
//! centimetres on a phone, too short to set precisely (GNOME
//! Settings' volume rows use the same layout).
//!
//! Moving the slider changes a ringing preview at once. Once it is let
//! go (or rests, from the keyboard), the level is saved and the bell
//! plays at it, with a Stop button
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
/// The same while a pointer holds the slider, which plays on release;
/// this only catches a release a swipe gesture took.
const HELD_FALLBACK: std::time::Duration = std::time::Duration::from_secs(2);

/// A level waiting to be saved: its timer, the bell it is for and
/// the level.
type Pending<T> = Option<(glib::SourceId, T, BellVolume)>;

/// A Volume row's slider, to set its level.
#[derive(Clone)]
pub struct VolumeRow {
    scale: gtk::Scale,
    /// Set while the level is set from stored state, so it isn't saved
    /// back or played.
    loading: Rc<Cell<bool>>,
    /// Saves a level still waiting for the slider to rest, unplayed.
    flush: Rc<dyn Fn()>,
}

impl std::fmt::Debug for VolumeRow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VolumeRow").field("scale", &self.scale).finish_non_exhaustive()
    }
}

impl VolumeRow {
    /// Build the slider into `row`. `target` names the bell (and mode)
    /// when the slider moves; `save` stores the level for it once the
    /// slider rests and returns the bell's sound to preview it with.
    pub fn install<T: 'static>(
        row: &adw::PreferencesRow,
        target: impl Fn() -> Option<T> + 'static,
        save: impl Fn(&T, BellVolume) -> Option<String> + 'static,
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

        let pending: Rc<RefCell<Pending<T>>> = Rc::default();
        let save = Rc::new(save);
        let flush: Rc<dyn Fn()> = Rc::new(glib::clone!(
            #[strong] pending,
            #[strong] save,
            move || {
                if let Some((id, target, volume)) = pending.take() {
                    id.remove();
                    save(&target, volume);
                }
            }
        ));
        // Whether a pointer holds the slider: as on Android, a held
        // slider restarts its bell only once let go.
        let pressed = Rc::new(Cell::new(false));
        // Leaving the screen (page closed, window closed) saves at once.
        scale.connect_unmap(glib::clone!(
            #[strong] pressed,
            #[strong] flush,
            move |_| {
                pressed.set(false);
                flush();
            }
        ));
        let this = Self { scale, loading: Rc::new(Cell::new(false)), flush };
        let loading = this.loading.clone();
        let stop = stop.downgrade();
        // This row's last preview, by bell uuid, to restart while it is
        // still the preview: dropping players in quick succession
        // crashes GStreamer's player thread.
        let last: Rc<RefCell<Option<(String, gtk::MediaFile)>>> = Rc::default();
        // Save a level and play the bell at it.
        let preview: Rc<dyn Fn(T, BellVolume)> = Rc::new(move |target, volume| {
            let Some(sound) = save(&target, volume) else { return };
            // Saved even when the page has closed; only the preview needs it.
            let Some(stop) = stop.upgrade() else { return };
            if last.borrow().as_ref().is_some_and(|(uuid, media)| {
                *uuid == sound && crate::sound::restart_preview(media, volume)
            }) {
                stop.set_visible(true);
                return;
            }
            let Some(app) = gtk::gio::Application::default()
                .and_then(|a| a.downcast::<MeditateApplication>().ok())
            else {
                return;
            };
            let Some(media) = crate::sound::play_bell_preview(&app, &sound, volume) else {
                return;
            };
            stop.set_visible(true);
            media.connect_notify_local(Some("playing"), move |m, _| {
                if !m.is_playing() {
                    stop.set_visible(false);
                }
            });
            last.replace(Some((sound, media)));
        });
        // Letting go plays at once; the keyboard waits for SETTLE.
        // GtkRange claims its pointer gestures, so a legacy controller
        // watches press and release.
        let release = gtk::EventControllerLegacy::new();
        release.set_propagation_phase(gtk::PropagationPhase::Capture);
        release.connect_event(glib::clone!(
            #[strong] pending,
            #[strong] preview,
            #[strong] pressed,
            move |_, event| {
                use gtk::gdk::EventType;
                match event.event_type() {
                    EventType::ButtonPress | EventType::TouchBegin => pressed.set(true),
                    EventType::ButtonRelease | EventType::TouchEnd | EventType::TouchCancel => {
                        pressed.set(false);
                        if let Some((id, target, volume)) = pending.take() {
                            id.remove();
                            preview(target, volume);
                        }
                    }
                    _ => {}
                }
                glib::Propagation::Proceed
            }
        ));
        this.scale.add_controller(release);
        this.scale.connect_value_changed(move |scale| {
            let volume = BellVolume::from_percent(scale.value());
            if scale.value() != f64::from(volume.percent()) {
                scale.set_value(volume.percent().into());
                return;
            }
            if loading.get() {
                return;
            }
            crate::sound::set_preview_volume(volume);
            let Some(target) = target() else { return };
            if let Some((id, ..)) = pending.take() {
                id.remove();
            }
            let delay = if pressed.get() { HELD_FALLBACK } else { SETTLE };
            let settled = pending.clone();
            let preview = preview.clone();
            let id = glib::timeout_add_local_once(delay, move || {
                if let Some((_, target, volume)) = settled.take() {
                    preview(target, volume);
                }
            });
            pending.replace(Some((id, target, volume)));
        });
        this
    }

    /// Save a level whose slider hasn't rested yet, before the row
    /// shows another bell's or mode's level.
    pub fn flush(&self) {
        (self.flush)();
    }

    /// Show a stored level without saving or playing it.
    pub fn set_volume(&self, volume: BellVolume) {
        self.loading.set(true);
        self.scale.set_value(volume.percent().into());
        self.loading.set(false);
    }
}

#[cfg(test)]
mod tests {
    /// Letting go of the slider (mouse or finger) saves and plays at
    /// once, as on Android, instead of after the settle delay; the
    /// keyboard still waits for it. GtkRange claims its own pointer
    /// gestures, so the release is watched by a capture-phase legacy
    /// controller, which no claim can cancel.
    #[test]
    fn letting_go_of_the_slider_plays_at_once() {
        let source = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/volume_row.rs"),
        )
        .unwrap();
        let code = source.split("#[cfg(test)]").next().unwrap();
        let at = code.find("gtk::EventControllerLegacy::new()").expect("the release is watched");
        let release = &code[at..code.find("this.scale.connect_value_changed").unwrap()];
        assert!(release.contains("this.scale.add_controller(release)"), "the controller is attached");
        assert!(release.contains("PropagationPhase::Capture"));
        assert!(release.contains("ButtonRelease") && release.contains("TouchEnd"), "mouse and finger");
        assert!(release.contains("Propagation::Proceed"), "the slider still gets the event");
        let take = release.find("pending.take()").expect("the waiting level is taken");
        let remove = release.find("id.remove()").expect("its timer is cancelled");
        let play = release.find("preview(").expect("and saved and played now");
        assert!(take < remove && remove < play);
        // As on Android, nothing plays while the slider is held, however
        // long it rests mid-drag; a cancelled touch still saves.
        assert!(release.contains("ButtonPress") && release.contains("TouchBegin"), "the hold is tracked");
        assert!(release.contains("TouchCancel"), "a cancelled touch is let go too");
        let arm = |kinds: &str| {
            let at = release.find(kinds).unwrap_or_else(|| panic!("no arm for {kinds}"));
            &release[at..at + release[at..].find("=>").unwrap() + 60]
        };
        let press = arm("EventType::ButtonPress");
        assert!(press.contains("TouchBegin") && press.contains("pressed.set(true)"));
        let let_go = arm("EventType::ButtonRelease");
        assert!(let_go.contains("TouchEnd") && let_go.contains("TouchCancel") && let_go.contains("pressed.set(false)"));
        let unmap = &code[code.find(".connect_unmap(").unwrap()..];
        assert!(unmap[..unmap.find("));").unwrap()].contains("pressed.set(false)"), "leaving the screen lets go");
        // A release eaten above the scale (a swipe gesture on the phone)
        // leaves it held; the long timer still saves and plays it.
        let changed = &code[code.find("this.scale.connect_value_changed").unwrap()..];
        let delay = changed.find("if pressed.get() { HELD_FALLBACK } else { SETTLE }").expect("a held slider waits longer");
        assert!(delay < changed.find("timeout_add_local_once(delay").unwrap());
        // Each step cancels the last timer, or a drag would play 2 s after its first step.
        let take = changed.find("pending.take()").unwrap();
        assert!(take < changed.find("id.remove()").unwrap() && changed.find("id.remove()").unwrap() < delay);
    }

    /// The settle timer skipped its whole body, the save included,
    /// once the bell page was gone (#53). GTK needs a display, so this
    /// reads the source.
    #[test]
    fn a_volume_is_saved_even_when_the_page_has_closed() {
        let source = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/volume_row.rs"),
        )
        .unwrap();
        let at = source.find("let preview: Rc<dyn Fn").unwrap();
        let settled = &source[at..source.find("let release = gtk::EventControllerLegacy").unwrap()];
        assert!(!settled.contains("#[weak] stop"), "a closed page doesn't skip the save");
        let save = settled.find("save(").expect("the settled level is saved");
        let preview = settled.find("stop.upgrade()").expect("the preview needs the page");
        assert!(save < preview, "save first, then the preview");
        // The Stop button is hidden until a preview plays, so asking
        // whether it was on screen silenced every preview. Off screen
        // needs no check: the unmap below cancels the timer.
        assert!(!settled.contains("is_mapped"), "no on-screen check before the preview");
        // A save landing after the page closed would write the page's
        // whole stale bell over a change made in the list meanwhile, and
        // quitting within 0.4 s lost it: saved, unplayed, once the row leaves the screen.
        let unmap = &source[source.find(".connect_unmap(").expect("saved as the row leaves the screen")..];
        assert!(unmap[..unmap.find("));").unwrap()].contains("flush();"));
        // Clicking levels quickly dropped one player per click, and
        // GStreamer's player thread crashed the app on such a drop: the
        // row restarts its own bell, and only a new player gets a handler.
        let restart = settled.find("restart_preview(").expect("the ringing bell restarts");
        let fresh = settled.find("play_bell_preview(").unwrap();
        assert!(restart < fresh, "restart is tried before a new player is built");
        assert!(settled.find("connect_notify_local(").unwrap() > fresh, "one handler per player");
        assert!(settled.contains("last.replace(Some((sound, media)))"), "a new player is kept to restart");
        assert!(settled[..restart].contains("*uuid == sound"), "another bell sound gets its own player");
        let restarted = &settled[restart..fresh];
        assert!(restarted.find("stop.set_visible(true)").unwrap() < restarted.find("return").unwrap(), "a restarted bell shows Stop");
        // A target-less step must not cancel the level still waiting.
        assert!(source.find("target() else").unwrap() < source.find("if let Some((id, ..)) = pending.take()").unwrap());
    }
}
