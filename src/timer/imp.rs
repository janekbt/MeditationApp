use std::cell::{Cell, RefCell};
use std::rc::Rc;
use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::{glib, CompositeTemplate};
use glib::subclass::Signal;
use std::sync::OnceLock;

use crate::db::{Label, SessionData, SessionMode};
use super::breathing::Pattern as BreathPattern;

use std::time::Instant;
use meditate_core::timer::{
    Countdown as CoreCountdown, CountdownTimer as CoreCountdownTimer,
    Stopwatch as CoreStopwatch,
};

// ── Per-mode independent state ────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TimerState {
    #[default]
    Idle,
    Running,
    Paused,
    Done,
}

/// Which of the three modes is currently selected. Encapsulates the
/// countdown_btn/stopwatch_btn/breathing_btn radio group in a single
/// readable value so callers don't sprinkle `is_active()` checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TimerMode {
    #[default]
    Countdown,
    Stopwatch,
    Breathing,
}

/// All state that belongs to one timer mode (countdown or stopwatch).
#[derive(Debug, Clone, Default)]
pub(super) struct ModeState {
    timer_state: TimerState,
    /// Seconds remaining (countdown) or elapsed (stopwatch).
    display_secs: u64,
    /// Original target in seconds — countdown only.
    target_secs: u64,
    /// Unix timestamp when this mode's current session started.
    session_start_time: i64,
}

// ── GObject impl ──────────────────────────────────────────────────────────────

#[derive(Debug, Default, CompositeTemplate)]
#[template(resource = "/io/github/janekbt/Meditate/ui/timer_view.ui")]
pub struct TimerView {
    // Template children
    #[template_child] pub view_stack:            TemplateChild<gtk::Stack>,
    #[template_child] pub streak_label:          TemplateChild<gtk::Label>,
    #[template_child] pub countdown_btn:         TemplateChild<gtk::ToggleButton>,
    #[template_child] pub stopwatch_btn:         TemplateChild<gtk::ToggleButton>,
    #[template_child] pub breathing_btn:         TemplateChild<gtk::ToggleButton>,
    #[template_child] pub big_time_label:         TemplateChild<gtk::Label>,
    #[template_child] pub countdown_inputs:       TemplateChild<gtk::Box>,
    #[template_child] pub presets_box:           TemplateChild<gtk::FlowBox>,
    #[template_child] pub boxbreath_inputs:       TemplateChild<gtk::Box>,
    #[template_child] pub breathing_presets_box:  TemplateChild<gtk::FlowBox>,
    #[template_child] pub phase_tiles_grid:       TemplateChild<gtk::Grid>,
    #[template_child] pub breathing_duration_row: TemplateChild<adw::SpinRow>,
    #[template_child] pub start_btn:             TemplateChild<gtk::Button>,
    #[template_child] pub resume_btn:            TemplateChild<gtk::Button>,
    #[template_child] pub stop_from_pause_btn:   TemplateChild<gtk::Button>,
    #[template_child] pub session_group:          TemplateChild<adw::PreferencesGroup>,
    #[template_child] pub setup_label_row:        TemplateChild<adw::ComboRow>,
    #[template_child] pub setup_sound_row:        TemplateChild<adw::ComboRow>,
    #[template_child] pub time_unit_label:        TemplateChild<gtk::Label>,
    #[template_child] pub done_duration_label:   TemplateChild<gtk::Label>,
    #[template_child] pub note_view:             TemplateChild<gtk::TextView>,
    #[template_child] pub note_caption:          TemplateChild<gtk::Label>,
    #[template_child] pub label_row:             TemplateChild<adw::ComboRow>,
    #[template_child] pub discard_btn:           TemplateChild<gtk::Button>,
    #[template_child] pub save_btn:              TemplateChild<gtk::Button>,

    // ── Per-mode state (fully independent) ───────────────────────────
    countdown_mode: RefCell<ModeState>,
    stopwatch_mode: RefCell<ModeState>,
    pub(super) breathing_mode: RefCell<ModeState>,

    /// Which mode the active tick belongs to. Only meaningful while
    /// tick_source is Some.
    tick_mode: Cell<TimerMode>,
    /// Legacy binary flag kept for a few call sites that still consume it;
    /// mirror of `tick_mode == Stopwatch`. Prefer `tick_mode` in new code.
    tick_is_stopwatch: Cell<bool>,

    /// Active glib timeout handle (at most one mode runs at a time).
    tick_source: RefCell<Option<glib::SourceId>>,
    /// Weak ref to the running-page time label for live updates.
    running_label: RefCell<Option<gtk::Label>>,
    /// Labels fetched from DB for the setup-page combo.
    setup_db_labels: RefCell<Vec<Label>>,
    /// True while refresh_setup_labels is rebuilding the setup combo model.
    setup_populating: Cell<bool>,
    /// Labels fetched from DB when entering Done state.
    db_labels: RefCell<Vec<Label>>,
    /// True while show_done/repopulate_label_combo is rebuilding the model,
    /// to suppress the notify::selected handler from opening the new-label dialog.
    populating_labels: Cell<bool>,
    /// True while refresh_streak is populating the setup sound combo.
    sound_populating: Cell<bool>,
    /// Currently-selected countdown duration in seconds, set by preset
    /// chips or the "Custom" dialog. Default 10 min; used as the target
    /// when the user taps Start.
    countdown_target_secs: Cell<u64>,
    /// Preset pills currently attached to presets_box, paired with their
    /// duration in minutes. Used to toggle the `.preset-chip-active` CSS
    /// class on the button whose minutes match countdown_target_secs.
    preset_buttons: RefCell<Vec<(gtk::Button, u32)>>,
    /// The trailing "Custom" pill — gets `.preset-chip-active` when the
    /// current countdown_target_secs doesn't match any preset.
    custom_preset_btn: RefCell<Option<gtk::Button>>,

    // ── Breathing (Box Breath) state ─────────────────────────────────
    /// Four phase durations. Defaults 4/4/4/4 (classic box breathing).
    pub(super) breathing_pattern: Cell<BreathPattern>,
    /// Total session length in minutes, drives the hero label and the
    /// cycle-aligned stop condition.
    breathing_session_mins: Cell<u32>,
    /// Which preset chip is currently highlighted ("4-4-4-4", "4-7-8-0",
    /// "5-5-5-5", or "custom"). Persisted so the chip state survives
    /// app restarts.
    breathing_preset_name: RefCell<String>,
    /// Preset pills currently attached to `breathing_presets_box`, paired
    /// with their pattern. Used to toggle `.preset-chip-active`.
    breathing_preset_buttons: RefCell<Vec<(gtk::Button, BreathPattern, String)>>,
    /// Per-phase stepper buttons + value labels, indexed 0..=3 (In, HoldIn,
    /// Out, HoldOut). Kept so `refresh_phase_tiles` can update the displayed
    /// values without rebuilding the DOM.
    phase_value_labels: RefCell<[Option<gtk::Label>; 4]>,
    /// Suppress persistence side-effects while `load_breathing_settings`
    /// is setting initial values from the DB.
    breathing_populating: Cell<bool>,
    /// High-resolution elapsed time (seconds) for the running Box-Breath
    /// session. Driven by the DrawingArea's tick callback in the running
    /// page so the dot animates smoothly; the window reads this to update
    /// phase label / countdown / top counter. Resets to 0 on each start.
    /// Wrapped in Rc so the window can share the same cell with its draw
    /// func + tick callback — otherwise we'd need a weak TimerView ref in
    /// every closure.
    pub(super) breathing_elapsed_secs: Rc<Cell<f64>>,

    /// Source of truth for countdown / stopwatch timing (graduation step 2/3).
    /// `start_instant` anchors monotonic time at on_start; the `*_core` fields
    /// are queried each tick and updated on pause/resume. Legacy `display_secs`
    /// is kept in sync as a derived shadow until callers are migrated.
    countdown_core: RefCell<Option<CoreCountdown>>,
    stopwatch_core: RefCell<Option<CoreStopwatch>>,
    /// Box-breath uses a Stopwatch + a separate target duration; the
    /// per-frame tick reads elapsed via wall-clock and checks done.
    breath_stopwatch: RefCell<Option<CoreStopwatch>>,
    breath_target: Cell<std::time::Duration>,
    start_instant: Cell<Option<Instant>>,
}

