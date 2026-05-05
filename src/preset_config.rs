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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PresetEndBell {
    pub enabled: bool,
    pub sound_uuid: String,
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
            },
            interval_bells: PresetIntervalBells {
                enabled: false,
                bells: vec![],
            },
            end_bell: PresetEndBell {
                enabled: true,
                sound_uuid: "end-uuid".to_string(),
            },
            timing: PresetTiming::Timer {
                stopwatch: false,
                duration_secs: 900,
            },
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
            },
            timing: PresetTiming::BoxBreath {
                inhale_secs: 4,
                hold_full_secs: 7,
                exhale_secs: 8,
                hold_empty_secs: 0,
                duration_secs: 600,
            },
        };
        let json = cfg.to_json();
        let back = PresetConfig::from_json(&json).unwrap();
        assert_eq!(cfg, back);
    }

    #[test]
    fn missing_optional_fields_default_on_deserialize() {
        // Forwards-compat: a JSON that only has `timing` should
        // deserialize, with the rest filled in by `Default`. This
        // guards against schema drift between binary versions.
        let json = r#"{"timing":{"mode":"timer","stopwatch":false,"duration_secs":900}}"#;
        let cfg = PresetConfig::from_json(json).unwrap();
        assert!(!cfg.label.enabled);
        assert!(!cfg.starting_bell.enabled);
        assert_eq!(cfg.starting_bell.sound_uuid, "");
        assert!(cfg.interval_bells.bells.is_empty());
        assert!(matches!(cfg.timing,
            PresetTiming::Timer { stopwatch: false, duration_secs: 900 }));
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
                    },
                    PresetIntervalBell {
                        kind: "fixed_from_start".to_string(),
                        minutes: 1,
                        jitter_pct: 0,
                        sound_uuid: "tick-uuid".to_string(),
                        enabled: false,
                    },
                ],
            },
            end_bell: PresetEndBell::default(),
            timing: PresetTiming::Timer { stopwatch: false, duration_secs: 600 },
        };
        let back = PresetConfig::from_json(&cfg.to_json()).unwrap();
        assert_eq!(cfg, back);
    }
}
