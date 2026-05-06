use std::cell::{Cell, RefCell};
use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::{glib, CompositeTemplate};
use glib::subclass::Signal;
use std::sync::OnceLock;

use crate::db::{Label, SessionData, SessionMode};
use super::breathing::Pattern as BreathPattern;

use meditate_core::timer::{
    Countdown as CoreCountdown, CountdownTimer as CoreCountdownTimer,
    Stopwatch as CoreStopwatch,
};

/// Suspend-resilient monotonic time. Linux's `std::time::Instant` uses
/// CLOCK_MONOTONIC, which freezes during system suspend — a 30s suspend
/// in the middle of a session would silently lose 30s of countdown.
/// CLOCK_BOOTTIME counts time including suspend, which is what a meditation
/// timer wants: real wall-clock progress regardless of OS power state.
fn boot_time_now() -> std::time::Duration {
    let mut ts: libc::timespec = unsafe { std::mem::zeroed() };
    let rc = unsafe { libc::clock_gettime(libc::CLOCK_BOOTTIME, &mut ts) };
    debug_assert_eq!(rc, 0, "clock_gettime(CLOCK_BOOTTIME) failed");
    std::time::Duration::new(ts.tv_sec as u64, ts.tv_nsec as u32)
}

/// Per-bell schedule used by the running tick. Built once at the
/// moment the session enters Running (after prep, if any) from the
/// enabled rows of `interval_bells`. Mutated in place each tick:
/// interval bells reroll their next ring after firing; fixed bells
/// flip their `fired` flag.
#[derive(Debug, Clone)]
struct ActiveBell {
    sound: String,
    vibration_pattern_uuid: String,
    signal_mode: crate::db::SignalMode,
    schedule: BellSchedule,
}

#[derive(Debug, Clone)]
enum BellSchedule {
    Interval {
        base_min: u32,
        jitter_pct: u32,
        next_ring_secs: u64,
    },
    Fixed {
        target_secs: u64,
        fired: bool,
    },
}

// ── Per-mode independent state ────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TimerState {
    #[default]
    Idle,
    /// Counting down the silent preparation interval before the
    /// starting bell fires and the actual Timer-mode session begins.
    /// Timer-mode only; Box Breathing skips this entirely.
    Preparing,
    Running,
    /// Countdown reached 0:00 but the user hasn't yet finished —
    /// big-clock readout flips from "remaining" to "elapsed past
    /// zero" (counting up), and Pause becomes Finish + an "Add
    /// MM:SS?" button appears that commits the overtime as part
    /// of the session duration. Interval bells keep firing on
    /// the original session timeline. Stopwatch and Box Breath
    /// don't enter this state — they have no countdown to overshoot.
    Overtime,
    Paused,
    Done,
}

/// Which of the two modes is currently selected. Encapsulates the
/// mode_toggle_group's active-name in a single readable value
/// so callers don't sprinkle `is_active()` checks.
///
/// Within `Timer` mode, the Stopwatch-Mode SwitchRow toggles between
/// counting down to a target and counting up open-endedly — that bit
/// lives on `TimerView::stopwatch_toggle_on`, not in this enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TimerMode {
    #[default]
    Timer,
    Breathing,
    /// Guided meditation — user picks an audio file and the session
    /// length is the file's natural duration. Setup view shows the
    /// guided-files section (Selected row + Open/Import buttons + a
    /// starred-files list + Manage Files button) plus the shared
    /// Label and End Bell rows. Runs through the same hero countdown
    /// pattern as the Timer countdown, with the audio playing in
    /// parallel via gst playbin.
    Guided,
}


// ── GObject impl ──────────────────────────────────────────────────────────────

#[derive(Debug, Default, CompositeTemplate)]
#[template(resource = "/io/github/janekbt/Meditate/ui/timer_view.ui")]
pub struct TimerView {
    // Template children
    #[template_child] pub view_stack:            TemplateChild<gtk::Stack>,
    #[template_child] pub streak_label:          TemplateChild<gtk::Label>,
    #[template_child] pub mode_toggle_group:     TemplateChild<adw::ToggleGroup>,
    #[template_child] pub big_time_label:         TemplateChild<gtk::Label>,
    #[template_child] pub countdown_inputs:       TemplateChild<gtk::Box>,
    #[template_child] pub stopwatch_mode_row:     TemplateChild<adw::SwitchRow>,
    #[template_child] pub presets_section:       TemplateChild<adw::Clamp>,
    #[template_child] pub presets_group:         TemplateChild<adw::PreferencesGroup>,
    #[template_child] pub save_settings_btn:     TemplateChild<gtk::Button>,
    #[template_child] pub manage_presets_btn:    TemplateChild<gtk::Button>,
    #[template_child] pub boxbreath_inputs:       TemplateChild<gtk::Box>,
    #[template_child] pub guided_section:         TemplateChild<adw::Clamp>,
    #[template_child] pub guided_inputs:          TemplateChild<gtk::Box>,
    #[template_child] pub guided_selected_group:  TemplateChild<adw::PreferencesGroup>,
    #[template_child] pub guided_selected_row:    TemplateChild<adw::ActionRow>,
    #[template_child] pub open_file_btn:          TemplateChild<gtk::Button>,
    #[template_child] pub import_file_btn:        TemplateChild<gtk::Button>,
    #[template_child] pub guided_files_group:     TemplateChild<adw::PreferencesGroup>,
    #[template_child] pub manage_guided_files_btn: TemplateChild<gtk::Button>,
    #[template_child] pub phase_tiles_grid:       TemplateChild<gtk::Grid>,
    #[template_child] pub start_btn:             TemplateChild<gtk::Button>,
    #[template_child] pub resume_btn:            TemplateChild<gtk::Button>,
    #[template_child] pub stop_from_pause_btn:   TemplateChild<gtk::Button>,
    #[template_child] pub session_group:          TemplateChild<adw::PreferencesGroup>,
    #[template_child] pub cues_signal_mode_row:    TemplateChild<adw::ActionRow>,
    #[template_child] pub cues_signal_toggle_host: TemplateChild<gtk::Box>,
    #[template_child] pub duration_row:            TemplateChild<adw::ActionRow>,
    #[template_child] pub duration_value_label:    TemplateChild<gtk::Label>,
    #[template_child] pub setup_label_enabled_row: TemplateChild<adw::ExpanderRow>,
    #[template_child] pub setup_label_chooser_row: TemplateChild<adw::ActionRow>,
    #[template_child] pub starting_bell_row:        TemplateChild<adw::ExpanderRow>,
    #[template_child] pub starting_bell_signal_mode_row:    TemplateChild<adw::ActionRow>,
    #[template_child] pub starting_bell_signal_toggle_host: TemplateChild<gtk::Box>,
    #[template_child] pub starting_bell_sound_revealer:     TemplateChild<gtk::Revealer>,
    #[template_child] pub starting_bell_sound_row:  TemplateChild<adw::ActionRow>,
    #[template_child] pub starting_bell_pattern_revealer:   TemplateChild<gtk::Revealer>,
    #[template_child] pub starting_bell_pattern_row:        TemplateChild<adw::ActionRow>,
    #[template_child] pub preparation_time_row:     TemplateChild<adw::ExpanderRow>,
    #[template_child] pub preparation_time_secs_row:TemplateChild<adw::SpinRow>,
    #[template_child] pub interval_bells_enabled_row: TemplateChild<adw::ExpanderRow>,
    #[template_child] pub interval_bells_row:       TemplateChild<adw::ActionRow>,
    #[template_child] pub end_bell_row:            TemplateChild<adw::ExpanderRow>,
    #[template_child] pub end_bell_signal_mode_row:  TemplateChild<adw::ActionRow>,
    #[template_child] pub end_bell_signal_toggle_host: TemplateChild<gtk::Box>,
    #[template_child] pub end_bell_sound_revealer:   TemplateChild<gtk::Revealer>,
    #[template_child] pub end_bell_sound_row:      TemplateChild<adw::ActionRow>,
    #[template_child] pub end_bell_pattern_revealer: TemplateChild<gtk::Revealer>,
    #[template_child] pub end_bell_pattern_row:      TemplateChild<adw::ActionRow>,
    // Vibration UI prototype — see setup_vibration_proto. Throwaway.
    #[template_child] pub boxbreath_phase_section:         TemplateChild<adw::Clamp>,
    #[template_child] pub boxbreath_master_row:           TemplateChild<adw::ExpanderRow>,
    #[template_child] pub boxbreath_phase_in_row:                  TemplateChild<adw::ExpanderRow>,
    #[template_child] pub boxbreath_phase_in_signal_toggle_host:   TemplateChild<gtk::Box>,
    #[template_child] pub boxbreath_phase_in_sound_revealer:       TemplateChild<gtk::Revealer>,
    #[template_child] pub boxbreath_phase_in_sound_row:            TemplateChild<adw::ActionRow>,
    #[template_child] pub boxbreath_phase_in_pattern_revealer:     TemplateChild<gtk::Revealer>,
    #[template_child] pub boxbreath_phase_in_pattern_row:          TemplateChild<adw::ActionRow>,
    #[template_child] pub boxbreath_phase_holdin_row:                  TemplateChild<adw::ExpanderRow>,
    #[template_child] pub boxbreath_phase_holdin_signal_toggle_host:   TemplateChild<gtk::Box>,
    #[template_child] pub boxbreath_phase_holdin_sound_revealer:       TemplateChild<gtk::Revealer>,
    #[template_child] pub boxbreath_phase_holdin_sound_row:            TemplateChild<adw::ActionRow>,
    #[template_child] pub boxbreath_phase_holdin_pattern_revealer:     TemplateChild<gtk::Revealer>,
    #[template_child] pub boxbreath_phase_holdin_pattern_row:          TemplateChild<adw::ActionRow>,
    #[template_child] pub boxbreath_phase_out_row:                  TemplateChild<adw::ExpanderRow>,
    #[template_child] pub boxbreath_phase_out_signal_toggle_host:   TemplateChild<gtk::Box>,
    #[template_child] pub boxbreath_phase_out_sound_revealer:       TemplateChild<gtk::Revealer>,
    #[template_child] pub boxbreath_phase_out_sound_row:            TemplateChild<adw::ActionRow>,
    #[template_child] pub boxbreath_phase_out_pattern_revealer:     TemplateChild<gtk::Revealer>,
    #[template_child] pub boxbreath_phase_out_pattern_row:          TemplateChild<adw::ActionRow>,
    #[template_child] pub boxbreath_phase_holdout_row:                  TemplateChild<adw::ExpanderRow>,
    #[template_child] pub boxbreath_phase_holdout_signal_toggle_host:   TemplateChild<gtk::Box>,
    #[template_child] pub boxbreath_phase_holdout_sound_revealer:       TemplateChild<gtk::Revealer>,
    #[template_child] pub boxbreath_phase_holdout_sound_row:            TemplateChild<adw::ActionRow>,
    #[template_child] pub boxbreath_phase_holdout_pattern_revealer:     TemplateChild<gtk::Revealer>,
    #[template_child] pub boxbreath_phase_holdout_pattern_row:          TemplateChild<adw::ActionRow>,
    #[template_child] pub time_unit_label:        TemplateChild<gtk::Label>,
    #[template_child] pub done_duration_label:   TemplateChild<gtk::Label>,
    #[template_child] pub note_view:             TemplateChild<gtk::TextView>,
    #[template_child] pub note_caption:          TemplateChild<gtk::Label>,
    #[template_child] pub done_label_enabled_row: TemplateChild<adw::ExpanderRow>,
    #[template_child] pub done_label_chooser_row: TemplateChild<adw::ActionRow>,
    #[template_child] pub discard_btn:           TemplateChild<gtk::Button>,
    #[template_child] pub save_btn:              TemplateChild<gtk::Button>,

    // ── Active session state ─────────────────────────────────────────
    // Only one session runs at a time across the three modes.
    timer_state: Cell<TimerState>,
    /// Unix timestamp when the active session started (for DB save).
    session_start_time: Cell<i64>,

    /// Which mode the active tick belongs to. Only meaningful while
    /// tick_source is Some.
    tick_mode: Cell<TimerMode>,

    /// Active glib timeout handle (at most one mode runs at a time).
    tick_source: RefCell<Option<glib::SourceId>>,

    /// Held PatternPlayback handle for the most recent bell or
    /// phase-cue vibration. Replacing it with a new handle drops the
    /// previous one — the Drop impl fires `Vibrate(app_id, [])` to
    /// cancel any pattern still playing — so newest-wins overlap
    /// behaviour is automatic.
    current_vibration: RefCell<Option<crate::vibration::PatternPlayback>>,
    /// Weak ref to the running-page time label for live updates.
    running_label: RefCell<Option<gtk::Label>>,
    /// Refs to the running-page buttons so the Overtime transition
    /// can morph them in place (Pause → Finish, Stop hidden, the
    /// "Add MM:SS?" suffix shown). All three are dropped when the
    /// session ends to release the widgets.
    running_pause_btn: RefCell<Option<gtk::Button>>,
    running_stop_btn: RefCell<Option<gtk::Button>>,
    overtime_add_btn: RefCell<Option<gtk::Button>>,
    /// When `Some`, on_save records this duration instead of the
    /// raw elapsed. Used by the Overtime "Finish" button so the
    /// recorded session is exactly the planned countdown — Add's
    /// path leaves it unset and on_save uses the natural elapsed
    /// (countdown target + overtime).
    final_duration_secs: Cell<Option<u64>>,
    /// True while a label-row update is being applied programmatically
    /// (mode switch, show_done refresh) — suppresses the
    /// enable_expansion_notify / activated callbacks so they don't
    /// re-write the same value back to settings or open a chooser.
    labels_loading: Cell<bool>,
    /// Per-session label pick on the Done page. Set in show_done
    /// from the Setup view's current state, mutable when the user
    /// taps the chooser on Done. Read by on_save. Stored as a
    /// resolved local id (not uuid) since on_save writes label_id
    /// to the session row, and the row is gone (label_id = NULL)
    /// when the toggle is off.
    done_selected_label_id: Cell<Option<i64>>,
    /// Currently-selected countdown duration in seconds, set by preset
    /// chips or the "Custom" dialog. Default 10 min; used as the target
    /// when the user taps Start (and Stopwatch Mode is off).
    countdown_target_secs: Cell<u64>,
    /// Live mirror of the persisted "stopwatch_mode_active" setting and
    /// of `stopwatch_mode_row`'s active state. `true` means count up
    /// from zero with no target; `false` means count down to
    /// `countdown_target_secs`.
    stopwatch_toggle_on: Cell<bool>,
    /// Suppress the SwitchRow's notify::active handler while
    /// `refresh_streak` is loading the persisted setting on visit.
    stopwatch_loading: Cell<bool>,
    /// Suppress notify handlers on the four bell-related rows
    /// (Starting-Bell switch, Bell-Sound combo, Preparation-Time switch,
    /// Preparation-Time SpinRow) while `refresh_streak` is loading their
    /// persisted values on visit. One flag covers all four because they
    /// load atomically in the same DB roundtrip.
    bells_loading: Cell<bool>,
    /// Starred-preset rows currently attached to `presets_group`,
    /// paired with their preset uuid. Tracked so the list can be
    /// rebuilt cleanly on mode switch / sync update without leaking
    /// rows from the previous mode.
    starred_preset_rows: RefCell<Vec<(adw::ActionRow, String)>>,
    /// The most-recently-shown apply toast. Tapping a second preset
    /// dismisses the prior toast immediately so the new one renders
    /// without waiting for the queue — otherwise the user has to
    /// wait through the full timeout before seeing the next "applied"
    /// confirmation. The Undo affordance on the dismissed toast is
    /// lost, but that's the right trade: the user has just chosen
    /// to apply a different preset, so undoing the previous one no
    /// longer makes sense.
    current_apply_toast: RefCell<Option<adw::Toast>>,

    // ── Guided meditation state ──────────────────────────────────────
    /// Transient "Open File" pick — set when the user picks a file via
    /// the file dialog, cleared when they tap Import File (which
    /// promotes it into the library) or pick a starred row from the
    /// list. Drives the hero countdown's target during a guided run.
    guided_pick: RefCell<Option<crate::guided::GuidedFilePick>>,
    /// UUID of the currently-selected library row, when the user has
    /// tapped a row in the starred list. `None` for transient picks
    /// AND for the empty state. The session-save path reads this so
    /// per-file stats can resolve later.
    guided_selected_uuid: RefCell<Option<String>>,
    /// Starred guided-file rows currently attached to the home-list
    /// group. Same shape as `starred_preset_rows` so the rebuild path
    /// can drain and re-add cleanly without leaking rows.
    starred_guided_rows: RefCell<Vec<(adw::ActionRow, String)>>,
    /// Active gst playbin instance during a Guided session. Set by
    /// `start_session`'s Guided arm, paused/resumed alongside the
    /// hero countdown, and torn down (Drop runs set_state(Null) +
    /// removes the bus signal-watch) on every session-end path.
    guided_playback: RefCell<Option<crate::guided::GuidedPlayback>>,

    // ── Breathing (Box Breath) state ─────────────────────────────────
    /// Four phase durations. Defaults 4/4/4/4 (classic box breathing).
    pub(super) breathing_pattern: Cell<BreathPattern>,
    /// Total session length in minutes, drives the hero label and the
    /// cycle-aligned stop condition.
    breathing_session_secs: Cell<u32>,
    /// Per-phase stepper buttons + value labels, indexed 0..=3 (In, HoldIn,
    /// Out, HoldOut). Kept so `refresh_phase_tiles` can update the displayed
    /// values without rebuilding the DOM.
    phase_value_labels: RefCell<[Option<gtk::Label>; 4]>,
    /// Suppress persistence side-effects while `load_breathing_settings`
    /// is setting initial values from the DB.
    breathing_populating: Cell<bool>,
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
    /// Timer-mode preparation-interval state. `prep_stopwatch` is
    /// `Some` while we're in (or paused-from) the Preparing state and
    /// gets cleared at the prep→Running transition. The tick reads
    /// elapsed against `prep_target` to decide when to play the bell
    /// and swap in the real countdown/stopwatch core.
    prep_stopwatch: RefCell<Option<CoreStopwatch>>,
    prep_target: Cell<std::time::Duration>,
    /// Snapshot of every interval/fixed bell that should fire during
    /// the current Timer-mode session. Built once at the moment the
    /// session enters Running (either directly from on_start with no
    /// prep, or from transition_prep_to_running). Per-tick check
    /// flips fired flags / reschedules the next ring on this in-
    /// memory state — the DB rows + their enabled flags are read
    /// only at session start, so toggling a bell mid-session is a
    /// no-op for that session and takes effect next time. Cleared on
    /// stop / done / reset.
    active_bells: RefCell<Vec<ActiveBell>>,
    /// Per-process xorshift state seeded once from the wall clock.
    /// The first jittered ring of the first session may roll a tiny
    /// non-uniform value; subsequent rolls are well-distributed.
    /// Lazy init (Cell<u64> defaulting to 0 means "not yet seeded").
    bell_rng_state: Cell<u64>,
    /// Boot-time anchor at session start. Suspend-resilient (see boot_time_now).
    start_boot_time: Cell<Option<std::time::Duration>>,
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
        self.breathing_session_secs.set(5 * 60);
        self.setup_buttons();
        self.build_breathing_setup();
        self.configure_preparation_time_secs_row();
        self.setup_boxbreath_phase_cues();

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

        // Mode toggle — Adw.ToggleGroup is one-of-N, so one
        // active-name change per switch. Single notify handler.
        self.mode_toggle_group.connect_active_name_notify(glib::clone!(
            #[weak(rename_to = this)] obj,
            move |_| {
                this.imp().on_mode_switched();
            }
        ));

        // Stopwatch-Mode SwitchRow: persist state, mirror on the cell,
        // refresh the hero label + preset sensitivity. The
        // stopwatch_loading guard suppresses persistence while
        // refresh_streak is restoring the value on visit.
        self.stopwatch_mode_row.connect_active_notify(glib::clone!(
            #[weak(rename_to = this)] obj,
            move |row| {
                let imp = this.imp();
                if imp.stopwatch_loading.get() { return; }
                let on = row.is_active();
                imp.stopwatch_toggle_on.set(on);
                if let Some(app) = imp.get_app() {
                    app.with_db_mut(|db| {
                        let _ = db.set_setting(
                            "stopwatch_mode_active",
                            if on { "true" } else { "false" },
                        );
                    });
                }
                imp.refresh_stopwatch_dependent_ui();
            }
        ));

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

        // ── Duration row: tap opens the H:M dialog ──────────────────
        // The only entry point for setting an ad-hoc Timer duration
        // (one not in any saved preset). Greyed out when stopwatch
        // mode is on — the planned-duration concept doesn't apply.
        self.duration_row.connect_activated(glib::clone!(
            #[weak(rename_to = this)] obj,
            move |_| this.imp().show_custom_time_dialog(),
        ));

