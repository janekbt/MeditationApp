//! Effects emitted by `Session::tick` and lifecycle calls.
//!
//! An `Effect` is something an app must do that it can't read from
//! the session afterwards: ring a bell, cut the signals, stop the
//! track, say "complete", take the end. Time, buttons and the
//! overtime are read from the session after every call. Both apps
//! consume the same enum.

use crate::bells::BellCue;
use crate::db::BoxBreathPhaseId;

/// Side effect the app should dispatch now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Ring a bell or a Box Breath phase cue on `channel`. `cue`
    /// carries the effective signal mode (the mode's signal setting
    /// already applied), so the apps play what it includes.
    Fire { channel: FireChannel, phase: Option<BoxBreathPhaseId>, cue: BellCue },
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

/// Which "fire channel" a `Fire` effect belongs to — the abstract
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

impl FireChannel {
    /// The diag tag a fired cue is logged under.
    pub fn log_tag(self, phase: Option<BoxBreathPhaseId>) -> &'static str {
        match (self, phase) {
            (_, Some(_)) => "fire_box_breath_phase_cue",
            (FireChannel::Starting, None) => "fire_starting_bell",
            (FireChannel::End, None) => "fire_end_bell",
            (FireChannel::Interval, None) => "fire_interval_bell",
        }
    }
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
}
