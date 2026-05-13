use std::cell::{Cell, RefCell};
use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::{glib, CompositeTemplate};
use glib::subclass::Signal;
use std::sync::OnceLock;

use crate::db::{Label, SessionData, SessionMode};

use std::time::Duration;
use meditate_core::breath::BreathPattern;
use meditate_core::format::format_time;
use meditate_core::time::boot_time_now;

use meditate_core::bells::ActiveBell;
use meditate_core::session::{
    Effect as CoreSessionEffect,
    Session as CoreSession,
    SessionSettings as CoreSessionSettings,
};

// ── Per-mode independent state ────────────────────────────────────────────────

pub use meditate_core::session::UiState;

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

/// Bridge to core's `SessionMode` for the per-mode helpers in
/// `meditate_core::settings_keys` (which expect `SessionMode`).
/// `Breathing` ↔ `BoxBreath` is the only naming difference; the
/// variants are otherwise 1:1.
impl From<TimerMode> for meditate_core::SessionMode {
    fn from(m: TimerMode) -> Self {
        match m {
            TimerMode::Timer => meditate_core::SessionMode::Timer,
            TimerMode::Breathing => meditate_core::SessionMode::BoxBreath,
            TimerMode::Guided => meditate_core::SessionMode::Guided,
        }
    }
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
    #[template_child] pub keep_screen_awake_row:  TemplateChild<adw::SwitchRow>,
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
    // The high-level state (Idle / Preparing / Running / Overtime /
    // Paused / Done) is derived from `core_session.ui_state()` —
    // Session retains its `Stopped` phase + the saved duration after
    // a session ends, so the Done view reads both off the Session
    // until the shell drops it at reset_mode.
    /// Unix timestamp when the active session started (for DB save).
    session_start_time: Cell<i64>,

    /// Which mode the active tick belongs to. Only meaningful while
    /// tick_source is Some.
    tick_mode: Cell<TimerMode>,

    /// Active glib timeout handle (at most one mode runs at a time).
    tick_source: RefCell<Option<glib::SourceId>>,

    /// Slow heartbeat that refreshes the crash-recovery snapshot's
    /// `accumulated_secs`. Separate from `tick_source` so the 1 Hz
    /// running-display tick can stay decoupled from the on-disk
    /// snapshot write (which is ~hundreds of µs of eMMC fsync). One
    /// fire per ~60s while a session is in flight; cleared at
    /// `reset_mode`.
    snapshot_tick_source: RefCell<Option<glib::SourceId>>,

    /// Held PatternPlayback handle for the most recent bell or
    /// phase-cue vibration. Replacing it with a new handle drops the
    /// previous one — the Drop impl fires `Vibrate(app_id, [])` to
    /// cancel any pattern still playing — so newest-wins overlap
    /// behaviour is automatic.
    current_vibration: RefCell<Option<crate::vibration::PatternPlayback>>,
    /// Cookie returned by `gtk::Application::inhibit` while a session
    /// is running and the active mode's keep-screen-awake toggle is
    /// on. 0 means "no inhibit held". Released on every `timer-stopped`
    /// emit site (user-stop, countdown finish, breath finish).
    screen_awake_cookie: Cell<u32>,
    /// Weak ref to the running-page time label for live updates.
    running_label: RefCell<Option<gtk::Label>>,
    /// Refs to the running-page buttons so the Overtime transition
    /// can morph them in place (Pause → Finish, Stop hidden, the
    /// "Add MM:SS?" suffix shown). All three are dropped when the
    /// session ends to release the widgets.
    running_pause_btn: RefCell<Option<gtk::Button>>,
    running_stop_btn: RefCell<Option<gtk::Button>>,
    overtime_add_btn: RefCell<Option<gtk::Button>>,
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
    /// Live mirror of the active mode's persisted stopwatch flag and
    /// of `stopwatch_mode_row`'s active state. `true` means count up
    /// from zero with no target; `false` means count down to
    /// `countdown_target_secs`.
    pub(super) stopwatch_toggle_on: Cell<bool>,
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
    /// Suppress persistence side-effects while `load_timer_settings`
    /// is restoring the countdown target from the DB.
    timer_populating: Cell<bool>,
    /// Boot-time anchor at session start. Suspend-resilient (see boot_time_now).
    start_boot_time: Cell<Option<std::time::Duration>>,
    /// `meditate_core::session::Session` — the portable state machine
    /// that owns prep / running / overtime / box-breath / bells /
    /// pause logic. Sole source of truth for elapsed time across
    /// every mode and phase post Stage 6 of CORE_MIGRATION item 13.
    /// `Some` between start_session and on_stop / finish_overtime /
    /// add_overtime_and_finish; `None` while idle.
    core_session: RefCell<Option<CoreSession>>,
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
        // Defaults match the blueprint's hero label (`00:10`) and the
        // canonical 4-4-4-4 / 5-min box-breath baseline. The core
        // constants pin them across shells; `load_*_settings`
        // overrides from the DB in a moment.
        self.countdown_target_secs.set(meditate_core::session::TIMER_DEFAULT_SECS);
        self.breathing_pattern.set(BreathPattern {
            in_secs: 4, hold_in: 4, out_secs: 4, hold_out: 4,
        });
        self.breathing_session_secs.set(meditate_core::session::BREATHING_DEFAULT_SECS);
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
                    let key = stopwatch_key_for_mode(imp.current_mode());
                    app.with_db_mut(|db| {
                        let _ = db.set_setting(
                            key,
                            meditate_core::format_bool(on),
                        );
                    });
                }
                imp.refresh_stopwatch_dependent_ui();
            }
        ));

        // Keep-Screen-Awake SwitchRow: persists per-mode (timer /
        // guided / boxbreath_keep_screen_awake) so each mode can have
        // its own preference. The bells_loading guard reuses the
        // existing on-visit suppression flag, since the row is loaded
        // alongside the bell rows on every page-visit + mode switch.
        self.keep_screen_awake_row.connect_active_notify(glib::clone!(
            #[weak(rename_to = this)] obj,
            move |row| {
                let imp = this.imp();
                if imp.bells_loading.get() { return; }
                let on = row.is_active();
                if let Some(app) = imp.get_app() {
                    let key = keep_screen_awake_key_for_mode(imp.current_mode());
                    app.with_db_mut(|db| {
                        let _ = db.set_setting(
                            key,
                            meditate_core::format_bool(on),
                        );
                    });
                }
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
                let session_mode: crate::db::SessionMode = imp.current_mode().into();
                if !meditate_core::preset_config::mode_supports_presets(session_mode) {
                    return;
                }
                let snapshot = Box::new(imp.snapshot_current_setup());
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
                let session_mode: crate::db::SessionMode = imp.current_mode().into();
                if !meditate_core::preset_config::mode_supports_presets(session_mode) {
                    return;
                }
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
                            meditate_core::format_bool(on),
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
                    let default = meditate_core::settings_keys::default_label_uuid_for_mode(
                        mode.into(),
                    );
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
                            meditate_core::format_bool(on),
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
                            meditate_core::format_bool(on),
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
                            meditate_core::format_bool(on),
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
        let saved = app
            .with_db(|db| {
                meditate_core::bells::signal_mode_override_from_db(
                    db.core(),
                    self.current_mode().into(),
                )
            })
            .unwrap_or(crate::db::SignalMode::Both);
        let initial = meditate_core::bells::clamp_signal_mode_for_haptic(
            saved, app.has_haptic(),
        ).as_db_str();
        // Set populating flag so the active-name notify handler
        // doesn't write the just-loaded value back to the DB.
        self.bells_loading.set(true);
        toggle_group.set_active_name(Some(initial));
        self.bells_loading.set(false);
    }

    /// Sync the Keep-Screen-Awake switch row with the current mode's
    /// stored value. Called on visit and on every mode switch so the
    /// switch reflects the key the runtime will read on session start.
    pub(crate) fn refresh_keep_screen_awake_state(&self) {
        let Some(app) = self.get_app() else { return; };
        let mode = self.current_mode().into();
        let on = app
            .with_db(|db| meditate_core::settings_keys::keep_screen_awake_from_db(db.core(), mode))
            .unwrap_or(false);
        self.bells_loading.set(true);
        self.keep_screen_awake_row.set_active(on);
        self.bells_loading.set(false);
    }

    /// If the active mode's keep-screen-awake setting is on, hold an
    /// idle inhibit for the duration of the session. Cookie is stashed
    /// on `screen_awake_cookie`; calling release_screen_awake_lock
    /// uninhibits and clears it. No-op if no mode requested it or if
    /// the cookie is already held (idempotent).
    pub(crate) fn acquire_screen_awake_lock(
        &self,
        app: &crate::application::MeditateApplication,
    ) {
        if self.screen_awake_cookie.get() != 0 { return; }
        let mode = self.current_mode().into();
        let active = app
            .with_db(|db| meditate_core::settings_keys::keep_screen_awake_from_db(db.core(), mode))
            .unwrap_or(false);
        if !active { return; }
        let window = app.active_window();
        let cookie = app.inhibit(
            window.as_ref(),
            gtk::ApplicationInhibitFlags::IDLE,
            Some(&crate::i18n::gettext("Meditation session running")),
        );
        self.screen_awake_cookie.set(cookie);
    }

    /// Release the idle-inhibit cookie acquired at session start, if
    /// any. Idempotent.
    pub(crate) fn release_screen_awake_lock(
        &self,
        app: &crate::application::MeditateApplication,
    ) {
        let cookie = self.screen_awake_cookie.get();
        if cookie == 0 { return; }
        app.uninhibit(cookie);
        self.screen_awake_cookie.set(0);
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
                        meditate_core::format_bool(on),
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
            .with_db(|db| meditate_core::read_bool(db.core(), "boxbreath_cues_active", false))
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
        let Some((sound_name, pattern_name)) = app
            .with_db(|db| meditate_core::bells::phase_cue_names(db.core(), phase))
            .flatten()
        else { return; };
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
        let mode = crate::db::SignalMode::from_db_str(name.as_str())
            .unwrap_or(crate::db::SignalMode::Sound);
        if let Some(app) = get_app() {
            app.with_db_mut(|db| db.set_setting(setting_key, mode.as_db_str()));
        }
        sound_revealer.set_reveal_child(mode.includes_sound());
        pattern_revealer.set_reveal_child(mode.includes_vibration());
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

    // Force-display 'sound' on no-haptic devices regardless of saved
    // value; the persisted setting stays untouched so syncing to a
    // phone restores the user's intent.
    let saved = app
        .with_db(|db| db.get_setting(setting_key, "sound"))
        .and_then(|r| r.ok())
        .and_then(|s| crate::db::SignalMode::from_db_str(&s))
        .unwrap_or(crate::db::SignalMode::Sound);
    let initial = meditate_core::bells::clamp_signal_mode_for_haptic(
        saved, app.has_haptic(),
    );
    toggle_group.set_active_name(Some(initial.as_db_str()));
    sound_revealer.set_reveal_child(initial.includes_sound());
    pattern_revealer.set_reveal_child(initial.includes_vibration());
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
        let mode = crate::db::SignalMode::from_db_str(name.as_str())
            .unwrap_or(crate::db::SignalMode::Sound);
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
        sound_revealer.set_reveal_child(mode.includes_sound());
        pattern_revealer.set_reveal_child(mode.includes_vibration());
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
    let initial = meditate_core::bells::clamp_signal_mode_for_haptic(saved, app.has_haptic());
    toggle_group.set_active_name(Some(initial.as_db_str()));
    sound_revealer.set_reveal_child(initial.includes_sound());
    pattern_revealer.set_reveal_child(initial.includes_vibration());
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
        let mode = crate::db::SignalMode::from_db_str(name.as_str())
            .unwrap_or(crate::db::SignalMode::Both);
        let Some(app) = get_app() else { return; };
        let setting_key = setting_key_for_mode(get_mode());
        app.with_db_mut(|db| db.set_setting(setting_key, mode.as_db_str()));
    });
}