        // ── Save Settings / Manage Presets buttons ──────────────────
        // Both push the same chooser NavigationPage — the variant
        // (Save vs Manage) determines whether the synthetic "Create
        // new preset…" row appears, whether row taps trigger an
        // override-confirmation dialog, and whether rename/delete
        // suffix buttons render.
        self.save_settings_btn.connect_clicked(glib::clone!(
            #[weak(rename_to = this)] obj,
            move |_| {
                let imp = this.imp();
                let Some(app) = imp.get_app() else { return; };
                let Some(window) = this.root()
                    .and_downcast::<crate::window::MeditateWindow>()
                else { return; };
                let session_mode = match imp.current_mode() {
                    TimerMode::Timer     => crate::db::SessionMode::Timer,
                    TimerMode::Breathing => crate::db::SessionMode::BoxBreath,
                    // Guided mode hides this button — pre-empt anyway
                    // so a future flag-flip can't accidentally drive
                    // a Save Preset flow against a non-preset mode.
                    TimerMode::Guided    => return,
                };
                let snapshot = imp.snapshot_current_setup();
                let this_for_changed = this.clone();
                window.push_presets_chooser(
                    &app,
                    session_mode,
                    crate::presets::ChooserMode::Save { snapshot },
                    move || this_for_changed.imp().rebuild_starred_presets_list(),
                );
            },
        ));
        self.manage_presets_btn.connect_clicked(glib::clone!(
            #[weak(rename_to = this)] obj,
            move |_| {
                let imp = this.imp();
                let Some(app) = imp.get_app() else { return; };
                let Some(window) = this.root()
                    .and_downcast::<crate::window::MeditateWindow>()
                else { return; };
                let session_mode = match imp.current_mode() {
                    TimerMode::Timer     => crate::db::SessionMode::Timer,
                    TimerMode::Breathing => crate::db::SessionMode::BoxBreath,
                    TimerMode::Guided    => return,
                };
                let this_for_changed = this.clone();
                window.push_presets_chooser(
                    &app,
                    session_mode,
                    crate::presets::ChooserMode::Manage,
                    move || this_for_changed.imp().rebuild_starred_presets_list(),
                );
            },
        ));

        // ── Guided-mode buttons ─────────────────────────────────────
        // Open File: pop the gtk::FileDialog, on success populate the
        // Selected row + hero countdown, ungrey Import.
        self.open_file_btn.connect_clicked(glib::clone!(
            #[weak(rename_to = this)] obj,
            move |_| {
                let Some(window) = this.root()
                    .and_then(|r| r.downcast::<gtk::Window>().ok())
                else { return; };
                let this_for_pick = this.clone();
                crate::guided::pick_file_for_open(&window, move |pick| {
                    let imp = this_for_pick.imp();
                    *imp.guided_pick.borrow_mut() = Some(pick);
                    // Transient pick — clear any prior starred-row uuid
                    // so the session-save path logs guided_file_uuid=None.
                    *imp.guided_selected_uuid.borrow_mut() = None;
                    imp.refresh_guided_selected_row();
                    imp.refresh_hero_for_idle();
                });
            },
        ));

        // Import File: take the current transient pick, run the name
        // dialog → transcode → DB insert pipeline, and on success
        // promote the row into the starred list. The button is greyed
        // when there's no transient pick to import (toggled in
        // refresh_guided_selected_row).
        self.import_file_btn.connect_clicked(glib::clone!(
            #[weak(rename_to = this)] obj,
            move |_| {
                let imp = this.imp();
                let Some(app) = imp.get_app() else { return; };
                let Some(window) = this.root()
                    .and_then(|r| r.downcast::<gtk::Window>().ok())
                else { return; };
                let Some(pick) = imp.guided_pick.borrow().clone() else { return; };
                let this_for_done = this.clone();
                crate::guided::import_picked_file(
                    &window,
                    &app,
                    pick,
                    move |row| {
                        // Promote the freshly-imported row into the
                        // Selected slot — it stays as the active pick
                        // (now with a uuid attached), so the user can
                        // hit Start without re-tapping anything.
                        let imp = this_for_done.imp();
                        *imp.guided_selected_uuid.borrow_mut() = Some(row.uuid.clone());
                        *imp.guided_pick.borrow_mut() = Some(crate::guided::GuidedFilePick {
                            display_name: row.name.clone(),
                            source_path: std::path::PathBuf::from(&row.file_path),
                            duration_secs: row.duration_secs,
                        });
                        imp.rebuild_starred_guided_list();
                        imp.refresh_guided_selected_row();
                        imp.refresh_hero_for_idle();
                    },
                );
            },
        ));

        // Manage Files: push the chooser NavigationPage. On every
        // change inside (rename / star toggle / delete / import),
        // refresh the home-list so the Setup view reflects state.
        self.manage_guided_files_btn.connect_clicked(glib::clone!(
            #[weak(rename_to = this)] obj,
            move |_| {
                let imp = this.imp();
                let Some(app) = imp.get_app() else { return; };
                let Some(window) = this.root()
                    .and_downcast::<crate::window::MeditateWindow>()
                else { return; };
                let this_for_changed = this.clone();
                window.push_guided_files_chooser(
                    &app,
                    move || this_for_changed.imp().rebuild_starred_guided_list(),
                );
            },
        ));

        // ── Done-page label expander ────────────────────────────────
        // Per-session pick. Initialized in show_done from the Setup
        // view's currently-active label. Toggling here doesn't write
        // any persistent setting — the choice rides with the session.
        self.done_label_enabled_row.connect_enable_expansion_notify(glib::clone!(
            #[weak(rename_to = this)] obj,
            move |row| {
                let imp = this.imp();
                if imp.labels_loading.get() { return; }
                if !row.enables_expansion() {
                    imp.done_selected_label_id.set(None);
                    imp.refresh_done_label_chooser_subtitle();
                    return;
                }
                // Toggling on: if no per-session pick is set yet,
                // resolve the mode-default and adopt it.
                if imp.done_selected_label_id.get().is_none() {
                    let id = imp.resolve_label_for_mode(imp.current_mode())
                        .map(|l| l.id);
                    imp.done_selected_label_id.set(id);
                }
                imp.refresh_done_label_chooser_subtitle();
            }
        ));
        self.done_label_chooser_row.connect_activated(glib::clone!(
            #[weak(rename_to = this)] obj,
            move |_| {
                let imp = this.imp();
                let Some(app) = imp.get_app() else { return; };
                let Some(window) = this.root()
                    .and_then(|r| r.downcast::<crate::window::MeditateWindow>().ok())
                else { return; };
                let current_id = imp.done_selected_label_id.get();
                let this_for_pick = this.clone();
                window.push_label_chooser(&app, current_id, move |label| {
                    let imp2 = this_for_pick.imp();
                    imp2.done_selected_label_id.set(Some(label.id));
                    imp2.refresh_done_label_chooser_subtitle();
                });
            }
        ));

        // End Bell master toggle — gates whether the bell plays at the
        // end of a session. Persists end_bell_active.
        self.end_bell_row.connect_enable_expansion_notify(glib::clone!(
            #[weak(rename_to = this)] obj,
            move |row| {
                let imp = this.imp();
                if imp.bells_loading.get() { return; }
                let on = row.enables_expansion();
                if let Some(app) = imp.get_app() {
                    app.with_db_mut(|db| {
                        let _ = db.set_setting(
                            "end_bell_active",
                            if on { "true" } else { "false" },
                        );
                    });
                    // Re-warm the preload so the next play_end_bell()
                    // either has a MediaFile ready (active=true) or
                    // doesn't waste cycles trying to reuse a stale one.
                    crate::sound::preload_end_bell(&app);
                }
            }
        ));

        // The Bell Sound + Pattern rows are wrapped in Gtk.Revealers
        // for the slide-down animation when the user flips Sound /
        // Vibration / Both. That wrapping breaks the listbox row-
        // activated signal chain that AdwActionRow.connect_activated
        // normally hooks — so we re-emit `activated` from an explicit
        // GestureClick on each wrapped row.
        attach_revealer_row_click(&self.end_bell_sound_row);
        attach_revealer_row_click(&self.end_bell_pattern_row);
        attach_revealer_row_click(&self.starting_bell_sound_row);
        attach_revealer_row_click(&self.starting_bell_pattern_row);

        // End Bell sound row — tap pushes the bell-sound chooser.
        self.end_bell_sound_row.connect_activated(glib::clone!(
            #[weak(rename_to = this)] obj,
            move |_| {
                let imp = this.imp();
                let Some(app) = imp.get_app() else { return; };
                let Some(window) = this.root()
                    .and_then(|r| r.downcast::<crate::window::MeditateWindow>().ok())
                else { return; };
                let current = app
                    .with_db(|db| db.get_setting("end_bell_sound", crate::db::BUNDLED_BOWL_UUID))
                    .and_then(|r| r.ok());
                let app_for_pick = app.clone();
                let this_for_pick = this.clone();
                window.push_sound_chooser(
                    &app,
                    crate::db::BellSoundCategory::General,
                    current,
                    move |uuid| {
                        app_for_pick.with_db_mut(|db| db.set_setting("end_bell_sound", &uuid));
                        crate::sound::preload_end_bell(&app_for_pick);
                        this_for_pick.imp().refresh_end_bell_sound_subtitle();
                    },
                );
            }
        ));

        // End Bell pattern row — tap pushes the vibration-pattern
        // chooser. Persists end_bell_pattern setting on pick.
        self.end_bell_pattern_row.connect_activated(glib::clone!(
            #[weak(rename_to = this)] obj,
            move |_| {
                let imp = this.imp();
                let Some(app) = imp.get_app() else { return; };
                let Some(window) = this.root()
                    .and_then(|r| r.downcast::<crate::window::MeditateWindow>().ok())
                else { return; };
                let current = app
                    .with_db(|db| db.get_setting(
                        "end_bell_pattern",
                        crate::db::BUNDLED_PATTERN_PULSE_UUID,
                    ))
                    .and_then(|r| r.ok());
                let app_for_pick = app.clone();
                let this_for_pick = this.clone();
                window.push_vibrations_chooser(&app, current, move |uuid| {
                    app_for_pick.with_db_mut(|db| db.set_setting("end_bell_pattern", &uuid));
                    this_for_pick.imp().refresh_end_bell_pattern_subtitle();
                });
            }
        ));

        // End Bell signal-mode AdwToggleGroup — built in Rust because
        // Adw.Toggle isn't ergonomic from Blueprint without a matching
        // .ui parser version. Toggle changes persist end_bell_signal_mode
        // and reveal/hide the Bell Sound + Pattern rows accordingly.
        self.setup_end_bell_signal_mode_toggle();
        self.setup_cues_signal_mode_toggle();

        // ── Setup-page label expander ───────────────────────────────
        // Master toggle persists `label_active_<mode>`; the inner
        // chooser-row pushes the label chooser and persists the
        // selected uuid per-mode.
        self.setup_label_enabled_row.connect_enable_expansion_notify(glib::clone!(
            #[weak(rename_to = this)] obj,
            move |row| {
                let imp = this.imp();
                if imp.labels_loading.get() { return; }
                let on = row.enables_expansion();
                let mode = imp.current_mode();
                imp.persist_label_active_for_mode(mode, on);
                if on && imp.persisted_label_uuid_for_mode(mode).is_none() {
                    // First time the toggle flips on for this mode:
                    // adopt the mode-default uuid so subsequent reads
                    // resolve cleanly.
                    let default = imp.mode_default_label_uuid(mode);
                    imp.persist_label_uuid_for_mode(mode, default);
                }
                imp.refresh_setup_label_chooser_subtitle();
            }
        ));
        self.setup_label_chooser_row.connect_activated(glib::clone!(
            #[weak(rename_to = this)] obj,
            move |_| {
                let imp = this.imp();
                let Some(app) = imp.get_app() else { return; };
                let Some(window) = this.root()
                    .and_then(|r| r.downcast::<crate::window::MeditateWindow>().ok())
                else { return; };
                let mode = imp.current_mode();
                let current_id = imp.resolve_label_for_mode(mode).map(|l| l.id);
                let this_for_pick = this.clone();
                window.push_label_chooser(&app, current_id, move |label| {
                    let imp2 = this_for_pick.imp();
                    let mode = imp2.current_mode();
                    imp2.persist_label_uuid_for_mode(mode, &label.uuid);
                    imp2.refresh_setup_label_chooser_subtitle();
                });
            }
        ));

        // ── Starting Bell expander ───────────────────────────────────
        // Adw.ExpanderRow drives the slide-down animation itself when
        // enable-expansion flips. The bells_loading guard suppresses
        // persistence while `refresh_streak` is restoring the saved
        // state on visit, so the read-back can't masquerade as a user
        // toggle and re-write the same value.
        self.starting_bell_row.connect_enable_expansion_notify(glib::clone!(
            #[weak(rename_to = this)] obj,
            move |row| {
                let imp = this.imp();
                if imp.bells_loading.get() { return; }
                let on = row.enables_expansion();
                if let Some(app) = imp.get_app() {
                    app.with_db_mut(|db| {
                        let _ = db.set_setting(
                            "starting_bell_active",
                            if on { "true" } else { "false" },
                        );
                    });
                }
            }
        ));

        // Starting-Bell sound row — tap pushes the bell-sound chooser.
        // "No bell" is still handled by the parent ExpanderRow's
        // master toggle; the chooser only lists real sounds.
        self.starting_bell_sound_row.connect_activated(glib::clone!(
            #[weak(rename_to = this)] obj,
            move |_| {
                let imp = this.imp();
                let Some(app) = imp.get_app() else { return; };
                let Some(window) = this.root()
                    .and_then(|r| r.downcast::<crate::window::MeditateWindow>().ok())
                else { return; };
                let current = app
                    .with_db(|db| db.get_setting("starting_bell_sound", crate::db::BUNDLED_BOWL_UUID))
                    .and_then(|r| r.ok());
                let app_for_pick = app.clone();
                let this_for_pick = this.clone();
                window.push_sound_chooser(
                    &app,
                    crate::db::BellSoundCategory::General,
                    current,
                    move |uuid| {
                        app_for_pick.with_db_mut(|db| db.set_setting("starting_bell_sound", &uuid));
                        this_for_pick.imp().refresh_starting_bell_sound_subtitle();
                    },
                );
            }
        ));

        // Starting Bell pattern row — drills into the vibrations chooser.
        self.starting_bell_pattern_row.connect_activated(glib::clone!(
            #[weak(rename_to = this)] obj,
            move |_| {
                let imp = this.imp();
                let Some(app) = imp.get_app() else { return; };
                let Some(window) = this.root()
                    .and_then(|r| r.downcast::<crate::window::MeditateWindow>().ok())
                else { return; };
                let current = app
                    .with_db(|db| db.get_setting(
                        "starting_bell_pattern",
                        crate::db::BUNDLED_PATTERN_PULSE_UUID,
                    ))
                    .and_then(|r| r.ok());
                let app_for_pick = app.clone();
                let this_for_pick = this.clone();
                window.push_vibrations_chooser(&app, current, move |uuid| {
                    app_for_pick.with_db_mut(|db| db.set_setting("starting_bell_pattern", &uuid));
                    this_for_pick.imp().refresh_starting_bell_pattern_subtitle();
                });
            }
        ));

        // Starting Bell signal-mode AdwToggleGroup.
        self.setup_starting_bell_signal_mode_toggle();

        // Preparation Time expander — nested inside the Starting Bell
        // expander, animates the seconds spin in and out the same way.
        self.preparation_time_row.connect_enable_expansion_notify(glib::clone!(
            #[weak(rename_to = this)] obj,
            move |row| {
                let imp = this.imp();
                if imp.bells_loading.get() { return; }
                let on = row.enables_expansion();
                if let Some(app) = imp.get_app() {
                    app.with_db_mut(|db| {
                        let _ = db.set_setting(
                            "preparation_time_active",
                            if on { "true" } else { "false" },
                        );
                    });
                }
            }
        ));

        // Interval Bells master toggle — same persistence + bells_loading
        // guard pattern as Starting Bell. The ExpanderRow's switch gates
        // whether the running tick fires interval bells at all (B.3.4
        // checks `interval_bells_active` before iterating the library).
        self.interval_bells_enabled_row.connect_enable_expansion_notify(glib::clone!(
            #[weak(rename_to = this)] obj,
            move |row| {
                let imp = this.imp();
                if imp.bells_loading.get() { return; }
                let on = row.enables_expansion();
                if let Some(app) = imp.get_app() {
                    app.with_db_mut(|db| {
                        let _ = db.set_setting(
                            "interval_bells_active",
                            if on { "true" } else { "false" },
                        );
                    });
                }
            }
        ));

        // "Manage Bells" row — tap pushes the bell-library NavigationPage.
        self.interval_bells_row.connect_activated(glib::clone!(
            #[weak(rename_to = this)] obj,
            move |_| {
                let imp = this.imp();
                let Some(app) = imp.get_app() else { return; };
                let Some(window) = this.root()
                    .and_then(|r| r.downcast::<crate::window::MeditateWindow>().ok())
                else { return; };
                window.push_bells_page(&app);
            }
        ));

