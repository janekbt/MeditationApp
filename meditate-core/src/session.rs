//! Session state machine — the core of a meditation session's
//! lifecycle, owned by the shell and driven via `tick(now)` once a
//! second.
//!
//! Currently implements the Prep phase only. Subsequent stages will
//! extend `Session` with Running / Box-Breath / Pause-Resume /
//! Overtime / Bells / Stop semantics. See item 13 in
//! `CORE_MIGRATION.md`.
//!
//! Ownership pattern: the shell holds `Cell<Option<Session>>` —
//! `None` when no session is in flight (the setup view is showing),
//! `Some(session)` while a session is running. Each tick the shell
//! calls `session.tick(now)`, gets a `Vec<Effect>`, and dispatches
//! each effect through its native side-effect layer (gtk widget
//! updates / D-Bus haptics / gstreamer audio / Android Vibrator+
//! MediaPlayer / etc.). The gtk-side reactive plumbing collapses to
//! a thin pump.

use crate::bells::ActiveBell;
use crate::breath::{BreathPattern, Phase, PhaseInfo};
use crate::db::{BoxBreathPhaseId, SessionMode, SignalMode};
use crate::timer::Stopwatch;
use std::time::Duration;

/// What the running-page view should reflect right now. Strict
/// superset of `SessionPhase` plus `Idle` (no in-flight session)
/// and an explicit `Paused` that captures the orthogonal pause flag
/// (`Session::is_paused`) rather than the rarely-set `SessionPhase::
/// Paused` variant. The shell branches on this enum to drive the
/// view-stack page transitions and the running-page button row.
///
/// Resolved through `Session::ui_state()` (for in-flight sessions)
/// or the free fn `ui_state(session)` which handles `None → Idle`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UiState {
    /// No session in flight — the Setup view is showing.
    #[default]
    Idle,
    /// Prep-silence countdown before the starting bell.
    Preparing,
    /// Active session (countdown, stopwatch, or box-breath cycling).
    Running,
    /// Countdown crossed zero; the user hasn't tapped Finish or Add.
    Overtime,
    /// Session paused; tick produces no effects.
    Paused,
    /// Session ended (any terminal path) — Done view is showing.
    Done,
}

/// Resolve the running-page `UiState` from the shell's
/// `Option<Session>` slot. `None` → `Idle`; `Some(s)` delegates to
/// `s.ui_state()`.
pub fn ui_state(session: Option<&Session>) -> UiState {
    session.map(|s| s.ui_state()).unwrap_or(UiState::Idle)
}

/// Which lifecycle phase the in-flight session is currently in.
/// "Idle" is not a phase — it corresponds to no session being in
/// flight at all (the shell holds `Option<Session>::None`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionPhase {
    /// Silent preparation interval before the starting bell fires
    /// and the actual session timing begins. Timer-mode only;
    /// Box Breath skips this entirely. Stage 0a only implements
    /// this phase; the others arrive in subsequent stages.
    Prep,
    /// Active session: countdown, stopwatch, or box-breath cycling.
    Running,
    /// User-paused. Stopwatch and breath-cycle are frozen; tick
    /// produces no effects until resumed.
    Paused,
    /// Countdown Timer/Guided crossed zero but the user hasn't
    /// chosen Finish or Add yet. Hero is frozen at the planned
    /// duration; the Add-button label counts up as overtime
    /// accumulates; interval bells keep firing on the original
    /// session timeline.
    Overtime,
    /// Terminal phase reached via `stop` / `finish_overtime` /
    /// `add_overtime_and_finish`. Subsequent `tick()` calls return
    /// no effects; calling stop again is a no-op. The shell typically
    /// drops the `Session` right after seeing `Effect::EndSession`.
    Stopped,
}

/// All the configuration a fresh session needs. Built by the shell
/// from its setup-view state and handed to `Session::start_prep` or
/// `Session::start_running`.
#[derive(Debug, Clone)]
pub struct SessionSettings {
    pub mode: SessionMode,
    /// Some(secs) when prep silence is enabled; None otherwise.
    /// Only consulted by `start_prep` — `start_running` skips prep
    /// entirely.
    pub prep_secs: Option<u32>,
    /// Target session length in seconds. `Some(secs)` for countdown
    /// (Timer/Guided default; Box Breath when a fixed duration is
    /// set). `None` for stopwatch-only Timer / Box-Breath where no
    /// natural end exists. Drives the Running → Overtime transition
    /// and Box-Breath's cycle-aligned end. **Independent of
    /// `stopwatch_display`** — Guided keeps target=Some even when
    /// the user toggles count-up display.
    pub target_secs: Option<u32>,
    /// `true` → the running readout counts up (floor-rounded
    /// elapsed); `false` → counts down (ceiling-rounded remaining,
    /// when `target_secs` is set) or up (no target). Captures the
    /// gtk-shell `stopwatch_toggle_on` user setting at session
    /// start; same dimension for Guided (where the toggle changes
    /// display but not the underlying target).
    pub stopwatch_display: bool,
    /// `Some` only in Box Breath mode. The pattern drives phase
    /// boundary detection (for cue firing) and cycle-aligned session
    /// completion (Box Breath always ends on a full-cycle boundary
    /// rather than mid-phase, so the user finishes on a natural
    /// hold-out).
    pub breath_pattern: Option<BreathPattern>,
    /// Per-session bell schedule. Pre-built by the shell (typically
    /// from the `interval_bells` table filtered by enabled-flag and
    /// the active mode's stopwatch toggle); moves into Session at
    /// construction. Empty Vec is fine — means no interval bells.
    pub bells: Vec<ActiveBell>,
    /// Seed for the xorshift64 used by interval bells' jitter draws.
    /// Caller picks: production usually seeds from wall-clock nanos,
    /// tests pass a fixed value for determinism. Zero is replaced
    /// with 1 internally (xorshift64 outputs 0 forever from a 0
    /// seed).
    pub bell_rng_seed: u64,
    /// Per-mode signal-mode override the user picked on the Setup
    /// view's "Cues" ToggleGroup. AND'd with each bell / phase-cue's
    /// own `signal_mode` at fire time to compute the effective
    /// channel mix. Defaults to `Both` (no extra cap).
    pub signal_mode_override: SignalMode,
    /// Starting-bell cue, if the user enabled it. Fired at the
    /// prep→Running boundary (or immediately when there's no prep).
    /// `None` means starting bell is off — no FireStartingBell
    /// effect emitted.
    pub starting_bell: Option<crate::bells::BellCue>,
    /// End-bell cue, if the user enabled it. Fired at the natural
    /// end of the session — Running→Overtime for Timer/Guided
    /// countdown, EndBoxBreath for Box-Breath cycle-aligned target.
    /// Stopwatch-only sessions never reach those boundaries so the
    /// end bell stays silent without an explicit `None` check.
    pub end_bell: Option<crate::bells::BellCue>,
    /// Box-Breath per-phase cue config. Only `Some` for BoxBreath
    /// sessions; ignored otherwise.
    pub box_breath_cues: Option<crate::bells::BoxBreathCueConfig>,
}

/// Side effect the shell should dispatch this tick. The shell
/// matches on each variant and routes to its native layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Update the running-page big time display. `secs` is the
    /// already-rounded display value (ceiling for prep/countdown,
    /// floor for stopwatch / box-breath elapsed). Shell formats via
    /// `format::format_time`.
    UpdateDisplay { secs: u64 },
    /// Prep silence elapsed; transition to Running. Shell:
    /// constructs the countdown core for Timer mode, plays the
    /// starting bell, refreshes the running button row.
    EndPrep,
    /// Box Breath: a phase boundary was just crossed AND a cue is
    /// configured for the new phase (master toggle on + per-phase
    /// row enabled). Carries the resolved sound + vibration + the
    /// effective signal_mode (per-phase AND per-mode override
    /// already applied). The first tick of a box-breath session
    /// seeds `last_phase` silently; only subsequent transitions
    /// emit.
    FireBoxBreathCue {
        phase: BoxBreathPhaseId,
        sound_uuid: String,
        vibration_pattern_uuid: String,
        signal_mode: SignalMode,
    },
    /// Starting bell — fired at the prep→Running boundary, or
    /// immediately at session start when there's no prep. Carries
    /// the effective signal_mode (per-bell AND per-mode override).
    FireStartingBell {
        sound_uuid: String,
        vibration_pattern_uuid: String,
        signal_mode: SignalMode,
    },
    /// End bell — fired at Running→Overtime (Timer/Guided countdown
    /// crosses zero) or at the cycle-aligned end of a Box-Breath
    /// session. Stopwatch-only sessions never reach those
    /// boundaries so this effect simply never emits for them.
    FireEndBell {
        sound_uuid: String,
        vibration_pattern_uuid: String,
        signal_mode: SignalMode,
    },
    /// Box Breath: the cycle-aligned target was reached, ending the
    /// session naturally. Shell drops the session and shows the
    /// done view with this duration. Never fired in stopwatch-only
    /// box-breath mode (no target; user must press Stop).
    EndBoxBreath { duration_secs: u64 },
    /// Interval / fixed bell crossed its ring boundary this tick.
    /// `signal_mode` is the *effective* mode (per-bell AND per-mode
    /// override already applied by Session); the shell dispatches
    /// sound + vibration based on that variant directly — no extra
    /// gating needed.
    FireBell {
        sound_uuid: String,
        vibration_pattern_uuid: String,
        signal_mode: SignalMode,
    },
    /// Timer/Guided countdown crossed zero. Shell: morphs Pause →
    /// Finish, hides Stop, reveals the Add button, freezes the
    /// hero at the planned duration, fires the end bell, and sends
    /// a system notification when the app isn't focused. The
    /// session continues ticking in Overtime — interval bells keep
    /// firing on the original session timeline; the Add button's
    /// label counts up via subsequent `UpdateOvertimeLabel`.
    EnterOvertime,
    /// Overtime tick: how much past the target the session has
    /// gone. Shell renders the Add button as
    /// `<localized prefix> <MM:SS> ?`.
    UpdateOvertimeLabel { overtime: Duration },
    /// Session terminated via `stop` / `finish_overtime` /
    /// `add_overtime_and_finish`. `duration_secs` is what the saved
    /// row should record — prep elapsed when stopped during prep,
    /// running elapsed mid-session, target_secs for Finish-overtime,
    /// running+overtime elapsed for Add-overtime. Shell: stops
    /// playback, releases screen-awake, transitions to the Done UI
    /// with this duration.
    EndSession { duration_secs: u64 },
    /// Any in-flight bell sound / vibration / phase cue should be
    /// cut immediately. Emitted by Session lifecycle calls that
    /// represent a user-driven boundary — pause (the user wants
    /// everything to stop), stop (session ends), finish_overtime /
    /// add_overtime_and_finish (user acknowledges the session is
    /// over). Mechanics are shell-specific (gtk: MediaFile +
    /// feedbackd DBus cancel; Android: equivalent native calls)
    /// but the *rule* — that these lifecycle moments cut the user's
    /// in-flight feedback — lives in core.
    StopActiveSignals,
}

