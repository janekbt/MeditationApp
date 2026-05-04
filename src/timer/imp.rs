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
/// countdown_btn/breathing_btn radio group in a single readable value
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
}


// ── GObject impl ──────────────────────────────────────────────────────────────

#[derive(Debug, Default, CompositeTemplate)]
#[template(resource = "/io/github/janekbt/Meditate/ui/timer_view.ui")]
pub struct TimerView {
    // Template children
    #[template_child] pub view_stack:            TemplateChild<gtk::Stack>,
    #[template_child] pub streak_label:          TemplateChild<gtk::Label>,
    #[template_child] pub countdown_btn:         TemplateChild<gtk::ToggleButton>,
    #[template_child] pub breathing_btn:         TemplateChild<gtk::ToggleButton>,
    #[template_child] pub big_time_label:         TemplateChild<gtk::Label>,
    #[template_child] pub countdown_inputs:       TemplateChild<gtk::Box>,
    #[template_child] pub stopwatch_mode_row:     TemplateChild<adw::SwitchRow>,
    #[template_child] pub presets_group:         TemplateChild<adw::PreferencesGroup>,
    #[template_child] pub save_settings_btn:     TemplateChild<gtk::Button>,
    #[template_child] pub manage_presets_btn:    TemplateChild<gtk::Button>,
    #[template_child] pub boxbreath_inputs:       TemplateChild<gtk::Box>,
    #[template_child] pub breathing_presets_box:  TemplateChild<gtk::FlowBox>,
    #[template_child] pub phase_tiles_grid:       TemplateChild<gtk::Grid>,
    #[template_child] pub breathing_duration_row: TemplateChild<adw::SpinRow>,
    #[template_child] pub start_btn:             TemplateChild<gtk::Button>,
    #[template_child] pub resume_btn:            TemplateChild<gtk::Button>,
    #[template_child] pub stop_from_pause_btn:   TemplateChild<gtk::Button>,
    #[template_child] pub session_group:          TemplateChild<adw::PreferencesGroup>,
    #[template_child] pub duration_row:            TemplateChild<adw::ActionRow>,
    #[template_child] pub duration_value_label:    TemplateChild<gtk::Label>,
    #[template_child] pub setup_label_enabled_row: TemplateChild<adw::ExpanderRow>,
    #[template_child] pub setup_label_chooser_row: TemplateChild<adw::ActionRow>,
    #[template_child] pub starting_bell_row:        TemplateChild<adw::ExpanderRow>,
    #[template_child] pub starting_bell_sound_row:  TemplateChild<adw::ActionRow>,
    #[template_child] pub preparation_time_row:     TemplateChild<adw::ExpanderRow>,
    #[template_child] pub preparation_time_secs_row:TemplateChild<adw::SpinRow>,
    #[template_child] pub interval_bells_enabled_row: TemplateChild<adw::ExpanderRow>,
    #[template_child] pub interval_bells_row:       TemplateChild<adw::ActionRow>,
    #[template_child] pub end_bell_row:            TemplateChild<adw::ExpanderRow>,
    #[template_child] pub end_bell_sound_row:      TemplateChild<adw::ActionRow>,
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
        self.breathing_session_mins.set(5);
        *self.breathing_preset_name.borrow_mut() = "4-4-4-4".to_string();
        self.setup_buttons();
        self.build_breathing_setup();
        self.configure_preparation_time_secs_row();

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