        // Preparation Time SpinRow — value persisted as a plain integer
        // string, parsed on read via `meditate_core::format::parse_prep_secs`
        // so out-of-range or garbage values can never crash the shell.
        self.preparation_time_secs_row.connect_notify_local(
            Some("value"),
            glib::clone!(
                #[weak(rename_to = this)] obj,
                move |row, _| {
                    let imp = this.imp();
                    if imp.bells_loading.get() { return; }
                    let v = row.value().round() as i64;
                    let v = v.clamp(
                        meditate_core::format::PREP_SECS_MIN as i64,
                        meditate_core::format::PREP_SECS_MAX as i64,
                    );
                    if let Some(app) = imp.get_app() {
                        app.with_db_mut(|db| {
                            let _ = db.set_setting("preparation_time_secs", &v.to_string());
                        });
                    }
                }
            ),
        );
    }

    /// Set the SpinRow's adjustment to the bell prep-time bounds. Called
    /// once at construction; the actual current value is restored from
    /// the DB by `refresh_streak`.
    fn configure_preparation_time_secs_row(&self) {
        let adj = gtk::Adjustment::new(
            meditate_core::format::PREP_SECS_DEFAULT as f64,
            meditate_core::format::PREP_SECS_MIN as f64,
            meditate_core::format::PREP_SECS_MAX as f64,
            5.0, 15.0, 0.0,
        );
        self.preparation_time_secs_row.set_adjustment(Some(&adj));
    }

    /// Build the Starting Bell's Sound / Vibration / Both selector at
    /// construction time. Mirrors the End Bell setup — see
    /// `setup_end_bell_signal_mode_toggle` for the construction-time /
    /// refresh-time split rationale.
    fn setup_starting_bell_signal_mode_toggle(&self) {
        let obj = self.obj();
        build_signal_mode_toggle_widget(
            &self.starting_bell_signal_toggle_host,
            &self.starting_bell_sound_revealer,
            &self.starting_bell_pattern_revealer,
            "starting_bell_signal_mode",
            glib::clone!(
                #[weak] obj,
                #[upgrade_or] None,
                move || obj.imp().get_app()
            ),
        );
    }

    /// Apply the saved starting_bell_signal_mode + capability gating.
    pub(crate) fn refresh_starting_bell_signal_mode_state(&self) {
        let Some(app) = self.get_app() else { return; };
        apply_signal_mode_state(
            &self.starting_bell_signal_toggle_host,
            &self.starting_bell_sound_revealer,
            &self.starting_bell_pattern_revealer,
            &app,
            "starting_bell_signal_mode",
        );
    }

    /// Build the End Bell's Sound / Vibration / Both selector at
    /// construction time. The widget structure goes in synchronously;
    /// the saved-state load + capability gating run later from
    /// `refresh_end_bell_signal_mode_state` once the widget is
    /// attached and `get_app()` resolves.
    fn setup_end_bell_signal_mode_toggle(&self) {
        let obj = self.obj();
        build_signal_mode_toggle_widget(
            &self.end_bell_signal_toggle_host,
            &self.end_bell_sound_revealer,
            &self.end_bell_pattern_revealer,
            "end_bell_signal_mode",
            glib::clone!(
                #[weak] obj,
                #[upgrade_or] None,
                move || obj.imp().get_app()
            ),
        );
    }

    /// Apply the saved end_bell_signal_mode + capability gating to
    /// the toggle group. Called from refresh-on-visit once the
    /// widget is attached and `get_app()` resolves.
    pub(crate) fn refresh_end_bell_signal_mode_state(&self) {
        let Some(app) = self.get_app() else { return; };
        apply_signal_mode_state(
            &self.end_bell_signal_toggle_host,
            &self.end_bell_sound_revealer,
            &self.end_bell_pattern_revealer,
            &app,
            "end_bell_signal_mode",
        );
    }

    /// Build the per-mode Cues toggle (Sound / Vibration / Both) at
    /// the top of the Session group. Persists to whichever mode's
    /// signal-mode setting is current at click time. State load +
    /// capability gating happen later from
    /// `refresh_cues_signal_mode_state` once the widget is attached.
    fn setup_cues_signal_mode_toggle(&self) {
        let obj = self.obj();
        build_per_mode_signal_toggle_widget(
            &self.cues_signal_toggle_host,
            glib::clone!(
                #[weak] obj,
                #[upgrade_or] None,
                move || obj.imp().get_app()
            ),
            glib::clone!(
                #[weak] obj,
                #[upgrade_or] TimerMode::Timer,
                move || obj.imp().current_mode()
            ),
        );
    }

    /// Apply the saved per-mode signal_mode + capability gating to
    /// the Cues toggle. Reads the setting key matching `current_mode()`,
    /// so this is also called from `on_mode_switched` to sync the
    /// displayed value when the user flips between modes.
    pub(crate) fn refresh_cues_signal_mode_state(&self) {
        let Some(app) = self.get_app() else { return; };
        let Some(toggle_group) =
            first_toggle_group_in(&self.cues_signal_toggle_host)
        else { return; };
        if !app.has_haptic() {
            if let Some(t) = toggle_group.toggle_by_name("vibration") {
                t.set_enabled(false);
            }
            if let Some(t) = toggle_group.toggle_by_name("both") {
                t.set_enabled(false);
            }
        }
        let setting_key = setting_key_for_mode(self.current_mode());
        let saved = app
            .with_db(|db| db.get_setting(setting_key, "both"))
            .and_then(|r| r.ok())
            .unwrap_or_else(|| "both".to_string());
        let initial = if !app.has_haptic() {
            "sound"
        } else {
            match saved.as_str() {
                "sound"     => "sound",
                "vibration" => "vibration",
                _           => "both",
            }
        };
        // Set populating flag so the active-name notify handler
        // doesn't write the just-loaded value back to the DB.
        self.bells_loading.set(true);
        toggle_group.set_active_name(Some(initial));
        self.bells_loading.set(false);
    }

    /// Throwaway: build the Sound / Vibration / Both AdwToggleGroup
    /// Box Breath phase-vibrations prototype only — Start / End bell
    /// prototypes graduated in step 6. The outer expander's
    /// show-enable-switch handles reveal/hide of the four nested
    /// phase rows for free; nothing here actually wires up — these
    /// `let _` markers just signal that the template children are
    /// Wire all Box Breath phase-cue widgetry: master expander +
    /// four phase expanders + the per-phase Sound/Vibration/Both
    /// toggle groups, Bell Sound and Pattern click handlers, and the
    /// click-gesture workarounds for the Revealer-wrapped rows. State
    /// load + capability gating run later from
    /// `refresh_boxbreath_phase_state`.
    fn setup_boxbreath_phase_cues(&self) {
        let obj = self.obj();

        // Master "Cues during phases" enable-switch. Persists to
        // boxbreath_cues_active. We use enable_expansion notify
        // (not the row's expansion state itself) so the user's
        // toggling reads as on/off, not collapse/expand.
        self.boxbreath_master_row.connect_enable_expansion_notify(glib::clone!(
            #[weak(rename_to = this)] obj,
            move |row| {
                let imp = this.imp();
                if imp.bells_loading.get() { return; }
                let on = row.enables_expansion();
                if let Some(app) = imp.get_app() {
                    app.with_db_mut(|db| db.set_setting(
                        "boxbreath_cues_active",
                        if on { "true" } else { "false" },
                    ));
                }
            }
        ));

        // Per-phase wiring. Each phase has identical structure but
        // distinct template-children — call a small helper four times.
        use crate::db::BoxBreathPhaseId as P;
        self.wire_boxbreath_phase(
            P::In,
            &self.boxbreath_phase_in_row,
            &self.boxbreath_phase_in_signal_toggle_host,
            &self.boxbreath_phase_in_sound_revealer,
            &self.boxbreath_phase_in_sound_row,
            &self.boxbreath_phase_in_pattern_revealer,
            &self.boxbreath_phase_in_pattern_row,
        );
        self.wire_boxbreath_phase(
            P::HoldIn,
            &self.boxbreath_phase_holdin_row,
            &self.boxbreath_phase_holdin_signal_toggle_host,
            &self.boxbreath_phase_holdin_sound_revealer,
            &self.boxbreath_phase_holdin_sound_row,
            &self.boxbreath_phase_holdin_pattern_revealer,
            &self.boxbreath_phase_holdin_pattern_row,
        );
        self.wire_boxbreath_phase(
            P::Out,
            &self.boxbreath_phase_out_row,
            &self.boxbreath_phase_out_signal_toggle_host,
            &self.boxbreath_phase_out_sound_revealer,
            &self.boxbreath_phase_out_sound_row,
            &self.boxbreath_phase_out_pattern_revealer,
            &self.boxbreath_phase_out_pattern_row,
        );
        self.wire_boxbreath_phase(
            P::HoldOut,
            &self.boxbreath_phase_holdout_row,
            &self.boxbreath_phase_holdout_signal_toggle_host,
            &self.boxbreath_phase_holdout_sound_revealer,
            &self.boxbreath_phase_holdout_sound_row,
            &self.boxbreath_phase_holdout_pattern_revealer,
            &self.boxbreath_phase_holdout_pattern_row,
        );
    }

    /// Wire one Box Breath phase row's interactive widgets:
    ///   * The phase's enable-switch persists to its row's `enabled`
    ///     column via set_box_breath_phase.
    ///   * The Sound/Vibration/Both toggle group persists to the
    ///     row's `signal_mode` column + reveals the right config rows.
    ///   * Bell Sound row pushes the bell-sound chooser
    ///     (BellSoundCategory::BoxBreath); on pick, persists
    ///     `sound_uuid` and refreshes subtitles.
    ///   * Pattern row pushes the vibration-pattern chooser; on pick,
    ///     persists `pattern_uuid` and refreshes subtitles.
    ///   * GestureClicks on the wrapped rows re-fire `activated` so
    ///     the listbox-row-activated chain works through the Revealer.
    fn wire_boxbreath_phase(
        &self,
        phase: crate::db::BoxBreathPhaseId,
        phase_row: &adw::ExpanderRow,
        toggle_host: &gtk::Box,
        sound_revealer: &gtk::Revealer,
        sound_row: &adw::ActionRow,
        pattern_revealer: &gtk::Revealer,
        pattern_row: &adw::ActionRow,
    ) {
        let obj = self.obj();

        // Phase enable-switch persists to the row's enabled column.
        phase_row.connect_enable_expansion_notify(glib::clone!(
            #[weak(rename_to = this)] obj,
            move |row| {
                let imp = this.imp();
                if imp.bells_loading.get() { return; }
                let on = row.enables_expansion();
                if let Some(app) = imp.get_app() {
                    if let Some(p) = app
                        .with_db(|db| db.get_box_breath_phase(phase))
                        .and_then(|r| r.ok())
                        .flatten()
                    {
                        app.with_db_mut(|db| db.set_box_breath_phase(
                            phase, on, p.signal_mode, &p.sound_uuid, &p.pattern_uuid,
                        ));
                    }
                }
            }
        ));

        // Re-emit `activated` on the wrapped rows.
        attach_revealer_row_click(sound_row);
        attach_revealer_row_click(pattern_row);

        // Bell Sound row -> push sound chooser (BoxBreath category).
        let phase_for_sound = phase;
        sound_row.connect_activated(glib::clone!(
            #[weak(rename_to = this)] obj,
            move |_| {
                let imp = this.imp();
                let Some(app) = imp.get_app() else { return; };
                let Some(window) = this.root()
                    .and_then(|r| r.downcast::<crate::window::MeditateWindow>().ok())
                else { return; };
                let p = match app
                    .with_db(|db| db.get_box_breath_phase(phase_for_sound))
                    .and_then(|r| r.ok())
                    .flatten()
                {
                    Some(p) => p,
                    None => return,
                };
                let app_for_pick = app.clone();
                let this_for_pick = this.clone();
                let p_for_pick = p.clone();
                window.push_sound_chooser(
                    &app,
                    crate::db::BellSoundCategory::BoxBreath,
                    Some(p.sound_uuid.clone()),
                    move |uuid| {
                        app_for_pick.with_db_mut(|db| db.set_box_breath_phase(
                            phase_for_sound,
                            p_for_pick.enabled,
                            p_for_pick.signal_mode,
                            &uuid,
                            &p_for_pick.pattern_uuid,
                        ));
                        this_for_pick.imp().refresh_boxbreath_phase_subtitles(phase_for_sound);
                    },
                );
            }
        ));

        // Pattern row -> push vibration-pattern chooser.
        let phase_for_pattern = phase;
        pattern_row.connect_activated(glib::clone!(
            #[weak(rename_to = this)] obj,
            move |_| {
                let imp = this.imp();
                let Some(app) = imp.get_app() else { return; };
                let Some(window) = this.root()
                    .and_then(|r| r.downcast::<crate::window::MeditateWindow>().ok())
                else { return; };
                let p = match app
                    .with_db(|db| db.get_box_breath_phase(phase_for_pattern))
                    .and_then(|r| r.ok())
                    .flatten()
                {
                    Some(p) => p,
                    None => return,
                };
                let app_for_pick = app.clone();
                let this_for_pick = this.clone();
                let p_for_pick = p.clone();
                window.push_vibrations_chooser(
                    &app,
                    Some(p.pattern_uuid.clone()),
                    move |uuid| {
                        app_for_pick.with_db_mut(|db| db.set_box_breath_phase(
                            phase_for_pattern,
                            p_for_pick.enabled,
                            p_for_pick.signal_mode,
                            &p_for_pick.sound_uuid,
                            &uuid,
                        ));
                        this_for_pick.imp().refresh_boxbreath_phase_subtitles(phase_for_pattern);
                    },
                );
            }
        ));

        // Sound/Vibration/Both AdwToggleGroup. Built imperatively
        // (no app at construction time); persistence resolves app
        // lazily and writes through set_box_breath_phase.
        build_phase_signal_mode_toggle_widget(
            toggle_host,
            sound_revealer,
            pattern_revealer,
            phase,
            glib::clone!(
                #[weak] obj,
                #[upgrade_or] None,
                move || obj.imp().get_app()
            ),
        );
    }

    /// Apply the saved state + capability gating to all four phase
    /// rows + the master toggle. Called from refresh-on-visit once
    /// the widget is attached.
    pub(crate) fn refresh_boxbreath_phase_state(&self) {
        let Some(app) = self.get_app() else { return; };

        self.bells_loading.set(true);

        // Master row.
        let master_on = app
            .with_db(|db| db.get_setting("boxbreath_cues_active", "false"))
            .and_then(|r| r.ok())
            .map(|s| s == "true")
            .unwrap_or(false);
        self.boxbreath_master_row.set_enable_expansion(master_on);
        self.boxbreath_master_row.set_expanded(master_on);

        // Per-phase: enable + toggle + revealers + subtitles.
        use crate::db::BoxBreathPhaseId as P;
        self.refresh_boxbreath_phase_row(
            P::In,
            &self.boxbreath_phase_in_row,
            &self.boxbreath_phase_in_signal_toggle_host,
            &self.boxbreath_phase_in_sound_revealer,
            &self.boxbreath_phase_in_pattern_revealer,
            &app,
        );
        self.refresh_boxbreath_phase_row(
            P::HoldIn,
            &self.boxbreath_phase_holdin_row,
            &self.boxbreath_phase_holdin_signal_toggle_host,
            &self.boxbreath_phase_holdin_sound_revealer,
            &self.boxbreath_phase_holdin_pattern_revealer,
            &app,
        );
        self.refresh_boxbreath_phase_row(
            P::Out,
            &self.boxbreath_phase_out_row,
            &self.boxbreath_phase_out_signal_toggle_host,
            &self.boxbreath_phase_out_sound_revealer,
            &self.boxbreath_phase_out_pattern_revealer,
            &app,
        );
        self.refresh_boxbreath_phase_row(
            P::HoldOut,
            &self.boxbreath_phase_holdout_row,
            &self.boxbreath_phase_holdout_signal_toggle_host,
            &self.boxbreath_phase_holdout_sound_revealer,
            &self.boxbreath_phase_holdout_pattern_revealer,
            &app,
        );
        for phase in P::all() {
            self.refresh_boxbreath_phase_subtitles(*phase);
        }

        self.bells_loading.set(false);
    }

    fn refresh_boxbreath_phase_row(
        &self,
        phase: crate::db::BoxBreathPhaseId,
        phase_row: &adw::ExpanderRow,
        toggle_host: &gtk::Box,
        sound_revealer: &gtk::Revealer,
        pattern_revealer: &gtk::Revealer,
        app: &crate::application::MeditateApplication,
    ) {
        let p = match app
            .with_db(|db| db.get_box_breath_phase(phase))
            .and_then(|r| r.ok())
            .flatten()
        {
            Some(p) => p,
            None => return,
        };
        phase_row.set_enable_expansion(p.enabled);
        phase_row.set_expanded(p.enabled);
        apply_phase_signal_mode_state(
            toggle_host, sound_revealer, pattern_revealer,
            app, p.signal_mode,
        );
    }

    pub(crate) fn refresh_boxbreath_phase_subtitles(
        &self,
        phase: crate::db::BoxBreathPhaseId,
    ) {
        let Some(app) = self.get_app() else { return; };
        let Some(p) = app
            .with_db(|db| db.get_box_breath_phase(phase))
            .and_then(|r| r.ok())
            .flatten()
        else { return; };
        let sound_name = app
            .with_db(|db| db.list_bell_sounds())
            .and_then(|r| r.ok())
            .unwrap_or_default()
            .into_iter()
            .find(|s| s.uuid == p.sound_uuid)
            .map(|s| s.name)
            .unwrap_or_default();
        let pattern_name = app
            .with_db(|db| db.find_vibration_pattern_by_uuid(&p.pattern_uuid))
            .and_then(|r| r.ok())
            .flatten()
            .map(|p| p.name)
            .unwrap_or_default();
        use crate::db::BoxBreathPhaseId as PP;
        let (sound_row, pattern_row): (&adw::ActionRow, &adw::ActionRow) = match phase {
            PP::In      => (&self.boxbreath_phase_in_sound_row,      &self.boxbreath_phase_in_pattern_row),
            PP::HoldIn  => (&self.boxbreath_phase_holdin_sound_row,  &self.boxbreath_phase_holdin_pattern_row),
            PP::Out     => (&self.boxbreath_phase_out_sound_row,     &self.boxbreath_phase_out_pattern_row),
            PP::HoldOut => (&self.boxbreath_phase_holdout_sound_row, &self.boxbreath_phase_holdout_pattern_row),
        };
        sound_row.set_subtitle(&sound_name);
        pattern_row.set_subtitle(&pattern_name);
    }
}

/// Build the AdwToggleGroup for a Sound / Vibration / Both selector
/// at construction time and append it to `host`. The notify handler
/// resolves `app` lazily via `get_app` so the widget can be wired
/// before the timer view has a root. Saved-state load + capability
/// gating run later via `apply_signal_mode_state`.
pub(crate) fn build_signal_mode_toggle_widget(
    host: &gtk::Box,
    sound_revealer: &gtk::Revealer,
    pattern_revealer: &gtk::Revealer,
    setting_key: &'static str,
    get_app: impl Fn() -> Option<crate::application::MeditateApplication> + 'static,
) {
    let toggle_group = adw::ToggleGroup::builder()
        .css_classes(["round"])
        .valign(gtk::Align::Center)
        .build();

    let sound_toggle = adw::Toggle::builder()
        .name("sound")
        .label(crate::i18n::gettext("Sound"))
        .build();
    let vibration_toggle = adw::Toggle::builder()
        .name("vibration")
        .label(crate::i18n::gettext("Vibration"))
        .build();
    let both_toggle = adw::Toggle::builder()
        .name("both")
        .label(crate::i18n::gettext("Both"))
        .build();

    toggle_group.add(sound_toggle);
    toggle_group.add(vibration_toggle);
    toggle_group.add(both_toggle);
    toggle_group.set_active_name(Some("sound"));
    sound_revealer.set_reveal_child(true);
    pattern_revealer.set_reveal_child(false);

    host.append(&toggle_group);

    let sound_revealer = sound_revealer.clone();
    let pattern_revealer = pattern_revealer.clone();
    toggle_group.connect_active_name_notify(move |tg| {
        let Some(name) = tg.active_name() else { return; };
        let value = match name.as_str() {
            "vibration" => "vibration",
            "both"      => "both",
            _           => "sound",
        };
        if let Some(app) = get_app() {
            app.with_db_mut(|db| db.set_setting(setting_key, value));
        }
        let show_sound = matches!(value, "sound" | "both");
        let show_pattern = matches!(value, "vibration" | "both");
        sound_revealer.set_reveal_child(show_sound);
        pattern_revealer.set_reveal_child(show_pattern);
    });
}

/// Apply the saved signal_mode setting to a previously-built toggle
/// group, plus capability gating: when `app.has_haptic()` is false,
/// the Vibration / Both segments go insensitive and the active state
/// is forced to 'sound' (without touching the persisted setting, so
/// syncing to a phone restores intent).
pub(crate) fn apply_signal_mode_state(
    host: &gtk::Box,
    sound_revealer: &gtk::Revealer,
    pattern_revealer: &gtk::Revealer,
    app: &crate::application::MeditateApplication,
    setting_key: &'static str,
) {
    let Some(toggle_group) = first_toggle_group_in(host) else { return; };

    if !app.has_haptic() {
        if let Some(t) = toggle_group.toggle_by_name("vibration") {
            t.set_enabled(false);
        }
        if let Some(t) = toggle_group.toggle_by_name("both") {
            t.set_enabled(false);
        }
    }

    let saved = app
        .with_db(|db| db.get_setting(setting_key, "sound"))
        .and_then(|r| r.ok())
        .unwrap_or_else(|| "sound".to_string());
    let initial = if !app.has_haptic() {
        // Force-display 'sound' on no-haptic devices, regardless of
        // saved value. Persisted setting stays untouched so a sync
        // to a phone restores the user's intent.
        "sound"
    } else {
        match saved.as_str() {
            "vibration" => "vibration",
            "both"      => "both",
            _           => "sound",
        }
    };
    toggle_group.set_active_name(Some(initial));
    let show_sound = matches!(initial, "sound" | "both");
    let show_pattern = matches!(initial, "vibration" | "both");
    sound_revealer.set_reveal_child(show_sound);
    pattern_revealer.set_reveal_child(show_pattern);
}

/// Phase-config variant of `build_signal_mode_toggle_widget`. The
/// notify handler resolves app lazily (via `get_app`) and persists
/// the new mode through `set_box_breath_phase` instead of writing
/// to a settings key. Initial state is sound-revealed / pattern-
/// hidden; refresh-on-visit applies the saved column value.
pub(crate) fn build_phase_signal_mode_toggle_widget(
    host: &gtk::Box,
    sound_revealer: &gtk::Revealer,
    pattern_revealer: &gtk::Revealer,
    phase: crate::db::BoxBreathPhaseId,
    get_app: impl Fn() -> Option<crate::application::MeditateApplication> + 'static,
) {
    let toggle_group = adw::ToggleGroup::builder()
        .css_classes(["round"])
        .valign(gtk::Align::Center)
        .build();
    toggle_group.add(adw::Toggle::builder()
        .name("sound").label(crate::i18n::gettext("Sound")).build());
    toggle_group.add(adw::Toggle::builder()
        .name("vibration").label(crate::i18n::gettext("Vibration")).build());
    toggle_group.add(adw::Toggle::builder()
        .name("both").label(crate::i18n::gettext("Both")).build());
    toggle_group.set_active_name(Some("sound"));
    sound_revealer.set_reveal_child(true);
    pattern_revealer.set_reveal_child(false);

    host.append(&toggle_group);

    let sound_revealer = sound_revealer.clone();
    let pattern_revealer = pattern_revealer.clone();
    toggle_group.connect_active_name_notify(move |tg| {
        let Some(name) = tg.active_name() else { return; };
        let mode = match name.as_str() {
            "vibration" => crate::db::SignalMode::Vibration,
            "both"      => crate::db::SignalMode::Both,
            _           => crate::db::SignalMode::Sound,
        };
        if let Some(app) = get_app() {
            if let Some(p) = app
                .with_db(|db| db.get_box_breath_phase(phase))
                .and_then(|r| r.ok())
                .flatten()
            {
                app.with_db_mut(|db| db.set_box_breath_phase(
                    phase, p.enabled, mode, &p.sound_uuid, &p.pattern_uuid,
                ));
            }
        }
        sound_revealer.set_reveal_child(matches!(
            mode,
            crate::db::SignalMode::Sound | crate::db::SignalMode::Both
        ));
        pattern_revealer.set_reveal_child(matches!(
            mode,
            crate::db::SignalMode::Vibration | crate::db::SignalMode::Both
        ));
    });
}

