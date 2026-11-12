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
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionShape {
    TimerCountdown { target_secs: u32 },
    TimerStopwatch,
    BoxBreathCountdown { pattern: BreathPattern, target_secs: u32 },
    BoxBreathStopwatch { pattern: BreathPattern },
    Guided { duration_secs: u32, count_up_display: bool },
}

impl SessionShape {
    /// The shape a Start builds from the Setup page: `target_secs` is
    /// the set length (Guided: the file's length); `None` when there is
    /// nothing to count down.
    pub fn new(mode: SessionMode, stopwatch_on: bool, target_secs: u32, pattern: BreathPattern) -> Option<Self> {
        let shape = match (mode, stopwatch_on) {
            (SessionMode::Timer, true) => Self::TimerStopwatch,
            (SessionMode::Timer, false) => Self::TimerCountdown { target_secs },
            (SessionMode::BoxBreath, true) => Self::BoxBreathStopwatch { pattern },
            // Ends on a full cycle, never mid-breath.
            (SessionMode::BoxBreath, false) => Self::BoxBreathCountdown {
                pattern,
                target_secs: u32::try_from(pattern.cycle_aligned_target_secs(u64::from(target_secs))).unwrap_or(u32::MAX),
            },
            // The file has its length either way; the switch only flips the time.
            (SessionMode::Guided, count_up_display) => Self::Guided { duration_secs: target_secs, count_up_display },
        };
        (shape.target_secs() != Some(0)).then_some(shape)
    }

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
    /// tests pass a fixed value for determinism. `bells::xorshift64`
    /// guards a 0.
    pub bell_rng_seed: u64,
    /// Per-mode signal-mode override the user picked on the Setup
    /// view's "Cues" ToggleGroup. AND'd with each bell / phase-cue's
    /// own `signal_mode` at fire time to compute the effective
    /// channel mix. Defaults to `Both` (no extra cap).
    pub signal_mode_override: SignalMode,
    /// Starting-bell cue, if the user enabled it. Fired at the
    /// prep→Running boundary (or immediately when there's no prep).
    /// `None` means the starting bell is off.
    pub starting_bell: Option<BellCue>,
    /// End-bell cue, if the user enabled it. Fired when a countdown
    /// reaches its end (overtime, Auto-Finish, Box Breath's
    /// cycle-aligned end).
    /// Stopwatch-only sessions never reach those boundaries so the
    /// end bell stays silent without an explicit `None` check.
    pub end_bell: Option<BellCue>,
    /// Box-Breath per-phase cue config. Only `Some` for BoxBreath
    /// sessions; ignored otherwise.
    pub box_breath_cues: Option<BoxBreathCueConfig>,
    /// Auto-Finish: a countdown saves itself at its end instead of
    /// going into overtime (`Effect::Ended` with `save_now`).
    pub auto_finish: bool,
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
            auto_finish: crate::settings_keys::auto_finish_from_db(db, mode),
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
            auto_finish: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::bells::{self, DisplayMode};
    use crate::db::{Database, IntervalBellKind};
    use crate::seeds::{BUNDLED_BOWL_UUID, BUNDLED_PATTERN_PULSE_UUID};

    #[test]
    fn each_mode_builds_its_shape() {
        use SessionMode::{BoxBreath, Guided, Timer};
        let p = BreathPattern::clamp_from_raw(4, 4, 4, 4);
        let new = |mode, stopwatch_on, secs| SessionShape::new(mode, stopwatch_on, secs, p);
        assert_eq!(new(Timer, false, 600), Some(SessionShape::TimerCountdown { target_secs: 600 }));
        assert_eq!(new(Timer, true, 600), Some(SessionShape::TimerStopwatch));
        assert_eq!(new(BoxBreath, false, 60), Some(SessionShape::BoxBreathCountdown { pattern: p, target_secs: 64 }));
        assert_eq!(new(BoxBreath, true, 60), Some(SessionShape::BoxBreathStopwatch { pattern: p }));
        assert_eq!(new(Guided, false, 900), Some(SessionShape::Guided { duration_secs: 900, count_up_display: false }));
        assert_eq!(new(Guided, true, 900), Some(SessionShape::Guided { duration_secs: 900, count_up_display: true }));
        assert_eq!(new(Timer, false, 0), None);
        assert_eq!(new(Guided, false, 0), None);
        assert_eq!(new(Guided, true, 0), None);
    }

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
            auto_finish: crate::settings_keys::auto_finish_from_db(db, mode),
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
            IntervalBellKind::Interval, crate::bells::BellTime::new(2, crate::bells::TimeUnit::Min), crate::bells::BellTime::ZERO, crate::bells::BellTime::ZERO, BUNDLED_BOWL_UUID, BUNDLED_PATTERN_PULSE_UUID, SignalMode::Sound,
        )
        .unwrap();
        db
    }

    /// A bell the Timer's signal setting mutes is left out of the
    /// Bells count and of the session's ring and counter.
    #[test]
    fn a_muted_bell_is_left_out() {
        use crate::bells::{BellTime, TimeUnit};
        let db = Database::open_in_memory().unwrap();
        db.set_setting("interval_bells_active", "true").unwrap();
        db.set_setting(crate::settings_keys::signal_mode_key_for_mode(SessionMode::Timer), SignalMode::Vibration.as_db_str()).unwrap();
        for (min, signal) in [(5, SignalMode::Sound), (6, SignalMode::Vibration)] {
            db.insert_interval_bell(
                IntervalBellKind::FixedFromStart, BellTime::new(min, TimeUnit::Min), BellTime::ZERO, BellTime::ZERO, BUNDLED_BOWL_UUID, BUNDLED_PATTERN_PULSE_UUID, signal,
            )
            .unwrap();
        }
        assert_eq!(bells::interval_bells_count(&db), 1);
        let settings = SessionSettings::from_db(&db, SessionShape::TimerCountdown { target_secs: 600 });
        let (s, _) = crate::session::Session::start(settings, std::time::Duration::ZERO);
        let ring = s.bell_ring(std::time::Duration::ZERO);
        assert_eq!(ring.ring.map(|r| r.beads.len()), Some(1));
        assert_eq!(ring.counters, [crate::session::BellCounter { rung: 0, total: Some(1) }]);
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