/// Which "fire channel" a `Fire*` effect belongs to — the abstract
/// slot the shell routes the sound through. The gtk shell maps to
/// its three thread-local `MediaFile` slots (Starting, End,
/// Interval); the Android shell maps to its `MediaPlayer` channels.
/// Box-Breath phase cues share the `Interval` channel because they
/// stack the same way interval bells do — two cues coinciding both
/// play through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FireChannel {
    Starting,
    End,
    Interval,
}

/// Resolved routing info for a `Fire*` effect: the channel slot, a
/// stable diag tag (logged via `meditate_core::diag::log`), and the
/// three carried fields (sound uuid, vibration pattern uuid,
/// effective signal_mode). The shell dispatches sound + vibration
/// based on `signal_mode.includes_*` — no extra gating needed
/// because Session already AND'd the per-mode override.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FireRoute<'a> {
    pub channel: FireChannel,
    pub log_tag: &'static str,
    pub sound_uuid: &'a str,
    pub vibration_pattern_uuid: &'a str,
    pub signal_mode: SignalMode,
}

impl Effect {
    /// Resolve a Fire effect to its routing info. `None` for every
    /// non-`Fire*` variant (UpdateDisplay, EndPrep, EndSession,
    /// EnterOvertime, UpdateOvertimeLabel, EndBoxBreath,
    /// StopActiveSignals — those are consumed at their tick
    /// callsites). The shell's dispatch loop becomes
    /// `for r in effects.iter().filter_map(Effect::fire_route) {
    /// play(r.channel, r.sound_uuid); ... }`.
    pub fn fire_route(&self) -> Option<FireRoute<'_>> {
        match self {
            Effect::FireBell { sound_uuid, vibration_pattern_uuid, signal_mode } => {
                Some(FireRoute {
                    channel: FireChannel::Interval,
                    log_tag: "fire_interval_bell",
                    sound_uuid,
                    vibration_pattern_uuid,
                    signal_mode: *signal_mode,
                })
            }
            Effect::FireStartingBell { sound_uuid, vibration_pattern_uuid, signal_mode } => {
                Some(FireRoute {
                    channel: FireChannel::Starting,
                    log_tag: "fire_starting_bell",
                    sound_uuid,
                    vibration_pattern_uuid,
                    signal_mode: *signal_mode,
                })
            }
            Effect::FireEndBell { sound_uuid, vibration_pattern_uuid, signal_mode } => {
                Some(FireRoute {
                    channel: FireChannel::End,
                    log_tag: "fire_end_bell",
                    sound_uuid,
                    vibration_pattern_uuid,
                    signal_mode: *signal_mode,
                })
            }
            Effect::FireBoxBreathCue {
                phase: _,
                sound_uuid,
                vibration_pattern_uuid,
                signal_mode,
            } => Some(FireRoute {
                channel: FireChannel::Interval,
                log_tag: "fire_box_breath_phase_cue",
                sound_uuid,
                vibration_pattern_uuid,
                signal_mode: *signal_mode,
            }),
            _ => None,
        }
    }
}

/// In-flight session. Created by `start_prep` (when prep silence is
/// enabled) or `start_running` (skip-prep path); driven by `tick(now)`
/// thereafter; transitioned to `SessionPhase::Stopped` by `stop` /
/// `finish_overtime` / `add_overtime_and_finish`, after which the
/// shell drops it.
#[derive(Debug)]
pub struct Session {
    settings: SessionSettings,
    phase: SessionPhase,
    /// Per-phase elapsed-tracker. Re-anchored on phase transitions
    /// (Prep→Running, eventually Running→Overtime) so the new
    /// phase's elapsed counts from that moment. Pause/resume mutate
    /// this in place so paused time doesn't leak into elapsed.
    phase_clock: Stopwatch,
    /// True when the session is paused (`pause()` was called more
    /// recently than `resume()`). Tick is a no-op while paused;
    /// `phase_clock` is in its `Paused` variant so elapsed reads
    /// stay frozen at the pause moment.
    is_paused: bool,
    /// Box-Breath phase boundary tracking. `None` until the first
    /// Running tick (which seeds it silently — the starting bell
    /// already fired). `Some(p)` thereafter; a tick observes
    /// `phase_at(elapsed) != p` and fires `FireBoxBreathCue`.
    /// Always `None` outside Box Breath mode.
    last_breath_phase: Option<Phase>,
    /// Per-session bell schedule. Mutated each tick: Interval
    /// bells reroll their `next_ring_secs`; Fixed bells flip their
    /// `fired` flag.
    bells: Vec<ActiveBell>,
    /// Xorshift64 state for bell-jitter rolls. Initialized from
    /// `SessionSettings::bell_rng_seed` (zero → 1 to avoid the
    /// degenerate xorshift seed).
    bell_rng_state: u64,
    /// Set when the session transitions to `Stopped` — the duration
    /// the shell should persist in the saved session row. `None`
    /// while in flight (Prep/Running/Overtime/Paused). Survives the
    /// `Stopped` phase until the shell drops the Session at reset
    /// time, so the Done view can render the saved length without
    /// the shell having to shadow the value itself.
    final_duration_secs: Option<u64>,
}

impl Session {
    /// Start a session in Prep phase. `prep_secs` must be set in
    /// `settings` — caller ensures (an `assert!` would be friendlier
    /// than a silent skip; debug-asserted here).
    pub fn start_prep(mut settings: SessionSettings, now: Duration) -> Self {
        debug_assert!(
            settings.prep_secs.is_some(),
            "start_prep called without prep_secs in settings",
        );
        let bells = std::mem::take(&mut settings.bells);
        let bell_rng_state = settings.bell_rng_seed.max(1);
        Self {
            settings,
            phase: SessionPhase::Prep,
            phase_clock: Stopwatch::started_at(now),
            is_paused: false,
            last_breath_phase: None,
            bells,
            bell_rng_state,
            final_duration_secs: None,
        }
    }

    /// Start a session directly in Running phase, skipping prep
    /// silence. Used when `prep_secs` is `None` or the user has
    /// `preparation_time_active = false`. The Running stopwatch
    /// anchors at `now`.
    pub fn start_running(mut settings: SessionSettings, now: Duration) -> Self {
        let bells = std::mem::take(&mut settings.bells);
        let bell_rng_state = settings.bell_rng_seed.max(1);
        Self {
            settings,
            phase: SessionPhase::Running,
            phase_clock: Stopwatch::started_at(now),
            is_paused: false,
            last_breath_phase: None,
            bells,
            bell_rng_state,
            final_duration_secs: None,
        }
    }

    /// Freeze the session's elapsed clock at `now`. Returns a
    /// `StopActiveSignals` effect on the leading edge so the shell
    /// cuts in-flight bells/vibration; subsequent `tick()` calls
    /// return no effects. The phase is preserved — resume
    /// continues from whatever phase pause was called from.
    /// Idempotent: calling pause on an already-paused session is a
    /// no-op (won't double-count paused time, returns empty Vec).
    pub fn pause(&mut self, now: Duration) -> Vec<Effect> {
        if self.is_paused {
            return Vec::new();
        }
        // Stopwatch::paused_at consumes self; swap-and-replace.
        let dummy = Stopwatch::started_at(Duration::ZERO);
        let s = std::mem::replace(&mut self.phase_clock, dummy);
        self.phase_clock = s.paused_at(now);
        self.is_paused = true;
        vec![Effect::StopActiveSignals]
    }

    /// Unfreeze the session. Subsequent ticks resume producing
    /// effects, with elapsed continuing from the pause moment (no
    /// leak from the pause window). Idempotent: calling resume on
    /// an already-running session is a no-op.
    pub fn resume(&mut self, now: Duration) {
        if !self.is_paused {
            return;
        }
        let dummy = Stopwatch::started_at(Duration::ZERO);
        let s = std::mem::replace(&mut self.phase_clock, dummy);
        self.phase_clock = s.resumed_at(now);
        self.is_paused = false;
    }

    pub fn is_paused(&self) -> bool {
        self.is_paused
    }

    /// User-initiated stop. Returns a single `EndSession` effect
    /// whose `duration_secs` is what the shell should persist as
    /// the saved row's duration:
    ///
    /// - From Prep: prep elapsed so far (floor seconds). Lets a
    ///   "stop during prep" still save a real session row instead
    ///   of dropping the time the user spent waiting.
    /// - From Running: running elapsed (floor seconds). Same as
    ///   `phase_clock.elapsed(now)` — Running's clock anchored at
    ///   the Prep→Running transition.
    /// - From Overtime: running+overtime elapsed. The phase_clock
    ///   was preserved across the Running→Overtime transition, so
    ///   `elapsed(now)` already includes both.
    ///
    /// Phase transitions to `Stopped`; further ticks return no
    /// effects. Idempotent: calling stop on an already-Stopped
    /// session returns an empty Vec without re-emitting the effect.
    /// Pause state is honoured — stop while paused returns the
    /// duration captured at the pause moment, since the phase_clock
    /// is frozen there.
    pub fn stop(&mut self, now: Duration) -> Vec<Effect> {
        if self.phase == SessionPhase::Stopped {
            return Vec::new();
        }
        let duration_secs = self.phase_clock.elapsed(now).as_secs();
        self.phase = SessionPhase::Stopped;
        self.final_duration_secs = Some(duration_secs);
        vec![
            Effect::StopActiveSignals,
            Effect::EndSession { duration_secs },
        ]
    }