/// Apply the saved phase-row signal_mode + capability gating. Called
/// from refresh-on-visit. Force-displays Sound when has_haptic is
/// false, leaving the saved column value untouched.
pub(crate) fn apply_phase_signal_mode_state(
    host: &gtk::Box,
    sound_revealer: &gtk::Revealer,
    pattern_revealer: &gtk::Revealer,
    app: &crate::application::MeditateApplication,
    saved: crate::db::SignalMode,
) {
    let Some(toggle_group) = first_toggle_group_in(host) else { return; };
    if !app.has_haptic() {
        if let Some(t) = toggle_group.toggle_by_name("vibration") { t.set_enabled(false); }
        if let Some(t) = toggle_group.toggle_by_name("both")      { t.set_enabled(false); }
    }
    let initial = if !app.has_haptic() {
        crate::db::SignalMode::Sound
    } else {
        saved
    };
    let name = match initial {
        crate::db::SignalMode::Sound     => "sound",
        crate::db::SignalMode::Vibration => "vibration",
        crate::db::SignalMode::Both      => "both",
    };
    toggle_group.set_active_name(Some(name));
    sound_revealer.set_reveal_child(matches!(
        initial,
        crate::db::SignalMode::Sound | crate::db::SignalMode::Both
    ));
    pattern_revealer.set_reveal_child(matches!(
        initial,
        crate::db::SignalMode::Vibration | crate::db::SignalMode::Both
    ));
}

/// Per-mode "what plays" Cues toggle. The persistence handler
/// resolves the active mode + app lazily at click time and writes
/// to the matching setting key — `timer_signal_mode`,
/// `guided_signal_mode`, or `boxbreath_signal_mode` — so the same
/// widget serves all three modes. State load + capability gating
/// run later via `refresh_cues_signal_mode_state`.
pub(crate) fn build_per_mode_signal_toggle_widget(
    host: &gtk::Box,
    get_app: impl Fn() -> Option<crate::application::MeditateApplication> + 'static,
    get_mode: impl Fn() -> TimerMode + 'static,
) {
    let toggle_group = adw::ToggleGroup::builder()
        .css_classes(["round"])
        .valign(gtk::Align::Center)
        .build();
    toggle_group.add(adw::Toggle::builder()
        .name("sound").label(crate::i18n::gettext("Sound")).build());
    toggle_group.add(adw::Toggle::builder()
        .name("vibration").label(crate::i18n::gettext("Vibration")).build());
    toggle_group.add(adw::Toggle::builder()
        .name("both").label(crate::i18n::gettext("Both")).build());
    toggle_group.set_active_name(Some("both"));

    host.append(&toggle_group);

    toggle_group.connect_active_name_notify(move |tg| {
        let Some(name) = tg.active_name() else { return; };
        let value = match name.as_str() {
            "sound"     => "sound",
            "vibration" => "vibration",
            _           => "both",
        };
        let Some(app) = get_app() else { return; };
        let setting_key = setting_key_for_mode(get_mode());
        app.with_db_mut(|db| db.set_setting(setting_key, value));
    });
}

/// Map a TimerMode to its per-mode signal-mode setting key.
pub(crate) fn setting_key_for_mode(mode: TimerMode) -> &'static str {
    match mode {
        TimerMode::Timer     => "timer_signal_mode",
        TimerMode::Guided    => "guided_signal_mode",
        TimerMode::Breathing => "boxbreath_signal_mode",
    }
}

/// Walk a Gtk.Box and return the first AdwToggleGroup child, or
/// None if the host doesn't have one yet.
fn first_toggle_group_in(host: &gtk::Box) -> Option<adw::ToggleGroup> {
    use gtk::prelude::WidgetExt;
    let mut child = host.first_child();
    while let Some(w) = child {
        if let Ok(tg) = w.clone().downcast::<adw::ToggleGroup>() {
            return Some(tg);
        }
        child = w.next_sibling();
    }
    None
}

/// AdwActionRow's `activated` signal only fires when the row is a
/// direct GtkListBox child — wrapping it in a Gtk.Revealer breaks
/// the chain. Attach a primary-button click gesture that calls
/// `widget.activate()` on the row, re-firing the activated signal
/// so existing `connect_activated` handlers still work.
fn attach_revealer_row_click(row: &adw::ActionRow) {
    use gtk::prelude::WidgetExt;
    let click = gtk::GestureClick::new();
    click.set_button(gtk::gdk::BUTTON_PRIMARY);
    let row_weak = row.downgrade();
    click.connect_released(move |gesture, _n_press, _x, _y| {
        if let Some(row) = row_weak.upgrade() {
            // ActionRowExt::activate (NOT WidgetExt::activate) is what
            // emits the row's "activated" signal — the listbox-driven
            // path that connect_activated hooks. WidgetExt::activate
            // calls the generic activate-default handler instead and
            // wouldn't reach our listener.
            adw::prelude::ActionRowExt::activate(&row);
        }
        gesture.set_state(gtk::EventSequenceState::Claimed);
    });
    row.add_controller(click);
}

// ── Mode switching ────────────────────────────────────────────────────────────

impl TimerView {
    pub(super) fn breathing_target_secs(&self) -> u64 {
        self.breath_target.get().as_secs()
    }


    /// Which mode the radio group currently reflects. Exactly one of
    /// the three toggles is active at any time (they share a group).
    /// Stopwatch-vs-countdown lives on `stopwatch_toggle_on` within
    /// the Timer branch.
    pub(crate) fn current_mode(&self) -> TimerMode {
        match self.mode_toggle_group.active_name().as_deref() {
            Some("guided")    => TimerMode::Guided,
            Some("breathing") => TimerMode::Breathing,
            _                 => TimerMode::Timer,
        }
    }

    /// Called when any of the three mode toggles gains active state.
    fn on_mode_switched(&self) {
        let mode = self.current_mode();

        // Input panels: only the active mode's inputs are visible.
        // Toggle visibility on the OUTER clamp wrappers (where they
        // exist) so the parent-box spacing chain skips the slot
        // entirely. Hiding only the inner content would leave an
        // empty visible clamp in the chain and add a phantom 14 px
        // gap on either side of it.
        self.countdown_inputs.set_visible(mode == TimerMode::Timer);
        self.boxbreath_inputs.set_visible(mode == TimerMode::Breathing);
        self.guided_section.set_visible(mode == TimerMode::Guided);
        self.boxbreath_phase_section.set_visible(mode == TimerMode::Breathing);
        // Starting Bell + Preparation Time + Interval Bells apply to
        // Timer mode only — Box Breathing has its own independent
        // rhythm + start-cue model, and Guided mode's "start cue" is
        // the audio file's natural opening, so the whole bell stack
        // goes away outside Timer.
        self.starting_bell_row.set_visible(mode == TimerMode::Timer);
        self.interval_bells_enabled_row.set_visible(mode == TimerMode::Timer);
        // Stopwatch toggle only makes sense in Timer mode (Box Breath
        // has no count-up mode; Guided mode's duration comes from the
        // file). Hide the row entirely outside Timer.
        self.stopwatch_mode_row.set_visible(mode == TimerMode::Timer);
        // Duration row hides in Guided mode — the duration is read
        // from the picked file's metadata, the user can't dial it in.
        self.duration_row.set_visible(mode != TimerMode::Guided);
        // Presets section also hides in Guided mode — guided meditation
        // has its own library (the starred-files group inside
        // guided_inputs) so the preset machinery is irrelevant. Hide
        // the OUTER clamp (presets_section), not just the inner widgets.
        self.presets_section.set_visible(mode != TimerMode::Guided);
        // Refresh the duration label from the appropriate Cell on every
        // mode switch so the suffix doesn't lag.
        self.refresh_duration_value_label();
        // Per-mode Cues toggle reflects the new mode's saved value.
        self.refresh_cues_signal_mode_state();
        // Visible-list contents are mode-strict (Timer presets only
        // appear in Timer mode, Box-Breath presets in Box Breath mode)
        // — rebuild on every switch. Guided mode rebuilds its own
        // starred-files list instead.
        if mode == TimerMode::Guided {
            self.rebuild_starred_guided_list();
            self.refresh_guided_selected_row();
        } else {
            self.rebuild_starred_presets_list();
        }

        // Each mode keeps its own last-used label. On switch, pull the
        // stored preference (or fall back to the mode-specific default —
        // "Box-breathing" for Breathing, "Guided Meditation" for Guided,
        // "Meditation" for Timer) and apply it to the setup combo.
        self.apply_preferred_label_for_mode(mode);

        match self.timer_state.get() {
            TimerState::Idle      => self.show_idle_ui(),
            TimerState::Paused    => self.show_paused_ui(self.current_display_secs()),
            TimerState::Done      => self.view_stack.set_visible_child_name("done"),
            // Running, Preparing, and Overtime normally can't reach
            // here (the nav page blocks the toggle while a session
            // or prep is in flight); fall back to idle UI as a
            // safety net.
            TimerState::Running | TimerState::Preparing | TimerState::Overtime => {
                self.show_idle_ui()
            }
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
        self.guided_section.set_sensitive(true);
        self.countdown_inputs.set_visible(mode == TimerMode::Timer);
        self.boxbreath_inputs.set_visible(mode == TimerMode::Breathing);
        self.guided_section.set_visible(mode == TimerMode::Guided);
        self.boxbreath_phase_section.set_visible(mode == TimerMode::Breathing);
        self.starting_bell_row.set_visible(mode == TimerMode::Timer);
        self.interval_bells_enabled_row.set_visible(mode == TimerMode::Timer);
        self.stopwatch_mode_row.set_visible(mode == TimerMode::Timer);
        self.duration_row.set_visible(mode != TimerMode::Guided);
        self.presets_section.set_visible(mode != TimerMode::Guided);
        self.refresh_duration_value_label();
        self.mode_toggle_group.set_sensitive(true);
        self.session_group.set_sensitive(true);
        self.refresh_hero_for_idle();
    }

    /// Pull the right Cell into the shared Duration row's value label.
    /// Both modes store seconds; divide by 60 here for the H:MM render.
    fn refresh_duration_value_label(&self) {
        let mins = match self.current_mode() {
            TimerMode::Timer     => self.countdown_target_secs.get() / 60,
            TimerMode::Breathing => self.breathing_session_secs.get() as u64 / 60,
            // Duration row is hidden in Guided mode (the duration
            // comes from the picked file's metadata, the user can't
            // dial it in). The label would never render — read 0
            // so a future flag-flip can't expose stale numbers.
            TimerMode::Guided    => 0,
        };
        let h = mins / 60;
        let m = mins % 60;
        self.duration_value_label.set_label(&format!("{h:02}:{m:02}"));
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
        self.mode_toggle_group.set_sensitive(false);
        self.session_group.set_sensitive(false);
        self.big_time_label.set_label(&format_time(display_secs));
        self.time_unit_label.set_label(&crate::i18n::gettext("Paused"));
        self.time_unit_label.set_visible(true);
    }

    /// Set the hero time display + subtitle to their idle-state values for
    /// whichever mode is currently active.
    fn refresh_hero_for_idle(&self) {
        let label = match self.current_mode() {
            TimerMode::Timer => {
                if self.stopwatch_toggle_on.get() {
                    "00:00".to_string()
                } else {
                    let secs = self.countdown_target_secs.get();
                    let h = secs / 3600;
                    let m = (secs % 3600) / 60;
                    format!("{h:02}:{m:02}")
                }
            }
            TimerMode::Breathing => {
                // Same hh:mm format as Timer for layout consistency.
                // breathing_session_secs is the canonical store; divide
                // by 60 to get minutes for the display computation.
                let m = self.breathing_session_secs.get() / 60;
                format!("{:02}:{:02}", m / 60, m % 60)
            }
            TimerMode::Guided => {
                // Hero shows the picked file's natural duration — the
                // session length is whatever the audio runs for. Empty
                // state (no file selected) reads 00:00 so the layout
                // doesn't shift when the user picks something.
                let secs = self
                    .guided_pick
                    .borrow()
                    .as_ref()
                    .map(|p| p.duration_secs)
                    .unwrap_or(0) as u64;
                let h = secs / 3600;
                let m = (secs % 3600) / 60;
                format!("{h:02}:{m:02}")
            }
        };
        self.big_time_label.set_label(&label);
        self.time_unit_label.set_label(&crate::i18n::gettext("Hours · Minutes"));
        self.time_unit_label.set_visible(true);
    }

    /// Re-apply the stopwatch toggle's effect on the rest of the setup
    /// page: hero label flips between the picked target and 00:00, and
    /// the Quick Presets card greys out so the user can't tap a chip
    /// while the toggle is on.
    fn refresh_stopwatch_dependent_ui(&self) {
        if self.timer_state.get() == TimerState::Idle
            && self.current_mode() == TimerMode::Timer
        {
            self.refresh_hero_for_idle();
        }
        // Stopwatch on ⇒ planned-duration concept inert; grey out
        // the Duration row only. The presets list stays interactive —
        // tapping a preset is a higher-level action that legitimately
        // re-arms the duration (and resets the stopwatch toggle as
        // part of its config).
        let duration_active = !self.stopwatch_toggle_on.get();
        self.duration_row.set_sensitive(duration_active);
        // Fixed-from-end bells become inert when stopwatch flips on,
        // active again when it flips off — refresh the Manage Bells
        // subtitle so the count matches what will actually fire. End
        // Bell falls into the same bucket: stopwatch has no end so
        // the bell can't fire. Override the row to off + insensitive
        // without touching the persisted setting, so flipping
        // stopwatch back off restores the user's previous choice.
        self.refresh_interval_bells_count();
        self.refresh_end_bell_dependent_ui();
    }

    /// Mute / restore the End Bell row as a function of the
    /// stopwatch toggle. UI-only override — the persisted
    /// `end_bell_active` setting stays as the user left it, so
    /// flipping stopwatch off brings the previous state back. The
    /// bells_loading guard suppresses the row's own notify handler
    /// during the programmatic state change.
    fn refresh_end_bell_dependent_ui(&self) {
        let stopwatch_on = self.stopwatch_toggle_on.get();
        let persisted_on = self
            .get_app()
            .and_then(|app| {
                app.with_db(|db| {
                    db.get_setting("end_bell_active", "true")
                        .map(|v| v == "true")
                        .unwrap_or(true)
                })
            })
            .unwrap_or(true);
        self.bells_loading.set(true);
        if stopwatch_on {
            self.end_bell_row.set_enable_expansion(false);
            self.end_bell_row.set_expanded(false);
            self.end_bell_row.set_sensitive(false);
        } else {
            self.end_bell_row.set_enable_expansion(persisted_on);
            self.end_bell_row.set_expanded(persisted_on);
            self.end_bell_row.set_sensitive(true);
        }
        self.bells_loading.set(false);
    }
}

// ── Timer state machine ───────────────────────────────────────────────────────

impl TimerView {
    fn on_start(&self) {
        let mode = self.current_mode();

        // Timer mode + Preparation Time on: enter Preparing, defer the
        // real cores + starting bell until the prep tick transitions.
        // Box Breathing skips prep entirely (it's a Timer-only feature).
        let prep = if mode == TimerMode::Timer {
            self.get_app()
                .and_then(|app| {
                    app.with_db(|db| {
                        let active = db
                            .get_setting("preparation_time_active", "false")
                            .map(|v| v == "true")
                            .unwrap_or(false);
                        let starting = db
                            .get_setting("starting_bell_active", "false")
                            .map(|v| v == "true")
                            .unwrap_or(false);
                        let secs = db
                            .get_setting(
                                "preparation_time_secs",
                                &meditate_core::format::PREP_SECS_DEFAULT.to_string(),
                            )
                            .map(|s| meditate_core::format::parse_prep_secs(&s))
                            .unwrap_or(meditate_core::format::PREP_SECS_DEFAULT);
                        // Prep only makes sense if there's a starting bell
                        // to delay — silence with no bell is just a wait
                        // for nothing.
                        meditate_core::format::prep_target_duration(active && starting, secs)
                    })
                })
                .flatten()
        } else {
            None
        };

        match mode {
            TimerMode::Timer => {
                if prep.is_none() {
                    self.start_boot_time.set(Some(boot_time_now()));
                    if self.stopwatch_toggle_on.get() {
                        *self.stopwatch_core.borrow_mut() =
                            Some(CoreStopwatch::started_at(std::time::Duration::ZERO));
                    } else {
                        let target = self.countdown_target_secs.get();
                        if target == 0 {
                            return;
                        }
                        let timer =
                            CoreCountdownTimer::new(std::time::Duration::from_secs(target));
                        let sw = CoreStopwatch::started_at(std::time::Duration::ZERO);
                        *self.countdown_core.borrow_mut() =
                            Some(CoreCountdown::new(timer, sw));
                    }
                }
                // Else: cores stay None until transition_prep_to_running.
                // Validate countdown target up front so a 0-target
                // countdown doesn't enter prep just to land on an
                // un-startable session.
                if prep.is_some()
                    && !self.stopwatch_toggle_on.get()
                    && self.countdown_target_secs.get() == 0
                {
                    return;
                }
            }
            TimerMode::Breathing => {
                let pattern = self.breathing_pattern.get();
                let cycle = pattern.cycle_secs().max(1) as u64;
                // "Finish the breath" before stopping: round the requested
                // duration up to the next full cycle so the session always
                // ends on an exhale/hold-out boundary.
                let raw = self.breathing_session_secs.get() as u64;
                let target = raw.div_ceil(cycle) * cycle;
                self.start_boot_time.set(Some(boot_time_now()));
                *self.breath_stopwatch.borrow_mut() =
                    Some(CoreStopwatch::started_at(std::time::Duration::ZERO));
                self.breath_target.set(std::time::Duration::from_secs(target));
            }
            TimerMode::Guided => {
                // Build the countdown core (drives the hero) AND the
                // gst playbin (drives the audio). Both are tied to the
                // same target duration probed at file-pick time. The
                // playbin's EOS signal-watch slides into Overtime in
                // case the file ends slightly before the probed
                // duration — keeps the session.end-bell handshake
                // honest even with sub-second drift between the two.
                let pick = self.guided_pick.borrow().clone();
                let Some(pick) = pick else { return; };
                let target = pick.duration_secs as u64;
                if target == 0 {
                    return;
                }

                // Audio first: a failure here (corrupt file, missing
                // codec) bails the whole start path so the user sees
                // a toast and the session never enters Running.
                let obj_for_eos = self.obj().clone();
                match crate::guided::GuidedPlayback::start(
                    &pick.source_path,
                    move |/* on_eos */| {
                        // EOS arrives on the GTK main thread thanks
                        // to the bus signal watch + glib's default
                        // MainContext. Slide into Overtime if Running;
                        // a no-op if we've already transitioned.
                        let imp = obj_for_eos.imp();
                        if imp.timer_state.get() == TimerState::Running {
                            imp.transition_running_to_overtime();
                        }
                    },
                ) {
                    Ok(playback) => {
                        *self.guided_playback.borrow_mut() = Some(playback);
                    }
                    Err(e) => {
                        self.toast(&format!(
                            "{}: {e}",
                            crate::i18n::gettext("Couldn't start playback"),
                        ));
                        return;
                    }
                }

                self.start_boot_time.set(Some(boot_time_now()));
                let timer = CoreCountdownTimer::new(std::time::Duration::from_secs(target));
                let sw = CoreStopwatch::started_at(std::time::Duration::ZERO);
                *self.countdown_core.borrow_mut() = Some(CoreCountdown::new(timer, sw));
            }
        }

        // Prep-mode setup: anchor the boot time, install the prep
        // stopwatch + target, and the tick will count down before
        // playing the bell + setting up the real cores.
        if let Some(prep_dur) = prep {
            self.start_boot_time.set(Some(boot_time_now()));
            *self.prep_stopwatch.borrow_mut() =
                Some(CoreStopwatch::started_at(std::time::Duration::ZERO));
            self.prep_target.set(prep_dur);
            self.timer_state.set(TimerState::Preparing);
        } else {
            self.timer_state.set(TimerState::Running);
            // Bell-library schedule is built when Running starts. With
            // prep, the same call lives in transition_prep_to_running.
            if mode == TimerMode::Timer {
                self.load_active_bells_for_running();
            }
        }

        self.session_start_time.set(unix_now());

        // Starting bell at session start — only when there's no prep.
        // With prep, the bell fires at the prep→Running transition.
        // Box Breathing never plays the starting bell (Timer-only).
        if mode == TimerMode::Timer && prep.is_none() {
            if let Some(app) = self.get_app() {
                self.fire_starting_bell(&app);
            }
        }

        self.tick_mode.set(mode);
        // Countdown/stopwatch use the shared 1 Hz tick; Breathing drives
        // its own DrawingArea tick from window::push_running_page.
        // Preparing is Timer-mode-only and uses the same tick — the
        // tick's state branch handles prep countdown vs. running.
        if mode != TimerMode::Breathing {
            self.start_tick();
        }
        self.obj().emit_by_name::<()>("timer-started", &[]);
    }

