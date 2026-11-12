//! Session state machine — the core of a meditation session's
//! lifecycle, owned by the shell and driven via `tick(now)` once a
//! second.
//!
//! `Session` covers every phase of an in-flight session:
//! Prep → Running → (Overtime or Box-Breath) → Stop, with
//! Pause/Resume layered on top via `Stopwatch`. Bells (starting,
//! interval, end) and Box-Breath phase cues fire as `Effect`s.
//!
//! Ownership pattern: the app holds `Option<Session>`, `None` while
//! Setup shows. Each tick it calls `session.tick(now)`, dispatches the
//! returned effects (bells, signals, the end) and then redraws the
//! time, the buttons and the overtime from the session.

use crate::bells::{ActiveBell, BellCue, BellSchedule};
use crate::breath::{Phase, PhaseInfo};
use crate::db::{BoxBreathPhaseId, SessionMode, SignalMode};
use crate::timer::Stopwatch;
use std::time::Duration;

mod effect;
mod settings;
pub use effect::{Effect, FireChannel};
pub use settings::{SessionSettings, SessionShape};

/// What the running page should reflect right now: the phase, with
/// a pause on top, and `Idle` for no session (the app's `None`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiState {
    /// No session in flight — the Setup view is showing.
    Idle,
    /// Prep-silence countdown before the starting bell.
    Preparing,
    /// Active session (countdown, stopwatch, or box-breath cycling).
    Running,
    /// Countdown crossed zero; the user hasn't tapped Finish or Add.
    Overtime,
    /// Session paused; tick produces no effects.
    Paused,
    /// Session ended (any terminal path); the running page shows Save.
    Done,
}

/// What the play/pause-key tap should do. `Session::toggle_action`
/// for a session, `Start` without one; `NoOp` covers overtime and an
/// ended session, where only the page's buttons end it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToggleAction {
    Start,
    Pause,
    Resume,
    NoOp,
}

/// A button in the running page's row; Discard has its own line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndButton {
    Pause,
    Resume,
    Save,
    /// Overtime: save with the overtime added.
    Add,
}

/// What the running page shows: a row of one or two buttons, Discard
/// under it in every state, and optionally the note field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EndButtons {
    pub first: EndButton,
    pub second: Option<EndButton>,
    pub note: bool,
}

impl EndButtons {
    /// A session that ended but is not saved yet (Box Breath's own
    /// end, or a save that failed).
    pub const ENDED: Self = Self { first: EndButton::Save, second: None, note: true };

    /// The row's two places, left to right.
    pub fn row(&self) -> [Option<EndButton>; 2] {
        [Some(self.first), self.second]
    }
}

/// What the running page draws around the time: the ring (a Timer
/// countdown or a Guided session) and the counter of all its bells,
/// empty when no bell rings between the starting and the end bell.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BellRing {
    pub ring: Option<Ring>,
    pub counters: Vec<BellCounter>,
}

/// Positions are parts of the planned length, 0 at the top, 1 back
/// at the top, clockwise.
#[derive(Debug, Clone, PartialEq)]
pub struct Ring {
    pub filled: f64,
    /// The small ring round the start point, while preparing.
    pub prep: Option<f64>,
    pub start_filled: bool,
    pub beads: Vec<Bead>,
}

/// One ring of a bell: `large` for a one-off bell.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bead {
    pub at: f64,
    pub large: bool,
    pub rung: bool,
}

/// "2 of 5": `total` is `None` where no end bounds it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BellCounter {
    pub rung: u32,
    pub total: Option<u32>,
}

/// What the session's media control (the phone's notification, the
/// Linux media player) shows: one line, and the bar for a session
/// with an end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediaStatus {
    pub line: MediaLine,
    pub bar: Option<MediaBar>,
    /// The time stands: paused, or in overtime.
    pub standing: bool,
}

/// The media control's title: the guided file's name, else the app's.
pub fn media_title(guided_name: Option<&str>) -> &str {
    guided_name.unwrap_or("Meditate")
}

/// The media control's line; each app words it, with "Paused" for a
/// standing `Left` or `SoFar`. Whole seconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaLine {
    Left(u64),
    SoFar(u64),
    Preparing(u64),
    Overtime(u64),
}

/// The bar: how far in and how long. It moves on by itself unless
/// the status is `standing`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediaBar {
    /// Exact, so `now - elapsed` (the bar's start) stays the same
    /// from tick to tick.
    pub elapsed: Duration,
    pub total_secs: u64,
}

/// Default Timer-mode countdown in seconds for a fresh user
/// (10 minutes). Used by the shell when restoring on first launch
/// before any persisted value exists, AND as the fallback when the
/// `timer_session_secs` row is missing or unparseable.
pub const TIMER_DEFAULT_SECS: u32 = 10 * 60;

/// Default Box-Breath session length in seconds for a fresh user
/// (5 minutes). Used by the shell the same way as
/// `TIMER_DEFAULT_SECS`.
pub const BREATHING_DEFAULT_SECS: u32 = 5 * 60;

/// Which lifecycle phase the in-flight session is in. A pause is
/// not a phase: it lives in the session's clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionPhase {
    /// Silent preparation before the starting bell.
    Prep,
    /// Active session: countdown, stopwatch, or box-breath cycling.
    Running,
    /// A Timer or Guided countdown reached its end; the user hasn't
    /// chosen Save or Add yet. Bells keep ringing on the session's
    /// timeline.
    Overtime,
    /// Ended, with the length to save, and whether the countdown had
    /// reached its planned end (the ring and the end bell's count show
    /// it, whatever length is saved). Ticks do nothing from here.
    Stopped { duration_secs: u64, reached_end: bool },
}

/// In-flight session. Created by `start` (in Prep when prep silence
/// is enabled, else in Running); driven by `tick(now)` thereafter;
/// ended by `stop`, `finish_overtime`, `add_overtime_and_finish` or
/// its own end.
#[derive(Debug)]
pub struct Session {
    settings: SessionSettings,
    phase: SessionPhase,
    /// Per-phase elapsed-tracker, re-anchored when prep ends. Paused
    /// while the session is paused, so paused time doesn't count.
    phase_clock: Stopwatch,
    /// Box-Breath phase boundary tracking. `None` until the first
    /// Running tick, which seeds it silently because `Session::start`
    /// already cued the first inhale. `Some(p)` thereafter; a tick observes
    /// `phase_at(elapsed) != p` and fires its phase cue.
    /// Always `None` outside Box Breath mode.
    last_breath_phase: Option<Phase>,
    /// Per-session bell schedule. Mutated each tick: Interval
    /// bells reroll their `next_ring_secs`; Fixed bells flip their
    /// `fired` flag.
    bells: Vec<ActiveBell>,
    /// Xorshift64 state for bell-jitter rolls, from
    /// `SessionSettings::bell_rng_seed` (`bells::xorshift64` guards a 0).
    bell_rng_state: u64,
    /// The guided track ended while paused: the first tick after Resume
    /// ends the countdown.
    track_ended: bool,
}

impl Session {
    /// Start a session: in Prep when `settings.prep_secs` is set,
    /// otherwise straight in Running. Returns the session together
    /// with the effects to dispatch right away, so a shell can't
    /// start a session and lose its starting bell.
    ///
    /// # Lifecycle the caller is signing up for
    ///
    /// 1. **Construct** via this fn and dispatch the effects it
    ///    returns. The `Session` is in `Prep` when
    ///    `settings.prep_secs` is set, otherwise in `Running`;
    ///    clock running from `now`. The effects are
    ///    what plays at the start: the starting bell when there is
    ///    no prep (with prep it comes from the tick that ends prep).
    /// 2. **Tick** once per second from the caller's clock, passing
    ///    a monotonic `now: Duration` (typically `boot_time_now()` so
    ///    suspend doesn't freeze the clock). Each tick returns
    ///    `Vec<Effect>`: bells, haptics, box-breath cues, the end.
    ///    Redraw the time and buttons from the session after it. The
    ///    session's internal phase transitions (Prep → Running →
    ///    Overtime, etc.) happen inside `tick`.
    /// 3. **Optional pause/resume** via `pause(now)` / `resume(now)`.
    ///    The phase clock freezes during pause; resume rebases off
    ///    the new `now` so a 5-minute pause doesn't burn 5 minutes
    ///    of session.
    /// 4. **Terminate** via one of: `stop(now)` (user-initiated;
    ///    persist the elapsed duration), `finish_overtime()` (user
    ///    tapped Save during overtime; persist the planned target),
    ///    `add_overtime_and_finish(now)` (user tapped Add; persist
    ///    elapsed-including-overtime). Each returns `StopActiveSignals`
    ///    and `Effect::Ended` with the length to save. A countdown can
    ///    also end by itself (Auto-Finish, a Box Breath end).
    /// 5. **Drop** the session on the Done-view dismissal. The
    ///    persisted DB row is the source of truth past that point.
    ///
    /// # Example
    ///
    /// ```
    /// use meditate_core::session::{Session, SessionSettings};
    /// use std::time::Duration;
    ///
    /// let (mut s, start_effects) = Session::start(
    ///     SessionSettings::default(),
    ///     Duration::from_secs(100),
    /// );
    /// // Dispatch `start_effects` (the starting bell, if any).
    /// assert!(start_effects.is_empty()); // no starting bell configured
    /// // Shell drives by calling `tick(now)` once per second.
    /// let _effects = s.tick(Duration::from_secs(101));
    /// // Terminate via `stop`, `finish_overtime`, or
    /// // `add_overtime_and_finish` — `Effect::Ended` carries the
    /// // length to persist.
    /// let end_effects = s.stop(Duration::from_secs(110));
    /// assert!(!end_effects.is_empty());
    /// ```
    pub fn start(settings: SessionSettings, now: Duration) -> (Self, Vec<Effect>) {
        if settings.prep_secs.is_some() {
            (Self::new(settings, SessionPhase::Prep, now), Vec::new())
        } else {
            let session = Self::new(settings, SessionPhase::Running, now);
            let mut effects: Vec<Effect> = starting_bell_effect(&session.settings).into_iter().collect();
            // Box Breath has no starting bell; its first inhale is cued here,
            // and the first tick then seeds the phase silently.
            if session.mode() == SessionMode::BoxBreath {
                effects.extend(box_breath_cue_effect(&session.settings, BoxBreathPhaseId::In));
            }
            (session, effects)
        }
    }

    fn new(mut settings: SessionSettings, phase: SessionPhase, now: Duration) -> Self {
        // A bell the mode's signal setting mutes never rings: not on the ring, not counted.
        let signal = settings.signal_mode_override;
        let rings = |cue: &BellCue| crate::bells::effective_signal_mode(cue.signal_mode, signal).is_some();
        settings.starting_bell = settings.starting_bell.take().filter(|cue| rings(cue));
        settings.end_bell = settings.end_bell.take().filter(|cue| rings(cue));
        let mut bells = std::mem::take(&mut settings.bells);
        bells.retain(|bell| rings(&bell.cue));
        let bell_rng_state = settings.bell_rng_seed;
        Self {
            settings,
            phase,
            phase_clock: Stopwatch::started_at(now),
            last_breath_phase: None,
            bells,
            bell_rng_state,
            track_ended: false,
        }
    }

    /// Test shortcut: a session in Prep, start effects dropped.
    /// `prep_secs` must be set in `settings`.
    #[cfg(test)]
    pub(crate) fn start_prep(settings: SessionSettings, now: Duration) -> Self {
        debug_assert!(
            settings.prep_secs.is_some(),
            "start_prep called without prep_secs in settings",
        );
        Self::new(settings, SessionPhase::Prep, now)
    }

    /// Test shortcut: a session straight in Running, start effects
    /// (the starting bell) dropped.
    #[cfg(test)]
    pub(crate) fn start_running(settings: SessionSettings, now: Duration) -> Self {
        Self::new(settings, SessionPhase::Running, now)
    }

    /// Freeze the session's elapsed clock at `now`. Returns a
    /// `StopActiveSignals` effect on the leading edge so the shell
    /// cuts in-flight bells/vibration; subsequent `tick()` calls
    /// return no effects. The phase is preserved — resume
    /// continues from whatever phase pause was called from.
    /// Idempotent: calling pause on an already-paused session is a
    /// no-op (won't double-count paused time, returns empty Vec).
    pub fn pause(&mut self, now: Duration) -> Vec<Effect> {
        if self.is_paused() || self.final_duration_secs().is_some() {
            return Vec::new();
        }
        self.phase_clock = self.phase_clock.paused_at(now);
        vec![Effect::StopActiveSignals]
    }

    /// Unfreeze the session. Subsequent ticks resume producing
    /// effects, with elapsed continuing from the pause moment (no
    /// leak from the pause window). Idempotent: calling resume on
    /// an already-running session is a no-op.
    pub fn resume(&mut self, now: Duration) {
        if self.is_paused() {
            self.phase_clock = self.phase_clock.resumed_at(now);
        }
    }

    /// Paused and not ended: an ended session never resumes.
    pub fn is_paused(&self) -> bool {
        matches!(self.phase_clock, Stopwatch::Paused { .. }) && self.final_duration_secs().is_none()
    }

    /// End the session with `duration_secs` to save.
    fn end(&mut self, duration_secs: u64, save_now: bool) -> Effect {
        let reached_end = self.phase == SessionPhase::Overtime;
        self.phase = SessionPhase::Stopped { duration_secs, reached_end };
        Effect::Ended { duration_secs, save_now }
    }