#[glib::object_subclass]
impl ObjectSubclass for TimerView {
    const NAME: &'static str = "TimerView";
    type Type = super::TimerView;
    type ParentType = gtk::Widget;

    fn class_init(klass: &mut Self::Class) {
        klass.bind_template();
        klass.set_layout_manager_type::<gtk::BinLayout>();
    }

    fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
        obj.init_template();
    }
}

impl ObjectImpl for TimerView {
    fn signals() -> &'static [Signal] {
        static SIGNALS: OnceLock<Vec<Signal>> = OnceLock::new();
        SIGNALS.get_or_init(|| {
            vec![
                Signal::builder("timer-started").build(),
                Signal::builder("timer-paused").build(),
                Signal::builder("timer-stopped").build(),
            ]
        })
    }

    fn constructed(&self) {
        self.parent_constructed();
        // Default countdown target: 10 min — matches the hero label that's
        // set to "00:10" in the blueprint.
        self.countdown_target_secs.set(10 * 60);
        // Default breathing pattern (classic box: 4-4-4-4, 5 min session).
        // `load_breathing_settings` overrides from the DB in a moment.
        self.breathing_pattern.set(BreathPattern {
            in_secs: 4, hold_in: 4, out_secs: 4, hold_out: 4,
        });
        self.breathing_session_mins.set(5);
        *self.breathing_preset_name.borrow_mut() = "4-4-4-4".to_string();
        self.setup_buttons();
        self.build_breathing_setup();

        // Tell screen readers that the free-text editor is labelled by
        // its caption, matching the Log add/edit dialog.
        self.note_view.update_relation(&[gtk::accessible::Relation::LabelledBy(
            &[self.note_caption.upcast_ref::<gtk::Accessible>()],
        )]);
    }

    fn dispose(&self) {
        self.cancel_tick();
        if let Some(w) = self.obj().first_child() { w.unparent() }
    }
}

impl WidgetImpl for TimerView {}

// ── Button wiring ─────────────────────────────────────────────────────────────

impl TimerView {
    fn setup_buttons(&self) {
        let obj = self.obj();

        // Mode toggle — all three radios share a group, so exactly one
        // emits `toggled` with `is_active() == true` on every switch.
        // Route both into one handler.
        let mode_toggled = glib::clone!(
            #[weak(rename_to = this)] obj,
            move |btn: &gtk::ToggleButton| {
                if btn.is_active() {
                    this.imp().on_mode_switched();
                }
            }
        );
        self.countdown_btn.connect_toggled(mode_toggled.clone());
        self.stopwatch_btn.connect_toggled(mode_toggled.clone());
        self.breathing_btn.connect_toggled(mode_toggled);

        self.start_btn.connect_clicked(glib::clone!(
            #[weak(rename_to = this)] obj,
            move |_| this.imp().on_start()
        ));
        self.resume_btn.connect_clicked(glib::clone!(
            #[weak(rename_to = this)] obj,
            move |_| this.imp().on_resume()
        ));
        self.stop_from_pause_btn.connect_clicked(glib::clone!(
            #[weak(rename_to = this)] obj,
            move |_| this.imp().on_stop()
        ));
        self.save_btn.connect_clicked(glib::clone!(
            #[weak(rename_to = this)] obj,
            move |_| this.imp().on_save()
        ));
        self.discard_btn.connect_clicked(glib::clone!(
            #[weak(rename_to = this)] obj,
            move |_| this.imp().on_discard()
        ));

        // "＋ New label" is index 0; show creation dialog when selected.
        self.label_row.connect_notify_local(
            Some("selected"),
            glib::clone!(
                #[weak(rename_to = this)] obj,
                move |_, _| {
                    let imp = this.imp();
                    if imp.populating_labels.get() { return; }
                    if imp.label_row.selected() == 0 {
                        imp.show_new_label_dialog();
                    }
                }
            ),
        );

        // Completion Sound row on the setup page — mirrors the Preferences sound setting.
        self.setup_sound_row.connect_notify_local(
            Some("selected"),
            glib::clone!(
                #[weak(rename_to = this)] obj,
                move |row, _| {
                    let imp = this.imp();
                    if imp.sound_populating.get() { return; }
                    let key = match row.selected() {
                        1 => "bowl",
                        2 => "bell",
                        3 => "gong",
                        4 => "custom",
                        _ => "none",
                    };
                    if let Some(app) = imp.get_app() {
                        app.with_db(|db| db.set_setting("end_sound", key));
                        crate::sound::preload_end_sound(&app);
                    }
                }
            ),
        );

        // Same for the pre-start label selector.
        self.setup_label_row.connect_notify_local(
            Some("selected"),
            glib::clone!(
                #[weak(rename_to = this)] obj,
                move |_, _| {
                    let imp = this.imp();
                    if imp.setup_populating.get() { return; }
                    let idx = imp.setup_label_row.selected();
                    if idx == 0 {
                        imp.show_new_label_dialog_for_setup();
                        return;
                    }
                    // Persist the user's choice as the preferred label for
                    // the currently active mode so the next visit to that
                    // mode re-applies it. idx == 1 is "None"; 2+ are labels.
                    let name = if idx == 1 {
                        None
                    } else {
                        imp.setup_db_labels.borrow()
                            .get(idx as usize - 2)
                            .map(|l| l.name.clone())
                    };
                    imp.persist_label_for_mode(imp.current_mode(), name);
                }
            ),
        );
    }
}

// ── Mode switching ────────────────────────────────────────────────────────────

impl TimerView {
    pub(super) fn breathing_target_secs(&self) -> u64 {
        self.breathing_mode.borrow().target_secs
    }

    pub(super) fn breathing_timer_state(&self) -> TimerState {
        self.breathing_mode.borrow().timer_state
    }

    /// Which mode the radio group currently reflects. Exactly one of the
    /// three buttons is active at any time (they share a group), so the
    /// priority order is: breathing → stopwatch → countdown (default).
    pub(crate) fn current_mode(&self) -> TimerMode {
        if self.breathing_btn.is_active() {
            TimerMode::Breathing
        } else if self.stopwatch_btn.is_active() {
            TimerMode::Stopwatch
        } else {
            TimerMode::Countdown
        }
    }

    /// Called when any of the three mode toggles gains active state.
    fn on_mode_switched(&self) {
        let mode = self.current_mode();

        // Input panels: only the active mode's inputs are visible.
        self.countdown_inputs.set_visible(mode == TimerMode::Countdown);
        self.boxbreath_inputs.set_visible(mode == TimerMode::Breathing);

        // Each mode keeps its own last-used label. On switch, pull the
        // stored preference (or fall back to the mode-specific default —
        // "Box-breathing" for Breathing's first visit, "None" for the
        // other two) and apply it to the setup combo.
        self.apply_preferred_label_for_mode(mode);

        let mode_state = match mode {
            TimerMode::Countdown => self.countdown_mode.borrow(),
            TimerMode::Stopwatch => self.stopwatch_mode.borrow(),
            TimerMode::Breathing => self.breathing_mode.borrow(),
        };
        let (timer_state, display_secs) = (mode_state.timer_state, mode_state.display_secs);
        drop(mode_state);

        match timer_state {
            TimerState::Idle    => self.show_idle_ui(),
            TimerState::Paused  => self.show_paused_ui(display_secs),
            TimerState::Done    => self.view_stack.set_visible_child_name("done"),
            // Running normally can't reach here (the nav page blocks the toggle);
            // fall back to idle UI as a safety net.
            TimerState::Running => self.show_idle_ui(),
        }
    }

    fn show_idle_ui(&self) {
        self.start_btn.set_visible(true);
        self.resume_btn.set_visible(false);
        self.stop_from_pause_btn.set_visible(false);
        self.view_stack.set_visible_child_name("setup");
        let mode = self.current_mode();
        self.countdown_inputs.set_sensitive(true);
        self.boxbreath_inputs.set_sensitive(true);
        self.countdown_inputs.set_visible(mode == TimerMode::Countdown);
        self.boxbreath_inputs.set_visible(mode == TimerMode::Breathing);
        self.countdown_btn.set_sensitive(true);
        self.stopwatch_btn.set_sensitive(true);
        self.breathing_btn.set_sensitive(true);
        self.session_group.set_sensitive(true);
        self.refresh_hero_for_idle();
    }

