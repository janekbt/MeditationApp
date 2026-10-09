//! `SessionSettings` — every input needed to construct an in-flight
//! `Session`. Built by the shell from its setup-view state and
//! consumed by `Session::start`.

use crate::bells::{ActiveBell, BellCue, BoxBreathCueConfig};
use crate::breath::BreathPattern;
use crate::db::{SessionMode, SignalMode};

/// Type-encoded shape of a session — the per-mode variant the
/// timer/box-breath/guided distinction collapses into. Each variant
/// carries only the fields that variant actually uses, so the
/// type system reflects what Box-Breath countdown vs Box-Breath
/// stopwatch vs Guided actually need at runtime. Replaces the four
/// loosely-correlated fields (`mode + target_secs + breath_pattern +
/// stopwatch_display`) on the old `SessionSettings`, eliminating
/// three `.expect("…")` panics in the tick loop.
///
/// Display rules baked into the variants:
///
/// - `TimerStopwatch` / `BoxBreathStopwatch` count up by definition.
/// - `BoxBreathCountdown` shows elapsed (count-up) regardless of
///   target; the cycle-aligned end still fires off `target_secs`.
/// - `TimerCountdown` shows ceiling-rounded remaining.
/// - `Guided` carries an explicit `count_up_display` flag — the file
///   always has a probed duration, but the user can flip the running
///   readout between count-up and count-down independently.
#[derive(Debug, Clone)]
pub enum SessionShape {
    TimerCountdown { target_secs: u32 },
    TimerStopwatch,
    BoxBreathCountdown { pattern: BreathPattern, target_secs: u32 },
    BoxBreathStopwatch { pattern: BreathPattern },
    Guided { duration_secs: u32, count_up_display: bool },
}

impl SessionShape {
    /// The legacy `SessionMode` enum tag, derived from the variant.
    /// Kept around for the small handful of code paths (notifications,
    /// stats categorisation) that key off the user-visible mode label
    /// rather than the per-variant payload.
    pub fn mode(&self) -> SessionMode {
        match self {
            Self::TimerCountdown { .. } | Self::TimerStopwatch => SessionMode::Timer,
            Self::BoxBreathCountdown { .. } | Self::BoxBreathStopwatch { .. } => {
                SessionMode::BoxBreath
            }
            Self::Guided { .. } => SessionMode::Guided,
        }
    }

    /// Whether the mode's stopwatch toggle is on: the stopwatch shapes,
    /// and Guided counting up. Interval and end bells key off it.
    pub fn stopwatch_on(&self) -> bool {
        match self {
            Self::TimerStopwatch | Self::BoxBreathStopwatch { .. } => true,
            Self::Guided { count_up_display, .. } => *count_up_display,
            Self::TimerCountdown { .. } | Self::BoxBreathCountdown { .. } => false,
        }
    }

    /// Target session length in seconds when the shape has one;
    /// `None` for stopwatch sessions. Drives the Running→Overtime
    /// transition and Box-Breath's cycle-aligned end.
    pub fn target_secs(&self) -> Option<u32> {
        match self {
            Self::TimerCountdown { target_secs }
            | Self::BoxBreathCountdown { target_secs, .. } => Some(*target_secs),
            Self::Guided { duration_secs, .. } => Some(*duration_secs),
            Self::TimerStopwatch | Self::BoxBreathStopwatch { .. } => None,
        }
    }
}

/// All the configuration a fresh session needs. Built by the shell
/// from its setup-view state and handed to `Session::start`.
#[derive(Debug, Clone)]
pub struct SessionSettings {
    /// Per-mode shape — replaces the prior loose tuple of
    /// `mode + target_secs + breath_pattern + stopwatch_display`.
    /// See [`SessionShape`] for the per-variant payload contract.
    pub shape: SessionShape,
    /// Some(secs) when prep silence is enabled; None otherwise.
    /// `Session::start` opens in Prep when set, in Running when not.
    pub prep_secs: Option<u32>,
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
    pub starting_bell: Option<BellCue>,
    /// End-bell cue, if the user enabled it. Fired at the natural
    /// end of the session — Running→Overtime for Timer/Guided
    /// countdown, EndBoxBreath for Box-Breath cycle-aligned target.
    /// Stopwatch-only sessions never reach those boundaries so the
    /// end bell stays silent without an explicit `None` check.
    pub end_bell: Option<BellCue>,
    /// Box-Breath per-phase cue config. Only `Some` for BoxBreath
    /// sessions; ignored otherwise.
    pub box_breath_cues: Option<BoxBreathCueConfig>,
}

