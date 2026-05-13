//! Stable bundled UUIDs + the portable seed lists that depend on
//! them. Lives in core so every shell that opens the same DB seeds
//! exactly the same set of canonical rows — divergent UUIDs would
//! mean a synced peer and a fresh peer end up with parallel
//! "Singing Bowl" rows after the first round-trip.
//!
//! Bell-sound seeds keep their resource paths in the GTK shell
//! (each row is bundled as a GResource at `/io/github/janekbt/Meditate/sounds/...`,
//! which is gtk-only); a future Android shell will pair these UUIDs
//! with its own asset paths. Vibration patterns and labels are pure
//! data and travel here in their entirety.

use crate::db::{ChartKind, SessionMode};
use crate::preset_config::{
    PresetBoxBreathCues, PresetConfig, PresetEndBell, PresetIntervalBells,
    PresetLabel, PresetStartingBell, PresetTiming,
};

// ── Bell-sound UUIDs ───────────────────────────────────────────────
// Public so any bundled-row default that needs to spell out a stable
// UUID at compile time (preset configs, SQL schema column DEFAULTs,
// test fixtures across the workspace) can reference the canonical
// constant rather than copy the literal.

pub const BUNDLED_BOWL_UUID: &str = "f0c2e8a1-3a72-4d4f-9c8b-1b0e5d8c0001";
pub const BUNDLED_BELL_UUID: &str = "f0c2e8a1-3a72-4d4f-9c8b-1b0e5d8c0002";
pub const BUNDLED_GONG_UUID: &str = "f0c2e8a1-3a72-4d4f-9c8b-1b0e5d8c0003";

// ── Default-label UUIDs ───────────────────────────────────────────
// Stable UUIDs for the seeded default labels. The seed runs once on
// first open (gated by `LABELS_SEEDED_KEY`) and never again — a
// renamed default still resolves through the UUID, and a *deleted*
// default stays deleted instead of resurrecting from the next open.

pub const DEFAULT_TIMER_LABEL_UUID: &str = "e2d5a4b8-7c91-4e3f-a826-d40f1c5b9001";
pub const DEFAULT_BREATHING_LABEL_UUID: &str = "e2d5a4b8-7c91-4e3f-a826-d40f1c5b9002";
pub const DEFAULT_GUIDED_LABEL_UUID: &str = "e2d5a4b8-7c91-4e3f-a826-d40f1c5b9003";

/// Seed list mirrors `BUNDLED_BELL_SOUNDS` — uuid + display name.
/// Append-only on UUID; the user can rename or delete the row from
/// the chooser like any other label.
pub const DEFAULT_LABELS: &[(&str, &str)] = &[
    (DEFAULT_TIMER_LABEL_UUID, "Meditation"),
    (DEFAULT_BREATHING_LABEL_UUID, "Box-Breathing"),
    (DEFAULT_GUIDED_LABEL_UUID, "Guided Meditation"),
];

// ── One-shot seed-flag setting keys ───────────────────────────────
// Stored in the `settings` table. Set to "1" after the first
// successful seed; subsequent `open()` calls early-return from the
// seed function. Without these, a deleted seed row would resurrect
// on the next open (and re-emit an `*_insert` event that overrides
// the user's delete on every synced peer).

pub const LABELS_SEEDED_KEY: &str = "default_labels_seeded";
pub const BELLS_SEEDED_KEY: &str = "bundled_bell_sounds_seeded";
pub const PRESETS_SEEDED_KEY: &str = "default_presets_seeded";
pub const VIBRATION_PATTERNS_SEEDED_KEY: &str = "bundled_vibration_patterns_seeded";

// ── Bundled vibration patterns ────────────────────────────────────
// Stable hardcoded UUIDs in their own family (separate from the
// bell-sounds family for visual disambiguation in DB inspection) so
// that peers seeded independently end up with the same row identity
// per pattern and don't accumulate duplicates after sync.