    /// User-initiated stop: ends with the meditation time so far (the
    /// moment of a pause, if paused; 0 during preparation, which is
    /// never meditation; with the overtime in overtime). Returns
    /// `StopActiveSignals` and `Ended`; nothing once ended.
    pub fn stop(&mut self, now: Duration) -> Vec<Effect> {
        if self.final_duration_secs().is_some() {
            return Vec::new();
        }
        let duration_secs = self.saved_secs(now);
        vec![Effect::StopActiveSignals, self.end(duration_secs, false)]
    }

    /// Overtime Save: ends with the planned length; the overtime is
    /// dropped. Nothing outside Overtime, so a stale tap is harmless.
    pub fn finish_overtime(&mut self) -> Vec<Effect> {
        if self.phase != SessionPhase::Overtime {
            return Vec::new();
        }
        let planned = self.completion_duration_secs();
        vec![Effect::StopActiveSignals, self.end(planned, false)]
    }

    /// Overtime Add: ends with the overtime included. Nothing outside
    /// Overtime.
    pub fn add_overtime_and_finish(&mut self, now: Duration) -> Vec<Effect> {
        if self.phase != SessionPhase::Overtime {
            return Vec::new();
        }
        self.stop(now)
    }

    /// The guided track ended before the tick reached its length: end
    /// the countdown now. Only a running Guided session follows its
    /// audio (a preview ending under a Timer is ignored); nothing
    /// otherwise, so the app can call it on every track end. Paused,
    /// the end waits for the tick after Resume.
    pub fn enter_overtime(&mut self, now: Duration) -> Vec<Effect> {
        if self.phase != SessionPhase::Running || !matches!(self.settings.shape, SessionShape::Guided { .. }) {
            return Vec::new();
        }
        // The tick ends it: now, or after Resume when paused.
        self.track_ended = true;
        self.tick(now)
    }

    /// The countdown reached its planned end at `elapsed` (by its time,
    /// or the track's end): every bell due by then or by the planned
    /// length is skipped, so none rings after the end bell; "complete";
    /// then overtime, or an end at the planned length (Auto-Finish, and
    /// Box Breath, which has no overtime).
    fn reach_end(&mut self, elapsed: Duration) -> Vec<Effect> {
        let planned = self.completion_duration_secs();
        let mut rng = bell_rng(&mut self.bell_rng_state);
        for bell in &mut self.bells {
            bell.skip_due(elapsed.as_secs().max(planned), &mut rng);
        }
        drop(rng);
        let mut effects = Vec::new();
        if matches!(self.settings.shape, SessionShape::Guided { .. }) {
            effects.push(Effect::StopGuidedAudio);
        }
        effects.push(Effect::Completed { planned_secs: planned });
        self.phase = SessionPhase::Overtime;
        if self.settings.auto_finish || self.mode() == SessionMode::BoxBreath {
            effects.push(self.end(planned, self.settings.auto_finish));
        }
        effects.extend(end_bell_effect(&self.settings));
        effects
    }

    /// Drive the session forward by one tick: phase changes, bells,
    /// the end. Nothing while paused or ended.
    pub fn tick(&mut self, now: Duration) -> Vec<Effect> {
        if self.is_paused() {
            return Vec::new();
        }
        let elapsed = self.phase_clock.elapsed(now);
        match self.phase {
            SessionPhase::Prep => self.tick_prep(now, elapsed),
            SessionPhase::Running => self.tick_running(elapsed),
            SessionPhase::Overtime => self.ring_due_bells(elapsed),
            SessionPhase::Stopped { .. } => Vec::new(),
        }
    }

    fn tick_prep(&mut self, now: Duration, elapsed: Duration) -> Vec<Effect> {
        let target = Duration::from_secs(u64::from(self.settings.prep_secs.unwrap_or(0)));
        if elapsed < target {
            return Vec::new();
        }
        // Running counts from the moment prep ended, not from this
        // (possibly late) tick.
        self.phase = SessionPhase::Running;
        let late = elapsed - target;
        self.phase_clock = Stopwatch::started_at(now.saturating_sub(late));
        // The starting bell rings with the boundary crossing, unless
        // a suspend already carried the session past its end: then
        // only the end bell, on the next tick, makes sense.
        let past_end = self
            .settings
            .shape
            .target_secs()
            .is_some_and(|t| late >= Duration::from_secs(u64::from(t)));
        if past_end {
            // It never rings, so it isn't counted either.
            self.settings.starting_bell = None;
            Vec::new()
        } else {
            starting_bell_effect(&self.settings).into_iter().collect()
        }
    }

    fn tick_running(&mut self, elapsed: Duration) -> Vec<Effect> {
        // Guided with count-up display still ends at its length.
        let end = self.settings.shape.target_secs().map(|t| Duration::from_secs(u64::from(t)));
        // Box Breath checks its end before the phase change, so the
        // end doesn't cue another inhale.
        if self.track_ended || end.is_some_and(|end| elapsed >= end) {
            return self.reach_end(elapsed);
        }
        let mut effects = Vec::new();
        if let SessionShape::BoxBreathCountdown { pattern, .. } | SessionShape::BoxBreathStopwatch { pattern } = self.settings.shape {
            effects.extend(self.cue_breath_phase(pattern.phase_at(elapsed).phase));
        }
        effects.extend(self.ring_due_bells(elapsed));
        effects
    }

    /// The Box Breath phase now: a cue when it changed. The first tick
    /// seeds it silently: `Session::start` already cued the first inhale.
    fn cue_breath_phase(&mut self, phase: Phase) -> Option<Effect> {
        let prev = self.last_breath_phase.replace(phase)?;
        (prev != phase).then(|| box_breath_cue_effect(&self.settings, phase_to_id(phase))).flatten()
    }

    fn ring_due_bells(&mut self, elapsed: Duration) -> Vec<Effect> {
        fire_due_bells(&mut self.bells, &mut self.bell_rng_state, self.settings.signal_mode_override, elapsed)
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
        let pattern = match self.settings.shape {
            SessionShape::BoxBreathCountdown { pattern, .. }
            | SessionShape::BoxBreathStopwatch { pattern } => pattern,
            _ => return None,
        };
        let elapsed = self.phase_clock.elapsed(now);
        Some(pattern.phase_at(elapsed))
    }

    pub fn phase(&self) -> SessionPhase {
        self.phase
    }

    pub fn elapsed(&self, now: Duration) -> Duration {
        self.phase_clock.elapsed(now)
    }

    /// The length to save once the session has ended; `None` while
    /// it runs.
    pub fn final_duration_secs(&self) -> Option<u64> {
        match self.phase {
            SessionPhase::Stopped { duration_secs, .. } => Some(duration_secs),
            _ => None,
        }
    }

    /// The planned length: the countdown's target, the guided file's
    /// length, Box Breath's cycle-aligned target; 0 for a stopwatch.
    /// "Meditation complete" and the frozen overtime time show it.
    pub fn completion_duration_secs(&self) -> u64 {
        self.settings.shape.target_secs().map_or(0, u64::from)
    }

    /// `meditated` in whole seconds: what Save and the crash-recovery
    /// heartbeat record.
    pub fn saved_secs(&self, now: Duration) -> u64 {
        self.meditated(now).as_secs()
    }

    /// Meditation time now: none in preparation, the saved length once
    /// ended, else the clock.
    fn meditated(&self, now: Duration) -> Duration {
        match self.phase {
            SessionPhase::Prep => Duration::ZERO,
            SessionPhase::Stopped { duration_secs, .. } => Duration::from_secs(duration_secs),
            SessionPhase::Running | SessionPhase::Overtime => self.phase_clock.elapsed(now),
        }
    }

    /// Whether the countdown reached its planned end.
    fn reached_end(&self) -> bool {
        match self.phase {
            SessionPhase::Overtime => true,
            SessionPhase::Stopped { reached_end, .. } => reached_end,
            SessionPhase::Prep | SessionPhase::Running => false,
        }
    }

    /// How far past its planned end the session is; `None` outside
    /// overtime. The Add button shows it.
    pub fn overtime(&self, now: Duration) -> Option<Duration> {
        (self.phase == SessionPhase::Overtime)
            .then(|| self.phase_clock.elapsed(now).saturating_sub(Duration::from_secs(self.completion_duration_secs())))
    }

    /// What the play/pause key does now. One key never saves: Save and
    /// Add are on the page.
    pub fn toggle_action(&self) -> ToggleAction {
        match self.ui_state() {
            UiState::Preparing | UiState::Running => ToggleAction::Pause,
            UiState::Paused => ToggleAction::Resume,
            UiState::Idle | UiState::Overtime | UiState::Done => ToggleAction::NoOp,
        }
    }

    /// The running page's buttons for this session right now.
    pub fn end_buttons(&self) -> EndButtons {
        use EndButton::{Add, Pause, Resume, Save};
        let buttons = |first, second, note| EndButtons { first, second, note };
        match (self.phase, self.is_paused()) {
            (SessionPhase::Stopped { .. }, _) => EndButtons::ENDED,
            // Preparation seconds are never saved as meditation.
            (SessionPhase::Prep, true) => buttons(Resume, None, false),
            (_, true) => buttons(Resume, Some(Save), true),
            (SessionPhase::Overtime, false) => buttons(Save, Some(Add), true),
            (SessionPhase::Prep, false) => buttons(Pause, None, false),
            _ => buttons(Pause, Some(Save), false),
        }
    }

    /// What the running page should reflect right now. An ended
    /// session is ended, paused or not.
    pub fn ui_state(&self) -> UiState {
        match self.phase {
            SessionPhase::Stopped { .. } => UiState::Done,
            _ if self.is_paused() => UiState::Paused,
            SessionPhase::Prep => UiState::Preparing,
            SessionPhase::Running => UiState::Running,
            SessionPhase::Overtime => UiState::Overtime,
        }
    }

    /// Mode of the in-flight session. Captured at start; never
    /// changes during a session.
    pub fn mode(&self) -> SessionMode {
        self.settings.shape.mode()
    }

    /// Seconds of preparation left, rounded up; `None` outside it.
    pub fn prep_remaining_secs(&self, now: Duration) -> Option<u64> {
        if self.phase != SessionPhase::Prep {
            return None;
        }
        let prep = self.settings.prep_secs?;
        Some(secs_left(u64::from(prep), self.phase_clock.elapsed(now)))
    }

    /// The ring and the bell counters for the running page right now.
    pub fn bell_ring(&self, now: Duration) -> BellRing {
        let end = match self.settings.shape {
            SessionShape::BoxBreathCountdown { .. } => None,
            ref shape => shape.target_secs().map(u64::from),
        };
        let preparing = self.phase == SessionPhase::Prep;
        // Ended: the length it saves, as the time shows.
        let elapsed = self.meditated(now).as_secs_f64();
        let past_end = self.reached_end();
        let part = |secs: f64| end.map_or(0.0, |end| (secs / end as f64).min(1.0));
        let mut beads = Vec::new();
        // One count for every bell heard; none without bells between start and end.
        let (mut rung, mut total, mut counted) = (0u32, 0u32, false);
        for bell in &self.bells {
            match &bell.schedule {
                BellSchedule::Fixed { target_secs, fired } => {
                    let heard = *fired == Some(true);
                    beads.push(Bead { at: part(*target_secs as f64), large: true, rung: heard });
                    rung += u32::from(heard);
                    total += 1;
                    counted = true;
                }
                BellSchedule::Interval { base_secs, step_secs, gaps, rang, .. } => {
                    // A bell with no time (old data) rings each second;
                    // a bead a second would say nothing.
                    if *base_secs == 0 && *step_secs == 0 {
                        continue;
                    }
                    // A bead per planned ring that can come before the end,
                    // at its earliest time until it rings.
                    let mut mine = 0;
                    if let Some(end) = end {
                        let mut heard = rang.iter().peekable();
                        for (index, earliest) in bell.schedule.rings_before(end) {
                            let at = heard.next_if(|&&(i, _)| i == index).map(|&(_, at)| at as f64);
                            let secs = match at {
                                Some(at) => at,
                                None if index + 1 == *gaps => (earliest as f64).max(elapsed),
                                None => earliest as f64,
                            };
                            beads.push(Bead { at: part(secs), large: false, rung: at.is_some() });
                            mine += 1;
                        }
                        if mine == 0 {
                            continue;
                        }
                    }
                    rung += rang.len() as u32;
                    total += mine;
                    counted = true;
                }
            }
        }
        // The starting and end bells count too, once there are bells between them.
        if counted {
            if self.settings.starting_bell.is_some() {
                rung += u32::from(!preparing);
                total += 1;
            }
            if end.is_some() && self.settings.end_bell.is_some() {
                // Rang when the countdown reached its end.
                rung += u32::from(past_end);
                total += 1;
            }
        }
        let total = (end.is_some() && !past_end).then_some(total);
        let counters = if counted { vec![BellCounter { rung, total }] } else { Vec::new() };
        let ring = end.map(|_| Ring {
            filled: if past_end { 1.0 } else { part(elapsed) },
            prep: preparing.then(|| {
                let prep = f64::from(self.settings.prep_secs.unwrap_or(1).max(1));
                (self.phase_clock.elapsed(now).as_secs_f64() / prep).min(1.0)
            }),
            start_filled: !preparing,
            beads,
        });
        BellRing { ring, counters }
    }

