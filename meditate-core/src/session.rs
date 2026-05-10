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

use crate::breath::{BreathPattern, Phase, PhaseInfo};
use crate::db::{BoxBreathPhaseId, SessionMode};
use std::time::Duration;

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
    /// set). `None` for stopwatch-only (Timer-stopwatch /
    /// Guided-stopwatch / Box-Breath-stopwatch). Decides whether the
    /// running display reads as ceiling-rounded remaining or
    /// floor-rounded elapsed in Timer/Guided, and whether Box Breath
    /// auto-ends on a cycle-aligned boundary.
    pub target_secs: Option<u32>,
    /// `Some` only in Box Breath mode. The pattern drives phase
    /// boundary detection (for cue firing) and cycle-aligned session
    /// completion (Box Breath always ends on a full-cycle boundary
    /// rather than mid-phase, so the user finishes on a natural
    /// hold-out).
    pub breath_pattern: Option<BreathPattern>,
    // More fields land in later stages: bells, etc.
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
    /// Box Breath: a phase boundary was just crossed. Shell fires
    /// the per-phase cue (sound + vibration as configured for that
    /// phase). The first tick of a box-breath session seeds the
    /// `last_phase` silently (the starting bell already fired); only
    /// subsequent transitions emit this effect.
    FireBoxBreathCue(BoxBreathPhaseId),
    /// Box Breath: the cycle-aligned target was reached, ending the
    /// session naturally. Shell drops the session and shows the
    /// done view with this duration. Never fired in stopwatch-only
    /// box-breath mode (no target; user must press Stop).
    EndBoxBreath { duration_secs: u64 },
}

/// In-flight session. Created by `start_prep` (when prep silence is
/// enabled) or `start_running` (skip-prep path); driven by `tick(now)`
/// thereafter; consumed by `stop`/`add_overtime`/`finish_overtime`
/// in later stages.
#[derive(Debug)]
pub struct Session {
    settings: SessionSettings,
    phase: SessionPhase,
    /// Boot-time when the *current phase* started. For Prep it's
    /// the start of prep silence; updated to `now` when phase
    /// transitions to Running (so Running's elapsed counts from
    /// the post-prep moment).
    phase_started_at: Duration,
    /// Box-Breath phase boundary tracking. `None` until the first
    /// Running tick (which seeds it silently — the starting bell
    /// already fired). `Some(p)` thereafter; a tick observes
    /// `phase_at(elapsed) != p` and fires `FireBoxBreathCue`.
    /// Always `None` outside Box Breath mode.
    last_breath_phase: Option<Phase>,
}

impl Session {
    /// Start a session in Prep phase. `prep_secs` must be set in
    /// `settings` — caller ensures (an `assert!` would be friendlier
    /// than a silent skip; debug-asserted here).
    pub fn start_prep(settings: SessionSettings, now: Duration) -> Self {
        debug_assert!(
            settings.prep_secs.is_some(),
            "start_prep called without prep_secs in settings",
        );
        Self {
            settings,
            phase: SessionPhase::Prep,
            phase_started_at: now,
            last_breath_phase: None,
        }
    }

    /// Start a session directly in Running phase, skipping prep
    /// silence. Used when `prep_secs` is `None` or the user has
    /// `preparation_time_active = false`. The Running stopwatch
    /// anchors at `now`.
    pub fn start_running(settings: SessionSettings, now: Duration) -> Self {
        Self {
            settings,
            phase: SessionPhase::Running,
            phase_started_at: now,
            last_breath_phase: None,
        }
    }

    /// Drive the session forward by one tick. Returns the effects
    /// the shell should dispatch this tick. Internal phase
    /// transitions (Prep→Running, Running→Overtime, etc.) emit
    /// transition effects (`EndPrep`, `EnterOvertime`, …) so the
    /// shell can fire bell sounds / morph buttons / etc. exactly
    /// at the boundary.
    pub fn tick(&mut self, now: Duration) -> Vec<Effect> {
        match self.phase {
            SessionPhase::Prep => self.tick_prep(now),
            SessionPhase::Running => self.tick_running(now),
            // Stages 0d–0g add the other phases.
            SessionPhase::Paused | SessionPhase::Overtime => Vec::new(),
        }
    }

    fn tick_prep(&mut self, now: Duration) -> Vec<Effect> {
        let target_secs = self.settings.prep_secs.unwrap_or(0) as u64;
        let target = Duration::from_secs(target_secs);
        let elapsed = now.saturating_sub(self.phase_started_at);

        if elapsed >= target {
            // Crossed the prep boundary. Transition to Running and
            // anchor the new phase's start time at `now` so the
            // running phase counts from this exact moment.
            self.phase = SessionPhase::Running;
            self.phase_started_at = now;
            return vec![Effect::EndPrep];
        }

        // Still in prep — emit the ceiling-rounded display value.
        // (k-1, k] remaining → display k, matching the existing
        // gtk shell's tick_prep behaviour.
        let remaining = target.saturating_sub(elapsed);
        let display = remaining.as_secs() + u64::from(remaining.subsec_nanos() > 0);
        vec![Effect::UpdateDisplay { secs: display }]
    }