pub const BUNDLED_PATTERN_PULSE_UUID: &str = "7e9c4d2f-5a8b-4f1d-9e3c-2d6f7a8b0001";
pub const BUNDLED_PATTERN_HEARTBEAT_UUID: &str = "7e9c4d2f-5a8b-4f1d-9e3c-2d6f7a8b0002";
pub const BUNDLED_PATTERN_WAVE_UUID: &str = "7e9c4d2f-5a8b-4f1d-9e3c-2d6f7a8b0003";
pub const BUNDLED_PATTERN_RIPPLE_UUID: &str = "7e9c4d2f-5a8b-4f1d-9e3c-2d6f7a8b0004";
pub const BUNDLED_PATTERN_PYRAMID_UUID: &str = "7e9c4d2f-5a8b-4f1d-9e3c-2d6f7a8b0005";

/// Seed list: (uuid, name, duration_ms, intensities, chart_kind).
/// Pulse/Heartbeat/Wave/Ripple are line patterns; Pyramid ships in
/// bar mode to demo the abrupt-step variant out of the box.
pub const BUNDLED_VIBRATION_PATTERNS: &[(&str, &str, u32, &[f32], ChartKind)] = &[
    (
        BUNDLED_PATTERN_PULSE_UUID,
        "Pulse",
        400,
        &[0.0, 1.0, 0.0],
        ChartKind::Line,
    ),
    (
        BUNDLED_PATTERN_HEARTBEAT_UUID,
        "Heartbeat",
        1500,
        &[0.0, 0.6, 0.0, 0.0, 1.0, 0.0],
        ChartKind::Line,
    ),
    (
        BUNDLED_PATTERN_WAVE_UUID,
        "Wave",
        2000,
        &[0.0, 0.4, 0.7, 1.0, 0.7, 0.4, 0.0],
        ChartKind::Line,
    ),
    (
        BUNDLED_PATTERN_RIPPLE_UUID,
        "Ripple",
        2500,
        &[1.0, 0.7, 0.5, 0.3, 0.15, 0.0],
        ChartKind::Line,
    ),
    (
        BUNDLED_PATTERN_PYRAMID_UUID,
        "Pyramid",
        3000,
        &[0.2, 0.5, 1.0, 0.5, 0.2],
        ChartKind::Bar,
    ),
];

// ── Default-preset UUIDs ──────────────────────────────────────────
// Stable UUIDs for the three seeded default presets. Bundled rows
// have no special property at the schema level — they're regular
// presets that the user can rename, restar, or delete just like
// their own. The UUIDs let the one-shot seed know "we already did
// this" without scanning by name.

pub const DEFAULT_SITTING_PRESET_UUID: &str = "b9e1c5a4-2d3f-4d8b-9c70-7a0e1d2c3001";
pub const DEFAULT_BOX_BREATH_4444_UUID: &str = "b9e1c5a4-2d3f-4d8b-9c70-7a0e1d2c3002";
pub const DEFAULT_BOX_BREATH_4780_UUID: &str = "b9e1c5a4-2d3f-4d8b-9c70-7a0e1d2c3003";