    /// Paused state: same layout as idle, but the hero shows the live time,
    /// the subtitle says "Paused", and every interactive input is dimmed
    /// so the user can't change mode / presets / session settings until
    /// they Resume or Stop.
    fn show_paused_ui(&self, display_secs: u64) {
        self.start_btn.set_visible(false);
        self.resume_btn.set_visible(true);
        self.stop_from_pause_btn.set_visible(true);
        self.view_stack.set_visible_child_name("setup");
        self.countdown_inputs.set_sensitive(false);
        self.boxbreath_inputs.set_sensitive(false);
        self.countdown_btn.set_sensitive(false);
        self.stopwatch_btn.set_sensitive(false);
        self.breathing_btn.set_sensitive(false);
        self.session_group.set_sensitive(false);
        self.big_time_label.set_label(&format_time(display_secs));
        self.time_unit_label.set_label(&crate::i18n::gettext("Paused"));
        self.time_unit_label.set_visible(true);
    }

    /// Set the hero time display + subtitle to their idle-state values for
    /// whichever mode is currently active.
    fn refresh_hero_for_idle(&self) {
        let label = match self.current_mode() {
            TimerMode::Stopwatch => "00:00".to_string(),
            TimerMode::Countdown => {
                let secs = self.countdown_target_secs.get();
                let h = secs / 3600;
                let m = (secs % 3600) / 60;
                format!("{h:02}:{m:02}")
            }
            TimerMode::Breathing => {
                // Breathing sessions are always sub-hour by construction
                // (duration spinner caps at 60 min), but use the same
                // hh:mm format for layout consistency with countdown.
                let m = self.breathing_session_mins.get();
                format!("{:02}:{:02}", m / 60, m % 60)
            }
        };
        self.big_time_label.set_label(&label);
        self.time_unit_label.set_label(&crate::i18n::gettext("Hours · Minutes"));
        self.time_unit_label.set_visible(true);
    }
}

// ── Timer state machine ───────────────────────────────────────────────────────

impl TimerView {
    fn on_start(&self) {
        let mode = self.current_mode();

        match mode {
            TimerMode::Stopwatch => {
                let mut m = self.stopwatch_mode.borrow_mut();
                m.timer_state = TimerState::Running;
                m.display_secs = 0;
                m.session_start_time = unix_now();
                drop(m);
                self.start_instant.set(Some(Instant::now()));
                *self.stopwatch_core.borrow_mut() =
                    Some(CoreStopwatch::started_at(std::time::Duration::ZERO));
            }
            TimerMode::Countdown => {
                let target = self.countdown_target_secs.get();
                if target == 0 {
                    return;
                }
                let mut m = self.countdown_mode.borrow_mut();
                m.timer_state = TimerState::Running;
                m.target_secs = target;
                m.display_secs = target;
                m.session_start_time = unix_now();
                drop(m);
                // Anchor monotonic time and build the meditate-core countdown.
                self.start_instant.set(Some(Instant::now()));
                let timer = CoreCountdownTimer::new(std::time::Duration::from_secs(target));
                let sw = CoreStopwatch::started_at(std::time::Duration::ZERO);
                *self.countdown_core.borrow_mut() = Some(CoreCountdown::new(timer, sw));
            }
            TimerMode::Breathing => {
                let pattern = self.breathing_pattern.get();
                let cycle = pattern.cycle_secs().max(1) as u64;
                // "Finish the breath" before stopping: round the requested
                // minutes up to the next full cycle so the session always
                // ends on an exhale/hold-out boundary.
                let raw = self.breathing_session_mins.get() as u64 * 60;
                let target = raw.div_ceil(cycle) * cycle;
                let mut m = self.breathing_mode.borrow_mut();
                m.timer_state = TimerState::Running;
                m.target_secs = target;
                m.display_secs = 0;
                m.session_start_time = unix_now();
                self.breathing_elapsed_secs.set(0.0);
                drop(m);
                self.start_instant.set(Some(Instant::now()));
                *self.breath_stopwatch.borrow_mut() =
                    Some(CoreStopwatch::started_at(std::time::Duration::ZERO));
                self.breath_target.set(std::time::Duration::from_secs(target));
            }
        }

        self.tick_mode.set(mode);
        self.tick_is_stopwatch.set(mode == TimerMode::Stopwatch);
        // Countdown/stopwatch use the shared 1 Hz tick; Breathing drives
        // its own DrawingArea tick from window::push_running_page.
        if mode != TimerMode::Breathing {
            self.start_tick();
        }
        self.obj().emit_by_name::<()>("timer-started", &[]);
    }

    fn on_resume(&self) {
        let mode = self.current_mode();

        match mode {
            TimerMode::Stopwatch => {
                self.stopwatch_mode.borrow_mut().timer_state = TimerState::Running;
                let now = self.elapsed_since_start();
                let mut slot = self.stopwatch_core.borrow_mut();
                *slot = slot.take().map(|s| s.resumed_at(now));
            }
            TimerMode::Countdown => {
                self.countdown_mode.borrow_mut().timer_state = TimerState::Running;
                let now = self.elapsed_since_start();
                let mut slot = self.countdown_core.borrow_mut();
                *slot = slot.take().map(|c| c.resume(now));
            }
            TimerMode::Breathing => {
                self.breathing_mode.borrow_mut().timer_state = TimerState::Running;
                let now = self.elapsed_since_start();
                let mut slot = self.breath_stopwatch.borrow_mut();
                *slot = slot.take().map(|s| s.resumed_at(now));
            }
        }

        self.tick_mode.set(mode);
        self.tick_is_stopwatch.set(mode == TimerMode::Stopwatch);
        if mode != TimerMode::Breathing {
            self.start_tick();
        }
        self.obj().emit_by_name::<()>("timer-started", &[]);
    }

    /// Called by the window when the running page's Pause button is pressed.
    pub fn on_pause(&self) {
        self.cancel_tick();

        let mode = self.tick_mode.get();
        let display_secs = match mode {
            TimerMode::Stopwatch => {
                let mut m = self.stopwatch_mode.borrow_mut();
                m.timer_state = TimerState::Paused;
                let display = m.display_secs;
                drop(m);
                let now = self.elapsed_since_start();
                let mut slot = self.stopwatch_core.borrow_mut();
                *slot = slot.take().map(|s| s.paused_at(now));
                display
            }
            TimerMode::Countdown => {
                let mut m = self.countdown_mode.borrow_mut();
                m.timer_state = TimerState::Paused;
                let display = m.display_secs;
                drop(m);
                let now = self.elapsed_since_start();
                let mut slot = self.countdown_core.borrow_mut();
                *slot = slot.take().map(|c| c.pause(now));
                display
            }
            TimerMode::Breathing => {
                let now = self.elapsed_since_start();
                let mut slot = self.breath_stopwatch.borrow_mut();
                *slot = slot.take().map(|s| s.paused_at(now));
                drop(slot);
                let mut m = self.breathing_mode.borrow_mut();
                m.timer_state = TimerState::Paused;
                self.breathing_elapsed_secs.get() as u64
            }
        };

        self.show_paused_ui(display_secs);
        self.obj().emit_by_name::<()>("timer-paused", &[]);
    }

    /// Called by the window when Stop is pressed (from running page or paused state).
    pub fn on_stop(&self) {
        self.cancel_tick();

        let mode = self.current_mode();

        let elapsed = match mode {
            TimerMode::Stopwatch => {
                let mut m = self.stopwatch_mode.borrow_mut();
                m.timer_state = TimerState::Done;
                m.display_secs
            }
            TimerMode::Countdown => {
                let mut m = self.countdown_mode.borrow_mut();
                m.timer_state = TimerState::Done;
                m.target_secs.saturating_sub(m.display_secs)
            }
            TimerMode::Breathing => {
                let mut m = self.breathing_mode.borrow_mut();
                m.timer_state = TimerState::Done;
                self.breathing_elapsed_secs.get() as u64
            }
        };

        self.obj().emit_by_name::<()>("timer-stopped", &[]);
        self.show_done(elapsed);
    }

