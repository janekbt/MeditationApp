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

/// The duration core says to save for the session that just ended:
/// `EndSession` (Stop, Finish, Add) or `EndBoxBreath` (a Box Breath
/// session reaching its cycle-aligned end). `None` when the effects
/// end nothing.
pub fn ended_duration_secs(effects: &[Effect]) -> Option<u64> {
    effects.iter().rev().find_map(|e| match e {
        Effect::EndSession { duration_secs } | Effect::EndBoxBreath { duration_secs } => {
            Some(*duration_secs)
        }
        _ => None,
    })
}

/// Interval-bell editor kind chip index ↔ core kind. Chip order:
/// Every N min, At N min, N min before end. An unknown index is
/// core's default kind.
pub fn bell_kind_index(kind: meditate_core::db::IntervalBellKind) -> i32 {
    use meditate_core::db::IntervalBellKind as K;
    match kind {
        K::Interval => 0,
        K::FixedFromStart => 1,
        K::FixedFromEnd => 2,
    }
}

pub fn bell_kind_from_index(index: i32) -> meditate_core::db::IntervalBellKind {
    use meditate_core::db::IntervalBellKind as K;
    match index {
        0 => K::Interval,
        1 => K::FixedFromStart,
        2 => K::FixedFromEnd,
        _ => meditate_core::bells::DEFAULT_NEW_BELL_KIND,
    }
}

/// The editor's minutes, clamped to core's bell range.
pub fn clamp_bell_minutes(value: i32) -> u32 {
    use meditate_core::bells::{BELL_MINUTES_MAX, BELL_MINUTES_MIN};
    u32::try_from(value).unwrap_or(0).clamp(BELL_MINUTES_MIN, BELL_MINUTES_MAX)
}

/// The editor's jitter %, clamped to core's range.
pub fn clamp_bell_jitter(value: i32) -> u32 {
    use meditate_core::bells::{BELL_JITTER_PCT_MAX, BELL_JITTER_PCT_MIN};
    u32::try_from(value).unwrap_or(0).clamp(BELL_JITTER_PCT_MIN, BELL_JITTER_PCT_MAX)
}

/// Player slot in `MeditateAudio.kt` for a bell on core's channel.
/// The numbers are the Kotlin `CHANNEL_*` constants.
pub fn bell_channel_slot(channel: meditate_core::session::FireChannel) -> i32 {
    use meditate_core::session::FireChannel;
    match channel {
        FireChannel::Starting => 0,
        FireChannel::End => 1,
        FireChannel::Interval => 2,
    }
}