        // Mode toggle — both radios share a group, so exactly one
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
        self.breathing_btn.connect_toggled(mode_toggled);

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
        // Stubbed for P.4a — wiring the chooser-page push lands in
        // P.4c (Save) and P.4d (Manage). For now a toast acknowledges
        // the tap so the user knows the affordance is wired up.
        self.save_settings_btn.connect_clicked(glib::clone!(
            #[weak(rename_to = this)] obj,
            move |_| {
                if let Some(window) = this.root()
                    .and_downcast::<crate::window::MeditateWindow>()
                {
                    window.add_toast(adw::Toast::new(
                        &crate::i18n::gettext("Save Settings: chooser coming next commit"),
                    ));
                }
            },
        ));
        self.manage_presets_btn.connect_clicked(glib::clone!(
            #[weak(rename_to = this)] obj,
            move |_| {
                if let Some(window) = this.root()
                    .and_downcast::<crate::window::MeditateWindow>()
                {
                    window.add_toast(adw::Toast::new(
                        &crate::i18n::gettext("Manage Presets: chooser coming next commit"),
                    ));
                }
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
                window.push_sound_chooser(&app, current, move |uuid| {
                    app_for_pick.with_db_mut(|db| db.set_setting("end_bell_sound", &uuid));
                    crate::sound::preload_end_bell(&app_for_pick);
                    this_for_pick.imp().refresh_end_bell_sound_subtitle();
                });
            }
        ));

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
                window.push_sound_chooser(&app, current, move |uuid| {
                    app_for_pick.with_db_mut(|db| db.set_setting("starting_bell_sound", &uuid));
                    this_for_pick.imp().refresh_starting_bell_sound_subtitle();
                });
            }
        ));

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
}

// ── Mode switching ────────────────────────────────────────────────────────────

impl TimerView {
    pub(super) fn breathing_target_secs(&self) -> u64 {
        self.breath_target.get().as_secs()
    }


    /// Which mode the radio group currently reflects. Exactly one of
    /// the two buttons is active at any time (they share a group).
    /// Stopwatch-vs-countdown lives on `stopwatch_toggle_on` within
    /// the Timer branch.
    pub(crate) fn current_mode(&self) -> TimerMode {
        if self.breathing_btn.is_active() {
            TimerMode::Breathing
        } else {
            TimerMode::Timer
        }
    }

    /// Called when any of the three mode toggles gains active state.
    fn on_mode_switched(&self) {
        let mode = self.current_mode();

        // Input panels: only the active mode's inputs are visible.
        self.countdown_inputs.set_visible(mode == TimerMode::Timer);
        self.boxbreath_inputs.set_visible(mode == TimerMode::Breathing);
        // Starting Bell + Preparation Time + Interval Bells apply to
        // Timer mode only — Box Breathing has its own independent
        // rhythm and start-of-session cues, so the whole bell stack
        // goes away when breathing is active.
        self.starting_bell_row.set_visible(mode == TimerMode::Timer);
        self.interval_bells_enabled_row.set_visible(mode == TimerMode::Timer);
        // Duration row lives in the shared session_group but only
        // applies to Timer mode (Box Breath has its own
        // breathing_duration_row inside boxbreath_inputs).
        self.duration_row.set_visible(mode == TimerMode::Timer);
        // Visible-list contents are mode-strict (Timer presets only
        // appear in Timer mode, Box-Breath presets in Box Breath mode)
        // — rebuild on every switch.
        self.rebuild_starred_presets_list();

        // Each mode keeps its own last-used label. On switch, pull the
        // stored preference (or fall back to the mode-specific default —
        // "Box-breathing" for Breathing's first visit, "None" for the
        // other two) and apply it to the setup combo.
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
        self.countdown_inputs.set_visible(mode == TimerMode::Timer);
        self.boxbreath_inputs.set_visible(mode == TimerMode::Breathing);
        self.starting_bell_row.set_visible(mode == TimerMode::Timer);
        self.interval_bells_enabled_row.set_visible(mode == TimerMode::Timer);
        self.duration_row.set_visible(mode == TimerMode::Timer);
        self.countdown_btn.set_sensitive(true);
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
                // Breathing sessions are always sub-hour by construction
                // (duration spinner caps at 60 min), but use the same
                // hh:mm format for layout consistency.
                let m = self.breathing_session_mins.get();
                format!("{:02}:{:02}", m / 60, m % 60)
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
        // The Duration row + preset-list belong together: when
        // stopwatch mode flips on, the planned-duration concept
        // becomes inert — grey out the Duration row and the starred-
        // preset list (tapping them would change a target the
        // stopwatch session never reads).
        let duration_active = !self.stopwatch_toggle_on.get();
        self.duration_row.set_sensitive(duration_active);
        self.presets_group.set_sensitive(duration_active);
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
                // minutes up to the next full cycle so the session always
                // ends on an exhale/hold-out boundary.
                let raw = self.breathing_session_mins.get() as u64 * 60;
                let target = raw.div_ceil(cycle) * cycle;
                self.start_boot_time.set(Some(boot_time_now()));
                *self.breath_stopwatch.borrow_mut() =
                    Some(CoreStopwatch::started_at(std::time::Duration::ZERO));
                self.breath_target.set(std::time::Duration::from_secs(target));
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
                crate::sound::play_starting_sound(&app);
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
        self.timer_state.set(TimerState::Done);
        // Drop any prep state — the user stopped during prep, the
        // session's "elapsed" came from the prep stopwatch above.
        // Active bells stop firing the moment we leave Running, but
        // the schedule is also dropped here so a quick re-Start
        // rebuilds it from current settings.
        *self.prep_stopwatch.borrow_mut() = None;
        self.active_bells.borrow_mut().clear();

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
        };