    /// Overtime "Finish" — record exactly the planned countdown
    /// duration; the overtime delta is discarded. Returns
    /// `EndSession { duration_secs: target_secs }`. Only valid in
    /// Overtime phase; from any other phase returns an empty Vec
    /// without changing state, so the shell can call this
    /// unconditionally on the user's Finish tap without first
    /// checking phase.
    pub fn finish_overtime(&mut self) -> Vec<Effect> {
        if self.phase != SessionPhase::Overtime {
            return Vec::new();
        }
        let target_secs = self
            .settings
            .target_secs
            .expect("Overtime requires a target") as u64;
        self.phase = SessionPhase::Stopped;
        self.final_duration_secs = Some(target_secs);
        vec![
            Effect::StopActiveSignals,
            Effect::EndSession { duration_secs: target_secs },
        ]
    }

    /// Overtime "Add" — record the running+overtime elapsed
    /// (`phase_clock.elapsed(now)`), keeping the user's bonus
    /// minutes. Returns `EndSession { duration_secs: elapsed }`.
    /// Only valid in Overtime; otherwise empty Vec.
    pub fn add_overtime_and_finish(&mut self, now: Duration) -> Vec<Effect> {
        if self.phase != SessionPhase::Overtime {
            return Vec::new();
        }
        let duration_secs = self.phase_clock.elapsed(now).as_secs();
        self.phase = SessionPhase::Stopped;
        self.final_duration_secs = Some(duration_secs);
        vec![
            Effect::StopActiveSignals,
            Effect::EndSession { duration_secs },
        ]
    }

    /// Force the Running→Overtime transition externally. Used when
    /// the shell observes the countdown crossing zero before the
    /// next `tick(now)` would — e.g. the gtk shell's gst EOS
    /// callback in Guided mode, which fires when the audio file
    /// ends slightly before the probed duration. Returns a single
    /// `EnterOvertime` effect on the transition; idempotent — no-op
    /// (empty Vec) from any other phase, so callers can dispatch
    /// unconditionally.
    pub fn enter_overtime(&mut self) -> Vec<Effect> {
        if self.phase != SessionPhase::Running {
            return Vec::new();
        }
        self.phase = SessionPhase::Overtime;
        let mut effects = vec![Effect::EnterOvertime];
        if let Some(eff) = end_bell_effect(&self.settings) {
            effects.push(eff);
        }
        effects
    }

    /// Drive the session forward by one tick. Returns the effects
    /// the shell should dispatch this tick. Internal phase
    /// transitions (Prep→Running, Running→Overtime, etc.) emit
    /// transition effects (`EndPrep`, `EnterOvertime`, …) so the
    /// shell can fire bell sounds / morph buttons / etc. exactly
    /// at the boundary.
    pub fn tick(&mut self, now: Duration) -> Vec<Effect> {
        if self.is_paused {
            return Vec::new();
        }
        match self.phase {
            SessionPhase::Prep => self.tick_prep(now),
            SessionPhase::Running => self.tick_running(now),
            SessionPhase::Overtime => self.tick_overtime(now),
            // Pause is orthogonal to phase — handled by the
            // early-return above. SessionPhase::Paused is reserved
            // and currently unreached.
            SessionPhase::Paused => Vec::new(),
            // Terminal phase set by `stop` / `finish_overtime` /
            // `add_overtime_and_finish`. Any further ticks the shell
            // happens to dispatch before dropping the Session are
            // silent.
            SessionPhase::Stopped => Vec::new(),
        }
    }

    fn tick_prep(&mut self, now: Duration) -> Vec<Effect> {
        let target_secs = self.settings.prep_secs.unwrap_or(0) as u64;
        let target = Duration::from_secs(target_secs);
        let elapsed = self.phase_clock.elapsed(now);

        if elapsed >= target {
            // Crossed the prep boundary. Transition to Running and
            // restart the phase clock so Running's elapsed counts
            // from this exact moment, not from prep start.
            self.phase = SessionPhase::Running;
            self.phase_clock = Stopwatch::started_at(now);
            let mut effects = vec![Effect::EndPrep];
            // Starting bell fires on the same tick as the boundary
            // crossing — same instant the shell would have heard it
            // before the migration.
            if let Some(eff) = starting_bell_effect(&self.settings) {
                effects.push(eff);
            }
            return effects;
        }

        // Still in prep — emit the ceiling-rounded display value.
        // (k-1, k] remaining → display k, matching the existing
        // gtk shell's tick_prep behaviour.
        let remaining = target.saturating_sub(elapsed);
        let display = remaining.as_secs() + u64::from(remaining.subsec_nanos() > 0);
        vec![Effect::UpdateDisplay { secs: display }]
    }

    fn tick_running(&mut self, now: Duration) -> Vec<Effect> {
        let elapsed = self.phase_clock.elapsed(now);

        // Box Breath running has its own tick shape: phase-boundary
        // detection + cycle-aligned end + elapsed counter.
        if self.settings.breath_pattern.is_some() {
            return self.tick_box_breath(elapsed);
        }

        // Overtime transition is independent of display mode — Guided
        // with `stopwatch_display=true` still has a `target_secs` to
        // cross (the audio file's natural length).
        if let Some(target_secs) = self.settings.target_secs {
            let target = Duration::from_secs(target_secs as u64);
            if target.saturating_sub(elapsed).is_zero() {
                // Bells skip this tick — gtk's historical behaviour is
                // "transition first, bells fire on the next tick."
                self.phase = SessionPhase::Overtime;
                let mut effects = vec![Effect::EnterOvertime];
                if let Some(eff) = end_bell_effect(&self.settings) {
                    effects.push(eff);
                }
                return effects;
            }
        }

        let display_secs = display_secs_for_running(&self.settings, elapsed);
        let mut effects = vec![Effect::UpdateDisplay { secs: display_secs }];
        effects.extend(fire_due_bells(
            &mut self.bells,
            &mut self.bell_rng_state,
            self.settings.signal_mode_override,
            elapsed,
        ));
        effects
    }

    fn tick_overtime(&mut self, now: Duration) -> Vec<Effect> {
        // Overtime keeps the same phase_clock ticking that drove
        // Running — total elapsed = target + overtime. The shell
        // freezes the hero label at the planned duration; only the
        // Add button's label updates each tick. Interval bells
        // keep firing on the original session timeline because
        // their `next_ring_secs` was rolled against `elapsed` from
        // the Running phase, which continues to accumulate.
        let target_secs = self
            .settings
            .target_secs
            .expect("Overtime phase requires a target");
        let target = Duration::from_secs(target_secs as u64);
        let elapsed = self.phase_clock.elapsed(now);
        let overtime = elapsed.saturating_sub(target);
        let mut effects = vec![Effect::UpdateOvertimeLabel { overtime }];
        effects.extend(fire_due_bells(
            &mut self.bells,
            &mut self.bell_rng_state,
            self.settings.signal_mode_override,
            elapsed,
        ));
        effects
    }

    fn tick_box_breath(&mut self, elapsed: Duration) -> Vec<Effect> {
        let pattern = self
            .settings
            .breath_pattern
            .as_ref()
            .expect("tick_box_breath called without breath_pattern");
        let mut effects: Vec<Effect> = Vec::new();

        // Phase boundary detection. The first Running tick seeds
        // `last_breath_phase` silently — the starting bell already
        // fired at on_start so we don't double-cue Phase::In at t=0.
        let info = pattern.phase_at(elapsed);
        match self.last_breath_phase {
            None => {
                self.last_breath_phase = Some(info.phase);
            }
            Some(prev) if prev != info.phase => {
                self.last_breath_phase = Some(info.phase);
                let phase_id = phase_to_id(info.phase);
                if let Some(eff) = box_breath_cue_effect(&self.settings, phase_id) {
                    effects.push(eff);
                }
            }
            _ => {}
        }

        // Cycle-aligned end: the gtk shell rounds the chosen
        // duration UP to the next full cycle at start time, so
        // `elapsed >= target` already lands exactly on a cycle
        // boundary. Box Breath is finished when that crossing
        // happens. Stopwatch-only Box Breath (target_secs None)
        // never auto-ends.
        if let Some(target_secs) = self.settings.target_secs {
            let target = Duration::from_secs(target_secs as u64);
            if elapsed >= target {
                let duration_secs = elapsed.as_secs();
                self.phase = SessionPhase::Stopped;
                self.final_duration_secs = Some(duration_secs);
                effects.push(Effect::EndBoxBreath { duration_secs });
                if let Some(eff) = end_bell_effect(&self.settings) {
                    effects.push(eff);
                }
                return effects;
            }
        }

        // Counter readout for the running-page top strip. Floor-
        // rounded elapsed; the shell composes "elapsed / target" or
        // just "elapsed" in stopwatch-only mode using settings.
        effects.push(Effect::UpdateDisplay {
            secs: elapsed.as_secs(),
        });
        // Box Breath sessions can have interval bells too; same
        // tick semantics as Timer/Guided Running.
        effects.extend(fire_due_bells(
            &mut self.bells,
            &mut self.bell_rng_state,
            self.settings.signal_mode_override,
            elapsed,
        ));
        effects
    }

    /// Box-breath phase / progress at `now`. Used by the shell's
    /// frame-rate redraw of the dot + per-phase countdown overlay
    /// (independent of the 1 Hz tick that drives effects). Returns
    /// `None` when the session isn't a Box-Breath running session
    /// (no pattern, or not in Running phase).
    pub fn box_breath_phase_info(&self, now: Duration) -> Option<PhaseInfo> {
        if self.phase != SessionPhase::Running {
            return None;
        }
        let pattern = self.settings.breath_pattern.as_ref()?;
        let elapsed = self.phase_clock.elapsed(now);
        Some(pattern.phase_at(elapsed))
    }

    pub fn phase(&self) -> SessionPhase {
        self.phase
    }

    pub fn elapsed(&self, now: Duration) -> Duration {
        self.phase_clock.elapsed(now)
    }

    /// Saved duration the shell should record for this session.
    /// `Some(secs)` once the session has stopped (any terminal path —
    /// user Stop, Overtime Finish, Overtime Add, Box-Breath natural
    /// end); `None` while in flight (Prep / Running / Overtime /
    /// Paused). Survives until the shell drops the Session at reset
    /// time, so the Done view can render the saved length without a
    /// shell-side Cell shadow.
    pub fn final_duration_secs(&self) -> Option<u64> {
        self.final_duration_secs
    }

