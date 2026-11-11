use meditate_core::db::Refresh;
use std::cell::{Cell, RefCell};
use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::{glib, CompositeTemplate};
use glib::subclass::Signal;
use std::sync::OnceLock;

use crate::db::{BoxBreathPhaseId, Label, SessionData, SessionMode};
use crate::i18n::{gettext, ngettext};
use glib::clone;

use std::time::Duration;
use meditate_core::bell_volume::BellSlot;
use meditate_core::breath::BreathPattern;
use meditate_core::format::format_time;
use meditate_core::time::boot_time_now;

use meditate_core::session::{
    Effect as CoreSessionEffect,
    Session as CoreSession,
    SessionSettings as CoreSessionSettings,
    SessionShape as CoreSessionShape,
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
/// The mode toggle's names (`timer_view.blp`) and their modes.
const MODE_NAMES: [(&str, TimerMode); 3] =
    [("timer", TimerMode::Timer), ("guided", TimerMode::Guided), ("breathing", TimerMode::Breathing)];

impl From<TimerMode> for meditate_core::SessionMode {
    fn from(m: TimerMode) -> Self {
        match m {
            TimerMode::Timer => meditate_core::SessionMode::Timer,
            TimerMode::Breathing => meditate_core::SessionMode::BoxBreath,
            TimerMode::Guided => meditate_core::SessionMode::Guided,
        }
    }
}


/// The running page's end of a session, built by the window.
#[derive(Debug)]
pub struct RunningControls {
    pub first: gtk::Button,
    pub second: gtk::Button,
    /// The note caption and field, shown or hidden together.
    pub note: gtk::Widget,
    pub note_view: gtk::TextView,
}

// ── GObject impl ──────────────────────────────────────────────────────────────

/// A Setup-view bell Volume row. The end bell's row edits the current
/// mode's end bell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SetupVolume {
    Starting,
    End,
    Cue(BoxBreathPhaseId),
}