    fn tick_running(&mut self, now: Duration) -> Vec<Effect> {
        let elapsed = now.saturating_sub(self.phase_started_at);

        // Box Breath running has its own tick shape: phase-boundary
        // detection + cycle-aligned end + elapsed counter.
        if self.settings.breath_pattern.is_some() {
            return self.tick_box_breath(elapsed);
        }

        let display_secs = match self.settings.target_secs {
            // Countdown: ceiling-rounded remaining. (k-1, k] → k.
            // Avoids skipping "0:59" on the very first tick that
            // fires slightly past 1.0 s.
            Some(target_secs) => {
                let target = Duration::from_secs(target_secs as u64);
                let remaining = target.saturating_sub(elapsed);
                if remaining.is_zero() {
                    // Countdown reached zero — display 0. Stage 0e
                    // adds the Running→Overtime transition; until
                    // then a session past the target sits at zero.
                    0
                } else {
                    remaining.as_secs() + u64::from(remaining.subsec_nanos() > 0)
                }
            }
            // Stopwatch-only: floor-rounded elapsed. "0:00" until
            // 1.0 s crosses, "0:01" thereafter.
            None => elapsed.as_secs(),
        };
        vec![Effect::UpdateDisplay { secs: display_secs }]
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
                effects.push(Effect::FireBoxBreathCue(phase_to_id(info.phase)));
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
                effects.push(Effect::EndBoxBreath {
                    duration_secs: elapsed.as_secs(),
                });
                return effects;
            }
        }

        // Counter readout for the running-page top strip. Floor-
        // rounded elapsed; the shell composes "elapsed / target" or
        // just "elapsed" in stopwatch-only mode using settings.
        effects.push(Effect::UpdateDisplay {
            secs: elapsed.as_secs(),
        });
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
        let elapsed = now.saturating_sub(self.phase_started_at);
        Some(pattern.phase_at(elapsed))
    }

    pub fn phase(&self) -> SessionPhase {
        self.phase
    }

    pub fn elapsed(&self, now: Duration) -> Duration {
        now.saturating_sub(self.phase_started_at)
    }

    /// Mode of the in-flight session. Captured at start; never
    /// changes during a session.
    pub fn mode(&self) -> SessionMode {
        self.settings.mode
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

#[cfg(test)]
mod tests {
    use super::*;

    fn timer_settings_with_prep(prep_secs: u32) -> SessionSettings {
        SessionSettings {
            mode: SessionMode::Timer,
            prep_secs: Some(prep_secs),
            target_secs: Some(600),
            breath_pattern: None,
        }
    }

    fn timer_countdown_settings(target_secs: u32) -> SessionSettings {
        SessionSettings {
            mode: SessionMode::Timer,
            prep_secs: None,
            target_secs: Some(target_secs),
            breath_pattern: None,
        }
    }

    fn timer_stopwatch_settings() -> SessionSettings {
        SessionSettings {
            mode: SessionMode::Timer,
            prep_secs: None,
            target_secs: None,
            breath_pattern: None,
        }
    }

    fn box_breath_settings(target_secs: Option<u32>) -> SessionSettings {
        SessionSettings {
            mode: SessionMode::BoxBreath,
            prep_secs: None,
            target_secs,
            breath_pattern: Some(BreathPattern::box_breath()),
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
    fn tick_running_countdown_at_target_displays_zero() {
        // Exactly at the target — countdown is "complete." Display 0.
        // The Running→Overtime transition lands in Stage 0e; until
        // then the session sits at zero.
        let mut s = Session::start_running(
            timer_countdown_settings(60),
            Duration::from_secs(100),
        );
        let effects = s.tick(Duration::from_secs(160));
        assert_eq!(effects, vec![Effect::UpdateDisplay { secs: 0 }]);
        assert_eq!(s.phase(), SessionPhase::Running);
    }

    #[test]
    fn tick_running_countdown_past_target_clamps_at_zero() {
        let mut s = Session::start_running(
            timer_countdown_settings(60),
            Duration::from_secs(100),
        );
        let effects = s.tick(Duration::from_secs(200));
        assert_eq!(effects, vec![Effect::UpdateDisplay { secs: 0 }]);
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
            !effects.iter().any(|e| matches!(e, Effect::FireBoxBreathCue(_))),
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
            effects.contains(&Effect::FireBoxBreathCue(BoxBreathPhaseId::HoldIn)),
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
            !effects.iter().any(|e| matches!(e, Effect::FireBoxBreathCue(_))),
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
            .contains(&Effect::FireBoxBreathCue(BoxBreathPhaseId::HoldIn)));
        // Out boundary at t=108
        assert!(s
            .tick(Duration::from_secs(108))
            .contains(&Effect::FireBoxBreathCue(BoxBreathPhaseId::Out)));
        // HoldOut boundary at t=112
        assert!(s
            .tick(Duration::from_secs(112))
            .contains(&Effect::FireBoxBreathCue(BoxBreathPhaseId::HoldOut)));
        // Wrap to In at t=116
        assert!(s
            .tick(Duration::from_secs(116))
            .contains(&Effect::FireBoxBreathCue(BoxBreathPhaseId::In)));
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
}