    /// What the shell's running-page view should reflect right now.
    /// Pure dispatch over `phase` + `is_paused`; the free fn
    /// `session::ui_state(session)` handles the `None` (= Idle) case
    /// for the shell's top-level `Option<Session>` slot.
    pub fn ui_state(&self) -> UiState {
        if self.is_paused {
            return UiState::Paused;
        }
        match self.phase {
            SessionPhase::Prep => UiState::Preparing,
            SessionPhase::Running => UiState::Running,
            SessionPhase::Overtime => UiState::Overtime,
            SessionPhase::Paused => UiState::Paused,
            SessionPhase::Stopped => UiState::Done,
        }
    }

    /// Mode of the in-flight session. Captured at start; never
    /// changes during a session.
    pub fn mode(&self) -> SessionMode {
        self.settings.mode
    }

    /// Ceiling-rounded remaining seconds of prep silence. `None`
    /// outside the Prep phase. Used by the gtk shell's hero-label
    /// readout when re-entering the running page mid-prep (e.g.,
    /// from the paused screen) — same `(k-1, k] → k` rule as the
    /// per-tick UpdateDisplay.
    pub fn prep_remaining_secs(&self, now: Duration) -> Option<u64> {
        if self.phase != SessionPhase::Prep {
            return None;
        }
        let target_secs = self.settings.prep_secs?;
        let target = Duration::from_secs(target_secs as u64);
        let remaining = target.saturating_sub(self.phase_clock.elapsed(now));
        Some(remaining.as_secs() + u64::from(remaining.subsec_nanos() > 0))
    }

    /// The whole-seconds value the running label should show right
    /// now, dispatching on phase + display mode:
    ///
    /// - Prep: ceiling-rounded prep remaining (same as
    ///   `prep_remaining_secs`).
    /// - Running: floor elapsed if `stopwatch_display` is true,
    ///   else ceiling-remaining (countdown) or floor elapsed
    ///   (no target).
    /// - Overtime: ceiling remaining clamps at 0 once past target;
    ///   the gtk shell keeps the hero frozen at the planned
    ///   duration via the same `target_secs` value, so callers
    ///   that respect that freeze typically don't invoke this
    ///   during Overtime.
    /// - Stopped: 0 (caller usually reads `final_duration_secs`
    ///   instead at that point).
    ///
    /// Centralises the rounding + dispatch the gtk shell used to
    /// split across `tick_running`, `elapsed_secs_for_mode`, and
    /// `current_display_secs`. Pure read — does not advance the
    /// session.
    pub fn display_secs(&self, now: Duration) -> u64 {
        if let Some(prep_remaining) = self.prep_remaining_secs(now) {
            return prep_remaining;
        }
        match self.phase {
            SessionPhase::Prep | SessionPhase::Stopped | SessionPhase::Paused => 0,
            SessionPhase::Running | SessionPhase::Overtime => {
                display_secs_for_running(&self.settings, self.phase_clock.elapsed(now))
            }
        }
    }
}

fn phase_to_id(phase: Phase) -> BoxBreathPhaseId {
    match phase {
        Phase::In => BoxBreathPhaseId::In,
        Phase::HoldIn => BoxBreathPhaseId::HoldIn,
        Phase::Out => BoxBreathPhaseId::Out,
        Phase::HoldOut => BoxBreathPhaseId::HoldOut,
    }
}

/// Display value (in whole seconds) for a Running session at the
/// given `elapsed`. Encapsulates the ceiling-vs-floor rounding
/// rule + the stopwatch-vs-countdown branch in one place; called
/// from both `tick_running` (for per-tick UpdateDisplay) and
/// `Session::display_secs` (for shell refreshes outside the tick
/// loop).
///
/// - `stopwatch_display` → floor-rounded elapsed regardless of
///   whether `target_secs` is set. Guided + stopwatch toggle on
///   is the motivating case: the file still has a probed duration
///   (target_secs=Some) but the user picked count-up display.
/// - Otherwise + `target_secs` set → ceiling-rounded remaining.
///   `(k-1, k]` remaining → display `k` so a fresh "10:00"
///   doesn't flicker to "09:59" on the first sub-1.0 s tick.
/// - Otherwise (no target, no stopwatch flag) → floor elapsed,
///   the natural stopwatch-only readout.
fn display_secs_for_running(settings: &SessionSettings, elapsed: Duration) -> u64 {
    if settings.stopwatch_display {
        return elapsed.as_secs();
    }
    match settings.target_secs {
        Some(target_secs) => {
            let target = Duration::from_secs(target_secs as u64);
            let remaining = target.saturating_sub(elapsed);
            remaining.as_secs() + u64::from(remaining.subsec_nanos() > 0)
        }
        None => elapsed.as_secs(),
    }
}

/// Build a `FireStartingBell` effect from the session's settings,
/// AND-combining the per-bell signal_mode with the per-mode
/// override. `None` when the user disabled the starting bell or
/// when the effective channel mix is empty (both gates off).
fn starting_bell_effect(settings: &SessionSettings) -> Option<Effect> {
    let cue = settings.starting_bell.as_ref()?;
    let signal_mode = crate::bells::effective_signal_mode(
        cue.signal_mode,
        settings.signal_mode_override,
    )?;
    Some(Effect::FireStartingBell {
        sound_uuid: cue.sound_uuid.clone(),
        vibration_pattern_uuid: cue.vibration_pattern_uuid.clone(),
        signal_mode,
    })
}

/// Mirror of `starting_bell_effect` for the end bell.
fn end_bell_effect(settings: &SessionSettings) -> Option<Effect> {
    let cue = settings.end_bell.as_ref()?;
    let signal_mode = crate::bells::effective_signal_mode(
        cue.signal_mode,
        settings.signal_mode_override,
    )?;
    Some(Effect::FireEndBell {
        sound_uuid: cue.sound_uuid.clone(),
        vibration_pattern_uuid: cue.vibration_pattern_uuid.clone(),
        signal_mode,
    })
}

/// Build a `FireBoxBreathCue` effect for `phase`, AND-combining
/// the per-phase signal_mode with the per-mode override. `None`
/// when the master toggle is off, the per-phase row is missing
/// / disabled, or the effective channel mix is empty.
fn box_breath_cue_effect(
    settings: &SessionSettings,
    phase: BoxBreathPhaseId,
) -> Option<Effect> {
    let cfg = settings.box_breath_cues.as_ref()?;
    let cue = cfg.cue_for(phase)?;
    let signal_mode = crate::bells::effective_signal_mode(
        cue.signal_mode,
        settings.signal_mode_override,
    )?;
    Some(Effect::FireBoxBreathCue {
        phase,
        sound_uuid: cue.sound_uuid.clone(),
        vibration_pattern_uuid: cue.vibration_pattern_uuid.clone(),
        signal_mode,
    })
}

/// Effects the shell should dispatch IMMEDIATELY after constructing
/// a Session via `start_running` (no-prep path). For the prep path,
/// the FireStartingBell is emitted as part of the tick that crosses
/// the prep boundary; the shell calls this helper only for sessions
/// that skip prep.
impl Session {
    pub fn start_signals(&self) -> Vec<Effect> {
        match self.phase {
            SessionPhase::Running => starting_bell_effect(&self.settings)
                .into_iter()
                .collect(),
            _ => Vec::new(),
        }
    }
}

/// Iterate over the session's bells, tick each against `elapsed`,
/// and emit `FireBell` effects for every bell that crosses its
/// ring boundary this tick. Mutates `bells` (Interval bells reroll
/// their next_ring_secs; Fixed bells flip their fired flag) and
/// the xorshift state (one draw per Interval-bell fire).
///
/// Free function rather than a method so the borrow checker sees
/// the disjoint `&mut Vec<ActiveBell>` and `&mut u64` borrows
/// without complaining about overlapping borrows of `&mut Session`.
fn fire_due_bells(
    bells: &mut [ActiveBell],
    rng_state: &mut u64,
    signal_mode_override: SignalMode,
    elapsed: Duration,
) -> Vec<Effect> {
    let mut effects = Vec::new();
    let elapsed_secs = elapsed.as_secs();
    for bell in bells.iter_mut() {
        let mut rng = || -> f64 {
            let (unit, next) = crate::rng::xorshift64(*rng_state);
            *rng_state = next;
            unit
        };
        if bell.tick(elapsed_secs, &mut rng) {
            if let Some(eff) = crate::bells::effective_signal_mode(
                bell.signal_mode,
                signal_mode_override,
            ) {
                effects.push(Effect::FireBell {
                    sound_uuid: bell.sound.clone(),
                    vibration_pattern_uuid: bell.vibration_pattern_uuid.clone(),
                    signal_mode: eff,
                });
            }
        }
    }
    effects
}

#[cfg(test)]
mod tests {
    use super::*;

    fn timer_settings_with_prep(prep_secs: u32) -> SessionSettings {
        SessionSettings {
            mode: SessionMode::Timer,
            prep_secs: Some(prep_secs),
            target_secs: Some(600),
            stopwatch_display: false,
            breath_pattern: None,
            bells: Vec::new(),
            bell_rng_seed: 1,
            signal_mode_override: SignalMode::Both,
            starting_bell: None,
            end_bell: None,
            box_breath_cues: None,
        }
    }

    fn timer_countdown_settings(target_secs: u32) -> SessionSettings {
        SessionSettings {
            mode: SessionMode::Timer,
            prep_secs: None,
            target_secs: Some(target_secs),
            stopwatch_display: false,
            breath_pattern: None,
            bells: Vec::new(),
            bell_rng_seed: 1,
            signal_mode_override: SignalMode::Both,
            starting_bell: None,
            end_bell: None,
            box_breath_cues: None,
        }
    }

    fn timer_prep_settings(prep_secs: u32, target_secs: u32) -> SessionSettings {
        let mut s = timer_countdown_settings(target_secs);
        s.prep_secs = Some(prep_secs);
        s
    }

    fn timer_stopwatch_settings() -> SessionSettings {
        SessionSettings {
            mode: SessionMode::Timer,
            prep_secs: None,
            target_secs: None,
            stopwatch_display: true,
            breath_pattern: None,
            bells: Vec::new(),
            bell_rng_seed: 1,
            signal_mode_override: SignalMode::Both,
            starting_bell: None,
            end_bell: None,
            box_breath_cues: None,
        }
    }