    /// The media control's line and bar; `None` once stopped.
    pub fn media_status(&self, now: Duration) -> Option<MediaStatus> {
        let elapsed = self.phase_clock.elapsed(now);
        let total = self.settings.shape.target_secs().map(u64::from);
        let (line, bar) = match self.phase {
            SessionPhase::Stopped { .. } => return None,
            SessionPhase::Prep => {
                let prep = u64::from(self.settings.prep_secs?);
                let bar = MediaBar { elapsed: elapsed.min(Duration::from_secs(prep)), total_secs: prep };
                (MediaLine::Preparing(secs_left(prep, elapsed)), Some(bar))
            }
            SessionPhase::Overtime => {
                let total = total.unwrap_or(0);
                let over = self.overtime(now).unwrap_or_default().as_secs();
                let full = MediaBar { elapsed: Duration::from_secs(total), total_secs: total };
                (MediaLine::Overtime(over), Some(full))
            }
            SessionPhase::Running => match total {
                Some(total) => {
                    let bar = MediaBar { elapsed: elapsed.min(Duration::from_secs(total)), total_secs: total };
                    (MediaLine::Left(secs_left(total, elapsed)), Some(bar))
                }
                None => (MediaLine::SoFar(elapsed.as_secs()), None),
            },
        };
        let standing = self.is_paused() || self.phase == SessionPhase::Overtime;
        Some(MediaStatus { line, bar, standing })
    }

    /// The whole seconds the running page's time shows now: the
    /// preparation left; while running, the countdown left (rounded
    /// up) or the time so far (rounded down) for stopwatches, Box
    /// Breath and a count-up Guided; in overtime the planned length,
    /// frozen (also when a guided track ended early: only the Add
    /// button counts the overtime); once ended, the length saved.
    pub fn display_secs(&self, now: Duration) -> u64 {
        let elapsed = self.phase_clock.elapsed(now);
        match self.phase {
            SessionPhase::Prep => self.prep_remaining_secs(now).unwrap_or(0),
            SessionPhase::Running => match self.settings.shape {
                SessionShape::TimerCountdown { target_secs }
                | SessionShape::Guided { duration_secs: target_secs, count_up_display: false } => {
                    secs_left(u64::from(target_secs), elapsed)
                }
                _ => elapsed.as_secs(),
            },
            SessionPhase::Overtime => self.completion_duration_secs(),
            SessionPhase::Stopped { duration_secs, .. } => duration_secs,
        }
    }
}

/// Whole seconds left of `target_secs` after `elapsed`, rounded up:
/// what a countdown shows, so a fresh 10:00 doesn't flicker to 09:59.
fn secs_left(target_secs: u64, elapsed: Duration) -> u64 {
    let left = Duration::from_secs(target_secs).saturating_sub(elapsed);
    left.as_secs() + u64::from(left.subsec_nanos() > 0)
}

fn phase_to_id(phase: Phase) -> BoxBreathPhaseId {
    match phase {
        Phase::In => BoxBreathPhaseId::In,
        Phase::HoldIn => BoxBreathPhaseId::HoldIn,
        Phase::Out => BoxBreathPhaseId::Out,
        Phase::HoldOut => BoxBreathPhaseId::HoldOut,
    }
}

/// A bell's or phase cue's effect on `channel`, with the mode's signal
/// setting applied; `None` when that leaves nothing to play.
fn fire(channel: FireChannel, phase: Option<BoxBreathPhaseId>, cue: &BellCue, signal_override: SignalMode) -> Option<Effect> {
    let signal_mode = crate::bells::effective_signal_mode(cue.signal_mode, signal_override)?;
    Some(Effect::Fire { channel, phase, cue: BellCue { signal_mode, ..cue.clone() } })
}

fn starting_bell_effect(settings: &SessionSettings) -> Option<Effect> {
    fire(FireChannel::Starting, None, settings.starting_bell.as_ref()?, settings.signal_mode_override)
}

fn end_bell_effect(settings: &SessionSettings) -> Option<Effect> {
    fire(FireChannel::End, None, settings.end_bell.as_ref()?, settings.signal_mode_override)
}

/// Box Breath's cue for `phase`; `None` when the cues are off or the phase has none.
fn box_breath_cue_effect(settings: &SessionSettings, phase: BoxBreathPhaseId) -> Option<Effect> {
    let cue = settings.box_breath_cues.as_ref()?.cue_for(phase)?;
    fire(FireChannel::Interval, Some(phase), cue, settings.signal_mode_override)
}

/// The session's jitter draws, advancing its xorshift state.
fn bell_rng(state: &mut u64) -> impl FnMut() -> f64 + '_ {
    move || {
        let (unit, next) = crate::bells::xorshift64(*state);
        *state = next;
        unit
    }
}

