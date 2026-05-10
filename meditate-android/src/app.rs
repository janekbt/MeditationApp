// Pure Rust adapter sitting between meditate-core's immutable
// Countdown and the Slint UI's properties / callbacks. Lifted out
// of lib.rs so it's unit-testable without a Slint runtime.

use meditate_core::timer::{Countdown, CountdownTimer, Stopwatch};
use std::time::Duration;

#[derive(Debug)]
pub enum AppState {
    Idle,
    Running(Countdown),
    Paused(Countdown),
    Finished,
}

impl AppState {
    pub fn idle() -> Self {
        Self::Idle
    }

    pub fn is_idle(&self) -> bool {
        matches!(self, Self::Idle)
    }
    pub fn is_running(&self) -> bool {
        matches!(self, Self::Running(_))
    }
    pub fn is_paused(&self) -> bool {
        matches!(self, Self::Paused(_))
    }
    pub fn is_finished(&self) -> bool {
        matches!(self, Self::Finished)
    }

    /// Primary action: Start / Pause / Resume / Restart depending
    /// on current state.
    pub fn toggle(self, total: Duration, now: Duration) -> Self {
        match self {
            Self::Idle | Self::Finished => Self::Running(Countdown::new(
                CountdownTimer::new(total),
                Stopwatch::started_at(now),
            )),
            Self::Running(c) => Self::Paused(c.pause(now)),
            Self::Paused(c) => Self::Running(c.resume(now)),
        }
    }

    /// Stop button. Always returns to Idle.
    pub fn stop(self) -> Self {
        Self::Idle
    }

    /// Called by the tick loop. Promotes Running → Finished once the
    /// countdown's remaining hits zero. Paused does not auto-advance
    /// because a paused countdown's remaining is frozen.
    pub fn tick(self, now: Duration) -> Self {
        match self {
            Self::Running(ref c) if c.is_finished(now) => Self::Finished,
            other => other,
        }
    }

    /// Remaining time the big mm:ss display should show.
    pub fn remaining(&self, total: Duration, now: Duration) -> Duration {
        match self {
            Self::Idle => total,
            Self::Running(c) | Self::Paused(c) => c.remaining(now),
            Self::Finished => Duration::ZERO,
        }
    }

    /// Label on the primary action button.
    pub fn primary_label(&self) -> &'static str {
        match self {
            Self::Idle | Self::Finished => "Start Session",
            Self::Running(_) => "Pause",
            Self::Paused(_) => "Resume",
        }
    }

    /// Whether the Stop button should be visible / enabled.
    pub fn can_stop(&self) -> bool {
        matches!(
            self,
            Self::Running(_) | Self::Paused(_) | Self::Finished
        )
    }

    /// Whether the running-page layout should be shown (vs. the
    /// setup page). Idle is the only state that shows setup.
    pub fn is_running_page(&self) -> bool {
        !self.is_idle()
    }
}