    fn box_breath_settings(target_secs: Option<u32>) -> SessionSettings {
        let cue = || crate::bells::BellCue {
            sound_uuid: "cue-sound".into(),
            vibration_pattern_uuid: "cue-pattern".into(),
            signal_mode: SignalMode::Both,
        };
        SessionSettings {
            mode: SessionMode::BoxBreath,
            prep_secs: None,
            target_secs,
            stopwatch_display: true,
            breath_pattern: Some(BreathPattern::box_breath()),
            bells: Vec::new(),
            bell_rng_seed: 1,
            signal_mode_override: SignalMode::Both,
            starting_bell: None,
            end_bell: None,
            // Wire all four phases so phase-boundary tests see
            // FireBoxBreathCue emissions; the gate-disabled cases
            // get tested via their own dedicated tests.
            box_breath_cues: Some(crate::bells::BoxBreathCueConfig {
                master_enabled: true,
                in_phase: Some(cue()),
                hold_in: Some(cue()),
                out_phase: Some(cue()),
                hold_out: Some(cue()),
            }),
        }
    }

    fn fixed_bell(target_secs: u64, sound: &str) -> ActiveBell {
        use crate::bells::BellSchedule;
        ActiveBell {
            sound: sound.to_string(),
            vibration_pattern_uuid: "pattern".to_string(),
            signal_mode: SignalMode::Sound,
            schedule: BellSchedule::Fixed { target_secs, fired: false },
        }
    }

    fn interval_bell(base_min: u32, sound: &str) -> ActiveBell {
        use crate::bells::BellSchedule;
        ActiveBell {
            sound: sound.to_string(),
            vibration_pattern_uuid: "pattern".to_string(),
            signal_mode: SignalMode::Sound,
            schedule: BellSchedule::Interval {
                base_min,
                jitter_pct: 0,
                next_ring_secs: (base_min as u64) * 60,
            },
        }
    }

    #[test]
    fn start_prep_puts_session_in_prep_phase() {
        let s = Session::start_prep(timer_settings_with_prep(30), Duration::from_secs(100));
        assert_eq!(s.phase(), SessionPhase::Prep);
        assert_eq!(s.mode(), SessionMode::Timer);
    }

    #[test]
    fn tick_during_prep_emits_ceiling_rounded_display() {
        let mut s = Session::start_prep(timer_settings_with_prep(30), Duration::from_secs(100));
        // 5 s in: 25 s remaining; display "25".
        let effects = s.tick(Duration::from_secs(105));
        assert_eq!(effects, vec![Effect::UpdateDisplay { secs: 25 }]);
        // Still in Prep (haven't reached target).
        assert_eq!(s.phase(), SessionPhase::Prep);
    }

    #[test]
    fn tick_in_prep_ceils_subsecond_remainder() {
        // 4.001 s in: 25.999 s remaining → display 26 (ceiling).
        // Same rule as the gtk shell's tick_prep — avoids skipping
        // a number on the very first tick that fires slightly past
        // the second boundary.
        let mut s = Session::start_prep(timer_settings_with_prep(30), Duration::from_secs(100));
        let effects = s.tick(Duration::from_millis(100_000 + 4_001));
        assert_eq!(effects, vec![Effect::UpdateDisplay { secs: 26 }]);
    }

    #[test]
    fn tick_at_prep_boundary_emits_end_prep_and_transitions() {
        let mut s = Session::start_prep(timer_settings_with_prep(30), Duration::from_secs(100));
        // Exactly at target: 30 s in → end_prep, transition to Running.
        let effects = s.tick(Duration::from_secs(130));
        assert_eq!(effects, vec![Effect::EndPrep]);
        assert_eq!(s.phase(), SessionPhase::Running);
    }

    #[test]
    fn tick_past_prep_boundary_still_emits_end_prep_once() {
        // App backgrounded for several seconds during prep; the
        // first tick after wake catches up. We fire EndPrep once
        // and transition; subsequent ticks are then in Running and
        // produce running-display updates (Stage 0b onward).
        let mut s = Session::start_prep(timer_settings_with_prep(30), Duration::from_secs(100));
        let effects = s.tick(Duration::from_secs(135));
        assert_eq!(effects, vec![Effect::EndPrep]);
        assert_eq!(s.phase(), SessionPhase::Running);

        // Subsequent tick is now in Running. EndPrep transitioned
        // at host-time 135 s; one tick later, 1 s of Running has
        // elapsed against the 600 s target → display 599.
        let effects = s.tick(Duration::from_secs(136));
        assert_eq!(effects, vec![Effect::UpdateDisplay { secs: 599 }]);
    }

    #[test]
    fn elapsed_during_prep_counts_from_start() {
        let s = Session::start_prep(timer_settings_with_prep(30), Duration::from_secs(100));
        assert_eq!(s.elapsed(Duration::from_secs(105)), Duration::from_secs(5));
        assert_eq!(s.elapsed(Duration::from_secs(125)), Duration::from_secs(25));
    }

    #[test]
    fn elapsed_after_prep_to_running_re_anchors_at_transition() {
        // After Prep→Running, elapsed should count from the
        // transition moment, not from the prep start. Subsequent
        // stages will read this for the running display; the
        // re-anchor matters because the user's "session length"
        // excludes the prep silence.
        let mut s = Session::start_prep(timer_settings_with_prep(30), Duration::from_secs(100));
        let _ = s.tick(Duration::from_secs(130)); // transitions
        // 5 s after the transition → elapsed = 5 (not 35).
        assert_eq!(s.elapsed(Duration::from_secs(135)), Duration::from_secs(5));
    }

    // ── Running phase ─────────────────────────────────────────────────

    #[test]
    fn start_running_skips_prep() {
        let s = Session::start_running(
            timer_countdown_settings(600),
            Duration::from_secs(100),
        );
        assert_eq!(s.phase(), SessionPhase::Running);
    }

    #[test]
    fn tick_running_countdown_emits_ceiling_remaining() {
        // 600 s target, 60 s elapsed → 540 s remaining → display 540.
        let mut s = Session::start_running(
            timer_countdown_settings(600),
            Duration::from_secs(100),
        );
        let effects = s.tick(Duration::from_secs(160));
        assert_eq!(effects, vec![Effect::UpdateDisplay { secs: 540 }]);
    }

    #[test]
    fn tick_running_countdown_ceils_subsecond_remainder() {
        // 0.001 s in: 599.999 s remaining → display 600 (ceiling).
        // Same anti-flicker trick as Prep — first-tick-past-zero
        // shouldn't drop the display digit immediately.
        let mut s = Session::start_running(
            timer_countdown_settings(600),
            Duration::from_secs(100),
        );
        let effects = s.tick(Duration::from_millis(100_000 + 1));
        assert_eq!(effects, vec![Effect::UpdateDisplay { secs: 600 }]);
    }

    #[test]
    fn tick_running_countdown_at_target_enters_overtime() {
        // Exactly at the target — countdown is "complete." Emit
        // EnterOvertime + transition phase. Shell will fire the end
        // bell + morph buttons + freeze hero in response.
        let mut s = Session::start_running(
            timer_countdown_settings(60),
            Duration::from_secs(100),
        );
        let effects = s.tick(Duration::from_secs(160));
        assert_eq!(effects, vec![Effect::EnterOvertime]);
        assert_eq!(s.phase(), SessionPhase::Overtime);
    }

    #[test]
    fn tick_running_countdown_past_target_enters_overtime_once() {
        // App backgrounded; first tick after wake catches up past
        // the target. We transition once and produce EnterOvertime
        // once; the next tick is in Overtime phase.
        let mut s = Session::start_running(
            timer_countdown_settings(60),
            Duration::from_secs(100),
        );
        let effects = s.tick(Duration::from_secs(200));
        assert_eq!(effects, vec![Effect::EnterOvertime]);
        assert_eq!(s.phase(), SessionPhase::Overtime);
    }

    #[test]
    fn tick_running_stopwatch_emits_floor_elapsed() {
        // Stopwatch-only: display reads as floor-rounded elapsed.
        // At 5.999 s → display 5; at 6.0 s → display 6.
        let mut s = Session::start_running(
            timer_stopwatch_settings(),
            Duration::from_secs(100),
        );
        let effects = s.tick(Duration::from_millis(100_000 + 5_999));
        assert_eq!(effects, vec![Effect::UpdateDisplay { secs: 5 }]);
        let effects = s.tick(Duration::from_millis(100_000 + 6_000));
        assert_eq!(effects, vec![Effect::UpdateDisplay { secs: 6 }]);
    }

    // ── Box Breath running ───────────────────────────────────────────

    #[test]
    fn first_box_breath_tick_seeds_phase_silently() {
        // First tick into a Box-Breath running session must NOT
        // emit FireBoxBreathCue — the starting bell already played
        // at on_start; emitting a Phase::In cue here would be a
        // duplicate.
        let mut s = Session::start_running(box_breath_settings(None), Duration::from_secs(100));
        let effects = s.tick(Duration::from_millis(100_500)); // 0.5 s in, still Phase::In
        assert!(
            !effects.iter().any(|e| matches!(e, Effect::FireBoxBreathCue { .. })),
            "first tick must not fire a phase cue: {:?}",
            effects
        );
        // Should produce a UpdateDisplay though.
        assert_eq!(effects, vec![Effect::UpdateDisplay { secs: 0 }]);
    }

    #[test]
    fn box_breath_tick_at_phase_boundary_fires_cue() {
        // Box pattern: In [0..4), HoldIn [4..8), Out [8..12), HoldOut [12..16).
        // Start at t=100, seed Phase::In on first tick at t=100.5.
        // At t=104 we're in HoldIn → should fire HoldIn cue.
        let mut s = Session::start_running(box_breath_settings(None), Duration::from_secs(100));
        let _ = s.tick(Duration::from_millis(100_500)); // seed In
        let effects = s.tick(Duration::from_secs(104));
        assert!(
            effects.iter().any(|e| matches!(
                e,
                Effect::FireBoxBreathCue { phase: BoxBreathPhaseId::HoldIn, .. }
            )),
            "expected HoldIn cue, got {:?}",
            effects
        );
    }

    #[test]
    fn box_breath_tick_within_same_phase_does_not_re_fire_cue() {
        // Two ticks both in Phase::In must produce only one
        // (silent) seed and no extra cue.
        let mut s = Session::start_running(box_breath_settings(None), Duration::from_secs(100));
        let _ = s.tick(Duration::from_millis(100_500)); // seed In
        let effects = s.tick(Duration::from_millis(101_500)); // still In
        assert!(
            !effects.iter().any(|e| matches!(e, Effect::FireBoxBreathCue { .. })),
            "no cue for same-phase tick"
        );
    }

