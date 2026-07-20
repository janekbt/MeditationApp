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
use meditate_core::session::{Effect, Session, SessionSettings, SessionShape, UiState};
use std::time::Duration;

/// User-visible mode chip group at the top of the Setup view —
/// mirrors `meditate-gtk/src/timer/imp.rs::TimerMode` so per-mode
/// helpers in `meditate_core::settings_keys` (which expect the core
/// `SessionMode` enum) map across the two shells identically.
///
/// Naming follows GTK exactly (`Breathing` is the shell name for
/// what core calls `BoxBreath`); the From impl bridges the gap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TimerMode {
    #[default]
    Timer,
    Breathing,
    /// Guided meditation — user picks an audio file and the session
    /// length is the file's natural duration. Surface placeholder
    /// until phase 5's audio engine arrives.
    Guided,
}

impl From<TimerMode> for meditate_core::SessionMode {
    fn from(m: TimerMode) -> Self {
        match m {
            TimerMode::Timer => meditate_core::SessionMode::Timer,
            TimerMode::Breathing => meditate_core::SessionMode::BoxBreath,
            TimerMode::Guided => meditate_core::SessionMode::Guided,
        }
    }
}

/// Helpers for the per-mode Cues SegmentedButton — bridge the
/// Slint `current-index: int` to core's `SignalMode`. Index order
/// matches GTK's `cues_signal_toggle_host` toggle list (Sound /
/// Vibration / Both); changing the order here breaks the index
/// encoding shared with the .blp.
pub fn signal_mode_from_chip_index(idx: i32) -> meditate_core::SignalMode {
    match idx {
        0 => meditate_core::SignalMode::Sound,
        1 => meditate_core::SignalMode::Vibration,
        _ => meditate_core::SignalMode::Both,
    }
}

pub fn signal_mode_to_chip_index(m: meditate_core::SignalMode) -> i32 {
    match m {
        meditate_core::SignalMode::Sound => 0,
        meditate_core::SignalMode::Vibration => 1,
        meditate_core::SignalMode::Both => 2,
    }
}

impl TimerMode {
    /// Map the Slint chip group's `current-index` (Timer=0,
    /// Guided=1, Breathing=2 — mirroring the .blp `Adw.Toggle`
    /// order) onto a TimerMode. Out-of-range falls back to the
    /// default (Timer) rather than panicking.
    pub fn from_chip_index(idx: i32) -> Self {
        match idx {
            0 => Self::Timer,
            1 => Self::Guided,
            2 => Self::Breathing,
            _ => Self::default(),
        }
    }

    /// Inverse of `from_chip_index` — used by the Rust side to
    /// echo the model state back to Slint when the user picks a
    /// mode out-of-band (e.g., default at startup).
    pub fn to_chip_index(self) -> i32 {
        match self {
            Self::Timer => 0,
            Self::Guided => 1,
            Self::Breathing => 2,
        }
    }
}

/// System clock convention for rendering times of day — parsed
/// from `MeditateAbout.timeFormat` ("24" or "12|AM|PM").
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClockFormat {
    H24,
    H12 { am: String, pm: String },
}

impl ClockFormat {
    pub fn parse(raw: &str) -> Self {
        let mut it = raw.split('|');
        match (it.next(), it.next(), it.next()) {
            (Some("12"), Some(am), Some(pm)) => Self::H12 {
                am: am.to_string(),
                pm: pm.to_string(),
            },
            _ => Self::H24,
        }
    }
}

/// Render a local time of day per the system clock convention.
/// 12-hour follows the platform's own wrapping: 0 → 12 AM,
/// 12 → 12 PM, 13 → 1 PM.
pub fn render_time_of_day(
    key: meditate_core::format::TimeOfDayKey,
    fmt: &ClockFormat,
) -> String {
    match fmt {
        ClockFormat::H24 => {
            format!("{:02}:{:02}", key.hour, key.minute)
        }
        ClockFormat::H12 { am, pm } => {
            let marker = if key.hour < 12 { am } else { pm };
            let h12 = match key.hour % 12 {
                0 => 12,
                h => h,
            };
            format!("{}:{:02} {}", h12, key.minute, marker)
        }
    }
}

