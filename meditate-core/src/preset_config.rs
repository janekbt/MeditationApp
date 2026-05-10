//! Preset config schema — the self-contained snapshot of a Setup-view
//! state that a preset captures.
//!
//! `PresetConfig` is what gets serialised into the `config_json`
//! column of the `presets` table. Core treats that column as opaque;
//! this module owns the schema. Round-trips via serde_json.
//!
//! Schema evolution: every field uses `#[serde(default)]` so a config
//! serialised by an older binary still deserialises after a new field
//! is added. Removed fields are tolerated by serde's default behaviour
//! of ignoring unknown keys.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PresetConfig {
    #[serde(default)]
    pub label: PresetLabel,
    #[serde(default)]
    pub starting_bell: PresetStartingBell,
    #[serde(default)]
    pub interval_bells: PresetIntervalBells,
    #[serde(default)]
    pub end_bell: PresetEndBell,
    pub timing: PresetTiming,
    /// Per-mode "Cues" override — db-string form ("sound" /
    /// "vibration" / "both").
    pub cues_signal_mode: String,
    /// Per-mode "Keep screen awake" toggle.
    pub keep_screen_awake: bool,
    /// Box-Breath phase cues snapshot. Captured for every preset
    /// (Timer presets just hold their default-empty form); only
    /// the Box-Breath apply path actually writes them back.
    pub box_breath_cues: PresetBoxBreathCues,
}

