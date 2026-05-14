//! `SessionSettings` — every input needed to construct an in-flight
//! `Session`. Built by the shell from its setup-view state and
//! consumed by `Session::start_prep` / `Session::start_running`.

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
/// from its setup-view state and handed to `Session::start_prep` or
/// `Session::start_running`.
#[derive(Debug, Clone)]
pub struct SessionSettings {
    /// Per-mode shape — replaces the prior loose tuple of
    /// `mode + target_secs + breath_pattern + stopwatch_display`.
    /// See [`SessionShape`] for the per-variant payload contract.
    pub shape: SessionShape,
    /// Some(secs) when prep silence is enabled; None otherwise.
    /// Only consulted by `start_prep` — `start_running` skips prep
    /// entirely.
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