/// Seed list: (uuid, name, mode, config). The `PresetConfig` values
/// hold owned `String`s so this is a fn, not a `const`. Called once
/// per fresh DB by `seed_default_presets`; the gated one-shot keeps
/// re-opens from doing the work again.
///
/// Sitting is a 15-minute Timer with starting + end bells on the
/// default sound, prep-time off. The two Box-Breath presets share
/// everything except their phase timing (4-4-4-4 vs 4-7-8-0). All
/// three ship starred so the home-screen chip list isn't empty on
/// first launch.
pub fn default_presets() -> [(&'static str, &'static str, SessionMode, PresetConfig); 3] {
    let default_starting_bell_off = || PresetStartingBell {
        enabled: false,
        sound_uuid: BUNDLED_BOWL_UUID.to_string(),
        prep_time_enabled: false,
        prep_time_secs: 5,
        signal_mode: "sound".to_string(),
        vibration_pattern_uuid: BUNDLED_PATTERN_PULSE_UUID.to_string(),
    };
    let sitting = PresetConfig {
        label: PresetLabel {
            enabled: true,
            uuid: Some(crate::db::LabelUuid::new(DEFAULT_TIMER_LABEL_UUID)),
        },
        starting_bell: PresetStartingBell { enabled: true, ..default_starting_bell_off() },
        interval_bells: PresetIntervalBells::default(),
        end_bell: PresetEndBell {
            enabled: true,
            sound_uuid: BUNDLED_BELL_UUID.to_string(),
            signal_mode: "sound".to_string(),
            vibration_pattern_uuid: BUNDLED_PATTERN_PULSE_UUID.to_string(),
        },
        timing: PresetTiming::Timer { stopwatch: false, duration_secs: 15 * 60 },
        cues_signal_mode: "both".to_string(),
        keep_screen_awake: false,
        box_breath_cues: PresetBoxBreathCues::default(),
    };
    let box_4444 = PresetConfig {
        label: PresetLabel {
            enabled: true,
            uuid: Some(crate::db::LabelUuid::new(DEFAULT_BREATHING_LABEL_UUID)),
        },
        starting_bell: default_starting_bell_off(),
        interval_bells: PresetIntervalBells::default(),
        end_bell: PresetEndBell {
            enabled: true,
            sound_uuid: BUNDLED_BELL_UUID.to_string(),
            signal_mode: "sound".to_string(),
            vibration_pattern_uuid: BUNDLED_PATTERN_PULSE_UUID.to_string(),
        },
        timing: PresetTiming::BoxBreath {
            stopwatch: false,
            inhale_secs: 4,
            hold_full_secs: 4,
            exhale_secs: 4,
            hold_empty_secs: 4,
            duration_secs: 10 * 60,
        },
        cues_signal_mode: "both".to_string(),
        keep_screen_awake: false,
        box_breath_cues: PresetBoxBreathCues::default(),
    };
    let box_4780 = PresetConfig {
        timing: PresetTiming::BoxBreath {
            stopwatch: false,
            inhale_secs: 4,
            hold_full_secs: 7,
            exhale_secs: 8,
            hold_empty_secs: 0,
            duration_secs: 10 * 60,
        },
        ..box_4444.clone()
    };
    [
        (DEFAULT_SITTING_PRESET_UUID, "Sitting", SessionMode::Timer, sitting),
        (DEFAULT_BOX_BREATH_4444_UUID, "Box Breath 4-4-4-4", SessionMode::BoxBreath, box_4444),
        (DEFAULT_BOX_BREATH_4780_UUID, "Box Breath 4-7-8-0", SessionMode::BoxBreath, box_4780),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every UUID has to be unique across all bundled rows that share a
    /// table — bell sounds, vibration patterns, labels, presets each
    /// have their own table, and rows in one table never reference
    /// rows in another by UUID. The constants here are the source of
    /// truth, so we pin both per-table uniqueness and the canonical
    /// shape of each UUID family.
    #[test]
    fn bell_sound_uuids_are_distinct() {
        let uuids = [BUNDLED_BOWL_UUID, BUNDLED_BELL_UUID, BUNDLED_GONG_UUID];
        let mut sorted = uuids;
        sorted.sort_unstable();
        sorted.windows(2).for_each(|w| {
            assert_ne!(w[0], w[1], "duplicate bell-sound UUID: {}", w[0]);
        });
    }

    #[test]
    fn vibration_pattern_uuids_are_distinct() {
        let uuids = [
            BUNDLED_PATTERN_PULSE_UUID,
            BUNDLED_PATTERN_HEARTBEAT_UUID,
            BUNDLED_PATTERN_WAVE_UUID,
            BUNDLED_PATTERN_RIPPLE_UUID,
            BUNDLED_PATTERN_PYRAMID_UUID,
        ];
        let mut sorted = uuids;
        sorted.sort_unstable();
        sorted.windows(2).for_each(|w| {
            assert_ne!(w[0], w[1], "duplicate vibration-pattern UUID: {}", w[0]);
        });
    }

    #[test]
    fn default_label_uuids_are_distinct() {
        let uuids = [
            DEFAULT_TIMER_LABEL_UUID,
            DEFAULT_BREATHING_LABEL_UUID,
            DEFAULT_GUIDED_LABEL_UUID,
        ];
        let mut sorted = uuids;
        sorted.sort_unstable();
        sorted.windows(2).for_each(|w| {
            assert_ne!(w[0], w[1], "duplicate default-label UUID: {}", w[0]);
        });
    }

    #[test]
    fn default_preset_uuids_are_distinct() {
        let uuids = [
            DEFAULT_SITTING_PRESET_UUID,
            DEFAULT_BOX_BREATH_4444_UUID,
            DEFAULT_BOX_BREATH_4780_UUID,
        ];
        let mut sorted = uuids;
        sorted.sort_unstable();
        sorted.windows(2).for_each(|w| {
            assert_ne!(w[0], w[1], "duplicate preset UUID: {}", w[0]);
        });
    }

    /// `DEFAULT_LABELS` must reference exactly the three label UUID
    /// constants, in the documented order. Drift between the constants
    /// and the seed list would mean a renamed-but-still-bundled label
    /// resolves through a UUID that doesn't match what the seeder
    /// inserted.
    #[test]
    fn default_labels_array_aligns_with_uuid_constants() {
        assert_eq!(DEFAULT_LABELS.len(), 3);
        assert_eq!(DEFAULT_LABELS[0].0, DEFAULT_TIMER_LABEL_UUID);
        assert_eq!(DEFAULT_LABELS[1].0, DEFAULT_BREATHING_LABEL_UUID);
        assert_eq!(DEFAULT_LABELS[2].0, DEFAULT_GUIDED_LABEL_UUID);
    }

    /// `BUNDLED_VIBRATION_PATTERNS` rows reference the
    /// `BUNDLED_PATTERN_*_UUID` constants in the documented order.
    /// Same reasoning as `default_labels_array_aligns_with_uuid_constants`.
    #[test]
    fn bundled_vibration_patterns_array_aligns_with_uuid_constants() {
        assert_eq!(BUNDLED_VIBRATION_PATTERNS.len(), 5);
        assert_eq!(BUNDLED_VIBRATION_PATTERNS[0].0, BUNDLED_PATTERN_PULSE_UUID);
        assert_eq!(BUNDLED_VIBRATION_PATTERNS[1].0, BUNDLED_PATTERN_HEARTBEAT_UUID);
        assert_eq!(BUNDLED_VIBRATION_PATTERNS[2].0, BUNDLED_PATTERN_WAVE_UUID);
        assert_eq!(BUNDLED_VIBRATION_PATTERNS[3].0, BUNDLED_PATTERN_RIPPLE_UUID);
        assert_eq!(BUNDLED_VIBRATION_PATTERNS[4].0, BUNDLED_PATTERN_PYRAMID_UUID);
    }

    /// Each vibration-pattern intensities array is non-empty and
    /// duration is positive — the encoder in `meditate_core::vibration`
    /// returns an empty envelope on either condition, which would mean
    /// a "play this pattern" call silently does nothing.
    #[test]
    fn bundled_vibration_patterns_have_non_empty_envelopes() {
        for &(uuid, name, duration_ms, intensities, _) in BUNDLED_VIBRATION_PATTERNS {
            assert!(
                !intensities.is_empty(),
                "{name} ({uuid}) has empty intensities"
            );
            assert!(duration_ms > 0, "{name} ({uuid}) has zero duration");
        }
    }
}