    fn show_done(&self, elapsed_secs: u64) {
        self.done_duration_label.set_label(&format_time(elapsed_secs));
        self.note_view.buffer().set_text("");
        self.repopulate_label_combo(self.setup_selected_label_id());
        self.view_stack.set_visible_child_name("done");
        // Without this, GTK's default-focus logic lands on `note_view` (the
        // first focusable descendant), which on phones pops the on-screen
        // keyboard up and hides Save/Discard. Putting focus on Save keeps
        // the action buttons visible; the user can still tap the note view
        // explicitly to start typing.
        self.save_btn.grab_focus();
    }

    fn on_save(&self) {
        crate::sound::stop_current();
        let mode = self.current_mode();

        let (elapsed, start_time) = match mode {
            TimerMode::Stopwatch => {
                let m = self.stopwatch_mode.borrow();
                (m.display_secs, m.session_start_time)
            }
            TimerMode::Countdown => {
                let m = self.countdown_mode.borrow();
                (m.target_secs.saturating_sub(m.display_secs), m.session_start_time)
            }
            TimerMode::Breathing => {
                let m = self.breathing_mode.borrow();
                (self.breathing_elapsed_secs.get() as u64, m.session_start_time)
            }
        };

        if elapsed == 0 {
            self.reset_mode(mode);
            return;
        }

        let note = {
            let buffer = self.note_view.buffer();
            let (start, end) = buffer.bounds();
            let t = buffer.text(&start, &end, false);
            if t.is_empty() { None } else { Some(t.to_string()) }
        };
        // Index 0 = "+ New label" (shouldn't reach Save), 1 = "None", 2+ = labels
        let selected = self.label_row.selected() as usize;
        let label_id = match selected {
            0 | 1 => None,
            n => self.db_labels.borrow().get(n - 2).map(|l| l.id),
        };

        let session_mode = match mode {
            TimerMode::Stopwatch => SessionMode::Stopwatch,
            TimerMode::Countdown => SessionMode::Countdown,
            TimerMode::Breathing => SessionMode::Breathing,
        };

        let data = SessionData {
            start_time,
            duration_secs: elapsed as i64,
            mode:          session_mode,
            label_id,
            note,
        };

        // Record the label the user actually saved under for this mode —
        // covers the case where they changed the selection on the Done
        // screen (setup_label_row's notify handler would miss that).
        let saved_label_name: Option<String> = label_id.and_then(|id| {
            self.db_labels.borrow().iter().find(|l| l.id == id).map(|l| l.name.clone())
        });
        self.persist_label_for_mode(mode, saved_label_name);

        // Fire-and-forget DB write on the blocking pool. SQLite fsync on
        // eMMC costs ~15 ms even with synchronous=NORMAL; doing it on the
        // main thread is directly felt as a stall at session end. When
        // the write lands we're back on the main thread (spawn_local) so
        // we can push the new session into the log feed incrementally
        // and mark stats stale for lazy refresh on tab re-entry.
        if let Some(app) = self.get_app() {
            glib::MainContext::default().spawn_local(async move {
                let result = app
                    .with_db_blocking(move |db| db.create_session(&data))
                    .await;
                let Some(Ok(session)) = result else { return; };

                app.invalidate(crate::application::InvalidateScope::STATS);
                if let Some(win) = app.active_window()
                    .and_then(|w| w.downcast::<crate::window::MeditateWindow>().ok())
                {
                    let imp = win.imp();
                    imp.log_view.prepend_session(session);
                    imp.timer_view.refresh_streak();
                }
            });
        }

        self.reset_mode(mode);
    }

    fn on_discard(&self) {
        crate::sound::stop_current();
        let buffer = self.note_view.buffer();
        let (start, end) = buffer.bounds();
        let note = buffer.text(&start, &end, false);
        if !note.is_empty() {
            let dialog = adw::AlertDialog::builder()
                .heading(crate::i18n::gettext("Discard Session?"))
                .body(crate::i18n::gettext("Your note will be lost."))
                .close_response("cancel")
                .default_response("discard")
                .build();
            // libadwaita-rs 0.9 doesn't expose set_response_use_underline,
            // so we can't mark a mnemonic letter on AdwAlertDialog buttons
            // without the underscore rendering literally. Return and Esc
            // still cover the common activations.
            dialog.add_response("cancel", &crate::i18n::gettext("Cancel"));
            dialog.add_response("discard", &crate::i18n::gettext("Discard"));
            dialog.set_response_appearance("discard", adw::ResponseAppearance::Destructive);

            let obj = self.obj().clone();
            let mode = self.current_mode();
            dialog.connect_response(None, move |_, id| {
                if id == "discard" {
                    obj.imp().reset_mode(mode);
                }
            });

            if let Some(win) = self.obj().root()
                .and_then(|r| r.downcast::<gtk::Window>().ok())
            {
                dialog.present(Some(&win));
            }
        } else {
            self.reset_mode(self.current_mode());
        }
    }

    /// Reset a single mode back to Idle and update the UI if it's currently shown.
    fn reset_mode(&self, mode: TimerMode) {
        match mode {
            TimerMode::Stopwatch => *self.stopwatch_mode.borrow_mut() = ModeState::default(),
            TimerMode::Countdown => *self.countdown_mode.borrow_mut() = ModeState::default(),
            TimerMode::Breathing => {
                *self.breathing_mode.borrow_mut() = ModeState::default();
                self.breathing_elapsed_secs.set(0.0);
            }
        }

        // Only update the visible UI if this mode is the one currently shown.
        if mode == self.current_mode() {
            self.show_idle_ui();
            self.refresh_streak();
        }
    }

    fn start_tick(&self) {
        self.cancel_tick();
        let obj = self.obj().clone();
        let is_stopwatch = self.tick_is_stopwatch.get();

        let source_id = glib::timeout_add_local(
            std::time::Duration::from_secs(1),
            move || {
                let imp = obj.imp();

                // Read + update the correct mode state
                let (new_secs, done) = {
                    let mut m = if is_stopwatch {
                        imp.stopwatch_mode.borrow_mut()
                    } else {
                        imp.countdown_mode.borrow_mut()
                    };

                    if m.timer_state != TimerState::Running {
                        return glib::ControlFlow::Break;
                    }

                    if is_stopwatch {
                        // Stopwatch: query meditate-core; floor is correct
                        // here (display "0:01" once we cross 1.0s).
                        let now = imp.elapsed_since_start();
                        let sw = imp.stopwatch_core.borrow();
                        let Some(s) = sw.as_ref() else {
                            return glib::ControlFlow::Break;
                        };
                        let elapsed = s.elapsed(now).as_secs();
                        m.display_secs = elapsed;
                        (elapsed, false)
                    } else {
                        // Countdown: query meditate-core via wall-clock; this
                        // makes the countdown immune to tick drift / OS suspend.
                        let now = imp.elapsed_since_start();
                        let core = imp.countdown_core.borrow();
                        let Some(c) = core.as_ref() else {
                            // Should not happen — on_start always sets it.
                            return glib::ControlFlow::Break;
                        };
                        if c.is_finished(now) {
                            m.timer_state = TimerState::Done;
                            m.display_secs = 0;
                            (m.target_secs, true)
                        } else {
                            // Ceiling seconds for display: while remaining is
                            // in (k-1, k] seconds, show k. Truncation would
                            // skip the "0:59" frame on the first tick after
                            // start (which fires slightly past t=1.0s).
                            let r = c.remaining(now);
                            let remaining = r.as_secs() + (r.subsec_nanos() > 0) as u64;
                            m.display_secs = remaining;
                            (remaining, false)
                        }
                    }
                };

                if done {
                    // Clear the SourceId before GLib removes it. If we leave it
                    // set, cancel_tick() in dispose() will call src.remove() on
                    // an already-removed source and panic.
                    *imp.tick_source.borrow_mut() = None;
                    *imp.running_label.borrow_mut() = None;

                    obj.emit_by_name::<()>("timer-stopped", &[]);
                    imp.show_done(new_secs);
                    if let Some(app) = imp.get_app() {
                        crate::sound::play_end_sound(&app);
                        crate::vibration::trigger_if_enabled(&app);
                        // Only send a system notification when the app is not
                        // the focused window — the done screen is already shown
                        // in-app, so a notification would be redundant noise.
                        if !app.active_window().map(|w| w.is_active()).unwrap_or(false) {
                            let n = gtk::gio::Notification::new("Meditation Complete");
                            n.set_body(Some(&format!("Session: {}", format_time(new_secs))));
                            app.send_notification(Some("timer-done"), &n);
                        }
                    }
                    return glib::ControlFlow::Break;
                }

                if let Some(label) = imp.running_label.borrow().as_ref() {
                    label.set_label(&format_time(new_secs));
                }

                glib::ControlFlow::Continue
            },
        );
        *self.tick_source.borrow_mut() = Some(source_id);
    }

