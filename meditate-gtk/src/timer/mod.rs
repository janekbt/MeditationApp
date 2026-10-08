mod imp;

pub use imp::TimerMode;

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

    pub fn breathing_pattern(&self) -> meditate_core::breath::BreathPattern {
        self.imp().breathing_pattern.get()
    }

    pub fn breathing_target_secs(&self) -> u64 {
        self.imp().breathing_target_secs()
    }

    pub fn breath_elapsed(&self) -> std::time::Duration {
        self.imp().breath_elapsed()
    }

    /// Active mode's stopwatch flag (refreshed on visit, on mode
    /// switch, and on toggle). Used by the running pages to render
    /// the counter as elapsed-only instead of elapsed/target when on.
    pub fn stopwatch_active(&self) -> bool {
        self.imp().stopwatch_toggle_on.get()
    }

    pub fn finish_breath_session(&self) {
        self.imp().finish_breath_session();
    }

}

#[cfg(test)]
mod tests {
    /// Which bells may ring in which mode is decided in
    /// `meditate_core::bells`: every session takes its starting and
    /// interval bells from the core helpers, never from a literal
    /// "none" (that hand-written copy of the rule is how Android came
    /// to ring Timer bells in Guided, issue #3).
    #[test]
    fn every_session_takes_its_bells_from_core() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/timer/imp.rs");
        let source = std::fs::read_to_string(path).unwrap();
        let code = source.split("#[cfg(test)]\nmod tests").next().unwrap();
        for literal in ["bells: Vec::new()", "bells: vec![]", "starting_bell: None"] {
            assert!(!code.contains(literal), "found `{literal}`");
        }
        let sessions = code.matches("CoreSessionSettings {").count();
        assert!(sessions >= 3, "Timer, Box Breath and Guided: {sessions}");
        assert_eq!(code.matches("self.build_session_bells(").count(), sessions);
        assert_eq!(code.matches("self.build_starting_bell_cue(").count(), sessions);
    }

    /// Start dismisses the "'X' applied" toast: its Undo would
    /// re-apply the old settings under the running session.
    #[test]
    fn starting_a_session_dismisses_the_preset_undo() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/timer/imp.rs");
        let source = std::fs::read_to_string(path).unwrap();
        let at = source.find("    fn on_start(&self) {").unwrap();
        let head = &source[at..at + 700];
        assert!(head.contains("if let Some(toast) = self.current_apply_toast.replace(None) {\n            toast.dismiss();"));
    }

    /// Everything `CoreSession::start` returns (the starting bell
    /// without prep) is dispatched at once; there is no second call
    /// to forget.
    #[test]
    fn every_start_dispatches_what_core_returns() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/timer/imp.rs");
        let source = std::fs::read_to_string(path).unwrap();
        let code = source.split("#[cfg(test)]\nmod tests").next().unwrap();
        assert!(!code.contains("start_signals"));
        let starts: Vec<usize> = code.match_indices("CoreSession::start(").map(|(i, _)| i).collect();
        assert_eq!(starts.len(), 4, "Timer with and without prep, Box Breath, Guided");
        for at in starts {
            let rest = &code[at..];
            let dispatch = rest
                .find("self.dispatch_session_effects(&start_effects);")
                .expect("start effects dispatched");
            assert!(!rest[1..dispatch].contains("CoreSession::start("), "dispatched right after its own start");
        }
        assert_eq!(code.matches("let (session, start_effects) = CoreSession::start(").count(), 4);
    }

    /// Every bell with a Bell Sound row in the Setup view has a Volume
    /// row right after it, shown and hidden with it, and the view
    /// installs a slider in each (issue #1).
    #[test]
    fn every_setup_bell_has_a_volume_row() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let blp = std::fs::read_to_string(dir.join("data/ui/timer_view.blp")).unwrap();
        let imp = std::fs::read_to_string(dir.join("src/timer/imp.rs")).unwrap();
        let bells: Vec<&str> = blp
            .match_indices("_sound_revealer {")
            .map(|(at, _)| {
                let head = &blp[..at];
                &head[head.rfind(' ').unwrap() + 1..]
            })
            .collect();
        assert_eq!(bells.len(), 6, "{bells:?}");
        for bell in bells {
            let sound = blp.find(&format!("Gtk.Revealer {bell}_sound_revealer {{")).unwrap();
            let volume = blp
                .find(&format!("Gtk.Revealer {bell}_volume_revealer {{"))
                .unwrap_or_else(|| panic!("{bell}: no volume row"));
            let pattern = blp.find(&format!("Gtk.Revealer {bell}_pattern_revealer {{")).unwrap();
            assert!(sound < volume && volume < pattern, "{bell}: volume row after the sound row");
            assert!(
                blp[volume..].contains(&format!("reveal-child: bind {bell}_sound_revealer.reveal-child;")),
                "{bell}: shown with the sound row",
            );
            assert!(imp.contains(&format!("&*self.{bell}_volume_row)")), "{bell}: slider installed");
        }
    }
}