    #[test]
    fn box_breath_tick_through_full_cycle_fires_each_boundary() {
        let mut s = Session::start_running(box_breath_settings(None), Duration::from_secs(100));
        let _ = s.tick(Duration::from_millis(100_500)); // seed In
        // HoldIn boundary at t=104
        assert!(s
            .tick(Duration::from_secs(104))
            .iter()
            .any(|e| matches!(e, Effect::FireBoxBreathCue { phase: BoxBreathPhaseId::HoldIn, .. })));
        // Out boundary at t=108
        assert!(s
            .tick(Duration::from_secs(108))
            .iter()
            .any(|e| matches!(e, Effect::FireBoxBreathCue { phase: BoxBreathPhaseId::Out, .. })));
        // HoldOut boundary at t=112
        assert!(s
            .tick(Duration::from_secs(112))
            .iter()
            .any(|e| matches!(e, Effect::FireBoxBreathCue { phase: BoxBreathPhaseId::HoldOut, .. })));
        // Wrap to In at t=116
        assert!(s
            .tick(Duration::from_secs(116))
            .iter()
            .any(|e| matches!(e, Effect::FireBoxBreathCue { phase: BoxBreathPhaseId::In, .. })));
    }

    #[test]
    fn box_breath_with_target_emits_end_at_target_boundary() {
        // 16 s target = exactly 1 cycle. End at t=116 (16 s after
        // session start at t=100).
        let mut s = Session::start_running(
            box_breath_settings(Some(16)),
            Duration::from_secs(100),
        );
        let _ = s.tick(Duration::from_millis(100_500)); // seed
        let effects = s.tick(Duration::from_secs(116));
        assert!(
            effects.contains(&Effect::EndBoxBreath { duration_secs: 16 }),
            "expected EndBoxBreath, got {:?}",
            effects
        );
        // EndBoxBreath is a terminal path — Session transitions to
        // Stopped and stashes the duration so the shell's Done view
        // can read it without a Cell shadow.
        assert_eq!(s.phase(), SessionPhase::Stopped);
        assert_eq!(s.final_duration_secs(), Some(16));
    }

    #[test]
    fn box_breath_stopwatch_never_emits_end() {
        // No target → never auto-ends regardless of elapsed.
        let mut s = Session::start_running(box_breath_settings(None), Duration::from_secs(100));
        let _ = s.tick(Duration::from_millis(100_500));
        let effects = s.tick(Duration::from_secs(100 + 16 * 100)); // many cycles
        assert!(
            !effects.iter().any(|e| matches!(e, Effect::EndBoxBreath { .. })),
            "stopwatch box-breath must never auto-end"
        );
    }

    #[test]
    fn box_breath_phase_info_reflects_current_phase() {
        // Frame-rate inspector — shell uses this for the dot's
        // perimeter position + the per-phase countdown overlay.
        let s = Session::start_running(box_breath_settings(None), Duration::from_secs(100));
        let info = s.box_breath_phase_info(Duration::from_secs(102)).unwrap();
        assert_eq!(info.phase, Phase::In);
        assert_eq!(info.elapsed_in_phase, Duration::from_secs(2));

        let info = s.box_breath_phase_info(Duration::from_secs(105)).unwrap();
        assert_eq!(info.phase, Phase::HoldIn);
    }

    #[test]
    fn box_breath_phase_info_is_none_for_non_box_breath_session() {
        let s = Session::start_running(timer_countdown_settings(600), Duration::from_secs(100));
        assert!(s.box_breath_phase_info(Duration::from_secs(105)).is_none());
    }

    // ── Overtime ─────────────────────────────────────────────────────

    #[test]
    fn overtime_tick_emits_overtime_label_with_delta_past_target() {
        let mut s = Session::start_running(
            timer_countdown_settings(60),
            Duration::from_secs(100),
        );
        // Cross zero — transition.
        let _ = s.tick(Duration::from_secs(160));
        assert_eq!(s.phase(), SessionPhase::Overtime);
        // 5 s of overtime have accumulated.
        let effects = s.tick(Duration::from_secs(165));
        assert_eq!(
            effects,
            vec![Effect::UpdateOvertimeLabel {
                overtime: Duration::from_secs(5)
            }]
        );
    }

    #[test]
    fn overtime_tick_at_transition_moment_reports_zero_overtime() {
        // The very first tick that fires AFTER EnterOvertime, at
        // host time = 60 s in (exactly target), reports overtime=0.
        // Subsequent ticks count up.
        let mut s = Session::start_running(
            timer_countdown_settings(60),
            Duration::from_secs(100),
        );
        let _ = s.tick(Duration::from_secs(160)); // EnterOvertime
        let effects = s.tick(Duration::from_secs(160));
        assert_eq!(
            effects,
            vec![Effect::UpdateOvertimeLabel {
                overtime: Duration::ZERO
            }]
        );
    }

    #[test]
    fn overtime_phase_clock_continues_uninterrupted_from_running() {
        // Running→Overtime transition keeps phase_clock ticking
        // (no reset); total elapsed = target + overtime accumulates
        // continuously. Interval bells in Stage 0f will rely on
        // this continuity.
        let mut s = Session::start_running(
            timer_countdown_settings(60),
            Duration::from_secs(100),
        );
        let _ = s.tick(Duration::from_secs(160)); // transition
        // Total elapsed at host time 200 s = 100 s (60 s target +
        // 40 s overtime).
        assert_eq!(
            s.elapsed(Duration::from_secs(200)),
            Duration::from_secs(100)
        );
    }

    #[test]
    fn pause_during_overtime_freezes_overtime_delta() {
        let mut s = Session::start_running(
            timer_countdown_settings(60),
            Duration::from_secs(100),
        );
        let _ = s.tick(Duration::from_secs(160)); // EnterOvertime
        let _ = s.tick(Duration::from_secs(170)); // 10 s of overtime
        let _ = s.pause(Duration::from_secs(170));
        // 100 s of host time pass while paused, then resume.
        s.resume(Duration::from_secs(270));
        // Active elapsed should still report 70 s total (60 target +
        // 10 overtime), and tick should report 10 s overtime.
        let effects = s.tick(Duration::from_secs(270));
        assert_eq!(
            effects,
            vec![Effect::UpdateOvertimeLabel {
                overtime: Duration::from_secs(10)
            }]
        );
    }

    // ── Bells ────────────────────────────────────────────────────────

    #[test]
    fn fixed_bell_fires_at_its_target_during_running() {
        let mut settings = timer_countdown_settings(600);
        settings.bells = vec![fixed_bell(60, "halftime")];
        let mut s = Session::start_running(settings, Duration::from_secs(100));
        // Tick 60 s in — the fixed bell fires.
        let effects = s.tick(Duration::from_secs(160));
        assert!(
            effects.iter().any(|e| matches!(
                e,
                Effect::FireBell { sound_uuid, .. } if sound_uuid == "halftime"
            )),
            "expected FireBell for 'halftime', got {:?}",
            effects
        );
    }

    #[test]
    fn fixed_bell_does_not_re_fire_after_initial() {
        let mut settings = timer_countdown_settings(600);
        settings.bells = vec![fixed_bell(60, "halftime")];
        let mut s = Session::start_running(settings, Duration::from_secs(100));
        let _ = s.tick(Duration::from_secs(160)); // fires
        let effects = s.tick(Duration::from_secs(180)); // shouldn't re-fire
        assert!(
            !effects.iter().any(|e| matches!(e, Effect::FireBell { .. })),
            "fixed bell must not re-fire: {:?}",
            effects
        );
    }

    #[test]
    fn interval_bell_fires_periodically_and_rerolls() {
        // Interval bell at 5 min (300 s), no jitter. Should fire at
        // 300 s, then reroll to 600 s, fire again, and so on.
        let mut settings = timer_stopwatch_settings();
        settings.bells = vec![interval_bell(5, "ding")];
        let mut s = Session::start_running(settings, Duration::from_secs(100));
        // Just before — no fire.
        let effects = s.tick(Duration::from_secs(399));
        assert!(!effects.iter().any(|e| matches!(e, Effect::FireBell { .. })));
        // At 300 s in — fires, reroll to 600.
        let effects = s.tick(Duration::from_secs(400));
        assert!(effects.iter().any(|e| matches!(e, Effect::FireBell { .. })));
        // At 599 s in — no fire yet.
        let effects = s.tick(Duration::from_secs(699));
        assert!(!effects.iter().any(|e| matches!(e, Effect::FireBell { .. })));
        // At 600 s in — fires.
        let effects = s.tick(Duration::from_secs(700));
        assert!(effects.iter().any(|e| matches!(e, Effect::FireBell { .. })));
    }

    #[test]
    fn bells_do_not_fire_during_prep() {
        // Bells are loaded at session construction but mustn't fire
        // during the prep silence — only when Running starts.
        let mut settings = timer_settings_with_prep(30);
        settings.bells = vec![fixed_bell(5, "early")]; // would fire if checked during prep
        let mut s = Session::start_prep(settings, Duration::from_secs(100));
        let effects = s.tick(Duration::from_secs(110));
        assert!(
            !effects.iter().any(|e| matches!(e, Effect::FireBell { .. })),
            "bells must not fire during prep: {:?}",
            effects
        );
    }

    #[test]
    fn bells_continue_firing_through_overtime() {
        // Interval bell at 5 min in a 60 s timer — the bell would
        // ring at the 5-min mark, well into overtime. The Running→
        // Overtime transition mustn't reset the bell schedule.
        let mut settings = timer_countdown_settings(60);
        settings.bells = vec![fixed_bell(300, "later")];
        let mut s = Session::start_running(settings, Duration::from_secs(100));
        let _ = s.tick(Duration::from_secs(160)); // EnterOvertime
        let _ = s.tick(Duration::from_secs(200)); // 100 s elapsed, still no fire
        let effects = s.tick(Duration::from_secs(400)); // 300 s elapsed
        assert!(
            effects.iter().any(|e| matches!(e, Effect::FireBell { .. })),
            "bell at 5-min mark should fire even through overtime"
        );
    }

    #[test]
    fn box_breath_session_fires_interval_bells() {
        let mut settings = box_breath_settings(None);
        settings.bells = vec![fixed_bell(60, "midbreath")];
        let mut s = Session::start_running(settings, Duration::from_secs(100));
        let _ = s.tick(Duration::from_millis(100_500)); // seed phase
        let effects = s.tick(Duration::from_secs(160));
        assert!(
            effects.iter().any(|e| matches!(e, Effect::FireBell { .. })),
            "Box-Breath running ticks must dispatch interval bells"
        );
    }