/// Mode-specific timing. Variant must match the column-level `mode`
/// on the same `presets` row.
///
/// Both variants store the session duration in seconds (`duration_secs`)
/// even though the UIs currently only set minute-aligned values —
/// keeps the schema future-proof for sub-minute granularity later
/// without another migration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum PresetTiming {
    Timer { stopwatch: bool, duration_secs: u32 },
    BoxBreath {
        stopwatch: bool,
        inhale_secs: u32,
        hold_full_secs: u32,
        exhale_secs: u32,
        hold_empty_secs: u32,
        duration_secs: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PresetLabel {
    pub enabled: bool,
    /// `None` ⇒ apply mode default (Meditation in Timer, Box-Breathing
    /// in Box Breath). `Some(uuid)` ⇒ pinned to a specific label.
    pub uuid: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PresetStartingBell {
    pub enabled: bool,
    pub sound_uuid: String,
    pub prep_time_enabled: bool,
    pub prep_time_secs: u32,
    /// Per-bell sound/vibration/both mode (db-string form).
    pub signal_mode: String,
    /// Per-bell vibration pattern uuid.
    pub vibration_pattern_uuid: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PresetIntervalBells {
    pub enabled: bool,
    pub bells: Vec<PresetIntervalBell>,
}

/// Snapshot of one row from the `interval_bells` library. `kind` is the
/// db-string form ("interval", "fixed_from_start", "fixed_from_end")
/// to keep this file decoupled from `meditate_core::db::IntervalBellKind`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PresetIntervalBell {
    pub kind: String,
    pub minutes: u32,
    pub jitter_pct: u32,
    pub sound_uuid: String,
    pub enabled: bool,
    /// Per-bell sound/vibration/both mode (db-string form).
    pub signal_mode: String,
    /// Per-bell vibration pattern uuid.
    pub vibration_pattern_uuid: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PresetEndBell {
    pub enabled: bool,
    pub sound_uuid: String,
    /// Per-bell sound/vibration/both mode (db-string form).
    pub signal_mode: String,
    /// Per-bell vibration pattern uuid.
    pub vibration_pattern_uuid: String,
}

/// Box-Breath per-phase cue config — master enable + four phase
/// snapshots. The phase string matches the `box_breath_phases.phase`
/// column ("in", "holdin", "out", "holdout"). Empty `phases` is the
/// natural default for Timer presets that don't carry phase state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PresetBoxBreathCues {
    pub master_enabled: bool,
    pub phases: Vec<PresetBoxBreathPhase>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PresetBoxBreathPhase {
    pub phase: String,
    pub enabled: bool,
    pub signal_mode: String,
    pub sound_uuid: String,
    pub pattern_uuid: String,
}

impl PresetConfig {
    pub fn to_json(&self) -> String {
        serde_json::to_string(self)
            .expect("PresetConfig serializes to JSON")
    }

    pub fn from_json(s: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn timer_config() -> PresetConfig {
        PresetConfig {
            label: PresetLabel {
                enabled: true,
                uuid: Some("label-uuid".to_string()),
            },
            starting_bell: PresetStartingBell {
                enabled: true,
                sound_uuid: "bell-uuid".to_string(),
                prep_time_enabled: false,
                prep_time_secs: 5,
                signal_mode: "both".to_string(),
                vibration_pattern_uuid: "pulse-uuid".to_string(),
            },
            interval_bells: PresetIntervalBells {
                enabled: false,
                bells: vec![],
            },
            end_bell: PresetEndBell {
                enabled: true,
                sound_uuid: "end-uuid".to_string(),
                signal_mode: "vibration".to_string(),
                vibration_pattern_uuid: "heartbeat-uuid".to_string(),
            },
            timing: PresetTiming::Timer {
                stopwatch: false,
                duration_secs: 900,
            },
            cues_signal_mode: "both".to_string(),
            keep_screen_awake: true,
            box_breath_cues: PresetBoxBreathCues::default(),
        }
    }

    #[test]
    fn timer_config_round_trips_through_json() {
        let cfg = timer_config();
        let json = cfg.to_json();
        let back = PresetConfig::from_json(&json).unwrap();
        assert_eq!(cfg, back);
    }

    #[test]
    fn box_breath_config_round_trips_through_json() {
        let cfg = PresetConfig {
            label: PresetLabel { enabled: true, uuid: None },
            starting_bell: PresetStartingBell::default(),
            interval_bells: PresetIntervalBells::default(),
            end_bell: PresetEndBell {
                enabled: true,
                sound_uuid: "end-uuid".to_string(),
                signal_mode: "sound".to_string(),
                vibration_pattern_uuid: String::new(),
            },
            timing: PresetTiming::BoxBreath {
                stopwatch: false,
                inhale_secs: 4,
                hold_full_secs: 7,
                exhale_secs: 8,
                hold_empty_secs: 0,
                duration_secs: 600,
            },
            cues_signal_mode: "vibration".to_string(),
            keep_screen_awake: false,
            box_breath_cues: PresetBoxBreathCues {
                master_enabled: true,
                phases: vec![
                    PresetBoxBreathPhase {
                        phase: "in".to_string(),
                        enabled: true,
                        signal_mode: "vibration".to_string(),
                        sound_uuid: String::new(),
                        pattern_uuid: "pulse-uuid".to_string(),
                    },
                    PresetBoxBreathPhase {
                        phase: "holdin".to_string(),
                        enabled: false,
                        signal_mode: "sound".to_string(),
                        sound_uuid: "voice-uuid".to_string(),
                        pattern_uuid: "pulse-uuid".to_string(),
                    },
                    PresetBoxBreathPhase {
                        phase: "out".to_string(),
                        enabled: true,
                        signal_mode: "both".to_string(),
                        sound_uuid: "voice-uuid".to_string(),
                        pattern_uuid: "wave-uuid".to_string(),
                    },
                    PresetBoxBreathPhase {
                        phase: "holdout".to_string(),
                        enabled: false,
                        signal_mode: "sound".to_string(),
                        sound_uuid: String::new(),
                        pattern_uuid: String::new(),
                    },
                ],
            },
        };
        let json = cfg.to_json();
        let back = PresetConfig::from_json(&json).unwrap();
        assert_eq!(cfg, back);
    }


    #[test]
    fn interval_bell_snapshot_round_trips() {
        let cfg = PresetConfig {
            label: PresetLabel::default(),
            starting_bell: PresetStartingBell::default(),
            interval_bells: PresetIntervalBells {
                enabled: true,
                bells: vec![
                    PresetIntervalBell {
                        kind: "interval".to_string(),
                        minutes: 5,
                        jitter_pct: 10,
                        sound_uuid: "ping-uuid".to_string(),
                        enabled: true,
                        signal_mode: "vibration".to_string(),
                        vibration_pattern_uuid: "wave-uuid".to_string(),
                    },
                    PresetIntervalBell {
                        kind: "fixed_from_start".to_string(),
                        minutes: 1,
                        jitter_pct: 0,
                        sound_uuid: "tick-uuid".to_string(),
                        enabled: false,
                        signal_mode: "sound".to_string(),
                        vibration_pattern_uuid: String::new(),
                    },
                ],
            },
            end_bell: PresetEndBell::default(),
            timing: PresetTiming::Timer { stopwatch: false, duration_secs: 600 },
            cues_signal_mode: String::new(),
            keep_screen_awake: false,
            box_breath_cues: PresetBoxBreathCues::default(),
        };
        let back = PresetConfig::from_json(&cfg.to_json()).unwrap();
        assert_eq!(cfg, back);
    }
}