impl SetupVolume {
    fn slot(self, mode: meditate_core::SessionMode) -> BellSlot {
        match self {
            Self::Starting => BellSlot::Starting,
            Self::End => BellSlot::End(mode),
            Self::Cue(phase) => BellSlot::BoxBreathCue(phase),
        }
    }
}

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
    #[template_child] pub auto_finish_row:        TemplateChild<adw::SwitchRow>,
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
    #[template_child] pub local_folder_btn:      TemplateChild<gtk::Button>,
    #[template_child] pub phase_tiles_grid:       TemplateChild<gtk::Grid>,
    #[template_child] pub start_btn:             TemplateChild<gtk::Button>,
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
    #[template_child] pub starting_bell_volume_row:         TemplateChild<adw::PreferencesRow>,
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
    #[template_child] pub end_bell_volume_row:       TemplateChild<adw::PreferencesRow>,
    #[template_child] pub end_bell_pattern_revealer: TemplateChild<gtk::Revealer>,
    #[template_child] pub end_bell_pattern_row:      TemplateChild<adw::ActionRow>,
    // Vibration UI prototype — see setup_vibration_proto. Throwaway.
    #[template_child] pub boxbreath_phase_section:         TemplateChild<adw::Clamp>,
    #[template_child] pub boxbreath_master_row:           TemplateChild<adw::ExpanderRow>,
    #[template_child] pub boxbreath_phase_in_row:                  TemplateChild<adw::ExpanderRow>,
    #[template_child] pub boxbreath_phase_in_signal_toggle_host:   TemplateChild<gtk::Box>,
    #[template_child] pub boxbreath_phase_in_sound_revealer:       TemplateChild<gtk::Revealer>,
    #[template_child] pub boxbreath_phase_in_sound_row:            TemplateChild<adw::ActionRow>,
    #[template_child] pub boxbreath_phase_in_volume_row:            TemplateChild<adw::PreferencesRow>,
    #[template_child] pub boxbreath_phase_in_pattern_revealer:     TemplateChild<gtk::Revealer>,
    #[template_child] pub boxbreath_phase_in_pattern_row:          TemplateChild<adw::ActionRow>,
    #[template_child] pub boxbreath_phase_holdin_row:                  TemplateChild<adw::ExpanderRow>,
    #[template_child] pub boxbreath_phase_holdin_signal_toggle_host:   TemplateChild<gtk::Box>,
    #[template_child] pub boxbreath_phase_holdin_sound_revealer:       TemplateChild<gtk::Revealer>,
    #[template_child] pub boxbreath_phase_holdin_sound_row:            TemplateChild<adw::ActionRow>,
    #[template_child] pub boxbreath_phase_holdin_volume_row:            TemplateChild<adw::PreferencesRow>,
    #[template_child] pub boxbreath_phase_holdin_pattern_revealer:     TemplateChild<gtk::Revealer>,
    #[template_child] pub boxbreath_phase_holdin_pattern_row:          TemplateChild<adw::ActionRow>,
    #[template_child] pub boxbreath_phase_out_row:                  TemplateChild<adw::ExpanderRow>,
    #[template_child] pub boxbreath_phase_out_signal_toggle_host:   TemplateChild<gtk::Box>,
    #[template_child] pub boxbreath_phase_out_sound_revealer:       TemplateChild<gtk::Revealer>,
    #[template_child] pub boxbreath_phase_out_sound_row:            TemplateChild<adw::ActionRow>,
    #[template_child] pub boxbreath_phase_out_volume_row:            TemplateChild<adw::PreferencesRow>,
    #[template_child] pub boxbreath_phase_out_pattern_revealer:     TemplateChild<gtk::Revealer>,
    #[template_child] pub boxbreath_phase_out_pattern_row:          TemplateChild<adw::ActionRow>,
    #[template_child] pub boxbreath_phase_holdout_row:                  TemplateChild<adw::ExpanderRow>,
    #[template_child] pub boxbreath_phase_holdout_signal_toggle_host:   TemplateChild<gtk::Box>,
    #[template_child] pub boxbreath_phase_holdout_sound_revealer:       TemplateChild<gtk::Revealer>,
    #[template_child] pub boxbreath_phase_holdout_sound_row:            TemplateChild<adw::ActionRow>,
    #[template_child] pub boxbreath_phase_holdout_volume_row:            TemplateChild<adw::PreferencesRow>,
    #[template_child] pub boxbreath_phase_holdout_pattern_revealer:     TemplateChild<gtk::Revealer>,
    #[template_child] pub boxbreath_phase_holdout_pattern_row:          TemplateChild<adw::ActionRow>,
    #[template_child] pub time_unit_label:        TemplateChild<gtk::Label>,
    #[template_child] pub time_btn:               TemplateChild<gtk::Button>,
    #[template_child] pub session_summary_row:    TemplateChild<adw::ActionRow>,
    #[template_child] pub bells_summary_row:      TemplateChild<adw::ActionRow>,
    #[template_child] pub session_page:           TemplateChild<adw::NavigationPage>,
    #[template_child] pub bells_page:             TemplateChild<adw::NavigationPage>,
    #[template_child] pub session_toolbar:        TemplateChild<adw::ToolbarView>,
    #[template_child] pub bells_toolbar:          TemplateChild<adw::ToolbarView>,

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
    /// Bumped by every stop of the signals: a vibration still waiting
    /// for its bell's lead-in is dropped.
    signal_gen: Cell<u64>,
    /// Cookie returned by `gtk::Application::inhibit` while a session
    /// is running and the active mode's keep-screen-awake toggle is
    /// on. 0 means "no inhibit held". Released on every `timer-stopped`
    /// emit site (user-stop, countdown finish, breath finish).
    screen_awake_cookie: Cell<u32>,
    /// Weak ref to the running-page time label for live updates.
    running_label: RefCell<Option<gtk::Label>>,
    /// The running page's buttons and note, set from core's button
    /// set by `refresh_end_buttons`. Dropped when the page pops.
    running_controls: RefCell<Option<RunningControls>>,
    /// Overtime so far, for the Add button's label.
    overtime_delta: Cell<Duration>,
    /// True while a label-row update is being applied programmatically
    /// (mode switch, show_done refresh) — suppresses the
    /// enable_expansion_notify / activated callbacks so they don't
    /// re-write the same value back to settings or open a chooser.
    labels_loading: Cell<bool>,
    /// Currently-selected countdown duration in seconds, set by preset
    /// chips or the "Custom" dialog. Default 10 min; used as the target
    /// when the user taps Start (and Stopwatch Mode is off).
    countdown_target_secs: Cell<u32>,
    /// Live mirror of the active mode's persisted stopwatch flag and
    /// of `stopwatch_mode_row`'s active state. `true` means count up
    /// from zero with no target; `false` means count down to
    /// `countdown_target_secs`.
    pub(super) stopwatch_toggle_on: Cell<bool>,
    /// Suppress the SwitchRow's notify::active handler while
    /// `refresh_streak` is loading the persisted setting on visit.
    stopwatch_loading: Cell<bool>,
    /// A rebuild queued by a write (`queue_presets_rebuild`).
    presets_rebuild_queued: RefCell<Option<glib::SourceId>>,
    /// Suppress notify handlers on the four bell-related rows
    /// (Starting-Bell switch, Bell-Sound combo, Preparation-Time switch,
    /// Preparation-Time SpinRow) while `refresh_streak` is loading their
    /// persisted values on visit. One flag covers all four because they
    /// load atomically in the same DB roundtrip.
    bells_loading: Cell<bool>,
    /// The Setup view's bell Volume rows, set from stored state by
    /// `refresh_volume_rows`.
    volume_rows: RefCell<Vec<(SetupVolume, crate::volume_row::VolumeRow)>>,
    /// Starred-preset rows currently attached to `presets_group`,
    /// paired with their preset uuid. Tracked so the list can be
    /// rebuilt cleanly on mode switch / sync update without leaking
    /// rows from the previous mode.
    starred_preset_rows: RefCell<Vec<(adw::ActionRow, String)>>,
    /// A Save write is in flight; a second tap must not insert twice.
    saving: Cell<bool>,
    /// The open "Discard Session?" question, closed when the session
    /// saves itself under it.
    discard_dialog: RefCell<Option<adw::AlertDialog>>,

    // ── Guided meditation state ──────────────────────────────────────
    /// Transient "Open File" pick — set when the user picks a file via
    /// the file dialog, cleared when they tap Import File (which
    /// promotes it into the library) or pick a starred row from the
    /// list. Drives the hero countdown's target during a guided run.
    guided_pick: RefCell<Option<crate::guided::GuidedFilePick>>,
    /// The last mode and file were read back: only from then on may a
    /// shown selection or mode overwrite them.
    choice_restored: Cell<bool>,
    /// UUID of the currently-selected library row, when the user has
    /// tapped a row in the starred list. `None` for transient picks
    /// AND for the empty state. The session-save path reads this so
    /// per-file stats can resolve later.
    guided_selected_uuid: RefCell<Option<String>>,
    /// The selected library file a delete cleared, so its Undo can
    /// select it again.
    guided_cleared_uuid: RefCell<Option<String>>,
    /// Starred guided-file rows currently attached to the home-list
    /// group. Same shape as `starred_preset_rows` so the rebuild path
    /// can drain and re-add cleanly without leaking rows.
    starred_guided_rows: RefCell<Vec<adw::ActionRow>>,
    /// The pickable starred rows by key (`<uuid>` or `local:<id>`) with
    /// their check, so the selected one is marked in place (#189).
    starred_guided_checks: RefCell<Vec<(String, adw::ActionRow, gtk::Image)>>,
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
    /// every mode and phase.
    /// `Some` between start_session and on_stop / finish_overtime /
    /// add_overtime_and_finish; `None` while idle.
    core_session: RefCell<Option<CoreSession>>,
    /// The session as a media player in the shell; `None` without one.
    media_player: RefCell<Option<crate::mpris::Player>>,
    /// The player couldn't be registered: no retry until the next session.
    media_failed: Cell<bool>,
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
        // Back from a chooser: the bells' sound or pattern may be renamed
        // or gone. The rows sit on the Bells page (#20), which a pop
        // shows again without mapping this view.
        let view = self.obj().downgrade();
        self.bells_page.connect_shown(move |_| {
            if let Some(view) = view.upgrade() {
                view.imp().refresh_bell_row_names();
            }
        });
        self.build_breathing_setup();
        self.configure_preparation_time_secs_row();
        self.setup_boxbreath_phase_cues();
        self.setup_volume_rows();
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
        self.mode_toggle_group.connect_active_name_notify(clone!(
            #[weak(rename_to = this)] obj,
            move |_| {
                this.imp().on_mode_switched();
            }
        ));

        // Stopwatch-Mode SwitchRow: persist state, mirror on the cell,
        // refresh the hero label + preset sensitivity. The
        // stopwatch_loading guard suppresses persistence while
        // refresh_streak is restoring the value on visit.
        self.stopwatch_mode_row.connect_active_notify(clone!(
            #[weak(rename_to = this)] obj,
            move |row| {
                let imp = this.imp();
                if imp.stopwatch_loading.get() { return; }
                let on = row.is_active();
                imp.stopwatch_toggle_on.set(on);
                if let Some(app) = imp.get_app() {
                    let key = meditate_core::settings_keys::stopwatch_key_for_mode(imp.current_mode().into());
                    setting_saved(&this, "timer.setting", app.with_db(|db| db.set_setting(
                            key,
                            meditate_core::format_bool(on),
                        )));
                }
                imp.refresh_stopwatch_dependent_ui();
            }
        ));

        // Keep-Screen-Awake SwitchRow: persists per-mode (timer /
        // guided / boxbreath_keep_screen_awake) so each mode can have
        // its own preference. The bells_loading guard reuses the
        // existing on-visit suppression flag, since the row is loaded
        // alongside the bell rows on every page-visit + mode switch.
        self.keep_screen_awake_row.connect_active_notify(clone!(
            #[weak(rename_to = this)] obj,
            move |row| {
                let imp = this.imp();
                if imp.bells_loading.get() { return; }
                let on = row.is_active();
                if let Some(app) = imp.get_app() {
                    let key = meditate_core::settings_keys::keep_screen_awake_key_for_mode(imp.current_mode().into());
                    setting_saved(&this, "timer.setting", app.with_db(|db| db.set_setting(
                            key,
                            meditate_core::format_bool(on),
                        )));
                }
            }
        ));
        self.auto_finish_row.connect_active_notify(clone!(
            #[weak(rename_to = this)] obj,
            move |row| {
                let imp = this.imp();
                if imp.bells_loading.get() { return; }
                let on = row.is_active();
                if let Some(app) = imp.get_app() {
                    let key = meditate_core::settings_keys::auto_finish_key_for_mode(imp.current_mode().into());
                    setting_saved(&this, "timer.setting", app.with_db(|db| db.set_setting(
                            key,
                            meditate_core::format_bool(on),
                        )));
                }
            }
        ));

        self.start_btn.connect_clicked(clone!(
            #[weak(rename_to = this)] obj,
            move |_| this.imp().on_start()
        ));

        // ── Session and Bells pages (#20) ───────────────────────────
        // The time opens the Duration dialog wherever the Duration row
        // would; elsewhere (Guided, Stopwatch) it doesn't react at all.
        self.time_btn.connect_clicked(clone!(
            #[weak(rename_to = this)] obj,
            move |_| this.imp().on_time_tapped()
        ));
        let follow_duration_row = clone!(
            #[weak(rename_to = this)] obj,
            move |row: &adw::ActionRow| {
                let on = row.is_visible() && row.is_sensitive();
                this.imp().time_btn.set_can_target(on);
                this.imp().time_btn.set_focusable(on);
            }
        );
        follow_duration_row(&self.duration_row);
        for prop in ["visible", "sensitive"] {
            let follow = follow_duration_row.clone();
            self.duration_row.connect_notify_local(Some(prop), move |row, _| follow(row));
        }
        for (row, page) in [
            (self.session_summary_row.get(), self.session_page.get()),
            (self.bells_summary_row.get(), self.bells_page.get()),
        ] {
            row.connect_activated(clone!(
                #[weak(rename_to = this)] obj,
                #[weak] page,
                move |_| {
                    if let Some(window) = this.root().and_downcast::<crate::window::MeditateWindow>() {
                        window.push_page(&page);
                    }
                }
            ));
            page.connect_hidden(clone!(
                #[weak(rename_to = this)] obj,
                move |_| {
                    // The Bells page holds Volume rows; a preview stops with it.
                    crate::sound::stop_preview();
                    this.imp().refresh_setup_summaries();
                }
            ));
        }
        self.session_toolbar.add_bottom_bar(&self.build_start_bar());
        self.bells_toolbar.add_bottom_bar(&self.build_start_bar());

        // ── Duration row: tap opens the H:M dialog ──────────────────
        // The only entry point for setting an ad-hoc Timer duration
        // (one not in any saved preset). Greyed out when stopwatch
        // mode is on — the planned-duration concept doesn't apply.
        self.duration_row.connect_activated(clone!(
            #[weak(rename_to = this)] obj,
            move |_| this.imp().show_custom_time_dialog(),
        ));

        // ── Save Settings / Manage Presets buttons ──────────────────
        // Both push the same chooser NavigationPage — the variant
        // (Save vs Manage) determines whether the synthetic "Create
        // new preset…" row appears, whether row taps trigger an
        // override-confirmation dialog, and whether rename/delete
        // suffix buttons render.
        self.save_settings_btn.connect_clicked(clone!(
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
                // A failure has said so already.
                let Some(snapshot) = imp.snapshot_current_setup() else { return; };
                let snapshot = Box::new(snapshot);
                window.push_presets_chooser(&app, session_mode, crate::presets::ChooserMode::Save { snapshot });
            },
        ));
        self.manage_presets_btn.connect_clicked(clone!(
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
                window.push_presets_chooser(&app, session_mode, crate::presets::ChooserMode::Manage);
            },
        ));

        // ── Guided-mode buttons ─────────────────────────────────────
        // Open File: pop the gtk::FileDialog, on success populate the
        // Selected row + hero countdown, ungrey Import.
        self.open_file_btn.connect_clicked(clone!(
            #[weak(rename_to = this)] obj,
            move |_| {
                let this_for_pick = this.clone();
                crate::guided::pick_file_for_open(this.upcast_ref(), move |pick| {
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
        self.import_file_btn.connect_clicked(clone!(
            #[weak(rename_to = this)] obj,
            move |_| {
                let imp = this.imp();
                let Some(app) = imp.get_app() else { return; };
                let Some(pick) = imp.guided_pick.borrow().clone() else { return; };
                let this_for_done = this.clone();
                crate::guided::import_picked_file(
                    this.upcast_ref(),
                    &app,
                    pick,
                    move |row| {
                        // Promote the freshly-imported row into the
                        // Selected slot — it stays as the active pick
                        // (now with a uuid attached), so the user can
                        // hit Start without re-tapping anything.
                        let imp = this_for_done.imp();
                        *imp.guided_selected_uuid.borrow_mut() = Some(row.uuid.0.clone());
                        *imp.guided_pick.borrow_mut() = Some(crate::guided::GuidedFilePick {
                            display_name: row.name.clone(),
                            source_path: meditate_core::audio_files::guided_file_path(
                                &crate::sync_runner::local_guided_dir(),
                                row.uuid.as_str(),
                            ),
                            duration_secs: row.duration_secs,
                            local_file_id: None,
                            folder: None,
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
        self.manage_guided_files_btn.connect_clicked(clone!(
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
                    move || {
                        let imp = this_for_changed.imp();
                        imp.rebuild_starred_guided_list();
                        imp.follow_selected_guided_file();
                    },
                );
            },
        ));

        // Local Folder: browse the user's own folder; a tapped file
        // becomes the current file, played in place.
        self.local_folder_btn.connect_clicked(clone!(
            #[weak(rename_to = this)] obj,
            move |_| {
                let imp = this.imp();
                let Some(app) = imp.get_app() else { return; };
                let Some(window) = this.root()
                    .and_downcast::<crate::window::MeditateWindow>()
                else { return; };
                let this_for_pick = this.clone();
                window.push_local_folder_page(&app, std::rc::Rc::new(move |pick| {
                    this_for_pick.imp().select_local_pick(pick);
                }));
            },
        ));
        // A scan or a length read: refresh Starred Files, and finish a
        // pick that was waiting for its length.
        let weak = obj.downgrade();
        crate::local_folder::on_change(move || {
            let Some(this) = weak.upgrade() else { return false };
            this.imp().on_local_folder_change();
            true
        });

        // A label renamed or deleted in the chooser (or by sync) shows
        // as it is now when the row is back on screen.
        self.setup_label_chooser_row.connect_map(clone!(
            #[weak(rename_to = this)] obj,
            move |_| this.imp().refresh_setup_label_chooser_subtitle()
        ));

        // End Bell master toggle — gates whether the bell plays at the
        // end of a session. Persists end_bell_active.
        self.end_bell_row.connect_enable_expansion_notify(clone!(
            #[weak(rename_to = this)] obj,
            move |row| {
                let imp = this.imp();
                if imp.bells_loading.get() { return; }
                let on = row.enables_expansion();
                if let Some(app) = imp.get_app() {
                    let mode: meditate_core::SessionMode =
                        imp.current_mode().into();
                    setting_saved(&this, "timer.setting", app.with_db(|db| db.set_setting(
                            meditate_core::settings_keys::end_bell_active_key_for_mode(mode),
                            meditate_core::format_bool(on),
                        )));
                    // Re-warm the preload so the next play_end_bell()
                    // either has a MediaFile ready (active=true) or
                    // doesn't waste cycles trying to reuse a stale one.
                    crate::sound::preload_end_bell(&app, mode);
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
        self.end_bell_sound_row.connect_activated(clone!(
            #[weak(rename_to = this)] obj,
            move |_| {
                let imp = this.imp();
                let Some(app) = imp.get_app() else { return; };
                let Some(window) = this.root()
                    .and_then(|r| r.downcast::<crate::window::MeditateWindow>().ok())
                else { return; };
                let mode: meditate_core::SessionMode = imp.current_mode().into();
                let current = app
                    .with_db(|db| db.get_setting(
                        meditate_core::settings_keys::end_bell_sound_key_for_mode(mode),
                        crate::db::BUNDLED_BOWL_UUID,
                    ))
                    .and_then(std::result::Result::ok);
                let app_for_pick = app.clone();
                let this_for_pick = this.clone();
                let volume = app
                    .with_db(|db| meditate_core::bell_volume::read(db, BellSlot::End(mode)))
                    .unwrap_or_default();
                window.push_sound_chooser(
                    &app,
                    crate::db::BellSoundCategory::General,
                    current,
                    volume,
                    move |uuid| {
                        setting_saved(&this_for_pick, "timer.setting", app_for_pick.with_db(|db| db.set_setting(
                            meditate_core::settings_keys::end_bell_sound_key_for_mode(mode),
                            &uuid,
                        )));
                        crate::sound::preload_end_bell(&app_for_pick, mode);
                        this_for_pick.imp().refresh_end_bell_sound_subtitle();
                    },
                );
            }
        ));

        // End Bell pattern row — tap pushes the vibration-pattern
        // chooser. Persists end_bell_pattern setting on pick.
        self.end_bell_pattern_row.connect_activated(clone!(
            #[weak(rename_to = this)] obj,
            move |_| {
                let imp = this.imp();
                let Some(app) = imp.get_app() else { return; };
                let Some(window) = this.root()
                    .and_then(|r| r.downcast::<crate::window::MeditateWindow>().ok())
                else { return; };
                let mode: meditate_core::SessionMode = imp.current_mode().into();
                let current = app
                    .with_db(|db| db.get_setting(
                        meditate_core::settings_keys::end_bell_pattern_key_for_mode(mode),
                        crate::db::BUNDLED_PATTERN_PULSE_UUID,
                    ))
                    .and_then(std::result::Result::ok);
                let app_for_pick = app.clone();
                let this_for_pick = this.clone();
                window.push_vibrations_chooser(&app, current, move |uuid| {
                    setting_saved(&this_for_pick, "timer.setting", app_for_pick.with_db(|db| db.set_setting(
                        meditate_core::settings_keys::end_bell_pattern_key_for_mode(mode),
                        &uuid,
                    )));
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
        self.setup_label_enabled_row.connect_enable_expansion_notify(clone!(
            #[weak(rename_to = this)] obj,
            move |row| {
                let imp = this.imp();
                if imp.labels_loading.get() { return; }
                let on = row.enables_expansion();
                let mode = imp.current_mode();
                // Core adopts the mode's default label the first time
                // the toggle flips on.
                if let Some(app) = imp.get_app() {
                    setting_saved(&this, "timer.setting", app.with_db(|db| meditate_core::labels::set_active_for_mode(db, mode.into(), on)));
                }
                imp.refresh_setup_label_chooser_subtitle();
            }
        ));
        self.setup_label_chooser_row.connect_activated(clone!(
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
                    imp2.persist_label_uuid_for_mode(mode, label.uuid.as_str());
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
        self.starting_bell_row.connect_enable_expansion_notify(clone!(
            #[weak(rename_to = this)] obj,
            move |row| {
                let imp = this.imp();
                if imp.bells_loading.get() { return; }
                let on = row.enables_expansion();
                if let Some(app) = imp.get_app() {
                    setting_saved(&this, "timer.setting", app.with_db(|db| db.set_setting(
                            "starting_bell_active",
                            meditate_core::format_bool(on),
                        )));
                }
            }
        ));

        // Starting-Bell sound row — tap pushes the bell-sound chooser.
        // "No bell" is still handled by the parent ExpanderRow's
        // master toggle; the chooser only lists real sounds.
        self.starting_bell_sound_row.connect_activated(clone!(
            #[weak(rename_to = this)] obj,
            move |_| {
                let imp = this.imp();
                let Some(app) = imp.get_app() else { return; };
                let Some(window) = this.root()
                    .and_then(|r| r.downcast::<crate::window::MeditateWindow>().ok())
                else { return; };
                let current = app
                    .with_db(|db| db.get_setting("starting_bell_sound", crate::db::BUNDLED_BOWL_UUID))
                    .and_then(std::result::Result::ok);
                let app_for_pick = app.clone();
                let this_for_pick = this.clone();
                let volume = app
                    .with_db(|db| meditate_core::bell_volume::read(db, BellSlot::Starting))
                    .unwrap_or_default();
                window.push_sound_chooser(
                    &app,
                    crate::db::BellSoundCategory::General,
                    current,
                    volume,
                    move |uuid| {
                        setting_saved(&this_for_pick, "timer.setting", app_for_pick.with_db(|db| db.set_setting("starting_bell_sound", &uuid)));
                        this_for_pick.imp().refresh_starting_bell_sound_subtitle();
                    },
                );
            }
        ));

        // Starting Bell pattern row — drills into the vibrations chooser.
        self.starting_bell_pattern_row.connect_activated(clone!(
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
                    .and_then(std::result::Result::ok);
                let app_for_pick = app.clone();
                let this_for_pick = this.clone();
                window.push_vibrations_chooser(&app, current, move |uuid| {
                    setting_saved(&this_for_pick, "timer.setting", app_for_pick.with_db(|db| db.set_setting("starting_bell_pattern", &uuid)));
                    this_for_pick.imp().refresh_starting_bell_pattern_subtitle();
                });
            }
        ));

        // Starting Bell signal-mode AdwToggleGroup.
        self.setup_starting_bell_signal_mode_toggle();

        // Preparation Time expander — nested inside the Starting Bell
        // expander, animates the seconds spin in and out the same way.
        self.preparation_time_row.connect_enable_expansion_notify(clone!(
            #[weak(rename_to = this)] obj,
            move |row| {
                let imp = this.imp();
                if imp.bells_loading.get() { return; }
                let on = row.enables_expansion();
                if let Some(app) = imp.get_app() {
                    setting_saved(&this, "timer.setting", app.with_db(|db| db.set_setting(
                            "preparation_time_active",
                            meditate_core::format_bool(on),
                        )));
                }
            }
        ));

        // Interval Bells master toggle — same persistence + bells_loading
        // guard pattern as Starting Bell. The ExpanderRow's switch gates
        // whether the running tick fires interval bells at all (B.3.4
        // checks `interval_bells_active` before iterating the library).
        self.interval_bells_enabled_row.connect_enable_expansion_notify(clone!(
            #[weak(rename_to = this)] obj,
            move |row| {
                let imp = this.imp();
                if imp.bells_loading.get() { return; }
                let on = row.enables_expansion();
                if let Some(app) = imp.get_app() {
                    setting_saved(&this, "timer.setting", app.with_db(|db| db.set_setting(
                            "interval_bells_active",
                            meditate_core::format_bool(on),
                        )));
                }
            }
        ));

        // "Manage Bells" row — tap pushes the bell-library NavigationPage.
        self.interval_bells_row.connect_activated(clone!(
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
            clone!(
                #[weak(rename_to = this)] obj,
                move |row, _| {
                    let imp = this.imp();
                    if imp.bells_loading.get() { return; }
                    let v = row.value().round() as i64;
                    let v = v.clamp(
                        i64::from(meditate_core::format::PREP_SECS_MIN),
                        i64::from(meditate_core::format::PREP_SECS_MAX),
                    );
                    if let Some(app) = imp.get_app() {
                        setting_saved(&this, "timer.setting", app.with_db(|db| db.set_setting("preparation_time_secs", &v.to_string())));
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
            f64::from(meditate_core::format::PREP_SECS_DEFAULT),
            f64::from(meditate_core::format::PREP_SECS_MIN),
            f64::from(meditate_core::format::PREP_SECS_MAX),
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
            || "starting_bell_signal_mode",
            clone!(
                #[weak] obj,
                #[upgrade_or] None,
                // Loading the stored value writes nothing back.
                move || (!obj.imp().bells_loading.get()).then(|| obj.imp().get_app()).flatten()
            ),
        );
    }

    /// Apply the saved starting_bell_signal_mode + capability gating.
    pub(crate) fn refresh_starting_bell_signal_mode_state(&self) {
        let Some(app) = self.get_app() else { return; };
        // Callers may already be loading; put their flag back after.
        let was = self.bells_loading.replace(true);
        apply_signal_mode_state(
            &self.starting_bell_signal_toggle_host,
            &self.starting_bell_sound_revealer,
            &self.starting_bell_pattern_revealer,
            &app,
            "starting_bell_signal_mode",
        );
        self.bells_loading.set(was);
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
            clone!(
                #[weak] obj,
                #[upgrade_or] "timer_end_bell_signal_mode",
                move || meditate_core::settings_keys::end_bell_signal_mode_key_for_mode(
                    obj.imp().current_mode().into(),
                )
            ),
            clone!(
                #[weak] obj,
                #[upgrade_or] None,
                // Loading the stored value writes nothing back.
                move || (!obj.imp().bells_loading.get()).then(|| obj.imp().get_app()).flatten()
            ),
        );
    }

    /// Apply the saved end_bell_signal_mode + capability gating to
    /// the toggle group. Called from refresh-on-visit once the
    /// widget is attached and `get_app()` resolves.
    pub(crate) fn refresh_end_bell_signal_mode_state(&self) {
        let Some(app) = self.get_app() else { return; };
        // Callers may already be loading; put their flag back after.
        let was = self.bells_loading.replace(true);
        apply_signal_mode_state(
            &self.end_bell_signal_toggle_host,
            &self.end_bell_sound_revealer,
            &self.end_bell_pattern_revealer,
            &app,
            meditate_core::settings_keys::end_bell_signal_mode_key_for_mode(self.current_mode().into()),
        );
        self.bells_loading.set(was);
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
            clone!(
                #[weak] obj,
                #[upgrade_or] None,
                // Loading the stored value writes nothing back.
                move || (!obj.imp().bells_loading.get()).then(|| obj.imp().get_app()).flatten()
            ),
            clone!(
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
        let saved = app
            .with_db(|db| {
                meditate_core::bells::signal_mode_override_from_db(
                    db,
                    self.current_mode().into(),
                )
            })
            .unwrap_or(crate::db::SignalMode::Both);
        // Set populating flag so the active-name notify handler
        // doesn't write the just-loaded value back to the DB.
        self.bells_loading.set(true);
        toggle_group.set_active_name(Some(saved.as_db_str()));
        self.bells_loading.set(false);
        show_no_vibration_note(&toggle_group);
    }

    /// Sync the Keep-Screen-Awake switch row with the current mode's
    /// stored value. Called on visit and on every mode switch so the
    /// switch reflects the key the runtime will read on session start.
    pub(crate) fn refresh_keep_screen_awake_state(&self) {
        let Some(app) = self.get_app() else { return; };
        let mode = self.current_mode().into();
        let on = app
            .with_db(|db| meditate_core::settings_keys::keep_screen_awake_from_db(db, mode))
            .unwrap_or(false);
        self.bells_loading.set(true);
        self.keep_screen_awake_row.set_active(on);
        self.bells_loading.set(false);
    }

    /// Sync the Auto-Finish switch with the current mode's stored
    /// value; hidden while counting up, except for Guided, whose file
    /// always ends.
    pub(crate) fn refresh_auto_finish_state(&self) {
        let Some(app) = self.get_app() else { return; };
        let mode = self.current_mode();
        let on = app
            .with_db(|db| meditate_core::settings_keys::auto_finish_from_db(db, mode.into()))
            .unwrap_or(false);
        self.bells_loading.set(true);
        self.auto_finish_row.set_active(on);
        self.bells_loading.set(false);
        self.auto_finish_row.set_sensitive(mode == TimerMode::Guided || !self.stopwatch_toggle_on.get());
    }

    /// Hold a suspend inhibit while the session runs (not while
    /// paused), plus an idle inhibit when the mode's keep-screen-awake
    /// setting is on. Says so when the system refuses it.
    /// Cookie is stashed on `screen_awake_cookie`;
    /// release_screen_awake_lock uninhibits and clears it. No-op if
    /// the cookie is already held (idempotent).
    pub(crate) fn acquire_screen_awake_lock(
        &self,
        app: &crate::application::MeditateApplication,
    ) {
        if self.screen_awake_cookie.get() != 0 { return; }
        let mode = self.current_mode().into();
        let keep_screen_on = app
            .with_db(|db| meditate_core::settings_keys::keep_screen_awake_from_db(db, mode))
            .unwrap_or(false);
        // Every session blocks suspend so bells and the end stay on
        // time; the screen stays on only with "Keep screen awake".
        let mut flags = gtk::ApplicationInhibitFlags::SUSPEND;
        if keep_screen_on {
            flags |= gtk::ApplicationInhibitFlags::IDLE;
        }
        let window = app.active_window();
        let cookie = app.inhibit(
            window.as_ref(),
            flags,
            Some(&gettext("Meditation session running")),
        );
        if cookie == 0 {
            crate::db::show_in_view(gettext("Couldn't keep the computer awake for the bells"));
        }
        self.screen_awake_cookie.set(cookie);
    }

    /// Release the inhibit cookie acquired at session start, if
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
    /// Build every Setup-view bell's Volume row: save the level when
    /// its slider rests, then play the bell at it.
    fn setup_volume_rows(&self) {
        let rows = [
            (SetupVolume::Starting, &*self.starting_bell_volume_row),
            (SetupVolume::End, &*self.end_bell_volume_row),
            (SetupVolume::Cue(BoxBreathPhaseId::In), &*self.boxbreath_phase_in_volume_row),
            (SetupVolume::Cue(BoxBreathPhaseId::HoldIn), &*self.boxbreath_phase_holdin_volume_row),
            (SetupVolume::Cue(BoxBreathPhaseId::Out), &*self.boxbreath_phase_out_volume_row),
            (SetupVolume::Cue(BoxBreathPhaseId::HoldOut), &*self.boxbreath_phase_holdout_volume_row),
        ];
        let obj = self.obj();
        let mut installed = Vec::new();
        for (key, row) in rows {
            let view = obj.downgrade();
            let target = {
                let view = view.clone();
                move || {
                    let mode: meditate_core::SessionMode = view.upgrade()?.imp().current_mode().into();
                    Some((key.slot(mode), mode))
                }
            };
            let save = move |&(slot, mode): &(meditate_core::bell_volume::BellSlot, meditate_core::SessionMode), volume| {
                let app = view.upgrade()?.imp().get_app()?;
                let this = view.upgrade()?;
                // No preview of a level that springs back.
                if !setting_saved(&this, "bell_volume.write", app.with_db(|db| meditate_core::bell_volume::write(db, slot, volume))) {
                    return None;
                }
                let sound = app.with_db(|db| meditate_core::bell_volume::sound_uuid(db, slot));
                if key == SetupVolume::End {
                    // The end bell is preloaded; reload it at the new level.
                    crate::sound::preload_end_bell(&app, mode);
                }
                sound
            };
            installed.push((key, crate::volume_row::VolumeRow::install(row, target, save)));
        }
        *self.volume_rows.borrow_mut() = installed;
        self.refresh_volume_rows();
    }

    /// Save every level whose slider hasn't rested yet.
    fn flush_volume_rows(&self) {
        for (_, row) in self.volume_rows.borrow().iter() {
            row.flush();
        }
    }

    /// Show every Setup-view bell's stored volume (the end bell's for
    /// the current mode).
    pub(crate) fn refresh_volume_rows(&self) {
        let Some(app) = self.get_app() else { return; };
        // A level still settling belongs to the bell and mode it was set in.
        self.flush_volume_rows();
        let mode: meditate_core::SessionMode = self.current_mode().into();
        for (key, row) in self.volume_rows.borrow().iter() {
            let slot = key.slot(mode);
            if let Some(volume) = app.with_db(|db| meditate_core::bell_volume::read(db, slot)) {
                row.set_volume(volume);
            }
        }
    }

    fn setup_boxbreath_phase_cues(&self) {
        let obj = self.obj();

        // Master "Cues during phases" enable-switch. Persists to
        // boxbreath_cues_active. We use enable_expansion notify
        // (not the row's expansion state itself) so the user's
        // toggling reads as on/off, not collapse/expand.
        self.boxbreath_master_row.connect_enable_expansion_notify(clone!(
            #[weak(rename_to = this)] obj,
            move |row| {
                let imp = this.imp();
                if imp.bells_loading.get() { return; }
                let on = row.enables_expansion();
                if let Some(app) = imp.get_app() {
                    setting_saved(&this, "timer.setting", app.with_db(|db| db.set_setting(
                        "boxbreath_cues_active",
                        meditate_core::format_bool(on),
                    )));
                }
            }
        ));

        // Per-phase wiring. Each phase has identical structure but
        // distinct template-children — call a small helper four times.
        self.wire_boxbreath_phase(
            BoxBreathPhaseId::In,
            &self.boxbreath_phase_in_row,
            &self.boxbreath_phase_in_signal_toggle_host,
            &self.boxbreath_phase_in_sound_revealer,
            &self.boxbreath_phase_in_sound_row,
            &self.boxbreath_phase_in_pattern_revealer,
            &self.boxbreath_phase_in_pattern_row,
        );
        self.wire_boxbreath_phase(
            BoxBreathPhaseId::HoldIn,
            &self.boxbreath_phase_holdin_row,
            &self.boxbreath_phase_holdin_signal_toggle_host,
            &self.boxbreath_phase_holdin_sound_revealer,
            &self.boxbreath_phase_holdin_sound_row,
            &self.boxbreath_phase_holdin_pattern_revealer,
            &self.boxbreath_phase_holdin_pattern_row,
        );
        self.wire_boxbreath_phase(
            BoxBreathPhaseId::Out,
            &self.boxbreath_phase_out_row,
            &self.boxbreath_phase_out_signal_toggle_host,
            &self.boxbreath_phase_out_sound_revealer,
            &self.boxbreath_phase_out_sound_row,
            &self.boxbreath_phase_out_pattern_revealer,
            &self.boxbreath_phase_out_pattern_row,
        );
        self.wire_boxbreath_phase(
            BoxBreathPhaseId::HoldOut,
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
    ///     column via edit_box_breath_phase.
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
        phase_row.connect_enable_expansion_notify(clone!(
            #[weak(rename_to = this)] obj,
            move |row| {
                let imp = this.imp();
                if imp.bells_loading.get() { return; }
                let on = row.enables_expansion();
                if let Some(app) = imp.get_app() {
                    setting_saved(&this, "timer.setting", app.with_db(|db| db.edit_box_breath_phase(phase, |p| p.enabled = on)));
                }
            }
        ));

        // Re-emit `activated` on the wrapped rows.
        attach_revealer_row_click(sound_row);
        attach_revealer_row_click(pattern_row);

        // Bell Sound row -> push sound chooser (BoxBreath category).
        let phase_for_sound = phase;
        sound_row.connect_activated(clone!(
            #[weak(rename_to = this)] obj,
            move |_| {
                let imp = this.imp();
                let Some(app) = imp.get_app() else { return; };
                let Some(window) = this.root()
                    .and_then(|r| r.downcast::<crate::window::MeditateWindow>().ok())
                else { return; };
                let p = match app
                    .with_db(|db| db.get_box_breath_phase(phase_for_sound))
                    .and_then(std::result::Result::ok)
                    .flatten()
                {
                    Some(p) => p,
                    None => return,
                };
                let app_for_pick = app.clone();
                let this_for_pick = this.clone();
                window.push_sound_chooser(
                    &app,
                    crate::db::BellSoundCategory::BoxBreath,
                    Some(p.sound_uuid.0.clone()),
                    p.volume,
                    move |uuid| {
                        // The row as stored now: a volume dragged since stays (#325).
                        setting_saved(&this_for_pick, "timer.setting", app_for_pick.with_db(|db| {
                            db.edit_box_breath_phase(phase_for_sound, |p| p.sound_uuid = uuid.as_str().into())
                        }));
                        this_for_pick.imp().refresh_boxbreath_phase_subtitles(phase_for_sound);
                    },
                );
            }
        ));

        // Pattern row -> push vibration-pattern chooser.
        let phase_for_pattern = phase;
        pattern_row.connect_activated(clone!(
            #[weak(rename_to = this)] obj,
            move |_| {
                let imp = this.imp();
                let Some(app) = imp.get_app() else { return; };
                let Some(window) = this.root()
                    .and_then(|r| r.downcast::<crate::window::MeditateWindow>().ok())
                else { return; };
                let p = match app
                    .with_db(|db| db.get_box_breath_phase(phase_for_pattern))
                    .and_then(std::result::Result::ok)
                    .flatten()
                {
                    Some(p) => p,
                    None => return,
                };
                let app_for_pick = app.clone();
                let this_for_pick = this.clone();
                window.push_vibrations_chooser(
                    &app,
                    Some(p.pattern_uuid.0.clone()),
                    move |uuid| {
                        setting_saved(&this_for_pick, "timer.setting", app_for_pick.with_db(|db| {
                            db.edit_box_breath_phase(phase_for_pattern, |p| p.pattern_uuid = uuid.as_str().into())
                        }));
                        this_for_pick.imp().refresh_boxbreath_phase_subtitles(phase_for_pattern);
                    },
                );
            }
        ));

        // Sound/Vibration/Both AdwToggleGroup. Built imperatively
        // (no app at construction time); persistence resolves app
        // lazily and writes through edit_box_breath_phase.
        build_phase_signal_mode_toggle_widget(
            toggle_host,
            sound_revealer,
            pattern_revealer,
            phase,
            clone!(
                #[weak] obj,
                #[upgrade_or] None,
                // Loading the stored value writes nothing back.
                move || (!obj.imp().bells_loading.get()).then(|| obj.imp().get_app()).flatten()
            ),
        );
    }

    /// Apply the saved state + capability gating to all four phase
    /// rows + the master toggle. Called from refresh-on-visit once
    /// the widget is attached.
    pub(crate) fn refresh_boxbreath_phase_state(&self) {
        self.refresh_volume_rows();
        let Some(app) = self.get_app() else { return; };

        self.bells_loading.set(true);

        // Master row.
        let master_on = app
            .with_db(|db| meditate_core::read_bool(db, "boxbreath_cues_active", false))
            .unwrap_or(false);
        self.boxbreath_master_row.set_enable_expansion(master_on);
        self.boxbreath_master_row.set_expanded(master_on);

        // Per-phase: enable + toggle + revealers + subtitles.
        self.refresh_boxbreath_phase_row(
            BoxBreathPhaseId::In,
            &self.boxbreath_phase_in_row,
            &self.boxbreath_phase_in_signal_toggle_host,
            &self.boxbreath_phase_in_sound_revealer,
            &self.boxbreath_phase_in_pattern_revealer,
            &app,
        );
        self.refresh_boxbreath_phase_row(
            BoxBreathPhaseId::HoldIn,
            &self.boxbreath_phase_holdin_row,
            &self.boxbreath_phase_holdin_signal_toggle_host,
            &self.boxbreath_phase_holdin_sound_revealer,
            &self.boxbreath_phase_holdin_pattern_revealer,
            &app,
        );
        self.refresh_boxbreath_phase_row(
            BoxBreathPhaseId::Out,
            &self.boxbreath_phase_out_row,
            &self.boxbreath_phase_out_signal_toggle_host,
            &self.boxbreath_phase_out_sound_revealer,
            &self.boxbreath_phase_out_pattern_revealer,
            &app,
        );
        self.refresh_boxbreath_phase_row(
            BoxBreathPhaseId::HoldOut,
            &self.boxbreath_phase_holdout_row,
            &self.boxbreath_phase_holdout_signal_toggle_host,
            &self.boxbreath_phase_holdout_sound_revealer,
            &self.boxbreath_phase_holdout_pattern_revealer,
            &app,
        );
        for phase in BoxBreathPhaseId::all() {
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
            .and_then(std::result::Result::ok)
            .flatten()
        {
            Some(p) => p,
            None => return,
        };
        phase_row.set_enable_expansion(p.enabled);
        phase_row.set_expanded(p.enabled);
        apply_phase_signal_mode_state(
            toggle_host, sound_revealer, pattern_revealer, p.signal_mode,
        );
    }

    pub(crate) fn refresh_boxbreath_phase_subtitles(
        &self,
        phase: crate::db::BoxBreathPhaseId,
    ) {
        let Some(app) = self.get_app() else { return; };
        let Some((sound_name, pattern_name)) = app
            .with_db(|db| meditate_core::bells::phase_cue_names(db, phase))
            .flatten()
        else { return; };
        let sound_subtitle = crate::bells::render_resolved_name(sound_name);
        let pattern_subtitle = crate::bells::render_resolved_name(pattern_name);
        let (sound_row, pattern_row): (&adw::ActionRow, &adw::ActionRow) = match phase {
            BoxBreathPhaseId::In      => (&self.boxbreath_phase_in_sound_row,      &self.boxbreath_phase_in_pattern_row),
            BoxBreathPhaseId::HoldIn  => (&self.boxbreath_phase_holdin_sound_row,  &self.boxbreath_phase_holdin_pattern_row),
            BoxBreathPhaseId::Out     => (&self.boxbreath_phase_out_sound_row,     &self.boxbreath_phase_out_pattern_row),
            BoxBreathPhaseId::HoldOut => (&self.boxbreath_phase_holdout_sound_row, &self.boxbreath_phase_holdout_pattern_row),
        };
        sound_row.set_subtitle(&sound_subtitle);
        pattern_row.set_subtitle(&pattern_subtitle);
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
    key_for_write: impl Fn() -> &'static str + 'static,
    get_app: impl Fn() -> Option<crate::application::MeditateApplication> + 'static,
) {
    let toggle_group = adw::ToggleGroup::builder()
        .css_classes(["round"])
        .valign(gtk::Align::Center)
        .build();

    let sound_toggle = adw::Toggle::builder()
        .name("sound")
        .label(gettext("Sound"))
        .build();
    let vibration_toggle = adw::Toggle::builder()
        .name("vibration")
        .label(gettext("Vibration"))
        .build();
    let both_toggle = adw::Toggle::builder()
        .name("both")
        .label(gettext("Both"))
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
        show_no_vibration_note(tg);
        if let Some(app) = get_app() {
            setting_saved(tg, "timer.setting", app.with_db(|db| db.set_setting(key_for_write(), mode.as_db_str())));
        }
        sound_revealer.set_reveal_child(mode.includes_sound());
        pattern_revealer.set_reveal_child(mode.includes_vibration());
    });
}

/// Apply the saved signal_mode setting to a previously-built toggle
/// group. A device without vibration keeps every choice selectable
/// and says under the row what still plays.
pub(crate) fn apply_signal_mode_state(
    host: &gtk::Box,
    sound_revealer: &gtk::Revealer,
    pattern_revealer: &gtk::Revealer,
    app: &crate::application::MeditateApplication,
    setting_key: &'static str,
) {
    let Some(toggle_group) = first_toggle_group_in(host) else { return; };
    let saved = app
        .with_db(|db| db.get_setting(setting_key, "sound"))
        .and_then(std::result::Result::ok)
        .and_then(|s| crate::db::SignalMode::from_db_str(&s))
        .unwrap_or(crate::db::SignalMode::Sound);
    toggle_group.set_active_name(Some(saved.as_db_str()));
    sound_revealer.set_reveal_child(saved.includes_sound());
    pattern_revealer.set_reveal_child(saved.includes_vibration());
    show_no_vibration_note(&toggle_group);
}

/// Phase-config variant of `build_signal_mode_toggle_widget`. The
/// notify handler resolves app lazily (via `get_app`) and persists
/// the new mode through `edit_box_breath_phase` instead of writing
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
        .name("sound").label(gettext("Sound")).build());
    toggle_group.add(adw::Toggle::builder()
        .name("vibration").label(gettext("Vibration")).build());
    toggle_group.add(adw::Toggle::builder()
        .name("both").label(gettext("Both")).build());
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
        show_no_vibration_note(tg);
        if let Some(app) = get_app() {
            setting_saved(tg, "timer.setting", app.with_db(|db| db.edit_box_breath_phase(phase, |p| p.signal_mode = mode)));
        }
        sound_revealer.set_reveal_child(mode.includes_sound());
        pattern_revealer.set_reveal_child(mode.includes_vibration());
    });
}

/// The name of the row that has the keyboard focus, so a rebuild can
/// give it back (a list rebuilt after every change would otherwise drop
/// the focus on the page's first row).
fn focused_row_name<'a>(mut rows: impl Iterator<Item = &'a adw::ActionRow>) -> Option<String> {
    rows.find(|row| row.is_focus() || row.focus_child().is_some()).map(|row| row.widget_name().to_string())
}

fn refocus_row<'a>(name: Option<&str>, mut rows: impl Iterator<Item = &'a adw::ActionRow>) {
    if let Some(row) = name.and_then(|n| rows.find(|row| row.widget_name() == n)) { row.grab_focus(); }
}

/// #191: a Timer page setting that failed to save says so, and the
/// page shows the saved value again on the next turn (a spin row or
/// toggle is still mid-change inside its handler).
pub(crate) fn setting_saved<T, E: std::fmt::Debug + 'static>(
    anchor: &impl IsA<gtk::Widget>,
    tag: &str,
    result: Option<std::result::Result<T, E>>,
) -> bool {
    if crate::db::write_ok(anchor, tag, result, crate::db::Failed::Save) {
        return true;
    }
    // The Session and Bells pages sit outside the view's widget tree (#20).
    let view = anchor.ancestor(super::TimerView::static_type()).and_downcast::<super::TimerView>()
        .or_else(|| anchor.root().and_downcast::<crate::window::MeditateWindow>().map(|w| w.timer_view()));
    if let Some(view) = view {
        glib::idle_add_local_once(move || view.imp().reload_setup());
    }
    false
}

/// Apply the saved phase-row signal_mode. Called from refresh-on-visit.
pub(crate) fn apply_phase_signal_mode_state(
    host: &gtk::Box,
    sound_revealer: &gtk::Revealer,
    pattern_revealer: &gtk::Revealer,
    saved: crate::db::SignalMode,
) {
    let Some(toggle_group) = first_toggle_group_in(host) else { return; };
    toggle_group.set_active_name(Some(saved.as_db_str()));
    sound_revealer.set_reveal_child(saved.includes_sound());
    pattern_revealer.set_reveal_child(saved.includes_vibration());
    show_no_vibration_note(&toggle_group);
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
        .name("sound").label(gettext("Sound")).build());
    toggle_group.add(adw::Toggle::builder()
        .name("vibration").label(gettext("Vibration")).build());
    toggle_group.add(adw::Toggle::builder()
        .name("both").label(gettext("Both")).build());
    toggle_group.set_active_name(Some("both"));

    host.append(&toggle_group);

    toggle_group.connect_active_name_notify(move |tg| {
        let Some(name) = tg.active_name() else { return; };
        let mode = crate::db::SignalMode::from_db_str(name.as_str())
            .unwrap_or(crate::db::SignalMode::Both);
        show_no_vibration_note(tg);
        let Some(app) = get_app() else { return; };
        let setting_key = meditate_core::settings_keys::signal_mode_key_for_mode(get_mode().into());
        setting_saved(tg, "timer.setting", app.with_db(|db| db.set_setting(setting_key, mode.as_db_str())));
    });
}

/// On a device that can't vibrate, say under the choice's row what a
/// Vibration or Both choice still plays.
pub(crate) fn show_no_vibration_note(toggle_group: &adw::ToggleGroup) {
    use meditate_core::bells::{no_vibration_note, NoVibrationNote};
    let Some(row) = toggle_group.ancestor(adw::ActionRow::static_type()).and_downcast::<adw::ActionRow>() else {
        return;
    };
    let haptic = crate::application::MeditateApplication::get().map_or(true, |app| app.has_haptic());
    let mode = toggle_group
        .active_name()
        .and_then(|n| crate::db::SignalMode::from_db_str(&n))
        .unwrap_or(crate::db::SignalMode::Sound);
    let note = no_vibration_note(mode, haptic);
    row.set_subtitle(&match note {
        None => String::new(),
        Some(NoVibrationNote::Silent) => gettext("This device can't vibrate, so nothing plays"),
        Some(NoVibrationNote::SoundOnly) => gettext("This device can't vibrate, so only the sound plays"),
    });
    // The note would take width from the choices until they read "S…";
    // at 360 px the row still fits with every choice whole.
    let whole = note.map_or(-1, |_| toggle_group.measure(gtk::Orientation::Horizontal, -1).1);
    toggle_group.set_size_request(whole, -1);
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
            .cycle_aligned_target_secs(u64::from(self.breathing_session_secs.get()))
    }


    /// Which mode the radio group currently reflects. Exactly one of
    /// the three toggles is active at any time (they share a group).
    /// Stopwatch-vs-countdown lives on `stopwatch_toggle_on` within
    /// the Timer branch.
    pub(crate) fn current_mode(&self) -> TimerMode {
        let name = self.mode_toggle_group.active_name();
        MODE_NAMES.iter().find(|(n, _)| Some(*n) == name.as_deref()).map_or(TimerMode::Timer, |(_, m)| *m)
    }

    /// Called when any of the three mode toggles gains active state.
    fn on_mode_switched(&self) {
        let mode = self.current_mode();
        // An open Undo (a preset's, above all) belongs to the mode left.
        if let Some(window) = self.obj().root().and_downcast::<crate::window::MeditateWindow>() {
            window.dismiss_toasts();
        }
        // The device opens in this mode next time (#22). After the
        // dismiss, so a failed save stays on screen.
        // A failed write says so, but has no Setup value to reload.
        if let Some(app) = self.get_app().filter(|_| self.choice_restored.get()) {
            if let Some(res) = app.with_db(|db| meditate_core::settings_keys::set_last_mode(db, mode.into())) {
                crate::db::write_ok(&*self.obj(), "mode.remember", Some(res), crate::db::Failed::Save);
            }
        }

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
        self.refresh_auto_finish_state();
        // Visible-list contents are mode-strict (Timer presets only
        // appear in Timer mode, Box-Breath presets in Box Breath mode)
        // — rebuild on every switch. Guided mode rebuilds its own
        // starred-files list instead.
        if mode == TimerMode::Guided {
            self.rebuild_starred_guided_list();
            self.refresh_guided_selected_row();
            if let Some(app) = self.get_app() {
                crate::local_folder::scan(&app);
            }
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
            let key = meditate_core::settings_keys::stopwatch_key_for_mode(mode.into());
            let on = app
                .with_db(|db| meditate_core::read_bool(db, key, false))
                .unwrap_or(false);
            self.stopwatch_loading.set(true);
            self.stopwatch_mode_row.set_active(on);
            self.stopwatch_toggle_on.set(on);
            self.stopwatch_loading.set(false);
        }
        self.refresh_stopwatch_dependent_ui();
        // End Bell config is per-mode (2026-07-17) — reload its
        // sound / pattern subtitles + Type toggle for the new
        // mode. The row's active state already reloads inside
        // refresh_stopwatch_dependent_ui via end_bell_row_state.
        self.refresh_end_bell_sound_subtitle();
        self.refresh_end_bell_pattern_subtitle();
        self.refresh_end_bell_signal_mode_state();
        self.refresh_volume_rows();

        match self.ui_state() {
            UiState::Idle      => self.show_idle_ui(),
            UiState::Paused    => self.show_paused_ui(self.current_display_secs()),
            // Running, Preparing, Overtime and an ended session normally
            // can't reach here (the running page covers the toggle);
            // fall back to idle UI as a safety net.
            UiState::Running | UiState::Preparing | UiState::Overtime | UiState::Done => {
                self.show_idle_ui()
            }
        }
    }

    fn show_idle_ui(&self) {
        self.start_btn.set_visible(true);
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
        self.refresh_start_sensitive(mode);
        self.countdown_inputs.set_visible(v.countdown);
        self.boxbreath_inputs.set_visible(v.boxbreath);
        self.guided_section.set_visible(v.guided);
        self.boxbreath_phase_section.set_visible(v.boxbreath_phase);
        self.starting_bell_row.set_visible(v.starting_bell);
        self.interval_bells_enabled_row.set_visible(v.interval_bells);
        self.duration_row.set_visible(v.duration);
        self.presets_section.set_visible(v.presets);
        self.refresh_setup_summaries();
    }

    /// Pull the right Cell into the shared Duration row's value label.
    /// Both modes store seconds; divide by 60 here for the H:MM render.
    fn refresh_duration_value_label(&self) {
        let mins = match self.current_mode() {
            TimerMode::Timer     => self.countdown_target_secs.get() / 60,
            TimerMode::Breathing => self.breathing_session_secs.get() / 60,
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
        self.view_stack.set_visible_child_name("setup");
        self.countdown_inputs.set_sensitive(false);
        self.boxbreath_inputs.set_sensitive(false);
        self.mode_toggle_group.set_sensitive(false);
        self.session_group.set_sensitive(false);
        self.big_time_label.set_label(&format_time(Duration::from_secs(display_secs)));
        self.time_unit_label.set_label(&gettext("Paused"));
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
        let guided_duration_secs = self
            .guided_pick
            .borrow()
            .as_ref()
            .map_or(0, |p| p.duration_secs);
        let label = meditate_core::format::idle_hero_label_from_modes(
            self.current_mode().into(),
            meditate_core::bells::DisplayMode::from_stopwatch_flag(
                self.stopwatch_toggle_on.get(),
            ),
            self.countdown_target_secs.get(),
            self.breathing_session_secs.get(),
            guided_duration_secs,
        );
        self.big_time_label.set_label(&label);
        self.time_unit_label.set_label(&gettext("Hours · Minutes"));
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
        self.refresh_auto_finish_state();
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
                    meditate_core::bells::end_bell_row_state(
                        db,
                        meditate_core::bells::DisplayMode::from_stopwatch_flag(stopwatch_on),
                        self.current_mode().into(),
                    )
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
        // A Volume-row preview doesn't ring into the session, and a
        // level still settling is what the session's bells read.
        crate::sound::stop_preview();
        crate::sound::rearm_audio_error_toast();
        self.flush_volume_rows();
        // Nor can an Undo change its settings: every toast goes.
        if let Some(window) = self.obj().root().and_downcast::<crate::window::MeditateWindow>() {
            window.dismiss_toasts();
        }

        // Each mode builds only its shape; the settings come from core
        // (preparation, bells, cues), and `CoreSession::start` opens in
        // Prep when `prep_secs` is set.
        let stopwatch_on = self.stopwatch_toggle_on.get();
        let shape = match mode {
            TimerMode::Timer => {
                // A 0-target countdown doesn't start at all.
                if stopwatch_on {
                    CoreSessionShape::TimerStopwatch
                } else {
                    let target_secs = self.countdown_target_secs.get();
                    if target_secs == 0 {
                        return;
                    }
                    CoreSessionShape::TimerCountdown { target_secs }
                }
            }
            TimerMode::Breathing => {
                let pattern = self.breathing_pattern.get();
                // "Finish the breath" before stopping: round to the
                // next full cycle so the session always ends on an
                // exhale/hold-out boundary.
                let target = pattern.cycle_aligned_target_secs(
                    u64::from(self.breathing_session_secs.get()),
                );
                // Stopwatch toggle on → BoxBreathStopwatch (no auto-end;
                // user must press Stop). Off → BoxBreathCountdown,
                // cycle-aligned end fires off `target_secs`. Either
                // way Box Breath shows count-up elapsed.
                if stopwatch_on {
                    CoreSessionShape::BoxBreathStopwatch { pattern }
                } else {
                    CoreSessionShape::BoxBreathCountdown {
                        pattern,
                        target_secs: target as u32,
                    }
                }
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
                let target = u64::from(pick.duration_secs);
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
                            .map(meditate_core::session::Session::enter_overtime)
                            .unwrap_or_default();
                        imp.dispatch_session_effects(&effects);
                        if effects.iter().any(|e| matches!(e, CoreSessionEffect::AutoFinish { .. })) {
                            imp.cancel_tick();
                            // Not from inside the track's own callback:
                            // the end drops the track.
                            let obj = obj_for_eos.clone();
                            glib::idle_add_local_once(move || obj.imp().auto_finish_session());
                        } else if effects.iter().any(|e| matches!(e, CoreSessionEffect::EnterOvertime)) {
                            imp.transition_running_to_overtime();
                        }
                    },
                ) {
                    Ok(playback) => {
                        *self.guided_playback.borrow_mut() = Some(playback);
                    }
                    Err(e) => {
                        crate::db::show_in_view(gettext("Couldn't start playback: {error}").replace("{error}", &e.to_string()));
                        return;
                    }
                }

                // Guided always carries a target (the file's probed
                // duration); the stopwatch toggle only flips the
                // running display between count-up and count-down.
                CoreSessionShape::Guided {
                    duration_secs: target as u32,
                    count_up_display: stopwatch_on,
                }
            }
        };

        let Some(app) = self.get_app() else { return; };
        let core_settings = app
            .with_db(|db| CoreSessionSettings::from_db(db, shape.clone()))
            .unwrap_or(CoreSessionSettings { shape, ..Default::default() });
        self.start_boot_time.set(Some(boot_time_now()));
        let (session, start_effects) = CoreSession::start(
            core_settings,
            std::time::Duration::ZERO,
        );
        self.dispatch_session_effects(&start_effects);
        *self.core_session.borrow_mut() = Some(session);

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
        // - No-prep Timer: returned by CoreSession::start and
        //   dispatched above.
        // - With-prep Timer: dispatched alongside EndPrep at the prep
        //   boundary tick (see Session::tick_prep).
        // - Box Breath / Guided: starting_bell is None in their
        //   SessionSettings, so no effect ever emits — matches the
        //   prior Timer-only behaviour.

        self.tick_mode.set(mode);
        // Every mode ticks on a timer, so a minimised window or a
        // blank screen doesn't stop the session; Box Breath's
        // DrawingArea frame callback only draws.
        self.start_tick();
        self.media_failed.set(false);
        self.refresh_media();
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
        self.start_tick();
        if let Some(app) = self.get_app() {
            self.acquire_screen_awake_lock(&app);
        }
        self.refresh_end_buttons();
        self.refresh_media();
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
        // Esc reaches here in any state; overtime can't pause (#126).
        if meditate_core::session::Session::toggle_action(self.ui_state()) != meditate_core::session::ToggleAction::Pause {
            return;
        }
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
        // A forgotten pause shouldn't keep the computer awake.
        if let Some(app) = self.get_app() {
            self.release_screen_awake_lock(&app);
        }

        self.refresh_end_buttons();
        self.refresh_media();

        self.show_paused_ui(self.current_display_secs());
        self.obj().emit_by_name::<()>("timer-paused", &[]);
    }

    /// End the in-flight session with `f` (stop, finish or add) and
    /// leave the running page showing the ended session's buttons.
    /// Returns the duration core recorded.
    fn end_in_flight<F>(&self, f: F) -> Option<u64>
    where
        F: FnOnce(&mut CoreSession, Duration) -> Vec<CoreSessionEffect>,
    {
        self.cancel_tick();
        let elapsed = self.core_session_end(f);
        self.session_ended();
        elapsed
    }

    /// Shell side of any end: the track stops, the inhibit goes, the
    /// snapshot holds the final length for a kill before Save, and
    /// the page shows Save and Discard.
    fn session_ended(&self) {
        // Drop runs set_state(Null) + drops the bus signal-watch.
        *self.guided_playback.borrow_mut() = None;
        if let Some(app) = self.get_app() {
            self.release_screen_awake_lock(&app);
        }
        if let Some(secs) = self.session_final_duration_secs() {
            self.write_in_progress_snapshot(secs.try_into().unwrap_or(u32::MAX));
        }
        self.refresh_end_buttons();
        self.refresh_media();
    }

    /// Show the session in the shell's media controls, or take it
    /// off once there is no live session.
    fn refresh_media(&self) {
        // A failed registration leaves no player to update or drop.
        if self.media_failed.get() {
            return;
        }
        let now = self.elapsed_since_start();
        let guided = self.guided_file_name(self.tick_mode.get() == TimerMode::Guided);
        let shown = self.core_session.borrow().as_ref().and_then(|s| crate::mpris::shown(s, now, guided.as_deref()));
        let Some(shown) = shown else {
            self.media_player.take();
            return;
        };
        if let Some(player) = self.media_player.borrow().as_ref() {
            player.update(shown);
            return;
        }
        let Some(conn) = self.get_app().and_then(|app| app.dbus_connection()) else { return; };
        let weak = self.obj().downgrade();
        let toggle = move || {
            if let Some(obj) = weak.upgrade() {
                obj.imp().toggle_playback();
            }
        };
        let weak = self.obj().downgrade();
        let raise = move || {
            if let Some(win) = weak.upgrade().and_then(|o| o.root()).and_downcast::<gtk::Window>() {
                win.present();
            }
        };
        let player = crate::mpris::Player::start(&conn, shown, toggle, raise);
        self.media_failed.set(player.is_none());
        *self.media_player.borrow_mut() = player;
    }

    /// Set the running page's buttons and note from core's button set.
    fn refresh_end_buttons(&self) {
        use meditate_core::session::EndButton;
        let Some(buttons) = self.core_session.borrow().as_ref().map(CoreSession::end_buttons) else { return; };
        let controls = self.running_controls.borrow();
        let Some(c) = controls.as_ref() else { return; };
        c.second.set_visible(buttons.second.is_some());
        for (btn, which) in [&c.first, &c.second].into_iter().zip(buttons.row()) {
            let Some(which) = which else { continue; };
            let (label, classes): (String, &[&str]) = match which {
                EndButton::Pause => (gettext("Pause"), &["pill"]),
                EndButton::Resume => (gettext("Resume"), &["pill", "suggested-action"]),
                EndButton::Save => (gettext("Save"), &["pill", "suggested-action"]),
                EndButton::Add => (overtime_add_button_label(self.overtime_delta.get()), &["pill", "tabular-nums"]),
            };
            btn.set_label(&label);
            btn.set_css_classes(classes);
            // "+ 03:00 and Save" takes two lines where one doesn't fit.
            if let Some(text) = btn.child().and_downcast::<gtk::Label>() {
                text.set_wrap(true);
                text.set_justify(gtk::Justification::Center);
            }
        }
        c.note.set_visible(buttons.note);
    }

    /// One of the running page's two row buttons was clicked.
    pub(crate) fn end_button_clicked(&self, index: usize) {
        use meditate_core::session::EndButton;
        let Some(buttons) = self.core_session.borrow().as_ref().map(CoreSession::end_buttons) else { return; };
        let Some(which) = buttons.row()[index] else { return; };
        match which {
            EndButton::Pause => self.on_pause(),
            EndButton::Resume => self.on_resume(),
            EndButton::Add => {
                self.end_in_flight(CoreSession::add_overtime_and_finish);
                self.on_save(false);
            }
            EndButton::Save => {
                match self.ui_state() {
                    UiState::Overtime => { self.end_in_flight(|s, _now| s.finish_overtime()); }
                    UiState::Done => {}
                    _ => { self.end_in_flight(CoreSession::stop); }
                }
                self.on_save(false);
            }
        }
    }

    /// The countdown reached its end with Auto-Finish on: core already
    /// stopped the session at its planned length.
    fn auto_finish_session(&self) {
        // The tick source ends itself; forget its id.
        self.tick_source.borrow_mut().take();
        self.session_ended();
        self.notify_complete();
        if let Some(dialog) = self.discard_dialog.take() {
            dialog.close();
        }
        self.on_save(true);
    }

    /// "Meditation Complete" when the window isn't focused; the page
    /// itself shows the end otherwise.
    fn notify_complete(&self) {
        let Some(app) = self.get_app() else { return; };
        if app.active_window().is_some_and(|w| w.is_active()) {
            return;
        }
        let n = gtk::gio::Notification::new(&gettext("Meditation Complete"));
        let target = self
            .core_session
            .borrow()
            .as_ref()
            .map_or(0, meditate_core::session::Session::completion_duration_secs);
        n.set_body(Some(&gettext("Session: {duration}").replace("{duration}", &format_time(Duration::from_secs(target)))));
        app.send_notification(Some("timer-done"), &n);
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
            .and_then(meditate_core::session::Session::final_duration_secs)
    }

    fn on_save(&self, auto: bool) {
        if self.saving.get() {
            return;
        }
        // Auto-Finish's end bell plays on.
        if !auto {
            self.stop_active_signals();
        }
        let mode = self.current_mode();

        let elapsed = self.elapsed_secs_for_mode();
        let start_time = self.session_start_time.get();

        if elapsed == 0 {
            self.reset_mode(mode, true);
            return;
        }

        // Core saves a whitespace-only note as no note.
        let note = Some(self.note_text());
        // Setup's label; Setup already keeps it for next time.
        let label_id = self.setup_selected_label_id();

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
            guided_file_name: self.guided_file_name(mode == TimerMode::Guided),
        };
        // A file from the Local Folder: the save counts its play.
        let local_file_id = if mode == TimerMode::Guided {
            self.guided_pick.borrow().as_ref().and_then(|p| p.local_file_id)
        } else {
            None
        };

        // Fire-and-forget DB write on the blocking pool. SQLite fsync on
        // eMMC costs ~15 ms even with synchronous=NORMAL; doing it on the
        // main thread is directly felt as a stall at session end. When
        // the write lands we're back on the main thread (spawn_local) to
        // re-read the shown page (`data_changed`).
        if let Some(app) = self.get_app() {
            self.saving.set(true);
            let obj = self.obj().clone();
            glib::MainContext::default().spawn_local(async move {
                let result = app
                    .with_db_blocking(move |db| crate::db::save_ended_session(db, &data, local_file_id))
                    .await;
                obj.imp().saving.set(false);
                let session = match result {
                    Some(Ok(s)) => s,
                    Some(Err(e)) => {
                        meditate_core::log(
                            "session.save",
                            &meditate_core::format::session_save_failure_log_message(
                                meditate_core::format::SessionSaveFailureKind::StorageError,
                                &e.to_string(),
                            ),
                        );
                        crate::db::show_in_view(gettext("Couldn't save session: storage error"));
                        return;
                    }
                    None => {
                        meditate_core::log(
                            "session.save",
                            &meditate_core::format::session_save_failure_log_message(
                                meditate_core::format::SessionSaveFailureKind::DbUnopened,
                                "with_db_blocking returned None",
                            ),
                        );
                        crate::db::show_in_view(gettext("Couldn't save session: storage unavailable"));
                        return;
                    }
                };

                // Only now: on a failed write the page stays up, because the
                // next Start would overwrite the snapshot, the only copy.
                obj.imp().reset_mode(mode, false);
                let uuid = session.uuid.clone();
                app.data_changed(Refresh::SESSIONS);
                let win = app.main_window();
                if let Some(win) = win {
                    if auto && win.is_active() {
                        let app_for_undo = app.clone();
                        let win_for_undo = win.clone();
                        let toast = crate::db::undo_toast(gettext("Session saved"), move || {
                            // The play it counted goes with it (#324).
                            let deleted = app_for_undo.with_db(|db| db.undo_session_save(&uuid, local_file_id));
                            crate::db::write_ok(&win_for_undo, "session.auto_finish.undo", deleted, crate::db::Failed::Delete);
                            app_for_undo.data_changed(Refresh::ALL);
                        }, || {});
                        toast.set_timeout(8);
                        crate::db::post_toast(&win, Some("session.saved"), toast);
                    }
                }
            });
        }
    }

    /// The running page's note, empty when there is no page.
    fn note_text(&self) -> String {
        self.running_controls.borrow().as_ref().map_or_else(String::new, |c| {
            let buffer = c.note_view.buffer();
            let (start, end) = buffer.bounds();
            buffer.text(&start, &end, false).to_string()
        })
    }

    pub(crate) fn on_discard(&self) {
        if !self.note_text().is_empty() {
            let dialog = adw::AlertDialog::builder()
                .heading(gettext("Discard Session?"))
                .body(gettext("Your note will be lost."))
                .close_response("cancel")
                .default_response("discard")
                .build();
            // libadwaita-rs 0.9 doesn't expose set_response_use_underline,
            // so we can't mark a mnemonic letter on AdwAlertDialog buttons
            // without the underscore rendering literally. Return and Esc
            // still cover the common activations.
            dialog.add_response("cancel", &gettext("Cancel"));
            dialog.add_response("discard", &gettext("Discard"));
            dialog.set_response_appearance("discard", adw::ResponseAppearance::Destructive);

            let obj = self.obj().clone();
            let mode = self.current_mode();
            dialog.connect_response(None, move |_, id| {
                obj.imp().discard_dialog.take();
                if id == "discard" {
                    obj.imp().stop_active_signals();
                    obj.imp().reset_mode(mode, true);
                }
            });
            self.discard_dialog.replace(Some(dialog.clone()));

            dialog.present(Some(&*self.obj()));
        } else {
            self.stop_active_signals();
            self.reset_mode(self.current_mode(), true);
        }
    }

    /// Reset a single mode back to Idle and update the UI if it's currently shown.
    /// `clear_snapshot` is false only for Save, whose insert clears it.
    fn reset_mode(&self, mode: TimerMode, clear_snapshot: bool) {
        // Discard can end a session that still runs.
        self.cancel_tick();
        if let Some(app) = self.get_app() {
            self.release_screen_awake_lock(&app);
        }
        // Session is the sole owner of session timing state; drop
        // it once, regardless of mode. The gst playbin is the only
        // mode-specific gtk-side artefact left that still needs
        // explicit teardown.
        *self.core_session.borrow_mut() = None;
        self.media_player.take();
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

        // Cancel the 60 s snapshot heartbeat and drop the crash-
        // recovery snapshot. Every path that ends a session funnels
        // through here; Save leaves the drop to its own insert.
        self.cancel_snapshot_tick();
        if clear_snapshot {
            self.clear_in_progress_snapshot();
        }

        // The running page pops; its widgets go with it.
        *self.running_label.borrow_mut() = None;
        *self.running_controls.borrow_mut() = None;
        self.overtime_delta.set(Duration::ZERO);
        self.obj().emit_by_name::<()>("timer-stopped", &[]);

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
    /// The played file's title for the Log: the current pick's name.
    fn guided_file_name(&self, guided: bool) -> Option<String> {
        guided.then(|| self.guided_pick.borrow().as_ref().map(|p| p.display_name.clone())).flatten()
    }

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
            self.guided_selected_uuid.borrow().clone().map(meditate_core::db::GuidedFileUuid::new)
        } else {
            None
        };
        let snapshot = meditate_core::db::SessionInProgress {
            start_iso: meditate_core::time::unix_to_local_iso(unix_start),
            accumulated_secs,
            mode,
            guided_file_name: self.guided_file_name(matches!(mode, meditate_core::SessionMode::Guided)),
            label_id,
            guided_file_uuid,
        };
        // Local-only row, no event: nothing for a sync to push.
        // Only logged: telling the user is #323.
        if let Some(Err(e)) = app.with_db(|db| db.set_session_in_progress(&snapshot)) {
            meditate_core::log("session.recovery", &format!("set snapshot failed: {e}"));
        }
    }

    /// Drop the crash-recovery snapshot if any. Idempotent — a no-op
    /// when no session was in flight. DB errors are logged but
    /// otherwise swallowed: failing to clear the snapshot is at
    /// worst a one-off phantom auto-finalize on the next launch
    /// (which the user can Undo), not a state corruption.
    fn clear_in_progress_snapshot(&self) {
        let Some(app) = self.get_app() else { return; };
        // Only logged: telling the user is #323.
        if let Some(Err(e)) = app.with_db(|db| db.clear_session_in_progress()) {
            meditate_core::log("session.recovery", &format!("clear snapshot failed: {e}"));
        }
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
    /// Rewrite the snapshot with the current time, for a window close:
    /// the 60 s heartbeat alone would lose up to 59 s.
    pub(super) fn save_snapshot_now(&self) {
        if self.core_session.borrow().is_some() {
            let secs = self.elapsed_secs_for_mode().try_into().unwrap_or(u32::MAX);
            self.write_in_progress_snapshot(secs);
        }
    }

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
        // Box Breath cues land on phase boundaries, so it ticks every
        // 200 ms like the Android app; the others show whole seconds.
        let breathing = self.tick_mode.get() == TimerMode::Breathing;
        let interval = if breathing {
            std::time::Duration::from_millis(200)
        } else {
            std::time::Duration::from_secs(1)
        };

        let source_id = glib::timeout_add_local(
            interval,
            move || {
                let imp = obj.imp();
                let flow = match imp.ui_state() {
                    UiState::Preparing => imp.tick_prep(&obj),
                    UiState::Running if breathing => imp.tick_breathing(),
                    UiState::Running => imp.tick_running(&obj),
                    UiState::Overtime => imp.tick_overtime(&obj),
                    _ => glib::ControlFlow::Break,
                };
                imp.refresh_media();
                flow
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
        // Save joins Pause once the session runs.
        if effects.contains(&CoreSessionEffect::EndPrep) {
            self.refresh_end_buttons();
        }
        glib::ControlFlow::Continue
    }

    /// Box Breath tick: phase cues, the end bell and the cycle-aligned
    /// end all come from the Session's effects.
    fn tick_breathing(&self) -> glib::ControlFlow {
        let effects = self.box_breath_session_tick();
        self.dispatch_session_effects(&effects);
        if effects.iter().any(|e| matches!(e, CoreSessionEffect::AutoFinish { .. })) {
            self.auto_finish_session();
            return glib::ControlFlow::Break;
        }
        if effects.iter().any(|e| matches!(e, CoreSessionEffect::EndBoxBreath { .. })) {
            // The source ends with Break; forget its id so a later
            // cancel_tick doesn't remove it a second time. The page
            // stays up with Save and Discard.
            self.tick_source.borrow_mut().take();
            self.session_ended();
            self.notify_complete();
            return glib::ControlFlow::Break;
        }
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

        if effects.iter().any(|e| matches!(e, CoreSessionEffect::AutoFinish { .. })) {
            self.auto_finish_session();
            return glib::ControlFlow::Break;
        }
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
        // The guided track already stopped on core's StopGuidedAudio,
        // dispatched ahead of the end bell, which core also fired.
        self.notify_complete();
        self.overtime_delta.set(Duration::ZERO);
        self.refresh_end_buttons();
        // Hero stays frozen at the planned countdown duration —
        // the user chose that target, so the static reading is
        // their accomplishment. Only the Add button counts up,
        // surfacing how much extra time they've accumulated.
        // Core knows the planned length — the guided file's, not the
        // Timer setup's target.
        let planned = self
            .core_session
            .borrow()
            .as_ref()
            .map_or(0, meditate_core::session::Session::completion_duration_secs);
        if let Some(label) = self.running_label.borrow().as_ref() {
            label.set_label(&format_time(Duration::from_secs(planned)));
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

        self.overtime_delta.set(overtime);
        self.refresh_end_buttons();
        glib::ControlFlow::Continue
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
    /// elapsed; used by `tick_breathing`. Returns the effects
    /// (FireBoxBreathCue + the cycle-aligned EndBoxBreath). Empty Vec
    /// when no session is in flight or the session is paused.
    fn box_breath_session_tick(&self) -> Vec<CoreSessionEffect> {
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

    /// Only the streak chip, for a change that can't touch Setup.
    pub fn refresh_streak_label(&self) {
        if let Some(Ok(streak)) = self.get_app().and_then(|app| app.with_db(|db| meditate_core::db::get_streak_from_db(db, meditate_core::time::today_local()))) {
            self.set_streak_text(streak);
        }
    }

    fn set_streak_text(&self, streak: u32) {
        // .streak-chip applies text-transform: uppercase, so we keep the
        // source text sentence-case here. Core picks the variant; the
        // gtk shell maps each to a gettext-translated phrase.
        let text = match meditate_core::format::streak_key(streak) {
            StreakKey::Zero => gettext("Start your streak today"),
            StreakKey::One => ngettext("1 day streak", "{n} day streak", 1).replace("{n}", "1"),
            StreakKey::Many(n) =>
                ngettext("1 day streak", "{n} day streak", n)
                    .replace("{n}", &n.to_string()),
        };
        self.streak_label.set_label(&text);
    }

    /// Re-read Setup and the streak. Under a running or paused session
    /// only the streak: Setup would put its target over the session's
    /// time; `reset_mode` re-reads Setup when the session ends.
    pub fn refresh_streak(&self) {
        if self.ui_state() != UiState::Idle {
            self.refresh_streak_label();
            // Neither touches the session's time.
            self.refresh_setup_label_chooser_subtitle();
            self.refresh_setup_summaries();
            return;
        }
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
        // A sync may have renamed or deleted the selected guided file.
        self.follow_selected_guided_file();
        // ...or renamed the label or switched a bell the rows sum up (#20).
        self.refresh_setup_summaries();

        // Batch every DB read this visit needs into a single borrow:
        // one get_app() walk, one RefCell lock, four SQL queries instead
        // of as many separate calls. The bells block also rides along —
        // four extra get_setting() calls are cheap next to the existing
        // streak / labels SQL we're already running.
        let stopwatch_key = meditate_core::settings_keys::stopwatch_key_for_mode(self.current_mode().into());
        let (streak, stopwatch_on, bells, intervals) = app
            .with_db(|db| {
                use meditate_core::read_bool;
                let streak  = meditate_core::db::get_streak_from_db(db, meditate_core::time::today_local()).unwrap_or(0);
                let stopwatch_on = read_bool(db, stopwatch_key, false);
                let starting_bell_on = read_bool(db, "starting_bell_active", false);
                let starting_bell_sound = db
                    .get_setting("starting_bell_sound", "bowl")
                    .unwrap_or_else(|_| "bowl".to_string());
                let prep_on = read_bool(db, "preparation_time_active", false);
                let prep_secs = db
                    .get_setting(
                        "preparation_time_secs",
                        &meditate_core::format::PREP_SECS_DEFAULT.to_string(),
                    )
                    .map_or(meditate_core::format::PREP_SECS_DEFAULT, |s| meditate_core::format::parse_prep_secs(&s));
                let intervals_on = read_bool(db, "interval_bells_active", false);
                // Stopwatch mode mutes fixed-from-end bells — no end
                // to count backwards from. The persisted enabled flag
                // stays untouched (returns when stopwatch flips off);
                // the UI subtitle just reflects what will actually
                // fire right now.
                let intervals_enabled_count = meditate_core::bells::interval_bells_count(
                    db,
                    meditate_core::bells::DisplayMode::from_stopwatch_flag(stopwatch_on),
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
        self.preparation_time_secs_row.set_value(f64::from(prep_secs));
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
        self.refresh_auto_finish_state();

        self.set_streak_text(streak);

        // Rebuild visible starred-preset list for the current mode.
        self.rebuild_starred_presets_list();
        // Sync the duration row's value label with the current mode's length.
        self.refresh_duration_value_label();

        // End Bell master toggle is restored by refresh_stopwatch_
        // dependent_ui above (it calls refresh_end_bell_dependent_ui,
        // which reads end_bell_active and either applies it or
        // overrides to off when stopwatch mode is on). Just refresh
        // the sound-row subtitle, the pattern-row subtitle, and the
        // signal-mode toggle group's saved state + capability gating.
        self.refresh_end_bell_sound_subtitle();
        self.refresh_end_bell_pattern_subtitle();
        self.refresh_end_bell_signal_mode_state();
        self.refresh_volume_rows();

        // Rebuild the Setup view's label chooser-row + master toggle
        // from the per-mode persisted state.
        self.apply_preferred_label_for_mode(self.current_mode());
    }

    /// After a Setup write: rebuild once the writes pause, so a held
    /// spin button or a dragged slider rebuilds once, not per step.
    pub fn queue_presets_rebuild(&self) {
        if let Some(queued) = self.presets_rebuild_queued.take() { queued.remove(); }
        let obj = self.obj().downgrade();
        let id = glib::timeout_add_local_once(Duration::from_millis(300), move || {
            if let Some(obj) = obj.upgrade() {
                obj.imp().presets_rebuild_queued.take();
                obj.imp().rebuild_starred_presets_list();
            }
        });
        self.presets_rebuild_queued.replace(Some(id));
    }

    /// Rebuild the visible starred-preset list — mode-strict, so a
    /// Timer-mode user only ever sees Timer presets here (and same
    /// for Box Breath). Empty list ⇒ a description hint sitting under
    /// the group title prompts the user to Save Settings; non-empty ⇒
    /// the description is cleared and one row per preset is appended,
    /// each tap-to-apply. Replaces a queued rebuild.
    pub fn rebuild_starred_presets_list(&self) {
        if let Some(queued) = self.presets_rebuild_queued.take() { queued.remove(); }
        // Drop any rows from the previous mode / refresh — Adw groups
        // don't expose a .clear() so we walk the tracking vec.
        let focused = focused_row_name(self.starred_preset_rows.borrow().iter().map(|(row, _)| row));
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
            .and_then(|app| app.with_db(|db| meditate_core::db::list_starred_presets_for_mode_from_db(db, session_mode)))
            .and_then(std::result::Result::ok)
            .unwrap_or_default();

        if presets.is_empty() {
            self.presets_group.set_description(Some(
                &gettext("Tap Save Settings to create your first preset"),
            ));
            return;
        }
        self.presets_group.set_description(None::<&str>);

        // Once per rebuild, so each row's subtitle lookup is O(1).
        let label_names: std::collections::HashMap<String, String> = app_opt
            .as_ref()
            .and_then(|app| app.with_db(meditate_core::db::label_names_from_db))
            .and_then(std::result::Result::ok)
            .unwrap_or_default();
        let active = app_opt
            .as_ref()
            .and_then(|app| app.with_db(|db| meditate_core::preset_config::active_presets(db, session_mode, &presets)))
            .unwrap_or_default();

        let obj = self.obj();
        let move_to: crate::presets::MoveTo = {
            let obj = obj.downgrade();
            std::rc::Rc::new(move |anchor, uuid, to| {
                let Some(obj) = obj.upgrade() else { return };
                let imp = obj.imp();
                let Some(app) = imp.get_app() else { return };
                let written = app.with_db(|db| db.move_starred_preset(uuid, to));
                setting_saved(anchor, "preset.move", written);
                // At once: the rows' indices drive the next key move.
                imp.rebuild_starred_presets_list();
            })
        };
        let mut tracked: Vec<(adw::ActionRow, String)> = Vec::with_capacity(presets.len());
        for (index, p) in presets.into_iter().enumerate() {
            let row = adw::ActionRow::builder()
                .title(&p.name)
                .subtitle(preset_subtitle(&p, &label_names))
                .activatable(true)
                .build();
            if active.contains(&p.uuid.0) {
                // Setup matches this preset: the choosers' check plus
                // the accent title.
                let check = gtk::Image::from_icon_name("object-select-symbolic");
                check.add_css_class("selected-check");
                row.add_suffix(&check);
                row.add_css_class("accent");
                row.update_state(&[gtk::accessible::State::Selected(Some(true))]);
            }
            let uuid = p.uuid.0.clone();
            row.connect_activated(clone!(
                #[weak(rename_to = this)] obj,
                #[strong] uuid,
                move |_| this.imp().on_preset_row_activated(&uuid),
            ));
            crate::presets::add_reordering(&row, p.uuid.0.clone(), index, move_to.clone());
            row.set_widget_name(&p.uuid.0);
            self.presets_group.add(&row);
            tracked.push((row, p.uuid.0));
        }
        refocus_row(focused.as_deref(), tracked.iter().map(|(row, _)| row));
        *self.starred_preset_rows.borrow_mut() = tracked;
    }

    /// Rebuild the starred-guided-files list under
    /// `guided_files_group`. Mirrors `rebuild_starred_presets_list`
    /// shape — drain the tracking vec, query the DB for starred rows,
    /// rebuild fresh. Tap on a row populates the Selected slot AND
    /// stashes the uuid in `guided_selected_uuid` so the session-save
    /// path can record per-file attribution.
    pub fn rebuild_starred_guided_list(&self) {
        let focused = focused_row_name(self.starred_guided_rows.borrow().iter());
        for row in self.starred_guided_rows.borrow_mut().drain(..) {
            self.guided_files_group.remove(&row);
        }
        self.starred_guided_checks.borrow_mut().clear();

        let app_opt = self.get_app();
        let starred = app_opt
            .as_ref()
            .and_then(|app| app.with_db(meditate_core::local_folder::starred_guided_from_db))
            .and_then(std::result::Result::ok)
            .unwrap_or_default();

        if starred.is_empty() {
            // Empty-state row inside the group keeps the section
            // visually tall enough that the [Open / Import] buttons
            // above don't feel cramped against the group title.
            // Mirrors the bells.rs empty-state pattern.
            let row = adw::ActionRow::builder()
                .title(gettext("No starred files"))
                .subtitle(gettext(
                    "Tap Open File then Import File, or star a file in Manage Imports",
                ))
                .activatable(false)
                .selectable(false)
                .build();
            row.add_css_class("dim-label");
            self.guided_files_group.add(&row);
            self.starred_guided_rows.borrow_mut().push(row);
            return;
        }

        let move_to: crate::presets::MoveTo = {
            let obj = self.obj().downgrade();
            std::rc::Rc::new(move |anchor, key, to| {
                let Some(obj) = obj.upgrade() else { return };
                let imp = obj.imp();
                let Some(app) = imp.get_app() else { return };
                let id = meditate_core::local_folder::StarredGuidedId::from_key(key);
                let written = app.with_db(|db| db.move_starred_guided(&id, to));
                setting_saved(anchor, "guided.move", written);
                imp.rebuild_starred_guided_list();
            })
        };
        let mut tracked: Vec<adw::ActionRow> = Vec::with_capacity(starred.len());
        let mut checks = Vec::with_capacity(starred.len());
        for (index, entry) in starred.into_iter().enumerate() {
            let drag_key = entry.id().key();
            let (row, key) = match entry {
                meditate_core::local_folder::StarredGuided::Library(f) => {
                    let key = f.uuid.0.clone();
                    (self.starred_library_row(f), Some(key))
                }
                meditate_core::local_folder::StarredGuided::Local { file, found } => {
                    let key = found.then(|| format!("local:{}", file.id));
                    (self.starred_local_row(file, found), key)
                }
            };
            if let Some(key) = key {
                let check = gtk::Image::from_icon_name("object-select-symbolic");
                check.add_css_class("selected-check");
                row.add_suffix(&check);
                checks.push((key, row.clone(), check));
            }
            row.set_widget_name(&drag_key);
            crate::presets::add_reordering(&row, drag_key, index, move_to.clone());
            self.guided_files_group.add(&row);
            tracked.push(row);
        }
        refocus_row(focused.as_deref(), tracked.iter());
        *self.starred_guided_rows.borrow_mut() = tracked;
        *self.starred_guided_checks.borrow_mut() = checks;
        self.mark_selected_starred_row();
    }

    /// Mark the selected file's starred row as the preset rows mark
    /// theirs: the check, the accent title and the selected state.
    fn mark_selected_starred_row(&self) {
        let key = self.guided_selected_uuid.borrow().clone().or_else(|| {
            let local = self.guided_pick.borrow().as_ref().and_then(|p| p.local_file_id);
            local.map(|id| format!("local:{id}"))
        });
        for (k, row, check) in self.starred_guided_checks.borrow().iter() {
            let on = key.as_deref() == Some(k.as_str());
            check.set_visible(on);
            if on {
                row.add_css_class("accent");
            } else {
                row.remove_css_class("accent");
            }
            row.update_state(&[gtk::accessible::State::Selected(Some(on))]);
        }
    }

    /// A starred library file: tap makes it the current file.
    fn starred_library_row(&self, f: crate::db::GuidedFile) -> adw::ActionRow {
        let row = adw::ActionRow::builder()
            .title(glib::markup_escape_text(&f.name))
            .subtitle(crate::guided::format_duration_brief(f.duration_secs))
            .activatable(true)
            .build();
        // Star prefix (always on for the home-list — destarring
        // happens via Manage Files).
        let star = gtk::Image::from_icon_name("starred-symbolic");
        star.add_css_class("preset-star-on");
        row.add_prefix(&star);

        let uuid = f.uuid.0.clone();
        let obj = self.obj().clone();
        row.connect_activated(move |_| {
            let imp = obj.imp();
            // Promote this starred row into the Selected slot and
            // record its uuid for the session-save path.
            *imp.guided_selected_uuid.borrow_mut() = Some(uuid.clone());
            *imp.guided_pick.borrow_mut() = Some(library_pick(&f));
            imp.refresh_guided_selected_row();
            imp.refresh_hero_for_idle();
        });
        row
    }

    /// A starred Local Folder file. One the last scan did not find is
    /// highlighted, can't be picked, and can be unstarred right here.
    fn starred_local_row(
        &self,
        file: meditate_core::db::LocalGuidedFile,
        found: bool,
    ) -> adw::ActionRow {
        let detail = if found {
            crate::local_folder::length_text(file.duration_secs)
        } else {
            gettext("not found")
        };
        let row = adw::ActionRow::builder()
            .title(glib::markup_escape_text(meditate_core::local_folder::file_name(&file.rel_path)))
            .subtitle(glib::markup_escape_text(&format!("{} · {detail}", gettext("Local file"))))
            .activatable(true)
            .build();
        let Some(app) = self.get_app() else { return row };
        if found {
            let star = gtk::Image::from_icon_name("starred-symbolic");
            star.add_css_class("preset-star-on");
            row.add_prefix(&star);
            let obj = self.obj().clone();
            row.connect_activated(move |_| {
                obj.imp().select_local_pick(crate::local_folder::pick_for(&app, &file));
            });
            return row;
        }
        row.add_css_class("warning");
        row.add_prefix(&gtk::Image::from_icon_name("dialog-warning-symbolic"));
        let unstar = gtk::Button::builder()
            .icon_name("starred-symbolic")
            .tooltip_text(gettext("Unstar"))
            .valign(gtk::Align::Center)
            .css_classes(["flat", "preset-star-on"])
            .build();
        let (id, app_for_unstar) = (file.id, app.clone());
        let obj = self.obj().clone();
        unstar.connect_clicked(move |_| {
            crate::local_folder::save(&app_for_unstar, |db| db.set_local_file_starred(id, false));
            obj.imp().rebuild_starred_guided_list();
        });
        row.add_suffix(&unstar);
        let obj = self.obj().clone();
        row.connect_activated(move |_| {
            crate::db::show_toast(&obj, gettext("File not found in the local folder"));
        });
        row
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
                self.guided_selected_row.set_title(&glib::markup_escape_text(&pick.display_name));
                let length = if pick.reading_length() {
                    gettext("Reading length")
                } else {
                    crate::guided::format_duration_brief(pick.duration_secs)
                };
                let subtitle = match pick.folder.as_deref() {
                    None => length,
                    Some("") => format!("{} · {length}", gettext("Local folder")),
                    Some(folder) => format!("{} · {folder} · {length}", gettext("Local folder")),
                };
                self.guided_selected_row.set_subtitle(&glib::markup_escape_text(&subtitle));
                self.guided_selected_row.remove_css_class("dim-label");
            }
            None => {
                self.guided_selected_row.set_title(&gettext("No file selected"));
                self.guided_selected_row.set_subtitle(
                    &gettext("Tap Open File or pick from list below"),
                );
                self.guided_selected_row.add_css_class("dim-label");
            }
        }
        // Reflect the "you already have a pick — tapping replaces it"
        // semantic in the button label so the affordance is honest.
        self.open_file_btn.set_label(&if has_pick {
            gettext("Open New File")
        } else {
            gettext("Open File")
        });
        // Import button is greyed when there's no transient pick OR
        // when the current pick is already a starred library row
        // (selected_uuid Some → already imported).
        // Local Folder files are played in place, not imported.
        let local = self.guided_pick.borrow().as_ref().is_some_and(|p| p.local_file_id.is_some());
        let has_transient = has_pick && self.guided_selected_uuid.borrow().is_none();
        self.import_file_btn.set_sensitive(has_transient && !local);
        self.refresh_start_sensitive(self.current_mode());
        self.mark_selected_starred_row();
        // The device remembers what is shown, for the next start; an
        // Open File pick (no uuid, no local row) forgets.
        if let Some(app) = self.get_app().filter(|_| self.choice_restored.get()) {
            use meditate_core::local_folder::StarredGuidedId;
            let chosen = self.guided_selected_uuid.borrow().clone().map(StarredGuidedId::Library).or_else(|| {
                self.guided_pick.borrow().as_ref().and_then(|p| p.local_file_id).map(StarredGuidedId::Local)
            });
            if let Some(res) = app.with_db(|db| meditate_core::guided::remember_guided(db, chosen.as_ref())) {
                crate::db::write_ok(&*self.obj(), "guided.remember", Some(res), crate::db::Failed::Save);
            }
        }
    }

    /// Open the mode and the Guided file this device had last (#22).
    /// Once, before the window shows and before the recovery toast
    /// (a mode switch dismisses toasts).
    pub fn restore_last_choice(&self) {
        use meditate_core::local_folder::StarredGuidedId;
        let Some(app) = self.get_app() else { return };
        self.choice_restored.set(true);
        let Some((mode, file)) = app.with_db(|db| {
            (
                meditate_core::settings_keys::last_mode_from_db(db),
                meditate_core::guided::remembered_guided_from_db(db),
            )
        }) else {
            return;
        };
        match file {
            Some(StarredGuidedId::Library(uuid)) => {
                if let Some(Ok(Some(f))) = app.with_db(|db| meditate_core::db::find_guided_file_by_uuid_from_db(db, &uuid)) {
                    *self.guided_pick.borrow_mut() = Some(library_pick(&f));
                    *self.guided_selected_uuid.borrow_mut() = Some(uuid);
                }
            }
            Some(StarredGuidedId::Local(id)) => {
                if let Some(Ok(Some(file))) = app.with_db(|db| meditate_core::db::find_local_file_from_db(db, id)) {
                    self.select_local_pick(crate::local_folder::pick_for(&app, &file));
                }
            }
            None => {}
        }
        let name = MODE_NAMES.iter().find(|(_, m)| meditate_core::SessionMode::from(*m) == mode).map_or("timer", |(n, _)| *n);
        self.mode_toggle_group.set_active_name(Some(name));
        self.refresh_guided_selected_row();
        self.refresh_hero_for_idle();
    }

    /// Start waits while a Local Folder file's length is being read.
    fn refresh_start_sensitive(&self, mode: TimerMode) {
        let reading = mode == TimerMode::Guided
            && self.guided_pick.borrow().as_ref().is_some_and(|p| p.reading_length());
        self.start_btn.set_sensitive(!reading);
    }

    /// The selected library file was renamed or deleted (Manage Files,
    /// or a sync): show its new name, or clear it. Undo of that delete
    /// selects it again. Never during a session or on Done, whose Save
    /// reads the selection.
    fn follow_selected_guided_file(&self) {
        if self.ui_state() != UiState::Idle {
            return;
        }
        let Some(app) = self.get_app() else { return };
        let find = |uuid: &str| app.with_db(|db| meditate_core::db::find_guided_file_by_uuid_from_db(db, uuid)).and_then(Result::ok);
        let selected = self.guided_selected_uuid.borrow().clone();
        let cleared = self.guided_cleared_uuid.borrow().clone();
        let other_pick = selected.is_none() && self.guided_pick.borrow().is_some();
        use meditate_core::guided::Follow;
        match meditate_core::guided::follow_selection(selected.as_deref(), other_pick, cleared.as_deref(), |uuid| {
            find(uuid).map(|f| f.is_some())
        }) {
            Follow::Keep => return,
            Follow::Refresh | Follow::Reselect(_) => {
                let Some(uuid) = selected.or(cleared) else { return };
                let Some(Some(f)) = find(&uuid) else { return };
                *self.guided_pick.borrow_mut() = Some(library_pick(&f));
                *self.guided_selected_uuid.borrow_mut() = Some(uuid);
                *self.guided_cleared_uuid.borrow_mut() = None;
            }
            Follow::Clear => {
                *self.guided_pick.borrow_mut() = None;
                *self.guided_cleared_uuid.borrow_mut() = self.guided_selected_uuid.borrow_mut().take();
            }
        }
        self.refresh_guided_selected_row();
        self.refresh_hero_for_idle();
    }

    /// Make a Local Folder file the current file.
    pub fn select_local_pick(&self, pick: crate::guided::GuidedFilePick) {
        *self.guided_selected_uuid.borrow_mut() = None;
        *self.guided_pick.borrow_mut() = Some(pick);
        self.refresh_guided_selected_row();
        self.refresh_hero_for_idle();
    }

    /// After a Local Folder scan or length read.
    fn on_local_folder_change(&self) {
        if self.current_mode() == TimerMode::Guided {
            self.rebuild_starred_guided_list();
        }
        let waiting = self
            .guided_pick
            .borrow()
            .as_ref()
            .filter(|p| p.reading_length())
            .and_then(|p| p.local_file_id);
        let Some(id) = waiting else { return };
        let Some(app) = self.get_app() else { return };
        if crate::local_folder::length_failed(id) {
            *self.guided_pick.borrow_mut() = None;
            self.refresh_guided_selected_row();
            self.refresh_hero_for_idle();
            crate::db::show_in_view(gettext("Couldn't read audio file"));
            return;
        }
        let secs = app
            .with_db(|db| meditate_core::db::find_local_file_from_db(db, id))
            .and_then(Result::ok)
            .flatten()
            .and_then(|f| f.duration_secs);
        if let Some(secs) = secs {
            if let Some(pick) = self.guided_pick.borrow_mut().as_mut() {
                pick.duration_secs = secs;
            }
            self.refresh_guided_selected_row();
            self.refresh_hero_for_idle();
        }
    }

    /// Snapshot the live Setup state into a `PresetConfig`. Reads
    /// from the same persistence points the apply path writes to, so
    /// `apply_config(snapshot_current_setup())` is a round-trip with
    /// no observable change. Used by the Undo path on preset-tap
    /// (capture pre-apply state) and "Save current setup" (capture
    /// state to write into a new or overwritten preset). `None` when
    /// it can't be read (that said "Save failed") and in Guided, which
    /// has no presets.
    fn snapshot_current_setup(&self) -> Option<meditate_core::preset_config::PresetConfig> {
        use meditate_core::preset_config::{snapshot, PresetTiming};
        let mode = self.current_mode();

        // Build the live-UI-state half from gtk Cell<>s. Core's
        // snapshot consumes this + reads everything else from `db`.
        let timing = match mode {
            TimerMode::Timer => PresetTiming::Timer {
                stopwatch: self.stopwatch_toggle_on.get(),
                duration_secs: self.countdown_target_secs.get(),
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
            // Guided has no presets (its buttons are hidden); a
            // snapshot there would carry no length to apply.
            TimerMode::Guided => return None,
        };

        // A setup that can't be read is no preset: "Save failed" (#291).
        match self.get_app().and_then(|app| app.with_db(|db| snapshot(db, mode.into(), timing))) {
            Some(Ok(cfg)) => Some(cfg),
            failed => {
                setting_saved(&*self.obj(), "preset.snapshot", failed);
                None
            }
        }
    }

    /// Apply a `PresetConfig` to the live Setup state. Delegates the
    /// persistence work (settings + interval-bell replay +
    /// box-breath phase rows, the length) to
    /// `meditate_core::preset_config::apply`, then reloads the Setup
    /// from the DB with `refresh_streak`.
    ///
    /// Returns true iff the apply happened. On a DB error (or no DB)
    /// nothing of it was stored, and a failure has said "Save failed".
    fn apply_config(&self, cfg: &meditate_core::preset_config::PresetConfig) -> bool {
        use meditate_core::preset_config::apply;
        let Some(app) = self.get_app() else { return false; };
        let mode = self.current_mode();

        if !setting_saved(&*self.obj(), "preset.apply", app.with_db(|db| apply(db, cfg, mode.into()))) {
            return false;
        }
        // Core stored all of it, the length included; the reload shows
        // it (the hero too, through refresh_stopwatch_dependent_ui).
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
        let preset = match app.with_db(|db| meditate_core::db::find_preset_by_uuid_from_db(db, uuid)) {
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

        // A failure has said so already.
        let Some(snapshot) = self.snapshot_current_setup() else { return; };
        if !self.apply_config(&cfg) { return; }

        // Toast with Undo. The action callback re-applies the
        // pre-apply snapshot, putting every persistence point back to
        // its previous value (including a destructive interval-bells
        // round-trip — same DB cost as the forward apply, accepted
        // for what's already a low-frequency action).
        //
        // A second tap replaces this offer: undoing the previous preset
        // no longer makes sense.
        let obj = self.obj().clone();
        let toast = crate::db::undo_toast(
            gettext("'{name}' applied").replace("{name}", &preset.name),
            move || { obj.imp().apply_config(&snapshot); },
            || {},
        );
        crate::db::post_toast(&*self.obj(), Some("preset.apply"), toast);
    }

    /// False when the length could not be saved (the user was told).
    fn set_countdown_target(&self, secs: u32) -> bool {
        self.countdown_target_secs.set(secs);
        let label = meditate_core::format::format_hhmm(secs);
        self.big_time_label.set_label(&label);
        self.refresh_duration_value_label();
        if self.timer_populating.get() { return true; }
        let Some(app) = self.get_app() else { return true; };
        setting_saved(&*self.obj(), "timer.setting", app.with_db(|db| {
            meditate_core::settings_keys::set_timer_session_secs(db, secs)
        }))
    }

    /// Restore the persisted Timer-mode countdown target. Falls back
    /// to the 10-min default the Cell was initialised with if the
    /// setting is missing or unparseable. The `timer_populating` guard
    /// suppresses the write-back that `set_countdown_target` would
    /// otherwise do.
    fn load_timer_settings(&self) {
        let Some(app) = self.get_app() else { return; };
        let default = meditate_core::session::TIMER_DEFAULT_SECS;
        let secs = app
            .with_db(meditate_core::settings_keys::timer_session_secs_from_db)
            .unwrap_or(default);
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
                (f64::from(s / 3600), f64::from((s % 3600) / 60))
            }
            TimerMode::Breathing => {
                let m = self.breathing_session_secs.get() / 60;
                (f64::from(m / 60), f64::from(m % 60))
            }
            TimerMode::Guided => unreachable!("guarded above"),
        };

        let (dialog, hours_spin, minutes_spin) = custom_time_dialog(cur_h, cur_m);

        let obj = self.obj().clone();
        let mode_for_response = mode;
        dialog.connect_response(None, move |_, response| {
            if response != "set" { return; }
            let Some(total_mins) = custom_time_minutes(&hours_spin, &minutes_spin) else { return; };
            match mode_for_response {
                TimerMode::Timer => {
                    obj.imp().set_countdown_target(total_mins * 60);
                }
                TimerMode::Breathing => {
                    obj.imp().set_breathing_duration_secs(total_mins * 60);
                }
                TimerMode::Guided => {} // unreachable per show_custom_time_dialog guard
            }
        });

        dialog.present(Some(&*self.obj()));
    }

    fn resolve_label_for_mode(&self, mode: TimerMode) -> Option<Label> {
        self.get_app()?.with_db(|db| {
            meditate_core::labels::resolve_label_for_mode(db, mode.into())
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
            self.resolve_label_for_mode(mode).map_or_else(|| gettext("(none, pick one)"), |l| l.name)
        } else {
            gettext("Off")
        };
        self.setup_label_chooser_row.set_subtitle(&subtitle);

        // Also keep the ExpanderRow's switch state in sync without
        // re-firing the persist callback.
        self.labels_loading.set(true);
        self.setup_label_enabled_row.set_enable_expansion(active);
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
            .map_or(0, |s| s.display_secs(now))
    }

    /// The ring and bell counters around the running time, right now.
    pub fn bell_ring(&self) -> meditate_core::session::BellRing {
        let now = self.elapsed_since_start();
        self.core_session.borrow().as_ref().map(|s| s.bell_ring(now)).unwrap_or_default()
    }

    pub fn set_running_label(&self, label: gtk::Label) {
        *self.running_label.borrow_mut() = Some(label);
    }

    /// Both running pages hand over their buttons and note.
    pub fn set_running_controls(&self, controls: RunningControls) {
        *self.running_controls.borrow_mut() = Some(controls);
        self.refresh_end_buttons();
    }

    pub fn toggle_playback(&self) {
        use meditate_core::session::ToggleAction;
        match meditate_core::session::Session::toggle_action(self.ui_state()) {
            ToggleAction::Start => self.on_start(),
            ToggleAction::Pause => self.on_pause(),
            ToggleAction::Resume => self.on_resume(),
            ToggleAction::NoOp => {}
        }
    }
}

/// Which `MEDIA` slot a fire-cue effect routes through. Three
/// slots so polyphony works the way users expect: starting bell
/// supersedes its own prior playback; end bell supersedes its
use meditate_core::session::FireChannel;

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
                        db, meditate_core::settings_keys::stopwatch_key_for_mode(mode.into()), false,
                    );
                    meditate_core::bells::interval_bells_count(db, meditate_core::bells::DisplayMode::from_stopwatch_flag(stopwatch_on))
                })
            })
            .unwrap_or(0);
        self.interval_bells_row.set_subtitle(&intervals_count_subtitle(count));
    }

    fn refresh_bell_row_names(&self) {
        if self.get_app().is_none() {
            return; // Not wired yet; the first refresh_streak fills them.
        }
        self.refresh_starting_bell_sound_subtitle();
        self.refresh_end_bell_sound_subtitle();
        self.refresh_starting_bell_pattern_subtitle();
        self.refresh_end_bell_pattern_subtitle();
        for phase in BoxBreathPhaseId::all() {
            self.refresh_boxbreath_phase_subtitles(*phase);
        }
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
        let name = self.lookup_sound_name_for_setting(
            meditate_core::settings_keys::end_bell_sound_key_for_mode(self.current_mode().into()),
        );
        self.end_bell_sound_row.set_subtitle(&name);
    }

    /// Cut any in-flight bell sound + vibration pattern. Sole shell-
    /// side implementation of `Effect::StopActiveSignals`; only
    /// called from `dispatch_session_effects` and from `on_save` /
    /// `on_discard` (which are pure shell flows with no Session
    /// alive). The decision *when* to stop signals belongs to
    /// Session — see the variant's doc-comment in core.
    fn stop_active_signals(&self) {
        self.signal_gen.set(self.signal_gen.get().wrapping_add(1));
        crate::sound::stop_all();
        // Dropping the handle cancels the pattern.
        self.current_vibration.borrow_mut().take();
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
            // Guided: drop the playbin before the end bell in the
            // same batch, so the bell isn't competing with the last
            // frames of audio (gst playbin buffers ahead of the wall
            // clock). Drop runs set_state(Null) + removes the bus
            // watch.
            if matches!(effect, CoreSessionEffect::StopGuidedAudio) {
                *self.guided_playback.borrow_mut() = None;
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
        let wait = if route.signal_mode.includes_sound() {
            match route.channel {
                FireChannel::Interval => crate::sound::play_interval_sound(route.sound_uuid, route.volume, app),
                FireChannel::Starting => crate::sound::play_starting_uuid(route.sound_uuid, route.volume, app),
                FireChannel::End => crate::sound::play_end_bell_uuid(route.sound_uuid, route.volume, app),
            }
        } else {
            std::time::Duration::ZERO
        };
        let pattern = if route.signal_mode.includes_vibration() && app.has_haptic() {
            app.with_db(|db| meditate_core::db::find_vibration_pattern_by_uuid_from_db(db, route.vibration_pattern_uuid))
                .and_then(std::result::Result::ok)
                .flatten()
        } else {
            None
        };
        meditate_core::log(
            route.log_tag,
            &format!(
                "signal_mode={} fired={}",
                route.signal_mode.as_db_str(),
                pattern.is_some(),
            ),
        );
        match pattern {
            // With a lead-in the sound starts late; the buzz waits for it.
            Some(pattern) if !wait.is_zero() => {
                let generation = self.signal_gen.get();
                let this = self.obj().downgrade();
                let app = app.clone();
                glib::timeout_add_local_once(wait, move || {
                    let Some(this) = this.upgrade() else { return };
                    if this.imp().signal_gen.get() != generation {
                        return;
                    }
                    let handle = crate::vibration::PatternPlayback::play(&app, &pattern);
                    crate::vibration::replace_playback(&mut this.imp().current_vibration.borrow_mut(), Some(handle));
                });
            }
            pattern => {
                let handle = pattern.map(|pattern| crate::vibration::PatternPlayback::play(app, &pattern));
                crate::vibration::replace_playback(&mut self.current_vibration.borrow_mut(), handle);
            }
        }
    }

    pub(crate) fn refresh_end_bell_pattern_subtitle(&self) {
        let name = self.lookup_pattern_name_for_setting(
            meditate_core::settings_keys::end_bell_pattern_key_for_mode(self.current_mode().into()),
        );
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
            crate::bells::render_resolved_name(
                meditate_core::bells::resolve_sound_name(db, &uuid),
            )
        }).unwrap_or_default()
    }

    fn lookup_pattern_name_for_setting(&self, setting_key: &str) -> String {
        let Some(app) = self.get_app() else { return String::new(); };
        app.with_db(|db| {
            let uuid = db
                .get_setting(setting_key, crate::db::BUNDLED_PATTERN_PULSE_UUID)
                .unwrap_or_default();
            crate::bells::render_resolved_name(
                meditate_core::bells::resolve_pattern_name(db, &uuid),
            )
        }).unwrap_or_default()
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// A library file as the current pick, played from this device's copy.
fn library_pick(f: &crate::db::GuidedFile) -> crate::guided::GuidedFilePick {
    crate::guided::GuidedFilePick {
        display_name: f.name.clone(),
        source_path: meditate_core::audio_files::guided_file_path(
            &crate::sync_runner::local_guided_dir(),
            f.uuid.as_str(),
        ),
        duration_secs: f.duration_secs,
        local_file_id: None,
        folder: None,
    }
}

/// "+ MM:SS and Save" running-page button label. `{time}` is the
/// placeholder for the formatted overtime duration; translators can
/// position it anywhere in the localised string. Core returns the
/// formatted MM:SS via `format_time`; the shell owns word order via
/// this gettext template.
fn overtime_add_button_label(overtime: std::time::Duration) -> String {
    gettext("+ {time} and Save")
        .replace("{time}", &meditate_core::format::format_time(overtime))
}

/// The Session and Bells rows' subtitles: core's parts in core's order (#20).
fn session_summary_text(parts: &[meditate_core::preset_config::SessionSummaryPart]) -> String {
    use meditate_core::preset_config::SessionSummaryPart;
    parts.iter().map(|p| match p {
        SessionSummaryPart::Stopwatch => gettext("Stopwatch"),
        SessionSummaryPart::Label(name) => glib::markup_escape_text(name).to_string(),
        SessionSummaryPart::AutoFinish => gettext("Auto-Finish"),
    }).collect::<Vec<_>>().join(" · ")
}

fn bells_summary_text(parts: &[meditate_core::preset_config::BellsSummaryPart]) -> String {
    use meditate_core::preset_config::BellsSummaryPart;
    if parts.is_empty() {
        return gettext("No bells");
    }
    parts.iter().map(|p| match *p {
        BellsSummaryPart::StartingBell => gettext("Starting Bell"),
        BellsSummaryPart::IntervalBells(n) => ngettext("1 interval bell", "{n} interval bells", n as u32)
            .replace("{n}", &n.to_string()),
        BellsSummaryPart::EndBell => gettext("End Bell"),
        BellsSummaryPart::PhaseCues => gettext("Cues during phases"),
    }).collect::<Vec<_>>().join(" · ")
}

/// library's bells are currently enabled. Uses gettext so the count
/// can be localised; "None" is its own string for grammatical reasons
/// in some languages.
fn intervals_count_subtitle(enabled_count: usize) -> String {
    use meditate_core::format::IntervalsCountKey;
    match meditate_core::format::intervals_count_key(enabled_count) {
        IntervalsCountKey::None => gettext("None enabled"),
        IntervalsCountKey::One => ngettext("1 enabled", "{n} enabled", 1).replace("{n}", "1"),
        IntervalsCountKey::Many(n) => ngettext("1 enabled", "{n} enabled", n as u32)
            .replace("{n}", &n.to_string()),
    }
}

use crate::preset_subtitle::preset_subtitle;

use meditate_core::time::unix_now;

// ── Breathing (Box Breath) setup wiring ───────────────────────────────────────

// Per-phase + cycle invariants live in meditate_core::breath.
use meditate_core::breath::{clamp_session_secs, CYCLE_MIN_SECS, PHASE_MAX_SECS};
use meditate_core::format::StreakKey;

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
            .tooltip_text(gettext("Decrease"))
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
            .tooltip_text(gettext("Increase"))
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
    fn set_breathing_duration_secs(&self, secs: u32) -> bool {
        let secs = clamp_session_secs(secs);
        self.breathing_session_secs.set(secs);
        let saved = self.save_breathing_settings();
        // Duration row label is shared between modes; reflect the new
        // value here so a Box-Breath edit shows up immediately.
        self.refresh_duration_value_label();
        if self.current_mode() == TimerMode::Breathing {
            self.refresh_hero_for_idle();
        }
        saved
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
        if p.cycle().as_secs() < u64::from(CYCLE_MIN_SECS) {
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
                l.set_label(&crate::format::secs_text(&val.to_string()));
            }
        }
    }

    fn load_breathing_settings(&self) {
        let Some(app) = self.get_app() else { return; };
        self.breathing_populating.set(true);
        let (p, secs) = app.with_db(|db| {
            meditate_core::settings_keys::breathing_from_db(db)
        }).unwrap_or((
            BreathPattern::box_breath(),
            meditate_core::session::BREATHING_DEFAULT_SECS,
        ));
        self.breathing_pattern.set(p);
        self.breathing_session_secs.set(secs);
        self.refresh_duration_value_label();
        self.breathing_populating.set(false);
    }

    fn save_breathing_settings(&self) -> bool {
        if self.breathing_populating.get() { return true; }
        let Some(app) = self.get_app() else { return true; };
        let p = self.breathing_pattern.get();
        let secs = self.breathing_session_secs.get();
        setting_saved(&*self.obj(), "timer.setting", app.with_db(|db| meditate_core::settings_keys::set_breathing(db, p, secs)))
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

    /// Show every Setup value as saved, after a write that failed.
    fn reload_setup(&self) {
        self.refresh_streak();
        self.refresh_setup_label_chooser_subtitle();
    }

    /// The Session and Bells rows' subtitles on the main screen (#20).
    pub(crate) fn refresh_setup_summaries(&self) {
        use meditate_core::preset_config::{bells_summary, session_summary};
        let Some(app) = self.get_app() else { return; };
        let mode: SessionMode = self.current_mode().into();
        let Some((session, bells)) = app.with_db(|db| (session_summary(db, mode), bells_summary(db, mode))) else { return; };
        self.session_summary_row.set_subtitle(&session_summary_text(&session));
        self.bells_summary_row.set_subtitle(&bells_summary_text(&bells));
    }

    fn on_time_tapped(&self) {
        if self.duration_row.is_visible() && self.duration_row.is_sensitive() {
            self.show_custom_time_dialog();
        }
    }

    /// The start bar of Session, Bells and Manage Bells (#20): the main
    /// screen's time and Start, so a session starts without going back.
    pub(crate) fn build_start_bar(&self) -> gtk::Widget {
        let obj = self.obj();
        let time = gtk::Label::builder().css_classes(["start-bar-time"]).build();
        let unit = gtk::Label::builder().css_classes(["caption", "dimmed"]).build();
        for (from, to) in [(&*self.big_time_label, &time), (&*self.time_unit_label, &unit)] {
            from.bind_property("label", to, "label").sync_create().build();
        }
        self.time_unit_label.bind_property("visible", &unit, "visible").sync_create().build();
        let column = gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(4).build();
        column.append(&time);
        column.append(&unit);
        let time_btn = gtk::Button::builder()
            .child(&column)
            .tooltip_text(gettext("Duration"))
            .valign(gtk::Align::Center)
            .css_classes(["flat"])
            .build();
        for prop in ["can-target", "focusable"] {
            self.time_btn.bind_property(prop, &time_btn, prop).sync_create().build();
        }
        time_btn.connect_clicked(clone!(
            #[weak(rename_to = this)] obj,
            move |_| this.imp().on_time_tapped()
        ));
        let start = gtk::Button::builder()
            .hexpand(true)
            .valign(gtk::Align::Center)
            .css_classes(["pill", "suggested-action", "start-bar-button"])
            .build();
        for prop in ["label", "sensitive", "visible"] {
            self.start_btn.bind_property(prop, &start, prop).sync_create().build();
        }
        start.connect_clicked(clone!(
            #[weak(rename_to = this)] obj,
            move |_| this.imp().start_btn.emit_clicked()
        ));
        let bar = gtk::Box::builder().spacing(18).margin_top(6).margin_bottom(6).margin_start(18).margin_end(18).build();
        bar.append(&time_btn);
        bar.append(&start);
        adw::Clamp::builder().maximum_size(360).tightening_threshold(300).child(&bar).build().upcast()
    }

    fn persisted_label_active_for_mode(&self, mode: TimerMode) -> bool {
        self.get_app()
            .and_then(|app| {
                app.with_db(|db| {
                    meditate_core::labels::label_active_from_db(db, mode.into())
                })
            })
            .unwrap_or(mode == TimerMode::Guided)
    }

    fn persist_label_uuid_for_mode(&self, mode: TimerMode, uuid: &str) {
        let Some(app) = self.get_app() else { return; };
        setting_saved(&*self.obj(), "timer.setting", app.with_db(|db| meditate_core::labels::persist_uuid_for_mode(db, mode.into(), uuid)));
    }
}

/// The Custom Time dialog and its two spins, opened on `h`:`m`.
fn custom_time_dialog(h: f64, m: f64) -> (adw::AlertDialog, gtk::SpinButton, gtk::SpinButton) {
    // Tooltips double as accessible names — without them screen
    // readers only announce the raw numeric value.
    let hours_spin = gtk::SpinButton::builder()
        .orientation(gtk::Orientation::Vertical)
        .numeric(true)
        .width_chars(2)
        .adjustment(&gtk::Adjustment::new(h, 0.0, 23.0, 1.0, 1.0, 0.0))
        .tooltip_text(gettext("Hours"))
        .build();
    let minutes_spin = gtk::SpinButton::builder()
        .orientation(gtk::Orientation::Vertical)
        .numeric(true)
        .width_chars(2)
        .adjustment(&gtk::Adjustment::new(m, 0.0, 59.0, 1.0, 5.0, 0.0))
        .tooltip_text(gettext("Minutes"))
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
        .heading(gettext("Custom Time"))
        .body(gettext("Hours : Minutes"))
        .close_response("cancel")
        .default_response("set")
        .extra_child(&row)
        .build();
    dialog.add_response("cancel", &gettext("Cancel"));
    dialog.add_response("set", &gettext("Set"));
    dialog.set_response_appearance("set", adw::ResponseAppearance::Suggested);
    // Never 0:00 (#475): Set is off while both are zero, as typed
    // (a spin takes typed text only on Tab or Enter; empty commits as 0).
    let gate = |dialog: &adw::AlertDialog, h: &gtk::SpinButton, m: &gtk::SpinButton| {
        let shown = |s: &gtk::SpinButton| s.text().trim().parse::<f64>().unwrap_or(0.0);
        dialog.set_response_enabled("set", shown(h) > 0.0 || shown(m) > 0.0);
    };
    gate(&dialog, &hours_spin, &minutes_spin);
    for spin in [&hours_spin, &minutes_spin] {
        spin.connect_changed(clone!(
            #[weak] dialog, #[weak] hours_spin, #[weak] minutes_spin,
            move |_| gate(&dialog, &hours_spin, &minutes_spin)
        ));
    }

    (dialog, hours_spin, minutes_spin)
}

/// The minutes Set chose, typed text included; `None` at 0:00.
fn custom_time_minutes(h: &gtk::SpinButton, m: &gtk::SpinButton) -> Option<u32> {
    h.update();
    m.update();
    let total = h.value() as u32 * 60 + m.value() as u32;
    (total > 0).then_some(total)
}

#[cfg(test)]
mod tests {
    fn production() -> String {
        let src = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/timer/imp.rs")).unwrap();
        src.split("#[cfg(test)]\nmod tests").next().unwrap().to_string()
    }

    /// Set at 0:00 closed the dialog and changed nothing, without a
    /// word (#475): Set is off while the time is zero.
    #[test]
    fn custom_time_cannot_be_set_to_zero() {
        use adw::prelude::*;
        crate::gtk_test::run(|| {
            let (dialog, hours, minutes) = super::custom_time_dialog(0.0, 10.0);
            assert!(dialog.is_response_enabled("set"));
            minutes.set_value(0.0);
            assert!(!dialog.is_response_enabled("set"), "0:00");
            hours.set_value(1.0);
            assert!(dialog.is_response_enabled("set"), "1:00");
            // Typed, not yet committed by Tab or Enter.
            hours.set_text("0");
            assert!(!dialog.is_response_enabled("set"), "0 typed");
            minutes.set_text("5");
            assert!(dialog.is_response_enabled("set"), "5 typed");
            // An emptied field is 0 to GTK when it commits.
            minutes.set_value(10.0);
            minutes.set_text("");
            assert!(!dialog.is_response_enabled("set"), "0 and empty");
            // Each field on its own: "-1" commits as 0 hours.
            hours.set_text("-1");
            minutes.set_text("1");
            assert!(dialog.is_response_enabled("set"), "-1 and 1");
            // Set takes typed text that Tab or Enter never committed.
            let (_, hours, minutes) = super::custom_time_dialog(0.0, 0.0);
            minutes.set_text("5");
            assert_eq!(super::custom_time_minutes(&hours, &minutes), Some(5));
            hours.set_text("1");
            minutes.set_text("");
            assert_eq!(super::custom_time_minutes(&hours, &minutes), Some(60));
            hours.set_text("0");
            assert_eq!(super::custom_time_minutes(&hours, &minutes), None);
            let (dialog, _, _) = super::custom_time_dialog(0.0, 0.0);
            assert!(!dialog.is_response_enabled("set"), "opened at 0:00");
        });
    }

    fn body(code: &str, sig: &str) -> String {
        let at = code.find(sig).unwrap_or_else(|| panic!("{sig} not found"));
        let end = code[at..].find("\n    }\n").unwrap();
        code[at..at + end].to_string()
    }

    #[test]
    fn a_failed_setting_says_so_and_springs_back() {
        // #191: a Timer page setting that fails to save says "Save
        // failed", and the page shows the saved value again, on the
        // next main-loop turn (spin rows are mid-change in the handler).
        let code = production();
        let at = code.find("pub(crate) fn setting_saved<").expect("setting_saved");
        let saved = &code[at..at + code[at..].find("\n}\n").unwrap()];
        let failed = saved.find("crate::db::write_ok(").expect("the failure message");
        let reload = saved.find("idle_add_local_once").expect("reload on the next turn");
        assert!(failed < reload && saved[reload..].contains(".reload_setup()"));
        assert_eq!(code.matches("crate::db::write_ok(").count(), 4, "every write goes through setting_saved, but the Undo of an auto-save and the two memory writes");
        // The memory of the last mode and file has no Setup value to show
        // again: a failure says so without the reload, which would refresh
        // the selected row, write again and fail again without end.
        for tag in ["\"mode.remember\"", "\"guided.remember\""] {
            let at = code.find(tag).expect(tag);
            assert!(code[..at].rfind("crate::db::write_ok(").is_some_and(|w| at - w < 40), "{tag}");
        }

        let reload = body(&code, "fn reload_setup(&self)");
        for f in ["refresh_streak()", "refresh_setup_label_chooser_subtitle()"] {
            assert!(reload.contains(f), "{f}");
        }
        let streak = body(&code, "pub fn refresh_streak(&self)");
        for f in ["load_timer_settings()", "load_breathing_settings()", "refresh_volume_rows()"] {
            assert!(streak.contains(f), "{f}");
        }
        // A volume that didn't save plays no preview: `save` gives no sound.
        let volume = body(&code, "let save = move |&(slot, mode)");
        assert!(volume.contains("setting_saved(") && volume.contains("return None"));
        // A preset's length is saved inside core's apply; the widgets
        // follow without a second write that could fail on its own.
        let apply = body(&code, "fn apply_config(&self");
        assert!(!apply.contains("set_countdown_target(") && !apply.contains("set_breathing_duration_secs("));
        assert!(!apply.contains("PresetTiming::"), "the reload shows the stored length");
        assert!(apply.contains("self.refresh_streak();"));
        assert!(body(&code, "fn set_countdown_target(&self, secs: u32) -> bool").contains("setting_saved("));
    }

    #[test]
    fn setup_summaries_follow_a_sync_and_leaving_a_page_stops_a_preview() {
        // #20 review: a sync pull and closing Preferences only call
        // refresh_streak, so the rows' summaries must follow there; the
        // Bells page holds Volume rows, so leaving it stops a preview.
        let code = production();
        assert!(body(&code, "    pub fn refresh_streak(&self)").contains("self.refresh_setup_summaries()"));
        let hidden = code.find("page.connect_hidden(").expect("the pages refresh when left");
        assert!(code[hidden..hidden + 300].contains("crate::sound::stop_preview()"));
    }

    #[test]
    fn a_failed_preset_apply_is_not_written_twice() {
        // K3 #193: core applies a preset whole or not at all, so a
        // failure leaves the setup from before; writing it again could
        // fail on the same lock and leave half of it.
        let code = production();
        let apply = body(&code, "fn apply_config(&self");
        assert_eq!(apply.matches("apply(db, ").count(), 1, "one apply");
        assert!(!apply.contains("snapshot_current_setup"), "nothing to put back");
        assert!(apply.contains("setting_saved("), "a failure says Save failed");
    }

    #[test]
    fn showing_a_saved_signal_mode_writes_nothing() {
        // #191 review: the reload after a failed save puts the saved
        // Sound / Vibration / Both back; that must not write again (it
        // would wait on the same lock a second time).
        let code = production();
        for setup in [
            "fn setup_starting_bell_signal_mode_toggle(&self)",
            "fn setup_end_bell_signal_mode_toggle(&self)",
            "fn setup_cues_signal_mode_toggle(&self)",
        ] {
            assert!(body(&code, setup).contains("!obj.imp().bells_loading.get()"), "{setup}");
        }
        let phases = &code[code.find("build_phase_signal_mode_toggle_widget(\n").unwrap()..];
        assert!(phases[..phases.find(");\n").unwrap()].contains("bells_loading.get()"), "phase toggles");
        for refresh in [
            "pub(crate) fn refresh_starting_bell_signal_mode_state(&self)",
            "pub(crate) fn refresh_end_bell_signal_mode_state(&self)",
        ] {
            let b = body(&code, refresh);
            assert!(b.contains("self.bells_loading.replace(true)") && b.contains("self.bells_loading.set(was)"), "{refresh}");
        }
    }

    /// A new day or a pull under a paused session put the Setup target
    /// over the session's time.
    #[test]
    fn setup_re_reads_only_while_no_session_runs() {
        let code = production();
        let streak = body(&code, "    pub fn refresh_streak(&self)");
        let guard = streak.find("if self.ui_state() != UiState::Idle {").expect("only the chip while a session runs");
        let load = streak.find("self.load_timer_settings()").unwrap();
        let session = &streak[guard..load];
        assert!(guard < load && session.contains("return;"));
        // The label row and summaries don't touch the time: they follow.
        assert!(session.contains("self.refresh_setup_label_chooser_subtitle();") && session.contains("self.refresh_setup_summaries();"));
    }

    #[test]
    fn an_auto_finished_session_is_saved_with_undo() {
        // #6: with Auto-Finish the countdown saves itself (note
        // included, same path as Save), then "Session saved" offers
        // Undo; a failed Undo says "Delete failed".
        let code = production();
        assert!(code.contains("CoreSessionEffect::AutoFinish { .. }"), "every tick and the track end look for it");
        assert_eq!(code.matches(".auto_finish_session()").count(), 3, "Timer and Guided ticks, Box Breath, the track end");
        let auto = body(&code, "fn auto_finish_session(&self)");
        assert!(auto.contains("self.on_save(true)"));
        assert!(auto.contains("self.notify_complete()"), "the notification still goes out");
        let save = &code[code.find("fn on_save(&self, auto: bool)").unwrap()..];
        let save = &save[..save.find("\n    fn ").unwrap()];
        let ok = save.find("reset_mode(mode, false)").unwrap();
        assert!(save.contains("gettext(\"Session saved\")"));
        assert!(save.contains("undo_session_save(&uuid, local_file_id)"));
        assert!(save.contains("crate::db::Failed::Delete"));
        // Away from the window no toast is left waiting.
        assert!(save[ok..].contains("if auto && win.is_active() {"), "only after the write landed");
        // Undo also puts the streak chip under it back, and an open Log follows at once.
        let undo = &save[save.find("undo_session_save(&uuid, local_file_id)").unwrap()..];
        assert!(undo[..undo.find("post_toast(&win, Some(\"session.saved\"), toast)").unwrap()].contains("data_changed(Refresh::ALL)"));
        // The switch: per mode, written like every other setting.
        let row = body(&code, "fn refresh_auto_finish_state(&self)");
        assert!(row.contains("auto_finish_from_db("));
        assert!(row.contains("auto_finish_row.set_sensitive(") && !row.contains("auto_finish_row.set_visible("), "greyed out, not hidden");
        assert!(code.contains("auto_finish_key_for_mode("));
    }

    #[test]
    fn the_add_button_says_that_it_saves() {
        assert_eq!(super::overtime_add_button_label(std::time::Duration::from_secs(180)), "+ 03:00 and Save");
        // Two lines where one doesn't fit.
        let code = production();
        let refresh = body(&code, "fn refresh_end_buttons(&self)");
        assert!(refresh.contains("set_wrap(true)") && refresh.contains("Justification::Center"));
    }

    #[test]
    fn an_auto_save_lets_the_end_bell_ring() {
        // #6 review: the save silenced everything, so the end bell
        // that Auto-Finish just rang was cut at once.
        let code = production();
        let save = &code[code.find("fn on_save(&self, auto: bool)").unwrap()..];
        let save = &save[..save.find("\n    fn ").unwrap()];
        let stop = save.find("self.stop_active_signals();").expect("a tapped Save silences what rings");
        assert!(save[..stop].ends_with("if !auto {\n            "), "only a tapped Save");
    }

    #[test]
    fn a_discard_question_closes_when_the_session_saves_itself() {
        // #6 review: Discard asked, the time ran out, the session saved
        // itself behind the dialog, and the later "Discard" did nothing.
        let code = production();
        let auto = body(&code, "fn auto_finish_session(&self)");
        assert!(auto.contains("self.discard_dialog.take()") && auto.contains(".close()"));
        // Asking silences nothing: Cancel leaves a ringing bell ringing.
        let discard = body(&code, "pub(crate) fn on_discard(&self)");
        let ask = discard.find("AlertDialog::builder()").unwrap();
        assert!(!discard[..ask].contains("stop_active_signals"));
        assert_eq!(discard.matches("self.stop_active_signals();").count() + discard.matches("obj.imp().stop_active_signals();").count(), 2, "on confirm, and without a note");
    }

    #[test]
    fn setup_rereads_bell_rows_when_shown_again() {
        // #84: back from a chooser where the bell's sound or pattern
        // was renamed or deleted, Setup shows the new name or "Missing".
        let code = production();
        let constructed = body(&code, "fn constructed(&self)");
        let shown = &constructed[constructed.find("self.bells_page.connect_shown(").expect("the Bells page re-reads when shown again")..];
        assert!(shown[..shown.find("});").unwrap()].contains("refresh_bell_row_names()"));
        let names = body(&code, "fn refresh_bell_row_names(&self)");
        for f in [
            "refresh_starting_bell_sound_subtitle()", "refresh_end_bell_sound_subtitle()",
            "refresh_starting_bell_pattern_subtitle()", "refresh_end_bell_pattern_subtitle()",
        ] {
            assert!(names.contains(f), "{f}");
        }
        // Box Breath's phase cues name a sound and a pattern too.
        assert!(names.contains("self.refresh_boxbreath_phase_subtitles(*phase);"));
    }

    #[test]
    fn manage_files_changes_follow_the_selected_file() {
        // #124: a selected file deleted or renamed in Manage Files, or
        // by a sync pull, is cleared or renamed on Setup, never during
        // a session or on Done (Save reads the selection).
        let code = production();
        let manage = body(&code, "self.manage_guided_files_btn.connect_clicked(");
        assert!(manage.contains("follow_selected_guided_file()"), "Manage Files changes");
        let streak = body(&code, "pub fn refresh_streak(&self)");
        assert!(streak.contains("follow_selected_guided_file()"), "after a sync pull");
        let follow = body(&code, "fn follow_selected_guided_file(&self)");
        let guard = follow.find("UiState::Idle").expect("only on Setup");
        assert!(guard < follow.find("guided_pick").unwrap());
        assert!(follow.contains("meditate_core::guided::follow_selection("), "the rule lives in core");
        assert!(follow.contains("refresh_guided_selected_row()") && follow.contains("refresh_hero_for_idle()"));
    }

    #[test]
    fn undo_of_a_deleted_guided_file_reselects_it() {
        // Undo brings the file back; Setup selects it again when
        // nothing else was picked meanwhile.
        let code = production();
        let follow = body(&code, "fn follow_selected_guided_file(&self)");
        assert!(follow.contains("guided_cleared_uuid"), "a cleared selection is remembered");
        assert!(follow.contains("library_pick("), "and restored as a library pick");
    }
}
