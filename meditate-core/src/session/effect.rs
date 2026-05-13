//! Effects emitted by `Session::tick` and lifecycle calls.
//!
//! `Effect` is the dispatch ISA between core and shell — every value
//! here represents something the shell does (update a label, fire a
//! bell, end the session). Both shells (gtk, future Android) consume
//! the same enum.

use crate::db::{BoxBreathPhaseId, SignalMode};
use std::time::Duration;

/// Side effect the shell should dispatch this tick. The shell
/// matches on each variant and routes to its native layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Update the running-page big time display. `secs` is the
    /// already-rounded display value (ceiling for prep/countdown,
    /// floor for stopwatch / box-breath elapsed). Shell formats via
    /// `format::format_time`.
    UpdateDisplay { secs: u64 },
    /// Prep silence elapsed; transition to Running. Shell:
    /// constructs the countdown core for Timer mode, plays the
    /// starting bell, refreshes the running button row.
    EndPrep,
    /// Box Breath: a phase boundary was just crossed AND a cue is
    /// configured for the new phase (master toggle on + per-phase
    /// row enabled). Carries the resolved sound + vibration + the
    /// effective signal_mode (per-phase AND per-mode override
    /// already applied). The first tick of a box-breath session
    /// seeds `last_phase` silently; only subsequent transitions
    /// emit.
    FireBoxBreathCue {
        phase: BoxBreathPhaseId,
        sound_uuid: String,
        vibration_pattern_uuid: String,
        signal_mode: SignalMode,
    },
    /// Starting bell — fired at the prep→Running boundary, or
    /// immediately at session start when there's no prep. Carries
    /// the effective signal_mode (per-bell AND per-mode override).
    FireStartingBell {
        sound_uuid: String,
        vibration_pattern_uuid: String,
        signal_mode: SignalMode,
    },
    /// End bell — fired at Running→Overtime (Timer/Guided countdown
    /// crosses zero) or at the cycle-aligned end of a Box-Breath
    /// session. Stopwatch-only sessions never reach those
    /// boundaries so this effect simply never emits for them.
    FireEndBell {
        sound_uuid: String,
        vibration_pattern_uuid: String,
        signal_mode: SignalMode,
    },
    /// Box Breath: the cycle-aligned target was reached, ending the
    /// session naturally. Shell drops the session and shows the
    /// done view with this duration. Never fired in stopwatch-only
    /// box-breath mode (no target; user must press Stop).
    EndBoxBreath { duration_secs: u64 },
    /// Interval / fixed bell crossed its ring boundary this tick.
    /// `signal_mode` is the *effective* mode (per-bell AND per-mode
    /// override already applied by Session); the shell dispatches
    /// sound + vibration based on that variant directly — no extra
    /// gating needed.
    FireBell {
        sound_uuid: String,
        vibration_pattern_uuid: String,
        signal_mode: SignalMode,
    },
    /// Timer/Guided countdown crossed zero. Shell: morphs Pause →
    /// Finish, hides Stop, reveals the Add button, freezes the
    /// hero at the planned duration, fires the end bell, and sends
    /// a system notification when the app isn't focused. The
    /// session continues ticking in Overtime — interval bells keep
    /// firing on the original session timeline; the Add button's
    /// label counts up via subsequent `UpdateOvertimeLabel`.
    EnterOvertime,
    /// Overtime tick: how much past the target the session has
    /// gone. Shell renders the Add button as
    /// `<localized prefix> <MM:SS> ?`.
    UpdateOvertimeLabel { overtime: Duration },
    /// Session terminated via `stop` / `finish_overtime` /
    /// `add_overtime_and_finish`. `duration_secs` is what the saved
    /// row should record — prep elapsed when stopped during prep,
    /// running elapsed mid-session, target_secs for Finish-overtime,
    /// running+overtime elapsed for Add-overtime. Shell: stops
    /// playback, releases screen-awake, transitions to the Done UI
    /// with this duration.
    EndSession { duration_secs: u64 },
    /// Any in-flight bell sound / vibration / phase cue should be
    /// cut immediately. Emitted by Session lifecycle calls that
    /// represent a user-driven boundary — pause (the user wants
    /// everything to stop), stop (session ends), finish_overtime /
    /// add_overtime_and_finish (user acknowledges the session is
    /// over). Mechanics are shell-specific (gtk: MediaFile +
    /// feedbackd DBus cancel; Android: equivalent native calls)
    /// but the *rule* — that these lifecycle moments cut the user's
    /// in-flight feedback — lives in core.
    StopActiveSignals,
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
}

impl Effect {
    /// Resolve a Fire effect to its routing info. `None` for every
    /// non-`Fire*` variant (UpdateDisplay, EndPrep, EndSession,
    /// EnterOvertime, UpdateOvertimeLabel, EndBoxBreath,
    /// StopActiveSignals — those are consumed at their tick
    /// callsites). The shell's dispatch loop becomes
    /// `for r in effects.iter().filter_map(Effect::fire_route) {
    /// play(r.channel, r.sound_uuid); ... }`.
    pub fn fire_route(&self) -> Option<FireRoute<'_>> {
        match self {
            Effect::FireBell { sound_uuid, vibration_pattern_uuid, signal_mode } => {
                Some(FireRoute {
                    channel: FireChannel::Interval,
                    log_tag: "fire_interval_bell",
                    sound_uuid,
                    vibration_pattern_uuid,
                    signal_mode: *signal_mode,
                })
            }
            Effect::FireStartingBell { sound_uuid, vibration_pattern_uuid, signal_mode } => {
                Some(FireRoute {
                    channel: FireChannel::Starting,
                    log_tag: "fire_starting_bell",
                    sound_uuid,
                    vibration_pattern_uuid,
                    signal_mode: *signal_mode,
                })
            }
            Effect::FireEndBell { sound_uuid, vibration_pattern_uuid, signal_mode } => {
                Some(FireRoute {
                    channel: FireChannel::End,
                    log_tag: "fire_end_bell",
                    sound_uuid,
                    vibration_pattern_uuid,
                    signal_mode: *signal_mode,
                })
            }
            Effect::FireBoxBreathCue {
                phase: _,
                sound_uuid,
                vibration_pattern_uuid,
                signal_mode,
            } => Some(FireRoute {
                channel: FireChannel::Interval,
                log_tag: "fire_box_breath_phase_cue",
                sound_uuid,
                vibration_pattern_uuid,
                signal_mode: *signal_mode,
            }),
            _ => None,
        }
    }
}