/// mm:ss with ceiling-rounding at the sub-second boundary so a
/// remaining of 599.999s reads "10:00" rather than "09:59" — matches
/// the GTK shell's `format_time` semantics ("Countdown display
/// ceiling-fixed (no skipped 0:59)" in 26.4.3 release notes).
pub fn format_mmss(d: Duration) -> String {
    let secs = d.as_secs() + u64::from(d.subsec_nanos() > 0);
    let mins = secs / 60;
    let secs = secs % 60;
    format!("{mins:02}:{secs:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── format_mmss ─────────────────────────────────────────────

    #[test]
    fn format_mmss_zero() {
        assert_eq!(format_mmss(Duration::ZERO), "00:00");
    }

    #[test]
    fn format_mmss_one_second() {
        assert_eq!(format_mmss(Duration::from_secs(1)), "00:01");
    }

    #[test]
    fn format_mmss_pads_seconds_with_leading_zero() {
        assert_eq!(format_mmss(Duration::from_secs(9)), "00:09");
    }

    #[test]
    fn format_mmss_one_minute() {
        assert_eq!(format_mmss(Duration::from_secs(60)), "01:00");
    }

    #[test]
    fn format_mmss_ten_minutes() {
        assert_eq!(format_mmss(Duration::from_secs(600)), "10:00");
    }

    #[test]
    fn format_mmss_max_under_an_hour() {
        assert_eq!(format_mmss(Duration::from_secs(3599)), "59:59");
    }

    #[test]
    fn format_mmss_ceils_subsecond_remainder_up() {
        // 599.999s → "10:00" (the GTK ceiling rule). If we floored,
        // pressing Start on a 10-min timer would briefly show "09:59"
        // because the very first tick captures elapsed > 0.
        assert_eq!(
            format_mmss(Duration::from_millis(599_999)),
            "10:00"
        );
    }

    #[test]
    fn format_mmss_does_not_ceil_an_exact_second_boundary() {
        // Exactly 600.000s stays at "10:00", does NOT bump to "10:01".
        assert_eq!(format_mmss(Duration::from_secs(600)), "10:00");
    }

    // ── AppState transitions ────────────────────────────────────

    fn ten_minutes() -> Duration {
        Duration::from_secs(600)
    }

    #[test]
    fn fresh_state_is_idle() {
        assert!(AppState::idle().is_idle());
    }

    #[test]
    fn toggle_from_idle_starts_running() {
        let s = AppState::idle().toggle(ten_minutes(), Duration::from_secs(100));
        assert!(s.is_running());
    }

    #[test]
    fn toggle_from_running_pauses() {
        let s = AppState::idle()
            .toggle(ten_minutes(), Duration::from_secs(100))
            .toggle(ten_minutes(), Duration::from_secs(110));
        assert!(s.is_paused());
    }

    #[test]
    fn toggle_from_paused_resumes() {
        let s = AppState::idle()
            .toggle(ten_minutes(), Duration::from_secs(100))
            .toggle(ten_minutes(), Duration::from_secs(110))
            .toggle(ten_minutes(), Duration::from_secs(150));
        assert!(s.is_running());
    }

    #[test]
    fn toggle_from_finished_starts_a_fresh_countdown() {
        let s = AppState::Finished.toggle(ten_minutes(), Duration::from_secs(100));
        assert!(s.is_running());
        // And the countdown is brand new — full duration remaining.
        assert_eq!(s.remaining(ten_minutes(), Duration::from_secs(100)), ten_minutes());
    }

    #[test]
    fn stop_from_idle_stays_idle() {
        assert!(AppState::idle().stop().is_idle());
    }

    #[test]
    fn stop_from_running_returns_to_idle() {
        let s = AppState::idle()
            .toggle(ten_minutes(), Duration::from_secs(100))
            .stop();
        assert!(s.is_idle());
    }

    #[test]
    fn stop_from_paused_returns_to_idle() {
        let s = AppState::idle()
            .toggle(ten_minutes(), Duration::from_secs(100))
            .toggle(ten_minutes(), Duration::from_secs(110))
            .stop();
        assert!(s.is_idle());
    }

    #[test]
    fn stop_from_finished_returns_to_idle() {
        assert!(AppState::Finished.stop().is_idle());
    }

    // ── tick ────────────────────────────────────────────────────

    #[test]
    fn tick_running_at_remaining_zero_advances_to_finished() {
        let s = AppState::idle()
            .toggle(Duration::from_secs(60), Duration::from_secs(100))
            .tick(Duration::from_secs(160));
        assert!(s.is_finished());
    }

    #[test]
    fn tick_running_with_time_left_stays_running() {
        let s = AppState::idle()
            .toggle(ten_minutes(), Duration::from_secs(100))
            .tick(Duration::from_secs(150));
        assert!(s.is_running());
    }

    #[test]
    fn tick_idle_stays_idle() {
        assert!(AppState::idle().tick(Duration::from_secs(999)).is_idle());
    }

    #[test]
    fn tick_paused_does_not_auto_finish() {
        // Even if `now` is way past total, a paused countdown's
        // remaining is frozen — it must not auto-advance to Finished.
        let s = AppState::idle()
            .toggle(Duration::from_secs(60), Duration::from_secs(100))
            .toggle(Duration::from_secs(60), Duration::from_secs(110))
            .tick(Duration::from_secs(9999));
        assert!(s.is_paused());
    }

    #[test]
    fn tick_finished_stays_finished() {
        assert!(AppState::Finished.tick(Duration::from_secs(0)).is_finished());
    }

    // ── remaining ────────────────────────────────────────────────

    #[test]
    fn remaining_idle_is_full_total_duration() {
        assert_eq!(
            AppState::idle().remaining(ten_minutes(), Duration::from_secs(100)),
            ten_minutes()
        );
    }

    #[test]
    fn remaining_finished_is_zero() {
        assert_eq!(
            AppState::Finished.remaining(ten_minutes(), Duration::from_secs(999)),
            Duration::ZERO
        );
    }

    #[test]
    fn remaining_running_decrements_with_now() {
        let s = AppState::idle().toggle(ten_minutes(), Duration::from_secs(100));
        assert_eq!(
            s.remaining(ten_minutes(), Duration::from_secs(150)),
            Duration::from_secs(550)
        );
    }

    #[test]
    fn remaining_paused_freezes_at_pause_moment() {
        let s = AppState::idle()
            .toggle(ten_minutes(), Duration::from_secs(100))
            .toggle(ten_minutes(), Duration::from_secs(150));
        // 50s elapsed before pause — remaining at any later `now`
        // should still be 600 - 50 = 550s.
        assert_eq!(
            s.remaining(ten_minutes(), Duration::from_secs(9999)),
            Duration::from_secs(550)
        );
    }

    // ── labels + flags ──────────────────────────────────────────

    #[test]
    fn primary_label_idle() {
        assert_eq!(AppState::idle().primary_label(), "Start Session");
    }

    #[test]
    fn primary_label_running() {
        let s = AppState::idle().toggle(ten_minutes(), Duration::from_secs(0));
        assert_eq!(s.primary_label(), "Pause");
    }

    #[test]
    fn primary_label_paused() {
        let s = AppState::idle()
            .toggle(ten_minutes(), Duration::from_secs(0))
            .toggle(ten_minutes(), Duration::from_secs(10));
        assert_eq!(s.primary_label(), "Resume");
    }

    #[test]
    fn primary_label_finished_is_start_again() {
        assert_eq!(AppState::Finished.primary_label(), "Start Session");
    }

    #[test]
    fn can_stop_idle_false() {
        assert!(!AppState::idle().can_stop());
    }

    #[test]
    fn can_stop_running_true() {
        let s = AppState::idle().toggle(ten_minutes(), Duration::from_secs(0));
        assert!(s.can_stop());
    }

    #[test]
    fn can_stop_paused_true() {
        let s = AppState::idle()
            .toggle(ten_minutes(), Duration::from_secs(0))
            .toggle(ten_minutes(), Duration::from_secs(10));
        assert!(s.can_stop());
    }

    #[test]
    fn can_stop_finished_true() {
        // Finished still wants Stop visible so the user can reset.
        assert!(AppState::Finished.can_stop());
    }

    #[test]
    fn is_running_page_idle_false() {
        assert!(!AppState::idle().is_running_page());
    }

    #[test]
    fn is_running_page_running_true() {
        let s = AppState::idle().toggle(ten_minutes(), Duration::from_secs(0));
        assert!(s.is_running_page());
    }
}
