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

    /// Refresh the "Manage Bells" subtitle (count of enabled interval
    /// bells). Called by the window after the user pops back from the
    /// bell-library page so the timer page reflects the new state
    /// without rebuilding everything else.
    pub fn refresh_interval_bells_count(&self) {
        self.imp().refresh_interval_bells_count();
    }

    /// Rebuild the visible starred-preset list from the database.
    /// Called by the chooser pages (P.4c onward) after a preset is
    /// created, updated, deleted, or re-starred so the home-view chip
    /// list converges without the user having to leave + return.
    pub fn rebuild_starred_presets_list(&self) {
        self.imp().rebuild_starred_presets_list();
    }

    /// Returns the current display time in seconds.
    pub fn current_display_secs(&self) -> u64 {
        self.imp().current_display_secs()
    }

    /// Store a reference to the running-page time label so tick updates it.
    pub fn set_running_label(&self, label: gtk::Label) {
        self.imp().set_running_label(label);
    }

    /// Stash the running-page Pause button so on_pause / on_resume
    /// can morph its label in place (Pause ↔ Resume) without
    /// popping the running page back to the setup view. Called by
    /// both the timer running page and the breathing running page.
    pub fn set_running_pause_btn(&self, btn: gtk::Button) {
        self.imp().set_running_pause_btn(btn);
    }

    /// Timer-mode-only: stash Stop + Add buttons so the Overtime
    /// transition can hide Stop and reveal the dynamic
    /// "Add MM:SS ?" button.
    pub fn set_running_overtime_widgets(
        &self,
        stop_btn: gtk::Button,
        add_btn: gtk::Button,
    ) {
        self.imp().set_running_overtime_widgets(stop_btn, add_btn);
    }

    /// Called by the running-page Add button — records the planned
    /// duration plus the elapsed overtime as the session length.
    pub fn add_overtime_and_finish(&self) {
        self.imp().add_overtime_and_finish();
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

    pub fn breath_elapsed(&self) -> std::time::Duration {
        self.imp().breath_elapsed()
    }

    pub fn breath_is_finished(&self) -> bool {
        self.imp().breath_is_finished()
    }

    pub fn finish_breath_session(&self) {
        self.imp().finish_breath_session();
    }

}