/// Iterate over the session's bells, tick each against `elapsed`,
/// and emit `Fire` effects for every bell that crosses its
/// ring boundary this tick. Mutates `bells` (Interval bells reroll
/// their next_ring_secs; Fixed bells flip their fired flag) and
/// the xorshift state (one draw per interval an Interval bell steps over).
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
    let mut rng = bell_rng(rng_state);
    // After a time jump only the latest due fixed bell rings, as a repeating bell rings once.
    let due = |bell: &ActiveBell| match bell.schedule {
        BellSchedule::Fixed { target_secs, fired: None } if target_secs <= elapsed_secs => Some(target_secs),
        _ => None,
    };
    let latest = bells.iter().filter_map(due).max();
    for bell in bells.iter_mut() {
        if due(bell).is_some_and(|t| Some(t) < latest) {
            bell.skip_due(elapsed_secs, &mut rng);
        } else if bell.tick(elapsed_secs, &mut rng) {
            effects.extend(fire(FireChannel::Interval, None, &bell.cue, signal_mode_override));
        }
    }
    effects
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::breath::BreathPattern;

    fn timer_settings_with_prep(prep_secs: u32) -> SessionSettings {
        SessionSettings {
            shape: SessionShape::TimerCountdown { target_secs: 600 },
            prep_secs: Some(prep_secs),
            bells: Vec::new(),
            bell_rng_seed: 1,
            signal_mode_override: SignalMode::Both,
            starting_bell: None,
            end_bell: None,
            box_breath_cues: None,
            auto_finish: false,
        }
    }

    fn timer_countdown_settings(target_secs: u32) -> SessionSettings {
        SessionSettings {
            shape: SessionShape::TimerCountdown { target_secs },
            prep_secs: None,
            bells: Vec::new(),
            bell_rng_seed: 1,
            signal_mode_override: SignalMode::Both,
            starting_bell: None,
            end_bell: None,
            box_breath_cues: None,
            auto_finish: false,
        }
    }

    fn timer_prep_settings(prep_secs: u32, target_secs: u32) -> SessionSettings {
        let mut s = timer_countdown_settings(target_secs);
        s.prep_secs = Some(prep_secs);
        s
    }

    fn timer_stopwatch_settings() -> SessionSettings {
        SessionSettings {
            shape: SessionShape::TimerStopwatch,
            prep_secs: None,
            bells: Vec::new(),
            bell_rng_seed: 1,
            signal_mode_override: SignalMode::Both,
            starting_bell: None,
            end_bell: None,
            box_breath_cues: None,
            auto_finish: false,
        }
    }

    fn box_breath_settings(target_secs: Option<u32>) -> SessionSettings {
        let cue = || crate::bells::BellCue {
            sound_uuid: "cue-sound".into(),
            vibration_pattern_uuid: "cue-pattern".into(),
            signal_mode: SignalMode::Both,
            volume: Default::default(),
        };
        let pattern = BreathPattern::box_breath();
        let shape = match target_secs {
            Some(target_secs) => SessionShape::BoxBreathCountdown { pattern, target_secs },
            None => SessionShape::BoxBreathStopwatch { pattern },
        };
        SessionSettings {
            shape,
            prep_secs: None,
            bells: Vec::new(),
            bell_rng_seed: 1,
            signal_mode_override: SignalMode::Both,
            starting_bell: None,
            end_bell: None,
            // Wire all four phases so phase-boundary tests see
            // phase cue emissions; the gate-disabled cases
            // get tested via their own dedicated tests.
            box_breath_cues: Some(crate::bells::BoxBreathCueConfig {
                master_enabled: true,
                in_phase: Some(cue()),
                hold_in: Some(cue()),
                out_phase: Some(cue()),
                hold_out: Some(cue()),
            }),
            auto_finish: false,
        }
    }

    fn fixed_bell(target_secs: u64, sound_uuid: &str) -> ActiveBell {
        use crate::bells::BellSchedule;
        ActiveBell {
            cue: BellCue {
                sound_uuid: sound_uuid.into(),
                vibration_pattern_uuid: "pattern".into(),
                signal_mode: SignalMode::Sound,
                volume: Default::default(),
            },
            schedule: BellSchedule::Fixed { target_secs, fired: None },
        }
    }

    fn interval_bell(base_min: u32, sound_uuid: &str) -> ActiveBell {
        use crate::bells::BellSchedule;
        ActiveBell {
            cue: BellCue {
                sound_uuid: sound_uuid.into(),
                vibration_pattern_uuid: "pattern".into(),
                signal_mode: SignalMode::Sound,
                volume: Default::default(),
            },
            schedule: BellSchedule::every(base_min * 60, 0, 0, None, 0.5),
        }
    }

    #[test]
    fn start_prep_puts_session_in_prep_phase() {
        let s = Session::start_prep(timer_settings_with_prep(30), Duration::from_secs(100));
        assert_eq!(s.phase(), SessionPhase::Prep);
        assert_eq!(s.mode(), SessionMode::Timer);
    }

    #[test]
    fn tick_during_prep_shows_ceiling_rounded_display() {
        let mut s = Session::start_prep(timer_settings_with_prep(30), Duration::from_secs(100));
        // 5 s in: 25 s remaining; display "25".
        assert!(s.tick(Duration::from_secs(105)).is_empty());
        assert_eq!(s.display_secs(Duration::from_secs(105)), 25);
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
        assert!(s.tick(Duration::from_millis(100_000 + 4_001)).is_empty());
        assert_eq!(s.display_secs(Duration::from_millis(100_000 + 4_001)), 26);
    }

    #[test]
    fn tick_at_prep_boundary_transitions() {
        let mut s = Session::start_prep(timer_settings_with_prep(30), Duration::from_secs(100));
        // Exactly at target: 30 s in → end_prep, transition to Running.
        let effects = s.tick(Duration::from_secs(130));
        assert!(effects.is_empty(), "{effects:?}");
        assert_eq!(s.phase(), SessionPhase::Running);
    }

    #[test]
    fn tick_past_prep_boundary_transitions_once() {
        // App backgrounded for several seconds during prep; the
        // first tick after wake catches up. It transitions once;
        // subsequent ticks are then in Running.
        let mut s = Session::start_prep(timer_settings_with_prep(30), Duration::from_secs(100));
        let effects = s.tick(Duration::from_secs(135));
        assert!(effects.is_empty(), "{effects:?}");
        assert_eq!(s.phase(), SessionPhase::Running);

        // Subsequent tick is now in Running. Prep ended at 130 s
        // (not at the late 135 s tick), so at 136 s, 6 s of the
        // 600 s target have run → display 594.
        assert!(s.tick(Duration::from_secs(136)).is_empty());
        assert_eq!(s.display_secs(Duration::from_secs(136)), 594);
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
        assert!(s.tick(Duration::from_secs(160)).is_empty());
        assert_eq!(s.display_secs(Duration::from_secs(160)), 540);
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
        assert!(s.tick(Duration::from_millis(100_000 + 1)).is_empty());
        assert_eq!(s.display_secs(Duration::from_millis(100_000 + 1)), 600);
    }

    #[test]
    fn tick_running_countdown_at_target_enters_overtime() {
        // Exactly at the target — countdown is "complete." Emit
        // overtime + transition phase. Shell will fire the end
        // bell + morph buttons + freeze hero in response.
        let mut s = Session::start_running(
            timer_countdown_settings(60),
            Duration::from_secs(100),
        );
        let effects = s.tick(Duration::from_secs(160));
        assert_eq!(effects, vec![Effect::Completed { planned_secs: 60 }]);
        assert_eq!(s.phase(), SessionPhase::Overtime);
    }

    #[test]
    fn tick_running_countdown_past_target_enters_overtime_once() {
        // App backgrounded; first tick after wake catches up past
        // the target. It transitions once and says "complete" once;
        // the next tick is in Overtime phase.
        let mut s = Session::start_running(
            timer_countdown_settings(60),
            Duration::from_secs(100),
        );
        let effects = s.tick(Duration::from_secs(200));
        assert_eq!(effects, vec![Effect::Completed { planned_secs: 60 }]);
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
        assert!(s.tick(Duration::from_millis(100_000 + 5_999)).is_empty());
        assert_eq!(s.display_secs(Duration::from_millis(100_000 + 5_999)), 5);
        assert!(s.tick(Duration::from_millis(100_000 + 6_000)).is_empty());
        assert_eq!(s.display_secs(Duration::from_millis(100_000 + 6_000)), 6);
    }

    // ── Box Breath running ───────────────────────────────────────────

    #[test]
    fn first_box_breath_tick_seeds_phase_silently() {
        // First tick into a Box-Breath running session must NOT
        // emit a phase cue: `Session::start` already cued the
        // first inhale, so a Phase::In cue here would be a duplicate.
        let mut s = Session::start_running(box_breath_settings(None), Duration::from_secs(100));
        let effects = s.tick(Duration::from_millis(100_500)); // 0.5 s in, still Phase::In
        assert!(
            !effects.iter().any(|e| matches!(e, Effect::Fire { phase: Some(_), .. })),
            "first tick must not fire a phase cue: {effects:?}"
        );
        assert!(effects.is_empty(), "{effects:?}");
        assert_eq!(s.display_secs(Duration::from_millis(100_500)), 0);
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
                Effect::Fire { phase: Some(BoxBreathPhaseId::HoldIn), .. }
            )),
            "expected HoldIn cue, got {effects:?}"
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
            !effects.iter().any(|e| matches!(e, Effect::Fire { phase: Some(_), .. })),
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
            .any(|e| matches!(e, Effect::Fire { phase: Some(BoxBreathPhaseId::HoldIn), .. })));
        // Out boundary at t=108
        assert!(s
            .tick(Duration::from_secs(108))
            .iter()
            .any(|e| matches!(e, Effect::Fire { phase: Some(BoxBreathPhaseId::Out), .. })));
        // HoldOut boundary at t=112
        assert!(s
            .tick(Duration::from_secs(112))
            .iter()
            .any(|e| matches!(e, Effect::Fire { phase: Some(BoxBreathPhaseId::HoldOut), .. })));
        // Wrap to In at t=116
        assert!(s
            .tick(Duration::from_secs(116))
            .iter()
            .any(|e| matches!(e, Effect::Fire { phase: Some(BoxBreathPhaseId::In), .. })));
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
            effects.contains(&Effect::Ended { duration_secs: 16, save_now: false }),
            "expected the end, got {effects:?}"
        );
        assert!(matches!(s.phase(), SessionPhase::Stopped { .. }));
        assert_eq!(s.final_duration_secs(), Some(16));
    }

    #[test]
    fn box_breath_stopwatch_never_emits_end() {
        // No target → never auto-ends regardless of elapsed.
        let mut s = Session::start_running(box_breath_settings(None), Duration::from_secs(100));
        let _ = s.tick(Duration::from_millis(100_500));
        let effects = s.tick(Duration::from_secs(100 + 16 * 100)); // many cycles
        assert!(
            !effects.iter().any(|e| matches!(e, Effect::Ended { .. } | Effect::Completed { .. })),
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
    fn overtime_counts_past_the_target() {
        let mut s = Session::start_running(
            timer_countdown_settings(60),
            Duration::from_secs(100),
        );
        // Cross zero — transition.
        let _ = s.tick(Duration::from_secs(160));
        assert_eq!(s.phase(), SessionPhase::Overtime);
        // 5 s of overtime have accumulated.
        assert!(s.tick(Duration::from_secs(165)).is_empty());
        assert_eq!(s.overtime(Duration::from_secs(165)), Some(Duration::from_secs(5)));
    }

    #[test]
    fn overtime_tick_at_transition_moment_reports_zero_overtime() {
        // The very first tick after the crossing, at
        // host time = 60 s in (exactly target), reports overtime=0.
        // Subsequent ticks count up.
        let mut s = Session::start_running(
            timer_countdown_settings(60),
            Duration::from_secs(100),
        );
        let _ = s.tick(Duration::from_secs(160)); // overtime
        assert!(s.tick(Duration::from_secs(160)).is_empty());
        assert_eq!(s.overtime(Duration::from_secs(160)), Some(Duration::ZERO));
    }

    #[test]
    fn overtime_phase_clock_continues_uninterrupted_from_running() {
        // Running→Overtime transition keeps phase_clock ticking
        // (no reset); total elapsed = target + overtime accumulates
        // continuously; interval bells rely on this continuity.
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
        let _ = s.tick(Duration::from_secs(160)); // overtime
        let _ = s.tick(Duration::from_secs(170)); // 10 s of overtime
        let _ = s.pause(Duration::from_secs(170));
        // 100 s of host time pass while paused, then resume.
        s.resume(Duration::from_secs(270));
        // Active elapsed should still report 70 s total (60 target +
        // 10 overtime), and tick should report 10 s overtime.
        assert!(s.tick(Duration::from_secs(270)).is_empty());
        assert_eq!(s.overtime(Duration::from_secs(270)), Some(Duration::from_secs(10)));
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
                Effect::Fire { channel: FireChannel::Interval, cue, .. } if cue.sound_uuid.as_str() == "halftime"
            )),
            "expected the 'halftime' bell, got {effects:?}"
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
            !effects.iter().any(|e| matches!(e, Effect::Fire { channel: FireChannel::Interval, phase: None, .. })),
            "fixed bell must not re-fire: {effects:?}"
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
        assert!(!effects.iter().any(|e| matches!(e, Effect::Fire { channel: FireChannel::Interval, phase: None, .. })));
        // At 300 s in — fires, reroll to 600.
        let effects = s.tick(Duration::from_secs(400));
        assert!(effects.iter().any(|e| matches!(e, Effect::Fire { channel: FireChannel::Interval, phase: None, .. })));
        // At 599 s in — no fire yet.
        let effects = s.tick(Duration::from_secs(699));
        assert!(!effects.iter().any(|e| matches!(e, Effect::Fire { channel: FireChannel::Interval, phase: None, .. })));
        // At 600 s in — fires.
        let effects = s.tick(Duration::from_secs(700));
        assert!(effects.iter().any(|e| matches!(e, Effect::Fire { channel: FireChannel::Interval, phase: None, .. })));
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
            !effects.iter().any(|e| matches!(e, Effect::Fire { channel: FireChannel::Interval, phase: None, .. })),
            "bells must not fire during prep: {effects:?}"
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
        let _ = s.tick(Duration::from_secs(160)); // overtime
        let _ = s.tick(Duration::from_secs(200)); // 100 s elapsed, still no fire
        let effects = s.tick(Duration::from_secs(400)); // 300 s elapsed
        assert!(
            effects.iter().any(|e| matches!(e, Effect::Fire { channel: FireChannel::Interval, phase: None, .. })),
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
            effects.iter().any(|e| matches!(e, Effect::Fire { channel: FireChannel::Interval, phase: None, .. })),
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
                if matches!(e, Effect::Fire { channel: FireChannel::Interval, phase: None, .. }) {
                    a_fires.push(offset);
                }
            }
            for e in b.tick(now) {
                if matches!(e, Effect::Fire { channel: FireChannel::Interval, phase: None, .. }) {
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
        assert!(second.is_empty(), "redundant pause must emit nothing: {second:?}");
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
        assert!(effects.is_empty(), "paused tick must produce nothing: {effects:?}");
    }

    #[test]
    fn pause_during_prep_works() {
        let mut s = Session::start_prep(timer_settings_with_prep(30), Duration::from_secs(100));
        let _ = s.pause(Duration::from_secs(105)); // 5 s into prep
        s.resume(Duration::from_secs(200));
        // Active prep elapsed is 5 s — first tick after resume
        // should still report 25 s remaining (not the 95 s of host
        // time that elapsed).
        assert!(s.tick(Duration::from_secs(200)).is_empty());
        assert_eq!(s.display_secs(Duration::from_secs(200)), 25);
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
        assert!(s.tick(Duration::from_secs(105)).is_empty());
        assert_eq!(s.display_secs(Duration::from_secs(105)), 25);
        // Cross prep target: transition, no starting bell configured.
        assert!(s.tick(Duration::from_secs(130)).is_empty());
        // 60 s into Running phase (which started at 130 s of host
        // time): 540 s remaining of the 600 s target.
        assert!(s.tick(Duration::from_secs(190)).is_empty());
        assert_eq!(s.display_secs(Duration::from_secs(190)), 540);
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
        // Guided + stopwatch toggle: file's probed duration is the
        // target, but the user picked count-up display.
        let settings = SessionSettings {
            shape: SessionShape::Guided { duration_secs: 600, count_up_display: true },
            ..timer_countdown_settings(600)
        };
        let s = Session::start_running(settings, Duration::from_secs(100));
        assert_eq!(s.display_secs(Duration::from_secs(245)), 145);
    }

    // ── Stop / Finish-overtime / Add-overtime ────────────────────────

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
                Effect::Ended { duration_secs: 145, save_now: false },
            ]
        );
        assert!(matches!(s.phase(), SessionPhase::Stopped { .. }));
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
                Effect::Ended { duration_secs: 90, save_now: false },
            ]
        );
        assert!(matches!(s.phase(), SessionPhase::Stopped { .. }));
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
                Effect::Ended { duration_secs: 50, save_now: false },
            ]
        );
        assert!(matches!(s.phase(), SessionPhase::Stopped { .. }));
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
                Effect::Ended { duration_secs: 20, save_now: false },
            ]
        );
        let second = s.stop(Duration::from_secs(140));
        assert!(
            second.is_empty(),
            "second stop must be a no-op, got {second:?}"
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
            "the length the end carried",
        );
    }

    #[test]
    fn finish_overtime_stashes_target_secs() {
        let mut s = Session::start_running(
            timer_countdown_settings(600),
            Duration::from_secs(0),
        );
        let _ = s.tick(Duration::from_secs(600));
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
        let mut s = Session::start_running(
            timer_countdown_settings(600),
            Duration::from_secs(0),
        );
        assert_eq!(s.ui_state(), UiState::Running);
        let _ = s.pause(Duration::from_secs(5));
        assert_eq!(s.ui_state(), UiState::Paused);
        s.resume(Duration::from_secs(10));
        let _ = s.tick(Duration::from_secs(700));
        assert_eq!(s.ui_state(), UiState::Overtime);
        let _ = s.finish_overtime();
        assert_eq!(s.ui_state(), UiState::Done);
    }

    #[test]
    fn each_fire_logs_under_its_own_tag() {
        assert_eq!(FireChannel::Starting.log_tag(None), "fire_starting_bell");
        assert_eq!(FireChannel::End.log_tag(None), "fire_end_bell");
        assert_eq!(FireChannel::Interval.log_tag(None), "fire_interval_bell");
        assert_eq!(FireChannel::Interval.log_tag(Some(BoxBreathPhaseId::In)), "fire_box_breath_phase_cue");
    }

    #[test]
    fn ui_state_preparing_during_prep() {
        let s = Session::start_prep(
            timer_prep_settings(10, 600),
            Duration::from_secs(0),
        );
        assert_eq!(s.ui_state(), UiState::Preparing);
    }

    #[test]
    fn ticks_after_stop_emit_no_effects() {
        let mut s = Session::start_running(
            timer_countdown_settings(600),
            Duration::from_secs(100),
        );
        let _ = s.stop(Duration::from_secs(120));
        let effects = s.tick(Duration::from_secs(125));
        assert!(effects.is_empty(), "tick after stop must be silent: {effects:?}");
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
                Effect::Ended { duration_secs: 60, save_now: false },
            ]
        );
        assert!(matches!(s.phase(), SessionPhase::Stopped { .. }));
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
        let _ = s.tick(Duration::from_secs(160)); // overtime
        let _ = s.tick(Duration::from_secs(195)); // 35 s of overtime
        let effects = s.add_overtime_and_finish(Duration::from_secs(195));
        // 60 s target + 35 s overtime = 95 s saved.
        assert_eq!(
            effects,
            vec![
                Effect::StopActiveSignals,
                Effect::Ended { duration_secs: 95, save_now: false },
            ]
        );
        assert!(matches!(s.phase(), SessionPhase::Stopped { .. }));
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

    // ── End buttons and Auto-Finish ──────────────────────────────────

    #[test]
    fn the_end_buttons_follow_the_session() {
        use EndButton::{Add, Pause, Resume, Save};
        let t0 = Duration::from_secs(100);
        let shown = |s: &Session| s.end_buttons();
        let row = |first: EndButton, second: Option<EndButton>, note: bool| EndButtons { first, second, note };

        let mut prep = timer_countdown_settings(600);
        prep.prep_secs = Some(10);
        let (mut s, _) = Session::start(prep, t0);
        assert_eq!(shown(&s), row(Pause, None, false), "preparing");
        let _ = s.pause(t0 + Duration::from_secs(2));
        assert_eq!(shown(&s), row(Resume, None, false), "paused in preparation");

        let mut s = Session::start_running(timer_countdown_settings(60), t0);
        assert_eq!(shown(&s), row(Pause, Some(Save), false), "running");
        let stopwatch = Session::start_running(timer_stopwatch_settings(), t0);
        assert_eq!(shown(&stopwatch), row(Pause, Some(Save), false), "stopwatch running");
        let breath = Session::start_running(box_breath_settings(Some(16)), t0);
        assert_eq!(shown(&breath), row(Pause, Some(Save), false), "box breath running");
        let _ = s.pause(t0 + Duration::from_secs(5));
        assert_eq!(shown(&s), row(Resume, Some(Save), true), "paused");
        s.resume(t0 + Duration::from_secs(10));
        let _ = s.tick(t0 + Duration::from_secs(70));
        assert_eq!(shown(&s), row(Save, Some(Add), true), "overtime");

        let mut s = Session::start_running(box_breath_settings(Some(16)), t0);
        let _ = s.tick(t0 + Duration::from_millis(500));
        let _ = s.tick(t0 + Duration::from_secs(16));
        assert_eq!(shown(&s), row(Save, None, true), "box breath ended");
        assert_eq!(EndButtons::ENDED, row(Save, None, true), "ended, not saved");
    }

    #[test]
    fn save_while_paused_keeps_the_time_so_far() {
        let t0 = Duration::from_secs(100);
        let mut s = Session::start_running(timer_countdown_settings(20 * 60), t0);
        let _ = s.pause(t0 + Duration::from_secs(7 * 60));
        let effects = s.stop(t0 + Duration::from_secs(9 * 60));
        assert_eq!(
            effects,
            vec![Effect::StopActiveSignals, Effect::Ended { duration_secs: 7 * 60, save_now: false }]
        );
    }

    #[test]
    fn a_countdown_with_auto_finish_ends_at_its_length() {
        let t0 = Duration::from_secs(100);
        let settings = SessionSettings {
            auto_finish: true,
            end_bell: Some(cue_at(70.0)),
            ..timer_countdown_settings(600)
        };
        let mut s = Session::start_running(settings, t0);
        let effects = s.tick(t0 + Duration::from_secs(605));
        assert_eq!(effects[..2], [Effect::Completed { planned_secs: 600 }, Effect::Ended { duration_secs: 600, save_now: true }], "{effects:?}");
        assert!(effects.iter().any(|e| matches!(e, Effect::Fire { channel: FireChannel::End, .. })), "the end bell rings");
        assert_ne!(s.phase(), SessionPhase::Overtime);
        assert!(!effects.contains(&Effect::StopActiveSignals), "the end bell is not cut");
        assert!(matches!(s.phase(), SessionPhase::Stopped { .. }));
        assert_eq!(s.final_duration_secs(), Some(600));
    }

    #[test]
    fn a_guided_file_with_auto_finish_ends_at_its_length() {
        let t0 = Duration::from_secs(100);
        let settings = || SessionSettings { auto_finish: true, ..guided_settings(600, false) };
        // The length is reached first.
        let (mut s, _) = Session::start(settings(), t0);
        let effects = s.tick(t0 + Duration::from_secs(600));
        assert_eq!(effects[..3], [Effect::StopGuidedAudio, Effect::Completed { planned_secs: 600 }, Effect::Ended { duration_secs: 600, save_now: true }], "{effects:?}");
        assert!(effects.iter().any(|e| matches!(e, Effect::Fire { channel: FireChannel::End, .. })));
        assert!(matches!(s.phase(), SessionPhase::Stopped { .. }));
        // The file ends first: the stored length is saved.
        let (mut s, _) = Session::start(settings(), t0);
        let _ = s.tick(t0 + Duration::from_secs(590));
        let effects = s.enter_overtime(Duration::ZERO);
        assert_eq!(effects[..3], [Effect::StopGuidedAudio, Effect::Completed { planned_secs: 600 }, Effect::Ended { duration_secs: 600, save_now: true }], "{effects:?}");
        assert_eq!(s.final_duration_secs(), Some(600));
    }

    #[test]
    fn a_box_breath_countdown_with_auto_finish_saves_by_itself() {
        let t0 = Duration::from_secs(100);
        let settings = SessionSettings { auto_finish: true, ..box_breath_settings(Some(16)) };
        let mut s = Session::start_running(settings, t0);
        let _ = s.tick(t0 + Duration::from_millis(500));
        let effects = s.tick(t0 + Duration::from_secs(16));
        assert_eq!(effects[..2], [Effect::Completed { planned_secs: 16 }, Effect::Ended { duration_secs: 16, save_now: true }], "{effects:?}");
        assert!(!effects.iter().any(|e| matches!(e, Effect::Ended { save_now: false, .. })));
        assert!(matches!(s.phase(), SessionPhase::Stopped { .. }));
    }

    // ── enter_overtime (external force-transition) ───────────────────

    #[test]
    fn enter_overtime_from_running_transitions_and_emits_effect() {
        let mut s = Session::start_running(
            guided_settings(600, false),
            Duration::from_secs(100),
        );
        let effects = s.enter_overtime(Duration::ZERO);
        assert!(effects.iter().any(|e| matches!(e, Effect::Completed { .. })), "{effects:?}");
        assert_eq!(s.phase(), SessionPhase::Overtime);
    }

    /// A guided preview's audio can end while a Timer or Box Breath
    /// session runs (started from the widget). Its end must not move
    /// that session into Overtime.
    #[test]
    fn only_a_guided_session_enters_overtime_from_outside() {
        for settings in [
            timer_countdown_settings(600),
            timer_stopwatch_settings(),
            box_breath_settings(Some(600)),
        ] {
            let mut s = Session::start_running(settings, Duration::from_secs(100));
            assert!(s.enter_overtime(Duration::ZERO).is_empty());
            assert_eq!(s.phase(), SessionPhase::Running);
        }
    }

    #[test]
    fn enter_overtime_is_idempotent_in_overtime() {
        let mut s = Session::start_running(
            timer_countdown_settings(60),
            Duration::from_secs(100),
        );
        let _ = s.tick(Duration::from_secs(160)); // transition Running → Overtime
        assert_eq!(s.phase(), SessionPhase::Overtime);
        let effects = s.enter_overtime(Duration::ZERO);
        assert!(effects.is_empty());
        assert_eq!(s.phase(), SessionPhase::Overtime);
    }

    #[test]
    fn enter_overtime_during_prep_is_a_noop() {
        let mut s = Session::start_prep(
            timer_settings_with_prep(30),
            Duration::from_secs(100),
        );
        let effects = s.enter_overtime(Duration::ZERO);
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
        let effects = s.enter_overtime(Duration::ZERO);
        assert!(effects.is_empty());
        assert!(matches!(s.phase(), SessionPhase::Stopped { .. }));
    }

    // ── Session::start — what plays at the start ──────────────────
    //
    // The starting bell must ring whether or not prep is on. Shells
    // dispatch exactly what `start` returns; there is no second call
    // to remember (Android once forgot it: no starting bell without
    // prep).

    fn starting_cue() -> crate::bells::BellCue {
        crate::bells::BellCue {
            sound_uuid: "start-sound".into(),
            vibration_pattern_uuid: "start-pattern".into(),
            signal_mode: SignalMode::Sound,
            volume: Default::default(),
        }
    }

    fn the_starting_bell() -> Effect {
        Effect::Fire { channel: FireChannel::Starting, phase: None, cue: starting_cue() }
    }

    fn with_starting_bell(mut settings: SessionSettings) -> SessionSettings {
        settings.starting_bell = Some(starting_cue());
        settings
    }

    fn starting_bells(effects: &[Effect]) -> usize {
        effects.iter().filter(|e| matches!(e, Effect::Fire { channel: FireChannel::Starting, .. })).count()
    }

    /// Start at 100 s, tick every second through prep, the 600 s
    /// target and a minute of overtime. Returns what `start` returned
    /// and everything after it.
    fn run_whole_session(settings: SessionSettings) -> (Vec<Effect>, Vec<Effect>) {
        let prep = u64::from(settings.prep_secs.unwrap_or(0));
        let t0 = Duration::from_secs(100);
        let (mut s, at_start) = Session::start(settings, t0);
        let mut later = Vec::new();
        for secs in 1..=prep + 660 {
            later.extend(s.tick(t0 + Duration::from_secs(secs)));
        }
        (at_start, later)
    }

    #[test]
    fn start_without_prep_runs_and_rings_the_starting_bell_at_once() {
        let settings = with_starting_bell(timer_countdown_settings(600));
        let (s, effects) = Session::start(settings, Duration::from_secs(100));
        assert_eq!(s.phase(), SessionPhase::Running);
        assert_eq!(effects, vec![the_starting_bell()]);
    }

    #[test]
    fn start_with_prep_waits_and_rings_the_starting_bell_when_prep_ends() {
        let settings = with_starting_bell(timer_prep_settings(10, 600));
        let t0 = Duration::from_secs(100);
        let (mut s, effects) = Session::start(settings, t0);
        assert_eq!(s.phase(), SessionPhase::Prep);
        assert!(effects.is_empty(), "{effects:?}");
        for secs in 1..10 {
            let effects = s.tick(t0 + Duration::from_secs(secs));
            assert_eq!(starting_bells(&effects), 0, "{secs} s into prep");
        }
        let effects = s.tick(t0 + Duration::from_secs(10));
        assert_eq!(s.phase(), SessionPhase::Running);
        assert_eq!(starting_bells(&effects), 1, "{effects:?}");
    }

    #[test]
    fn the_starting_bell_rings_exactly_once_per_session() {
        for prep in [None, Some(10)] {
            let mut settings = with_starting_bell(timer_countdown_settings(600));
            settings.prep_secs = prep;
            let (at_start, later) = run_whole_session(settings);
            assert_eq!(
                starting_bells(&at_start) + starting_bells(&later), 1,
                "prep {prep:?}",
            );
        }
    }

    #[test]
    fn stopwatch_sessions_ring_the_starting_bell_too() {
        let settings = with_starting_bell(timer_stopwatch_settings());
        let (_, effects) = Session::start(settings, Duration::from_secs(100));
        assert_eq!(effects, vec![the_starting_bell()]);
    }

    #[test]
    fn no_starting_bell_configured_rings_none() {
        for prep in [None, Some(10)] {
            let mut settings = timer_countdown_settings(600);
            settings.prep_secs = prep;
            let (at_start, later) = run_whole_session(settings);
            assert!(at_start.is_empty(), "prep {prep:?}: {at_start:?}");
            assert_eq!(starting_bells(&later), 0, "prep {prep:?}");
        }
    }

    #[test]
    fn a_starting_bell_muted_by_the_cues_toggle_rings_none() {
        // A sound-only bell under a vibration-only Cues setting has
        // no channel left.
        for prep in [None, Some(10)] {
            let mut settings = with_starting_bell(timer_countdown_settings(600));
            settings.prep_secs = prep;
            settings.signal_mode_override = SignalMode::Vibration;
            let (at_start, later) = run_whole_session(settings);
            assert_eq!(starting_bells(&at_start) + starting_bells(&later), 0, "prep {prep:?}");
        }
    }

    #[test]
    fn the_cues_toggle_shapes_the_starting_bell_at_start() {
        let mut settings = with_starting_bell(timer_countdown_settings(600));
        settings.starting_bell = Some(crate::bells::BellCue {
            signal_mode: SignalMode::Both,
            ..starting_cue()
        });
        settings.signal_mode_override = SignalMode::Vibration;
        let (_, effects) = Session::start(settings, Duration::from_secs(100));
        assert_eq!(
            effects,
            vec![Effect::Fire {
                channel: FireChannel::Starting,
                phase: None,
                cue: crate::bells::BellCue { signal_mode: SignalMode::Vibration, ..starting_cue() },
            }],
        );
    }

    #[test]
    fn start_returns_nothing_but_the_starting_bell() {
        // A due-at-once interval bell still waits for the first tick,
        // as before; the start edge only carries the starting bell.
        let mut settings = with_starting_bell(timer_countdown_settings(600));
        settings.bells = vec![fixed_bell(0, "at-zero")];
        let (_, effects) = Session::start(settings, Duration::from_secs(100));
        assert_eq!(effects, vec![the_starting_bell()]);
    }

    // ── Each effect carries its bell's volume ─────────────────────

    fn cue_at(percent: f64) -> crate::bells::BellCue {
        crate::bells::BellCue {
            volume: crate::bell_volume::BellVolume::from_percent(percent),
            ..starting_cue()
        }
    }

    fn volume_of(effect: &Effect) -> u8 {
        let Effect::Fire { cue, .. } = effect else { panic!("a fire effect: {effect:?}") };
        cue.volume.percent()
    }

    #[test]
    fn the_starting_bell_rings_at_its_volume() {
        let mut settings = timer_countdown_settings(600);
        settings.starting_bell = Some(cue_at(30.0));
        let (_, effects) = Session::start(settings, Duration::from_secs(100));
        assert_eq!(effects.len(), 1);
        assert_eq!(volume_of(&effects[0]), 30);
    }

    #[test]
    fn the_end_bell_rings_at_its_volume() {
        let mut settings = timer_countdown_settings(60);
        settings.end_bell = Some(cue_at(85.0));
        let t0 = Duration::from_secs(100);
        let (mut s, _) = Session::start(settings, t0);
        let effects = s.tick(t0 + Duration::from_secs(60));
        let end = effects.iter().find(|e| matches!(e, Effect::Fire { channel: FireChannel::End, .. })).expect("end bell");
        assert_eq!(volume_of(end), 85);
    }

    #[test]
    fn each_interval_bell_rings_at_its_own_volume() {
        let mut settings = timer_countdown_settings(600);
        let mut quiet = fixed_bell(60, "quiet");
        quiet.cue.volume = crate::bell_volume::BellVolume::from_percent(10.0);
        let mut loud = fixed_bell(120, "loud");
        loud.cue.volume = crate::bell_volume::BellVolume::from_percent(95.0);
        settings.bells = vec![quiet, loud];
        let t0 = Duration::from_secs(100);
        let (mut s, _) = Session::start(settings, t0);
        let mut fired = Vec::new();
        for secs in 1..=130 {
            for e in s.tick(t0 + Duration::from_secs(secs)) {
                if let Effect::Fire { channel: FireChannel::Interval, ref cue, .. } = e {
                    fired.push((cue.sound_uuid.as_str().to_string(), volume_of(&e)));
                }
            }
        }
        assert_eq!(fired, vec![("quiet".to_string(), 10), ("loud".to_string(), 95)]);
    }

    #[test]
    fn each_box_breath_cue_rings_at_its_phases_volume() {
        let mut settings = box_breath_settings(None);
        let cues = settings.box_breath_cues.as_mut().unwrap();
        cues.in_phase = Some(cue_at(20.0));
        cues.hold_in = Some(cue_at(40.0));
        cues.out_phase = Some(cue_at(60.0));
        cues.hold_out = Some(cue_at(80.0));
        let t0 = Duration::from_secs(100);
        let (mut s, _) = Session::start(settings, t0);
        let mut by_phase = std::collections::BTreeMap::new();
        for ms in (0..=20_000).step_by(250) {
            for e in s.tick(t0 + Duration::from_millis(ms)) {
                if let Effect::Fire { phase: Some(phase), .. } = e {
                    by_phase.entry(format!("{phase:?}")).or_insert(volume_of(&e));
                }
            }
        }
        assert_eq!(by_phase.len(), 4, "{by_phase:?}");
        assert_eq!(by_phase["In"], 20);
        assert_eq!(by_phase["HoldIn"], 40);
        assert_eq!(by_phase["Out"], 60);
        assert_eq!(by_phase["HoldOut"], 80);
    }

    // ── Media control ────────────────────────────────────────────────

    fn bar(elapsed_ms: u64, total_secs: u64) -> Option<MediaBar> {
        Some(MediaBar { elapsed: Duration::from_millis(elapsed_ms), total_secs })
    }

    #[test]
    fn the_media_control_counts_down_a_timed_session() {
        let s = Session::start_running(timer_countdown_settings(1200), Duration::from_secs(100));
        assert_eq!(
            s.media_status(Duration::from_millis(100_000 + 312_400)),
            Some(MediaStatus { line: MediaLine::Left(888), bar: bar(312_400, 1200), standing: false }),
        );
    }

    #[test]
    fn the_media_control_shows_the_time_left_when_the_app_counts_up() {
        let breath = Session::start_running(box_breath_settings(Some(1200)), Duration::from_secs(100));
        let guided = Session::start_running(guided_settings(1200, true), Duration::from_secs(100));
        for s in [breath, guided] {
            assert_eq!(
                s.media_status(Duration::from_secs(412)),
                Some(MediaStatus { line: MediaLine::Left(888), bar: bar(312_000, 1200), standing: false }),
            );
        }
    }

    #[test]
    fn the_media_control_counts_up_without_an_end() {
        let timer = Session::start_running(timer_stopwatch_settings(), Duration::from_secs(100));
        let breath = Session::start_running(box_breath_settings(None), Duration::from_secs(100));
        for s in [timer, breath] {
            assert_eq!(
                s.media_status(Duration::from_millis(854_900)),
                Some(MediaStatus { line: MediaLine::SoFar(754), bar: None, standing: false }),
            );
        }
    }

    #[test]
    fn the_media_control_shows_the_preparation() {
        let mut s = Session::start_prep(timer_prep_settings(10, 1200), Duration::from_secs(100));
        let preparing = |standing: bool| {
            let bar = bar(5_000, 10);
            Some(MediaStatus { line: MediaLine::Preparing(5), bar, standing })
        };
        assert_eq!(s.media_status(Duration::from_secs(105)), preparing(false));
        s.pause(Duration::from_secs(105));
        assert_eq!(s.media_status(Duration::from_secs(160)), preparing(true));
    }

    #[test]
    fn the_media_control_stands_still_while_paused() {
        let mut timer = Session::start_running(timer_countdown_settings(1200), Duration::from_secs(100));
        timer.pause(Duration::from_secs(412));
        assert_eq!(
            timer.media_status(Duration::from_secs(900)),
            Some(MediaStatus { line: MediaLine::Left(888), bar: bar(312_000, 1200), standing: true }),
        );
        let mut watch = Session::start_running(timer_stopwatch_settings(), Duration::from_secs(100));
        watch.pause(Duration::from_secs(854));
        assert_eq!(
            watch.media_status(Duration::from_secs(900)),
            Some(MediaStatus { line: MediaLine::SoFar(754), bar: None, standing: true }),
        );
    }

    #[test]
    fn the_media_control_shows_overtime_with_a_full_bar() {
        let mut s = Session::start_running(timer_countdown_settings(1200), Duration::from_secs(100));
        let _ = s.tick(Duration::from_secs(1300));
        assert_eq!(s.phase(), SessionPhase::Overtime);
        assert_eq!(
            s.media_status(Duration::from_millis(1_433_700)),
            Some(MediaStatus { line: MediaLine::Overtime(133), bar: bar(1_200_000, 1200), standing: true }),
        );
        let _ = s.finish_overtime();
        assert_eq!(s.media_status(Duration::from_secs(1500)), None, "gone once stopped");
    }

    // ── Guided audio stops when the session enters Overtime ─────────

    fn guided_settings(duration_secs: u32, count_up_display: bool) -> SessionSettings {
        SessionSettings {
            shape: SessionShape::Guided { duration_secs, count_up_display },
            end_bell: Some(cue_at(70.0)),
            ..timer_countdown_settings(duration_secs)
        }
    }

    fn position(effects: &[Effect], want: impl Fn(&Effect) -> bool) -> usize {
        effects.iter().position(want).expect("effect present")
    }

    #[test]
    fn guided_reaching_its_length_stops_the_track_before_the_end_bell() {
        let t0 = Duration::from_secs(100);
        let (mut s, _) = Session::start(guided_settings(60, false), t0);
        let effects = s.tick(t0 + Duration::from_secs(60));
        let stop = position(&effects, |e| matches!(e, Effect::StopGuidedAudio));
        let bell = position(&effects, |e| matches!(e, Effect::Fire { channel: FireChannel::End, .. }));
        assert!(stop < bell, "the track goes quiet before the end bell: {effects:?}");
        assert!(effects.iter().any(|e| matches!(e, Effect::Completed { .. })));
    }

    #[test]
    fn guided_track_ending_first_also_stops_it_before_the_end_bell() {
        // The EOS path: the shell forces Overtime when the track ends.
        let t0 = Duration::from_secs(100);
        let (mut s, _) = Session::start(guided_settings(60, false), t0);
        let _ = s.tick(t0 + Duration::from_secs(30));
        let effects = s.enter_overtime(Duration::ZERO);
        assert_eq!(effects.first(), Some(&Effect::StopGuidedAudio), "{effects:?}");
        assert!(effects.iter().any(|e| matches!(e, Effect::Fire { channel: FireChannel::End, .. })));
    }

    #[test]
    fn guided_counting_up_still_stops_the_track_at_its_length() {
        let t0 = Duration::from_secs(100);
        let (mut s, _) = Session::start(guided_settings(60, true), t0);
        let effects = s.tick(t0 + Duration::from_secs(60));
        assert_eq!(effects.first(), Some(&Effect::StopGuidedAudio), "{effects:?}");
    }

    #[test]
    fn guided_stops_the_track_only_once() {
        let t0 = Duration::from_secs(100);
        let (mut s, _) = Session::start(guided_settings(60, false), t0);
        let _ = s.tick(t0 + Duration::from_secs(60));
        let later = s.tick(t0 + Duration::from_secs(75));
        assert!(!later.contains(&Effect::StopGuidedAudio));
        assert!(s.enter_overtime(Duration::ZERO).is_empty(), "already in Overtime");
    }

    #[test]
    fn guided_running_ticks_leave_the_track_alone() {
        let t0 = Duration::from_secs(100);
        let (mut s, _) = Session::start(guided_settings(60, false), t0);
        for secs in [1, 30, 59] {
            assert!(!s.tick(t0 + Duration::from_secs(secs)).contains(&Effect::StopGuidedAudio));
        }
    }

    #[test]
    fn a_timer_entering_overtime_has_no_track_to_stop() {
        let t0 = Duration::from_secs(100);
        let mut settings = timer_countdown_settings(60);
        settings.end_bell = Some(cue_at(70.0));
        let (mut s, _) = Session::start(settings, t0);
        let effects = s.tick(t0 + Duration::from_secs(60));
        assert!(effects.iter().any(|e| matches!(e, Effect::Completed { .. })));
        assert!(!effects.contains(&Effect::StopGuidedAudio));
    }

    #[test]
    fn a_box_breath_end_has_no_track_to_stop() {
        let t0 = Duration::from_secs(100);
        let (mut s, _) = Session::start(box_breath_settings(Some(16)), t0);
        let mut all = Vec::new();
        for secs in 1..=16 {
            all.extend(s.tick(t0 + Duration::from_secs(secs)));
        }
        assert!(all.iter().any(|e| matches!(e, Effect::Ended { save_now: false, .. })));
        assert!(!all.contains(&Effect::StopGuidedAudio));
    }

    // ── The hero freezes at the planned length in Overtime ──────────

    #[test]
    fn a_timer_in_overtime_shows_its_planned_length() {
        let t0 = Duration::from_secs(100);
        let (mut s, _) = Session::start(timer_countdown_settings(60), t0);
        let _ = s.tick(t0 + Duration::from_secs(60));
        assert_eq!(s.display_secs(t0 + Duration::from_secs(60)), 60);
        assert_eq!(s.display_secs(t0 + Duration::from_secs(95)), 60, "frozen, not 00:00");
    }

    #[test]
    fn guided_in_overtime_shows_the_file_length() {
        let t0 = Duration::from_secs(100);
        let (mut s, _) = Session::start(guided_settings(60, false), t0);
        let _ = s.tick(t0 + Duration::from_secs(60));
        assert_eq!(s.display_secs(t0 + Duration::from_secs(80)), 60);
    }

    #[test]
    fn guided_counting_up_stops_counting_in_overtime() {
        let t0 = Duration::from_secs(100);
        let (mut s, _) = Session::start(guided_settings(60, true), t0);
        let _ = s.tick(t0 + Duration::from_secs(60));
        assert_eq!(s.display_secs(t0 + Duration::from_secs(80)), 60, "not 80");
    }

    #[test]
    fn a_track_ending_early_still_shows_the_planned_length() {
        let t0 = Duration::from_secs(100);
        let (mut s, _) = Session::start(guided_settings(60, false), t0);
        let _ = s.tick(t0 + Duration::from_secs(58));
        let _ = s.enter_overtime(Duration::ZERO);
        assert_eq!(s.display_secs(t0 + Duration::from_secs(59)), 60, "not the 2 s left");
    }

    #[test]
    fn running_display_is_unchanged() {
        let t0 = Duration::from_secs(100);
        let (s, _) = Session::start(timer_countdown_settings(60), t0);
        assert_eq!(s.display_secs(t0 + Duration::from_secs(20)), 40);
    }

    /// A blank screen or suspend delays the ending tick; the session
    /// was saved with the late elapsed time (40 min for a 5 min sit).
    #[test]
    fn a_box_breath_session_ending_late_saves_its_set_length() {
        let mut s = Session::start_running(box_breath_settings(Some(320)), Duration::from_secs(100));
        let effects = s.tick(Duration::from_secs(100 + 2400));
        assert!(effects.contains(&Effect::Ended { duration_secs: 320, save_now: false }), "{effects:?}");
        assert_eq!(s.final_duration_secs(), Some(320));
    }

    /// A suspend during preparation restarted the countdown from 0
    /// on waking; without preparation the same suspend ends it.
    #[test]
    fn a_late_tick_at_the_end_of_prep_keeps_the_countdown_on_time() {
        let t0 = Duration::from_secs(100);
        let mut s = Session::start_prep(timer_prep_settings(30, 600), t0);
        let _ = s.tick(t0 + Duration::from_secs(5));
        let _ = s.tick(t0 + Duration::from_secs(20 * 60)); // wakes: ends prep
        let effects = s.tick(t0 + Duration::from_secs(20 * 60 + 1));
        assert!(effects.iter().any(|e| matches!(e, Effect::Completed { .. })), "{effects:?}");
    }

    #[test]
    fn a_slightly_late_prep_tick_loses_no_countdown_time() {
        let t0 = Duration::from_secs(100);
        let mut s = Session::start_prep(timer_prep_settings(30, 600), t0);
        let _ = s.tick(t0 + Duration::from_millis(30_800));
        assert_eq!(s.elapsed(t0 + Duration::from_millis(30_800)), Duration::from_millis(800));
    }

    /// The tick that crossed into overtime skipped the bells, so a
    /// bell due at 10:00 rang at 10:01, after the end bell.
    #[test]
    fn an_interval_bell_due_at_the_end_gives_way_to_the_end_bell() {
        let mut settings = timer_countdown_settings(600);
        settings.bells = vec![interval_bell(5, "ding")];
        settings.end_bell = Some(crate::bells::BellCue {
            sound_uuid: "end".into(),
            vibration_pattern_uuid: "pattern".into(),
            signal_mode: SignalMode::Sound,
            volume: Default::default(),
        });
        let t0 = Duration::from_secs(100);
        let mut s = Session::start_running(settings, t0);
        assert!(s.tick(t0 + Duration::from_secs(300)).iter().any(|e| matches!(e, Effect::Fire { channel: FireChannel::Interval, phase: None, .. })));
        let effects = s.tick(t0 + Duration::from_secs(600));
        assert!(effects.iter().any(|e| matches!(e, Effect::Fire { channel: FireChannel::End, .. })), "{effects:?}");
        assert!(!effects.iter().any(|e| matches!(e, Effect::Fire { channel: FireChannel::Interval, phase: None, .. })), "{effects:?}");
        let effects = s.tick(t0 + Duration::from_secs(601));
        assert!(!effects.iter().any(|e| matches!(e, Effect::Fire { channel: FireChannel::Interval, phase: None, .. })), "{effects:?}");
    }

    /// The last tick of a cycle-aligned session also entered the next
    /// inhale, so "breathe in" played on top of the end bell.
    #[test]
    fn box_breath_ends_without_cueing_another_cycle() {
        let t0 = Duration::from_secs(100);
        let mut s = Session::start_running(box_breath_settings(Some(320)), t0);
        let mut last = Vec::new();
        for sec in 1..=320 {
            last = s.tick(t0 + Duration::from_secs(sec));
        }
        assert!(last.contains(&Effect::Ended { duration_secs: 320, save_now: false }), "{last:?}");
        assert!(!last.iter().any(|e| matches!(e, Effect::Fire { phase: Some(_), .. })), "{last:?}");
    }

    /// Box Breath has no starting bell, so with cues on the first
    /// signal was "hold" at 4 s.
    #[test]
    fn box_breath_cues_the_first_inhale_at_the_start() {
        let (mut s, effects) = Session::start(box_breath_settings(None), Duration::from_secs(100));
        assert!(
            effects.iter().any(|e| matches!(e, Effect::Fire { phase: Some(BoxBreathPhaseId::In), .. })),
            "{effects:?}"
        );
        let effects = s.tick(Duration::from_millis(100_500));
        assert!(!effects.iter().any(|e| matches!(e, Effect::Fire { phase: Some(_), .. })), "no second inhale cue");
    }

    /// A suspend that covered preparation and the whole countdown rang
    /// the starting bell on waking, minutes late, then the end bell.
    #[test]
    fn a_suspend_past_the_end_during_prep_rings_only_the_end_bell() {
        let cue = || crate::bells::BellCue {
            sound_uuid: "bell".into(),
            vibration_pattern_uuid: "pattern".into(),
            signal_mode: SignalMode::Sound,
            volume: Default::default(),
        };
        let mut settings = timer_prep_settings(30, 600);
        settings.starting_bell = Some(cue());
        settings.end_bell = Some(cue());
        let t0 = Duration::from_secs(100);
        let mut s = Session::start_prep(settings, t0);
        let _ = s.tick(t0 + Duration::from_secs(5));
        let mut effects = s.tick(t0 + Duration::from_secs(20 * 60));
        effects.extend(s.tick(t0 + Duration::from_secs(20 * 60 + 1)));
        assert!(!effects.iter().any(|e| matches!(e, Effect::Fire { channel: FireChannel::Starting, .. })), "{effects:?}");
        assert_eq!(effects.iter().filter(|e| matches!(e, Effect::Fire { channel: FireChannel::End, .. })).count(), 1, "{effects:?}");
    }

    #[test]
    fn a_suspend_past_the_end_during_prep_rings_no_interval_bell() {
        let cue = || crate::bells::BellCue {
            sound_uuid: "bell".into(),
            vibration_pattern_uuid: "pattern".into(),
            signal_mode: SignalMode::Sound,
            volume: Default::default(),
        };
        let mut settings = timer_prep_settings(30, 60);
        settings.starting_bell = Some(cue());
        settings.end_bell = Some(cue());
        settings.bells = vec![interval_bell(1, "interval")];
        let t0 = Duration::from_secs(100);
        let mut s = Session::start_prep(settings, t0);
        let _ = s.tick(t0 + Duration::from_secs(5));
        let mut effects = Vec::new();
        for secs in 180..185 {
            effects.extend(s.tick(t0 + Duration::from_secs(secs)));
        }
        assert!(!effects.iter().any(|e| matches!(e, Effect::Fire { channel: FireChannel::Interval, phase: None, .. })), "{effects:?}");
        assert_eq!(effects.iter().filter(|e| matches!(e, Effect::Fire { channel: FireChannel::End, .. })).count(), 1, "{effects:?}");
    }

    // ── The bell ring ─────────────────────────────────────────────

    /// A repeating bell every `base` s ± `jitter` s, `step` s more per
    /// gap, ending at `end`, its first ring drawn at `first`.
    fn every(base: u32, jitter: u32, step: u32, end: Option<u64>, first: u64) -> ActiveBell {
        use crate::bells::BellSchedule;
        let mut schedule = BellSchedule::every(base, jitter, step, end, 0.5);
        if let BellSchedule::Interval { next_ring_secs, .. } = &mut schedule {
            *next_ring_secs = first;
        }
        ActiveBell { schedule, ..interval_bell(1, "sound") }
    }

    fn timer_with(target_secs: u32, bells: Vec<ActiveBell>) -> Session {
        Session::start(SessionSettings { bells, ..timer_countdown_settings(target_secs) }, Duration::ZERO).0
    }

    fn at(s: u64) -> Duration {
        Duration::from_secs(s)
    }

    fn small(secs: f64, of: f64, rung: bool) -> Bead {
        Bead { at: secs / of, large: false, rung }
    }

    fn ring_of(s: &Session, now: u64) -> Ring {
        s.bell_ring(at(now)).ring.expect("a ring")
    }

    #[test]
    fn the_ring_fills_with_the_session() {
        let mut s = timer_with(1200, Vec::new());
        assert_eq!(ring_of(&s, 300).filled, 0.25);
        s.pause(at(300));
        assert_eq!(ring_of(&s, 480).filled, 0.25);
        assert!(ring_of(&s, 480).start_filled);
    }

    #[test]
    fn only_a_known_length_gets_a_ring() {
        let stopwatch = SessionSettings { shape: SessionShape::TimerStopwatch, ..timer_countdown_settings(600) };
        let (s, _) = Session::start(stopwatch, Duration::ZERO);
        assert_eq!(s.bell_ring(at(60)).ring, None);
        let (s, _) = Session::start(box_breath_settings(Some(300)), Duration::ZERO);
        assert_eq!(s.bell_ring(at(60)), BellRing::default());
        let (s, _) = Session::start(guided_settings(600, true), Duration::ZERO);
        assert_eq!(
            s.bell_ring(at(150)).ring,
            Some(Ring { filled: 0.25, prep: None, start_filled: true, beads: Vec::new() })
        );
    }

    #[test]
    fn one_off_bells_are_large_beads() {
        let mut s = timer_with(1800, vec![fixed_bell(180, "a"), fixed_bell(1680, "b")]);
        s.tick(at(600));
        assert_eq!(
            ring_of(&s, 600).beads,
            [
                Bead { at: 180.0 / 1800.0, large: true, rung: true },
                Bead { at: 1680.0 / 1800.0, large: true, rung: false },
            ]
        );
        assert_eq!(s.bell_ring(at(600)).counters, [BellCounter { rung: 1, total: Some(2) }], "one-off bells count too");
    }

    #[test]
    fn a_one_off_bell_slept_through_stays_hollow() {
        // Asleep from 27:00 to 31:00: the 28:00 bell never rang.
        let mut s = timer_with(1800, vec![fixed_bell(1680, "a")]);
        s.tick(at(1620));
        s.tick(at(1860));
        assert_eq!(s.phase(), SessionPhase::Overtime);
        assert_eq!(ring_of(&s, 1860).beads, [Bead { at: 1680.0 / 1800.0, large: true, rung: false }]);
    }

    #[test]
    fn repeating_bells_are_small_beads() {
        let five = |rung: usize| -> Vec<Bead> {
            [300.0, 600.0, 900.0, 1200.0, 1500.0].iter().enumerate().map(|(i, &t)| small(t, 1800.0, i < rung)).collect()
        };
        let mut s = timer_with(1800, vec![every(300, 0, 0, Some(1800), 300)]);
        for t in [300, 600, 720] {
            s.tick(at(t));
        }
        assert_eq!(ring_of(&s, 720).beads, five(2));
        assert_eq!(s.bell_ring(at(720)).counters, [BellCounter { rung: 2, total: Some(5) }]);
        for t in [900, 1200, 1500, 1799] {
            s.tick(at(t));
        }
        assert_eq!(s.bell_ring(at(1799)).counters, [BellCounter { rung: 5, total: Some(5) }]);
        // 30:00 is the end bell's: no sixth ring is counted.
        s.tick(at(1800));
        assert_eq!(s.bell_ring(at(1800)).counters, [BellCounter { rung: 5, total: None }]);

        // A bell with no time rings each second but is not drawn.
        let s0 = timer_with(1800, vec![every(0, 0, 0, Some(1800), 1)]);
        assert_eq!(s0.bell_ring(at(0)), BellRing { ring: Some(ring_of(&s0, 0)), counters: Vec::new() });
        assert_eq!(ring_of(&s0, 0).beads, []);

        // Growing by a minute: 5, 11, 18 and 26 minutes.
        let s = timer_with(1800, vec![every(300, 0, 60, Some(1800), 300)]);
        let beads: Vec<Bead> = [300.0, 660.0, 1080.0, 1560.0].iter().map(|&t| small(t, 1800.0, false)).collect();
        assert_eq!(ring_of(&s, 0).beads, beads);

        // Asleep from 4:00 to 12:00: one ring, for the 10 minute bead.
        let mut s = timer_with(1800, vec![every(300, 0, 0, Some(1800), 300)]);
        s.tick(at(240));
        s.tick(at(720));
        assert_eq!(
            ring_of(&s, 720).beads,
            [small(300.0, 1800.0, false), small(720.0, 1800.0, true), small(900.0, 1800.0, false),
             small(1200.0, 1800.0, false), small(1500.0, 1800.0, false)]
        );
        assert_eq!(s.bell_ring(at(720)).counters, [BellCounter { rung: 1, total: Some(5) }]);
    }

    #[test]
    fn a_random_bell_rides_the_fill_until_it_rings() {
        let earliest = [240.0, 540.0, 840.0, 1140.0, 1440.0, 1740.0];
        // Late: rings first at 5:40.
        let mut s = timer_with(1800, vec![every(300, 60, 0, Some(1800), 340)]);
        let hollow: Vec<Bead> = earliest.iter().map(|&t| small(t, 1800.0, false)).collect();
        assert_eq!(ring_of(&s, 180).beads, hollow);
        s.tick(at(320));
        assert_eq!(ring_of(&s, 320).beads[0], small(320.0, 1800.0, false));
        s.tick(at(340));
        assert_eq!(ring_of(&s, 400).beads[..2], [small(340.0, 1800.0, true), small(540.0, 1800.0, false)]);
        // Early: rings first at 4:30.
        let mut s = timer_with(1800, vec![every(300, 60, 0, Some(1800), 270)]);
        assert_eq!(ring_of(&s, 255).beads[0], small(255.0, 1800.0, false));
        s.tick(at(270));
        assert_eq!(ring_of(&s, 280).beads[..2], [small(270.0, 1800.0, true), small(540.0, 1800.0, false)]);
    }

    /// #280: a bell the mode's signal setting mutes never rang, so it
    /// is neither heard nor in the total.
    #[test]
    fn a_muted_starting_or_end_bell_is_not_counted() {
        let mut ten = fixed_bell(600, "ten");
        ten.cue.signal_mode = SignalMode::Both;
        let settings = SessionSettings {
            bells: vec![ten],
            starting_bell: Some(starting_cue()),
            end_bell: Some(starting_cue()),
            signal_mode_override: SignalMode::Vibration,
            ..timer_countdown_settings(1800)
        };
        let (mut s, _) = Session::start(settings, Duration::ZERO);
        assert_eq!(s.bell_ring(at(0)).counters, [BellCounter { rung: 0, total: Some(1) }]);
        s.tick(at(720));
        assert_eq!(s.bell_ring(at(720)).counters, [BellCounter { rung: 1, total: Some(1) }]);
        s.tick(at(1800));
        assert_eq!(s.bell_ring(at(1800)).counters, [BellCounter { rung: 1, total: None }]);
    }

    /// #280: a suspend through preparation past the end skips the
    /// starting bell; the counter must not hear it.
    #[test]
    fn a_starting_bell_skipped_by_a_suspend_is_not_counted() {
        let settings = SessionSettings {
            bells: vec![fixed_bell(30, "half")],
            starting_bell: Some(starting_cue()),
            end_bell: Some(starting_cue()),
            ..timer_prep_settings(30, 60)
        };
        let (mut s, _) = Session::start(settings, Duration::ZERO);
        let mut rang = s.tick(at(5));
        rang.extend(s.tick(at(185)));
        rang.extend(s.tick(at(186)));
        let channels: Vec<FireChannel> = rang.iter().filter_map(|e| match e {
            Effect::Fire { channel, .. } => Some(*channel),
            _ => None,
        }).collect();
        assert_eq!(channels, [FireChannel::End], "only the end bell rings");
        assert_eq!(s.bell_ring(at(186)).counters, [BellCounter { rung: 1, total: None }]);
    }

    /// #279: woken past three fixed bells, only the latest rings.
    #[test]
    fn after_a_suspend_only_the_latest_missed_fixed_bell_rings() {
        let mut s = timer_with(3600, vec![fixed_bell(600, "10"), fixed_bell(1200, "20"), fixed_bell(1800, "30")]);
        s.tick(at(300));
        let rang: Vec<String> = s.tick(at(2100)).iter().filter_map(|e| match e {
            Effect::Fire { cue, .. } => Some(cue.sound_uuid.as_str().to_string()),
            _ => None,
        }).collect();
        assert_eq!(rang, ["30"]);
        let ring = s.bell_ring(at(2100));
        let heard: Vec<bool> = ring.ring.unwrap().beads.iter().map(|b| b.rung).collect();
        assert_eq!(heard, [false, false, true]);
        assert_eq!(ring.counters, [BellCounter { rung: 1, total: Some(3) }]);
    }

    #[test]
    fn the_counter_counts_every_bell() {
        let counter = |rung, total| vec![BellCounter { rung, total: Some(total) }];
        // Two repeating bells share one count: 2 of 5 and 1 of 4.
        let mut s = timer_with(1800, vec![every(420, 0, 0, Some(1800), 420), every(300, 0, 0, Some(1800), 300)]);
        for t in [300, 420, 600, 720] {
            s.tick(at(t));
        }
        assert_eq!(s.bell_ring(at(720)).counters, counter(3, 9));

        // Starting bell, a one-off at 3:00, every 5 min, end bell: 4 of 8 at 12:00.
        let settings = SessionSettings {
            bells: vec![fixed_bell(180, "a"), every(300, 0, 0, Some(1800), 300)],
            starting_bell: Some(cue_at(70.0)),
            end_bell: Some(cue_at(70.0)),
            ..timer_countdown_settings(1800)
        };
        let (mut s, _) = Session::start(settings.clone(), Duration::ZERO);
        assert_eq!(s.bell_ring(at(0)).counters, counter(1, 8), "the starting bell rang");
        for t in [180, 300, 600, 720] {
            s.tick(at(t));
        }
        assert_eq!(s.bell_ring(at(720)).counters, counter(4, 8));
        for t in [900, 1200, 1500, 1800] {
            s.tick(at(t));
        }
        assert_eq!(s.bell_ring(at(1800)).counters, [BellCounter { rung: 8, total: None }], "the end bell rang");

        // Saved early, at 10:00: the end bell never rings, however long the page stays.
        let (mut early, _) = Session::start(settings.clone(), Duration::ZERO);
        for t in [180, 300, 600] {
            early.tick(at(t));
        }
        let _ = early.stop(at(600));
        assert_eq!(early.bell_ring(at(1900)).counters[0].rung, 4, "start, 3:00, 5:00, 10:00");

        // Preparation: the starting bell has not rung yet.
        let (s, _) = Session::start(SessionSettings { prep_secs: Some(10), ..settings }, Duration::ZERO);
        assert_eq!(s.bell_ring(at(5)).counters, counter(0, 8));

        // Only starting and end bells: nothing to count between them.
        let plain = SessionSettings { starting_bell: Some(cue_at(70.0)), end_bell: Some(cue_at(70.0)), ..timer_countdown_settings(1800) };
        assert_eq!(Session::start(plain, Duration::ZERO).0.bell_ring(at(60)).counters, []);
    }

    #[test]
    fn the_stopwatch_counts_without_a_total() {
        let settings = SessionSettings {
            shape: SessionShape::TimerStopwatch,
            bells: vec![every(300, 0, 0, None, 300)],
            starting_bell: Some(cue_at(70.0)),
            ..timer_countdown_settings(600)
        };
        let (mut s, _) = Session::start(settings, Duration::ZERO);
        for t in [300, 600, 720] {
            s.tick(at(t));
        }
        // The starting bell and two rings.
        assert_eq!(s.bell_ring(at(720)), BellRing { ring: None, counters: vec![BellCounter { rung: 3, total: None }] });
    }

    #[test]
    fn preparation_fills_the_small_ring() {
        let (mut s, _) = Session::start(timer_prep_settings(10, 600), Duration::ZERO);
        assert_eq!(
            s.bell_ring(at(5)).ring,
            Some(Ring { filled: 0.0, prep: Some(0.5), start_filled: false, beads: Vec::new() })
        );
        s.tick(at(10));
        let ring = ring_of(&s, 10);
        assert_eq!((ring.prep, ring.start_filled), (None, true));
    }

    #[test]
    fn overtime_shows_a_full_ring() {
        let mut s = timer_with(1800, vec![every(300, 60, 0, Some(1800), 280)]);
        let next = |s: &Session| match s.bells[0].schedule {
            crate::bells::BellSchedule::Interval { next_ring_secs, .. } => next_ring_secs,
            crate::bells::BellSchedule::Fixed { .. } => unreachable!(),
        };
        while next(&s) < 1800 {
            s.tick(at(next(&s)));
        }
        s.tick(at(1800));
        assert_eq!(s.phase(), SessionPhase::Overtime);
        let ring = s.bell_ring(at(1810));
        assert_eq!(ring.ring.map(|r| r.filled), Some(1.0));
        assert_eq!(ring.counters, [BellCounter { rung: 6, total: None }]);
        s.tick(at(next(&s)));
        assert_eq!(s.bell_ring(at(2200)).counters, [BellCounter { rung: 7, total: None }]);
    }

    // ── one-session-end ─────────────────────────────────────────────

    fn completed(effects: &[Effect]) -> Vec<u64> {
        effects.iter().filter_map(|e| match e { Effect::Completed { planned_secs } => Some(*planned_secs), _ => None }).collect()
    }

    #[test]
    fn only_an_end_reached_by_itself_is_complete() {
        let t0 = Duration::from_secs(100);
        let at = |secs| t0 + Duration::from_secs(secs);
        // A Timer runs into overtime: complete once, at the crossing.
        let mut s = Session::start_running(timer_countdown_settings(60), t0);
        assert!(completed(&s.tick(at(59))).is_empty());
        assert_eq!(completed(&s.tick(at(60))), [60]);
        assert!(completed(&s.tick(at(61))).is_empty(), "not on the ticks after");
        assert_eq!(s.toggle_action(), ToggleAction::NoOp, "the play key does nothing in overtime");
        assert!(completed(&s.finish_overtime()).is_empty(), "Save in overtime says nothing");
        // A Box Breath countdown reaches its end.
        let mut s = Session::start_running(box_breath_settings(Some(16)), t0);
        let _ = s.tick(t0 + Duration::from_millis(500));
        assert_eq!(completed(&s.tick(at(16))), [16]);
        // Auto-Finish.
        let mut s = Session::start_running(SessionSettings { auto_finish: true, ..timer_countdown_settings(60) }, t0);
        assert_eq!(completed(&s.tick(at(60))), [60]);
        // A guided track ending early reaches its end too.
        let (mut s, _) = Session::start(guided_settings(600, false), t0);
        let _ = s.tick(at(590));
        assert_eq!(completed(&s.enter_overtime(Duration::ZERO)), [600]);
        // Hand ends: Save while running, Add in overtime.
        let mut s = Session::start_running(timer_countdown_settings(60), t0);
        assert!(completed(&s.stop(at(30))).is_empty());
        let mut s = Session::start_running(timer_countdown_settings(60), t0);
        let _ = s.tick(at(60));
        assert!(completed(&s.add_overtime_and_finish(at(90))).is_empty());
        // A Box Breath stopwatch never ends by itself.
        let mut s = Session::start_running(box_breath_settings(None), t0);
        for secs in 1..=40 {
            assert!(completed(&s.tick(at(secs))).is_empty());
        }
    }

    #[test]
    fn a_stop_during_preparation_records_no_time() {
        let t0 = Duration::from_secs(100);
        let mut s = Session::start_prep(timer_prep_settings(30, 600), t0);
        let _ = s.stop(t0 + Duration::from_secs(20));
        assert_eq!(s.final_duration_secs(), Some(0));
        // Paused during preparation, then stopped: the same.
        let mut s = Session::start_prep(timer_prep_settings(30, 600), t0);
        let _ = s.pause(t0 + Duration::from_secs(10));
        let _ = s.stop(t0 + Duration::from_secs(25));
        assert_eq!(s.final_duration_secs(), Some(0));
        // The crash-recovery heartbeat during preparation records 0 too.
        let s = Session::start_prep(timer_prep_settings(300, 600), t0);
        assert_eq!(s.saved_secs(t0 + Duration::from_secs(150)), 0);
        let mut s = Session::start_running(timer_countdown_settings(600), t0);
        assert_eq!(s.saved_secs(t0 + Duration::from_secs(150)), 150);
        let _ = s.stop(t0 + Duration::from_secs(200));
        assert_eq!(s.saved_secs(t0 + Duration::from_secs(400)), 200, "once ended, the length it saves");
    }

    #[test]
    fn a_session_stopped_while_paused_is_ended() {
        let t0 = Duration::from_secs(100);
        let mut s = Session::start_running(timer_countdown_settings(600), t0);
        let _ = s.pause(t0 + Duration::from_secs(120));
        let _ = s.stop(t0 + Duration::from_secs(200));
        assert_eq!(s.ui_state(), UiState::Done);
        assert_eq!(s.end_buttons(), EndButtons::ENDED);
        assert_eq!(s.toggle_action(), ToggleAction::NoOp, "the play key does nothing");
        assert!(s.media_status(t0 + Duration::from_secs(200)).is_none(), "no Resume in the media control");
        assert_eq!(s.final_duration_secs(), Some(120));
        assert!(!s.is_paused(), "ended, not paused");
        assert!(s.pause(t0 + Duration::from_secs(250)).is_empty(), "nothing to pause");
        s.resume(t0 + Duration::from_secs(300));
        assert_eq!(s.elapsed(t0 + Duration::from_secs(400)), Duration::from_secs(120), "an ended session's clock stays put");
    }

    #[test]
    fn a_guided_track_ending_early_rings_no_bell_after_the_end_bell() {
        let t0 = Duration::from_secs(100);
        let mut settings = guided_settings(900, false);
        settings.bells = vec![fixed_bell(899, "at-899"), fixed_bell(900, "at-900"), fixed_bell(960, "at-960")];
        let (mut s, _) = Session::start(settings, t0);
        let _ = s.tick(t0 + Duration::from_millis(898_900));
        // The track ends at 899.4 s, before the tick reaches 900; the 899 s
        // bell's tick never came.
        let end = s.enter_overtime(Duration::ZERO);
        assert!(end.iter().any(|e| matches!(e, Effect::Fire { channel: FireChannel::End, .. })), "{end:?}");
        let rung = |effects: Vec<Effect>| effects.iter().filter(|e| matches!(e, Effect::Fire { channel: FireChannel::Interval, phase: None, .. })).count();
        assert_eq!(rung(s.tick(t0 + Duration::from_secs(901))), 0, "nothing due by 900 s rings after the end bell");
        assert_eq!(rung(s.tick(t0 + Duration::from_secs(960))), 1, "a bell due in overtime still rings");
        // A suspend carries the session past its length and the track's
        // end arrives before the tick: bells due by then are skipped too.
        let mut settings = guided_settings(900, false);
        settings.bells = vec![fixed_bell(930, "at-930")];
        let (mut s, _) = Session::start(settings, t0);
        let _ = s.tick(t0 + Duration::from_secs(899));
        let _ = s.enter_overtime(t0 + Duration::from_secs(950));
        assert_eq!(rung(s.tick(t0 + Duration::from_secs(951))), 0, "the 930 s bell was due before the end was seen");
        // A track that ends (well early) while paused: the end waits for
        // the first tick after Resume, not for the planned length.
        let (mut s, _) = Session::start(guided_settings(900, false), t0);
        let _ = s.pause(t0 + Duration::from_secs(600));
        assert!(s.enter_overtime(Duration::ZERO).is_empty());
        assert_eq!(s.ui_state(), UiState::Paused);
        s.resume(t0 + Duration::from_secs(700));
        let end = s.tick(t0 + Duration::from_secs(701));
        assert!(end.iter().any(|e| matches!(e, Effect::Fire { channel: FireChannel::End, .. })), "{end:?}");
        assert_eq!(s.ui_state(), UiState::Overtime);
        // Add right after an early end: the ring still says the end was
        // reached, and the end bell is counted as heard.
        let mut settings = guided_settings(900, false);
        settings.bells = vec![fixed_bell(300, "mid")];
        settings.starting_bell = Some(cue_at(70.0));
        let (mut s, _) = Session::start(settings, t0);
        let _ = s.tick(t0 + Duration::from_secs(301));
        let _ = s.enter_overtime(Duration::ZERO);
        let _ = s.add_overtime_and_finish(t0 + Duration::from_millis(899_600));
        let ring = s.bell_ring(t0 + Duration::from_secs(950));
        assert_eq!(ring.ring.unwrap().filled, 1.0);
        assert_eq!(ring.counters, [BellCounter { rung: 3, total: None }]);
    }

    #[test]
    fn an_ended_session_shows_its_length() {
        let t0 = Duration::from_secs(100);
        let at = |secs| t0 + Duration::from_secs(secs);
        // By itself (Auto-Finish).
        let mut s = Session::start_running(SessionSettings { auto_finish: true, ..timer_countdown_settings(2700) }, t0);
        let _ = s.tick(at(2700));
        assert_eq!(s.display_secs(at(2705)), 2700);
        // Save in overtime: the planned length; Add: with the overtime.
        let mut s = Session::start_running(timer_countdown_settings(2700), t0);
        let _ = s.tick(at(2700));
        let _ = s.finish_overtime();
        assert_eq!(s.display_secs(at(2800)), 2700);
        let mut s = Session::start_running(timer_countdown_settings(2700), t0);
        let _ = s.tick(at(2700));
        let _ = s.add_overtime_and_finish(at(2880));
        assert_eq!(s.display_secs(at(2900)), 2880);
        // Save after 7 minutes.
        let mut s = Session::start_running(timer_countdown_settings(2700), t0);
        let _ = s.stop(at(420));
        assert_eq!(s.display_secs(at(500)), 420);
        // The ring shows the same: 7 of 45 minutes, not a full circle.
        let ring = s.bell_ring(at(900)).ring.unwrap();
        assert!((ring.filled - 420.0 / 2700.0).abs() < 1e-9, "{}", ring.filled);
    }
}
