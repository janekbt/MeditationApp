mod imp;
pub mod breathing;

pub use imp::{format_time, TimerMode, TimerState};

use gtk::glib;
use gtk::glib::prelude::*;
use gtk::glib::subclass::prelude::ObjectSubclassIsExt;

glib::wrapper! {
    pub struct TimerView(ObjectSubclass<imp::TimerView>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl TimerView {
    /// Refresh the streak label from the database.
    pub fn refresh_streak(&self) {
        self.imp().refresh_streak();
    }

    /// Rebuild the preset buttons from the database.
    pub fn refresh_presets(&self) {
        self.imp().refresh_presets();
    }

    /// Returns the current display time in seconds.
    pub fn current_display_secs(&self) -> u64 {
        self.imp().current_display_secs()
    }

    /// Store a reference to the running-page time label so tick updates it.
    pub fn set_running_label(&self, label: gtk::Label) {
        self.imp().set_running_label(label);
    }

    /// Called by the window when the running page's Pause button is pressed.
    pub fn pause(&self) {
        self.imp().on_pause();
    }

    /// Called by the window when the running page's Stop button is pressed.
    pub fn stop(&self) {
        self.imp().on_stop();
    }

    /// Toggle playback: Idle→start, Running→pause, Paused→resume, Done→noop.
    pub fn toggle_playback(&self) {
        self.imp().toggle_playback();
    }

    /// Connect to the "timer-started" signal (emitted on Start and Resume).
    pub fn connect_timer_started<F: Fn(&Self) + 'static>(&self, f: F) -> glib::SignalHandlerId {
        self.connect_local("timer-started", false, move |values| {
            let obj = values[0].get::<Self>().unwrap();
            f(&obj);
            None
        })
    }

    /// Connect to the "timer-paused" signal.
    pub fn connect_timer_paused<F: Fn(&Self) + 'static>(&self, f: F) -> glib::SignalHandlerId {
        self.connect_local("timer-paused", false, move |values| {
            let obj = values[0].get::<Self>().unwrap();
            f(&obj);
            None
        })
    }

    /// Connect to the "timer-stopped" signal.
    pub fn connect_timer_stopped<F: Fn(&Self) + 'static>(&self, f: F) -> glib::SignalHandlerId {
        self.connect_local("timer-stopped", false, move |values| {
            let obj = values[0].get::<Self>().unwrap();
            f(&obj);
            None
        })
    }

    // ── Box-Breath integration ────────────────────────────────────────
    // Thin wrappers used by window/imp.rs to build the square-frame
    // running page. All read from imp state; no side effects.

    pub fn is_breathing_mode(&self) -> bool {
        self.imp().current_mode() == TimerMode::Breathing
    }

    pub fn breathing_pattern(&self) -> breathing::Pattern {
        self.imp().breathing_pattern.get()
    }

    pub fn breathing_target_secs(&self) -> u64 {
        self.imp().breathing_target_secs()
    }

    pub fn breathing_timer_state(&self) -> TimerState {
        self.imp().breathing_timer_state()
    }

    /// Shared Rc<Cell<f64>> for the high-resolution elapsed time. The
    /// square-frame running page drives accumulation; TimerView reads it
    /// for the save/pause/stop paths.
    pub fn breathing_elapsed_handle(&self) -> std::rc::Rc<std::cell::Cell<f64>> {
        self.imp().breathing_elapsed_secs.clone()
    }
}