    #[test]
    fn bell_rng_state_advances_deterministically_from_seed() {
        // Same seed + same bell schedule → same fire times across
        // two independent sessions. Pin determinism.
        let mut a_settings = timer_stopwatch_settings();
        a_settings.bells = vec![interval_bell(5, "a")];
        a_settings.bell_rng_seed = 42;
        let mut a = Session::start_running(a_settings, Duration::from_secs(100));

        let mut b_settings = timer_stopwatch_settings();
        b_settings.bells = vec![interval_bell(5, "b")];
        b_settings.bell_rng_seed = 42;
        let mut b = Session::start_running(b_settings, Duration::from_secs(100));

        // Run both for 30 minutes; collect tick offsets where each fires.
        let mut a_fires: Vec<u64> = Vec::new();
        let mut b_fires: Vec<u64> = Vec::new();
        for offset in 1..=1800 {
            let now = Duration::from_secs(100 + offset);
            for e in a.tick(now) {
                if matches!(e, Effect::FireBell { .. }) {
                    a_fires.push(offset);
                }
            }
            for e in b.tick(now) {
                if matches!(e, Effect::FireBell { .. }) {
                    b_fires.push(offset);
                }
            }
        }
        assert_eq!(a_fires, b_fires, "same seed must give same fire schedule");
    }

    // ── Pause / Resume ───────────────────────────────────────────────

    #[test]
    fn pause_during_running_freezes_elapsed_clock() {
        let mut s = Session::start_running(
            timer_countdown_settings(600),
            Duration::from_secs(100),
        );
        let _ = s.tick(Duration::from_secs(110)); // 10 s in
        let _ = s.pause(Duration::from_secs(110));
        // 60 s of host time pass while paused.
        assert_eq!(s.elapsed(Duration::from_secs(170)), Duration::from_secs(10));
        assert!(s.is_paused());
    }

    #[test]
    fn pause_emits_stop_active_signals_on_leading_edge() {
        let mut s = Session::start_running(
            timer_countdown_settings(600),
            Duration::from_secs(100),
        );
        let first = s.pause(Duration::from_secs(110));
        assert_eq!(first, vec![Effect::StopActiveSignals]);
        // Idempotent re-pause emits nothing.
        let second = s.pause(Duration::from_secs(115));
        assert!(second.is_empty(), "redundant pause must emit nothing: {:?}", second);
    }

    #[test]
    fn resume_continues_from_pause_moment_no_paused_time_leaks() {
        let mut s = Session::start_running(
            timer_countdown_settings(600),
            Duration::from_secs(100),
        );
        let _ = s.pause(Duration::from_secs(110)); // 10 s in
        s.resume(Duration::from_secs(170)); // 60 s pause window
        // Active elapsed is still 10 s right after resume.
        assert_eq!(s.elapsed(Duration::from_secs(170)), Duration::from_secs(10));
        // 5 s of running time after resume → 15 s total active.
        assert_eq!(s.elapsed(Duration::from_secs(175)), Duration::from_secs(15));
    }

    #[test]
    fn tick_while_paused_emits_no_effects() {
        let mut s = Session::start_running(
            timer_countdown_settings(600),
            Duration::from_secs(100),
        );
        let _ = s.pause(Duration::from_secs(110));
        // Tick is a no-op while paused — caller should usually not
        // call it (the gtk shell stops the tick loop on pause), but
        // if it does, we don't update display or fire transitions.
        let effects = s.tick(Duration::from_secs(120));
        assert!(effects.is_empty(), "paused tick must produce nothing: {:?}", effects);
    }

    #[test]
    fn pause_during_prep_works() {
        let mut s = Session::start_prep(timer_settings_with_prep(30), Duration::from_secs(100));
        let _ = s.pause(Duration::from_secs(105)); // 5 s into prep
        s.resume(Duration::from_secs(200));
        // Active prep elapsed is 5 s — first tick after resume
        // should still report 25 s remaining (not the 95 s of host
        // time that elapsed).
        assert_eq!(
            s.tick(Duration::from_secs(200)),
            vec![Effect::UpdateDisplay { secs: 25 }]
        );
        assert_eq!(s.phase(), SessionPhase::Prep);
    }

    #[test]
    fn pause_during_box_breath_freezes_phase() {
        let mut s = Session::start_running(box_breath_settings(None), Duration::from_secs(100));
        let _ = s.tick(Duration::from_millis(100_500)); // seed In
        // 6 s in → still HoldIn (phase was at t=4 boundary).
        let _ = s.tick(Duration::from_secs(106));
        let _ = s.pause(Duration::from_secs(106));
        // 1000 s of host time pass; resume.
        s.resume(Duration::from_secs(1106));
        // Active elapsed is still 6 s → phase is HoldIn.
        let info = s.box_breath_phase_info(Duration::from_secs(1106)).unwrap();
        assert_eq!(info.phase, Phase::HoldIn);
    }

    #[test]
    fn pause_is_idempotent_and_resume_is_idempotent() {
        let mut s = Session::start_running(
            timer_stopwatch_settings(),
            Duration::from_secs(100),
        );
        // 5 s elapsed, pause twice (second is a no-op).
        let _ = s.pause(Duration::from_secs(105));
        let _ = s.pause(Duration::from_secs(150)); // would over-pause if not idempotent
        // 100 s of host time, then resume twice (second no-op).
        s.resume(Duration::from_secs(205));
        s.resume(Duration::from_secs(210)); // idempotent
        // Active elapsed should be 5 s + (210-205) = 10 s.
        assert_eq!(s.elapsed(Duration::from_secs(210)), Duration::from_secs(10));
    }

    #[test]
    fn prep_to_running_handoff_continues_ticking_correctly() {
        // End-to-end: Prep silences for 30 s, transitions to Running,
        // then Running ticks against a 600 s countdown target. The
        // re-anchor at Prep→Running matters — Running's elapsed
        // counts from the transition moment, not from prep start.
        let mut s = Session::start_prep(
            timer_settings_with_prep(30), // prep=30s, target=600s
            Duration::from_secs(100),
        );
        // Prep tick at 5 s in: display 25 remaining.
        assert_eq!(
            s.tick(Duration::from_secs(105)),
            vec![Effect::UpdateDisplay { secs: 25 }]
        );
        // Cross prep target: EndPrep + transition.
        assert_eq!(
            s.tick(Duration::from_secs(130)),
            vec![Effect::EndPrep]
        );
        // 60 s into Running phase (which started at 130 s of host
        // time): 540 s remaining of the 600 s target.
        assert_eq!(
            s.tick(Duration::from_secs(190)),
            vec![Effect::UpdateDisplay { secs: 540 }]
        );
    }

    // ── prep_remaining_secs ──────────────────────────────────────────

    #[test]
    fn prep_remaining_secs_during_prep_returns_ceiling_remaining() {
        let s = Session::start_prep(
            timer_settings_with_prep(30),
            Duration::from_secs(100),
        );
        // 12 s into prep: 18 s remaining.
        assert_eq!(s.prep_remaining_secs(Duration::from_secs(112)), Some(18));
    }

    #[test]
    fn prep_remaining_secs_ceils_subsecond_remainder() {
        let s = Session::start_prep(
            timer_settings_with_prep(30),
            Duration::from_secs(100),
        );
        // 12.4 s elapsed → 17.6 s remaining → ceil to 18.
        assert_eq!(s.prep_remaining_secs(Duration::from_millis(112_400)), Some(18));
    }

    #[test]
    fn prep_remaining_secs_outside_prep_is_none() {
        let s = Session::start_running(
            timer_countdown_settings(600),
            Duration::from_secs(100),
        );
        assert_eq!(s.prep_remaining_secs(Duration::from_secs(110)), None);
    }

    // ── display_secs (unified hero readout) ──────────────────────────

    #[test]
    fn display_secs_during_prep_matches_prep_remaining() {
        let s = Session::start_prep(
            timer_settings_with_prep(30),
            Duration::from_secs(100),
        );
        // 12 s in → 18 s remaining (same as prep_remaining_secs).
        assert_eq!(s.display_secs(Duration::from_secs(112)), 18);
    }

    #[test]
    fn display_secs_running_countdown_is_ceiling_remaining() {
        let s = Session::start_running(
            timer_countdown_settings(600),
            Duration::from_secs(100),
        );
        // 100 s elapsed → 500 s remaining.
        assert_eq!(s.display_secs(Duration::from_secs(200)), 500);
        // Subsecond → ceiling up.
        assert_eq!(s.display_secs(Duration::from_millis(200_500)), 500);
    }

    #[test]
    fn display_secs_running_stopwatch_is_floor_elapsed() {
        let s = Session::start_running(
            timer_stopwatch_settings(),
            Duration::from_secs(100),
        );
        assert_eq!(s.display_secs(Duration::from_secs(245)), 145);
        // Subsecond → floor.
        assert_eq!(s.display_secs(Duration::from_millis(245_700)), 145);
    }

    #[test]
    fn display_secs_guided_stopwatch_shows_elapsed_even_with_target() {
        // Guided + stopwatch toggle: target_secs is Some (file's
        // probed duration) but display is count-up.
        let mut settings = timer_countdown_settings(600);
        settings.mode = SessionMode::Guided;
        settings.stopwatch_display = true;
        let s = Session::start_running(settings, Duration::from_secs(100));
        assert_eq!(s.display_secs(Duration::from_secs(245)), 145);
    }

    // ── Stop / Finish-overtime / Add-overtime ────────────────────────

    #[test]
    fn stop_during_prep_returns_prep_elapsed() {
        let mut s = Session::start_prep(
            timer_settings_with_prep(30),
            Duration::from_secs(100),
        );
        // 12 s into prep: stop returns 12 s.
        let effects = s.stop(Duration::from_secs(112));
        assert_eq!(
            effects,
            vec![
                Effect::StopActiveSignals,
                Effect::EndSession { duration_secs: 12 },
            ]
        );
        assert_eq!(s.phase(), SessionPhase::Stopped);
    }