    fn on_resume(&self) {
        let mode = self.current_mode();

        let now = self.elapsed_since_start();
        // If the user paused during prep, the prep stopwatch is the
        // one to resume — the real cores haven't been built yet.
        let resuming_prep = self.prep_stopwatch.borrow().is_some();
        if resuming_prep {
            let mut slot = self.prep_stopwatch.borrow_mut();
            *slot = slot.take().map(|s| s.resumed_at(now));
        } else {
            match mode {
                TimerMode::Timer => {
                    if self.stopwatch_toggle_on.get() {
                        let mut slot = self.stopwatch_core.borrow_mut();
                        *slot = slot.take().map(|s| s.resumed_at(now));
                    } else {
                        let mut slot = self.countdown_core.borrow_mut();
                        *slot = slot.take().map(|c| c.resume(now));
                    }
                }
                TimerMode::Breathing => {
                    let mut slot = self.breath_stopwatch.borrow_mut();
                    *slot = slot.take().map(|s| s.resumed_at(now));
                }
                TimerMode::Guided => {
                    // Guided mode reuses the countdown_core for elapsed
                    // tracking AND drives a gst playbin alongside it.
                    // Resume both — the playbin picks up at the same
                    // position the user paused at; the countdown core
                    // resumes from its frozen elapsed value.
                    let mut slot = self.countdown_core.borrow_mut();
                    *slot = slot.take().map(|c| c.resume(now));
                    if let Some(p) = self.guided_playback.borrow().as_ref() {
                        p.resume();
                    }
                }
            }
        }
        self.timer_state.set(if resuming_prep {
            TimerState::Preparing
        } else {
            TimerState::Running
        });

        self.tick_mode.set(mode);
        if mode != TimerMode::Breathing {
            self.start_tick();
        }
        // Flip the pause-button label back from "Resume" to "Pause"
        // — the running page stays up across pause/resume now, so
        // we own this morph end-to-end.
        if let Some(btn) = self.running_pause_btn.borrow().as_ref() {
            btn.set_label(&crate::i18n::gettext("Pause"));
            btn.set_tooltip_text(Some(&crate::i18n::gettext("Pause Timer")));
        }
        // Refresh the hero label NOW instead of waiting up to ~1s for
        // the first post-resume tick. The cores' elapsed reading is
        // correct the moment resumed_at fires — without this push,
        // tick-scheduling jitter occasionally makes the first visible
        // update land >1s after the click and the user perceives a
        // skipped second.
        if let Some(label) = self.running_label.borrow().as_ref() {
            label.set_label(&format_time(self.current_display_secs()));
        }
        self.obj().emit_by_name::<()>("timer-started", &[]);
    }

    /// Called by the window when the running page's Pause button is pressed.
    pub fn on_pause(&self) {
        self.cancel_tick();

        let mode = self.tick_mode.get();
        let now = self.elapsed_since_start();
        // Prep gets paused on its own stopwatch; the real cores
        // haven't been set up yet during prep.
        if self.prep_stopwatch.borrow().is_some() {
            let mut slot = self.prep_stopwatch.borrow_mut();
            *slot = slot.take().map(|s| s.paused_at(now));
        } else {
            match mode {
                TimerMode::Timer => {
                    if self.stopwatch_toggle_on.get() {
                        let mut slot = self.stopwatch_core.borrow_mut();
                        *slot = slot.take().map(|s| s.paused_at(now));
                    } else {
                        let mut slot = self.countdown_core.borrow_mut();
                        *slot = slot.take().map(|c| c.pause(now));
                    }
                }
                TimerMode::Breathing => {
                    let mut slot = self.breath_stopwatch.borrow_mut();
                    *slot = slot.take().map(|s| s.paused_at(now));
                }
                TimerMode::Guided => {
                    let mut slot = self.countdown_core.borrow_mut();
                    *slot = slot.take().map(|c| c.pause(now));
                    if let Some(p) = self.guided_playback.borrow().as_ref() {
                        p.pause();
                    }
                }
            }
        }
        self.timer_state.set(TimerState::Paused);

        // Stay on the running page — morph the running pause-button
        // to "Resume" so the user can pick up without first popping
        // back to the dimmed setup view. The same physical button
        // is reused; toggle_playback dispatches Paused → on_resume.
        if let Some(btn) = self.running_pause_btn.borrow().as_ref() {
            btn.set_label(&crate::i18n::gettext("Resume"));
            btn.set_tooltip_text(Some(&crate::i18n::gettext("Resume Timer")));
        }

        self.show_paused_ui(self.current_display_secs());
        self.obj().emit_by_name::<()>("timer-paused", &[]);
    }

    /// Called by the window when Stop is pressed (from running page or paused state).
    pub fn on_stop(&self) {
        self.cancel_tick();

        let mode = self.current_mode();

        let elapsed = self.elapsed_secs_for_mode(mode);
        // Pin the elapsed we just computed so on_save reads the same
        // value the Done page is about to show. Without this, a stop
        // during prep loses the session: on_save recomputes elapsed
        // through `elapsed_secs_for_mode`, but by then prep_stopwatch
        // has been cleared (line below) and no running core exists
        // (transition_prep_to_running never ran), so the fallback
        // returns 0 and on_save silently drops the row. Mirrors the
        // Overtime Finish path which uses the same slot.
        self.final_duration_secs.set(Some(elapsed));
        self.timer_state.set(TimerState::Done);
        // Drop any prep state — the user stopped during prep, the
        // session's "elapsed" came from the prep stopwatch above.
        // Active bells stop firing the moment we leave Running, but
        // the schedule is also dropped here so a quick re-Start
        // rebuilds it from current settings.
        *self.prep_stopwatch.borrow_mut() = None;
        self.active_bells.borrow_mut().clear();
        // Guided playback stops the moment the user picks Stop —
        // Drop runs set_state(Null) + drops the bus signal-watch.
        // No-op for non-Guided sessions (slot is already None).
        *self.guided_playback.borrow_mut() = None;

        // Release the running-page widget refs — the page is about
        // to pop when "timer-stopped" fires below.
        *self.running_label.borrow_mut() = None;
        *self.running_pause_btn.borrow_mut() = None;
        *self.running_stop_btn.borrow_mut() = None;
        *self.overtime_add_btn.borrow_mut() = None;

        self.obj().emit_by_name::<()>("timer-stopped", &[]);
        self.show_done(elapsed);
    }

    /// Elapsed seconds for the active session, dispatching on mode +
    /// stopwatch toggle. Used by on_stop / on_save (both produce a
    /// session row whose `duration_secs` is what we return here).
    /// During (or paused-from) prep, the cores haven't been set up
    /// yet — fall back to the prep stopwatch so a "stop during prep"
    /// still saves a real session row with the time the user spent.
    fn elapsed_secs_for_mode(&self, mode: TimerMode) -> u64 {
        // Overtime "Finish" sets this so the saved duration is the
        // planned countdown instead of countdown + overtime. Add's
        // path leaves it unset and falls through to natural elapsed.
        if let Some(forced) = self.final_duration_secs.get() {
            return forced;
        }
        if let Some(prep) = self.prep_stopwatch.borrow().as_ref() {
            return prep.elapsed(self.elapsed_since_start()).as_secs();
        }
        match mode {
            TimerMode::Timer => {
                if self.stopwatch_toggle_on.get() {
                    self.stopwatch_elapsed_secs()
                } else {
                    self.countdown_elapsed_secs()
                }
            }
            TimerMode::Breathing => self.breath_elapsed().as_secs(),
            // Guided uses the countdown_core for elapsed tracking
            // (set up in start_session). Same shape as Timer countdown.
            TimerMode::Guided => self.countdown_elapsed_secs(),
        }
    }

    fn show_done(&self, elapsed_secs: u64) {
        self.done_duration_label.set_label(&format_time(elapsed_secs));
        self.note_view.buffer().set_text("");
        // Mirror the Setup view's currently-active label into the
        // Done page's per-session pick. The user can flip the toggle
        // off or change the pick before tapping Save.
        self.done_selected_label_id.set(self.setup_selected_label_id());
        self.refresh_done_label_chooser_subtitle();
        // Skip the stack's crossfade when entering Done — the running
        // nav page is about to pop on top of this stack, and a fade
        // here means the timer view bleeds through for the first
        // frames of the pop animation. Done is the destination, so
        // flip instantly; the back-to-setup path keeps its fade.
        let saved = self.view_stack.transition_type();
        self.view_stack.set_transition_type(gtk::StackTransitionType::None);
        self.view_stack.set_visible_child_name("done");
        self.view_stack.set_transition_type(saved);
        // Without this, GTK's default-focus logic lands on `note_view` (the
        // first focusable descendant), which on phones pops the on-screen
        // keyboard up and hides Save/Discard. Putting focus on Save keeps
        // the action buttons visible; the user can still tap the note view
        // explicitly to start typing.
        self.save_btn.grab_focus();
    }