    /// Natural completion path for a breath session: marks Done, plays the
    /// end chime, vibrates, and sends a notification when not focused.
    /// Mirrors the countdown's done branch (timer.imp at the 1 Hz tick).
    /// Distinct from `on_stop` (user-initiated), which is silent.
    pub(super) fn finish_breath_session(&self) {
        self.breathing_mode.borrow_mut().timer_state = TimerState::Done;
        let elapsed = self.breathing_elapsed_secs.get() as u64;
        self.obj().emit_by_name::<()>("timer-stopped", &[]);
        self.show_done(elapsed);
        if let Some(app) = self.get_app() {
            crate::sound::play_end_sound(&app);
            crate::vibration::trigger_if_enabled(&app);
            if !app.active_window().map(|w| w.is_active()).unwrap_or(false) {
                let n = gtk::gio::Notification::new("Meditation Complete");
                n.set_body(Some(&format!("Session: {}", format_time(elapsed))));
                app.send_notification(Some("timer-done"), &n);
            }
        }
    }

    /// Wall-clock-anchored elapsed time of the active breath session.
    /// Returns ZERO if no session is running. Pause freezes this value.
    pub(super) fn breath_elapsed(&self) -> std::time::Duration {
        let now = self.elapsed_since_start();
        self.breath_stopwatch
            .borrow()
            .as_ref()
            .map(|s| s.elapsed(now))
            .unwrap_or_default()
    }

    pub(super) fn breath_is_finished(&self) -> bool {
        self.breath_elapsed() >= self.breath_target.get()
    }

    /// Monotonic time since `on_start` set the anchor, used for feeding
    /// elapsed into the meditate-core `Countdown`. Returns ZERO if no
    /// session has been started — defensive, the tick won't be running
    /// in that case.
    fn elapsed_since_start(&self) -> std::time::Duration {
        self.start_instant
            .get()
            .map(|t| t.elapsed())
            .unwrap_or(std::time::Duration::ZERO)
    }

    fn cancel_tick(&self) {
        if let Some(src) = self.tick_source.borrow_mut().take() {
            src.remove();
        }
        *self.running_label.borrow_mut() = None;
    }

    pub fn refresh_streak(&self) {
        let Some(app) = self.get_app() else {
            // No app yet (shouldn't happen in practice) — use defaults.
            self.refresh_presets();
            self.refresh_setup_labels(self.setup_selected_label_id());
            return;
        };

        // Batch all three DB reads into a single borrow: one get_app() walk,
        // one RefCell lock, three SQL queries instead of three separate calls.
        let (streak, presets, labels) = app
            .with_db(|db| {
                let streak  = db.get_streak().unwrap_or(0);
                let presets = db.get_presets().unwrap_or_else(|_| vec![5, 10, 15, 20, 30]);
                let labels  = db.list_labels().unwrap_or_default();
                (streak, presets, labels)
            })
            .unwrap_or_else(|| (0, vec![5, 10, 15, 20, 30], vec![]));

        // Update streak label. .streak-chip applies text-transform:
        // uppercase, so we keep the source text sentence-case here.
        let text = match streak {
            0 => crate::i18n::gettext("Start your streak today"),
            1 => crate::i18n::gettext("1 day streak"),
            n => crate::i18n::gettext("{n} days streak").replace("{n}", &n.to_string()),
        };
        self.streak_label.set_label(&text);

        // Rebuild preset buttons with the data we already fetched
        self.rebuild_preset_chips(&presets);

        // Populate setup page sound row from DB setting.
        // Build the model here so we can route each option through gettext.
        let sound_choices = [
            crate::i18n::gettext("None"),
            crate::i18n::gettext("Singing bowl"),
            crate::i18n::gettext("Bell"),
            crate::i18n::gettext("Gong"),
            crate::i18n::gettext("Custom file…"),
        ];
        let sound_refs: Vec<&str> = sound_choices.iter().map(|s| s.as_str()).collect();
        // set_model() resets `selected` to 0, which fires the notify handler
        // — without the guard in place it'd persist "none" into the DB before
        // we get to read the actual setting below. Raise the flag first.
        self.sound_populating.set(true);
        self.setup_sound_row.set_model(Some(&gtk::StringList::new(&sound_refs)));
        let current_sound = app
            .with_db(|db| db.get_setting("end_sound", "bowl"))
            .and_then(|r| r.ok())
            .unwrap_or_else(|| "bowl".to_string());
        self.setup_sound_row.set_selected(match current_sound.as_str() {
            "bowl"   => 1,
            "bell"   => 2,
            "gong"   => 3,
            "custom" => 4,
            _        => 0,
        });
        self.sound_populating.set(false);

        // Rebuild setup label combo. The selection comes from the per-mode
        // persisted preference (via `apply_preferred_label_for_mode`)
        // rather than whatever `setup_label_row` happened to hold, so that:
        //   - on first launch, each mode starts at its documented default
        //     (None / None / Box-breathing);
        //   - after a Save on the Done screen changes the label, the next
        //     setup entry reflects the new choice instead of reverting to
        //     the stale setup-combo selection.
        // `apply_preferred_label_for_mode` → `refresh_setup_labels` does its
        // own model build + selection, so we can drop the redundant inline
        // version. The extra DB round-trips are trivial next to the visit-
        // triggered streak/preset queries we're already doing.
        *self.setup_db_labels.borrow_mut() = labels;
        self.apply_preferred_label_for_mode(self.current_mode());
    }

    pub fn refresh_presets(&self) {
        let presets = self.get_app()
            .and_then(|app| app.with_db(|db| db.get_presets()))
            .and_then(|r| r.ok())
            .unwrap_or_else(|| vec![5, 10, 15, 20, 30]);
        self.rebuild_preset_chips(&presets);
    }

    /// Rebuild the preset FlowBox: one pill per DB preset (each tapping
    /// it selects that duration), plus a trailing "Custom" pill that
    /// opens a dialog to pick an arbitrary H:M value.
    fn rebuild_preset_chips(&self, presets: &[u32]) {
        while let Some(child) = self.presets_box.first_child() {
            self.presets_box.remove(&child);
        }
        let mut tracked: Vec<(gtk::Button, u32)> = Vec::with_capacity(presets.len());
        let obj = self.obj();
        for &mins in presets {
            let (label, tooltip) = if mins < 60 {
                (format!("{mins}m"), format!("{mins} minutes"))
            } else {
                let h = mins / 60;
                let m = mins % 60;
                if m == 0 {
                    (format!("{h}h"), format!("{h} hour{}", if h == 1 { "" } else { "s" }))
                } else {
                    (format!("{h}h {m}m"), format!("{h}h {m}min"))
                }
            };
            let btn = gtk::Button::builder()
                .label(&label)
                .tooltip_text(&tooltip)
                .css_classes(["preset-chip"])
                .build();
            btn.connect_clicked(glib::clone!(
                #[weak(rename_to = this)] obj,
                move |_| {
                    this.imp().set_countdown_target((mins as u64) * 60);
                }
            ));
            self.presets_box.append(&btn);
            tracked.push((btn, mins));
        }

        // Trailing "Custom" pill — opens a dialog to pick an H:M value.
        let custom_btn = gtk::Button::builder()
            .label(crate::i18n::gettext("Custom…"))
            .tooltip_text(crate::i18n::gettext("Set a Custom Time"))
            .css_classes(["preset-chip"])
            .build();
        custom_btn.connect_clicked(glib::clone!(
            #[weak(rename_to = this)] obj,
            move |_| this.imp().show_custom_time_dialog()
        ));
        self.presets_box.append(&custom_btn);

        *self.preset_buttons.borrow_mut() = tracked;
        *self.custom_preset_btn.borrow_mut() = Some(custom_btn);
        // Reapply active highlight for the current target.
        self.refresh_preset_selection();
    }

