//! Effects emitted by `Session::tick` and lifecycle calls.
//!
//! An `Effect` is something an app must do that it can't read from
//! the session afterwards: ring a bell, cut the signals, stop the
//! track, say "complete", take the end. Time, buttons and the
//! overtime are read from the session after every call. Both apps
//! consume the same enum.

use crate::bell_volume::BellVolume;
use crate::db::{BoxBreathPhaseId, SignalMode};

/// Side effect the app should dispatch now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Box Breath: a phase boundary was just crossed AND a cue is
    /// configured for the new phase (master toggle on + per-phase
    /// row enabled). Carries the resolved sound + vibration + the
    /// effective signal_mode (per-phase AND per-mode override
    /// already applied). `Session::start` emits it for the first
    /// inhale; after that, each phase change emits it.
    FireBoxBreathCue {
        phase: BoxBreathPhaseId,
        sound_uuid: String,
        vibration_pattern_uuid: String,
        signal_mode: SignalMode,
        volume: BellVolume,
    },
    /// Starting bell — fired at the prep→Running boundary, or
    /// immediately at session start when there's no prep. Carries
    /// the effective signal_mode (per-bell AND per-mode override).
    FireStartingBell {
        sound_uuid: String,
        vibration_pattern_uuid: String,
        signal_mode: SignalMode,
        volume: BellVolume,
    },
    /// End bell: the countdown reached its end (overtime, Auto-Finish,
    /// a Box Breath end). Stopwatch sessions never get there.
    FireEndBell {
        sound_uuid: String,
        vibration_pattern_uuid: String,
        signal_mode: SignalMode,
        volume: BellVolume,
    },
    /// Interval / fixed bell crossed its ring boundary this tick.
    /// `signal_mode` is the *effective* mode (per-bell AND per-mode
    /// override already applied by Session); the shell dispatches
    /// sound + vibration based on that variant directly — no extra
    /// gating needed.
    FireBell {
        sound_uuid: String,
        vibration_pattern_uuid: String,
        signal_mode: SignalMode,
        volume: BellVolume,
    },
    /// Guided: the planned length is reached, so the track stops now,
    /// ahead of the end bell. The timer can reach the probed length a
    /// moment before the audio ends (length rounded down at import,
    /// player start latency), which would otherwise leave the tail
    /// playing under the end bell.
    StopGuidedAudio,
    /// Cut any bell sound, vibration or phase cue in flight: a pause or
    /// an end by hand. The mechanics are each app's; the rule that these
    /// moments cut the signals is core's.
    StopActiveSignals,
    /// The session reached its planned end by itself (overtime, a Box
    /// Breath end, Auto-Finish): "Meditation complete" while the app
    /// is not in front. Never for an end by hand.
    Completed { planned_secs: u64 },
    /// The session ended with this length to save. `save_now`:
    /// Auto-Finish, saved at once; otherwise the page shows Save.
    Ended { duration_secs: u64, save_now: bool },
}

/// Which "fire channel" a `Fire*` effect belongs to — the abstract
/// slot the shell routes the sound through. The gtk shell maps to
/// its three thread-local `MediaFile` slots (Starting, End,
/// Interval); the Android shell maps to its `MediaPlayer` channels.
/// Box-Breath phase cues share the `Interval` channel because they
/// stack the same way interval bells do — two cues coinciding both
/// play through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FireChannel {
    Starting,
    End,
    Interval,
}

/// Resolved routing info for a `Fire*` effect: the channel slot, a
/// stable diag tag (logged via `meditate_core::log`), and the
/// three carried fields (sound uuid, vibration pattern uuid,
/// effective signal_mode). The shell dispatches sound + vibration
/// based on `signal_mode.includes_*` — no extra gating needed
/// because Session already AND'd the per-mode override.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FireRoute<'a> {
    pub channel: FireChannel,
    pub log_tag: &'static str,
    pub sound_uuid: &'a str,
    pub vibration_pattern_uuid: &'a str,
    pub signal_mode: SignalMode,
    pub volume: BellVolume,
}

impl Effect {
    /// Whether these effects end the session to be saved at once
    /// (Auto-Finish).
    pub fn saves_now(effects: &[Effect]) -> bool {
        effects.iter().any(|e| matches!(e, Effect::Ended { save_now: true, .. }))
    }

    /// The planned length, when these effects say "complete".
    pub fn completed_secs(effects: &[Effect]) -> Option<u64> {
        effects.iter().find_map(|e| match e {
            Effect::Completed { planned_secs } => Some(*planned_secs),
            _ => None,
        })
    }

    /// The length to save, when these effects end the session.
    pub fn ended_secs(effects: &[Effect]) -> Option<u64> {
        effects.iter().find_map(|e| match e {
            Effect::Ended { duration_secs, .. } => Some(*duration_secs),
            _ => None,
        })
    }

    /// Resolve a Fire effect to its routing info; `None` for every
    /// other variant.
    pub fn fire_route(&self) -> Option<FireRoute<'_>> {
        match self {
            Effect::FireBell { sound_uuid, vibration_pattern_uuid, signal_mode, volume } => {
                Some(FireRoute {
                    channel: FireChannel::Interval,
                    log_tag: "fire_interval_bell",
                    sound_uuid,
                    vibration_pattern_uuid,
                    signal_mode: *signal_mode,
                    volume: *volume,
                })
            }
            Effect::FireStartingBell { sound_uuid, vibration_pattern_uuid, signal_mode, volume } => {
                Some(FireRoute {
                    channel: FireChannel::Starting,
                    log_tag: "fire_starting_bell",
                    sound_uuid,
                    vibration_pattern_uuid,
                    signal_mode: *signal_mode,
                    volume: *volume,
                })
            }
            Effect::FireEndBell { sound_uuid, vibration_pattern_uuid, signal_mode, volume } => {
                Some(FireRoute {
                    channel: FireChannel::End,
                    log_tag: "fire_end_bell",
                    sound_uuid,
                    vibration_pattern_uuid,
                    signal_mode: *signal_mode,
                    volume: *volume,
                })
            }
            Effect::FireBoxBreathCue {
                phase: _,
                sound_uuid,
                vibration_pattern_uuid,
                signal_mode,
                volume,
            } => Some(FireRoute {
                channel: FireChannel::Interval,
                log_tag: "fire_box_breath_phase_cue",
                sound_uuid,
                vibration_pattern_uuid,
                signal_mode: *signal_mode,
                volume: *volume,
            }),
            _ => None,
        }
    }
}