    fn on_save(&self) {
        crate::sound::stop_all();
        let mode = self.current_mode();

        let elapsed = self.elapsed_secs_for_mode(mode);
        let start_time = self.session_start_time.get();

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
        // Per-session pick is stored on `done_selected_label_id`,
        // mirrored from Setup at show_done and mutable on the Done
        // page. None = toggle off / no label.
        let label_id = self.done_selected_label_id.get();

        let session_mode = match mode {
            TimerMode::Timer => SessionMode::Timer,
            TimerMode::Breathing => SessionMode::BoxBreath,
            TimerMode::Guided => SessionMode::Guided,
        };

        // Guided sessions log the file's uuid (when the user played a
        // starred library row) so per-file stats can resolve later.
        // Transient Open-File picks log None.
        let guided_file_uuid = if mode == TimerMode::Guided {
            self.guided_selected_uuid.borrow().clone()
        } else {
            None
        };

        let data = SessionData {
            start_time,
            duration_secs: elapsed as i64,
            mode:          session_mode,
            label_id,
            note,
            guided_file_uuid,
        };

        // Record the user's pick as the new persisted default for
        // this mode — covers the case where they changed the
        // selection on the Done screen and want it stuck for next
        // session. Off-toggle clears the active flag so the next
        // session starts off too.
        match label_id {
            Some(id) => {
                let uuid = self.get_app().and_then(|app| {
                    app.with_db(|db| db.list_labels())
                        .and_then(|r| r.ok())
                        .unwrap_or_default()
                        .into_iter()
                        .find(|l| l.id == id)
                        .map(|l| l.uuid)
                });
                if let Some(uuid) = uuid {
                    self.persist_label_uuid_for_mode(mode, &uuid);
                    self.persist_label_active_for_mode(mode, true);
                }
            }
            None => self.persist_label_active_for_mode(mode, false),
        }

        // Fire-and-forget DB write on the blocking pool. SQLite fsync on
        // eMMC costs ~15 ms even with synchronous=NORMAL; doing it on the
        // main thread is directly felt as a stall at session end. When
        // the write lands we're back on the main thread (spawn_local) so
        // we can push the new session into the log feed incrementally
        // and mark stats stale for lazy refresh on tab re-entry.
        if let Some(app) = self.get_app() {
            glib::MainContext::default().spawn_local(async move {
                let result = app
                    .with_db_blocking_mut(move |db| db.create_session(&data))
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
        crate::sound::stop_all();
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
            TimerMode::Timer => {
                // Clear whichever core was running — at most one is Some
                // per session, but blanking both is safe and saves the
                // toggle-state read. The prep stopwatch is also Timer-
                // mode only and gets reset alongside, as is the active
                // bell schedule built at session start.
                *self.stopwatch_core.borrow_mut() = None;
                *self.countdown_core.borrow_mut() = None;
                *self.prep_stopwatch.borrow_mut() = None;
                self.active_bells.borrow_mut().clear();
            }
            TimerMode::Breathing => *self.breath_stopwatch.borrow_mut() = None,
            TimerMode::Guided => {
                // Same countdown_core slot as Timer countdown. Plus
                // tear down the gst playbin (Drop runs set_state(Null)
                // + removes the bus signal-watch). The playback might
                // already be None if a prior on_stop / overtime path
                // dropped it; the borrow_mut + None assignment is
                // idempotent.
                *self.countdown_core.borrow_mut() = None;
                *self.guided_playback.borrow_mut() = None;
            }
        }
        self.timer_state.set(TimerState::Idle);
        self.session_start_time.set(0);
        self.final_duration_secs.set(None);

        // Only update the visible UI if this mode is the one currently shown.
        if mode == self.current_mode() {
            self.show_idle_ui();
            self.refresh_streak();
        }
    }

    fn start_tick(&self) {
        self.cancel_tick();
        let obj = self.obj().clone();

        let source_id = glib::timeout_add_local(
            std::time::Duration::from_secs(1),
            move || {
                let imp = obj.imp();
                match imp.timer_state.get() {
                    TimerState::Preparing => imp.tick_prep(&obj),
                    TimerState::Running => imp.tick_running(&obj),
                    TimerState::Overtime => imp.tick_overtime(&obj),
                    _ => glib::ControlFlow::Break,
                }
            },
        );
        *self.tick_source.borrow_mut() = Some(source_id);
    }

    /// Prep tick: count down the silent preparation interval. When
    /// elapsed crosses the target, transition to Running — that
    /// flips the cores in, plays the starting bell, and the same
    /// tick keeps firing on the next iteration but takes the Running
    /// branch.
    fn tick_prep(&self, _obj: &super::TimerView) -> glib::ControlFlow {
        let now = self.elapsed_since_start();
        let target = self.prep_target.get();
        let elapsed = self.prep_stopwatch
            .borrow()
            .as_ref()
            .map(|s| s.elapsed(now))
            .unwrap_or_default();
        if elapsed >= target {
            self.transition_prep_to_running();
            return glib::ControlFlow::Continue;
        }
        let remaining = target.saturating_sub(elapsed);
        // Ceiling — when (k-1, k] remaining, show k. Same trick as
        // tick_running's countdown branch.
        let display = remaining.as_secs() + (remaining.subsec_nanos() > 0) as u64;
        if let Some(label) = self.running_label.borrow().as_ref() {
            label.set_label(&format_time(display));
        }
        glib::ControlFlow::Continue
    }

    fn tick_running(&self, _obj: &super::TimerView) -> glib::ControlFlow {
        let is_stopwatch = self.stopwatch_toggle_on.get();
        let (new_secs, done) = {
            if is_stopwatch {
                // Stopwatch: floor seconds (display "0:01" once we
                // cross 1.0s, "0:00" otherwise).
                (self.stopwatch_elapsed_secs(), false)
            } else {
                // Countdown: ceiling seconds (while remaining is in
                // (k-1, k], show k — avoids skipping "0:59" on the
                // first tick which fires slightly past 1.0s).
                let now = self.elapsed_since_start();
                let core = self.countdown_core.borrow();
                let Some(c) = core.as_ref() else {
                    return glib::ControlFlow::Break;
                };
                if c.is_finished(now) {
                    self.timer_state.set(TimerState::Done);
                    (c.elapsed(now).as_secs(), true)
                } else {
                    let r = c.remaining(now);
                    (r.as_secs() + (r.subsec_nanos() > 0) as u64, false)
                }
            }
        };

        if done {
            // Countdown crossed zero. Don't auto-finish — slide
            // into Overtime so the user can either commit the
            // extra time (Add) or stop at the planned duration
            // (Finish). The end bell, vibration, and system
            // notification still fire here because the *planned*
            // session is over; overtime is bonus.
            self.transition_running_to_overtime();
            return glib::ControlFlow::Continue;
        }

        // Fire any bell whose ring boundary has been crossed since the
        // previous tick. For Timer mode the relevant elapsed is the
        // running stopwatch's elapsed (post-prep, post-resume). The
        // collected list is empty when the master toggle is off, so
        // this is cheap when bells aren't in use.
        let elapsed_for_bells = if is_stopwatch {
            new_secs
        } else {
            // Countdown branch's `new_secs` is REMAINING; we want
            // ELAPSED. Read the core's elapsed directly.
            self.countdown_elapsed_secs()
        };
        self.fire_due_bells_at(elapsed_for_bells);

        if let Some(label) = self.running_label.borrow().as_ref() {
            label.set_label(&format_time(new_secs));
        }

        glib::ControlFlow::Continue
    }

    /// One-shot at zero-crossing: ring the end bell + vibrate +
    /// notify, morph the running buttons into the Finish/Add
    /// layout, and flip state to Overtime so subsequent ticks
    /// dispatch through `tick_overtime`. The 1 Hz tick itself
    /// keeps running.
    fn transition_running_to_overtime(&self) {
        self.timer_state.set(TimerState::Overtime);

        // Guided mode: drop the playbin BEFORE play_end_bell so the
        // end bell isn't competing with a few last frames of audio
        // (gst playbin holds a small buffer ahead of the wall clock,
        // so the file may still be sounding when the countdown hits
        // zero). Drop runs set_state(Null) + removes the bus watch.
        *self.guided_playback.borrow_mut() = None;

        if let Some(app) = self.get_app() {
            self.fire_end_bell(&app);
            // Only send a system notification when the app isn't
            // focused — the in-app overtime UI already signals
            // completion.
            if !app.active_window().map(|w| w.is_active()).unwrap_or(false) {
                let n = gtk::gio::Notification::new("Meditation Complete");
                // For Guided sessions, the Hero's frozen value is the
                // file's natural duration — read from the active pick
                // since countdown_target_secs is the Timer-mode field.
                let target = match self.current_mode() {
                    TimerMode::Guided => self
                        .guided_pick
                        .borrow()
                        .as_ref()
                        .map(|p| p.duration_secs as u64)
                        .unwrap_or(0),
                    _ => self.countdown_target_secs.get(),
                };
                n.set_body(Some(&format!("Session: {}", format_time(target))));
                app.send_notification(Some("timer-done"), &n);
            }
        }

        if let Some(stop_btn) = self.running_stop_btn.borrow().as_ref() {
            stop_btn.set_visible(false);
        }
        if let Some(pause_btn) = self.running_pause_btn.borrow().as_ref() {
            pause_btn.set_label(&crate::i18n::gettext("Finish"));
            pause_btn.set_tooltip_text(Some(&crate::i18n::gettext(
                "End at the planned duration",
            )));
        }
        if let Some(add_btn) = self.overtime_add_btn.borrow().as_ref() {
            add_btn.set_label(&format!(
                "{} {} ?",
                crate::i18n::gettext("Add"),
                format_time(0),
            ));
            // Visibility is owned by the Clamp wrapper that the
            // window builder put around the button — flipping the
            // button itself wouldn't reveal the row.
            if let Some(parent) = add_btn.parent() {
                parent.set_visible(true);
            }
        }
        // Hero stays frozen at the planned countdown duration —
        // the user chose that target, so the static reading is
        // their accomplishment. Only the Add button counts up,
        // surfacing how much extra time they've accumulated.
        if let Some(label) = self.running_label.borrow().as_ref() {
            let target = self.countdown_target_secs.get();
            label.set_label(&format_time(target));
        }
    }

    /// 1 Hz update for the Overtime state — refreshes only the
    /// dynamic Add button label, and keeps interval bells firing
    /// on the original session timeline. The hero readout stays
    /// frozen at the planned duration.
    fn tick_overtime(&self, _obj: &super::TimerView) -> glib::ControlFlow {
        let target = self.countdown_target_secs.get();
        let total_elapsed = self.countdown_elapsed_secs();
        let overtime = total_elapsed.saturating_sub(target);

        self.fire_due_bells_at(total_elapsed);

        if let Some(add_btn) = self.overtime_add_btn.borrow().as_ref() {
            add_btn.set_label(&format!(
                "{} {} ?",
                crate::i18n::gettext("Add"),
                format_time(overtime),
            ));
        }
        glib::ControlFlow::Continue
    }

    /// Overtime user picked "Add MM:SS?" — record the planned
    /// duration *plus* the elapsed overtime as the session length,
    /// pop the running page, surface the Done screen.
    pub(super) fn add_overtime_and_finish(&self) {
        if self.timer_state.get() != TimerState::Overtime {
            return;
        }
        let elapsed = self.countdown_elapsed_secs();
        self.end_overtime_session(elapsed);
    }

    /// Overtime user picked "Finish" — record exactly the planned
    /// countdown duration (overtime discarded). `final_duration_secs`
    /// overrides the natural elapsed reading in `elapsed_secs_for_mode`
    /// so the Save path stores the same value the Done screen shows.
    pub(super) fn finish_overtime_session(&self) {
        if self.timer_state.get() != TimerState::Overtime {
            return;
        }
        let target = self.countdown_target_secs.get();
        self.final_duration_secs.set(Some(target));
        self.end_overtime_session(target);
    }

    fn end_overtime_session(&self, elapsed_secs: u64) {
        // The end bell started ringing at the running→overtime
        // transition; once the user picks Finish or Add they've
        // acknowledged the session, so cut any still-playing bell
        // (end + interval) before the Done page comes up.
        crate::sound::stop_all();
        self.cancel_tick();
        *self.running_label.borrow_mut() = None;
        *self.running_pause_btn.borrow_mut() = None;
        *self.running_stop_btn.borrow_mut() = None;
        *self.overtime_add_btn.borrow_mut() = None;
        self.timer_state.set(TimerState::Done);
        self.obj().emit_by_name::<()>("timer-stopped", &[]);
        self.show_done(elapsed_secs);
    }

    /// Prep finished — drop the prep stopwatch, play the starting
    /// bell, set up the real countdown/stopwatch core, re-anchor the
    /// boot time so the running session counts from zero, and flip
    /// the state to Running. The same tick will pick up where this
    /// left off on its next iteration.
    fn transition_prep_to_running(&self) {
        *self.prep_stopwatch.borrow_mut() = None;

        if let Some(app) = self.get_app() {
            self.fire_starting_bell(&app);
        }

        // Re-anchor so the running cores see elapsed starting at zero,
        // not at prep_target.
        self.start_boot_time.set(Some(boot_time_now()));
        if self.stopwatch_toggle_on.get() {
            *self.stopwatch_core.borrow_mut() =
                Some(CoreStopwatch::started_at(std::time::Duration::ZERO));
        } else {
            let target = self.countdown_target_secs.get();
            let timer = CoreCountdownTimer::new(std::time::Duration::from_secs(target));
            let sw = CoreStopwatch::started_at(std::time::Duration::ZERO);
            *self.countdown_core.borrow_mut() = Some(CoreCountdown::new(timer, sw));
        }

        self.timer_state.set(TimerState::Running);
        // Now that Running has started, build the bell schedule.
        self.load_active_bells_for_running();
    }

    /// Natural completion path for a breath session: marks Done, plays the
    /// end chime, vibrates, and sends a notification when not focused.
    /// Mirrors the countdown's done branch (timer.imp at the 1 Hz tick).
    /// Distinct from `on_stop` (user-initiated), which is silent.
    pub(super) fn finish_breath_session(&self) {
        self.timer_state.set(TimerState::Done);
        let elapsed = self.breath_elapsed().as_secs();
        // Release running-page widget refs — the page pops next.
        *self.running_label.borrow_mut() = None;
        *self.running_pause_btn.borrow_mut() = None;
        self.obj().emit_by_name::<()>("timer-stopped", &[]);
        self.show_done(elapsed);
        if let Some(app) = self.get_app() {
            self.fire_end_bell(&app);
            if !app.active_window().map(|w| w.is_active()).unwrap_or(false) {
                let n = gtk::gio::Notification::new("Meditation Complete");
                n.set_body(Some(&format!("Session: {}", format_time(elapsed))));
                app.send_notification(Some("timer-done"), &n);
            }
        }
    }

    /// Countdown remaining seconds (ceiling), 0 if no session running.
    fn countdown_remaining_secs(&self) -> u64 {
        let now = self.elapsed_since_start();
        self.countdown_core
            .borrow()
            .as_ref()
            .map(|c| {
                let r = c.remaining(now);
                r.as_secs() + (r.subsec_nanos() > 0) as u64
            })
            .unwrap_or(0)
    }

    /// Countdown elapsed seconds (target - remaining, capped at target).
    fn countdown_elapsed_secs(&self) -> u64 {
        let now = self.elapsed_since_start();
        self.countdown_core
            .borrow()
            .as_ref()
            .map(|c| c.elapsed(now).as_secs())
            .unwrap_or(0)
    }

    fn stopwatch_elapsed_secs(&self) -> u64 {
        let now = self.elapsed_since_start();
        self.stopwatch_core
            .borrow()
            .as_ref()
            .map(|s| s.elapsed(now).as_secs())
            .unwrap_or(0)
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

    /// Suspend-resilient monotonic time since on_start set the anchor.
    /// Returns ZERO if no session has been started.
    fn elapsed_since_start(&self) -> std::time::Duration {
        match self.start_boot_time.get() {
            Some(start) => boot_time_now().saturating_sub(start),
            None => std::time::Duration::ZERO,
        }
    }

    fn cancel_tick(&self) {
        if let Some(src) = self.tick_source.borrow_mut().take() {
            src.remove();
        }
        // Don't drop running_label here — cancel_tick is also called
        // from on_pause now, and the running page stays up across
        // pause/resume so the label widget is still valid. Sessions
        // ending (on_stop, end_overtime_session) drop it explicitly.
    }

    pub fn refresh_streak(&self) {
        let Some(app) = self.get_app() else {
            // No app yet (shouldn't happen in practice) — use defaults.
            self.rebuild_starred_presets_list();
            self.refresh_setup_label_chooser_subtitle();
            return;
        };

        // Batch every DB read this visit needs into a single borrow:
        // one get_app() walk, one RefCell lock, four SQL queries instead
        // of as many separate calls. The bells block also rides along —
        // four extra get_setting() calls are cheap next to the existing
        // streak / labels SQL we're already running.
        let (streak, stopwatch_on, bells, intervals) = app
            .with_db(|db| {
                let streak  = db.get_streak().unwrap_or(0);
                let stopwatch_on = db
                    .get_setting("stopwatch_mode_active", "false")
                    .map(|v| v == "true")
                    .unwrap_or(false);
                let starting_bell_on = db
                    .get_setting("starting_bell_active", "false")
                    .map(|v| v == "true")
                    .unwrap_or(false);
                let starting_bell_sound = db
                    .get_setting("starting_bell_sound", "bowl")
                    .unwrap_or_else(|_| "bowl".to_string());
                let prep_on = db
                    .get_setting("preparation_time_active", "false")
                    .map(|v| v == "true")
                    .unwrap_or(false);
                let prep_secs = db
                    .get_setting(
                        "preparation_time_secs",
                        &meditate_core::format::PREP_SECS_DEFAULT.to_string(),
                    )
                    .map(|s| meditate_core::format::parse_prep_secs(&s))
                    .unwrap_or(meditate_core::format::PREP_SECS_DEFAULT);
                let intervals_on = db
                    .get_setting("interval_bells_active", "false")
                    .map(|v| v == "true")
                    .unwrap_or(false);
                let intervals_enabled_count = db
                    .list_interval_bells()
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|b| b.enabled)
                    // Stopwatch mode mutes fixed-from-end bells — no
                    // end to count backwards from. The persisted
                    // enabled flag stays untouched (returns when
                    // stopwatch flips off); the UI subtitle just
                    // reflects what will actually fire right now.
                    .filter(|b| !(stopwatch_on
                        && b.kind == meditate_core::db::IntervalBellKind::FixedFromEnd))
                    .count();
                (
                    streak,
                    stopwatch_on,
                    (starting_bell_on, starting_bell_sound, prep_on, prep_secs),
                    (intervals_on, intervals_enabled_count),
                )
            })
            .unwrap_or_else(|| {
                (
                    0,
                    false,
                    (false, "bowl".to_string(), false, meditate_core::format::PREP_SECS_DEFAULT),
                    (false, 0),
                )
            });

        // Restore the persisted Stopwatch-Mode toggle. The loading guard
        // suppresses the notify::active handler so this read-back doesn't
        // re-persist or fire a sync.
        self.stopwatch_loading.set(true);
        self.stopwatch_mode_row.set_active(stopwatch_on);
        self.stopwatch_toggle_on.set(stopwatch_on);
        self.stopwatch_loading.set(false);
        self.refresh_stopwatch_dependent_ui();

        // Restore bell-related rows. Each ExpanderRow's enable-expansion
        // flag drives both the persisted state and the slide animation;
        // the bells_loading guard prevents the program-driven
        // set_enable_expansion() calls from looking like user toggles
        // and re-writing the same value.
        //
        // We also call set_expanded with the same value — libadwaita
        // auto-mirrors expanded ↔ enable-expansion only on user switch
        // taps, not on programmatic set_enable_expansion. Without this
        // a row whose persisted toggle is on appears collapsed (sub-
        // rows hidden) on first launch / after restart, even though
        // the switch shows on.
        let (starting_bell_on, _starting_bell_sound_legacy, prep_on, prep_secs) = bells;
        self.bells_loading.set(true);
        self.starting_bell_row.set_enable_expansion(starting_bell_on);
        self.starting_bell_row.set_expanded(starting_bell_on);
        // Sound-row subtitle: name resolved from the bell_sounds library
        // by uuid. Empty subtitle if the persisted uuid is stale (e.g.,
        // a wiped DB seed) — the user re-picks via the chooser.
        self.refresh_starting_bell_sound_subtitle();
        self.refresh_starting_bell_pattern_subtitle();
        self.refresh_starting_bell_signal_mode_state();
        self.preparation_time_row.set_enable_expansion(prep_on);
        self.preparation_time_row.set_expanded(prep_on);
        self.preparation_time_secs_row.set_value(prep_secs as f64);
        // Interval-bells master toggle + count subtitle.
        let (intervals_on, intervals_enabled_count) = intervals;
        self.interval_bells_enabled_row.set_enable_expansion(intervals_on);
        self.interval_bells_enabled_row.set_expanded(intervals_on);
        self.interval_bells_row.set_subtitle(&intervals_count_subtitle(intervals_enabled_count));
        self.bells_loading.set(false);

        // Box Breath phase cues — master + four phase rows + every
        // toggle-group active state + every Bell Sound / Pattern
        // subtitle.
        self.refresh_boxbreath_phase_state();

        // Per-mode Cues toggle.
        self.refresh_cues_signal_mode_state();

        // Update streak label. .streak-chip applies text-transform:
        // uppercase, so we keep the source text sentence-case here.
        let text = match streak {
            0 => crate::i18n::gettext("Start your streak today"),
            1 => crate::i18n::gettext("1 day streak"),
            n => crate::i18n::gettext("{n} days streak").replace("{n}", &n.to_string()),
        };
        self.streak_label.set_label(&text);

        // Rebuild visible starred-preset list for the current mode.
        self.rebuild_starred_presets_list();
        // Sync the duration row's value label with the current target.
        let secs = self.countdown_target_secs.get();
        let h = secs / 3600;
        let m = (secs % 3600) / 60;
        self.duration_value_label.set_label(&format!("{h:02}:{m:02}"));

        // End Bell master toggle is restored by refresh_stopwatch_
        // dependent_ui above (it calls refresh_end_bell_dependent_ui,
        // which reads end_bell_active and either applies it or
        // overrides to off when stopwatch mode is on). Just refresh
        // the sound-row subtitle, the pattern-row subtitle, and the
        // signal-mode toggle group's saved state + capability gating.
        self.refresh_end_bell_sound_subtitle();
        self.refresh_end_bell_pattern_subtitle();
        self.refresh_end_bell_signal_mode_state();

        // Rebuild the Setup view's label chooser-row + master toggle
        // from the per-mode persisted state.
        self.apply_preferred_label_for_mode(self.current_mode());
    }

    /// Rebuild the visible starred-preset list — mode-strict, so a
    /// Timer-mode user only ever sees Timer presets here (and same
    /// for Box Breath). Empty list ⇒ a description hint sitting under
    /// the group title prompts the user to Save Settings; non-empty ⇒
    /// the description is cleared and one row per preset is appended,
    /// each tap-to-apply.
    pub fn rebuild_starred_presets_list(&self) {
        // Drop any rows from the previous mode / refresh — Adw groups
        // don't expose a .clear() so we walk the tracking vec.
        for (row, _) in self.starred_preset_rows.borrow_mut().drain(..) {
            self.presets_group.remove(&row);
        }

        let session_mode = match self.current_mode() {
            TimerMode::Timer     => crate::db::SessionMode::Timer,
            TimerMode::Breathing => crate::db::SessionMode::BoxBreath,
            // Guided mode rebuilds via `rebuild_starred_guided_list`
            // instead of this path. on_mode_switched routes them.
            TimerMode::Guided    => return,
        };
        let app_opt = self.get_app();
        let presets = app_opt
            .as_ref()
            .and_then(|app| app.with_db(|db| db.list_starred_presets_for_mode(session_mode)))
            .and_then(|r| r.ok())
            .unwrap_or_default();

        if presets.is_empty() {
            self.presets_group.set_description(Some(
                &crate::i18n::gettext("Tap Save Settings to create your first preset"),
            ));
            return;
        }
        self.presets_group.set_description(None::<&str>);

        // Resolve the labels table once per rebuild so each row's
        // subtitle lookup is O(1) against the in-memory map.
        let label_names: std::collections::HashMap<String, String> = app_opt
            .as_ref()
            .and_then(|app| app.with_db(|db| db.list_labels()))
            .and_then(|r| r.ok())
            .unwrap_or_default()
            .into_iter()
            .map(|l| (l.uuid, l.name))
            .collect();

        let obj = self.obj();
        let mut tracked: Vec<(adw::ActionRow, String)> = Vec::with_capacity(presets.len());
        for p in presets {
            let row = adw::ActionRow::builder()
                .title(&p.name)
                .subtitle(&preset_subtitle(&p, &label_names))
                .activatable(true)
                .build();
            let uuid = p.uuid.clone();
            row.connect_activated(glib::clone!(
                #[weak(rename_to = this)] obj,
                #[strong] uuid,
                move |_| this.imp().on_preset_row_activated(&uuid),
            ));
            self.presets_group.add(&row);
            tracked.push((row, p.uuid));
        }
        *self.starred_preset_rows.borrow_mut() = tracked;
    }

    /// Rebuild the starred-guided-files list under
    /// `guided_files_group`. Mirrors `rebuild_starred_presets_list`
    /// shape — drain the tracking vec, query the DB for starred rows,
    /// rebuild fresh. Tap on a row populates the Selected slot AND
    /// stashes the uuid in `guided_selected_uuid` so the session-save
    /// path can record per-file attribution.
    pub fn rebuild_starred_guided_list(&self) {
        for (row, _) in self.starred_guided_rows.borrow_mut().drain(..) {
            self.guided_files_group.remove(&row);
        }

        let app_opt = self.get_app();
        let files = app_opt
            .as_ref()
            .and_then(|app| app.with_db(|db| db.list_guided_files()))
            .and_then(|r| r.ok())
            .unwrap_or_default()
            .into_iter()
            .filter(|f| f.is_starred)
            .collect::<Vec<_>>();

        if files.is_empty() {
            // Empty-state row inside the group keeps the section
            // visually tall enough that the [Open / Import] buttons
            // above don't feel cramped against the group title.
            // Mirrors the bells.rs empty-state pattern.
            let row = adw::ActionRow::builder()
                .title(crate::i18n::gettext("No starred files"))
                .subtitle(crate::i18n::gettext(
                    "Tap Open File then Import File, or star a file in Manage Files",
                ))
                .activatable(false)
                .selectable(false)
                .build();
            row.add_css_class("dim-label");
            self.guided_files_group.add(&row);
            self.starred_guided_rows.borrow_mut().push((row, String::new()));
            return;
        }

        let mut tracked: Vec<(adw::ActionRow, String)> = Vec::with_capacity(files.len());
        for f in &files {
            let row = adw::ActionRow::builder()
                .title(&f.name)
                .subtitle(crate::guided::format_duration_brief(f.duration_secs))
                .activatable(true)
                .build();
            // Star prefix (always on for the home-list — destarring
            // happens via Manage Files).
            let star = gtk::Image::from_icon_name("starred-symbolic");
            star.add_css_class("preset-star-on");
            row.add_prefix(&star);

            let uuid = f.uuid.clone();
            let name = f.name.clone();
            let path = f.file_path.clone();
            let duration_secs = f.duration_secs;
            let obj = self.obj().clone();
            row.connect_activated(move |_| {
                let imp = obj.imp();
                // Promote this starred row into the Selected slot and
                // record its uuid for the session-save path.
                *imp.guided_selected_uuid.borrow_mut() = Some(uuid.clone());
                *imp.guided_pick.borrow_mut() = Some(crate::guided::GuidedFilePick {
                    display_name: name.clone(),
                    source_path: std::path::PathBuf::from(&path),
                    duration_secs,
                });
                imp.refresh_guided_selected_row();
                imp.refresh_hero_for_idle();
            });

            self.guided_files_group.add(&row);
            tracked.push((row, f.uuid.clone()));
        }
        *self.starred_guided_rows.borrow_mut() = tracked;
    }

    /// Update the Selected row's title/subtitle from the current
    /// `guided_pick` slot — empty state if nothing's picked, file
    /// name + duration otherwise. Also updates the Open File button
    /// label to "Open New File" when a pick is already populated, so
    /// the user understands tapping it replaces the selection.
    pub fn refresh_guided_selected_row(&self) {
        let has_pick = self.guided_pick.borrow().is_some();
        match self.guided_pick.borrow().as_ref() {
            Some(pick) => {
                self.guided_selected_row.set_title(&pick.display_name);
                self.guided_selected_row
                    .set_subtitle(&crate::guided::format_duration_brief(pick.duration_secs));
                self.guided_selected_row.remove_css_class("dim-label");
            }
            None => {
                self.guided_selected_row.set_title(&crate::i18n::gettext("No file selected"));
                self.guided_selected_row.set_subtitle(
                    &crate::i18n::gettext("Tap Open File or pick from list below"),
                );
                self.guided_selected_row.add_css_class("dim-label");
            }
        }
        // Reflect the "you already have a pick — tapping replaces it"
        // semantic in the button label so the affordance is honest.
        self.open_file_btn.set_label(&if has_pick {
            crate::i18n::gettext("Open New File")
        } else {
            crate::i18n::gettext("Open File")
        });
        // Import button is greyed when there's no transient pick OR
        // when the current pick is already a starred library row
        // (selected_uuid Some → already imported).
        let has_transient = has_pick && self.guided_selected_uuid.borrow().is_none();
        self.import_file_btn.set_sensitive(has_transient);
    }

    /// Snapshot the live Setup state into a `PresetConfig`. Reads
    /// from the same persistence points the apply path writes to, so
    /// `apply_config(snapshot_current_setup())` is a round-trip with
    /// no observable change. Used by both the Undo path on
    /// preset-tap (capture pre-apply state) and the future "Save
    /// Settings" chooser flow (capture state to write into a new or
    /// overwritten preset).
    fn snapshot_current_setup(&self) -> crate::preset_config::PresetConfig {
        use crate::preset_config::*;
        let mode = self.current_mode();

        let label = PresetLabel {
            enabled: self.persisted_label_active_for_mode(mode),
            uuid: self.persisted_label_uuid_for_mode(mode),
        };

        // Bell + interval state. Default values mirror the read-back
        // defaults in refresh_streak (starting bell off, prep off, etc.).
        let read_bool = |k: &str, default: bool| -> bool {
            self.get_app()
                .and_then(|app| app.with_db(|db| db.get_setting(
                    k, if default { "true" } else { "false" },
                )))
                .and_then(|r| r.ok())
                .map(|v| v == "true")
                .unwrap_or(default)
        };
        let read_str = |k: &str, default: &str| -> String {
            self.get_app()
                .and_then(|app| app.with_db(|db| db.get_setting(k, default)))
                .and_then(|r| r.ok())
                .unwrap_or_else(|| default.to_string())
        };
        let read_u32 = |k: &str, default: u32| -> u32 {
            read_str(k, &default.to_string()).parse::<u32>().unwrap_or(default)
        };

        let starting_bell = PresetStartingBell {
            enabled: read_bool("starting_bell_active", false),
            sound_uuid: read_str("starting_bell_sound", crate::db::BUNDLED_BOWL_UUID),
            prep_time_enabled: read_bool("preparation_time_active", false),
            prep_time_secs: read_u32(
                "preparation_time_secs",
                meditate_core::format::PREP_SECS_DEFAULT,
            ),
        };

        let end_bell = PresetEndBell {
            enabled: read_bool("end_bell_active", true),
            sound_uuid: read_str("end_bell_sound", crate::db::BUNDLED_BOWL_UUID),
        };

        let intervals_enabled = read_bool("interval_bells_active", false);
        let bells: Vec<PresetIntervalBell> = self.get_app()
            .and_then(|app| app.with_db(|db| db.list_interval_bells()))
            .and_then(|r| r.ok())
            .unwrap_or_default()
            .into_iter()
            .map(|b| PresetIntervalBell {
                kind: b.kind.as_db_str().to_string(),
                minutes: b.minutes,
                jitter_pct: b.jitter_pct,
                sound_uuid: b.sound,
                enabled: b.enabled,
            })
            .collect();
        let interval_bells = PresetIntervalBells {
            enabled: intervals_enabled,
            bells,
        };

        let timing = match mode {
            TimerMode::Timer => PresetTiming::Timer {
                stopwatch: self.stopwatch_toggle_on.get(),
                duration_secs: self.countdown_target_secs.get() as u32,
            },
            TimerMode::Breathing => {
                let p = self.breathing_pattern.get();
                PresetTiming::BoxBreath {
                    inhale_secs:    p.in_secs,
                    hold_full_secs: p.hold_in,
                    exhale_secs:    p.out_secs,
                    hold_empty_secs:p.hold_out,
                    duration_secs:  self.breathing_session_secs.get(),
                }
            }
            // Snapshot is unreachable in Guided (Save Settings button
            // is hidden + early-returns above). Synthesise a Timer-
            // shaped value just to satisfy the match — never read.
            TimerMode::Guided => PresetTiming::Timer {
                stopwatch: false,
                duration_secs: 0,
            },
        };

        PresetConfig { label, starting_bell, interval_bells, end_bell, timing }
    }

    /// Apply a `PresetConfig` to the live Setup state. Replays the
    /// config into every persistence point — per-mode settings, the
    /// interval-bell library (DELETE-ALL + re-INSERT from snapshot),
    /// the breath-pattern Cells, and the countdown target — then
    /// triggers refresh_streak so the on-screen rows converge.
    ///
    /// Returns true iff the apply happened. Returns false when a
    /// referenced bell sound hasn't arrived locally yet (sync-pending);
    /// callers can decide how to surface that to the user.
    fn apply_config(&self, cfg: &crate::preset_config::PresetConfig) -> bool {
        use crate::preset_config::PresetTiming;
        let Some(app) = self.get_app() else { return false; };

        // Sync sound-uuid lookups: a preset synced from another
        // device may reference a bell sound that hasn't arrived yet
        // through the WebDAV layer. Refuse to apply and let the
        // caller decide how to message it.
        let known_sound_uuids: std::collections::HashSet<String> = app
            .with_db(|db| db.list_bell_sounds())
            .and_then(|r| r.ok())
            .map(|sounds| sounds.into_iter().map(|s| s.uuid).collect())
            .unwrap_or_default();
        let mut needs_sound = Vec::<&str>::new();
        if cfg.starting_bell.enabled { needs_sound.push(&cfg.starting_bell.sound_uuid); }
        if cfg.end_bell.enabled      { needs_sound.push(&cfg.end_bell.sound_uuid); }
        for b in &cfg.interval_bells.bells {
            needs_sound.push(&b.sound_uuid);
        }
        if needs_sound.iter().any(|u| !known_sound_uuids.contains(*u)) {
            return false;
        }

        let mode = self.current_mode();
        let stopwatch_active = matches!(
            cfg.timing, PresetTiming::Timer { stopwatch: true, .. }
        );

        // Persist settings
        let label_uuid_opt = cfg.label.uuid.clone();
        let label_active = cfg.label.enabled;
        let cfg_owned = cfg.clone();
        app.with_db_mut(|db| {
            let _ = db.set_setting(
                label_active_setting_key(mode),
                if label_active { "true" } else { "false" },
            );
            if let Some(luuid) = label_uuid_opt.as_ref() {
                let _ = db.set_setting(label_uuid_setting_key(mode), luuid);
            }
            let _ = db.set_setting(
                "starting_bell_active",
                if cfg_owned.starting_bell.enabled { "true" } else { "false" },
            );
            if !cfg_owned.starting_bell.sound_uuid.is_empty() {
                let _ = db.set_setting("starting_bell_sound", &cfg_owned.starting_bell.sound_uuid);
            }
            let _ = db.set_setting(
                "preparation_time_active",
                if cfg_owned.starting_bell.prep_time_enabled { "true" } else { "false" },
            );
            let _ = db.set_setting(
                "preparation_time_secs",
                &cfg_owned.starting_bell.prep_time_secs.to_string(),
            );
            let _ = db.set_setting(
                "interval_bells_active",
                if cfg_owned.interval_bells.enabled { "true" } else { "false" },
            );
            let _ = db.set_setting(
                "end_bell_active",
                if cfg_owned.end_bell.enabled { "true" } else { "false" },
            );
            if !cfg_owned.end_bell.sound_uuid.is_empty() {
                let _ = db.set_setting("end_bell_sound", &cfg_owned.end_bell.sound_uuid);
            }
            let _ = db.set_setting(
                "stopwatch_mode_active",
                if stopwatch_active { "true" } else { "false" },
            );
        });

        // Replace interval-bell library from the snapshot.
        let snapshot_bells = cfg.interval_bells.bells.clone();
        app.with_db_mut(|db| {
            let existing = db.list_interval_bells().unwrap_or_default();
            for b in &existing {
                let _ = db.delete_interval_bell(&b.uuid);
            }
            for s in &snapshot_bells {
                let kind = match s.kind.as_str() {
                    "interval"          => crate::db::IntervalBellKind::Interval,
                    "fixed_from_start"  => crate::db::IntervalBellKind::FixedFromStart,
                    "fixed_from_end"    => crate::db::IntervalBellKind::FixedFromEnd,
                    _ => continue,
                };
                let rowid = match db.insert_interval_bell(
                    kind, s.minutes, s.jitter_pct, &s.sound_uuid,
                    crate::db::BUNDLED_PATTERN_PULSE_UUID,
                    crate::db::SignalMode::Sound,
                ) {
                    Ok(id) => id,
                    Err(_) => continue,
                };
                if !s.enabled {
                    if let Some(b) = db.list_interval_bells()
                        .ok()
                        .and_then(|bs| bs.into_iter().find(|b| b.id == rowid))
                    {
                        let _ = db.set_interval_bell_enabled(&b.uuid, false);
                    }
                }
            }
        });

        // Apply mode-specific live state.
        match cfg.timing {
            PresetTiming::Timer { stopwatch, duration_secs } => {
                self.set_countdown_target(duration_secs as u64);
                self.stopwatch_loading.set(true);
                self.stopwatch_mode_row.set_active(stopwatch);
                self.stopwatch_toggle_on.set(stopwatch);
                self.stopwatch_loading.set(false);
            }
            PresetTiming::BoxBreath {
                inhale_secs, hold_full_secs, exhale_secs, hold_empty_secs,
                duration_secs,
            } => {
                self.breathing_pattern.set(BreathPattern {
                    in_secs:  inhale_secs,
                    hold_in:  hold_full_secs,
                    out_secs: exhale_secs,
                    hold_out: hold_empty_secs,
                });
                self.set_breathing_duration_secs(duration_secs);
                self.refresh_phase_tiles();
            }
        }

        // Refresh dependent UI in one round.
        self.refresh_streak();
        true
    }

    /// Tap-to-apply on a starred preset row. Snapshots pre-apply
    /// state so a follow-up Undo button on the toast can put things
    /// back where they were. Mode-strict: cross-mode application is
    /// rejected defensively (the visible list is mode-filtered, but
    /// a sync race could still surface a stale row); the mode toggle
    /// is never side-effected from a tap.
    fn on_preset_row_activated(&self, uuid: &str) {
        use crate::preset_config::PresetConfig;

        let Some(app) = self.get_app() else { return; };
        let preset = match app.with_db(|db| db.find_preset_by_uuid(uuid)) {
            Some(Ok(Some(p))) => p,
            _ => return,
        };
        let cfg = match PresetConfig::from_json(&preset.config_json) {
            Ok(c) => c,
            Err(_) => return,
        };
        let want_session_mode = match self.current_mode() {
            TimerMode::Timer     => crate::db::SessionMode::Timer,
            TimerMode::Breathing => crate::db::SessionMode::BoxBreath,
            // Preset rows aren't surfaced in Guided mode; this would
            // only fire from a stale callback retained across a mode
            // switch. Refuse rather than mutating Setup state.
            TimerMode::Guided    => return,
        };
        if preset.mode != want_session_mode {
            return;
        }

        let snapshot = self.snapshot_current_setup();
        if !self.apply_config(&cfg) {
            self.toast(&crate::i18n::gettext(
                "Please wait until fully synced — not all bell sounds have arrived",
            ));
            return;
        }

        // Toast with Undo. The action callback re-applies the
        // pre-apply snapshot, putting every persistence point back to
        // its previous value (including a destructive interval-bells
        // round-trip — same DB cost as the forward apply, accepted
        // for what's already a low-frequency action).
        //
        // Dismiss any previous apply toast first so a quick second
        // tap shows the new "applied" message without waiting through
        // the queue — see `current_apply_toast` for the rationale.
        // Bind the previous toast into a local *before* calling
        // dismiss(): GTK fires the dismissed signal synchronously,
        // so a `take()` on the borrow_mut would still hold the
        // RefCell guard across the callback's own borrow_mut and
        // panic with "already borrowed".
        let prev_toast = self.current_apply_toast.replace(None);
        if let Some(prev) = prev_toast {
            prev.dismiss();
        }
        let toast = adw::Toast::builder()
            .title(crate::i18n::gettext("'{name}' applied")
                .replace("{name}", &preset.name))
            .button_label(crate::i18n::gettext("Undo"))
            .build();
        let obj = self.obj().clone();
        toast.connect_button_clicked(move |_| {
            obj.imp().apply_config(&snapshot);
        });
        // Clear the cached handle when the toast finishes (queue exit
        // / explicit dismiss / button click) — otherwise a long-lived
        // strong reference would outlive the toast on the overlay.
        // Two-step borrow (read then write) so the read-only borrow
        // doesn't span the assignment, matching the dismiss-during-
        // callback rule.
        let obj_for_dismiss = self.obj().clone();
        toast.connect_dismissed(move |t| {
            let imp = obj_for_dismiss.imp();
            let should_clear = imp.current_apply_toast
                .borrow()
                .as_ref()
                .map(|cur| cur == t)
                .unwrap_or(false);
            if should_clear {
                imp.current_apply_toast.replace(None);
            }
        });
        self.current_apply_toast.replace(Some(toast.clone()));
        if let Some(window) = self.obj().root().and_downcast::<crate::window::MeditateWindow>() {
            window.add_toast(toast);
        }
    }

    /// Push a plain (no-action) toast onto the window's overlay.
    fn toast(&self, message: &str) {
        if let Some(window) = self.obj().root().and_downcast::<crate::window::MeditateWindow>() {
            window.add_toast(adw::Toast::new(message));
        }
    }

    /// Update the countdown target + hero label + duration row suffix.
    fn set_countdown_target(&self, secs: u64) {
        self.countdown_target_secs.set(secs);
        let h = secs / 3600;
        let m = (secs % 3600) / 60;
        self.big_time_label.set_label(&format!("{h:02}:{m:02}"));
        self.duration_value_label.set_label(&format!("{h:02}:{m:02}"));
    }

    /// Show the H:M spin-button dialog; apply on Set. Same shape in
    /// both modes — both store seconds internally; the H:M dialog
    /// reads / writes minute-aligned values, multiplied by 60 on the
    /// way in. Both modes share the same 0-23 hour / 0-59 minute
    /// spinner ranges.
    fn show_custom_time_dialog(&self) {
        let mode = self.current_mode();
        // Duration row is hidden in Guided mode; this dialog can't
        // be reached from there. Bail out defensively.
        if mode == TimerMode::Guided {
            return;
        }
        let (cur_h, cur_m) = match mode {
            TimerMode::Timer => {
                let s = self.countdown_target_secs.get();
                ((s / 3600) as f64, ((s % 3600) / 60) as f64)
            }
            TimerMode::Breathing => {
                let m = self.breathing_session_secs.get() / 60;
                ((m / 60) as f64, (m % 60) as f64)
            }
            TimerMode::Guided => unreachable!("guarded above"),
        };

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
        let mode_for_response = mode;
        dialog.connect_response(None, move |_, response| {
            if response != "set" { return; }
            let h = hours_spin.value() as u64;
            let m = minutes_spin.value() as u64;
            let total_mins = h * 60 + m;
            if total_mins == 0 { return; }
            match mode_for_response {
                TimerMode::Timer => {
                    obj.imp().set_countdown_target(total_mins * 60);
                }
                TimerMode::Breathing => {
                    obj.imp().set_breathing_duration_secs((total_mins * 60) as u32);
                }
                TimerMode::Guided => {} // unreachable per show_custom_time_dialog guard
            }
        });

        if let Some(win) = self.obj().root().and_then(|r| r.downcast::<gtk::Window>().ok()) {
            dialog.present(Some(&win));
        }
    }

    /// Resolve the label currently configured for `mode`. Reads
    /// `default_label_uuid_<mode>`, falls back to the mode-default
    /// uuid (Meditation / Box-Breathing) when the setting is unset
    /// or empty. Returns `None` only when even the mode-default row
    /// has been deleted by the user.
    fn resolve_label_for_mode(&self, mode: TimerMode) -> Option<Label> {
        let app = self.get_app()?;
        let uuid = self
            .persisted_label_uuid_for_mode(mode)
            .unwrap_or_else(|| self.mode_default_label_uuid(mode).to_string());
        if uuid.is_empty() {
            return None;
        }
        app.with_db(|db| db.list_labels())
            .and_then(|r| r.ok())
            .unwrap_or_default()
            .into_iter()
            .find(|l| l.uuid == uuid)
    }

    /// Returns the label currently configured for the active Setup
    /// view — `None` when the master toggle is off OR the resolved
    /// row no longer exists.
    fn setup_selected_label_id(&self) -> Option<i64> {
        let mode = self.current_mode();
        if !self.persisted_label_active_for_mode(mode) {
            return None;
        }
        self.resolve_label_for_mode(mode).map(|l| l.id)
    }

    /// Refresh the Setup-view label chooser-row's subtitle to show
    /// the currently-resolved label name (or a hint when the toggle
    /// is off / the mode-default row has been deleted).
    fn refresh_setup_label_chooser_subtitle(&self) {
        let mode = self.current_mode();
        let active = self.persisted_label_active_for_mode(mode);
        // The chooser-row sits inside the ExpanderRow's expansion
        // body; its visibility tracks the toggle automatically.
        // We still update its subtitle so it's correct the moment
        // the user expands the row.
        let subtitle = if active {
            self.resolve_label_for_mode(mode)
                .map(|l| l.name)
                .unwrap_or_else(|| crate::i18n::gettext("(none — pick one)"))
        } else {
            crate::i18n::gettext("Off")
        };
        self.setup_label_chooser_row.set_subtitle(&subtitle);

        // Also keep the ExpanderRow's switch state in sync without
        // re-firing the persist callback.
        self.labels_loading.set(true);
        self.setup_label_enabled_row.set_enable_expansion(active);
        self.labels_loading.set(false);
    }

    /// Refresh the Done-view label chooser-row's subtitle from the
    /// current `done_selected_label_id` state.
    fn refresh_done_label_chooser_subtitle(&self) {
        let app = self.get_app();
        let id = self.done_selected_label_id.get();
        let labels = app
            .as_ref()
            .and_then(|a| a.with_db(|db| db.list_labels()))
            .and_then(|r| r.ok())
            .unwrap_or_default();
        let subtitle = id
            .and_then(|id| labels.iter().find(|l| l.id == id).map(|l| l.name.clone()))
            .unwrap_or_else(|| {
                if id.is_some() {
                    crate::i18n::gettext("(none — pick one)")
                } else {
                    crate::i18n::gettext("Off")
                }
            });
        self.done_label_chooser_row.set_subtitle(&subtitle);

        // Keep the ExpanderRow's switch state in sync with the
        // selected-id state without re-firing the toggle callback.
        self.labels_loading.set(true);
        self.done_label_enabled_row.set_enable_expansion(id.is_some());
        self.labels_loading.set(false);
    }

    fn get_app(&self) -> Option<crate::application::MeditateApplication> {
        self.obj()
            .root()
            .and_then(|r| r.downcast::<gtk::Window>().ok())
            .and_then(|w| w.application())
            .and_then(|a| a.downcast::<crate::application::MeditateApplication>().ok())
    }

    pub fn current_display_secs(&self) -> u64 {
        // While in (or paused from) prep, the hero shows the prep
        // remaining — the real cores haven't been wired up yet.
        if let Some(prep) = self.prep_stopwatch.borrow().as_ref() {
            let elapsed = prep.elapsed(self.elapsed_since_start());
            let remaining = self.prep_target.get().saturating_sub(elapsed);
            return remaining.as_secs() + (remaining.subsec_nanos() > 0) as u64;
        }
        // Return the display value for whichever mode is about to go running.
        match self.tick_mode.get() {
            TimerMode::Timer => {
                if self.stopwatch_toggle_on.get() {
                    self.stopwatch_elapsed_secs()
                } else {
                    self.countdown_remaining_secs()
                }
            }
            TimerMode::Breathing => self.breath_elapsed().as_secs(),
            // Guided mode counts down from the file's natural length;
            // same shape as Timer countdown.
            TimerMode::Guided => self.countdown_remaining_secs(),
        }
    }

    pub fn set_running_label(&self, label: gtk::Label) {
        *self.running_label.borrow_mut() = Some(label);
    }

    /// Both modes (timer + breathing) call this so on_pause /
    /// on_resume can morph the label in place.
    pub fn set_running_pause_btn(&self, btn: gtk::Button) {
        *self.running_pause_btn.borrow_mut() = Some(btn);
    }

    /// Timer-mode only — these are needed for the Overtime
    /// transition (Stop hidden, "Add MM:SS ?" revealed).
    pub fn set_running_overtime_widgets(
        &self,
        stop_btn: gtk::Button,
        add_btn: gtk::Button,
    ) {
        *self.running_stop_btn.borrow_mut() = Some(stop_btn);
        *self.overtime_add_btn.borrow_mut() = Some(add_btn);
    }

    pub fn toggle_playback(&self) {
        match self.timer_state.get() {
            TimerState::Idle      => self.on_start(),
            TimerState::Preparing => self.on_pause(),
            TimerState::Running   => self.on_pause(),
            TimerState::Overtime  => self.finish_overtime_session(),
            TimerState::Paused    => self.on_resume(),
            TimerState::Done      => {}
        }
    }
}

// ── Interval / fixed bell scheduling ─────────────────────────────────────────

impl TimerView {
    /// Build the per-session bell schedule from the library + the
    /// current session's parameters. Called at the moment the session
    /// enters Running (in on_start when prep is off, or in
    /// transition_prep_to_running when prep ends). No-op writes to
    /// active_bells when interval_bells_active is off — the empty
    /// vec means the per-tick check has nothing to do.
    fn load_active_bells_for_running(&self) {
        use meditate_core::db::IntervalBellKind as Kind;
        let mut new_bells: Vec<ActiveBell> = Vec::new();
        let Some(app) = self.get_app() else {
            *self.active_bells.borrow_mut() = new_bells;
            return;
        };
        let active = app
            .with_db(|db| {
                db.get_setting("interval_bells_active", "false")
                    .map(|v| v == "true")
                    .unwrap_or(false)
            })
            .unwrap_or(false);
        if !active {
            *self.active_bells.borrow_mut() = new_bells;
            return;
        }

        let stopwatch_on = self.stopwatch_toggle_on.get();
        let total_target_secs: Option<u64> = if stopwatch_on {
            None
        } else {
            Some(self.countdown_target_secs.get())
        };

        let bells = app
            .with_db(|db| db.list_interval_bells())
            .and_then(|r| r.ok())
            .unwrap_or_default();
        for b in bells {
            if !b.enabled {
                continue;
            }
            // Stopwatch mode mutes fixed-from-end bells — no end to
            // count backwards from. Mirrors the UI grey-out.
            if stopwatch_on && b.kind == Kind::FixedFromEnd {
                continue;
            }
            let schedule = match b.kind {
                Kind::Interval => {
                    let r = self.next_random_unit();
                    let next_ring = meditate_core::format::next_interval_ring_secs(
                        0, b.minutes, b.jitter_pct, r,
                    );
                    BellSchedule::Interval {
                        base_min: b.minutes,
                        jitter_pct: b.jitter_pct,
                        next_ring_secs: next_ring,
                    }
                }
                Kind::FixedFromStart => {
                    match meditate_core::format::fixed_from_start_target_secs(
                        b.minutes, total_target_secs,
                    ) {
                        Some(t) => BellSchedule::Fixed { target_secs: t, fired: false },
                        None => continue,
                    }
                }
                Kind::FixedFromEnd => {
                    let Some(total) = total_target_secs else { continue; };
                    match meditate_core::format::fixed_from_end_target_secs(
                        b.minutes, total,
                    ) {
                        Some(t) => BellSchedule::Fixed { target_secs: t, fired: false },
                        None => continue,
                    }
                }
            };
            new_bells.push(ActiveBell {
                sound: b.sound,
                vibration_pattern_uuid: b.vibration_pattern_uuid,
                signal_mode: b.signal_mode,
                schedule,
            });
        }
        *self.active_bells.borrow_mut() = new_bells;
    }

    /// Per-tick check: fire any bell whose ring boundary has been
    /// crossed since the previous tick. Interval bells reroll their
    /// next ring; fixed bells flip their fired flag so they don't
    /// re-fire on subsequent ticks. `elapsed_secs` is the running
    /// session's elapsed-secs (post-prep).
    fn fire_due_bells_at(&self, elapsed_secs: u64) {
        if self.active_bells.borrow().is_empty() {
            return;
        }
        // Collect-then-fire pattern so we don't keep the RefCell
        // borrowed across the play_interval_sound call (sound.rs uses
        // its own thread-locals; no recursion expected, but the
        // collect is also clearer).
        let mut to_play: Vec<(String, String, crate::db::SignalMode)> = Vec::new();
        let mut bells = self.active_bells.borrow_mut();
        for bell in bells.iter_mut() {
            let mut should_fire = false;
            match &mut bell.schedule {
                BellSchedule::Interval { base_min, jitter_pct, next_ring_secs } => {
                    if elapsed_secs >= *next_ring_secs {
                        should_fire = true;
                        let r = self.next_random_unit();
                        *next_ring_secs = meditate_core::format::next_interval_ring_secs(
                            *next_ring_secs, *base_min, *jitter_pct, r,
                        );
                    }
                }
                BellSchedule::Fixed { target_secs, fired } => {
                    if !*fired && elapsed_secs >= *target_secs {
                        should_fire = true;
                        *fired = true;
                    }
                }
            }
            if should_fire {
                to_play.push((
                    bell.sound.clone(),
                    bell.vibration_pattern_uuid.clone(),
                    bell.signal_mode,
                ));
            }
        }
        drop(bells);
        let Some(app) = self.get_app() else { return; };
        let mode_key = setting_key_for_mode(self.current_mode());
        for (sound_uuid, pattern_uuid, signal_mode) in to_play {
            // Sound channel — gate by per-bell + per-mode signal_mode.
            if crate::vibration::should_fire_sound(&app, signal_mode, mode_key) {
                crate::sound::play_interval_sound(&sound_uuid, &app);
            }
            // Vibration channel — same two-gate AND. Replace any
            // previous handle so newest-wins on overlapping bells.
            // install_vibration_handle disarms the old before
            // replacing — feedbackd supersedes per-app, so the
            // explicit cancel would race the new pattern.
            let handle = crate::vibration::fire_pattern_if_allowed(
                &app, signal_mode, mode_key, &pattern_uuid,
            );
            crate::diag::log(&format!(
                "fire_interval_bell: per_bell={} mode={} fired={}",
                signal_mode.as_db_str(), mode_key, handle.is_some()
            ));
            self.install_vibration_handle(handle);
        }
    }

    /// Lazy-seeded xorshift64 → unit-uniform f64 in [0, 1). Quality is
    /// fine for "shake bell timing slightly" — we're not doing crypto.
    /// Seeded once from wall-clock nanos on first use; subsequent
    /// rolls are well-distributed for the lifetime of the process.
    fn next_random_unit(&self) -> f64 {
        let mut s = self.bell_rng_state.get();
        if s == 0 {
            s = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(1)
                .max(1);
        }
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        self.bell_rng_state.set(s);
        // Top 53 bits → f64 in [0, 1) without losing precision.
        (s >> 11) as f64 / (1u64 << 53) as f64
    }
}

// ── Public refresh hooks ─────────────────────────────────────────────────────

impl TimerView {
    /// Refresh just the "Manage Bells" subtitle. Called when the user
    /// pops back from the bell-library page so the count stays in sync
    /// without us having to invalidate the whole streak/presets/labels
    /// read in refresh_streak.
    pub(crate) fn refresh_interval_bells_count(&self) {
        let count = self
            .get_app()
            .and_then(|app| {
                app.with_db(|db| {
                    let stopwatch_on = db
                        .get_setting("stopwatch_mode_active", "false")
                        .map(|v| v == "true")
                        .unwrap_or(false);
                    db.list_interval_bells()
                        .map(|bells| bells.into_iter()
                            .filter(|b| b.enabled)
                            .filter(|b| !(stopwatch_on
                                && b.kind == meditate_core::db::IntervalBellKind::FixedFromEnd))
                            .count())
                        .unwrap_or(0)
                })
            })
            .unwrap_or(0);
        self.interval_bells_row.set_subtitle(&intervals_count_subtitle(count));
    }

    /// Refresh the subtitle of the Starting Bell sound row to the
    /// human-readable name of whichever bell_sounds row the persisted
    /// uuid points at. Empty if the uuid is stale (post-wipe legacy
    /// value) — the user re-picks via the chooser to fix.
    pub(crate) fn refresh_starting_bell_sound_subtitle(&self) {
        let name = self.lookup_sound_name_for_setting("starting_bell_sound");
        self.starting_bell_sound_row.set_subtitle(&name);
    }

    /// Same for End Bell.
    pub(crate) fn refresh_end_bell_sound_subtitle(&self) {
        let name = self.lookup_sound_name_for_setting("end_bell_sound");
        self.end_bell_sound_row.set_subtitle(&name);
    }

    /// End-bell pattern row's subtitle reflects whichever
    /// vibration_patterns row the end_bell_pattern setting points at.
    /// Defaults to bundled Pulse on first ever read.
    /// Same dual-channel firing for the Starting Bell. Reads
    /// starting_bell_active + starting_bell_signal_mode +
    /// starting_bell_pattern + the per-mode override; ignores the
    /// row in modes where Starting Bell isn't shown (Box Breath /
    /// Guided — the caller already gates by mode before calling
    /// this, but defensive double-check is cheap).
    pub(crate) fn fire_starting_bell(
        &self,
        app: &crate::application::MeditateApplication,
    ) {
        let active = app
            .with_db(|db| db.get_setting("starting_bell_active", "false"))
            .and_then(|r| r.ok())
            .map(|s| s == "true")
            .unwrap_or(false);
        if !active { return; }

        let raw = app
            .with_db(|db| db.get_setting("starting_bell_signal_mode", "sound"))
            .and_then(|r| r.ok())
            .unwrap_or_else(|| "sound".to_string());
        let per_bell = crate::db::SignalMode::from_db_str(&raw)
            .unwrap_or(crate::db::SignalMode::Sound);
        let mode_key = setting_key_for_mode(self.current_mode());

        if crate::vibration::should_fire_sound(app, per_bell, mode_key) {
            crate::sound::play_starting_sound(app);
        }
        let pattern_uuid = app
            .with_db(|db| db.get_setting(
                "starting_bell_pattern",
                crate::db::BUNDLED_PATTERN_PULSE_UUID,
            ))
            .and_then(|r| r.ok())
            .unwrap_or_else(|| crate::db::BUNDLED_PATTERN_PULSE_UUID.to_string());
        let handle = crate::vibration::fire_pattern_if_allowed(
            app, per_bell, mode_key, &pattern_uuid,
        );
        crate::diag::log(&format!(
            "fire_starting_bell: per_bell={} mode={} fired={}",
            per_bell.as_db_str(), mode_key, handle.is_some()
        ));
        self.install_vibration_handle(handle);
    }

    /// Fire the End Bell's sound + vibration channels per the
    /// current per-bell signal_mode + per-mode override. Both gates
    /// ANDed: per-bell intent (end_bell_signal_mode) AND per-mode
    /// override (timer / guided / boxbreath signal_mode setting).
    /// The vibration handle stashes onto `current_vibration` so the
    /// pattern plays out (drop fires cancel).
    pub(crate) fn fire_end_bell(
        &self,
        app: &crate::application::MeditateApplication,
    ) {
        let active = app
            .with_db(|db| db.get_setting("end_bell_active", "true"))
            .and_then(|r| r.ok())
            .map(|s| s == "true")
            .unwrap_or(true);
        if !active { return; }

        let raw = app
            .with_db(|db| db.get_setting("end_bell_signal_mode", "sound"))
            .and_then(|r| r.ok())
            .unwrap_or_else(|| "sound".to_string());
        let per_bell = crate::db::SignalMode::from_db_str(&raw)
            .unwrap_or(crate::db::SignalMode::Sound);
        let mode_key = setting_key_for_mode(self.current_mode());

        if crate::vibration::should_fire_sound(app, per_bell, mode_key) {
            crate::sound::play_end_bell(app);
        }
        let pattern_uuid = app
            .with_db(|db| db.get_setting(
                "end_bell_pattern",
                crate::db::BUNDLED_PATTERN_PULSE_UUID,
            ))
            .and_then(|r| r.ok())
            .unwrap_or_else(|| crate::db::BUNDLED_PATTERN_PULSE_UUID.to_string());
        let handle = crate::vibration::fire_pattern_if_allowed(
            app, per_bell, mode_key, &pattern_uuid,
        );
        crate::diag::log(&format!(
            "fire_end_bell: per_bell={} mode={} fired={}",
            per_bell.as_db_str(), mode_key, handle.is_some()
        ));
        // Same-app Vibrate replaces in-flight, so disarm-then-replace
        // (no explicit cancel) avoids the cancel-races-the-new-pattern
        // bug. End-of-session pattern cleanly supersedes any
        // interval-bell or phase pattern that was mid-playback.
        self.install_vibration_handle(handle);
    }

    /// Fire the configured cue for a Box-Breath phase boundary.
    /// Gated by the master `boxbreath_cues_active` switch and the
    /// individual phase row's `enabled` flag — both must be on.
    /// Resolution then matches every other bell: per-phase
    /// signal_mode AND'd with the per-mode override
    /// (`boxbreath_signal_mode`) for the sound + pattern channels.
    pub(crate) fn fire_box_breath_phase_cue(
        &self,
        app: &crate::application::MeditateApplication,
        phase: crate::db::BoxBreathPhaseId,
    ) {
        let master_on = app
            .with_db(|db| db.get_setting("boxbreath_cues_active", "false"))
            .and_then(|r| r.ok())
            .map(|s| s == "true")
            .unwrap_or(false);
        if !master_on { return; }

        let row = match app
            .with_db(|db| db.get_box_breath_phase(phase))
            .and_then(|r| r.ok())
            .flatten()
        {
            Some(p) => p,
            None => return,
        };
        if !row.enabled { return; }

        let mode_key = "boxbreath_signal_mode";
        if crate::vibration::should_fire_sound(app, row.signal_mode, mode_key) {
            crate::sound::play_interval_sound(&row.sound_uuid, app);
        }
        let handle = crate::vibration::fire_pattern_if_allowed(
            app, row.signal_mode, mode_key, &row.pattern_uuid,
        );
        crate::diag::log(&format!(
            "fire_box_breath_phase_cue: phase={} per_phase={} fired={}",
            phase.as_db_str(), row.signal_mode.as_db_str(), handle.is_some()
        ));
        self.install_vibration_handle(handle);
    }

    /// Replace the current PatternPlayback handle. Disarms the old
    /// handle's Drop-cancel — a same-app `Vibrate(...)` already
    /// supersedes the previous in-flight pattern at feedbackd, so
    /// the explicit cancel would race behind the new pattern's
    /// call_future and silently kill it. Stash a None to clear the
    /// slot WITH cancel (e.g. session stopped manually).
    pub(crate) fn install_vibration_handle(
        &self,
        handle: Option<crate::vibration::PatternPlayback>,
    ) {
        let mut slot = self.current_vibration.borrow_mut();
        if handle.is_some() {
            if let Some(mut old) = slot.take() {
                old.disarm();
            }
        }
        *slot = handle;
    }

    pub(crate) fn refresh_end_bell_pattern_subtitle(&self) {
        let name = self.lookup_pattern_name_for_setting("end_bell_pattern");
        self.end_bell_pattern_row.set_subtitle(&name);
    }

    /// Same for Starting Bell pattern row.
    pub(crate) fn refresh_starting_bell_pattern_subtitle(&self) {
        let name = self.lookup_pattern_name_for_setting("starting_bell_pattern");
        self.starting_bell_pattern_row.set_subtitle(&name);
    }

    fn lookup_sound_name_for_setting(&self, setting_key: &str) -> String {
        let Some(app) = self.get_app() else { return String::new(); };
        let uuid = app
            .with_db(|db| db.get_setting(setting_key, crate::db::BUNDLED_BOWL_UUID))
            .and_then(|r| r.ok())
            .unwrap_or_default();
        if uuid.is_empty() {
            return String::new();
        }
        app.with_db(|db| db.list_bell_sounds())
            .and_then(|r| r.ok())
            .unwrap_or_default()
            .into_iter()
            .find(|s| s.uuid == uuid)
            .map(|s| s.name)
            .unwrap_or_default()
    }

    fn lookup_pattern_name_for_setting(&self, setting_key: &str) -> String {
        let Some(app) = self.get_app() else { return String::new(); };
        let uuid = app
            .with_db(|db| db.get_setting(
                setting_key,
                crate::db::BUNDLED_PATTERN_PULSE_UUID,
            ))
            .and_then(|r| r.ok())
            .unwrap_or_default();
        if uuid.is_empty() {
            return String::new();
        }
        app.with_db(|db| db.find_vibration_pattern_by_uuid(&uuid))
            .and_then(|r| r.ok())
            .flatten()
            .map(|p| p.name)
            .unwrap_or_default()
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Subtitle text for the "Manage Bells" row reflecting how many of the
/// library's bells are currently enabled. Uses gettext so the count
/// can be localised; "None" is its own string for grammatical reasons
/// in some languages.
fn intervals_count_subtitle(enabled_count: usize) -> String {
    match enabled_count {
        0 => crate::i18n::gettext("None enabled"),
        1 => crate::i18n::gettext("1 enabled"),
        n => crate::i18n::gettext("{n} enabled").replace("{n}", &n.to_string()),
    }
}

/// One-line subtitle for a preset row in the home-view starred list.
/// Composes timing + label + interval-bell count, matching the
/// chooser's subtitle for visual consistency. `label_names` is a
/// uuid → name map already resolved by the caller (one DB roundtrip
/// per rebuild instead of per row).
fn preset_subtitle(
    p: &meditate_core::db::Preset,
    label_names: &std::collections::HashMap<String, String>,
) -> String {
    use crate::preset_config::{PresetConfig, PresetTiming};
    let cfg = match PresetConfig::from_json(&p.config_json) {
        Ok(c) => c,
        Err(_) => return String::new(),
    };
    let mut parts: Vec<String> = Vec::new();
    match cfg.timing {
        PresetTiming::Timer { stopwatch: true, .. } => {
            parts.push(crate::i18n::gettext("Stopwatch"));
        }
        PresetTiming::Timer { stopwatch: false, duration_secs } => {
            let mins = duration_secs / 60;
            parts.push(crate::i18n::gettext("{n} min")
                .replace("{n}", &mins.to_string()));
        }
        PresetTiming::BoxBreath {
            inhale_secs, hold_full_secs, exhale_secs, hold_empty_secs,
            duration_secs,
        } => {
            parts.push(format!(
                "{}-{}-{}-{}",
                inhale_secs, hold_full_secs, exhale_secs, hold_empty_secs,
            ));
            let mins = duration_secs / 60;
            parts.push(crate::i18n::gettext("{n} min")
                .replace("{n}", &mins.to_string()));
        }
    }
    if cfg.label.enabled {
        if let Some(uuid) = cfg.label.uuid.as_ref() {
            if let Some(name) = label_names.get(uuid) {
                parts.push(name.clone());
            }
        }
    }
    if cfg.interval_bells.enabled && !cfg.interval_bells.bells.is_empty() {
        let n = cfg.interval_bells.bells.len();
        parts.push(if n == 1 {
            crate::i18n::gettext("1 bell")
        } else {
            crate::i18n::gettext("{n} bells").replace("{n}", &n.to_string())
        });
    }
    parts.join(" · ")
}

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

/// Minimum cycle length we allow — prevents a 0-0-0-0 pattern from ever
/// reaching the running view, which would panic phase_at.
const MIN_CYCLE_SECS: u32 = 1;
const PHASE_MAX_SECS: u32 = 20;

impl TimerView {
    fn build_breathing_setup(&self) {
        self.build_phase_tiles();
        // Load persisted values — overrides defaults set in `constructed`.
        self.load_breathing_settings();
        self.refresh_phase_tiles();
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

    /// Set the Box-Breath session-length cell, persist, refresh the
    /// hero label and the shared Duration row's value label. Used by
    /// both `load_breathing_settings` (initial visit) and the H:M
    /// dialog. Stored as seconds (future-proof for sub-minute UI);
    /// clamps to 60..=23h59m * 60 — same effective upper bound as
    /// Timer mode for consistency.
    fn set_breathing_duration_secs(&self, secs: u32) {
        let secs = secs.clamp(60, 23 * 3600 + 59 * 60);
        self.breathing_session_secs.set(secs);
        self.save_breathing_settings();
        // Duration row label is shared between modes; reflect the new
        // value here so a Box-Breath edit shows up immediately. H:MM
        // format matches Timer mode.
        let mins = secs / 60;
        let h = mins / 60;
        let m = mins % 60;
        self.duration_value_label.set_label(&format!("{h:02}:{m:02}"));
        if self.current_mode() == TimerMode::Breathing {
            self.refresh_hero_for_idle();
        }
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
        self.refresh_phase_tiles();
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

    fn load_breathing_settings(&self) {
        let Some(app) = self.get_app() else { return; };
        self.breathing_populating.set(true);
        let (p, secs) = app.with_db(|db| {
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
            let secs = read("breathing_session_secs", 5 * 60).clamp(60, 23 * 3600 + 59 * 60);
            (p, secs)
        }).unwrap_or((
            BreathPattern { in_secs: 4, hold_in: 4, out_secs: 4, hold_out: 4 },
            5 * 60,
        ));
        self.breathing_pattern.set(p);
        self.breathing_session_secs.set(secs);
        // The shared Duration row reflects whichever Cell the current
        // mode reads; reflect this load even if the user is currently
        // viewing Timer mode — switching to Box Breath later will
        // already have the right value visible.
        let mins = secs / 60;
        let h = mins / 60;
        let m = mins % 60;
        self.duration_value_label.set_label(&format!("{h:02}:{m:02}"));
        self.breathing_populating.set(false);
    }

    fn save_breathing_settings(&self) {
        if self.breathing_populating.get() { return; }
        let Some(app) = self.get_app() else { return; };
        let p = self.breathing_pattern.get();
        let secs = self.breathing_session_secs.get();
        app.with_db_mut(|db| {
            let _ = db.set_setting("breathing_in", &p.in_secs.to_string());
            let _ = db.set_setting("breathing_hold_in", &p.hold_in.to_string());
            let _ = db.set_setting("breathing_out", &p.out_secs.to_string());
            let _ = db.set_setting("breathing_hold_out", &p.hold_out.to_string());
            let _ = db.set_setting("breathing_session_secs", &secs.to_string());
        });
    }

    /// Apply the user's persisted label state for `mode` to the
    /// Setup view's chooser-row + master toggle. Read-only — never
    /// writes, so visit-time refreshes don't bump sync chatter.
    fn apply_preferred_label_for_mode(&self, _mode: TimerMode) {
        // refresh_setup_label_chooser_subtitle does the full
        // resolve-and-update dance from the persisted UUID + active
        // toggle, so this call is the single touchpoint.
        self.refresh_setup_label_chooser_subtitle();
    }

    fn persisted_label_active_for_mode(&self, mode: TimerMode) -> bool {
        let Some(app) = self.get_app() else { return false; };
        let key = label_active_setting_key(mode);
        app.with_db(|db| db.get_setting(key, "false"))
            .and_then(|r| r.ok())
            .map(|v| v == "true")
            .unwrap_or(false)
    }

    fn persist_label_active_for_mode(&self, mode: TimerMode, on: bool) {
        let Some(app) = self.get_app() else { return; };
        let key = label_active_setting_key(mode);
        app.with_db_mut(|db| {
            let _ = db.set_setting(key, if on { "true" } else { "false" });
        });
    }

    /// Read the persisted label uuid for `mode`. Returns `None` when
    /// the setting is missing or empty — callers fall back to
    /// `mode_default_label_uuid`.
    fn persisted_label_uuid_for_mode(&self, mode: TimerMode) -> Option<String> {
        let app = self.get_app()?;
        let key = label_uuid_setting_key(mode);
        let val = app
            .with_db(|db| db.get_setting(key, ""))
            .and_then(|r| r.ok())?;
        if val.is_empty() { None } else { Some(val) }
    }

    fn persist_label_uuid_for_mode(&self, mode: TimerMode, uuid: &str) {
        let Some(app) = self.get_app() else { return; };
        let key = label_uuid_setting_key(mode);
        app.with_db_mut(|db| { let _ = db.set_setting(key, uuid); });
    }

    /// Stable per-mode default label uuid used when the user's
    /// stored choice is missing — Meditation in Timer, Box-Breathing
    /// in Box Breath, Guided Meditation in Guided. Resolves through
    /// the seeded rows (`crate::db::DEFAULT_*_LABEL_UUID`).
    fn mode_default_label_uuid(&self, mode: TimerMode) -> &'static str {
        match mode {
            TimerMode::Timer => crate::db::DEFAULT_TIMER_LABEL_UUID,
            TimerMode::Breathing => crate::db::DEFAULT_BREATHING_LABEL_UUID,
            TimerMode::Guided => crate::db::DEFAULT_GUIDED_LABEL_UUID,
        }
    }
}

fn label_active_setting_key(mode: TimerMode) -> &'static str {
    match mode {
        TimerMode::Timer => "label_active_timer",
        TimerMode::Breathing => "label_active_breathing",
        TimerMode::Guided => "label_active_guided",
    }
}

fn label_uuid_setting_key(mode: TimerMode) -> &'static str {
    match mode {
        TimerMode::Timer => "default_label_uuid_timer",
        TimerMode::Breathing => "default_label_uuid_breathing",
        TimerMode::Guided => "default_label_uuid_guided",
    }
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