/// Group an integer's digits with the locale separator
/// ("2875" → "2.875" for de). Empty separator = no grouping.
pub fn group_digits(n: i64, sep: &str) -> String {
    let raw = n.abs().to_string();
    let grouped = if sep.is_empty() || raw.len() <= 3 {
        raw
    } else {
        let bytes = raw.as_bytes();
        let mut out = String::with_capacity(raw.len() + 4);
        for (i, b) in bytes.iter().enumerate() {
            if i > 0 && (bytes.len() - i) % 3 == 0 {
                out.push_str(sep);
            }
            out.push(*b as char);
        }
        out
    };
    if n < 0 {
        format!("-{grouped}")
    } else {
        grouped
    }
}

/// What the running-screen primary button does right now. Kept
/// as a typed key (not a rendered string) so translation happens
/// at the shell boundary — see [`AppState::primary_action`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrimaryAction {
    Start,
    Resume,
    Pause,
}

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

/// An `AppState` transition plus the core `Session` effects it
/// emitted. `toggle` / `tick` / `stop` return this so the shell
/// can dispatch the effects (bell sound / vibration / cut
/// in-flight signals) exactly the way GTK's
/// `dispatch_session_effects` does — the decision (which cue, or
/// none) stays in core; the shell only carries it out.
///
/// `Deref<Target = AppState>` so every read-only query
/// (`is_running`, `remaining`, `hero_label`, …) and the unit
/// tests that chain transitions keep working unchanged — they
/// see through to the inner state; only the shell reaches for
/// `.effects`.
pub struct Transition {
    pub state: AppState,
    pub effects: Vec<Effect>,
}

impl std::ops::Deref for Transition {
    type Target = AppState;
    fn deref(&self) -> &AppState {
        &self.state
    }
}

impl Transition {
    fn new(state: AppState, effects: Vec<Effect>) -> Self {
        Self { state, effects }
    }

    /// Chain another transition off this one. The shell never
    /// chains (it dispatches after each user action), but the
    /// state-machine tests do (`toggle(..).toggle(..).tick(..)`);
    /// prior effects are carried forward so nothing is silently
    /// dropped if a caller ever does chain.
    pub fn toggle(self, shape: SessionShape, now: Duration) -> Transition {
        let mut t = self.state.toggle(shape, now);
        prepend(&mut t.effects, self.effects);
        t
    }
    pub fn tick(self, now: Duration) -> Transition {
        let mut t = self.state.tick(now);
        prepend(&mut t.effects, self.effects);
        t
    }
    pub fn stop(self) -> Transition {
        let mut t = self.state.stop();
        prepend(&mut t.effects, self.effects);
        t
    }
    pub fn finish_overtime(self) -> Transition {
        let mut t = self.state.finish_overtime();
        prepend(&mut t.effects, self.effects);
        t
    }
    pub fn enter_overtime(self) -> Transition {
        let mut t = self.state.enter_overtime();
        prepend(&mut t.effects, self.effects);
        t
    }
    pub fn add_overtime(self, now: Duration) -> Transition {
        let mut t = self.state.add_overtime(now);
        prepend(&mut t.effects, self.effects);
        t
    }
    /// Done-screen Save / Discard — a pure UI transition with no
    /// core effects, so it returns the bare `AppState` (keeps the
    /// `lib.rs` `*s = …dismiss();` sites unchanged).
    pub fn dismiss(self) -> AppState {
        self.state.dismiss()
    }
}

