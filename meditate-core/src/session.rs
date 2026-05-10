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

use crate::db::SessionMode;
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
    /// floor-rounded elapsed.
    pub target_secs: Option<u32>,
    // More fields land in later stages: breath_pattern, bells, etc.
}

/// Side effect the shell should dispatch this tick. The shell
/// matches on each variant and routes to its native layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Update the running-page big time display. `secs` is the
    /// already-rounded display value (ceiling for prep/countdown,
    /// floor for stopwatch). Shell formats via `format::format_time`.
    UpdateDisplay { secs: u64 },
    /// Prep silence elapsed; transition to Running. Shell:
    /// constructs the countdown core for Timer mode, plays the
    /// starting bell, refreshes the running button row.
    EndPrep,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn timer_settings_with_prep(prep_secs: u32) -> SessionSettings {
        SessionSettings {
            mode: SessionMode::Timer,
            prep_secs: Some(prep_secs),
            target_secs: Some(600),
        }
    }

    fn timer_countdown_settings(target_secs: u32) -> SessionSettings {
        SessionSettings {
            mode: SessionMode::Timer,
            prep_secs: None,
            target_secs: Some(target_secs),
        }
    }

    fn timer_stopwatch_settings() -> SessionSettings {
        SessionSettings {
            mode: SessionMode::Timer,
            prep_secs: None,
            target_secs: None,
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