    /// Toggle the `.preset-chip-active` class on whichever chip matches
    /// the current countdown_target_secs (or on the Custom pill if no
    /// preset matches). Called whenever the target changes.
    fn refresh_preset_selection(&self) {
        let target_mins = (self.countdown_target_secs.get() / 60) as u32;
        let mut matched = false;
        for (btn, mins) in self.preset_buttons.borrow().iter() {
            if *mins == target_mins {
                btn.add_css_class("preset-chip-active");
                matched = true;
            } else {
                btn.remove_css_class("preset-chip-active");
            }
        }
        if let Some(custom) = self.custom_preset_btn.borrow().as_ref() {
            if matched {
                custom.remove_css_class("preset-chip-active");
            } else {
                custom.add_css_class("preset-chip-active");
            }
        }
    }

    /// Update the countdown target + hero label + preset highlight together.
    fn set_countdown_target(&self, secs: u64) {
        self.countdown_target_secs.set(secs);
        let h = secs / 3600;
        let m = (secs % 3600) / 60;
        self.big_time_label.set_label(&format!("{h:02}:{m:02}"));
        self.refresh_preset_selection();
    }

    /// Show a dialog with H:M spin buttons; apply result to the countdown
    /// target on "Set".
    fn show_custom_time_dialog(&self) {
        let current = self.countdown_target_secs.get();
        let cur_h = (current / 3600) as f64;
        let cur_m = ((current % 3600) / 60) as f64;

        // Tooltips double as accessible names — without them screen
        // readers only announce the raw numeric value.
        let hours_spin = gtk::SpinButton::builder()
            .orientation(gtk::Orientation::Vertical)
            .numeric(true)
            .width_chars(2)
            .adjustment(&gtk::Adjustment::new(cur_h, 0.0, 23.0, 1.0, 1.0, 0.0))
            .tooltip_text(crate::i18n::gettext("Hours"))
            .build();
        let minutes_spin = gtk::SpinButton::builder()
            .orientation(gtk::Orientation::Vertical)
            .numeric(true)
            .width_chars(2)
            .adjustment(&gtk::Adjustment::new(cur_m, 0.0, 59.0, 1.0, 5.0, 0.0))
            .tooltip_text(crate::i18n::gettext("Minutes"))
            .build();

        let colon = gtk::Label::builder()
            .label(":")
            .css_classes(["title-2"])
            .valign(gtk::Align::Center)
            .build();
        let row = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(6)
            .halign(gtk::Align::Center)
            .margin_top(6)
            .margin_bottom(6)
            .build();
        row.append(&hours_spin);
        row.append(&colon);
        row.append(&minutes_spin);

        let dialog = adw::AlertDialog::builder()
            .heading(crate::i18n::gettext("Custom Time"))
            .body(crate::i18n::gettext("Hours : Minutes"))
            .close_response("cancel")
            .default_response("set")
            .extra_child(&row)
            .build();
        dialog.add_response("cancel", &crate::i18n::gettext("Cancel"));
        dialog.add_response("set", &crate::i18n::gettext("Set"));
        dialog.set_response_appearance("set", adw::ResponseAppearance::Suggested);

        let obj = self.obj().clone();
        dialog.connect_response(None, move |_, response| {
            if response != "set" { return; }
            let h = hours_spin.value() as u64;
            let m = minutes_spin.value() as u64;
            let total = h * 3600 + m * 60;
            if total == 0 { return; }
            obj.imp().set_countdown_target(total);
        });

        if let Some(win) = self.obj().root().and_then(|r| r.downcast::<gtk::Window>().ok()) {
            dialog.present(Some(&win));
        }
    }

    /// Rebuild the label combo from the DB.
    /// `select_id`: if Some, auto-selects that label; otherwise selects "None" (index 1).
    fn repopulate_label_combo(&self, select_id: Option<i64>) {
        let mut labels = Vec::new();
        if let Some(app) = self.get_app() {
            if let Some(fetched) = app.with_db(|db| db.list_labels()) {
                labels = fetched.unwrap_or_default();
            }
        }

        let select_idx = select_id
            .and_then(|id| labels.iter().position(|l| l.id == id))
            .map(|pos| (pos + 2) as u32) // +2 for "+ New label" and "None"
            .unwrap_or(1);              // default = "None"

        let names: Vec<String> = std::iter::once(crate::i18n::gettext("+ New Label…"))
            .chain(std::iter::once(crate::i18n::gettext("None")))
            .chain(labels.iter().map(|l| l.name.clone()))
            .collect();
        let name_refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();

        *self.db_labels.borrow_mut() = labels;

        self.populating_labels.set(true);
        self.label_row.set_model(Some(&gtk::StringList::new(&name_refs)));
        self.label_row.set_selected(select_idx);
        self.populating_labels.set(false);
    }

    /// Populate the pre-start label combo from the DB.
    /// `select_id`: if Some, keeps that label selected; otherwise selects "None".
    fn refresh_setup_labels(&self, select_id: Option<i64>) {
        let labels = self.get_app()
            .and_then(|app| app.with_db(|db| db.list_labels()))
            .and_then(|r| r.ok())
            .unwrap_or_default();

        let select_idx = select_id
            .and_then(|id| labels.iter().position(|l| l.id == id))
            .map(|pos| (pos + 2) as u32)
            .unwrap_or(1); // default: "None"

        let names: Vec<String> = std::iter::once(crate::i18n::gettext("+ New Label…"))
            .chain(std::iter::once(crate::i18n::gettext("None")))
            .chain(labels.iter().map(|l| l.name.clone()))
            .collect();
        let name_refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();

        *self.setup_db_labels.borrow_mut() = labels;
        self.setup_populating.set(true);
        self.setup_label_row.set_model(Some(&gtk::StringList::new(&name_refs)));
        self.setup_label_row.set_selected(select_idx);
        self.setup_populating.set(false);
    }

    /// Returns the label ID currently selected in the pre-start combo, if any.
    fn setup_selected_label_id(&self) -> Option<i64> {
        let selected = self.setup_label_row.selected() as usize;
        match selected {
            0 | 1 => None,
            n => self.setup_db_labels.borrow().get(n - 2).map(|l| l.id),
        }
    }

    /// Show the new-label dialog, selecting the result in the pre-start combo.
    fn show_new_label_dialog_for_setup(&self) {
        let (entry, dialog) = build_new_label_dialog();
        let obj = self.obj().clone();
        dialog.connect_response(None, {
            let entry = entry.clone();
            move |_, response| {
                let imp = obj.imp();
                if response != "create" {
                    imp.setup_label_row.set_selected(1); // revert to "None"
                    return;
                }
                let name = entry.text().trim().to_string();
                if name.is_empty() { imp.setup_label_row.set_selected(1); return; }
                let new_label = imp.get_app()
                    .and_then(|app| app.with_db(|db| db.create_label(&name)))
                    .and_then(|r| r.ok());
                imp.refresh_setup_labels(new_label.map(|l| l.id));
            }
        });
        if let Some(win) = self.obj().root().and_then(|r| r.downcast::<gtk::Window>().ok()) {
            dialog.present(Some(&win));
        }
    }

    /// Show a dialog to create a new label, then select it in the done-page combo.
    fn show_new_label_dialog(&self) {
        let (entry, dialog) = build_new_label_dialog();
        let obj = self.obj().clone();
        dialog.connect_response(None, {
            let entry = entry.clone();
            move |_, response| {
                let imp = obj.imp();
                if response != "create" {
                    imp.label_row.set_selected(1); // revert to "None"
                    return;
                }
                let name = entry.text().trim().to_string();
                if name.is_empty() { imp.label_row.set_selected(1); return; }
                let new_label = imp.get_app()
                    .and_then(|app| app.with_db(|db| db.create_label(&name)))
                    .and_then(|r| r.ok());
                imp.repopulate_label_combo(new_label.map(|l| l.id));
            }
        });
        if let Some(win) = self.obj().root().and_then(|r| r.downcast::<gtk::Window>().ok()) {
            dialog.present(Some(&win));
        }
    }

    fn get_app(&self) -> Option<crate::application::MeditateApplication> {
        self.obj()
            .root()
            .and_then(|r| r.downcast::<gtk::Window>().ok())
            .and_then(|w| w.application())
            .and_then(|a| a.downcast::<crate::application::MeditateApplication>().ok())
    }