/// Map a TimerMode to its per-mode signal-mode setting key.
/// TimerMode-keyed wrappers around `meditate_core::settings_keys::*`.
/// Call sites pass `TimerMode`; the `From` impl above hands the
/// canonical `SessionMode` to core.
pub(crate) fn setting_key_for_mode(mode: TimerMode) -> &'static str {
    meditate_core::settings_keys::signal_mode_key_for_mode(mode.into())
}

pub(crate) fn keep_screen_awake_key_for_mode(mode: TimerMode) -> &'static str {
    meditate_core::settings_keys::keep_screen_awake_key_for_mode(mode.into())
}

pub(crate) fn stopwatch_key_for_mode(mode: TimerMode) -> &'static str {
    meditate_core::settings_keys::stopwatch_key_for_mode(mode.into())
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
        // Cycle-aligned target lives on BreathPattern in core.
        self.breathing_pattern
            .get()
            .cycle_aligned_target_secs(self.breathing_session_secs.get() as u64)
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
        // Per-mode visibility truth table lives in core so the
        // Android shell agrees on which row appears in which mode.
        self.apply_setup_visibility(mode);
        // Stopwatch is per-mode now: in Timer it counts up, in Box
        // Breath it runs without a target (user stops manually),
        // in Guided it suppresses the natural-end bell.
        self.stopwatch_mode_row.set_visible(true);
        // Refresh the duration label from the appropriate Cell on every
        // mode switch so the suffix doesn't lag.
        self.refresh_duration_value_label();
        // Per-mode Cues toggle reflects the new mode's saved value.
        self.refresh_cues_signal_mode_state();
        // Same for the Keep-Screen-Awake switch.
        self.refresh_keep_screen_awake_state();
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

        // Stopwatch is per-mode now: each of Timer / Guided /
        // Box Breath has its own active flag. Reload the live
        // mirror + the row UI from the new mode's setting before
        // running the dependent refresh.
        if let Some(app) = self.get_app() {
            let key = stopwatch_key_for_mode(mode);
            let on = app
                .with_db(|db| meditate_core::read_bool(db.core(), key, false))
                .unwrap_or(false);
            self.stopwatch_loading.set(true);
            self.stopwatch_mode_row.set_active(on);
            self.stopwatch_toggle_on.set(on);
            self.stopwatch_loading.set(false);
        }
        self.refresh_stopwatch_dependent_ui();

        match self.ui_state() {
            UiState::Idle      => self.show_idle_ui(),
            UiState::Paused    => self.show_paused_ui(self.current_display_secs()),
            UiState::Done      => self.view_stack.set_visible_child_name("done"),
            // Running, Preparing, and Overtime normally can't reach
            // here (the nav page blocks the toggle while a session
            // or prep is in flight); fall back to idle UI as a
            // safety net.
            UiState::Running | UiState::Preparing | UiState::Overtime => {
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
        self.apply_setup_visibility(mode);
        self.stopwatch_mode_row.set_visible(true);
        self.refresh_duration_value_label();
        self.mode_toggle_group.set_sensitive(true);
        self.session_group.set_sensitive(true);
        self.refresh_hero_for_idle();
    }

    /// Apply the per-mode Setup-view visibility truth table from
    /// `preset_config::setup_visibility` to the template-child rows.
    /// Called from both `on_mode_switched` (live mode-toggle flip)
    /// and `show_idle_ui` (post-session return). The decision lives
    /// in core so the Android shell consults the same source.
    fn apply_setup_visibility(&self, mode: TimerMode) {
        let v = meditate_core::preset_config::setup_visibility(mode.into());
        self.countdown_inputs.set_visible(v.countdown);
        self.boxbreath_inputs.set_visible(v.boxbreath);
        self.guided_section.set_visible(v.guided);
        self.boxbreath_phase_section.set_visible(v.boxbreath_phase);
        self.starting_bell_row.set_visible(v.starting_bell);
        self.interval_bells_enabled_row.set_visible(v.interval_bells);
        self.duration_row.set_visible(v.duration);
        self.presets_section.set_visible(v.presets);
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
        self.duration_value_label
            .set_label(&meditate_core::format::format_hhmm(mins * 60));
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
        self.big_time_label.set_label(&format_time(Duration::from_secs(display_secs)));
        self.time_unit_label.set_label(&crate::i18n::gettext("Paused"));
        self.time_unit_label.set_visible(true);
    }

    /// Set the hero time display + subtitle to their idle-state values for
    /// whichever mode is currently active.
    fn refresh_hero_for_idle(&self) {
        // Stopwatch flips the hero to "00:00" in any mode — there's
        // no target to display, the running tick will count up from
        // zero. When stopwatch is off the mode-specific target shows
        // (Timer's countdown, Box Breath's session duration, Guided's
        // picked file length).
        let target_secs = match self.current_mode() {
            TimerMode::Timer => self.countdown_target_secs.get(),
            TimerMode::Breathing => self.breathing_session_secs.get() as u64,
            TimerMode::Guided => self
                .guided_pick
                .borrow()
                .as_ref()
                .map(|p| p.duration_secs)
                .unwrap_or(0) as u64,
        };
        let label = meditate_core::format::idle_hero_label(
            self.stopwatch_toggle_on.get(),
            target_secs,
        );
        self.big_time_label.set_label(&label);
        self.time_unit_label.set_label(&crate::i18n::gettext("Hours · Minutes"));
        self.time_unit_label.set_visible(true);
    }

    /// Re-apply the stopwatch toggle's effect on the rest of the setup
    /// page: hero label flips between the picked target and 00:00, and
    /// the Quick Presets card greys out so the user can't tap a chip
    /// while the toggle is on.
    fn refresh_stopwatch_dependent_ui(&self) {
        // Hero refresh: stopwatch flips the hero between "00:00"
        // and the mode's target reading in every mode (Timer,
        // Box Breath, Guided), not just Timer.
        if self.ui_state() == UiState::Idle {
            self.refresh_hero_for_idle();
        }
        // Stopwatch on ⇒ planned-duration concept inert; grey out
        // the Duration row only. The presets list stays interactive —
        // tapping a preset is a higher-level action that legitimately
        // re-arms the duration (and resets the stopwatch toggle as
        // part of its config). `stopwatch_toggle_on` mirrors the
        // *current mode's* persisted stopwatch flag, so this gate
        // applies uniformly across Timer / Guided / Box Breath.
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
        // Stopwatch is a per-mode concept; the cell always reflects
        // the current mode's persisted state, so the gate applies
        // uniformly. The persisted `end_bell_active` setting stays
        // as the user left it — flipping stopwatch off in any mode
        // brings the previous state back.
        let stopwatch_on = self.stopwatch_toggle_on.get();
        let state = self
            .get_app()
            .and_then(|app| {
                app.with_db(|db| {
                    meditate_core::bells::end_bell_row_state(db.core(), stopwatch_on)
                })
            })
            .unwrap_or(meditate_core::bells::EndBellRowState {
                active: true,
                sensitive: !stopwatch_on,
            });
        self.bells_loading.set(true);
        self.end_bell_row.set_enable_expansion(state.active);
        self.end_bell_row.set_expanded(state.active);
        self.end_bell_row.set_sensitive(state.sensitive);
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
                    app.with_db(|db| meditate_core::format::prep_plan_from_db(db.core()))
                })
                .flatten()
        } else {
            None
        };

        match mode {
            TimerMode::Timer => {
                // Validate countdown target up front so a 0-target
                // countdown doesn't even start (regardless of prep).
                if !self.stopwatch_toggle_on.get()
                    && self.countdown_target_secs.get() == 0
                {
                    return;
                }
                // Anchor the boot time once. Without prep the Session
                // is built directly in the no-prep arm below; with
                // prep it's built in the prep-setup arm further down.
                if prep.is_none() {
                    self.start_boot_time.set(Some(boot_time_now()));
                }
            }
            TimerMode::Breathing => {
                let pattern = self.breathing_pattern.get();
                // "Finish the breath" before stopping: round to the
                // next full cycle so the session always ends on an
                // exhale/hold-out boundary.
                let target = pattern.cycle_aligned_target_secs(
                    self.breathing_session_secs.get() as u64,
                );
                self.start_boot_time.set(Some(boot_time_now()));
                // No target_secs → stopwatch-only Box Breath never
                // auto-ends; user must press Stop.
                let core_target = if self.stopwatch_toggle_on.get() {
                    None
                } else {
                    Some(target as u32)
                };
                let Some(app) = self.get_app() else { return; };
                let core_settings = CoreSessionSettings {
                    mode: SessionMode::BoxBreath,
                    prep_secs: None,
                    target_secs: core_target,
                    // Box Breath always shows count-up elapsed
                    // regardless of any toggle; the cycle-aligned
                    // end still fires via target_secs.
                    stopwatch_display: true,
                    breath_pattern: Some(pattern),
                    bells: Vec::new(),
                    bell_rng_seed: 1,
                    signal_mode_override: self.read_signal_mode_override(
                        &app, SessionMode::BoxBreath,
                    ),
                    starting_bell: None,
                    end_bell: self.build_end_bell_cue(&app),
                    box_breath_cues: Some(self.build_box_breath_cues(&app)),
                };
                let session = CoreSession::start_running(
                    core_settings,
                    std::time::Duration::ZERO,
                );
                self.dispatch_session_effects(&session.start_signals());
                *self.core_session.borrow_mut() = Some(session);
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
                        // MainContext. Ask Session to force-transition
                        // into Overtime; idempotent if we already
                        // crossed the boundary via tick_running.
                        // Session emits [EnterOvertime, FireEndBell]
                        // on the transition — dispatcher routes the
                        // bell; EnterOvertime triggers the button-
                        // morph + notification ceremony here.
                        let imp = obj_for_eos.imp();
                        let effects = imp.core_session.borrow_mut().as_mut()
                            .map(|s| s.enter_overtime())
                            .unwrap_or_default();
                        imp.dispatch_session_effects(&effects);
                        if effects.iter().any(|e| matches!(e, CoreSessionEffect::EnterOvertime)) {
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
                // Always carries a target (the file's probed
                // duration); the stopwatch toggle only affects the
                // running display (count-up vs count-down), not the
                // Session's target.
                let Some(app) = self.get_app() else { return; };
                let core_settings = CoreSessionSettings {
                    mode: SessionMode::Guided,
                    prep_secs: None,
                    target_secs: Some(target as u32),
                    stopwatch_display: self.stopwatch_toggle_on.get(),
                    breath_pattern: None,
                    bells: Vec::new(),
                    bell_rng_seed: 1,
                    signal_mode_override: self.read_signal_mode_override(
                        &app, SessionMode::Guided,
                    ),
                    // Guided sessions have no starting bell (the file
                    // is the "start"); end bell fires when the file
                    // ends or the user clicks Finish.
                    starting_bell: None,
                    end_bell: self.build_end_bell_cue(&app),
                    box_breath_cues: None,
                };
                let session = CoreSession::start_running(
                    core_settings,
                    std::time::Duration::ZERO,
                );
                self.dispatch_session_effects(&session.start_signals());
                *self.core_session.borrow_mut() = Some(session);
            }
        }

        // Prep / no-prep Timer share the same SessionSettings build;
        // only `prep_secs` and the construction call (start_prep vs.
        // start_running) differ.
        let build_timer_settings = |prep_dur: Option<Duration>| {
            let Some(app) = self.get_app() else { return None; };
            let stopwatch_on = self.stopwatch_toggle_on.get();
            let target_secs = if stopwatch_on {
                None
            } else {
                Some(self.countdown_target_secs.get() as u32)
            };
            let (bells, bell_rng_seed) = self.build_session_bells(
                target_secs.map(|t| t as u64),
                stopwatch_on,
            );
            Some(CoreSessionSettings {
                mode: SessionMode::Timer,
                prep_secs: prep_dur.map(|d| d.as_secs() as u32),
                target_secs,
                stopwatch_display: stopwatch_on,
                breath_pattern: None,
                bells,
                bell_rng_seed,
                signal_mode_override: self.read_signal_mode_override(
                    &app, SessionMode::Timer,
                ),
                starting_bell: self.build_starting_bell_cue(&app),
                end_bell: self.build_end_bell_cue(&app),
                box_breath_cues: None,
            })
        };

        if let Some(prep_dur) = prep {
            // Prep path: Session owns prep ticking + the prep→Running
            // transition internally; bells are pre-built so the
            // schedule survives the transition unchanged.
            self.start_boot_time.set(Some(boot_time_now()));
            let Some(core_settings) = build_timer_settings(Some(prep_dur)) else { return; };
            *self.core_session.borrow_mut() = Some(CoreSession::start_prep(
                core_settings,
                std::time::Duration::ZERO,
            ));
        } else {
            // No-prep Timer: build bells + start Session directly in
            // Running. Same SessionSettings shape as the prep path.
            if mode == TimerMode::Timer {
                let Some(core_settings) = build_timer_settings(None) else { return; };
                let session = CoreSession::start_running(
                    core_settings,
                    std::time::Duration::ZERO,
                );
                self.dispatch_session_effects(&session.start_signals());
                *self.core_session.borrow_mut() = Some(session);
            }
        }

        self.session_start_time.set(unix_now());

        // Crash-recovery snapshot. Initial write with accumulated_secs=0
        // so a process death before the first 60 s heartbeat still
        // leaves a row the next launch can finalise (as a 0-min
        // session — toast still surfaces, Undo trivially dismisses).
        // The snapshot heartbeat rewrites this with the current
        // elapsed every 60 s while the session is in flight.
        self.write_in_progress_snapshot(0);
        self.start_snapshot_tick();

        // Inhibit display sleep if the active mode requested it. Cookie
        // released at every timer-stopped emit site (user-stop,
        // countdown finish, breath finish).
        if let Some(app) = self.get_app() {
            self.acquire_screen_awake_lock(&app);
        }

        // Starting bell fires via Session's FireStartingBell effect:
        // - No-prep Timer: dispatched immediately after start_running
        //   via session.start_signals() above.
        // - With-prep Timer: dispatched alongside EndPrep at the prep
        //   boundary tick (see Session::tick_prep).
        // - Box Breath / Guided: starting_bell is None in their
        //   SessionSettings, so no effect ever emits — matches the
        //   prior Timer-only behaviour.

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
        // Session resumes uniformly; phase is preserved across the
        // pause window so the post-resume timer_state() automatically
        // returns to Preparing or Running based on Session.phase.
        // The gst playbin is the only gtk-side side-resume left.
        if let Some(s) = self.core_session.borrow_mut().as_mut() {
            s.resume(now);
        }
        if mode == TimerMode::Guided {
            if let Some(p) = self.guided_playback.borrow().as_ref() {
                p.resume();
            }
        }

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
            label.set_label(&format_time(Duration::from_secs(self.current_display_secs())));
        }
        self.obj().emit_by_name::<()>("timer-started", &[]);
    }

    /// Called by the window when the running page's Pause button is pressed.
    pub fn on_pause(&self) {
        self.cancel_tick();

        let now = self.elapsed_since_start();
        // Session decides whether pause is meaningful + what side
        // effects (StopActiveSignals on the leading edge) the shell
        // should run; we just dispatch. The gst playbin pause for
        // Guided is purely a shell mechanism — no Session-level
        // analogue.
        let effects = self
            .core_session
            .borrow_mut()
            .as_mut()
            .map(|s| s.pause(now))
            .unwrap_or_default();
        self.dispatch_session_effects(&effects);
        if self.tick_mode.get() == TimerMode::Guided {
            if let Some(p) = self.guided_playback.borrow().as_ref() {
                p.pause();
            }
        }

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

        // Drive the stop decision through Session: returns the
        // duration to save regardless of phase (prep elapsed for
        // stop-during-prep, running elapsed mid-Running, running+
        // overtime elapsed mid-Overtime, paused-at value if paused).
        let elapsed = self
            .core_session_end(|s, now| s.stop(now))
            .unwrap_or(0);
        // Session transitioned to Stopped and stashed `elapsed` as
        // its final duration — the Done view reads it back via
        // `session_final_duration_secs()`. Don't drop the Session
        // here; reset_mode drops it when the user leaves Done.
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

        // Release the keep-screen-awake inhibit (no-op if none held).
        if let Some(app) = self.get_app() {
            self.release_screen_awake_lock(&app);
        }

        self.obj().emit_by_name::<()>("timer-stopped", &[]);
        self.show_done(elapsed);
    }

    /// Elapsed seconds for the active session. Used by on_stop /
    /// on_save (both produce a session row whose `duration_secs` is
    /// what we return here). Once the session has Stopped, Session
    /// holds the canonical saved duration (target_secs for Finish-
    /// overtime, running+overtime elapsed for Add, etc.); during in-
    /// flight Prep / Running / Overtime the running phase_clock is
    /// the source.
    fn elapsed_secs_for_mode(&self) -> u64 {
        if let Some(secs) = self.session_final_duration_secs() {
            return secs;
        }
        self.session_elapsed().as_secs()
    }

    /// Saved-duration accessor wrapping `Session::final_duration_secs()`.
    /// Returns `None` when there is no Session or when the active
    /// Session is still in flight.
    fn session_final_duration_secs(&self) -> Option<u64> {
        self.core_session
            .borrow()
            .as_ref()
            .and_then(|s| s.final_duration_secs())
    }

    fn show_done(&self, elapsed_secs: u64) {
        self.done_duration_label.set_label(&format_time(Duration::from_secs(elapsed_secs)));
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
        self.stop_active_signals();
        let mode = self.current_mode();

        let elapsed = self.elapsed_secs_for_mode();
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
        // session starts off too. The "what action to take" decision
        // (set / deactivate / noop on a now-missing id) lives in
        // core::labels so the Android shell shares it.
        let labels = self
            .get_app()
            .and_then(|app| app.with_db(|db| db.list_labels()))
            .and_then(|r| r.ok())
            .unwrap_or_default();
        match meditate_core::labels::resolve_persist_action(label_id, &labels) {
            meditate_core::labels::PersistAction::SetUuidAndActivate { uuid } => {
                self.persist_label_uuid_for_mode(mode, &uuid);
                self.persist_label_active_for_mode(mode, true);
            }
            meditate_core::labels::PersistAction::Deactivate => {
                self.persist_label_active_for_mode(mode, false);
            }
            meditate_core::labels::PersistAction::NoOp => {}
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
                let session = match result {
                    Some(Ok(s)) => s,
                    Some(Err(e)) => {
                        meditate_core::log(
                            &meditate_core::format::session_save_failure_log_message(
                                meditate_core::format::SessionSaveFailureKind::StorageError,
                                &e.to_string(),
                            ),
                        );
                        if let Some(win) = app
                            .active_window()
                            .and_then(|w| w.downcast::<crate::window::MeditateWindow>().ok())
                        {
                            win.add_toast(adw::Toast::new(&crate::i18n::gettext(
                                "Couldn't save session — storage error",
                            )));
                        }
                        return;
                    }
                    None => {
                        meditate_core::log(
                            &meditate_core::format::session_save_failure_log_message(
                                meditate_core::format::SessionSaveFailureKind::DbUnopened,
                                "with_db_blocking_mut returned None",
                            ),
                        );
                        if let Some(win) = app
                            .active_window()
                            .and_then(|w| w.downcast::<crate::window::MeditateWindow>().ok())
                        {
                            win.add_toast(adw::Toast::new(&crate::i18n::gettext(
                                "Couldn't save session — storage unavailable",
                            )));
                        }
                        return;
                    }
                };

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
        self.stop_active_signals();
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
        // Session is the sole owner of session timing state; drop
        // it once, regardless of mode. The gst playbin is the only
        // mode-specific gtk-side artefact left that still needs
        // explicit teardown.
        *self.core_session.borrow_mut() = None;
        match mode {
            TimerMode::Timer => {}
            TimerMode::Breathing => {}
            TimerMode::Guided => {
                // Drop runs set_state(Null) + removes the bus
                // signal-watch. The playback might already be None
                // if a prior on_stop / overtime path dropped it;
                // borrow_mut + None assignment is idempotent.
                *self.guided_playback.borrow_mut() = None;
            }
        }
        // Dropping core_session above also drops the saved final
        // duration, so the next refresh sees `ui_state == Idle`.
        self.session_start_time.set(0);

        // Drop the crash-recovery snapshot + cancel its 60 s
        // heartbeat. Every path that ends a session — save, discard,
        // stop-from-pause, abandonment by mode switch — funnels
        // through here, so this single pair covers all of them.
        // set_session_in_progress / finalize on the next launch
        // will see no row and skip the toast.
        self.cancel_snapshot_tick();
        self.clear_in_progress_snapshot();

        // Only update the visible UI if this mode is the one currently shown.
        if mode == self.current_mode() {
            self.show_idle_ui();
            self.refresh_streak();
        }
    }

    /// Build the crash-recovery snapshot from current setup state +
    /// the supplied `accumulated_secs` and write it. Called at session
    /// start (with accumulated_secs=0) and on the 60s tick cadence so
    /// a kernel OOM / battery-death / app crash mid-session leaves a
    /// row the next launch can finalise into a real session.
    ///
    /// `mode_payload` is reserved for shell-side mode-specific data
    /// (e.g. a future v2 Resume feature might capture box-breath
    /// phase progress); v1 stores an empty JSON object — core treats
    /// the field as opaque.
    fn write_in_progress_snapshot(&self, accumulated_secs: u32) {
        let Some(app) = self.get_app() else { return; };
        let unix_start = self.session_start_time.get();
        if unix_start <= 0 {
            // No session has started yet (or already ended) — nothing
            // to snapshot.
            return;
        }
        let mode = match self.current_mode() {
            TimerMode::Timer => meditate_core::SessionMode::Timer,
            TimerMode::Breathing => meditate_core::SessionMode::BoxBreath,
            TimerMode::Guided => meditate_core::SessionMode::Guided,
        };
        let label_id = self.setup_selected_label_id();
        let guided_file_uuid = if matches!(mode, meditate_core::SessionMode::Guided) {
            self.guided_selected_uuid.borrow().clone()
        } else {
            None
        };
        let snapshot = meditate_core::db::SessionInProgress {
            start_iso: meditate_core::time::unix_to_local_iso(unix_start),
            accumulated_secs,
            mode,
            mode_payload: "{}".into(),
            label_id,
            guided_file_uuid,
        };
        app.with_db_mut(|db| {
            if let Err(e) = db.set_session_in_progress(&snapshot) {
                meditate_core::log(&format!(
                    "session_recovery: set snapshot failed: {e}"
                ));
            }
        });
    }

    /// Drop the crash-recovery snapshot if any. Idempotent — a no-op
    /// when no session was in flight. DB errors are logged but
    /// otherwise swallowed: failing to clear the snapshot is at
    /// worst a one-off phantom auto-finalize on the next launch
    /// (which the user can Undo), not a state corruption.
    fn clear_in_progress_snapshot(&self) {
        let Some(app) = self.get_app() else { return; };
        app.with_db_mut(|db| {
            if let Err(e) = db.clear_session_in_progress() {
                meditate_core::log(&format!(
                    "session_recovery: clear snapshot failed: {e}"
                ));
            }
        });
    }

    /// Schedule a slow heartbeat that rewrites the in-progress
    /// snapshot every 60 s with the current `elapsed_secs_for_mode`.
    /// The snapshot's accumulated_secs is the only timing fact the
    /// next-launch recovery needs; capturing it once a minute keeps
    /// the worst-case loss bounded to ~60 s of meditation if the
    /// process is killed between two beats.
    ///
    /// Coexists with `start_tick` (which drives the 1 Hz running
    /// display) — separate source so an eMMC fsync stall on the
    /// snapshot write doesn't visibly hitch the running label.
    fn start_snapshot_tick(&self) {
        self.cancel_snapshot_tick();
        let obj = self.obj().clone();
        let source_id = glib::timeout_add_seconds_local(60, move || {
            let imp = obj.imp();
            // No session in flight → tick has nothing to do. Bail
            // without rescheduling so a stale source after a session
            // ends doesn't keep waking up.
            if imp.core_session.borrow().is_none() {
                return glib::ControlFlow::Break;
            }
            let elapsed = imp.elapsed_secs_for_mode();
            let secs_u32: u32 = elapsed.try_into().unwrap_or(u32::MAX);
            imp.write_in_progress_snapshot(secs_u32);
            glib::ControlFlow::Continue
        });
        *self.snapshot_tick_source.borrow_mut() = Some(source_id);
    }

    /// Drop the 60 s snapshot heartbeat. Called from `reset_mode`
    /// so every session-end path cancels it.
    fn cancel_snapshot_tick(&self) {
        if let Some(src) = self.snapshot_tick_source.borrow_mut().take() {
            src.remove();
        }
    }

    fn start_tick(&self) {
        self.cancel_tick();
        let obj = self.obj().clone();

        let source_id = glib::timeout_add_local(
            std::time::Duration::from_secs(1),
            move || {
                let imp = obj.imp();
                match imp.ui_state() {
                    UiState::Preparing => imp.tick_prep(&obj),
                    UiState::Running => imp.tick_running(&obj),
                    UiState::Overtime => imp.tick_overtime(&obj),
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
        // Prep ticking is owned by the portable state machine —
        // Session emits EndPrep + flips its own phase to Running at
        // the boundary, with starting-bell FireStartingBell on the
        // same tick. The shell only renders display updates and
        // routes fire cues; the next tick lands in `tick_running`
        // automatically via `ui_state()`.
        let effects: Vec<CoreSessionEffect> = self
            .core_session
            .borrow_mut()
            .as_mut()
            .map(|s| s.tick(now))
            .unwrap_or_default();
        for effect in &effects {
            if let CoreSessionEffect::UpdateDisplay { secs } = effect {
                if let Some(label) = self.running_label.borrow().as_ref() {
                    label.set_label(&format_time(Duration::from_secs(*secs)));
                }
            }
        }
        // FireStartingBell (emitted by Session alongside EndPrep at
        // the boundary tick) flows through the shared dispatcher.
        self.dispatch_session_effects(&effects);
        glib::ControlFlow::Continue
    }

    fn tick_running(&self, _obj: &super::TimerView) -> glib::ControlFlow {
        // Every Timer/Guided running tick flows through the portable
        // Session. Session's UpdateDisplay already encodes the
        // ceiling-vs-floor rounding for both countdown and
        // stopwatch_display modes — the gtk shell just renders.
        let (effects, new_secs, done) = {
            let now = self.elapsed_since_start();
            let mut session = self.core_session.borrow_mut();
            let Some(s) = session.as_mut() else {
                return glib::ControlFlow::Break;
            };
            let (outcome, effects) = s.tick_summary(now);
            (effects, outcome.display_secs.unwrap_or(0), outcome.entered_overtime)
        };

        // Bell sounds + vibrations flow through the portable
        // dispatcher: FireBell during Running, FireEndBell at the
        // EnterOvertime crossing (Session emits both alongside the
        // transition).
        self.dispatch_session_effects(&effects);

        if done {
            // Countdown crossed zero. Don't auto-finish — slide
            // into Overtime so the user can either commit the
            // extra time (Add) or stop at the planned duration
            // (Finish). The end bell already fired above via
            // FireEndBell dispatch; transition_running_to_overtime
            // handles the button-morph + system-notification side.
            self.transition_running_to_overtime();
            return glib::ControlFlow::Continue;
        }

        if let Some(label) = self.running_label.borrow().as_ref() {
            label.set_label(&format_time(Duration::from_secs(new_secs)));
        }

        glib::ControlFlow::Continue
    }

    /// One-shot at zero-crossing: ring the end bell + vibrate +
    /// notify, morph the running buttons into the Finish/Add
    /// layout. Session.phase is already Overtime by the time we're
    /// called (either tick_running transitioned internally before
    /// dispatching EnterOvertime, or the gst EOS callback forced
    /// the transition via Session.enter_overtime); the 1 Hz tick
    /// itself keeps running and now dispatches tick_overtime.
    fn transition_running_to_overtime(&self) {

        // Guided mode: drop the playbin BEFORE play_end_bell so the
        // end bell isn't competing with a few last frames of audio
        // (gst playbin holds a small buffer ahead of the wall clock,
        // so the file may still be sounding when the countdown hits
        // zero). Drop runs set_state(Null) + removes the bus watch.
        *self.guided_playback.borrow_mut() = None;

        if let Some(app) = self.get_app() {
            // End bell fires via Session's FireEndBell effect (emitted
            // alongside EnterOvertime in tick_running, and from
            // Session::enter_overtime() on the EOS-driven path).
            // System notification stays here — it's a gtk mechanism.

            // Only send a system notification when the app isn't
            // focused — the in-app overtime UI already signals
            // completion.
            if !app.active_window().map(|w| w.is_active()).unwrap_or(false) {
                let n = gtk::gio::Notification::new("Meditation Complete");
                // Session knows its own planned target (Timer's
                // countdown, Guided's probed duration via target_secs,
                // Box-Breath's cycle-aligned end) — read it instead
                // of re-deriving per mode.
                let target = self
                    .core_session
                    .borrow()
                    .as_ref()
                    .map(|s| s.completion_duration_secs())
                    .unwrap_or(0);
                n.set_body(Some(&format!("Session: {}", format_time(Duration::from_secs(target)))));
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
            add_btn.set_label(&meditate_core::format::overtime_button_label(
                &crate::i18n::gettext("Add"),
                Duration::ZERO,
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
            label.set_label(&format_time(Duration::from_secs(target)));
        }
    }

    /// 1 Hz update for the Overtime state — refreshes only the
    /// dynamic Add button label, and keeps interval bells firing
    /// on the original session timeline. The hero readout stays
    /// frozen at the planned duration.
    fn tick_overtime(&self, _obj: &super::TimerView) -> glib::ControlFlow {
        // Overtime ticks read the overtime delta from Session and
        // dispatch bell effects through the portable dispatcher.
        let (effects, overtime) = {
            let now = self.elapsed_since_start();
            let mut session = self.core_session.borrow_mut();
            let Some(s) = session.as_mut() else {
                return glib::ControlFlow::Break;
            };
            let effects = s.tick(now);
            let mut delta = Duration::ZERO;
            for effect in &effects {
                if let CoreSessionEffect::UpdateOvertimeLabel { overtime } = effect {
                    delta = *overtime;
                }
            }
            (effects, delta)
        };
        self.dispatch_session_effects(&effects);

        if let Some(add_btn) = self.overtime_add_btn.borrow().as_ref() {
            add_btn.set_label(&meditate_core::format::overtime_button_label(
                &crate::i18n::gettext("Add"),
                overtime,
            ));
        }
        glib::ControlFlow::Continue
    }

    /// Overtime user picked "Add MM:SS?" — record the planned
    /// duration *plus* the elapsed overtime as the session length,
    /// pop the running page, surface the Done screen.
    pub(super) fn add_overtime_and_finish(&self) {
        if self.ui_state() != UiState::Overtime {
            return;
        }
        let elapsed = self
            .core_session_end(|s, now| s.add_overtime_and_finish(now))
            .unwrap_or(0);
        self.end_overtime_session(elapsed);
    }

    /// Overtime user picked "Finish" — record exactly the planned
    /// countdown duration (overtime discarded). Session stashes the
    /// target in its `final_duration_secs` slot via `finish_overtime`,
    /// which `elapsed_secs_for_mode` then reads back so the Save path
    /// stores the same value the Done screen shows.
    pub(super) fn finish_overtime_session(&self) {
        if self.ui_state() != UiState::Overtime {
            return;
        }
        let target = self.countdown_target_secs.get();
        let elapsed = self
            .core_session_end(|s, _now| s.finish_overtime())
            .unwrap_or(target);
        self.end_overtime_session(elapsed);
    }

    /// Run a terminating call against the in-flight Session,
    /// dispatch any portable side effects (StopActiveSignals etc.)
    /// to their shell handlers, and extract the
    /// EndSession.duration_secs for callers that need to pin the
    /// saved duration. Returns `None` when no Session is alive.
    fn core_session_end<F>(&self, f: F) -> Option<u64>
    where
        F: FnOnce(&mut CoreSession, Duration) -> Vec<CoreSessionEffect>,
    {
        let now = self.elapsed_since_start();
        let effects = {
            let mut slot = self.core_session.borrow_mut();
            let s = slot.as_mut()?;
            f(s, now)
        };
        let duration = effects.iter().find_map(|e| match e {
            CoreSessionEffect::EndSession { duration_secs } => Some(*duration_secs),
            _ => None,
        });
        self.dispatch_session_effects(&effects);
        duration
    }

    fn end_overtime_session(&self, elapsed_secs: u64) {
        // Cutting in-flight signals is Session's call — it fires
        // StopActiveSignals from finish_overtime / add_overtime_
        // and_finish, dispatched via core_session_end above.
        self.cancel_tick();
        *self.running_label.borrow_mut() = None;
        *self.running_pause_btn.borrow_mut() = None;
        *self.running_stop_btn.borrow_mut() = None;
        *self.overtime_add_btn.borrow_mut() = None;
        // Session is in `Stopped` with `elapsed_secs` stashed —
        // `ui_state()` flips to Done off that.
        if let Some(app) = self.get_app() {
            self.release_screen_awake_lock(&app);
        }
        self.obj().emit_by_name::<()>("timer-stopped", &[]);
        self.show_done(elapsed_secs);
    }

    /// Natural completion path for a breath session: marks Done, plays the
    /// end chime, vibrates, and sends a notification when not focused.
    /// Mirrors the countdown's done branch (timer.imp at the 1 Hz tick).
    /// Distinct from `on_stop` (user-initiated), which is silent.
    pub(super) fn finish_breath_session(&self) {
        // tick_box_breath already transitioned Session to Stopped
        // and stashed `duration_secs` into Session::final_duration_secs;
        // both survive until reset_mode drops the Session at user
        // dismissal of the Done view.
        let elapsed = self
            .session_final_duration_secs()
            .unwrap_or_else(|| self.breath_elapsed().as_secs());
        // Release running-page widget refs — the page pops next.
        *self.running_label.borrow_mut() = None;
        *self.running_pause_btn.borrow_mut() = None;
        if let Some(app) = self.get_app() {
            self.release_screen_awake_lock(&app);
        }
        self.obj().emit_by_name::<()>("timer-stopped", &[]);
        self.show_done(elapsed);
        // End bell fires via Session's FireEndBell effect — emitted
        // on EndBoxBreath in tick_box_breath, dispatched through
        // dispatch_session_effects on the frame-tick callback before
        // we get here. System notification stays in gtk.
        if let Some(app) = self.get_app() {
            if !app.active_window().map(|w| w.is_active()).unwrap_or(false) {
                let n = gtk::gio::Notification::new("Meditation Complete");
                let target = self
                    .core_session
                    .borrow()
                    .as_ref()
                    .map(|s| s.completion_duration_secs())
                    .unwrap_or(elapsed);
                n.set_body(Some(&format!("Session: {}", format_time(Duration::from_secs(target)))));
                app.send_notification(Some("timer-done"), &n);
            }
        }
    }

    /// Countdown remaining seconds (ceiling), 0 if no session running.
    /// Wall-clock-anchored, pause-frozen elapsed of the in-flight
    /// session. Returns ZERO if no session is running. Sole gtk-side
    /// reader of session elapsed post Stage 6 — every per-mode
    /// helper that used to dispatch into a Cell now collapses here.
    fn session_elapsed(&self) -> Duration {
        let now = self.elapsed_since_start();
        self.core_session
            .borrow()
            .as_ref()
            .map(|s| s.elapsed(now))
            .unwrap_or_default()
    }

    /// High-level UI state — delegated to the portable Session;
    /// `None` (no in-flight session) → `UiState::Idle`. Cheap: one
    /// RefCell borrow + a couple of comparisons inside core.
    pub(crate) fn ui_state(&self) -> UiState {
        meditate_core::session::ui_state(self.core_session.borrow().as_ref())
    }

    /// Wall-clock-anchored elapsed time of the active breath session.
    /// Returns ZERO if no session is running. Pause freezes this value.
    /// Surfaced for the window's per-frame Box-Breath callback (the
    /// dot's perimeter position is a function of elapsed); all other
    /// callers go through `Session::display_secs`.
    pub(super) fn breath_elapsed(&self) -> std::time::Duration {
        self.session_elapsed()
    }

    /// Tick the portable Session against the current boot-anchored
    /// elapsed; used by the box-breath per-frame callback in
    /// window/imp.rs. Returns the effects (FireBoxBreathCue + the
    /// cycle-aligned EndBoxBreath) so the caller can dispatch them
    /// without touching `core_session` directly. Empty Vec when no
    /// session is in flight or the session is paused.
    pub(crate) fn box_breath_session_tick(&self) -> Vec<CoreSessionEffect> {
        let now = self.elapsed_since_start();
        self.core_session
            .borrow_mut()
            .as_mut()
            .map(|s| s.tick(now))
            .unwrap_or_default()
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

        // Restore the persisted Timer-mode countdown target and
        // Box-Breath pattern / session length. Done here because
        // `constructed()` runs before the widget is rooted, so
        // `get_app()` would return None — same reason every other
        // persisted Setup-view value is loaded from this function
        // rather than constructed. `refresh_phase_tiles` reflects the
        // freshly-loaded pattern into the visible phase value labels.
        self.load_timer_settings();
        self.load_breathing_settings();
        self.refresh_phase_tiles();

        // Batch every DB read this visit needs into a single borrow:
        // one get_app() walk, one RefCell lock, four SQL queries instead
        // of as many separate calls. The bells block also rides along —
        // four extra get_setting() calls are cheap next to the existing
        // streak / labels SQL we're already running.
        let stopwatch_key = stopwatch_key_for_mode(self.current_mode());
        let (streak, stopwatch_on, bells, intervals) = app
            .with_db(|db| {
                use meditate_core::read_bool;
                let core_db = db.core();
                let streak  = db.get_streak().unwrap_or(0);
                let stopwatch_on = read_bool(core_db, stopwatch_key, false);
                let starting_bell_on = read_bool(core_db, "starting_bell_active", false);
                let starting_bell_sound = db
                    .get_setting("starting_bell_sound", "bowl")
                    .unwrap_or_else(|_| "bowl".to_string());
                let prep_on = read_bool(core_db, "preparation_time_active", false);
                let prep_secs = db
                    .get_setting(
                        "preparation_time_secs",
                        &meditate_core::format::PREP_SECS_DEFAULT.to_string(),
                    )
                    .map(|s| meditate_core::format::parse_prep_secs(&s))
                    .unwrap_or(meditate_core::format::PREP_SECS_DEFAULT);
                let intervals_on = read_bool(core_db, "interval_bells_active", false);
                // Stopwatch mode mutes fixed-from-end bells — no end
                // to count backwards from. The persisted enabled flag
                // stays untouched (returns when stopwatch flips off);
                // the UI subtitle just reflects what will actually
                // fire right now.
                let intervals_enabled_count = meditate_core::bells::interval_bells_count(
                    core_db, stopwatch_on,
                );
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

        // Per-mode Keep-Screen-Awake switch.
        self.refresh_keep_screen_awake_state();

        // Update streak label. .streak-chip applies text-transform:
        // uppercase, so we keep the source text sentence-case here.
        // Core picks the variant; the gtk shell maps each to a
        // gettext-translated phrase at the i18n boundary.
        use meditate_core::format::StreakKey;
        let text = match meditate_core::format::streak_key(streak) {
            StreakKey::Zero => crate::i18n::gettext("Start your streak today"),
            StreakKey::One => crate::i18n::gettext("1 day streak"),
            StreakKey::Many(n) => crate::i18n::gettext("{n} days streak")
                .replace("{n}", &n.to_string()),
        };
        self.streak_label.set_label(&text);

        // Rebuild visible starred-preset list for the current mode.
        self.rebuild_starred_presets_list();
        // Sync the duration row's value label with the current target.
        self.duration_value_label.set_label(&meditate_core::format::format_hhmm(
            self.countdown_target_secs.get(),
        ));

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

        let session_mode: crate::db::SessionMode = self.current_mode().into();
        // Guided mode rebuilds via `rebuild_starred_guided_list`
        // instead of this path. on_mode_switched routes them.
        if !meditate_core::preset_config::mode_supports_presets(session_mode) {
            return;
        }
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
                .subtitle(preset_subtitle(&p, &label_names))
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
    fn snapshot_current_setup(&self) -> meditate_core::preset_config::PresetConfig {
        use meditate_core::preset_config::{
            snapshot, PresetBoxBreathCues, PresetConfig, PresetEndBell,
            PresetIntervalBells, PresetLabel, PresetStartingBell, PresetTiming,
        };
        let mode = self.current_mode();

        // Build the live-UI-state half from gtk Cell<>s. Core's
        // snapshot consumes this + reads everything else from `db`.
        let timing = match mode {
            TimerMode::Timer => PresetTiming::Timer {
                stopwatch: self.stopwatch_toggle_on.get(),
                duration_secs: self.countdown_target_secs.get() as u32,
            },
            TimerMode::Breathing => {
                let p = self.breathing_pattern.get();
                PresetTiming::BoxBreath {
                    stopwatch:      self.stopwatch_toggle_on.get(),
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

        // App / DB unavailable: return a defaults-shaped PresetConfig
        // with the timing we just built. Unreachable in normal flow
        // (snapshot is called from a UI handler that has an app).
        self.get_app()
            .and_then(|app| {
                app.with_db(|db| snapshot(db.core(), mode.into(), timing.clone()))
            })
            .unwrap_or_else(|| PresetConfig {
                label: PresetLabel::default(),
                starting_bell: PresetStartingBell::default(),
                interval_bells: PresetIntervalBells::default(),
                end_bell: PresetEndBell::default(),
                timing,
                cues_signal_mode: "both".to_string(),
                keep_screen_awake: false,
                box_breath_cues: PresetBoxBreathCues::default(),
            })
    }

    /// Apply a `PresetConfig` to the live Setup state. Delegates the
    /// persistence work (settings + interval-bell replay +
    /// box-breath phase rows) to `meditate_core::preset_config::apply`,
    /// then unpacks the returned timing into the gtk-side Cell<>s and
    /// widgets, and finishes with `refresh_streak` so dependent rows
    /// converge.
    ///
    /// Returns true iff the apply happened. Returns false on
    /// `ApplyError::SyncPending` (referenced bell sound or vibration
    /// pattern hasn't arrived locally yet) or any underlying DB error;
    /// callers can decide how to surface that to the user.
    fn apply_config(&self, cfg: &meditate_core::preset_config::PresetConfig) -> bool {
        use meditate_core::preset_config::{apply, PresetTiming};
        let Some(app) = self.get_app() else { return false; };
        let mode = self.current_mode();

        let outcome = app.with_db_mut(|db| apply(db.core(), cfg, mode.into()));
        let timing = match outcome {
            Some(Ok(t)) => t,
            // SyncPending, DbError, or app/db unavailable.
            _ => return false,
        };

        // Apply mode-specific live state from the timing core just
        // wrote. The shell owns the gtk-side reactive plumbing.
        match timing {
            PresetTiming::Timer { stopwatch, duration_secs } => {
                self.set_countdown_target(duration_secs as u64);
                self.stopwatch_loading.set(true);
                self.stopwatch_mode_row.set_active(stopwatch);
                self.stopwatch_toggle_on.set(stopwatch);
                self.stopwatch_loading.set(false);
            }
            PresetTiming::BoxBreath {
                stopwatch,
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
                self.stopwatch_loading.set(true);
                self.stopwatch_mode_row.set_active(stopwatch);
                self.stopwatch_toggle_on.set(stopwatch);
                self.stopwatch_loading.set(false);
            }
        }

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
        use meditate_core::preset_config::PresetConfig;

        let Some(app) = self.get_app() else { return; };
        let preset = match app.with_db(|db| db.find_preset_by_uuid(uuid)) {
            Some(Ok(Some(p))) => p,
            _ => return,
        };
        let cfg = match PresetConfig::from_json(&preset.config_json) {
            Ok(c) => c,
            Err(_) => return,
        };
        let want_session_mode: crate::db::SessionMode = self.current_mode().into();
        // Preset rows aren't surfaced in Guided mode; this would
        // only fire from a stale callback retained across a mode
        // switch. Refuse rather than mutating Setup state.
        if !meditate_core::preset_config::mode_supports_presets(want_session_mode) {
            return;
        }
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

    fn set_countdown_target(&self, secs: u64) {
        self.countdown_target_secs.set(secs);
        let label = meditate_core::format::format_hhmm(secs);
        self.big_time_label.set_label(&label);
        self.duration_value_label.set_label(&label);
        if self.timer_populating.get() { return; }
        if let Some(app) = self.get_app() {
            let _ = app.with_db_mut(|db| {
                db.set_setting("timer_session_secs", &secs.to_string())
            });
        }
    }

    /// Restore the persisted Timer-mode countdown target. Falls back
    /// to the 10-min default the Cell was initialised with if the
    /// setting is missing or unparseable. The `timer_populating` guard
    /// suppresses the write-back that `set_countdown_target` would
    /// otherwise do.
    fn load_timer_settings(&self) {
        let Some(app) = self.get_app() else { return; };
        let default = meditate_core::session::TIMER_DEFAULT_SECS;
        let secs = app.with_db(|db| {
            db.get_setting("timer_session_secs", &default.to_string())
                .ok()
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(default)
        }).unwrap_or(default);
        self.timer_populating.set(true);
        self.set_countdown_target(secs);
        self.timer_populating.set(false);
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

    fn resolve_label_for_mode(&self, mode: TimerMode) -> Option<Label> {
        self.get_app()?.with_db(|db| {
            meditate_core::labels::resolve_label_for_mode(db.core(), mode.into())
        }).flatten()
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
        // The portable Session encapsulates "what number does the
        // hero show right now?" across every phase (Prep / Running
        // / Overtime) and every mode (Timer / Box Breath / Guided)
        // — ceiling vs floor + the stopwatch_display override are
        // all internal to `Session::display_secs`.
        let now = self.elapsed_since_start();
        self.core_session
            .borrow()
            .as_ref()
            .map(|s| s.display_secs(now))
            .unwrap_or(0)
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
        use meditate_core::session::ToggleAction;
        match meditate_core::session::Session::toggle_action(self.ui_state()) {
            ToggleAction::Start => self.on_start(),
            ToggleAction::Pause => self.on_pause(),
            ToggleAction::FinishOvertime => self.finish_overtime_session(),
            ToggleAction::Resume => self.on_resume(),
            ToggleAction::NoOp => {}
        }
    }
}

/// Which `MEDIA` slot a fire-cue effect routes through. Three
/// slots so polyphony works the way users expect: starting bell
/// supersedes its own prior playback; end bell supersedes its
use meditate_core::session::FireChannel;

// ── Interval / fixed bell scheduling ─────────────────────────────────────────

impl TimerView {
    /// Build the per-session bell schedule + seed for the running
    /// Session. Reads the user's bell library from the DB, applies
    /// the master-toggle gate (`interval_bells_active`), and hands
    /// the rest to `meditate_core::bells::build_active_bells` —
    /// schedule construction + jitter rolls live in core. Returns
    /// `(empty Vec, fresh seed)` when the master toggle is off so
    /// Session's per-tick check has nothing to do.
    // Session-config builders — one-line wrappers around the
    // core::bells::*_from_db readers so the shell's setup-state
    // assembly hands the same SessionSettings to Session that the
    // Android shell will. The math lives in core; this is just the
    // `app.with_db(...)` ceremony.

    fn build_session_bells(
        &self,
        total_target_secs: Option<u64>,
        stopwatch_on: bool,
    ) -> (Vec<ActiveBell>, u64) {
        self.get_app()
            .and_then(|app| {
                app.with_db(|db| {
                    meditate_core::bells::session_bells_from_db(
                        db.core(),
                        total_target_secs,
                        stopwatch_on,
                    )
                })
            })
            .unwrap_or_else(|| (Vec::new(), meditate_core::time::seed_now()))
    }

    fn read_signal_mode_override(
        &self,
        app: &crate::application::MeditateApplication,
        mode: SessionMode,
    ) -> crate::db::SignalMode {
        app.with_db(|db| meditate_core::bells::signal_mode_override_from_db(db.core(), mode))
            .unwrap_or(crate::db::SignalMode::Both)
    }

    fn build_starting_bell_cue(
        &self,
        app: &crate::application::MeditateApplication,
    ) -> Option<meditate_core::bells::BellCue> {
        app.with_db(|db| meditate_core::bells::starting_bell_cue_from_db(db.core()))
            .flatten()
    }

    fn build_end_bell_cue(
        &self,
        app: &crate::application::MeditateApplication,
    ) -> Option<meditate_core::bells::BellCue> {
        let stopwatch_on = self.stopwatch_toggle_on.get();
        app.with_db(|db| meditate_core::bells::end_bell_cue_from_db(db.core(), stopwatch_on))
            .flatten()
    }

    fn build_box_breath_cues(
        &self,
        app: &crate::application::MeditateApplication,
    ) -> meditate_core::bells::BoxBreathCueConfig {
        app.with_db(|db| meditate_core::bells::box_breath_cues_from_db(db.core()))
            .unwrap_or_default()
    }
}

// ── Public refresh hooks ─────────────────────────────────────────────────────

impl TimerView {
    /// Refresh just the "Manage Bells" subtitle. Called when the user
    /// pops back from the bell-library page so the count stays in sync
    /// without us having to invalidate the whole streak/presets/labels
    /// read in refresh_streak.
    pub(crate) fn refresh_interval_bells_count(&self) {
        let mode = self.current_mode();
        let count = self
            .get_app()
            .and_then(|app| {
                app.with_db(|db| {
                    let stopwatch_on = meditate_core::read_bool(
                        db.core(), stopwatch_key_for_mode(mode), false,
                    );
                    meditate_core::bells::interval_bells_count(db.core(), stopwatch_on)
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

    /// Cut any in-flight bell sound + vibration pattern. Sole shell-
    /// side implementation of `Effect::StopActiveSignals`; only
    /// called from `dispatch_session_effects` and from `on_save` /
    /// `on_discard` (which are pure shell flows with no Session
    /// alive). The decision *when* to stop signals belongs to
    /// Session — see the variant's doc-comment in core.
    fn stop_active_signals(&self) {
        crate::sound::stop_all();
        self.install_vibration_handle(None);
    }

    /// Run portable Session effects through their gtk-shell native
    /// dispatchers. Handles `StopActiveSignals` (sound + vibration
    /// cancel) and the four fire-* variants
    /// (`FireBell` / `FireStartingBell` / `FireEndBell` /
    /// `FireBoxBreathCue`). All four carry an *effective*
    /// `signal_mode` (Session already AND'd per-cue with per-mode
    /// override) so the shell just plays / vibrates per that
    /// variant. Other effects (UpdateDisplay / EnterOvertime /
    /// EndSession / EndPrep / UpdateOvertimeLabel / EndBoxBreath)
    /// are consumed at their tick callsites.
    pub(crate) fn dispatch_session_effects(&self, effects: &[CoreSessionEffect]) {
        let mut app = None;
        for effect in effects {
            if matches!(effect, CoreSessionEffect::StopActiveSignals) {
                self.stop_active_signals();
            }
            if let Some(route) = effect.fire_route() {
                self.dispatch_fire_route(&mut app, &route);
            }
        }
    }

    /// Shared fire-cue dispatch routed off `Effect::fire_route`: per
    /// the effective signal_mode (already AND'd with per-mode
    /// override by Session), play the sound via the right MEDIA
    /// slot, fire the haptic pattern when device supports it, stash
    /// the resulting handle.
    fn dispatch_fire_route(
        &self,
        app_cache: &mut Option<Option<crate::application::MeditateApplication>>,
        route: &meditate_core::session::FireRoute<'_>,
    ) {
        let app = app_cache.get_or_insert_with(|| self.get_app());
        let Some(app) = app.as_ref() else { return; };
        if route.signal_mode.includes_sound() {
            match route.channel {
                FireChannel::Interval => crate::sound::play_interval_sound(route.sound_uuid, app),
                FireChannel::Starting => crate::sound::play_starting_uuid(route.sound_uuid, app),
                FireChannel::End => crate::sound::play_end_bell_uuid(route.sound_uuid, app),
            }
        }
        let handle = if route.signal_mode.includes_vibration() && app.has_haptic() {
            app.with_db(|db| db.find_vibration_pattern_by_uuid(route.vibration_pattern_uuid))
                .and_then(|r| r.ok())
                .flatten()
                .map(|pattern| crate::vibration::PatternPlayback::play(app, &pattern))
        } else {
            None
        };
        meditate_core::log(&format!(
            "{}: signal_mode={} fired={}",
            route.log_tag,
            route.signal_mode.as_db_str(),
            handle.is_some(),
        ));
        self.install_vibration_handle(handle);
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
        app.with_db(|db| {
            let uuid = db.get_setting(setting_key, crate::db::BUNDLED_BOWL_UUID)
                .unwrap_or_default();
            meditate_core::bells::resolve_sound_name(db.core(), &uuid)
        }).unwrap_or_default()
    }

    fn lookup_pattern_name_for_setting(&self, setting_key: &str) -> String {
        let Some(app) = self.get_app() else { return String::new(); };
        app.with_db(|db| {
            let uuid = db
                .get_setting(setting_key, crate::db::BUNDLED_PATTERN_PULSE_UUID)
                .unwrap_or_default();
            meditate_core::bells::resolve_pattern_name(db.core(), &uuid)
        }).unwrap_or_default()
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Subtitle text for the "Manage Bells" row reflecting how many of the
/// library's bells are currently enabled. Uses gettext so the count
/// can be localised; "None" is its own string for grammatical reasons
/// in some languages.
fn intervals_count_subtitle(enabled_count: usize) -> String {
    use meditate_core::format::IntervalsCountKey;
    match meditate_core::format::intervals_count_key(enabled_count) {
        IntervalsCountKey::None => crate::i18n::gettext("None enabled"),
        IntervalsCountKey::One => crate::i18n::gettext("1 enabled"),
        IntervalsCountKey::Many(n) => {
            crate::i18n::gettext("{n} enabled").replace("{n}", &n.to_string())
        }
    }
}

use crate::preset_subtitle::preset_subtitle;

use meditate_core::time::unix_now;

// ── Breathing (Box Breath) setup wiring ───────────────────────────────────────

// Per-phase + cycle invariants live in meditate_core::breath.
use meditate_core::breath::{clamp_session_secs, MIN_CYCLE_SECS, PHASE_MAX_SECS};

impl TimerView {
    fn build_breathing_setup(&self) {
        self.build_phase_tiles();
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
        let min_val: u32 = BreathPattern::phase_min_secs(index);

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
        let secs = clamp_session_secs(secs);
        self.breathing_session_secs.set(secs);
        self.save_breathing_settings();
        // Duration row label is shared between modes; reflect the new
        // value here so a Box-Breath edit shows up immediately. H:MM
        // format matches Timer mode.
        self.duration_value_label
            .set_label(&meditate_core::format::format_hhmm(secs as u64));
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
        if p.cycle().as_secs() < MIN_CYCLE_SECS as u64 {
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
            let p = BreathPattern::clamp_from_raw(
                read("breathing_in", 4),
                read("breathing_hold_in", 4),
                read("breathing_out", 4),
                read("breathing_hold_out", 4),
            );
            let secs = clamp_session_secs(read(
                "breathing_session_secs",
                meditate_core::session::BREATHING_DEFAULT_SECS,
            ));
            (p, secs)
        }).unwrap_or((
            BreathPattern::box_breath(),
            meditate_core::session::BREATHING_DEFAULT_SECS,
        ));
        self.breathing_pattern.set(p);
        self.breathing_session_secs.set(secs);
        // The shared Duration row reflects whichever Cell the current
        // mode reads; reflect this load even if the user is currently
        // viewing Timer mode — switching to Box Breath later will
        // already have the right value visible.
        self.duration_value_label
            .set_label(&meditate_core::format::format_hhmm(secs as u64));
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
        self.get_app()
            .and_then(|app| {
                app.with_db(|db| {
                    meditate_core::labels::label_active_from_db(db.core(), mode.into())
                })
            })
            .unwrap_or(mode == TimerMode::Guided)
    }

    fn persist_label_active_for_mode(&self, mode: TimerMode, on: bool) {
        let Some(app) = self.get_app() else { return; };
        app.with_db_mut(|db| {
            let _ = meditate_core::labels::persist_active_for_mode(db.core(), mode.into(), on);
        });
    }

    fn persisted_label_uuid_for_mode(&self, mode: TimerMode) -> Option<String> {
        self.get_app()?.with_db(|db| {
            meditate_core::labels::label_uuid_from_db(db.core(), mode.into())
        }).flatten()
    }

    fn persist_label_uuid_for_mode(&self, mode: TimerMode, uuid: &str) {
        let Some(app) = self.get_app() else { return; };
        app.with_db_mut(|db| {
            let _ = meditate_core::labels::persist_uuid_for_mode(db.core(), mode.into(), uuid);
        });
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    // ── Per-mode setting-key helpers ─────────────────────────────────────

    #[test]
    fn setting_key_for_mode_uses_distinct_keys_per_mode() {
        // The whole point of these helpers is that no two modes
        // share a key — otherwise the per-mode toggles would leak
        // into each other.
        let timer = setting_key_for_mode(TimerMode::Timer);
        let guided = setting_key_for_mode(TimerMode::Guided);
        let breath = setting_key_for_mode(TimerMode::Breathing);
        assert_ne!(timer, guided);
        assert_ne!(timer, breath);
        assert_ne!(guided, breath);
    }

    #[test]
    fn keep_screen_awake_key_for_mode_uses_distinct_keys_per_mode() {
        let timer = keep_screen_awake_key_for_mode(TimerMode::Timer);
        let guided = keep_screen_awake_key_for_mode(TimerMode::Guided);
        let breath = keep_screen_awake_key_for_mode(TimerMode::Breathing);
        assert_ne!(timer, guided);
        assert_ne!(timer, breath);
        assert_ne!(guided, breath);
    }

    #[test]
    fn keep_screen_awake_key_does_not_collide_with_signal_mode_key() {
        // signal-mode and keep-screen-awake are independent per-mode
        // settings; sharing a key would mean toggling one writes the
        // other's value.
        for mode in [TimerMode::Timer, TimerMode::Guided, TimerMode::Breathing] {
            assert_ne!(
                setting_key_for_mode(mode),
                keep_screen_awake_key_for_mode(mode),
                "{mode:?}: signal-mode and keep-awake keys must differ",
            );
        }
    }

    #[test]
    fn stopwatch_key_for_mode_uses_distinct_keys_per_mode() {
        let timer = stopwatch_key_for_mode(TimerMode::Timer);
        let guided = stopwatch_key_for_mode(TimerMode::Guided);
        let breath = stopwatch_key_for_mode(TimerMode::Breathing);
        assert_ne!(timer, guided);
        assert_ne!(timer, breath);
        assert_ne!(guided, breath);
    }

    #[test]
    fn stopwatch_key_does_not_collide_with_other_per_mode_keys() {
        // signal-mode, keep-screen-awake, and stopwatch are three
        // independent per-mode flags. Any collision would mean
        // toggling one persists into another, scrambling the
        // remembered settings across pageloads.
        for mode in [TimerMode::Timer, TimerMode::Guided, TimerMode::Breathing] {
            let sw = stopwatch_key_for_mode(mode);
            assert_ne!(sw, setting_key_for_mode(mode),
                "{mode:?}: stopwatch and signal-mode keys must differ");
            assert_ne!(sw, keep_screen_awake_key_for_mode(mode),
                "{mode:?}: stopwatch and keep-awake keys must differ");
        }
    }
}