/// The guided library file a session's recovery snapshot records:
/// the selected row's uuid for a Guided session, nothing for an
/// Open-File pick (no library row) or any other mode (a leftover
/// guided selection doesn't belong to it). Mirrors GTK's
/// `guided_selected_uuid` gate in `write_in_progress_snapshot`.
pub fn snapshot_guided_file(mode: TimerMode, selected_uuid: Option<&str>) -> Option<String> {
    match mode {
        TimerMode::Guided => selected_uuid.map(str::to_string),
        TimerMode::Timer | TimerMode::Breathing => None,
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
    pub fn stop(self, now: Duration) -> Transition {
        let mut t = self.state.stop(now);
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

    /// Start a fresh session from fully-built `SessionSettings`.
    /// The shell assembles these from the DB (interval bells,
    /// starting/end cue, per-mode signal-mode override, prep)
    /// exactly like GTK's `build_timer_settings`, so core gets
    /// the real cue config and emits the right `Fire*` / end
    /// effects. Core picks Prep or Running from `prep_secs`; the
    /// returned effects are what plays at the start (the starting
    /// bell when there is no prep, else it comes with the tick that
    /// ends prep) and must be dispatched like any other transition.
    pub fn start_session(settings: SessionSettings, now: Duration) -> Transition {
        let (session, start_effects) = Session::start(settings, now);
        Transition::new(Self::Active(Box::new(session)), start_effects)
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
    pub fn stop(self, now: Duration) -> Transition {
        match self {
            // Core's `Session::stop` emits `StopActiveSignals` (cut
            // any in-flight bell / vibration) and `EndSession` with
            // the duration to save — read via `ended_duration_secs`,
            // like GTK. No `FireEndBell`: Stop is silent, while a
            // natural countdown finish (FireEndBell from `tick`)
            // still rings.
            Self::Active(mut s) => {
                let effects = s.stop(now);
                Transition::new(Self::Finished, effects)
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

/// The bell a Setup-view Volume row edits, by the row's slot name in
/// `main.slint` ("start", "end", "in", "holdin", "out", "holdout").
/// `mode` is the Setup view's mode, whose end bell "end" means.
/// `None` for "interval": the bell editor stages that volume and saves
/// it with the bell.
pub fn volume_slot(
    name: &str,
    mode: meditate_core::SessionMode,
) -> Option<meditate_core::bell_volume::BellSlot> {
    use meditate_core::bell_volume::BellSlot;
    use meditate_core::db::BoxBreathPhaseId as P;
    Some(match name {
        "start" => BellSlot::Starting,
        "end" => BellSlot::End(mode),
        "in" => BellSlot::BoxBreathCue(P::In),
        "holdin" => BellSlot::BoxBreathCue(P::HoldIn),
        "out" => BellSlot::BoxBreathCue(P::Out),
        "holdout" => BellSlot::BoxBreathCue(P::HoldOut),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    // ── Volume rows ─────────────────────────────────────────────

    #[test]
    fn every_volume_row_edits_its_own_bell() {
        use meditate_core::bell_volume::BellSlot;
        use meditate_core::db::BoxBreathPhaseId as P;
        use meditate_core::SessionMode;
        assert_eq!(volume_slot("start", SessionMode::Timer), Some(BellSlot::Starting));
        for mode in [SessionMode::Timer, SessionMode::Guided, SessionMode::BoxBreath] {
            assert_eq!(volume_slot("end", mode), Some(BellSlot::End(mode)));
        }
        assert_eq!(volume_slot("in", SessionMode::BoxBreath), Some(BellSlot::BoxBreathCue(P::In)));
        assert_eq!(volume_slot("holdin", SessionMode::BoxBreath), Some(BellSlot::BoxBreathCue(P::HoldIn)));
        assert_eq!(volume_slot("out", SessionMode::BoxBreath), Some(BellSlot::BoxBreathCue(P::Out)));
        assert_eq!(volume_slot("holdout", SessionMode::BoxBreath), Some(BellSlot::BoxBreathCue(P::HoldOut)));
        assert_eq!(volume_slot("interval", SessionMode::Timer), None);
        assert_eq!(volume_slot("", SessionMode::Timer), None);
    }

    /// Every Volume row in the UI uses a slot name the mapping knows.
    #[test]
    fn the_ui_uses_only_known_volume_slots() {
        let slint = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("ui/main.slint"),
        )
        .unwrap();
        let mut names: Vec<&str> = slint
            .match_indices("root.bell-volume-released(\"")
            .map(|(at, m)| {
                let rest = &slint[at + m.len()..];
                &rest[..rest.find('"').unwrap()]
            })
            .collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names, ["end", "holdin", "holdout", "in", "interval", "out", "start"]);
        for name in names {
            assert!(
                name == "interval" || volume_slot(name, meditate_core::SessionMode::Timer).is_some(),
                "{name}",
            );
        }
    }

    /// Which bells may ring in which mode is decided in
    /// `meditate_core::bells`: the session builder takes the starting
    /// and interval bells from the core helpers, passing the mode,
    /// never from a literal "none" (issue #3: Timer bells rang in
    /// Guided and Box Breath because the mode never reached core).
    #[test]
    fn every_session_takes_its_bells_from_core_with_its_mode() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs");
        let source = std::fs::read_to_string(path).unwrap();
        for literal in ["bells: Vec::new()", "bells: vec![]", "starting_bell: None"] {
            assert!(!source.contains(literal), "found `{literal}`");
        }
        assert_eq!(source.matches("bells::session_bells_from_db(db, target, display, mode)").count(), 1);
        assert_eq!(source.matches("bells::starting_bell_cue_from_db(db, mode)").count(), 1);
        assert_eq!(source.matches("session_bells_from_db(").count(), 1);
        assert_eq!(source.matches("starting_bell_cue_from_db(").count(), 1);
    }

    use super::*;

    // ── start_session: what plays at the start ─────────────────

    fn timer_settings_with_starting_bell(prep_secs: Option<u32>) -> SessionSettings {
        SessionSettings {
            shape: SessionShape::TimerCountdown { target_secs: 600 },
            prep_secs,
            starting_bell: Some(meditate_core::bells::BellCue {
                sound_uuid: "start-sound".into(),
                vibration_pattern_uuid: "start-pattern".into(),
                signal_mode: meditate_core::db::SignalMode::Sound,
                volume: Default::default(),
            }),
            ..Default::default()
        }
    }

    fn starting_bells(effects: &[Effect]) -> usize {
        effects.iter().filter(|e| matches!(e, Effect::FireStartingBell { .. })).count()
    }

    #[test]
    fn starting_without_prep_rings_the_starting_bell_at_once() {
        let t = AppState::start_session(timer_settings_with_starting_bell(None), Duration::from_secs(100));
        assert!(t.is_running());
        assert_eq!(starting_bells(&t.effects), 1, "{:?}", t.effects);
    }

    #[test]
    fn starting_with_prep_rings_the_starting_bell_when_prep_ends() {
        let t0 = Duration::from_secs(100);
        let t = AppState::start_session(timer_settings_with_starting_bell(Some(10)), t0);
        assert_eq!(starting_bells(&t.effects), 0, "{:?}", t.effects);
        let AppState::Active(mut session) = t.state else { panic!("not started") };
        let mut later = Vec::new();
        for secs in 1..=10 {
            later.extend(session.tick(t0 + Duration::from_secs(secs)));
        }
        assert_eq!(starting_bells(&later), 1, "{later:?}");
    }

    #[test]
    fn starting_with_no_starting_bell_plays_nothing() {
        for prep in [None, Some(10)] {
            let settings = SessionSettings {
                starting_bell: None,
                ..timer_settings_with_starting_bell(prep)
            };
            let t = AppState::start_session(settings, Duration::from_secs(100));
            assert!(t.effects.is_empty(), "prep {prep:?}: {:?}", t.effects);
        }
    }

    /// The start effects must reach the phone: the shell dispatches
    /// the effects of the transition `start_session` returns.
    #[test]
    fn the_shell_dispatches_the_start_effects() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs");
        let lib = std::fs::read_to_string(path).unwrap();
        assert_eq!(lib.matches("AppState::start_session(").count(), 1);
        let call = lib.find("AppState::start_session(").unwrap();
        let bound = lib[..call].rfind("let transition = ").expect("start feeds `transition`");
        let dispatch = call
            + lib[call..].find("dispatch_effects(&transition.effects)").expect("dispatched");
        let between = &lib[bound..dispatch];
        assert_eq!(between.matches("let transition = ").count(), 1, "same `transition`");
        let app = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/app.rs"),
        )
        .unwrap();
        let code = app.split("#[cfg(test)]\nmod tests").next().unwrap();
        assert!(!code.contains("start_signals"), "use Session::start");
        assert_eq!(code.matches("Session::start(").count(), 1);
    }

    /// An ended session stays recoverable until Save or Discard: every
    /// end path holds the snapshot with the final duration, and only
    /// the three ways off the Done screen (Save, Discard, Back) clear
    /// it. Clearing at the end lost the session when Android killed
    /// the app on the Done screen.
    /// A sync request that arrives while a sync runs must get one
    /// more pass, not be dropped. Core's `SyncCoordinator` owns that
    /// rule (GTK uses it too); the shell only spawns the worker.
    #[test]
    fn sync_requests_go_through_the_core_coordinator() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs");
        let lib = std::fs::read_to_string(path).unwrap();
        let code = lib.split("#[cfg(test)]\nmod tests").next().unwrap();
        assert!(!code.contains("SYNC_IN_FLIGHT"), "the old drop-if-busy flag is gone");
        assert!(
            code.contains("static SYNC_COORDINATOR: meditate_core::sync::coordinator::SyncCoordinator"),
            "one coordinator for the whole app",
        );
        let trigger = code.find("fn trigger_sync(").expect("trigger_sync");
        let body = &code[trigger..trigger + code[trigger..].find("\n}\n").unwrap()];
        assert!(body.contains("SYNC_COORDINATOR.request()"), "requests go to the coordinator");
        assert!(body.contains("SYNC_COORDINATOR.drain("), "the worker runs core's loop");
        for step in ["start_pass()", "should_run_again_after_pass()", ".release()"] {
            assert!(!body.contains(step), "the shell runs the loop step {step} itself");
        }
        // The spinner reads the same coordinator, so the "done" edge
        // must come after drain has freed the slot, not inside a pass
        // (that left the indicator spinning forever).
        assert!(code.contains("SYNC_COORDINATOR.is_in_flight()"));
        let drain = body.find("SYNC_COORDINATOR.drain(").unwrap();
        let after = &body[drain..];
        let drain_end = after.find("\n        });").expect("drain call closes inside the worker");
        assert!(
            !after[..drain_end].contains("SYNC_UI_DIRTY"),
            "the indicator is flagged inside a pass, while the slot is still taken",
        );
        assert!(after[drain_end..].contains("SYNC_UI_DIRTY.store(true"), "flag the indicator after drain");
    }

    /// When a sync brings in changes from another device, every
    /// screen that shows synced data re-reads it, whichever page is
    /// open. Core decides whether anything came in
    /// (`SyncStats::brought_changes`); the shell re-reads.
    #[test]
    fn screens_re_read_after_a_sync_brings_changes() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs");
        let lib = std::fs::read_to_string(path).unwrap();
        let code = lib.split("#[cfg(test)]\nmod tests").next().unwrap();
        let body_of = |sig: &str| {
            let at = code.find(sig).unwrap_or_else(|| panic!("missing {sig}"));
            &code[at..at + code[at..].find("\n}\n").unwrap()]
        };

        // The worker flags a pass that brought changes.
        let trigger = body_of("fn trigger_sync(");
        let drain = &trigger[trigger.find("SYNC_COORDINATOR.drain(").unwrap()..];
        let drain = &drain[..drain.find("\n        });").unwrap()];
        assert!(drain.contains(".brought_changes()"), "core decides what counts as a change");
        assert!(drain.contains("SYNC_PULLED_CHANGES.store(true"), "flag it per pass, so the screens update as soon as it lands");

        // One re-read list covering every synced surface.
        let refresh = body_of("fn refresh_after_pull(");
        for call in [
            "refresh_preset_chips(",
            "refresh_bell_rows(",
            "populate_interval_bells(",
            "refresh_guided_files(",
            "refresh_guided_manage(",
            "refresh_label_state(",
            "set_label_active(",
            "refresh_filter_label_items(",
            "reset_log_feed(",
            "refresh_stats(",
            "refresh_widget(",
        ] {
            assert!(refresh.contains(call), "refresh_after_pull misses {call}");
        }
        assert!(!refresh.contains("trigger_sync("), "re-reading must not start another sync");

        // The tick loop consumes the flag and re-reads.
        let flag = code.find("SYNC_PULLED_CHANGES\n").or_else(|| code.find("SYNC_PULLED_CHANGES.swap(false"))
            .expect("the tick loop reads the flag");
        let next = &code[flag..flag + 400];
        assert!(next.contains("swap(false"), "the flag is consumed once");
        assert!(next.contains("refresh_after_pull("), "and the screens re-read");

        // Wipe-local uses the same list: it re-reads now (empty) and
        // again when its sync brings the data back.
        let wipe = &code[code.find("ui.on_recovery_wipe_confirm_tap(").unwrap()..];
        let wipe = &wipe[..wipe.find("\n        });").unwrap()];
        assert!(wipe.contains("refresh_after_pull("), "wipe-local re-reads through the shared list");
    }

    /// Every local change that needs syncing starts a sync: core
    /// counts them where they are recorded, and the tick loop starts
    /// a sync when the count moves. Write paths don't call
    /// trigger_sync themselves, so none can forget it; only triggers
    /// that aren't a synced write stay explicit.
    #[test]
    fn local_changes_start_a_sync_from_one_place() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs");
        let lib = std::fs::read_to_string(path).unwrap();
        let code = lib.split("#[cfg(test)]\nmod tests").next().unwrap();

        assert!(code.contains("fn local_change_watch("), "one watch over core's count");
        let take = code.find(".take_new()").expect("the tick loop asks the watch");
        let after = &code[take..take + 300];
        assert!(after.contains("trigger_sync(\"local change\")"), "and starts a sync");

        let mut reasons: Vec<&str> = code
            .match_indices("trigger_sync(\"")
            .map(|(i, m)| {
                let rest = &code[i + m.len()..];
                &rest[..rest.find('"').unwrap()]
            })
            .collect();
        reasons.sort_unstable();
        assert_eq!(
            reasons,
            [
                "app resume",            // launch and every return to the app
                "indicator tap (retry)", // retry after a failed sync
                "local change",          // every synced write
                "prefs save",            // new account; not a synced write
                "recovery push-local",   // re-queues events, records none
                "recovery wipe-local",   // pull everything back
            ],
            "write paths must not trigger sync themselves",
        );
    }

    /// Coming back to the app pulls what other devices wrote in the
    /// meantime, like GTK syncing on every activation. Android
    /// reports a resume at launch too, so one trigger covers both.
    #[test]
    fn the_app_syncs_whenever_it_comes_to_the_foreground() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs");
        let lib = std::fs::read_to_string(path).unwrap();
        let code = lib.split("#[cfg(test)]\nmod tests").next().unwrap();

        assert!(!code.contains("slint::android::init(android_app)"), "init must listen to lifecycle events");
        let init = code.find("slint::android::init_with_event_listener(").expect("listener init");
        let listener = &code[init..init + 600];
        let resume = listener.find("MainEvent::Resume").expect("reacts to resume");
        assert!(listener[resume..].contains("trigger_sync(\"app resume\")"), "a resume starts a sync");
        assert!(!code.contains("trigger_sync(\"app launch\")"), "launch is a resume; no second trigger");
    }

    /// A guided session starts only if its track starts: the player
    /// reports failure instead of faking the track's end, the shell
    /// starts the track before the session (like GTK), and a failure
    /// shows "Couldn't start playback" with no session at all.
    #[test]
    fn a_guided_file_that_will_not_play_starts_no_session() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let kotlin = std::fs::read_to_string(root.join("kotlin/MeditateGuided.kt")).unwrap();
        let start = kotlin.find("fun startAudio(").expect("startAudio");
        let start_fn = &kotlin[start..start + kotlin[start..].find("\n    }\n").unwrap()];
        assert!(start_fn.starts_with("fun startAudio(context: Context, path: String): Boolean"),
            "the player reports whether the track started");
        let catch = &start_fn[start_fn.find("catch (e: Exception)").expect("failure branch")..];
        assert!(!catch.contains("markEos"), "a failed start must not pretend the track ended");

        let guided = std::fs::read_to_string(root.join("src/guided.rs")).unwrap();
        assert!(guided.contains("\"(Landroid/content/Context;Ljava/lang/String;)Z\""), "JNI reads the result");
        assert!(guided.contains("pub fn play(app: &AndroidApp, path: &str) -> bool"));

        let lib = std::fs::read_to_string(root.join("src/lib.rs")).unwrap();
        let code = lib.split("#[cfg(test)]\nmod tests").next().unwrap();
        let first_play = code.find("guided::play(app, &sel.path)").expect("session start plays the track");
        let session_start = code.find("AppState::start_session(").expect("session start");
        assert!(first_play < session_start, "the track starts before the session");
        let gate = &code[first_play..session_start];
        assert!(gate.contains("GUIDED_START_FAILED.store(true"), "a failure is flagged");
        assert!(gate.contains("return;"), "and nothing starts");
        assert_eq!(code.matches("guided::play(app, &sel.path)").count(), 1, "no second start after the session began");

        let flag = code.find("GUIDED_START_FAILED.swap(false").expect("the tick loop shows it");
        assert!(code[flag..flag + 300].contains("invoke_playback_failed()"));
        let slint = std::fs::read_to_string(root.join("ui/main.slint")).unwrap();
        assert!(slint.contains("public pure function playback-failed() -> string { return @tr(\"Couldn't start playback\"); }"));
    }

    #[test]
    fn a_guided_session_records_its_library_file() {
        assert_eq!(snapshot_guided_file(TimerMode::Guided, Some("gf-1")), Some("gf-1".to_string()));
    }

    #[test]
    fn an_open_file_pick_records_no_library_file() {
        assert_eq!(snapshot_guided_file(TimerMode::Guided, None), None);
    }

    #[test]
    fn other_modes_ignore_a_leftover_guided_selection() {
        assert_eq!(snapshot_guided_file(TimerMode::Timer, Some("gf-1")), None);
        assert_eq!(snapshot_guided_file(TimerMode::Breathing, Some("gf-1")), None);
    }

    /// A session killed in its first minute must still be recovered,
    /// and a recovered guided session keeps its file (GTK writes the
    /// snapshot at start and records the file).
    #[test]
    fn the_recovery_snapshot_is_written_at_start_and_names_the_guided_file() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs");
        let lib = std::fs::read_to_string(path).unwrap();
        let code = lib.split("#[cfg(test)]\nmod tests").next().unwrap();

        let builder = code.find("fn session_in_progress_snapshot(").unwrap();
        let builder = &code[builder..builder + code[builder..].find("\n}\n").unwrap()];
        assert!(!builder.contains("guided_file_uuid: None"), "the guided file was always dropped");
        assert!(builder.contains("session_guided_file()"), "every snapshot reads the session's file");

        let start = code.find("session_start_unix.set(Some(meditate_core::time::unix_now()));").expect("start edge");
        let after = &code[start..];
        let heartbeat = after.find("start_snapshot_heartbeat(").expect("heartbeat");
        let edge = &after[..heartbeat];
        assert!(edge.contains("set_session_guided_file(app::snapshot_guided_file("), "the file is fixed at start");
        assert!(edge.contains("write_session_in_progress_snapshot("), "first snapshot at start, not after 60 s");
        assert!(
            edge.find("set_session_guided_file(").unwrap() < edge.find("write_session_in_progress_snapshot(").unwrap(),
            "the first snapshot already names the file",
        );
    }

    /// Leaving the Done screen silences a bell or vibration that is
    /// still going (mostly a Box Breath natural end), like GTK's
    /// `stop_active_signals` in on_save / on_discard.
    #[test]
    fn leaving_the_done_screen_stops_ringing_signals() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs");
        let lib = std::fs::read_to_string(path).unwrap();
        let code = lib.split("#[cfg(test)]\nmod tests").next().unwrap();

        let helper = code.find("fn stop_active_signals()").expect("one stop helper");
        let helper = &code[helper..helper + code[helper..].find("\n}\n").unwrap()];
        assert!(helper.contains("Effect::StopActiveSignals"), "the same stop Stop/Finish use");

        for exit in ["ui.on_save_tap(", "ui.on_discard_tap(", "if ui.get_done_page() {"] {
            let at = code.find(exit).unwrap_or_else(|| panic!("missing {exit}"));
            let body = &code[at..at + 1500];
            assert!(body.contains("stop_active_signals();"), "{exit} must silence ringing signals");
        }
    }

    /// The guided track stops on core's `StopGuidedAudio`, which
    /// comes before the end bell when the session enters Overtime.
    #[test]
    fn guided_track_stops_on_cores_signal() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs");
        let lib = std::fs::read_to_string(path).unwrap();
        let code = lib.split("#[cfg(test)]\nmod tests").next().unwrap();
        let dispatch = code.find("fn dispatch_effects(").expect("dispatcher");
        let dispatch = &code[dispatch..dispatch + code[dispatch..].find("\n}\n").unwrap()];
        let arm = dispatch.find("Effect::StopGuidedAudio").expect("dispatcher handles it");
        let fire = dispatch.find("fire_route()").unwrap();
        assert!(arm < fire, "handled before the loop moves on to bells");
        assert!(dispatch[arm..fire].contains("guided::stop(app)"), "and stops the track");
    }

    #[test]
    fn each_bell_channel_has_its_own_player_slot() {
        use meditate_core::session::FireChannel;
        let slots = [
            bell_channel_slot(FireChannel::Starting),
            bell_channel_slot(FireChannel::End),
            bell_channel_slot(FireChannel::Interval),
        ];
        assert_eq!(slots, [0, 1, 2]);
    }

    /// Bells no longer cut each other off: each rings on its core
    /// channel's slot (Starting and End replace only themselves,
    /// Interval stacks), like GTK's sound.rs; the alarm volume goes
    /// back only once every slot is quiet.
    #[test]
    fn bells_ring_on_their_own_channels() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let lib = std::fs::read_to_string(root.join("src/lib.rs")).unwrap();
        let code = lib.split("#[cfg(test)]\nmod tests").next().unwrap();
        let dispatch = code.find("fn dispatch_effects(").unwrap();
        let dispatch = &code[dispatch..dispatch + code[dispatch..].find("\n}\n").unwrap()];
        let sound = &dispatch[dispatch.find("route.signal_mode.includes_sound()").unwrap()..];
        let sound = &sound[..sound.find("route.signal_mode.includes_vibration()").unwrap()];
        assert!(!sound.contains("audio::stop("), "a bell must not stop the others");
        assert!(sound.contains("audio::play_bell(app, app::bell_channel_slot(route.channel),"));

        let kotlin = std::fs::read_to_string(root.join("kotlin/MeditateAudio.kt")).unwrap();
        for (name, slot) in [("CHANNEL_STARTING", 0), ("CHANNEL_END", 1), ("CHANNEL_INTERVAL", 2)] {
            assert!(kotlin.contains(&format!("const val {name} = {slot}")), "{name} must match bell_channel_slot");
        }
        assert!(kotlin.contains("fun playBell(context: Context, channel: Int, path: String, gain: Float)"));
        assert!(kotlin.contains("private val intervals = mutableListOf<MediaPlayer>()"), "interval bells stack");
        let preview = kotlin.find("fun play(context: Context, path: String, gain: Float): Long").unwrap();
        let preview = &kotlin[preview..preview + kotlin[preview..].find("\n    }\n").unwrap()];
        assert!(preview.contains("releasePreviewLocked()") && !preview.contains("releaseAllLocked"),
            "a preview replaces only the preview");
        let restore = kotlin.find("private fun restoreIfIdleLocked(").expect("restore only when all quiet");
        assert!(kotlin[restore..restore + 200].contains("if (isIdleLocked())"));
        let idle = kotlin.find("private fun isIdleLocked()").unwrap();
        let idle = &kotlin[idle..idle + kotlin[idle..].find("\n\n").unwrap()];
        for slot in ["preview == null", "starting == null", "end == null", "intervals.isEmpty()"] {
            assert!(idle.contains(slot), "alarm volume must wait for {slot}");
        }
    }

    /// A failed save keeps the session (Done screen, pending pair and
    /// recovery snapshot) and says so, like GTK's "Couldn't save
    /// session" toasts. Before, the snapshot was cleared anyway and
    /// the session was gone without a word.
    #[test]
    fn a_failed_save_keeps_the_session_and_says_so() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let lib = std::fs::read_to_string(root.join("src/lib.rs")).unwrap();
        let code = lib.split("#[cfg(test)]\nmod tests").next().unwrap();

        let finalize = code.find("fn finalize_session(").unwrap();
        let sig = &code[finalize..finalize + code[finalize..].find('{').unwrap()];
        assert!(sig.contains("-> Result<(), meditate_core::format::SessionSaveFailureKind>"), "{sig}");
        let body = &code[finalize..finalize + code[finalize..].find("\n}\n").unwrap()];
        assert!(body.contains("session_save_failure_log_message("), "log through core's message");

        let save = code.find("ui.on_save_tap(").unwrap();
        let save = &code[save..save + code[save..].find("\n        });").unwrap()];
        let call = save.find("match finalize_session(").expect("the result is checked");
        let after = &save[call..];
        let ok = after.find("Ok(()) =>").expect("success branch");
        let err = after.find("Err(kind) =>").expect("failure branch");
        assert!(ok < err);
        let err_arm = &after[err..];
        let err_arm = &err_arm[..err_arm.find("return;").expect("a failed save leaves the Done screen up") + 7];
        assert!(err_arm.contains("pending_done.set(Some((unix_start, elapsed_secs)))"), "the session is kept");
        assert!(err_arm.contains("SESSION_SAVE_FAILED"), "and the failure is shown");
        assert!(!err_arm.contains("clear_session_in_progress_snapshot"), "the snapshot survives");
        assert!(after[ok..err].contains("clear_session_in_progress_snapshot();"), "cleared only once saved");

        let shown = code.find("SESSION_SAVE_FAILED.lock()").expect("the tick loop shows it");
        let shown = &code[shown..shown + 700];
        assert!(shown.contains("invoke_save_failed_storage()") && shown.contains("invoke_save_failed_unavailable()"));
        let slint = std::fs::read_to_string(root.join("ui/main.slint")).unwrap();
        assert!(slint.contains("@tr(\"Couldn't save session — storage error\")"));
        assert!(slint.contains("@tr(\"Couldn't save session — storage unavailable\")"));
    }

    /// A preset that can't be applied (its bell sound or pattern hasn't
    /// synced yet) says so instead of doing nothing — chip, widget and
    /// Undo alike, as they all go through apply_preset_json. GTK shows
    /// the same message.
    #[test]
    fn a_preset_that_cannot_apply_says_so() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let lib = std::fs::read_to_string(root.join("src/lib.rs")).unwrap();
        let code = lib.split("#[cfg(test)]\nmod tests").next().unwrap();

        let outer = code.find("fn apply_preset_json(").unwrap();
        let outer = &code[outer..outer + code[outer..].find("\n}\n").unwrap()];
        assert!(outer.contains("apply_preset_config_json("), "one wrapper around the real apply");
        assert!(outer.contains("PRESET_APPLY_FAILED.store(true"), "every failure is flagged");
        assert_eq!(code.matches("apply_preset_config_json(").count(), 2, "only the wrapper calls it");

        let shown = code.find("PRESET_APPLY_FAILED.swap(false").expect("the tick loop shows it");
        assert!(code[shown..shown + 300].contains("invoke_preset_sync_pending()"));
        let slint = std::fs::read_to_string(root.join("ui/main.slint")).unwrap();
        assert!(slint.contains("@tr(\"Please wait until fully synced — not all bell sounds have arrived\")"));
    }

    /// The label toggle goes through core's set_active_for_mode, which
    /// adopts the mode's default label the first time it's turned on
    /// (as on GTK), so a preset saved then pins the label.
    #[test]
    fn label_toggle_uses_cores_rule() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs");
        let lib = std::fs::read_to_string(path).unwrap();
        let code = lib.split("#[cfg(test)]\nmod tests").next().unwrap();
        let write = code.find("fn write_label_active_for_mode(").unwrap();
        let write = &code[write..write + code[write..].find("\n}\n").unwrap()];
        assert!(write.contains("meditate_core::labels::set_active_for_mode("));
        assert!(!write.contains("persist_active_for_mode("), "not the bare toggle write");
    }

    /// A picked bell file over core's 10 MB cap is refused before the
    /// import dialog, like GTK — sync would never upload it. The
    /// transient copy goes too.
    #[test]
    fn an_oversized_bell_file_is_refused() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let lib = std::fs::read_to_string(root.join("src/lib.rs")).unwrap();
        let code = lib.split("#[cfg(test)]\nmod tests").next().unwrap();
        let pick = code.find("guided::take_pending_sound_pick(app)").unwrap();
        let arm = &code[pick..pick + code[pick..].find("Some(Err(e)) =>").unwrap()];
        let check = arm.find("meditate_core::sound::is_within_size_limit(").expect("size checked");
        let dialog = arm.find("present_guided_import_dialog(").unwrap();
        assert!(check < dialog, "before the import dialog");
        let refused = &arm[arm[check..].find("} else {").unwrap() + check..];
        assert!(refused.contains("std::fs::remove_file(&pick.path)"), "the copy is removed");
        assert!(refused.contains("invoke_file_too_large()"), "and the user is told");
        let slint = std::fs::read_to_string(root.join("ui/main.slint")).unwrap();
        assert!(slint.contains("@tr(\"File is larger than 10 MB\")"));
    }

    /// A sound or pattern whose row is gone shows "Missing" (core's
    /// ResolvedName::Missing), like GTK, instead of a blank name —
    /// on the bell rows, the interval editor and the box-breath cues.
    #[test]
    fn a_deleted_sound_or_pattern_shows_missing() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let lib = std::fs::read_to_string(root.join("src/lib.rs")).unwrap();
        let code = lib.split("#[cfg(test)]\nmod tests").next().unwrap();
        let text = code.find("fn resolved_name_text(").expect("one mapping");
        let text = &code[text..text + code[text..].find("\n}\n").unwrap()];
        assert!(text.contains("ResolvedName::Missing => ui.global::<Tr>().invoke_missing()"));
        assert!(!code.contains("ResolvedName::Missing => String::new()"), "no blank names left");
        for f in ["fn bell_sound_name(", "fn pattern_name("] {
            let body = code.find(f).unwrap();
            let body = &code[body..body + code[body..].find("\n}\n").unwrap()];
            assert!(body.contains("resolved_name_text(ui,"), "{f} goes through core's resolver");
            assert!(!body.contains("unwrap_or_default()"), "{f}: no silent blank");
        }
        let slint = std::fs::read_to_string(root.join("ui/main.slint")).unwrap();
        assert!(slint.contains("public pure function missing() -> string { return @tr(\"Missing\"); }"));
    }

    /// The Stopwatch switch drives the End Bell row (off and greyed)
    /// and the Manage Bells count (before-end bells drop out) through
    /// core, like GTK; flipping it refreshes both, and greying the row
    /// never overwrites the stored End Bell choice.
    #[test]
    fn stopwatch_drives_the_end_bell_row_and_the_bell_count() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let lib = std::fs::read_to_string(root.join("src/lib.rs")).unwrap();
        let code = lib.split("#[cfg(test)]\nmod tests").next().unwrap();
        let body = |f: &str| {
            let at = code.find(f).unwrap_or_else(|| panic!("{f}"));
            &code[at..at + code[at..].find("\n}\n").unwrap()]
        };

        assert!(body("fn end_bell_row(").contains("meditate_core::bells::end_bell_row_state("), "row state from core");
        let rows = body("fn refresh_bell_rows(");
        assert!(rows.contains("end_bell_row(eb_mode)"));
        assert!(rows.contains("set_end_bell_sensitive(row.sensitive)"));
        assert!(rows.contains("set_end_bell_active(row.active)"));

        let count = body("fn interval_bells_summary(");
        assert!(count.contains("meditate_core::bells::display_mode_from_db(db,"), "count follows Stopwatch");
        assert!(!count.contains("DisplayMode::Countdown"));

        let sw = code.find("ui.on_stopwatch_toggled(").unwrap();
        assert!(code[sw..sw + 600].contains("refresh_bell_rows(&ui)"), "flipping Stopwatch refreshes both");

        let eb = code.find("ui.on_end_bell_toggled(").unwrap();
        let eb = &code[eb..eb + 900];
        let guard = eb.find("end_bell_row_sensitive(").expect("ignores the greyed row");
        assert!(guard < eb.find("write_global_setting(").unwrap(), "before the write");

        let slint = std::fs::read_to_string(root.join("ui/main.slint")).unwrap();
        assert_eq!(slint.matches("enabled: root.end-bell-sensitive;").count(), 3, "all three End Bell rows");
        let group = slint.find("component ExpanderGroup").unwrap();
        let group = &slint[group..group + 3500];
        assert!(group.contains("in property <bool> enabled: true;"));
        assert!(group.contains("enabled: root.enabled;"), "the switch greys out");
    }

    /// Every export failure says "Export failed": writing the temp CSV
    /// and opening the save dialog used to fail with only a log line
    /// (the copy-out step already showed it).
    #[test]
    fn every_export_failure_says_so() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let lib = std::fs::read_to_string(root.join("src/lib.rs")).unwrap();
        let code = lib.split("#[cfg(test)]\nmod tests").next().unwrap();
        let tap = code.find("ui.on_data_export_tap(").unwrap();
        let tap = &code[tap..tap + code[tap..].find("\n        });").unwrap()];
        assert!(tap.contains("if !started_export(") , "one outcome for the whole flow");
        assert!(tap.contains("EXPORT_FAILED.store(true"), "a failure is flagged");
        let started = code.find("fn started_export(").expect("helper");
        let started = &code[started..started + code[started..].find("\n}\n").unwrap()];
        assert!(started.contains("guided::open_export(") && started.contains("export_csv("));

        let guided = std::fs::read_to_string(root.join("src/guided.rs")).unwrap();
        assert!(guided.contains("pub fn open_export(app: &AndroidApp, src_path: &str, suggested: &str) -> bool"));

        let shown = code.find("EXPORT_FAILED.swap(false").expect("the tick loop shows it");
        assert!(code[shown..shown + 300].contains("invoke_export_failed()"));
    }

    // ── Interval-bell editor takes core's defaults and limits ───

    #[test]
    fn bell_kind_index_round_trips() {
        use meditate_core::db::IntervalBellKind as K;
        for kind in [K::Interval, K::FixedFromStart, K::FixedFromEnd] {
            assert_eq!(bell_kind_from_index(bell_kind_index(kind)), kind);
        }
        assert_eq!(bell_kind_index(K::Interval), 0, "the editor's chip order");
        assert_eq!(bell_kind_index(K::FixedFromEnd), 2);
    }

    #[test]
    fn an_unknown_kind_index_is_cores_default_kind() {
        assert_eq!(bell_kind_from_index(-1), meditate_core::bells::DEFAULT_NEW_BELL_KIND);
        assert_eq!(bell_kind_from_index(9), meditate_core::bells::DEFAULT_NEW_BELL_KIND);
    }

    #[test]
    fn bell_minutes_clamp_to_cores_range() {
        use meditate_core::bells::{BELL_MINUTES_MAX, BELL_MINUTES_MIN};
        assert_eq!(clamp_bell_minutes(0), BELL_MINUTES_MIN);
        assert_eq!(clamp_bell_minutes(-5), BELL_MINUTES_MIN);
        assert_eq!(clamp_bell_minutes(30), 30);
        assert_eq!(clamp_bell_minutes(10_000), BELL_MINUTES_MAX);
    }

    #[test]
    fn bell_jitter_clamps_to_cores_range() {
        use meditate_core::bells::{BELL_JITTER_PCT_MAX, BELL_JITTER_PCT_MIN};
        assert_eq!(clamp_bell_jitter(-1), BELL_JITTER_PCT_MIN);
        assert_eq!(clamp_bell_jitter(20), 20);
        assert_eq!(clamp_bell_jitter(99), BELL_JITTER_PCT_MAX);
    }

    /// No defaults or limits are copied by hand: Rust fills the
    /// editor from core's constants, the steppers' ranges included.
    #[test]
    fn the_interval_editor_takes_cores_values() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let lib = std::fs::read_to_string(root.join("src/lib.rs")).unwrap();
        let code = lib.split("#[cfg(test)]\nmod tests").next().unwrap();
        let create = code.find("ui.on_create_interval_bell_tap(").unwrap();
        let create = &code[create..create + 2000];
        for want in [
            "DEFAULT_NEW_BELL_KIND", "DEFAULT_NEW_BELL_MINUTES",
            "DEFAULT_NEW_BELL_JITTER_PCT", "DEFAULT_NEW_BELL_SIGNAL_MODE",
        ] {
            assert!(create.contains(want), "new bell uses {want}");
        }
        for literal in ["set_ie_kind(0)", "set_ie_minutes(5)", "set_ie_jitter(0)", "set_ie_signal_mode(0)"] {
            assert!(!create.contains(literal), "{literal} copies a core default");
        }
        assert!(!code.contains(".min(120)") && !code.contains(".min(50)"), "save clamps through core");
        assert!(code.contains("app::clamp_bell_minutes(ui.get_ie_minutes())"));
        assert!(code.contains("app::clamp_bell_jitter(ui.get_ie_jitter())"));
        for setter in ["set_ie_minutes_min(", "set_ie_minutes_max(", "set_ie_jitter_min(", "set_ie_jitter_max("] {
            assert!(code.contains(setter), "{setter} from core");
        }
        let slint = std::fs::read_to_string(root.join("ui/main.slint")).unwrap();
        assert!(slint.contains("min-value: root.ie-minutes-min;") && slint.contains("max-value: root.ie-minutes-max;"));
        assert!(slint.contains("min-value: root.ie-jitter-min;") && slint.contains("max-value: root.ie-jitter-max;"));
    }

    #[test]
    fn the_shell_holds_an_ended_session_until_save_or_discard() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs");
        let lib = std::fs::read_to_string(path).unwrap();
        let code = lib.split("#[cfg(test)]\nmod tests").next().unwrap();
        // The hold helper itself clears when there is nothing to keep;
        // leave its body out so only call sites are counted.
        let helper = code.find("fn hold_ended_session_snapshot(").expect("hold helper");
        let helper_end = helper + code[helper..].find("\n}\n").unwrap();
        let code = format!("{}{}", &code[..helper], &code[helper_end..]);
        let code = code.as_str();

        // Each end path stops the heartbeat and holds the snapshot.
        let ends: Vec<usize> = code.match_indices("snapshot_timer_ref.stop();").map(|(i, _)| i).collect();
        assert_eq!(ends.len(), 4, "Stop, Finish, Add and the natural end");
        for end in ends {
            let next = code[end..].lines().nth(1).unwrap().trim();
            assert!(
                next.starts_with("hold_ended_session_snapshot("),
                "an end path must hold the snapshot, found {next:?}",
            );
        }

        // Only the Done-screen exits clear it.
        let exits = ["ui.on_save_tap(", "ui.on_discard_tap(", "if ui.get_done_page() {"];
        let clears: Vec<usize> = code
            .match_indices("clear_session_in_progress_snapshot();")
            .map(|(i, _)| i)
            .collect();
        assert_eq!(clears.len(), exits.len(), "one clear per Done-screen exit");
        for exit in exits {
            let start = code.find(exit).unwrap_or_else(|| panic!("{exit} not found"));
            assert_eq!(code.matches(exit).count(), 1, "{exit} is unique");
            let nearest = clears
                .iter()
                .filter(|&&c| c > start)
                .min()
                .unwrap_or_else(|| panic!("{exit} must clear the snapshot"));
            let between = &code[start..*nearest];
            assert!(
                exits.iter().all(|e| !between[1..].contains(e)),
                "{exit} must clear the snapshot before the next exit handler",
            );
        }
    }

    /// Building a snapshot looks up the label, which takes the DB lock;
    /// the writers must build it before taking the lock themselves, or
    /// the re-lock freezes the app (it did, at Stop and at the first
    /// heartbeat).
    #[test]
    fn snapshot_writers_build_the_snapshot_before_locking_the_db() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs");
        let lib = std::fs::read_to_string(path).unwrap();
        for writer in ["fn write_session_in_progress_snapshot(", "fn hold_ended_session_snapshot("] {
            let start = lib.find(writer).unwrap_or_else(|| panic!("{writer} not found"));
            let body = &lib[start..start + lib[start..].find("\n}\n").unwrap()];
            let build = body
                .find("session_in_progress_snapshot(unix_start")
                .unwrap_or_else(|| panic!("{writer} builds a snapshot"));
            let lock = body.find("db_arc.lock()").unwrap_or_else(|| panic!("{writer} locks"));
            assert!(build < lock, "{writer} must build the snapshot before locking the DB");
        }
    }

    /// A row's stored `file_path` is the path on the device that created
    /// it; sync copies it everywhere (even onto the bundled bells), so
    /// the shell never reads it. Files are found through
    /// `meditate_core::audio_files` instead.
    #[test]
    fn no_shell_code_reads_the_stored_file_path() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut checked = 0;
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_none_or(|e| e != "rs") {
                continue;
            }
            let src = std::fs::read_to_string(&path).unwrap();
            let code = src.split("#[cfg(test)]\nmod tests").next().unwrap();
            for line in code.lines().filter(|l| !l.trim_start().starts_with("//")) {
                assert!(
                    !line.contains(".file_path"),
                    "{} reads the stored file_path: {}",
                    path.display(),
                    line.trim(),
                );
            }
            checked += 1;
        }
        assert!(checked > 10, "walked the shell sources");
    }

    /// Picked dates and Insight Timer rows go through core's
    /// `local_naive_to_unix` (via `insighttimer_started_at` for the
    /// import): chrono's `.single()` / `.earliest()` return None in a
    /// DST gap, which turned an edited start into "now" and aborted
    /// imports.
    #[test]
    fn local_times_are_converted_by_core() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs");
        let lib = std::fs::read_to_string(path).unwrap();
        let code = lib.split("#[cfg(test)]\nmod tests").next().unwrap();
        for own in ["with_ymd_and_hms(", "from_local_datetime("] {
            assert!(!code.contains(own), "shell converts local time itself: {own}");
        }
        assert!(code.contains("meditate_core::data_io::insighttimer_started_at"));
        assert_eq!(code.matches("meditate_core::time::local_naive_to_unix").count(), 1, "edit/add save");
    }

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
    fn hero_label_in_overtime_stays_at_the_planned_length() {
        // Was 00:00: the countdown clamped at zero.
        let s = AppState::idle()
            .toggle(timer_countdown(Duration::from_secs(60)), Duration::from_secs(100))
            .tick(Duration::from_secs(160));
        assert_eq!(s.hero_label(Duration::ZERO, Duration::from_secs(190), false), "01:00");
    }

    #[test]
    fn hero_label_in_guided_overtime_stops_counting_up() {
        let shape = SessionShape::Guided { duration_secs: 60, count_up_display: true };
        let s = AppState::idle().toggle(shape, Duration::from_secs(100)).tick(Duration::from_secs(160));
        assert_eq!(s.hero_label(Duration::ZERO, Duration::from_secs(190), true), "01:00");
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
        let t = AppState::idle().stop(Duration::from_secs(100));
        assert!(t.is_idle());
        assert!(t.effects.is_empty());
    }

    #[test]
    fn stop_from_running_advances_to_finished() {
        let s = AppState::idle()
            .toggle(timer_countdown(ten_minutes()), Duration::from_secs(100))
            .stop(Duration::from_secs(130));
        assert!(s.is_finished());
    }

    #[test]
    fn stop_from_paused_advances_to_finished() {
        let s = AppState::idle()
            .toggle(timer_countdown(ten_minutes()), Duration::from_secs(100))
            .toggle(timer_countdown(ten_minutes()), Duration::from_secs(110))
            .stop(Duration::from_secs(120));
        assert!(s.is_finished());
    }

    #[test]
    fn stop_from_finished_stays_finished() {
        // Stop button isn't reachable from Finished (Done screen has
        // Save / Discard instead), but defending against a stale
        // callback is cheap.
        let t = AppState::Finished.stop(Duration::from_secs(100));
        assert!(t.is_finished());
        assert!(t.effects.is_empty());
    }

    // ── The saved duration comes from core ──────────────────────

    #[test]
    fn stop_reports_cores_duration_and_silences_signals() {
        let t = AppState::idle()
            .toggle(timer_countdown(ten_minutes()), Duration::from_secs(100))
            .stop(Duration::from_secs(190));
        assert_eq!(t.effects.first(), Some(&Effect::StopActiveSignals));
        assert_eq!(ended_duration_secs(&t.effects), Some(90));
        assert!(
            !t.effects.iter().any(|e| matches!(e, Effect::FireEndBell { .. })),
            "Stop is silent",
        );
    }

    #[test]
    fn stop_leaves_paused_time_out() {
        let shape = || timer_countdown(ten_minutes());
        let t = AppState::idle()
            .toggle(shape(), Duration::from_secs(100))
            .toggle(shape(), Duration::from_secs(130)) // pause after 30 s
            .toggle(shape(), Duration::from_secs(400)) // resume
            .stop(Duration::from_secs(420));
        assert_eq!(ended_duration_secs(&t.effects), Some(50));
    }

    #[test]
    fn stop_in_overtime_counts_the_overtime() {
        let t = AppState::idle()
            .toggle(timer_countdown(Duration::from_secs(60)), Duration::from_secs(100))
            .tick(Duration::from_secs(160))
            .tick(Duration::from_secs(175));
        assert!(t.is_active(), "still in Overtime");
        let t = t.stop(Duration::from_secs(180));
        assert_eq!(ended_duration_secs(&t.effects), Some(80));
    }

    #[test]
    fn a_box_breath_end_reports_cores_cycle_aligned_duration() {
        let shape = SessionShape::BoxBreathCountdown {
            pattern: meditate_core::breath::BreathPattern::box_breath(),
            target_secs: 16,
        };
        let mut t = AppState::idle().toggle(shape, Duration::from_secs(100));
        for secs in 101..=116 {
            t = t.tick(Duration::from_secs(secs));
            if t.is_finished() {
                break;
            }
        }
        assert!(t.is_finished());
        assert_eq!(ended_duration_secs(&t.effects), Some(16));
    }

    #[test]
    fn finish_and_add_report_cores_duration() {
        let start = || {
            AppState::idle()
                .toggle(timer_countdown(Duration::from_secs(60)), Duration::from_secs(100))
                .tick(Duration::from_secs(160))
        };
        let finish = start().finish_overtime();
        assert_eq!(ended_duration_secs(&finish.effects), Some(60), "Finish keeps the planned length");
        let add = start().add_overtime(Duration::from_secs(190));
        assert_eq!(ended_duration_secs(&add.effects), Some(90), "Add keeps the overtime");
    }

    #[test]
    fn no_end_effect_means_no_duration() {
        assert_eq!(ended_duration_secs(&[]), None);
        assert_eq!(ended_duration_secs(&[Effect::StopActiveSignals]), None);
    }

    /// Every way a session ends takes its saved duration from core's
    /// effect, not from the shell's own clock arithmetic.
    #[test]
    fn the_shell_saves_cores_duration_on_every_end() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs");
        let lib = std::fs::read_to_string(path).unwrap();
        let code = lib.split("#[cfg(test)]\nmod tests").next().unwrap();
        assert!(!code.contains("fn end_session_duration("), "one helper, in app.rs");
        assert!(!code.contains("pre_elapsed"));
        assert!(!code.contains("session.elapsed(now).as_secs() as i64"), "no shell-side duration");
        assert_eq!(
            code.matches("app::ended_duration_secs(&transition.effects)").count(),
            4,
            "Stop, Finish, Add and the natural end",
        );
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
