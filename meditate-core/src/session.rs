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
    // More fields land in later stages: target_secs, breath_pattern,
    // bells, stopwatch_only, etc.
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

    /// Drive the session forward by one tick. Returns the effects
    /// the shell should dispatch this tick. Internal phase
    /// transitions (Prep→Running, Running→Overtime, etc.) emit
    /// transition effects (`EndPrep`, `EnterOvertime`, …) so the
    /// shell can fire bell sounds / morph buttons / etc. exactly
    /// at the boundary.
    pub fn tick(&mut self, now: Duration) -> Vec<Effect> {
        match self.phase {
            SessionPhase::Prep => self.tick_prep(now),
            // Stages 0b–0g add the other phases.
            SessionPhase::Running | SessionPhase::Paused | SessionPhase::Overtime => Vec::new(),
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
        // and transition; a follow-up tick is in Running (which is
        // a no-op until stage 0b).
        let mut s = Session::start_prep(timer_settings_with_prep(30), Duration::from_secs(100));
        let effects = s.tick(Duration::from_secs(135));
        assert_eq!(effects, vec![Effect::EndPrep]);
        assert_eq!(s.phase(), SessionPhase::Running);

        // Subsequent tick produces no effects (Running not yet
        // implemented — stage 0b lands it).
        let effects = s.tick(Duration::from_secs(136));
        assert!(effects.is_empty());
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
}
