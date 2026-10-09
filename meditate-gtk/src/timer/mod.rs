mod imp;

pub use imp::TimerMode;
pub(crate) use imp::show_no_vibration_note;

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
    pub fn save_snapshot_now(&self) {
        self.imp().save_snapshot_now();
    }

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
        // One build for every mode, in core (R3).
        assert_eq!(code.matches("CoreSessionSettings::from_db(").count(), 1);
    }

    /// Start and a mode switch close every toast: an Undo left up
    /// would change the running session, or put one mode's settings
    /// into another.
    #[test]
    fn start_and_a_mode_switch_close_every_toast() {
        let imp = read("src/timer/imp.rs");
        for sig in ["    fn on_start(&self) {", "    fn on_mode_switched(&self) {"] {
            assert!(body_of(&imp, sig).contains("window.dismiss_toasts();"), "{sig}");
        }
        assert!(read("src/window/mod.rs").contains("self.imp().toast_overlay.dismiss_all();"));
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
        assert_eq!(starts.len(), 1, "one start for every mode, prep or not");
        for at in starts {
            let rest = &code[at..];
            let dispatch = rest
                .find("self.dispatch_session_effects(&start_effects);")
                .expect("start effects dispatched");
            assert!(!rest[1..dispatch].contains("CoreSession::start("), "dispatched right after its own start");
        }
        assert_eq!(code.matches("let (session, start_effects) = CoreSession::start(").count(), 1);
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

    fn read(rel: &str) -> String {
        std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(rel)).unwrap()
    }

    /// A top-level function, up to its closing brace.
    fn top_level_fn<'a>(source: &'a str, signature: &str) -> &'a str {
        let at = source.find(signature).unwrap_or_else(|| panic!("{signature} not found"));
        &source[at..at + source[at..].find("\n}\n").unwrap()]
    }

    /// Loading the Cues toggle wrote "sound" on every launch of a
    /// device without vibration, and the phone took that over.
    #[test]
    fn the_cues_toggle_writes_nothing_while_it_loads() {
        let imp = read("src/timer/imp.rs");
        assert!(body_of(&imp, "    fn setup_cues_signal_mode_toggle(&self) {").contains("bells_loading.get()"));
    }

    /// Without vibration every choice stays selectable, so a mistaken
    /// tap on Sound can be taken back, and the row says what still
    /// plays. The session plays the choice minus the vibration.
    #[test]
    fn every_signal_choice_stays_selectable_without_vibration() {
        for file in ["src/timer/imp.rs", "src/bells.rs"] {
            let source = read(file);
            let code = source.split("#[cfg(test)]\nmod tests").next().unwrap();
            assert!(!code.contains("set_enabled(false)"), "{file}");
            assert!(!code.contains("clamp_signal_mode_for_haptic"), "{file}");
        }
        let imp = read("src/timer/imp.rs");
        for sig in [
            "pub(crate) fn build_signal_mode_toggle_widget(",
            "pub(crate) fn apply_signal_mode_state(",
            "pub(crate) fn build_phase_signal_mode_toggle_widget(",
            "pub(crate) fn apply_phase_signal_mode_state(",
            "pub(crate) fn build_per_mode_signal_toggle_widget(",
        ] {
            assert!(top_level_fn(&imp, sig).contains("show_no_vibration_note("), "{sig}");
        }
        assert!(body_of(&imp, "    pub(crate) fn refresh_cues_signal_mode_state(").contains("show_no_vibration_note("));
        assert_eq!(read("src/bells.rs").matches("show_no_vibration_note(").count(), 2, "when built and on change");
        // The note's width squeezed the choices until they read "S…".
        assert!(top_level_fn(&imp, "pub(crate) fn show_no_vibration_note(").contains("toggle_group.set_size_request("));
    }

    fn body_of<'a>(source: &'a str, signature: &str) -> &'a str {
        let at = source.find(signature).unwrap_or_else(|| panic!("{signature} not found"));
        let end = source[at + 1..].find("\n    fn ").map_or(source.len(), |e| at + 1 + e);
        &source[at..end]
    }

    /// Going back to Setup before the write succeeded let the next
    /// Start overwrite the only copy of a session whose save failed.
    #[test]
    fn only_a_successful_save_leaves_done() {
        let imp = read("src/timer/imp.rs");
        let body = body_of(&imp, "    fn on_save(&self) {");
        let resets: Vec<usize> = body.match_indices("reset_mode(mode, false)").map(|(i, _)| i).collect();
        assert_eq!(resets.len(), 1);
        assert!(resets[0] > body.find("None => {").unwrap(), "reset only once the write succeeded");
        assert!(body.contains("if self.saving.get() {"), "a second tap during the write is ignored");
    }

    /// The snapshot emits no event, so writing it must not start a
    /// network sync (once a minute during every session).
    #[test]
    fn snapshot_writes_do_not_start_a_sync() {
        let imp = read("src/timer/imp.rs");
        for sig in ["    fn write_in_progress_snapshot(", "    fn clear_in_progress_snapshot("] {
            let body = body_of(&imp, sig);
            assert!(body.contains("app.with_db(") && !body.contains("with_db_mut"), "{sig}");
        }
    }

    /// Alt+Left, the mouse Back button or a swipe returned to Setup,
    /// where Start replaced the running session.
    #[test]
    fn the_running_page_cannot_be_popped_by_the_user() {
        let win = read("src/window/imp.rs");
        let page = &win[win.find(".tag(\"running\")").unwrap()..];
        assert!(page[..page.find(".build()").unwrap()].contains(".can_pop(false)"));
    }

    /// An app-wide Space accelerator runs before the focused widget,
    /// so typed spaces never reached text fields.
    #[test]
    fn space_toggles_the_timer_only_outside_text_fields_and_dialogs() {
        let app = read("src/application.rs");
        assert!(!app.contains("timer-toggle"));
        let win = read("src/window/imp.rs");
        for needle in ["PropagationPhase::Capture", "is::<gtk::Text>()", "is::<gtk::TextView>()", "visible_dialog()"] {
            assert!(win.contains(needle), "{needle}");
        }
    }

    /// Closing the window dropped Log deletes still waiting for Undo
    /// and up to 59 s of a running session; Ctrl+Q skipped close.
    #[test]
    fn closing_keeps_pending_deletes_and_the_session_time() {
        let win = read("src/window/imp.rs");
        let close = body_of(&win, "    fn close_request(&self)");
        assert!(close.contains("commit_all_pending()"));
        assert!(close.contains("save_snapshot_now()"));
        let app = read("src/application.rs");
        let quit = &app[app.find("SimpleAction::new(\"quit\"").unwrap()..];
        assert!(!quit[..quit.find("add_action").unwrap()].contains("app.quit()"), "Ctrl+Q goes through close");
    }
}