    pub fn current_display_secs(&self) -> u64 {
        // Return the display value for whichever mode is about to go running.
        match self.tick_mode.get() {
            TimerMode::Stopwatch => self.stopwatch_mode.borrow().display_secs,
            TimerMode::Countdown => self.countdown_mode.borrow().display_secs,
            TimerMode::Breathing => self.breathing_elapsed_secs.get() as u64,
        }
    }

    pub fn set_running_label(&self, label: gtk::Label) {
        *self.running_label.borrow_mut() = Some(label);
    }

    pub fn toggle_playback(&self) {
        let state = match self.current_mode() {
            TimerMode::Stopwatch => self.stopwatch_mode.borrow().timer_state,
            TimerMode::Countdown => self.countdown_mode.borrow().timer_state,
            TimerMode::Breathing => self.breathing_mode.borrow().timer_state,
        };
        match state {
            TimerState::Idle    => self.on_start(),
            TimerState::Running => self.on_pause(),
            TimerState::Paused  => self.on_resume(),
            TimerState::Done    => {}
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

pub fn format_time(secs: u64) -> String {
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m:02}:{s:02}")
    }
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

// ── Breathing (Box Breath) setup wiring ───────────────────────────────────────

const BREATHING_PRESETS: &[(&str, BreathPattern)] = &[
    ("4-4-4-4", BreathPattern { in_secs: 4, hold_in: 4, out_secs: 4, hold_out: 4 }),
    ("4-7-8-0", BreathPattern { in_secs: 4, hold_in: 7, out_secs: 8, hold_out: 0 }),
    ("5-5-5-5", BreathPattern { in_secs: 5, hold_in: 5, out_secs: 5, hold_out: 5 }),
];

/// Minimum cycle length we allow — prevents a 0-0-0-0 pattern from ever
/// reaching the running view, which would panic phase_at.
const MIN_CYCLE_SECS: u32 = 1;
const PHASE_MAX_SECS: u32 = 20;

impl TimerView {
    fn build_breathing_setup(&self) {
        self.build_phase_tiles();
        self.build_breathing_presets();
        self.configure_breathing_duration_row();
        // Load persisted values — overrides defaults set in `constructed`.
        self.load_breathing_settings();
        self.refresh_phase_tiles();
        self.refresh_breathing_preset_state();
    }

    fn build_phase_tiles(&self) {
        use crate::i18n::gettext;
        // Index-aligned with the four fields of `BreathPattern`.
        let specs: [(&str, &str); 4] = [
            (&gettext("Inhale"),       "go-up-symbolic"),
            (&gettext("Hold (full)"),  "media-playback-pause-symbolic"),
            (&gettext("Exhale"),       "go-down-symbolic"),
            (&gettext("Hold (empty)"), "media-playback-pause-symbolic"),
        ];
        let obj = self.obj();
        let mut value_labels = self.phase_value_labels.borrow_mut();
        for (i, (title, icon_name)) in specs.iter().enumerate() {
            let tile = self.build_phase_tile(i as u8, title, icon_name, &obj);
            value_labels[i] = Some(tile.1);
            // 2×2 layout: (col, row) = (i%2, i/2).
            let col = (i % 2) as i32;
            let row = (i / 2) as i32;
            self.phase_tiles_grid.attach(&tile.0, col, row, 1, 1);
        }
    }

    /// Build a single phase tile: icon + title on one row, −/value/+ stepper
    /// below. Returns the tile Box and the value Label so the caller can
    /// update it on state change.
    fn build_phase_tile(
        &self,
        index: u8,
        title: &str,
        icon_name: &str,
        timer_obj: &super::TimerView,
    ) -> (gtk::Box, gtk::Label) {
        let tile = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(6)
            .css_classes(["card", "phase-tile"])
            .build();

        // Top row: icon + title.
        let head = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(6)
            .margin_top(10)
            .margin_start(12)
            .margin_end(12)
            .build();
        let icon = gtk::Image::from_icon_name(icon_name);
        icon.add_css_class("accent");
        let title_label = gtk::Label::builder()
            .label(title)
            .xalign(0.0)
            .hexpand(true)
            .css_classes(["caption", "dimmed"])
            .build();
        head.append(&icon);
        head.append(&title_label);
        tile.append(&head);

        // Stepper row: − value +
        let stepper = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(6)
            .halign(gtk::Align::Center)
            .margin_bottom(10)
            .margin_start(12)
            .margin_end(12)
            .build();
        let minus = gtk::Button::builder()
            .icon_name("list-remove-symbolic")
            .css_classes(["flat", "circular"])
            .tooltip_text(crate::i18n::gettext("Decrease"))
            .build();
        let value_label = gtk::Label::builder()
            .label("4s")
            .width_request(40)
            .xalign(0.5)
            .css_classes(["title-4", "numeric"])
            .build();
        let plus = gtk::Button::builder()
            .icon_name("list-add-symbolic")
            .css_classes(["flat", "circular"])
            .tooltip_text(crate::i18n::gettext("Increase"))
            .build();
        stepper.append(&minus);
        stepper.append(&value_label);
        stepper.append(&plus);
        tile.append(&stepper);

        // Hold phases (index 1, 3) accept 0s (no hold); inhale/exhale must
        // be at least 1s or the cycle would degenerate.
        let min_val: u32 = if index == 1 || index == 3 { 0 } else { 1 };

        let tv = timer_obj.clone();
        minus.connect_clicked(move |_| tv.imp().adjust_phase(index, -1, min_val));
        let tv = timer_obj.clone();
        plus.connect_clicked(move |_| tv.imp().adjust_phase(index, 1, min_val));

        (tile, value_label)
    }

    fn build_breathing_presets(&self) {
        let obj = self.obj();
        let mut buttons = self.breathing_preset_buttons.borrow_mut();
        buttons.clear();
        for (name, pattern) in BREATHING_PRESETS {
            let btn = gtk::Button::builder()
                .label(*name)
                .css_classes(["pill", "preset-chip"])
                .build();
            let name_owned = name.to_string();
            let pattern = *pattern;
            let tv = obj.clone();
            btn.connect_clicked(move |_| tv.imp().on_breathing_preset_clicked(&name_owned, pattern));
            let child = gtk::FlowBoxChild::builder()
                .can_focus(false)
                .build();
            child.set_child(Some(&btn));
            self.breathing_presets_box.append(&child);
            buttons.push((btn, pattern, name.to_string()));
        }
    }

    fn configure_breathing_duration_row(&self) {
        let adj = gtk::Adjustment::new(5.0, 1.0, 60.0, 1.0, 5.0, 0.0);
        self.breathing_duration_row.set_adjustment(Some(&adj));
        let obj = self.obj();
        self.breathing_duration_row.connect_notify_local(
            Some("value"),
            glib::clone!(
                #[weak(rename_to = this)] obj,
                move |row, _| {
                    let imp = this.imp();
                    if imp.breathing_populating.get() { return; }
                    let v = row.value().round().clamp(1.0, 60.0) as u32;
                    imp.breathing_session_mins.set(v);
                    imp.save_breathing_settings();
                    // Hero mirrors the minutes spinner while in breathing mode.
                    if imp.current_mode() == TimerMode::Breathing {
                        imp.refresh_hero_for_idle();
                    }
                }
            ),
        );
    }

    fn adjust_phase(&self, index: u8, delta: i32, min_val: u32) {
        let mut p = self.breathing_pattern.get();
        let slot: &mut u32 = match index {
            0 => &mut p.in_secs,
            1 => &mut p.hold_in,
            2 => &mut p.out_secs,
            3 => &mut p.hold_out,
            _ => return,
        };
        let new_val = (*slot as i32 + delta).clamp(min_val as i32, PHASE_MAX_SECS as i32) as u32;
        if new_val == *slot {
            return;
        }
        *slot = new_val;
        if p.cycle_secs() < MIN_CYCLE_SECS {
            // Defence in depth; shouldn't fire given the per-slot minimums
            // above enforce at least inhale=1 + exhale=1.
            return;
        }
        self.breathing_pattern.set(p);
        // Any manual edit drops us out of preset-land.
        *self.breathing_preset_name.borrow_mut() = "custom".to_string();
        self.refresh_phase_tiles();
        self.refresh_breathing_preset_state();
        self.save_breathing_settings();
    }

    fn on_breathing_preset_clicked(&self, name: &str, pattern: BreathPattern) {
        self.breathing_pattern.set(pattern);
        *self.breathing_preset_name.borrow_mut() = name.to_string();
        self.refresh_phase_tiles();
        self.refresh_breathing_preset_state();
        self.save_breathing_settings();
    }

    fn refresh_phase_tiles(&self) {
        let p = self.breathing_pattern.get();
        let vals = [p.in_secs, p.hold_in, p.out_secs, p.hold_out];
        let labels = self.phase_value_labels.borrow();
        for (i, val) in vals.iter().enumerate() {
            if let Some(l) = labels[i].as_ref() {
                l.set_label(&format!("{val}s"));
            }
        }
    }

    fn refresh_breathing_preset_state(&self) {
        let active = self.breathing_preset_name.borrow().clone();
        for (btn, _, name) in self.breathing_preset_buttons.borrow().iter() {
            if name == &active {
                btn.add_css_class("preset-chip-active");
            } else {
                btn.remove_css_class("preset-chip-active");
            }
        }
    }

    fn load_breathing_settings(&self) {
        let Some(app) = self.get_app() else { return; };
        self.breathing_populating.set(true);
        let (p, mins, preset) = app.with_db(|db| {
            let read = |k: &str, default: u32| -> u32 {
                db.get_setting(k, &default.to_string())
                    .ok()
                    .and_then(|s| s.parse::<u32>().ok())
                    .unwrap_or(default)
            };
            let p = BreathPattern {
                in_secs:  read("breathing_in", 4).clamp(1, PHASE_MAX_SECS),
                hold_in:  read("breathing_hold_in", 4).clamp(0, PHASE_MAX_SECS),
                out_secs: read("breathing_out", 4).clamp(1, PHASE_MAX_SECS),
                hold_out: read("breathing_hold_out", 4).clamp(0, PHASE_MAX_SECS),
            };
            let mins = read("breathing_session_mins", 5).clamp(1, 60);
            let preset = db.get_setting("breathing_preset", "4-4-4-4").unwrap_or_else(|_| "4-4-4-4".to_string());
            (p, mins, preset)
        }).unwrap_or((
            BreathPattern { in_secs: 4, hold_in: 4, out_secs: 4, hold_out: 4 },
            5,
            "4-4-4-4".to_string(),
        ));
        self.breathing_pattern.set(p);
        self.breathing_session_mins.set(mins);
        *self.breathing_preset_name.borrow_mut() = preset;
        self.breathing_duration_row.set_value(mins as f64);
        self.breathing_populating.set(false);
    }

    fn save_breathing_settings(&self) {
        if self.breathing_populating.get() { return; }
        let Some(app) = self.get_app() else { return; };
        let p = self.breathing_pattern.get();
        let mins = self.breathing_session_mins.get();
        let preset = self.breathing_preset_name.borrow().clone();
        app.with_db(|db| {
            let _ = db.set_setting("breathing_in", &p.in_secs.to_string());
            let _ = db.set_setting("breathing_hold_in", &p.hold_in.to_string());
            let _ = db.set_setting("breathing_out", &p.out_secs.to_string());
            let _ = db.set_setting("breathing_hold_out", &p.hold_out.to_string());
            let _ = db.set_setting("breathing_session_mins", &mins.to_string());
            let _ = db.set_setting("breathing_preset", &preset);
        });
    }

    /// Apply the user's last-chosen label for the given mode to the setup
    /// label combo. Each of the three modes carries its own preference:
    /// Breathing remembers "Box-breathing" (auto-created on first entry),
    /// Countdown and Stopwatch default to "None" until the user picks a
    /// label and saves or changes the selection.
    fn apply_preferred_label_for_mode(&self, mode: TimerMode) {
        let pref = self.persisted_label_for_mode(mode);
        let label_id: Option<i64> = match (mode, pref) {
            // First-time Breathing: the "Box-breathing" label is the shipped
            // default, create on demand so users don't have to set it up.
            (TimerMode::Breathing, None) => self.get_app().and_then(|app| {
                app.with_db(|db| db.find_or_create_label(
                    &crate::i18n::gettext("Box-breathing"),
                ).ok()).flatten()
            }),
            // First-time Countdown / Stopwatch, or explicit None: no label.
            (_, None) | (_, Some(None)) => None,
            // Explicit name: look up an *existing* label. We deliberately
            // do not auto-recreate a deleted label — if the user removed
            // Box-breathing, respect that and fall back to no label.
            (_, Some(Some(name))) => self.get_app().and_then(|app| {
                app.with_db(|db| {
                    db.list_labels().ok().and_then(|labels| labels.into_iter()
                        .find(|l| l.name.to_lowercase() == name.to_lowercase())
                        .map(|l| l.id))
                }).flatten()
            }),
        };
        self.refresh_setup_labels(label_id);
    }

    /// Read the persisted "last label" for this mode. Returns None when the
    /// key is entirely missing (first launch / first visit to the mode), so
    /// callers can tell apart "user explicitly chose None" from "never
    /// touched". The inner Option distinguishes None-selection (Some(None))
    /// from a named label (Some(Some(name))).
    fn persisted_label_for_mode(&self, mode: TimerMode) -> Option<Option<String>> {
        const SENTINEL: &str = "\x01unset\x01";
        let app = self.get_app()?;
        let key = label_setting_key(mode);
        let val = app.with_db(|db| db.get_setting(key, SENTINEL)
            .unwrap_or_else(|_| SENTINEL.to_string()))?;
        if val == SENTINEL {
            None
        } else if val.is_empty() {
            Some(None)
        } else {
            Some(Some(val))
        }
    }

    /// Store (or clear) the "last label" preference for this mode. Empty
    /// string means "user picked None"; anything else is the label's name.
    fn persist_label_for_mode(&self, mode: TimerMode, name: Option<String>) {
        let Some(app) = self.get_app() else { return; };
        let key = label_setting_key(mode);
        let val = name.unwrap_or_default();
        app.with_db(|db| { let _ = db.set_setting(key, &val); });
    }
}

fn label_setting_key(mode: TimerMode) -> &'static str {
    match mode {
        TimerMode::Countdown => "last_label_countdown",
        TimerMode::Stopwatch => "last_label_stopwatch",
        TimerMode::Breathing => "last_label_breathing",
    }
}

/// Build the shared "New Label" alert dialog + text entry.
fn build_new_label_dialog() -> (gtk::Entry, adw::AlertDialog) {
    let entry = gtk::Entry::builder()
        .placeholder_text(crate::i18n::gettext("Label name"))
        .activates_default(true)
        .build();
    let dialog = adw::AlertDialog::builder()
        .heading(crate::i18n::gettext("New Label"))
        .close_response("cancel")
        .default_response("create")
        .build();
    dialog.add_response("cancel", &crate::i18n::gettext("Cancel"));
    dialog.add_response("create", &crate::i18n::gettext("Create"));
    dialog.set_response_appearance("create", adw::ResponseAppearance::Suggested);
    dialog.set_response_enabled("create", false);
    dialog.set_extra_child(Some(&entry));
    entry.connect_changed(glib::clone!(
        #[weak] dialog,
        move |e| dialog.set_response_enabled("create", !e.text().trim().is_empty())
    ));
    (entry, dialog)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_time_sub_hour_pads_to_two_digits() {
        assert_eq!(format_time(0), "00:00");
        assert_eq!(format_time(1), "00:01");
        assert_eq!(format_time(59), "00:59");
        assert_eq!(format_time(60), "01:00");
        assert_eq!(format_time(61), "01:01");
        assert_eq!(format_time(10 * 60), "10:00");
        assert_eq!(format_time(59 * 60 + 59), "59:59");
    }

    #[test]
    fn format_time_hour_mark_switches_format() {
        // At one hour the formatter switches from MM:SS to H:MM:SS.
        assert_eq!(format_time(3600), "1:00:00");
        assert_eq!(format_time(3600 + 1), "1:00:01");
        assert_eq!(format_time(3600 + 60), "1:01:00");
        assert_eq!(format_time(3661), "1:01:01");
        assert_eq!(format_time(2 * 3600 + 5 * 60 + 9), "2:05:09");
        assert_eq!(format_time(10 * 3600), "10:00:00");
    }
}