impl SessionSettings {
    /// Everything a session of `shape` needs from the stored settings:
    /// preparation (Timer only, and only with the starting bell), the
    /// interval-bell schedule, signal mode, starting and end bell, and
    /// Box Breath's phase cues. Both shells start every session with it.
    pub fn from_db(db: &crate::db::Database, shape: SessionShape) -> Self {
        use crate::bells;
        let mode = shape.mode();
        let display = bells::DisplayMode::from_stopwatch_flag(shape.stopwatch_on());
        let prep_secs = if mode == SessionMode::Timer {
            crate::format::prep_plan_from_db(db).map(|d| d.as_secs() as u32)
        } else {
            None
        };
        let (bells, bell_rng_seed) =
            bells::session_bells_from_db(db, shape.target_secs().map(u64::from), display, mode);
        Self {
            prep_secs,
            bells,
            bell_rng_seed,
            signal_mode_override: bells::signal_mode_override_from_db(db, mode),
            starting_bell: bells::starting_bell_cue_from_db(db, mode),
            end_bell: bells::end_bell_cue_from_db(db, display, mode),
            box_breath_cues: (mode == SessionMode::BoxBreath).then(|| bells::box_breath_cues_from_db(db)),
            shape,
        }
    }
}

impl Default for SessionSettings {
    /// A no-frills Timer session: 10-minute countdown, no prep, no
    /// bells, no cues, signal-mode wide open. Useful as a starting
    /// point in doctests and for shells that want to mutate one or
    /// two fields without spelling out the other ten.
    fn default() -> Self {
        Self {
            shape: SessionShape::TimerCountdown { target_secs: 600 },
            prep_secs: None,
            bells: Vec::new(),
            bell_rng_seed: 1,
            signal_mode_override: SignalMode::Both,
            starting_bell: None,
            end_bell: None,
            box_breath_cues: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bells::{self, DisplayMode};
    use crate::db::{Database, IntervalBellKind};
    use crate::seeds::{BUNDLED_BOWL_UUID, BUNDLED_PATTERN_PULSE_UUID};

    /// The composition each shell built by hand before `from_db`.
    fn by_hand(db: &Database, shape: SessionShape, stopwatch_on: bool) -> SessionSettings {
        let mode = shape.mode();
        let display = DisplayMode::from_stopwatch_flag(stopwatch_on);
        let prep_secs = matches!(mode, SessionMode::Timer)
            .then(|| crate::format::prep_plan_from_db(db).map(|d| d.as_secs() as u32))
            .flatten();
        let (bells, bell_rng_seed) =
            bells::session_bells_from_db(db, shape.target_secs().map(u64::from), display, mode);
        SessionSettings {
            prep_secs,
            bells,
            bell_rng_seed,
            signal_mode_override: bells::signal_mode_override_from_db(db, mode),
            starting_bell: bells::starting_bell_cue_from_db(db, mode),
            end_bell: bells::end_bell_cue_from_db(db, display, mode),
            box_breath_cues: matches!(mode, SessionMode::BoxBreath).then(|| bells::box_breath_cues_from_db(db)),
            shape,
        }
    }

    fn same(a: &SessionSettings, b: &SessionSettings) -> bool {
        let strip = |s: &SessionSettings| format!("{:?}", SessionSettings { bell_rng_seed: 0, ..s.clone() });
        strip(a) == strip(b)
    }

    fn configured_db() -> Database {
        let db = Database::open_in_memory().unwrap();
        for (k, v) in [
            ("preparation_time_active", "true"),
            ("starting_bell_active", "true"),
            ("interval_bells_active", "true"),
            ("boxbreath_cues_active", "true"),
        ] {
            db.set_setting(k, v).unwrap();
        }
        db.insert_interval_bell(
            IntervalBellKind::Interval, 2, 0, BUNDLED_BOWL_UUID, BUNDLED_PATTERN_PULSE_UUID, SignalMode::Sound,
        )
        .unwrap();
        db
    }

    #[test]
    fn from_db_builds_what_the_shells_built() {
        let db = configured_db();
        let pattern = BreathPattern::default();
        for (shape, stopwatch_on) in [
            (SessionShape::TimerCountdown { target_secs: 600 }, false),
            (SessionShape::TimerStopwatch, true),
            (SessionShape::BoxBreathCountdown { pattern, target_secs: 320 }, false),
            (SessionShape::BoxBreathStopwatch { pattern }, true),
            (SessionShape::Guided { duration_secs: 900, count_up_display: false }, false),
            (SessionShape::Guided { duration_secs: 900, count_up_display: true }, true),
        ] {
            let got = SessionSettings::from_db(&db, shape.clone());
            assert!(same(&got, &by_hand(&db, shape.clone(), stopwatch_on)), "{shape:?}");
        }
    }

    #[test]
    fn from_db_gates_prep_starting_bell_and_cues_by_mode() {
        let db = configured_db();
        let timer = SessionSettings::from_db(&db, SessionShape::TimerCountdown { target_secs: 600 });
        assert!(timer.prep_secs.is_some() && timer.starting_bell.is_some());
        assert!(!timer.bells.is_empty() && timer.box_breath_cues.is_none());
        let breath = SessionSettings::from_db(
            &db,
            SessionShape::BoxBreathCountdown { pattern: BreathPattern::default(), target_secs: 320 },
        );
        assert!(breath.prep_secs.is_none(), "prep is Timer-only");
        assert!(breath.box_breath_cues.is_some());
        let guided = SessionSettings::from_db(&db, SessionShape::Guided { duration_secs: 900, count_up_display: false });
        assert!(guided.starting_bell.is_none(), "the file is the start");
    }
}