fn prepend(effects: &mut Vec<Effect>, mut earlier: Vec<Effect>) {
    if earlier.is_empty() {
        return;
    }
    earlier.append(effects);
    *effects = earlier;
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
    /// In overtime: the countdown crossed zero, the end bell rang,
    /// and the session keeps ticking until the user taps Finish or
    /// Add. The running overlay stays up but its buttons morph
    /// (mirrors GTK's Overtime phase).
    pub fn is_overtime(&self) -> bool {
        matches!(self, Self::Active(s) if matches!(s.ui_state(), UiState::Overtime))
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
    /// on current state. `shape` is consulted only when starting a
    /// fresh session; pause/resume ignore it (Session already
    /// remembers its shape). The shell picks the variant based on
    /// the active mode chip plus the Stopwatch-Mode switch — Timer
    /// with stopwatch off → `TimerCountdown`, with stopwatch on →
    /// `TimerStopwatch`, etc. Keeping shape construction shell-side
    /// matches the GTK shell's `on_start` (it builds the right
    /// `CoreSessionShape` from `current_mode()` + `stopwatch_toggle_on`).
    /// Start a fresh session from fully-built `SessionSettings`.
    /// The shell assembles these from the DB (interval bells,
    /// starting/end cue, per-mode signal-mode override, prep)
    /// exactly like GTK's `build_timer_settings`, so core gets
    /// the real cue config and emits the right `Fire*` / end
    /// effects (instead of the old `..Default::default()` that
    /// left every bell `None` and made core emit nothing).
    /// Honours prep: `Some` prep secs → `start_prep` (silent
    /// pre-roll, starting bell fires at the prep→Running edge),
    /// else `start_running`. No effects on the start edge itself
    /// (the starting bell, if any, arrives on a later tick).
    pub fn start_session(settings: SessionSettings, now: Duration) -> Transition {
        let session = if settings.prep_secs.is_some() {
            Session::start_prep(settings, now)
        } else {
            Session::start_running(settings, now)
        };
        Transition::new(Self::Active(Box::new(session)), Vec::new())
    }

    pub fn toggle(self, shape: SessionShape, now: Duration) -> Transition {
        match self {
            Self::Idle | Self::Finished => {
                // Test / host convenience: a bare default session
                // with no cues. The Android shell does NOT take
                // this path to start — it calls `start_session`
                // with DB-built settings so core has the real
                // bell config (see lib.rs).
                Self::start_session(
                    SessionSettings {
                        shape,
                        ..Default::default()
                    },
                    now,
                )
            }
            Self::Active(mut s) => {
                // Pause emits StopActiveSignals (the user wants
                // everything to hush); resume emits nothing. Both
                // flow back to the shell so a paused-mid-bell
                // session cuts the cue, exactly like GTK.
                let effects = if matches!(s.ui_state(), UiState::Paused) {
                    s.resume(now);
                    Vec::new()
                } else {
                    s.pause(now)
                };
                Transition::new(Self::Active(s), effects)
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
    pub fn stop(self) -> Transition {
        match self {
            // The session is dropped here (persistence runs off
            // the lib.rs pending_done cells, not core's
            // EndSession), so we don't call core `Session::stop`
            // — but Stop is still a user-driven boundary that
            // must cut any in-flight bell / vibration, so we
            // synthesize the one effect the dispatcher needs.
            // Mirrors GTK's `stop_active_signals()`; the absence
            // of `FireEndBell` here is exactly why Stop is silent
            // while a natural countdown finish (which emits
            // `FireEndBell` from `tick`) still rings.
            Self::Active(_) => {
                Transition::new(Self::Finished, vec![Effect::StopActiveSignals])
            }
            other => Transition::new(other, Vec::new()),
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
    /// transitions and surfaces the effects it emits.
    ///
    /// At the Running→Overtime zero-crossing core emits
    /// `EnterOvertime` + `FireEndBell`; we stay `Active` (now
    /// reporting `UiState::Overtime`) so the end bell rings and
    /// the session keeps ticking — the user ends it explicitly
    /// via Finish / Add (`finish_overtime` / `add_overtime`),
    /// mirroring GTK. Subsequent overtime ticks emit
    /// `UpdateOvertimeLabel { overtime }` (the Add-button text)
    /// plus any interval `FireBell`. `UiState::Done` (Box-Breath
    /// cycle-aligned end) still finalises directly.
    pub fn tick(self, now: Duration) -> Transition {
        match self {
            Self::Active(mut s) => {
                let effects = s.tick(now);
                match s.ui_state() {
                    UiState::Done => Transition::new(Self::Finished, effects),
                    // Running, Paused, AND Overtime all stay
                    // Active — overtime is no longer auto-finished.
                    _ => Transition::new(Self::Active(s), effects),
                }
            }
            other => Transition::new(other, Vec::new()),
        }
    }

    /// Overtime "Finish" tap — record exactly the planned
    /// countdown duration (the overtime delta is discarded).
    /// `finish_overtime` emits `StopActiveSignals` + `EndSession
    /// { duration_secs: target }`; the shell reads the latter for
    /// the saved-row duration. No-op (passes through) outside
    /// Overtime — defends a stale tap.
    pub fn finish_overtime(self) -> Transition {
        match self {
            Self::Active(mut s)
                if matches!(s.ui_state(), UiState::Overtime) =>
            {
                let effects = s.finish_overtime();
                Transition::new(Self::Finished, effects)
            }
            other => Transition::new(other, Vec::new()),
        }
    }

    /// Overtime "Add" tap — record the full elapsed time
    /// including the overtime the user let run. `add_overtime_
    /// and_finish` emits `StopActiveSignals` + `EndSession {
    /// duration_secs: total }`.
    pub fn add_overtime(self, now: Duration) -> Transition {
        match self {
            Self::Active(mut s)
                if matches!(s.ui_state(), UiState::Overtime) =>
            {
                let effects = s.add_overtime_and_finish(now);
                Transition::new(Self::Finished, effects)
            }
            other => Transition::new(other, Vec::new()),
        }
    }

    /// Guided audio reached EOS — force the session into Overtime
    /// now (core emits `EnterOvertime` + `FireEndBell`), instead
    /// of waiting for the countdown tick to cross the *probed*
    /// duration (which can be off by a beat). Idempotent: a no-op
    /// passthrough if the tick already entered Overtime, or if
    /// not Running — exactly GTK's EOS → `Session::enter_overtime`
    /// (`meditate-core/src/session/mod.rs:401`).
    pub fn enter_overtime(self) -> Transition {
        match self {
            Self::Active(mut s)
                if matches!(s.ui_state(), UiState::Running) =>
            {
                let effects = s.enter_overtime();
                Transition::new(Self::Active(s), effects)
            }
            other => Transition::new(other, Vec::new()),
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

    /// Typed key for the primary action button — the shell maps
    /// each variant to its translated label (Tr catalogue on
    /// Android, gettext on a future shell).
    pub fn primary_action(&self) -> PrimaryAction {
        match self {
            Self::Idle | Self::Finished => PrimaryAction::Start,
            Self::Active(s) if matches!(s.ui_state(), UiState::Paused) => {
                PrimaryAction::Resume
            }
            Self::Active(_) => PrimaryAction::Pause,
        }
    }

    /// English render of `primary_action` (tests + logging).
    pub fn primary_label(&self) -> &'static str {
        match self.primary_action() {
            PrimaryAction::Start => "Start Session",
            PrimaryAction::Resume => "Resume",
            PrimaryAction::Pause => "Pause",
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
    /// Stopwatch-on flips the Idle readout to `00:00` so the
    /// stopwatch's count-up starts visibly from zero — mirrors GTK's
    /// `refresh_stopwatch_dependent_ui` comment "stopwatch flips the
    /// hero between 00:00 and the mode's target reading in every
    /// mode". Running / Paused / Finished show `MM:SS` (or
    /// `HH:MM:SS` past the hour) of the live remaining-or-elapsed
    /// time — `Session::display_secs` handles the count-up vs
    /// count-down branch per shape variant, so this layer can stay
    /// shape-agnostic.
    pub fn hero_label(&self, total: Duration, now: Duration, stopwatch_on: bool) -> String {
        match self {
            Self::Idle => {
                if stopwatch_on {
                    format_time(Duration::ZERO)
                } else {
                    format_hhmm(total.as_secs() as u32)
                }
            }
            Self::Active(s) => {
                // Session's `display_secs` is the canonical readout —
                // ceiling-rounded remaining for countdowns, floor-
                // rounded elapsed for stopwatches. Same value the
                // GTK shell renders.
                format_time(Duration::from_secs(s.display_secs(now)))
            }
            Self::Finished => format_time(Duration::ZERO),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── TimerMode chip mapping ──────────────────────────────────

    #[test]
    fn chip_index_round_trips_for_every_mode() {
        for mode in [TimerMode::Timer, TimerMode::Breathing, TimerMode::Guided] {
            assert_eq!(TimerMode::from_chip_index(mode.to_chip_index()), mode);
        }
    }

    #[test]
    fn chip_index_order_matches_blp_toggle_order() {
        // `timer_view.blp` declares the Adw.ToggleGroup children in
        // this exact order: countdown_toggle, guided_toggle,
        // breathing_toggle. The Slint chip group must match so an
        // index passed from either shell selects the same row.
        assert_eq!(TimerMode::from_chip_index(0), TimerMode::Timer);
        assert_eq!(TimerMode::from_chip_index(1), TimerMode::Guided);
        assert_eq!(TimerMode::from_chip_index(2), TimerMode::Breathing);
    }

    #[test]
    fn chip_index_out_of_range_falls_back_to_default() {
        assert_eq!(TimerMode::from_chip_index(-1), TimerMode::Timer);
        assert_eq!(TimerMode::from_chip_index(99), TimerMode::Timer);
    }

    // ── SignalMode chip mapping ─────────────────────────────────

    #[test]
    fn signal_mode_chip_index_round_trips_for_every_variant() {
        use meditate_core::SignalMode;
        for m in [SignalMode::Sound, SignalMode::Vibration, SignalMode::Both] {
            assert_eq!(
                signal_mode_from_chip_index(signal_mode_to_chip_index(m)),
                m,
            );
        }
    }

    #[test]
    fn signal_mode_chip_index_order_matches_blp_toggle_order() {
        // `timer_view.blp` builds the Cues toggle group as
        // Sound (name "sound"), Vibration ("vibration"), Both
        // ("both") — in that order. The Slint chip group has to
        // agree so the same int round-trips through the DB.
        use meditate_core::SignalMode;
        assert_eq!(signal_mode_from_chip_index(0), SignalMode::Sound);
        assert_eq!(signal_mode_from_chip_index(1), SignalMode::Vibration);
        assert_eq!(signal_mode_from_chip_index(2), SignalMode::Both);
    }

    #[test]
    fn signal_mode_chip_index_out_of_range_falls_back_to_both() {
        // Falls back to Both because that's both the GTK shell's
        // default and the safest "all channels on" mode for a
        // user with a broken int.
        use meditate_core::SignalMode;
        assert_eq!(signal_mode_from_chip_index(-1), SignalMode::Both);
        assert_eq!(signal_mode_from_chip_index(99), SignalMode::Both);
    }

    #[test]
    fn timer_mode_maps_to_core_session_mode() {
        use meditate_core::SessionMode;
        assert_eq!(SessionMode::from(TimerMode::Timer), SessionMode::Timer);
        assert_eq!(SessionMode::from(TimerMode::Breathing), SessionMode::BoxBreath);
        assert_eq!(SessionMode::from(TimerMode::Guided), SessionMode::Guided);
    }


    // ── hero_label ──────────────────────────────────────────────

    #[test]
    fn hero_label_idle_renders_target_as_hh_mm() {
        // 10 min target → "00:10" (HH:MM, mirrors the GTK shell's
        // idle hero formatter). This is the case the user's bug
        // report flagged: previously the hero showed total-minutes
        // (MM:SS) so 1h 10m read as "70:00".
        let s = AppState::idle();
        assert_eq!(
            s.hero_label(Duration::from_secs(10 * 60), Duration::ZERO, false),
            "00:10",
        );
    }

    #[test]
    fn hero_label_idle_with_hours_pads_zero_minutes() {
        // 1h 10m → "01:10", not "70:00".
        let s = AppState::idle();
        assert_eq!(
            s.hero_label(Duration::from_secs(70 * 60), Duration::ZERO, false),
            "01:10",
        );
    }

    #[test]
    fn hero_label_idle_three_hours_renders_as_three_zero_zero() {
        let s = AppState::idle();
        assert_eq!(
            s.hero_label(Duration::from_secs(3 * 3600), Duration::ZERO, false),
            "03:00",
        );
    }

    #[test]
    fn hero_label_finished_is_double_zero() {
        let s = AppState::Finished;
        assert_eq!(
            s.hero_label(Duration::from_secs(600), Duration::ZERO, false),
            "00:00",
        );
    }

    #[test]
    fn hero_label_running_renders_mm_ss_under_an_hour() {
        // 10-min session started at t=100, viewed at t=150 → 8:20
        // ceiling-remaining (Session does the ceiling-rounding).
        // format_time picks MM:SS since remaining is under an hour.
        let s = AppState::idle().toggle(
            timer_countdown(Duration::from_secs(10 * 60)),
            Duration::from_secs(100),
        );
        assert_eq!(s.hero_label(Duration::ZERO, Duration::from_secs(200), false), "08:20");
    }

    #[test]
    fn hero_label_idle_with_stopwatch_on_renders_zero() {
        // Stopwatch flips the Idle hero from "configured target" to
        // "00:00" — matches the GTK shell's
        // `refresh_stopwatch_dependent_ui` comment ("stopwatch flips
        // the hero between 00:00 and the mode's target reading").
        let s = AppState::idle();
        assert_eq!(
            s.hero_label(Duration::from_secs(10 * 60), Duration::ZERO, true),
            "00:00",
        );
    }

    #[test]
    fn hero_label_running_stopwatch_counts_up() {
        // 50s into a TimerStopwatch session, hero shows "00:50".
        let shape = SessionShape::TimerStopwatch;
        let s = AppState::idle().toggle(shape, Duration::from_secs(100));
        assert_eq!(
            s.hero_label(Duration::ZERO, Duration::from_secs(150), true),
            "00:50",
        );
    }

    #[test]
    fn hero_label_running_switches_to_hh_mm_ss_over_an_hour() {
        // 90-min session at t=0 viewed at t=1 → 1:29:59 remaining,
        // so the hero must use HH:MM:SS format.
        let s = AppState::idle().toggle(
            timer_countdown(Duration::from_secs(90 * 60)),
            Duration::ZERO,
        );
        assert_eq!(s.hero_label(Duration::ZERO, Duration::from_secs(1), false), "01:29:59");
    }

    // (Legacy `format_mmss` tests dropped — the readout now flows
    //  through `meditate_core::format::format_time` / `format_hhmm`,
    //  both unit-tested in core. The hero_label cases above pin the
    //  per-state dispatch.)

    // ── AppState transitions ────────────────────────────────────

    fn ten_minutes() -> Duration {
        Duration::from_secs(600)
    }

    /// Test helper: a Timer-countdown shape with the given target
    /// seconds. Mirrors how the gtk + android shells build settings
    /// before calling `toggle` — extracted so test bodies stay
    /// focused on the state-machine assertion they're making.
    fn timer_countdown(target: Duration) -> SessionShape {
        SessionShape::TimerCountdown { target_secs: target.as_secs() as u32 }
    }

    #[test]
    fn fresh_state_is_idle() {
        assert!(AppState::idle().is_idle());
    }

    #[test]
    fn toggle_from_idle_starts_running() {
        let s = AppState::idle().toggle(timer_countdown(ten_minutes()), Duration::from_secs(100));
        assert!(s.is_running());
    }

    #[test]
    fn toggle_from_running_pauses() {
        let s = AppState::idle()
            .toggle(timer_countdown(ten_minutes()), Duration::from_secs(100))
            .toggle(timer_countdown(ten_minutes()), Duration::from_secs(110));
        assert!(s.is_paused());
    }

    #[test]
    fn toggle_from_paused_resumes() {
        let s = AppState::idle()
            .toggle(timer_countdown(ten_minutes()), Duration::from_secs(100))
            .toggle(timer_countdown(ten_minutes()), Duration::from_secs(110))
            .toggle(timer_countdown(ten_minutes()), Duration::from_secs(150));
        assert!(s.is_running());
    }

    #[test]
    fn toggle_from_finished_starts_a_fresh_countdown() {
        let s = AppState::Finished.toggle(timer_countdown(ten_minutes()), Duration::from_secs(100));
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
            .toggle(timer_countdown(ten_minutes()), Duration::from_secs(100))
            .stop();
        assert!(s.is_finished());
    }

    #[test]
    fn stop_from_paused_advances_to_finished() {
        let s = AppState::idle()
            .toggle(timer_countdown(ten_minutes()), Duration::from_secs(100))
            .toggle(timer_countdown(ten_minutes()), Duration::from_secs(110))
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
            .toggle(timer_countdown(ten_minutes()), Duration::from_secs(100))
            .dismiss();
        assert!(s.is_running());
    }

    // ── tick ────────────────────────────────────────────────────

    #[test]
    fn tick_running_at_remaining_zero_enters_overtime_not_finished() {
        // Target crossed: stays Active in Overtime (end bell rang,
        // session keeps ticking) — NOT auto-finished. The user
        // ends it via Finish / Add.
        let s = AppState::idle()
            .toggle(timer_countdown(Duration::from_secs(60)), Duration::from_secs(100))
            .tick(Duration::from_secs(160));
        assert!(!s.is_finished());
        assert!(s.is_overtime());
    }

    #[test]
    fn finish_overtime_from_overtime_finishes() {
        let s = AppState::idle()
            .toggle(timer_countdown(Duration::from_secs(60)), Duration::from_secs(100))
            .tick(Duration::from_secs(160))
            .finish_overtime();
        assert!(s.is_finished());
    }

    #[test]
    fn add_overtime_from_overtime_finishes() {
        let s = AppState::idle()
            .toggle(timer_countdown(Duration::from_secs(60)), Duration::from_secs(100))
            .tick(Duration::from_secs(160))
            .add_overtime(Duration::from_secs(175));
        assert!(s.is_finished());
    }

    /// Guided shape helper — `duration_secs` is the picked
    /// file's probed length; `count_up` mirrors the per-mode
    /// stopwatch flag.
    fn guided(target: Duration, count_up: bool) -> SessionShape {
        SessionShape::Guided {
            duration_secs: target.as_secs() as u32,
            count_up_display: count_up,
        }
    }

    #[test]
    fn guided_lifecycle_start_overtime_finish() {
        // Guided behaves like a countdown of the file length:
        // before the end it's running; when the file's duration
        // is crossed (audio EOS, or the tick crossing it) it
        // enters Overtime (end bell), stays Active, and Finish
        // ends it — exactly the Timer-countdown contract, just
        // sourced from the file. Mirrors GTK's Guided session.
        let running = AppState::idle().toggle(
            guided(Duration::from_secs(120), false),
            Duration::from_secs(100),
        );
        assert!(running.is_running());
        // 30s in — still playing.
        let mid = running.tick(Duration::from_secs(130));
        assert!(mid.is_running() && !mid.is_overtime());
        // Past the file length → overtime, NOT auto-finished.
        let over = mid.tick(Duration::from_secs(225));
        assert!(over.is_overtime() && !over.is_finished());
        assert!(over.finish_overtime().is_finished());
    }

    #[test]
    fn enter_overtime_from_running_enters_overtime_then_finish() {
        // Guided audio EOS forces overtime early (probe was a
        // beat short): Running → Overtime (stays Active), then
        // Finish ends it.
        let s = AppState::idle()
            .toggle(guided(Duration::from_secs(300), false),
                Duration::from_secs(0))
            .enter_overtime();
        assert!(s.is_overtime() && !s.is_finished());
        assert!(s.finish_overtime().is_finished());
    }

    #[test]
    fn enter_overtime_outside_running_is_a_passthrough() {
        // Idle / already-overtime → no-op (defends a late EOS
        // after the tick already crossed the probed duration).
        assert!(AppState::idle().enter_overtime().is_idle());
        let over = AppState::idle()
            .toggle(timer_countdown(Duration::from_secs(60)),
                Duration::from_secs(100))
            .tick(Duration::from_secs(160));
        assert!(over.is_overtime());
        assert!(over.enter_overtime().is_overtime()); // still, no double
    }

    #[test]
    fn guided_count_up_display_does_not_change_the_state_machine() {
        // The count_up flag is display-only; the session still
        // ends at the file's duration either way.
        let s = AppState::idle()
            .toggle(
                guided(Duration::from_secs(60), true),
                Duration::from_secs(0),
            )
            .tick(Duration::from_secs(120));
        assert!(s.is_overtime() && !s.is_finished());
    }

    #[test]
    fn finish_overtime_outside_overtime_is_a_passthrough() {
        // Running (not yet overtime) → Finish is a no-op.
        let s = AppState::idle()
            .toggle(timer_countdown(ten_minutes()), Duration::from_secs(100))
            .finish_overtime();
        assert!(s.is_running());
    }

    #[test]
    fn tick_running_with_time_left_stays_running() {
        let s = AppState::idle()
            .toggle(timer_countdown(ten_minutes()), Duration::from_secs(100))
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
            .toggle(timer_countdown(Duration::from_secs(60)), Duration::from_secs(100))
            .toggle(timer_countdown(Duration::from_secs(60)), Duration::from_secs(110))
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
        let s = AppState::idle().toggle(timer_countdown(ten_minutes()), Duration::from_secs(100));
        assert_eq!(
            s.remaining(ten_minutes(), Duration::from_secs(150)),
            Duration::from_secs(550)
        );
    }

    #[test]
    fn remaining_paused_freezes_at_pause_moment() {
        let s = AppState::idle()
            .toggle(timer_countdown(ten_minutes()), Duration::from_secs(100))
            .toggle(timer_countdown(ten_minutes()), Duration::from_secs(150));
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
        let s = AppState::idle().toggle(timer_countdown(ten_minutes()), Duration::from_secs(0));
        assert_eq!(s.primary_label(), "Pause");
    }

    #[test]
    fn primary_label_paused() {
        let s = AppState::idle()
            .toggle(timer_countdown(ten_minutes()), Duration::from_secs(0))
            .toggle(timer_countdown(ten_minutes()), Duration::from_secs(10));
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
        let s = AppState::idle().toggle(timer_countdown(ten_minutes()), Duration::from_secs(0));
        assert!(s.can_stop());
    }

    #[test]
    fn can_stop_paused_true() {
        let s = AppState::idle()
            .toggle(timer_countdown(ten_minutes()), Duration::from_secs(0))
            .toggle(timer_countdown(ten_minutes()), Duration::from_secs(10));
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
        let s = AppState::idle().toggle(timer_countdown(ten_minutes()), Duration::from_secs(0));
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
        let s = AppState::idle().toggle(timer_countdown(ten_minutes()), Duration::from_secs(0));
        assert!(!s.is_done_page());
    }

    #[test]
    fn clock_format_parses_both_conventions() {
        assert_eq!(ClockFormat::parse("24"), ClockFormat::H24);
        assert_eq!(
            ClockFormat::parse("12|AM|PM"),
            ClockFormat::H12 { am: "AM".into(), pm: "PM".into() },
        );
        // Garbage falls back to 24-hour.
        assert_eq!(ClockFormat::parse(""), ClockFormat::H24);
        assert_eq!(ClockFormat::parse("12|onlyone"), ClockFormat::H24);
    }

    #[test]
    fn render_time_of_day_24h() {
        let k = |h, m| meditate_core::format::TimeOfDayKey {
            hour: h,
            minute: m,
        };
        assert_eq!(render_time_of_day(k(0, 5), &ClockFormat::H24), "00:05");
        assert_eq!(render_time_of_day(k(13, 7), &ClockFormat::H24), "13:07");
    }

    #[test]
    fn render_time_of_day_12h_wraps_like_the_platform() {
        let fmt = ClockFormat::H12 { am: "AM".into(), pm: "PM".into() };
        let k = |h, m| meditate_core::format::TimeOfDayKey {
            hour: h,
            minute: m,
        };
        assert_eq!(render_time_of_day(k(0, 5), &fmt), "12:05 AM");
        assert_eq!(render_time_of_day(k(11, 59), &fmt), "11:59 AM");
        assert_eq!(render_time_of_day(k(12, 0), &fmt), "12:00 PM");
        assert_eq!(render_time_of_day(k(13, 7), &fmt), "1:07 PM");
        assert_eq!(render_time_of_day(k(23, 45), &fmt), "11:45 PM");
    }

    #[test]
    fn group_digits_inserts_separators_every_three() {
        assert_eq!(group_digits(0, "."), "0");
        assert_eq!(group_digits(999, "."), "999");
        assert_eq!(group_digits(1000, "."), "1.000");
        assert_eq!(group_digits(2875, "."), "2.875");
        assert_eq!(group_digits(1234567, " "), "1 234 567");
        assert_eq!(group_digits(-1000, "."), "-1.000");
        // Empty separator = no grouping (JNI fallback).
        assert_eq!(group_digits(123456, ""), "123456");
    }
}