    #[test]
    fn stop_during_running_returns_running_elapsed() {
        let mut s = Session::start_running(
            timer_countdown_settings(600),
            Duration::from_secs(100),
        );
        let effects = s.stop(Duration::from_secs(245));
        assert_eq!(
            effects,
            vec![
                Effect::StopActiveSignals,
                Effect::EndSession { duration_secs: 145 },
            ]
        );
        assert_eq!(s.phase(), SessionPhase::Stopped);
    }

    #[test]
    fn stop_during_overtime_returns_running_plus_overtime_elapsed() {
        let mut s = Session::start_running(
            timer_countdown_settings(60),
            Duration::from_secs(100),
        );
        // Cross zero → Overtime.
        let _ = s.tick(Duration::from_secs(160));
        assert_eq!(s.phase(), SessionPhase::Overtime);
        // Stop 30 s into overtime: total elapsed 90 s.
        let effects = s.stop(Duration::from_secs(190));
        assert_eq!(
            effects,
            vec![
                Effect::StopActiveSignals,
                Effect::EndSession { duration_secs: 90 },
            ]
        );
        assert_eq!(s.phase(), SessionPhase::Stopped);
    }

    #[test]
    fn stop_while_paused_returns_elapsed_at_pause() {
        let mut s = Session::start_running(
            timer_countdown_settings(600),
            Duration::from_secs(100),
        );
        let _ = s.pause(Duration::from_secs(150)); // 50 s elapsed
        // Stop later — duration should be 50, not 50+anything.
        let effects = s.stop(Duration::from_secs(300));
        assert_eq!(
            effects,
            vec![
                Effect::StopActiveSignals,
                Effect::EndSession { duration_secs: 50 },
            ]
        );
        assert_eq!(s.phase(), SessionPhase::Stopped);
    }

    #[test]
    fn stop_is_idempotent() {
        let mut s = Session::start_running(
            timer_countdown_settings(600),
            Duration::from_secs(100),
        );
        let first = s.stop(Duration::from_secs(120));
        assert_eq!(
            first,
            vec![
                Effect::StopActiveSignals,
                Effect::EndSession { duration_secs: 20 },
            ]
        );
        let second = s.stop(Duration::from_secs(140));
        assert!(
            second.is_empty(),
            "second stop must be a no-op, got {:?}",
            second
        );
    }

    #[test]
    fn stop_stashes_final_duration_for_done_view() {
        let mut s = Session::start_running(
            timer_countdown_settings(600),
            Duration::from_secs(100),
        );
        assert_eq!(s.final_duration_secs(), None, "in-flight before stop");
        let _ = s.stop(Duration::from_secs(123));
        assert_eq!(
            s.final_duration_secs(),
            Some(23),
            "stash mirrors EndSession.duration_secs",
        );
    }

    #[test]
    fn finish_overtime_stashes_target_secs() {
        let mut s = Session::start_running(
            timer_countdown_settings(600),
            Duration::from_secs(0),
        );
        let _ = s.enter_overtime();
        let _ = s.finish_overtime();
        assert_eq!(
            s.final_duration_secs(),
            Some(600),
            "Finish-overtime pins the planned target",
        );
    }

    #[test]
    fn add_overtime_and_finish_stashes_running_plus_overtime() {
        let mut s = Session::start_running(
            timer_countdown_settings(600),
            Duration::from_secs(0),
        );
        let _ = s.tick(Duration::from_secs(600));
        let _ = s.add_overtime_and_finish(Duration::from_secs(630));
        assert_eq!(s.final_duration_secs(), Some(630));
    }

    #[test]
    fn ui_state_reflects_phase_and_pause() {
        use crate::session::ui_state;
        let mut s = Session::start_running(
            timer_countdown_settings(600),
            Duration::from_secs(0),
        );
        assert_eq!(ui_state(Some(&s)), UiState::Running);
        let _ = s.pause(Duration::from_secs(5));
        assert_eq!(ui_state(Some(&s)), UiState::Paused);
        s.resume(Duration::from_secs(10));
        let _ = s.enter_overtime();
        assert_eq!(ui_state(Some(&s)), UiState::Overtime);
        let _ = s.finish_overtime();
        assert_eq!(ui_state(Some(&s)), UiState::Done);
    }

    #[test]
    fn fire_route_dispatches_each_fire_variant_to_its_channel() {
        let starting = Effect::FireStartingBell {
            sound_uuid: "s".into(),
            vibration_pattern_uuid: "v".into(),
            signal_mode: SignalMode::Both,
        };
        let r = starting.fire_route().expect("FireStartingBell routes");
        assert_eq!(r.channel, FireChannel::Starting);
        assert_eq!(r.log_tag, "fire_starting_bell");

        let interval = Effect::FireBell {
            sound_uuid: "s".into(),
            vibration_pattern_uuid: "v".into(),
            signal_mode: SignalMode::Sound,
        };
        assert_eq!(interval.fire_route().unwrap().channel, FireChannel::Interval);

        let end = Effect::FireEndBell {
            sound_uuid: "s".into(),
            vibration_pattern_uuid: "v".into(),
            signal_mode: SignalMode::Sound,
        };
        assert_eq!(end.fire_route().unwrap().channel, FireChannel::End);

        let phase = Effect::FireBoxBreathCue {
            phase: BoxBreathPhaseId::In,
            sound_uuid: "s".into(),
            vibration_pattern_uuid: "v".into(),
            signal_mode: SignalMode::Both,
        };
        assert_eq!(phase.fire_route().unwrap().channel, FireChannel::Interval);
    }

    #[test]
    fn fire_route_returns_none_for_non_fire_effects() {
        assert!(Effect::EndPrep.fire_route().is_none());
        assert!(Effect::UpdateDisplay { secs: 5 }.fire_route().is_none());
        assert!(Effect::StopActiveSignals.fire_route().is_none());
        assert!(Effect::EndSession { duration_secs: 10 }.fire_route().is_none());
        assert!(Effect::EnterOvertime.fire_route().is_none());
    }

    #[test]
    fn ui_state_idle_when_session_is_none() {
        use crate::session::ui_state;
        assert_eq!(ui_state(None), UiState::Idle);
    }

    #[test]
    fn ui_state_preparing_during_prep() {
        use crate::session::ui_state;
        let s = Session::start_prep(
            timer_prep_settings(10, 600),
            Duration::from_secs(0),
        );
        assert_eq!(ui_state(Some(&s)), UiState::Preparing);
    }

    #[test]
    fn ticks_after_stop_emit_no_effects() {
        let mut s = Session::start_running(
            timer_countdown_settings(600),
            Duration::from_secs(100),
        );
        let _ = s.stop(Duration::from_secs(120));
        let effects = s.tick(Duration::from_secs(125));
        assert!(effects.is_empty(), "tick after stop must be silent: {:?}", effects);
    }

    #[test]
    fn finish_overtime_returns_target_secs() {
        let mut s = Session::start_running(
            timer_countdown_settings(60),
            Duration::from_secs(100),
        );
        // Cross zero into Overtime, accumulate 30 s of overtime.
        let _ = s.tick(Duration::from_secs(160));
        let _ = s.tick(Duration::from_secs(190));
        let effects = s.finish_overtime();
        // Overtime discarded — saved duration is the planned target.
        assert_eq!(
            effects,
            vec![
                Effect::StopActiveSignals,
                Effect::EndSession { duration_secs: 60 },
            ]
        );
        assert_eq!(s.phase(), SessionPhase::Stopped);
    }

    #[test]
    fn finish_overtime_outside_overtime_is_a_noop() {
        let mut s = Session::start_running(
            timer_countdown_settings(600),
            Duration::from_secs(100),
        );
        // Still Running — no Overtime crossed yet.
        let effects = s.finish_overtime();
        assert!(effects.is_empty());
        assert_eq!(s.phase(), SessionPhase::Running);
    }

    #[test]
    fn add_overtime_and_finish_returns_running_plus_overtime() {
        let mut s = Session::start_running(
            timer_countdown_settings(60),
            Duration::from_secs(100),
        );
        let _ = s.tick(Duration::from_secs(160)); // EnterOvertime
        let _ = s.tick(Duration::from_secs(195)); // 35 s of overtime
        let effects = s.add_overtime_and_finish(Duration::from_secs(195));
        // 60 s target + 35 s overtime = 95 s saved.
        assert_eq!(
            effects,
            vec![
                Effect::StopActiveSignals,
                Effect::EndSession { duration_secs: 95 },
            ]
        );
        assert_eq!(s.phase(), SessionPhase::Stopped);
    }

    #[test]
    fn add_overtime_outside_overtime_is_a_noop() {
        let mut s = Session::start_running(
            timer_countdown_settings(600),
            Duration::from_secs(100),
        );
        let effects = s.add_overtime_and_finish(Duration::from_secs(200));
        assert!(effects.is_empty());
        assert_eq!(s.phase(), SessionPhase::Running);
    }

    // ── enter_overtime (external force-transition) ───────────────────

    #[test]
    fn enter_overtime_from_running_transitions_and_emits_effect() {
        let mut s = Session::start_running(
            timer_countdown_settings(600),
            Duration::from_secs(100),
        );
        let effects = s.enter_overtime();
        assert_eq!(effects, vec![Effect::EnterOvertime]);
        assert_eq!(s.phase(), SessionPhase::Overtime);
    }

    #[test]
    fn enter_overtime_is_idempotent_in_overtime() {
        let mut s = Session::start_running(
            timer_countdown_settings(60),
            Duration::from_secs(100),
        );
        let _ = s.tick(Duration::from_secs(160)); // transition Running → Overtime
        assert_eq!(s.phase(), SessionPhase::Overtime);
        let effects = s.enter_overtime();
        assert!(effects.is_empty());
        assert_eq!(s.phase(), SessionPhase::Overtime);
    }

    #[test]
    fn enter_overtime_during_prep_is_a_noop() {
        let mut s = Session::start_prep(
            timer_settings_with_prep(30),
            Duration::from_secs(100),
        );
        let effects = s.enter_overtime();
        assert!(effects.is_empty());
        assert_eq!(s.phase(), SessionPhase::Prep);
    }

    #[test]
    fn enter_overtime_after_stop_is_a_noop() {
        let mut s = Session::start_running(
            timer_countdown_settings(600),
            Duration::from_secs(100),
        );
        let _ = s.stop(Duration::from_secs(120));
        let effects = s.enter_overtime();
        assert!(effects.is_empty());
        assert_eq!(s.phase(), SessionPhase::Stopped);
    }
}
