//! `SessionSettings` — every input needed to construct an in-flight
//! `Session`. Built by the shell from its setup-view state and
//! consumed by `Session::start_prep` / `Session::start_running`.

use crate::bells::{ActiveBell, BellCue, BoxBreathCueConfig};
use crate::breath::BreathPattern;
use crate::db::{SessionMode, SignalMode};

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
    /// set). `None` for stopwatch-only Timer / Box-Breath where no
    /// natural end exists. Drives the Running → Overtime transition
    /// and Box-Breath's cycle-aligned end. **Independent of
    /// `stopwatch_display`** — Guided keeps target=Some even when
    /// the user toggles count-up display.
    pub target_secs: Option<u32>,
    /// `true` → the running readout counts up (floor-rounded
    /// elapsed); `false` → counts down (ceiling-rounded remaining,
    /// when `target_secs` is set) or up (no target). Captures the
    /// gtk-shell `stopwatch_toggle_on` user setting at session
    /// start; same dimension for Guided (where the toggle changes
    /// display but not the underlying target).
    pub stopwatch_display: bool,
    /// `Some` only in Box Breath mode. The pattern drives phase
    /// boundary detection (for cue firing) and cycle-aligned session
    /// completion (Box Breath always ends on a full-cycle boundary
    /// rather than mid-phase, so the user finishes on a natural
    /// hold-out).
    pub breath_pattern: Option<BreathPattern>,
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
