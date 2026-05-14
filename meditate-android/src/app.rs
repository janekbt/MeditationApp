// Pure Rust adapter sitting between meditate-core's Session state
// machine and the Slint UI's properties / callbacks. Lifted out of
// lib.rs so it's unit-testable without a Slint runtime.
//
// Sits one level above `meditate_core::session::Session`: the
// adapter exposes the four-state Idle/Active/Finished UI model the
// Slint screens want, while `Session` owns the underlying
// phase clock + pause / resume / overtime mechanics. When the
// session's target hits zero we auto-finalise (the Android shell
// doesn't expose "Add time" yet); finalising drops the session and
// lands us in `Finished`, so a subsequent `toggle` starts fresh.

use meditate_core::format::{format_hhmm, format_time};
use meditate_core::session::{Session, SessionSettings, SessionShape, UiState};
use std::time::Duration;

#[derive(Debug)]
pub enum AppState {
    Idle,
    /// A live Session — its `ui_state()` distinguishes Running from
    /// Paused (and Overtime, transiently, before `tick` finalises it
    /// into `Finished` here). Boxed so the enum's largest-variant
    /// size doesn't dominate the stack footprint of every `AppState`
    /// store (Session carries a few hundred bytes; Idle / Finished
    /// are unit variants).
    Active(Box<Session>),
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
        matches!(self, Self::Active(s) if matches!(s.ui_state(), UiState::Running))
    }
    pub fn is_paused(&self) -> bool {
        matches!(self, Self::Active(s) if matches!(s.ui_state(), UiState::Paused))
    }
    pub fn is_finished(&self) -> bool {
        matches!(self, Self::Finished)
    }
    /// Whether the timer is in flight (Running OR Paused). Distinct
    /// from `is_running` because Paused is also "active": the
    /// foreground service should stay up across pause, so the OS
    /// doesn't reclaim the process and lose the frozen-elapsed
    /// state stored in the Stopwatch.
    pub fn is_active(&self) -> bool {
        matches!(self, Self::Active(_))
    }

    /// Primary action: Start / Pause / Resume / Restart depending
    /// on current state. `total` is consulted only when starting a
    /// fresh session; pause/resume ignore it (Session already
    /// remembers its target).
    pub fn toggle(self, total: Duration, now: Duration) -> Self {
        match self {
            Self::Idle | Self::Finished => {
                let settings = SessionSettings {
                    shape: SessionShape::TimerCountdown {
                        target_secs: total.as_secs() as u32,
                    },
                    ..Default::default()
                };
                Self::Active(Box::new(Session::start_running(settings, now)))
            }
            Self::Active(mut s) => {
                if matches!(s.ui_state(), UiState::Paused) {
                    s.resume(now);
                } else {
                    let _ = s.pause(now);
                }
                Self::Active(s)
            }
        }
    }

    /// Stop button. Active → Finished so the shell can present the
    /// Done screen (elapsed readout + note field + Save / Discard).
    /// Idle / Finished pass through unchanged — Stop is only
    /// reachable from a live session, but defending against a
    /// double-tap or stale callback is cheap. The actual persistence
    /// decision happens later on the Done screen via `dismiss` (the
    /// Android shell stores the in-flight unix_start + elapsed in
    /// `lib.rs` cells; Finished is just a UI marker here).
    pub fn stop(self) -> Self {
        match self {
            Self::Active(_) => Self::Finished,
            other => other,
        }
    }

    /// Done-screen Save or Discard tap. Always returns to Idle. The
    /// Save vs Discard difference (write DB row or not) is handled
    /// in `lib.rs` before this call; AppState itself only models the
    /// UI screen transition.
    pub fn dismiss(self) -> Self {
        match self {
            Self::Finished => Self::Idle,
            other => other,
        }
    }

    /// Called by the tick loop. Drives Session's internal phase
    /// transitions; on Running→Overtime (target reached) we
    /// auto-finalise so the Slint UI flips to the Finished screen
    /// without needing a separate Finish-Overtime button.
    pub fn tick(self, now: Duration) -> Self {
        match self {
            Self::Active(mut s) => {
                let _ = s.tick(now);
                match s.ui_state() {
                    UiState::Overtime => {
                        // Auto-finish: the Android shell has no
                        // Add-time affordance, so target-reached
                        // is the end of the session.
                        let _ = s.finish_overtime();
                        Self::Finished
                    }
                    UiState::Done => Self::Finished,
                    _ => Self::Active(s),
                }
            }
            other => other,
        }
    }

    /// Remaining time the big mm:ss display should show. `total` is
    /// only consulted in the Idle branch; while a session is active
    /// the remaining is `target − pause-aware-elapsed`, clamped at
    /// zero post-target.
    pub fn remaining(&self, total: Duration, now: Duration) -> Duration {
        match self {
            Self::Idle => total,
            Self::Active(s) => total.saturating_sub(s.elapsed(now)),
            Self::Finished => Duration::ZERO,
        }
    }

    /// Label on the primary action button.
    pub fn primary_label(&self) -> &'static str {
        match self {
            Self::Idle | Self::Finished => "Start Session",
            Self::Active(s) if matches!(s.ui_state(), UiState::Paused) => "Resume",
            Self::Active(_) => "Pause",
        }
    }

    /// Whether the Stop button should be visible / enabled. Only
    /// true while a session is in flight — the Done screen has its
    /// own Save / Discard buttons, not a Stop button.
    pub fn can_stop(&self) -> bool {
        self.is_active()
    }

    /// Whether the Running overlay should be visible. Only Active
    /// (Running or Paused) qualifies; Finished swaps the base layer
    /// to the Done view instead.
    pub fn is_running_page(&self) -> bool {
        self.is_active()
    }

    /// Whether the Done base layer should replace Setup. True
    /// exactly when a session has just ended and the user hasn't
    /// yet Save / Discard'd it.
    pub fn is_done_page(&self) -> bool {
        self.is_finished()
    }

    /// Big hero-label text. Setup shows `HH:MM` of the configured
    /// target (matching the GTK shell's `idle_hero_label` —
    /// minute-precision since the user authors duration in minutes).
    /// Running / Paused / Finished show `MM:SS` (or `HH:MM:SS` past
    /// the hour) of the live remaining time, ceiling-rounded so
    /// "10:00" doesn't flicker to "09:59" on the first sub-second
    /// tick. The format pick mirrors core's
    /// `format::idle_hero_label` + `format::format_time` split, so
    /// both shells render identical strings for identical state.
    pub fn hero_label(&self, total: Duration, now: Duration) -> String {
        match self {
            Self::Idle => format_hhmm(total.as_secs() as u32),
            Self::Active(s) => {
                // Session's `display_secs` is the canonical ceiling-
                // rounded readout — same value the GTK shell renders.
                format_time(Duration::from_secs(s.display_secs(now)))
            }
            Self::Finished => format_time(Duration::ZERO),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── hero_label ──────────────────────────────────────────────

    #[test]
    fn hero_label_idle_renders_target_as_hh_mm() {
        // 10 min target → "00:10" (HH:MM, mirrors the GTK shell's
        // idle hero formatter). This is the case the user's bug
        // report flagged: previously the hero showed total-minutes
        // (MM:SS) so 1h 10m read as "70:00".
        let s = AppState::idle();
        assert_eq!(
            s.hero_label(Duration::from_secs(10 * 60), Duration::ZERO),
            "00:10",
        );
    }

    #[test]
    fn hero_label_idle_with_hours_pads_zero_minutes() {
        // 1h 10m → "01:10", not "70:00".
        let s = AppState::idle();
        assert_eq!(
            s.hero_label(Duration::from_secs(70 * 60), Duration::ZERO),
            "01:10",
        );
    }

    #[test]
    fn hero_label_idle_three_hours_renders_as_three_zero_zero() {
        let s = AppState::idle();
        assert_eq!(
            s.hero_label(Duration::from_secs(3 * 3600), Duration::ZERO),
            "03:00",
        );
    }

    #[test]
    fn hero_label_finished_is_double_zero() {
        let s = AppState::Finished;
        assert_eq!(
            s.hero_label(Duration::from_secs(600), Duration::ZERO),
            "00:00",
        );
    }

    #[test]
    fn hero_label_running_renders_mm_ss_under_an_hour() {
        // 10-min session started at t=100, viewed at t=150 → 8:20
        // ceiling-remaining (Session does the ceiling-rounding).
        // format_time picks MM:SS since remaining is under an hour.
        let s = AppState::idle().toggle(
            Duration::from_secs(10 * 60),
            Duration::from_secs(100),
        );
        assert_eq!(s.hero_label(Duration::ZERO, Duration::from_secs(200)), "08:20");
    }

    #[test]
    fn hero_label_running_switches_to_hh_mm_ss_over_an_hour() {
        // 90-min session at t=0 viewed at t=1 → 1:29:59 remaining,
        // so the hero must use HH:MM:SS format.
        let s = AppState::idle().toggle(
            Duration::from_secs(90 * 60),
            Duration::ZERO,
        );
        assert_eq!(s.hero_label(Duration::ZERO, Duration::from_secs(1)), "01:29:59");
    }

    // (Legacy `format_mmss` tests dropped — the readout now flows
    //  through `meditate_core::format::format_time` / `format_hhmm`,
    //  both unit-tested in core. The hero_label cases above pin the
    //  per-state dispatch.)

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
    fn stop_from_running_advances_to_finished() {
        let s = AppState::idle()
            .toggle(ten_minutes(), Duration::from_secs(100))
            .stop();
        assert!(s.is_finished());
    }

    #[test]
    fn stop_from_paused_advances_to_finished() {
        let s = AppState::idle()
            .toggle(ten_minutes(), Duration::from_secs(100))
            .toggle(ten_minutes(), Duration::from_secs(110))
            .stop();
        assert!(s.is_finished());
    }

    #[test]
    fn stop_from_finished_stays_finished() {
        // Stop button isn't reachable from Finished (Done screen has
        // Save / Discard instead), but defending against a stale
        // callback is cheap.
        assert!(AppState::Finished.stop().is_finished());
    }

    // ── dismiss ─────────────────────────────────────────────────

    #[test]
    fn dismiss_from_finished_returns_to_idle() {
        assert!(AppState::Finished.dismiss().is_idle());
    }

    #[test]
    fn dismiss_from_idle_stays_idle() {
        assert!(AppState::idle().dismiss().is_idle());
    }

    #[test]
    fn dismiss_from_active_stays_active() {
        // dismiss is the Save / Discard tap — only reachable when
        // the Done screen is up, so Active passthrough is the
        // defensive case.
        let s = AppState::idle()
            .toggle(ten_minutes(), Duration::from_secs(100))
            .dismiss();
        assert!(s.is_running());
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
    fn can_stop_finished_false() {
        // Done screen has its own Save / Discard buttons, not Stop.
        assert!(!AppState::Finished.can_stop());
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

    #[test]
    fn is_running_page_finished_false() {
        // Finished swaps the BASE layer (Setup → Done); the Running
        // overlay slides off to the right rather than staying on
        // top of Done. So is_running_page is false here.
        assert!(!AppState::Finished.is_running_page());
    }

    #[test]
    fn is_done_page_finished_true() {
        assert!(AppState::Finished.is_done_page());
    }

    #[test]
    fn is_done_page_idle_false() {
        assert!(!AppState::idle().is_done_page());
    }

    #[test]
    fn is_done_page_active_false() {
        let s = AppState::idle().toggle(ten_minutes(), Duration::from_secs(0));
        assert!(!s.is_done_page());
    }
}
