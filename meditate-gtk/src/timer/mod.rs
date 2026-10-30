mod imp;

pub use imp::{RunningControls, TimerMode};
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

    /// A label merge moved `from`'s sessions to `into`.
    pub fn label_merged(&self, from: i64, into: i64) {
        self.imp().label_merged(from, into);
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

    /// The ring and bell counters around the running time, right now.
    pub fn bell_ring(&self) -> meditate_core::session::BellRing {
        self.imp().bell_ring()
    }

    /// Store a reference to the running-page time label so tick updates it.
    pub fn set_running_label(&self, label: gtk::Label) {
        self.imp().set_running_label(label);
    }

    /// Hand over the running page's end controls; TimerView sets
    /// what they show from core's button set.
    pub fn set_running_controls(&self, controls: RunningControls) {
        self.imp().set_running_controls(controls);
    }

    /// One of the two row buttons (0 left, 1 right) was clicked.
    pub fn end_button_clicked(&self, index: usize) {
        self.imp().end_button_clicked(index);
    }

    /// The running page's Discard button.
    pub fn discard(&self) {
        self.imp().on_discard();
    }

    /// Called by the window when the running page's Pause button is pressed.
    pub fn pause(&self) {
        self.imp().on_pause();
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
    /// Only `reset_mode` pops the running page, and Save reaches it
    /// only once the write landed.
    #[test]
    fn only_a_successful_save_leaves_the_running_page() {
        let imp = read("src/timer/imp.rs");
        let code = imp.split("#[cfg(test)]\nmod tests").next().unwrap();
        let body = body_of(code, "    fn on_save(&self, auto: bool) {");
        let resets: Vec<usize> = body.match_indices("reset_mode(mode, false)").map(|(i, _)| i).collect();
        assert_eq!(resets.len(), 1);
        assert!(resets[0] > body.find("None => {").unwrap(), "reset only once the write succeeded");
        assert!(body.contains("if self.saving.get() {"), "a second tap during the write is ignored");
        assert_eq!(code.matches("emit_by_name::<()>(\"timer-stopped\"").count(), 1, "one place pops the page");
        assert!(method(code, "    fn reset_mode(").contains("emit_by_name::<()>(\"timer-stopped\""));
    }

    /// Two screens and two taps ended a session (#6): the running
    /// page now saves and discards itself, with the note, drawn from
    /// core's button set, and the label comes from Setup.
    #[test]
    fn the_running_page_is_the_only_end_screen() {
        let blp = read("data/ui/timer_view.blp");
        for gone in ["name: \"done\"", "done_label_enabled_row", "Gtk.Button save_btn", "stop_from_pause_btn"] {
            assert!(!blp.contains(gone), "{gone}");
        }
        let imp = read("src/timer/imp.rs");
        let code = imp.split("#[cfg(test)]\nmod tests").next().unwrap();
        for gone in ["fn show_done(", "done_selected_label_id", "resolve_persist_action", "Failed::LabelDefault"] {
            assert!(!code.contains(gone), "{gone}");
        }
        assert!(body_of(code, "    fn on_save(&self, auto: bool) {").contains("let label_id = self.setup_selected_label_id();"));
        let refresh = method(code, "    fn refresh_end_buttons(&self) {");
        assert!(refresh.contains("CoreSession::end_buttons"), "core decides the buttons");
        let win = read("src/window/imp.rs");
        assert!(win.contains("gtk::TextView::builder()"), "the note lives on the running page");
        assert_eq!(win.matches("fn end_controls(").count(), 1, "one set of controls for both running pages");
        assert!(!win.contains("timer_view.stop()"));
    }

    /// One Space press in overtime ended the session through Finish;
    /// without a Done page that would save at once.
    #[test]
    fn space_never_ends_a_session() {
        use meditate_core::session::{Session, ToggleAction, UiState};
        assert_eq!(Session::toggle_action(UiState::Overtime), ToggleAction::NoOp);
        assert_eq!(Session::toggle_action(UiState::Done), ToggleAction::NoOp);
        let imp = read("src/timer/imp.rs");
        assert!(!imp.contains("FinishOvertime"));
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

    /// Box Breath ticked only from the drawing area's frame clock, so
    /// a minimised window or a blank screen stopped its cues and its
    /// end. It now ticks on a timer like Android; frames only draw.
    #[test]
    fn box_breath_ticks_on_a_timer_not_on_redraws() {
        let imp = read("src/timer/imp.rs");
        let tick = body_of(&imp, "    fn start_tick(&self) {");
        assert!(tick.contains("Duration::from_millis(200)"), "Box Breath ticks every 200 ms");
        assert!(tick.contains("imp.tick_breathing()"));
        for sig in ["    fn on_start(&self) {", "    fn on_resume(&self) {"] {
            assert!(!body_of(&imp, sig).contains("if mode != TimerMode::Breathing"), "{sig}");
        }
        let window = read("src/window/imp.rs");
        assert!(!window.contains("box_breath_session_tick()"), "the frame callback only draws");
    }

    /// Automatic suspend could hit mid-session on Linux: only the
    /// screen was kept on, and only with "Keep screen awake".
    #[test]
    fn a_running_session_blocks_suspend() {
        let imp = read("src/timer/imp.rs");
        let lock = body_of(&imp, "    pub(crate) fn acquire_screen_awake_lock(");
        assert!(lock.contains("let mut flags = gtk::ApplicationInhibitFlags::SUSPEND;"), "SUSPEND whatever the setting");
        assert!(!lock.contains("if !active { return; }"), "suspend is blocked even without Keep screen awake");
    }

    /// A paused session held the suspend block, so a forgotten pause
    /// kept the computer awake for hours.
    #[test]
    fn a_paused_session_lets_the_computer_suspend() {
        let imp = read("src/timer/imp.rs");
        assert!(body_of(&imp, "    pub fn on_pause(&self) {").contains("self.release_screen_awake_lock(&app)"));
        assert!(body_of(&imp, "    fn on_resume(&self) {").contains("self.acquire_screen_awake_lock(&app)"));
    }

    /// GNOME or the portal can refuse the block (cookie 0); the
    /// session then ran without it and nothing said so.
    #[test]
    fn a_refused_suspend_block_tells_the_user() {
        let imp = read("src/timer/imp.rs");
        let lock = body_of(&imp, "    pub(crate) fn acquire_screen_awake_lock(");
        assert!(lock.contains("if cookie == 0 {"));
        assert!(lock.contains("self.toast(&gettext(\"Couldn't keep the computer awake for the bells\"))"));
    }

    /// One method of an impl block, up to its own closing brace
    /// (`body_of` runs on past `pub fn` boundaries).
    fn method<'a>(source: &'a str, signature: &str) -> &'a str {
        let at = source.find(signature).unwrap_or_else(|| panic!("{signature} not found"));
        &source[at..at + source[at..].find("\n    }\n").unwrap()]
    }

    /// Coming back from Stats in Box Breath, the Duration row showed
    /// the Timer length: several places wrote the shared label.
    #[test]
    fn the_duration_row_always_shows_the_selected_modes_length() {
        let imp = read("src/timer/imp.rs");
        let squeezed: String = imp.split_whitespace().collect();
        let refresh: String = method(&imp, "    fn refresh_duration_value_label(&self) {").split_whitespace().collect();
        assert_eq!(refresh.matches(".duration_value_label").count(), 1);
        assert_eq!(squeezed.matches(".duration_value_label").count(), 1, "only the refresh writes the row");
    }

    /// Esc in overtime paused a session that can't pause; "Pause"
    /// then ended it and dropped the overtime.
    #[test]
    fn escape_pauses_only_where_the_pause_button_would() {
        let imp = read("src/timer/imp.rs");
        let pause = method(&imp, "    pub fn on_pause(&self) {");
        let guard = pause
            .find("if meditate_core::session::Session::toggle_action(self.ui_state()) != meditate_core::session::ToggleAction::Pause {")
            .expect("on_pause asks core whether this state pauses");
        assert!(guard < pause.find("self.cancel_tick()").unwrap(), "before anything is stopped");
    }

    /// A mode switch within 0.4 s of an End Bell drag saved the level
    /// into the new mode: the mode was read when the save ran.
    #[test]
    fn a_volume_change_keeps_the_mode_it_was_made_in() {
        let imp = read("src/timer/imp.rs");
        let rows = method(&imp, "    fn setup_volume_rows(&self) {");
        let target = rows.find("let target =").expect("the bell and mode are taken when the slider moves");
        let save = rows.find("let save = move |").unwrap();
        assert!(target < save);
        assert!(rows[target..save].contains("current_mode()"));
        assert!(!rows[save..].contains("current_mode()"), "the save uses the captured mode");
    }

    /// A Starting Bell drag right before a mode switch: the rows
    /// reloaded the old level before the pending save landed.
    #[test]
    fn a_pending_volume_is_saved_before_the_rows_reload() {
        let imp = read("src/timer/imp.rs");
        let refresh = method(&imp, "    pub(crate) fn refresh_volume_rows(&self) {");
        let flush = refresh.find("self.flush_volume_rows()").expect("pending saves are written first");
        assert!(flush < refresh.find("bell_volume::read").unwrap());
    }

    /// Start within 0.4 s of a Starting Bell drag rang the bell at
    /// the old level: the session reads the stored one.
    #[test]
    fn start_saves_a_pending_volume_first() {
        let imp = read("src/timer/imp.rs");
        let start = method(&imp, "    fn on_start(&self) {");
        let flush = start.find("self.flush_volume_rows()").expect("pending levels are saved at Start");
        assert!(flush < start.find("CoreSession::start").unwrap(), "before the session reads them");
    }
}