        let data = SessionData {
            start_time,
            duration_secs: elapsed as i64,
            mode:          session_mode,
            label_id,
            note,
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

        if let Some(app) = self.get_app() {
            crate::sound::play_end_bell(&app);
            crate::vibration::trigger_if_enabled(&app);
            // Only send a system notification when the app isn't
            // focused — the in-app overtime UI already signals
            // completion.
            if !app.active_window().map(|w| w.is_active()).unwrap_or(false) {
                let n = gtk::gio::Notification::new("Meditation Complete");
                let target = self.countdown_target_secs.get();
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
            crate::sound::play_starting_sound(&app);
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
            crate::sound::play_end_bell(&app);
            crate::vibration::trigger_if_enabled(&app);
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
        self.preparation_time_row.set_enable_expansion(prep_on);
        self.preparation_time_row.set_expanded(prep_on);
        self.preparation_time_secs_row.set_value(prep_secs as f64);
        // Interval-bells master toggle + count subtitle.
        let (intervals_on, intervals_enabled_count) = intervals;
        self.interval_bells_enabled_row.set_enable_expansion(intervals_on);
        self.interval_bells_enabled_row.set_expanded(intervals_on);
        self.interval_bells_row.set_subtitle(&intervals_count_subtitle(intervals_enabled_count));
        self.bells_loading.set(false);

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
        // the sound-row subtitle here.
        self.refresh_end_bell_sound_subtitle();

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
        };
        let presets = self.get_app()
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

        let obj = self.obj();
        let mut tracked: Vec<(adw::ActionRow, String)> = Vec::with_capacity(presets.len());
        for p in presets {
            let row = adw::ActionRow::builder()
                .title(&p.name)
                .subtitle(&preset_subtitle(&p))
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

    /// Apply a saved preset to the live Setup state. Replays the
    /// stored config_json into every persistence point: per-mode
    /// settings, the interval-bell library (DELETEd then re-INSERTed
    /// from the snapshot), the breath-pattern Cells, and the
    /// countdown target. After the writes land we trigger one
    /// refresh_streak / load_breathing_settings round so the rows on
    /// screen converge to the new state without a stale frame.
    ///
    /// Mode-strict guard: preset's mode must match the current Setup
    /// view's mode. Per the design (2026-05-04, point B/C), tapping a
    /// preset must never side-effect the mode toggle — Box-Breath
    /// presets only show in Box-Breath mode and Timer presets only in
    /// Timer mode, so no cross-mode application happens in practice.
    /// Defensive `return` here covers the corner case where a sync
    /// race surfaces a stale row.
    fn on_preset_row_activated(&self, uuid: &str) {
        use crate::preset_config::{PresetConfig, PresetTiming};

        let Some(app) = self.get_app() else { return; };
        let preset = match app.with_db(|db| db.find_preset_by_uuid(uuid)) {
            Some(Ok(Some(p))) => p,
            _ => return,
        };
        let cfg = match PresetConfig::from_json(&preset.config_json) {
            Ok(c) => c,
            Err(_) => return,
        };

        // Reject cross-mode: shouldn't happen given the visible list
        // is mode-filtered, but never side-effect the mode toggle from
        // a tap.
        let current_mode = self.current_mode();
        let want_session_mode = match current_mode {
            TimerMode::Timer     => crate::db::SessionMode::Timer,
            TimerMode::Breathing => crate::db::SessionMode::BoxBreath,
        };
        if preset.mode != want_session_mode {
            return;
        }

        // Sync sound-uuid lookups: a preset synced from another
        // device may reference a bell sound that hasn't arrived yet
        // through the WebDAV layer. Refuse to apply and toast the
        // user — the sync spinner will eventually complete.
        let known_sound_uuids: std::collections::HashSet<String> = app
            .with_db(|db| db.list_bell_sounds())
            .and_then(|r| r.ok())
            .map(|sounds| sounds.into_iter().map(|s| s.uuid).collect())
            .unwrap_or_default();
        let mut needs_sound = Vec::<&str>::new();
        if cfg.starting_bell.enabled {
            needs_sound.push(&cfg.starting_bell.sound_uuid);
        }
        if cfg.end_bell.enabled {
            needs_sound.push(&cfg.end_bell.sound_uuid);
        }
        for b in &cfg.interval_bells.bells {
            needs_sound.push(&b.sound_uuid);
        }
        if needs_sound.iter().any(|u| !known_sound_uuids.contains(*u)) {
            self.toast(&crate::i18n::gettext(
                "Please wait until fully synced — not all bell sounds have arrived",
            ));
            return;
        }

        // ── Persist settings ─────────────────────────────────────
        let mode = current_mode;
        let label_active = cfg.label.enabled;
        let label_uuid_opt = cfg.label.uuid.clone();
        let stopwatch_active = matches!(
            cfg.timing, PresetTiming::Timer { stopwatch: true, .. }
        );
        app.with_db_mut(|db| {
            // Label rows
            let _ = db.set_setting(
                label_active_setting_key(mode),
                if label_active { "true" } else { "false" },
            );
            if let Some(luuid) = label_uuid_opt.as_ref() {
                let _ = db.set_setting(label_uuid_setting_key(mode), luuid);
            }
            // Bells (Timer-mode-only persistence; harmless to write
            // these in Box-Breath mode but the rows are hidden there)
            let _ = db.set_setting(
                "starting_bell_active",
                if cfg.starting_bell.enabled { "true" } else { "false" },
            );
            if !cfg.starting_bell.sound_uuid.is_empty() {
                let _ = db.set_setting("starting_bell_sound", &cfg.starting_bell.sound_uuid);
            }
            let _ = db.set_setting(
                "preparation_time_active",
                if cfg.starting_bell.prep_time_enabled { "true" } else { "false" },
            );
            let _ = db.set_setting(
                "preparation_time_secs",
                &cfg.starting_bell.prep_time_secs.to_string(),
            );
            let _ = db.set_setting(
                "interval_bells_active",
                if cfg.interval_bells.enabled { "true" } else { "false" },
            );
            let _ = db.set_setting(
                "end_bell_active",
                if cfg.end_bell.enabled { "true" } else { "false" },
            );
            if !cfg.end_bell.sound_uuid.is_empty() {
                let _ = db.set_setting("end_bell_sound", &cfg.end_bell.sound_uuid);
            }
            let _ = db.set_setting(
                "stopwatch_mode_active",
                if stopwatch_active { "true" } else { "false" },
            );
        });

        // ── Replace interval-bell library from the snapshot ─────
        // Destructive: every existing interval_bells row is dropped
        // (each emits one interval_bell_delete event for sync), then
        // the snapshot is inserted in order. Disabled-flag handling
        // is best-effort — `insert_interval_bell` always creates rows
        // enabled, so we follow up with a per-uuid set_enabled(false)
        // for snapshots that were saved as disabled. Acceptable cost
        // for what's a low-frequency action.
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

        // ── Apply mode-specific live state ──────────────────────
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
                duration_minutes,
            } => {
                self.breathing_pattern.set(BreathPattern {
                    in_secs:  inhale_secs,
                    hold_in:  hold_full_secs,
                    out_secs: exhale_secs,
                    hold_out: hold_empty_secs,
                });
                self.breathing_session_mins.set(duration_minutes);
                self.save_breathing_settings();
                self.breathing_populating.set(true);
                self.breathing_duration_row.set_value(duration_minutes as f64);
                self.breathing_populating.set(false);
                self.refresh_phase_tiles();
            }
        }

        // ── Refresh dependent UI ────────────────────────────────
        // refresh_streak picks up stopwatch / starting-bell / interval
        // counts from the freshly-written settings; refresh_setup_label
        // _chooser_subtitle picks up the label change. End-bell sound
        // subtitle and interval-bell count refresh on their own from
        // refresh_streak (which calls refresh_end_bell_sound_subtitle
        // and the count helpers).
        self.refresh_streak();

        self.toast(&crate::i18n::gettext("Preset '{name}' applied")
            .replace("{name}", &preset.name));
    }

    /// Push a toast onto the window's overlay. Quick helper since the
    /// preset apply path emits success / sync-pending toasts.
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
            new_bells.push(ActiveBell { sound: b.sound, schedule });
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
        let mut to_play: Vec<String> = Vec::new();
        let mut bells = self.active_bells.borrow_mut();
        for bell in bells.iter_mut() {
            match &mut bell.schedule {
                BellSchedule::Interval { base_min, jitter_pct, next_ring_secs } => {
                    if elapsed_secs >= *next_ring_secs {
                        to_play.push(bell.sound.clone());
                        let r = self.next_random_unit();
                        *next_ring_secs = meditate_core::format::next_interval_ring_secs(
                            *next_ring_secs, *base_min, *jitter_pct, r,
                        );
                    }
                }
                BellSchedule::Fixed { target_secs, fired } => {
                    if !*fired && elapsed_secs >= *target_secs {
                        to_play.push(bell.sound.clone());
                        *fired = true;
                    }
                }
            }
        }
        drop(bells);
        let Some(app) = self.get_app() else { return; };
        for sound_uuid in to_play {
            crate::sound::play_interval_sound(&sound_uuid, &app);
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
/// Mode-shaped: Timer ⇒ "{N} min" or "Stopwatch", Box Breath ⇒
/// "{i}-{h_full}-{e}-{h_empty} · {N} min". Renders the at-a-glance
/// summary the user needs without opening the preset.
fn preset_subtitle(p: &meditate_core::db::Preset) -> String {
    use crate::preset_config::{PresetConfig, PresetTiming};
    let cfg = match PresetConfig::from_json(&p.config_json) {
        Ok(c) => c,
        Err(_) => return String::new(),
    };
    match cfg.timing {
        PresetTiming::Timer { stopwatch: true, .. } =>
            crate::i18n::gettext("Stopwatch"),
        PresetTiming::Timer { stopwatch: false, duration_secs } => {
            let mins = duration_secs / 60;
            crate::i18n::gettext("{n} min").replace("{n}", &mins.to_string())
        }
        PresetTiming::BoxBreath {
            inhale_secs, hold_full_secs, exhale_secs, hold_empty_secs,
            duration_minutes,
        } => format!(
            "{}-{}-{}-{} · {}",
            inhale_secs, hold_full_secs, exhale_secs, hold_empty_secs,
            crate::i18n::gettext("{n} min").replace("{n}", &duration_minutes.to_string()),
        ),
    }
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
        app.with_db_mut(|db| {
            let _ = db.set_setting("breathing_in", &p.in_secs.to_string());
            let _ = db.set_setting("breathing_hold_in", &p.hold_in.to_string());
            let _ = db.set_setting("breathing_out", &p.out_secs.to_string());
            let _ = db.set_setting("breathing_hold_out", &p.hold_out.to_string());
            let _ = db.set_setting("breathing_session_mins", &mins.to_string());
            let _ = db.set_setting("breathing_preset", &preset);
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
    /// in Box Breath. Resolves through the seeded rows
    /// (`crate::db::DEFAULT_*_LABEL_UUID`).
    fn mode_default_label_uuid(&self, mode: TimerMode) -> &'static str {
        match mode {
            TimerMode::Timer => crate::db::DEFAULT_TIMER_LABEL_UUID,
            TimerMode::Breathing => crate::db::DEFAULT_BREATHING_LABEL_UUID,
        }
    }
}

fn label_active_setting_key(mode: TimerMode) -> &'static str {
    match mode {
        TimerMode::Timer => "label_active_timer",
        TimerMode::Breathing => "label_active_breathing",
    }
}

fn label_uuid_setting_key(mode: TimerMode) -> &'static str {
    match mode {
        TimerMode::Timer => "default_label_uuid_timer",
        TimerMode::Breathing => "default_label_uuid_breathing",
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
