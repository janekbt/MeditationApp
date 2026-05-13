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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
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

impl Default for PresetTiming {
    /// Timer + countdown, 10 minutes. Mirrors the bundled "Sitting"
    /// preset's default mode/duration.
    fn default() -> Self {
        Self::Timer { stopwatch: false, duration_secs: 600 }
    }
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
    /// Serialize the preset payload to the JSON blob stored verbatim
    /// in `presets.config_json`. The infallible `expect` is honest:
    /// every field shape is round-trippable, so a failure here would
    /// indicate a serde derive bug, not a runtime condition.
    ///
    /// # Example
    ///
    /// ```
    /// use meditate_core::preset_config::{PresetConfig, PresetTiming};
    /// let cfg = PresetConfig {
    ///     timing: PresetTiming::Timer { stopwatch: false, duration_secs: 600 },
    ///     ..Default::default()
    /// };
    /// let blob = cfg.to_json();
    /// let round_tripped = PresetConfig::from_json(&blob).unwrap();
    /// assert_eq!(cfg, round_tripped);
    /// ```
    pub fn to_json(&self) -> String {
        serde_json::to_string(self)
            .expect("PresetConfig serializes to JSON")
    }

    /// Parse a preset payload from its stored JSON blob. Returns the
    /// underlying serde error verbatim so the caller can distinguish
    /// "this row is corrupt" from "this column was empty".
    pub fn from_json(s: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(s)
    }
}

// ── snapshot / apply walkers ────────────────────────────────────────
//
// `snapshot` reads the current setup state out of `Database` (settings
// rows, interval-bell library, box-breath phase rows) and combines it
// with the live UI state (`mode`, `timing`) the shell already knows to
// produce a `PresetConfig`. `apply` is the inverse: validates sound +
// pattern UUIDs are locally present, then writes settings + library +
// phase rows; returns the cfg's `timing` so the shell can reflect it
// into widget state.
//
// Apply is NOT atomic — each underlying `db.set_setting` /
// `db.insert_interval_bell` opens its own per-method transaction. A
// future infrastructure pass on core::db (item: nestable transactions)
// would let apply wrap everything in one outer transaction; until then
// behaviour matches the GTK shell's prior non-transactional walker.

use crate::db::{
    BoxBreathPhaseId, Database, IntervalBellKind, SessionMode, SignalMode,
};
use crate::format::PREP_SECS_DEFAULT;
use crate::seeds::{BUNDLED_BOWL_UUID, BUNDLED_PATTERN_PULSE_UUID};
use crate::settings_keys::{
    keep_screen_awake_key_for_mode, label_active_key_for_mode, label_uuid_key_for_mode,
    signal_mode_key_for_mode, stopwatch_key_for_mode,
};

/// Whether a preset created from the "Save current setup" flow
/// starts pinned to the home-view starred chip list. The user can
/// always destar from Manage afterwards; this is the default-on
/// affordance so a fresh preset is immediately reachable.
/// Centralized here so every shell agrees on the policy.
pub fn default_starred_on_save() -> bool {
    true
}

/// Whether `mode` exposes the Setup-view presets affordance. Guided
/// drives off the picked-file metadata directly, so a preset (which
/// stores duration / labels / bells, not a file uuid) doesn't apply
/// — the gtk shell early-returns from every preset-related code
/// path in Guided. Pinned here so the Android shell takes the same
/// branch.
pub fn mode_supports_presets(mode: SessionMode) -> bool {
    !matches!(mode, SessionMode::Guided)
}

/// Star-button visual state for a preset row in the chooser / home
/// chip list. Variants pick the (icon name, css class, tooltip
/// translatable key) triple the gtk shell currently encodes as three
/// parallel `if is_starred { … } else { … }` blocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StarVisualState {
    /// Preset is pinned. Filled accent star, "Unpin from home"
    /// tooltip.
    Starred,
    /// Preset is unstarred. Outline dim star, "Pin to home"
    /// tooltip.
    Unstarred,
}

impl StarVisualState {
    /// Map the persisted `is_starred` boolean to its rendering variant.
    /// Trivial wrapper, but funnels every shell through one decision
    /// point so the star/unstar visual policy stays consistent.
    pub fn from_is_starred(is_starred: bool) -> Self {
        if is_starred {
            StarVisualState::Starred
        } else {
            StarVisualState::Unstarred
        }
    }
}

/// Per-mode visibility table for the Setup view's content rows.
/// The gtk shell currently runs the same eight-row truth table in
/// two places (`on_mode_switched` and `show_idle_ui`); collecting
/// the decision into one struct removes the parallel-edit hazard
/// and lets Android dispatch off the same shape.
///
/// `true` means "show the row in this mode"; `false` means "hide".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModeSetupVisibility {
    pub countdown: bool,
    pub boxbreath: bool,
    pub guided: bool,
    pub boxbreath_phase: bool,
    pub starting_bell: bool,
    pub interval_bells: bool,
    pub duration: bool,
    pub presets: bool,
}

pub fn setup_visibility(mode: SessionMode) -> ModeSetupVisibility {
    // Starting Bell + Interval Bells apply to Timer mode only —
    // Box Breath has its own start-cue model, Guided uses the
    // audio file's natural opening. Duration / presets hide in
    // Guided because the file's metadata supplies the duration and
    // Guided has its own starred-files library.
    let is_timer = matches!(mode, SessionMode::Timer);
    let is_breath = matches!(mode, SessionMode::BoxBreath);
    let is_guided = matches!(mode, SessionMode::Guided);
    ModeSetupVisibility {
        countdown: is_timer,
        boxbreath: is_breath,
        guided: is_guided,
        boxbreath_phase: is_breath,
        starting_bell: is_timer,
        interval_bells: is_timer,
        duration: !is_guided,
        presets: mode_supports_presets(mode),
    }
}

#[derive(Debug)]
pub enum ApplyError {
    /// One or more referenced sound or vibration-pattern UUIDs are not
    /// in the local DB yet — typically a preset synced from another
    /// device whose audio / vibration rows haven't replicated in. The
    /// shell can show a "still syncing" toast naming the missing rows.
    SyncPending {
        missing_sounds: Vec<String>,
        missing_patterns: Vec<String>,
    },
    /// Underlying SQLite or DB-layer error during one of the writes.
    DbError(String),
}

/// Snapshot the current setup state into a `PresetConfig`. The shell
/// builds `timing` from its widget / Cell state (no Database has the
/// in-flight Box-Breath pattern or countdown target — those are gtk-
/// side reactive plumbing) and supplies `mode` from its current view.
pub fn snapshot(db: &Database, mode: SessionMode, timing: PresetTiming) -> PresetConfig {
    // Forward to the canonical helpers in `settings_keys` (one
    // implementation of the get-or-default recipe) while keeping
    // the local `read_*(key, default)` call shape so this function's
    // body doesn't drown in `db,` arguments.
    let read_bool = |k: &str, default: bool| crate::settings_keys::read_bool(db, k, default);
    let read_str = |k: &str, default: &str| crate::settings_keys::read_str(db, k, default);
    let read_u32 = |k: &str, default: u32| crate::settings_keys::read_u32(db, k, default);

    let label = PresetLabel {
        enabled: read_bool(label_active_key_for_mode(mode), false),
        uuid: db
            .get_setting(label_uuid_key_for_mode(mode), "")
            .ok()
            .filter(|s| !s.is_empty()),
    };

    let starting_bell = PresetStartingBell {
        enabled: read_bool("starting_bell_active", false),
        sound_uuid: read_str("starting_bell_sound", BUNDLED_BOWL_UUID),
        prep_time_enabled: read_bool("preparation_time_active", false),
        prep_time_secs: read_u32("preparation_time_secs", PREP_SECS_DEFAULT),
        signal_mode: read_str("starting_bell_signal_mode", "sound"),
        vibration_pattern_uuid: read_str("starting_bell_pattern", BUNDLED_PATTERN_PULSE_UUID),
    };

    let end_bell = PresetEndBell {
        enabled: read_bool("end_bell_active", true),
        sound_uuid: read_str("end_bell_sound", BUNDLED_BOWL_UUID),
        signal_mode: read_str("end_bell_signal_mode", "sound"),
        vibration_pattern_uuid: read_str("end_bell_pattern", BUNDLED_PATTERN_PULSE_UUID),
    };

    let intervals_enabled = read_bool("interval_bells_active", false);
    let bells: Vec<PresetIntervalBell> = db
        .list_interval_bells()
        .unwrap_or_default()
        .into_iter()
        .map(|b| PresetIntervalBell {
            kind: b.kind.as_db_str().to_string(),
            minutes: b.minutes,
            jitter_pct: b.jitter_pct,
            sound_uuid: b.sound_uuid,
            enabled: b.enabled,
            signal_mode: b.signal_mode.as_db_str().to_string(),
            vibration_pattern_uuid: b.vibration_pattern_uuid,
        })
        .collect();
    let interval_bells = PresetIntervalBells {
        enabled: intervals_enabled,
        bells,
    };

    let phases: Vec<PresetBoxBreathPhase> = [
        BoxBreathPhaseId::In,
        BoxBreathPhaseId::HoldIn,
        BoxBreathPhaseId::Out,
        BoxBreathPhaseId::HoldOut,
    ]
    .iter()
    .filter_map(|&id| db.get_box_breath_phase(id).ok().flatten())
    .map(|p| PresetBoxBreathPhase {
        phase: p.phase.as_db_str().to_string(),
        enabled: p.enabled,
        signal_mode: p.signal_mode.as_db_str().to_string(),
        sound_uuid: p.sound_uuid,
        pattern_uuid: p.pattern_uuid,
    })
    .collect();
    let box_breath_cues = PresetBoxBreathCues {
        master_enabled: read_bool("boxbreath_cues_active", false),
        phases,
    };

    let cues_signal_mode = read_str(signal_mode_key_for_mode(mode), "both");
    let keep_screen_awake = read_bool(keep_screen_awake_key_for_mode(mode), false);

    PresetConfig {
        label,
        starting_bell,
        interval_bells,
        end_bell,
        timing,
        cues_signal_mode,
        keep_screen_awake,
        box_breath_cues,
    }
}

/// Apply a `PresetConfig` to the DB. Validates referenced sound +
/// pattern UUIDs are present (rejects with `SyncPending` if not),
/// writes per-mode + bell-related settings, replays the
/// interval-bell library, and writes box-breath phase rows when
/// `cfg.timing` is `BoxBreath` (skips them when `Timer` so a Timer
/// preset's apply doesn't wipe the user's box-breath authoring).
///
/// `mode` is the *active session mode* the shell is currently
/// showing — used to pick the per-mode setting keys. `cfg.timing` is
/// the captured timing in the preset (which may not match `mode` if
/// e.g. a Timer-mode user applies a Box-Breath preset).
///
/// Returns `Ok(timing)` on success — the shell uses it to update its
/// widget / Cell state.
pub fn apply(
    db: &Database,
    cfg: &PresetConfig,
    mode: SessionMode,
) -> Result<PresetTiming, ApplyError> {
    // 1. Validate referenced UUIDs are locally present.
    let known_sounds: std::collections::HashSet<String> = db
        .list_bell_sounds()
        .map_err(|e| ApplyError::DbError(format!("{e:?}")))?
        .into_iter()
        .map(|s| s.uuid)
        .collect();
    let mut needs_sound: Vec<&str> = Vec::new();
    if cfg.starting_bell.enabled {
        needs_sound.push(&cfg.starting_bell.sound_uuid);
    }
    if cfg.end_bell.enabled {
        needs_sound.push(&cfg.end_bell.sound_uuid);
    }
    for b in &cfg.interval_bells.bells {
        needs_sound.push(&b.sound_uuid);
    }
    let mut missing_sounds: Vec<String> = Vec::new();
    for u in &needs_sound {
        if !known_sounds.contains(*u) && !missing_sounds.iter().any(|m| m == *u) {
            missing_sounds.push(u.to_string());
        }
    }

    let known_patterns: std::collections::HashSet<String> = db
        .list_vibration_patterns()
        .map_err(|e| ApplyError::DbError(format!("{e:?}")))?
        .into_iter()
        .map(|p| p.uuid)
        .collect();
    let mut needs_pattern: Vec<&str> = Vec::new();
    if !cfg.starting_bell.vibration_pattern_uuid.is_empty() {
        needs_pattern.push(&cfg.starting_bell.vibration_pattern_uuid);
    }
    if !cfg.end_bell.vibration_pattern_uuid.is_empty() {
        needs_pattern.push(&cfg.end_bell.vibration_pattern_uuid);
    }
    for b in &cfg.interval_bells.bells {
        if !b.vibration_pattern_uuid.is_empty() {
            needs_pattern.push(&b.vibration_pattern_uuid);
        }
    }
    for p in &cfg.box_breath_cues.phases {
        if !p.pattern_uuid.is_empty() {
            needs_pattern.push(&p.pattern_uuid);
        }
    }
    let mut missing_patterns: Vec<String> = Vec::new();
    for u in &needs_pattern {
        if !known_patterns.contains(*u) && !missing_patterns.iter().any(|m| m == *u) {
            missing_patterns.push(u.to_string());
        }
    }

    if !missing_sounds.is_empty() || !missing_patterns.is_empty() {
        return Err(ApplyError::SyncPending {
            missing_sounds,
            missing_patterns,
        });
    }

    // 2. Per-mode + bell-related settings.
    let stopwatch_active = match cfg.timing {
        PresetTiming::Timer { stopwatch, .. } => stopwatch,
        PresetTiming::BoxBreath { stopwatch, .. } => stopwatch,
    };

    let set = |k: &str, v: &str| -> Result<(), ApplyError> {
        db.set_setting(k, v)
            .map_err(|e| ApplyError::DbError(format!("{e:?}")))
    };
    let bool_str = crate::settings_keys::format_bool;

    set(label_active_key_for_mode(mode), bool_str(cfg.label.enabled))?;
    if let Some(luuid) = cfg.label.uuid.as_ref() {
        set(label_uuid_key_for_mode(mode), luuid)?;
    }
    set("starting_bell_active", bool_str(cfg.starting_bell.enabled))?;
    if !cfg.starting_bell.sound_uuid.is_empty() {
        set("starting_bell_sound", &cfg.starting_bell.sound_uuid)?;
    }
    set(
        "preparation_time_active",
        bool_str(cfg.starting_bell.prep_time_enabled),
    )?;
    set(
        "preparation_time_secs",
        &cfg.starting_bell.prep_time_secs.to_string(),
    )?;
    set(
        "interval_bells_active",
        bool_str(cfg.interval_bells.enabled),
    )?;
    set("end_bell_active", bool_str(cfg.end_bell.enabled))?;
    if !cfg.end_bell.sound_uuid.is_empty() {
        set("end_bell_sound", &cfg.end_bell.sound_uuid)?;
    }
    set(stopwatch_key_for_mode(mode), bool_str(stopwatch_active))?;
    set("starting_bell_signal_mode", &cfg.starting_bell.signal_mode)?;
    set(
        "starting_bell_pattern",
        &cfg.starting_bell.vibration_pattern_uuid,
    )?;
    set("end_bell_signal_mode", &cfg.end_bell.signal_mode)?;
    set("end_bell_pattern", &cfg.end_bell.vibration_pattern_uuid)?;
    set(signal_mode_key_for_mode(mode), &cfg.cues_signal_mode)?;
    set(
        keep_screen_awake_key_for_mode(mode),
        bool_str(cfg.keep_screen_awake),
    )?;

    // 3. Box-breath phase rows — only when cfg is BoxBreath. Timer
    // presets carry the seed values; stamping them on apply would
    // wipe the user's box-breath authoring.
    let preset_is_box_breath = matches!(cfg.timing, PresetTiming::BoxBreath { .. });
    if preset_is_box_breath {
        set(
            "boxbreath_cues_active",
            bool_str(cfg.box_breath_cues.master_enabled),
        )?;
        for p in &cfg.box_breath_cues.phases {
            let Some(phase_id) = BoxBreathPhaseId::from_db_str(&p.phase) else {
                continue;
            };
            let Some(signal_mode) = SignalMode::from_db_str(&p.signal_mode) else {
                continue;
            };
            db.set_box_breath_phase(
                phase_id,
                p.enabled,
                signal_mode,
                &p.sound_uuid,
                &p.pattern_uuid,
            )
            .map_err(|e| ApplyError::DbError(format!("{e:?}")))?;
        }
    }

    // 4. Replay interval-bell library: delete-all + re-insert from cfg.
    let existing = db
        .list_interval_bells()
        .map_err(|e| ApplyError::DbError(format!("{e:?}")))?;
    for b in &existing {
        db.delete_interval_bell(&b.uuid)
            .map_err(|e| ApplyError::DbError(format!("{e:?}")))?;
    }
    for s in &cfg.interval_bells.bells {
        let kind = match s.kind.as_str() {
            "interval" => IntervalBellKind::Interval,
            "fixed_from_start" => IntervalBellKind::FixedFromStart,
            "fixed_from_end" => IntervalBellKind::FixedFromEnd,
            _ => continue,
        };
        let Some(signal_mode) = SignalMode::from_db_str(&s.signal_mode) else {
            continue;
        };
        let rowid = db
            .insert_interval_bell(
                kind,
                s.minutes,
                s.jitter_pct,
                &s.sound_uuid,
                &s.vibration_pattern_uuid,
                signal_mode,
            )
            .map_err(|e| ApplyError::DbError(format!("{e:?}")))?;
        if !s.enabled {
            if let Some(b) = db
                .list_interval_bells()
                .ok()
                .and_then(|bs| bs.into_iter().find(|b| b.id == rowid))
            {
                db.set_interval_bell_enabled(&b.uuid, false)
                    .map_err(|e| ApplyError::DbError(format!("{e:?}")))?;
            }
        }
    }

    Ok(cfg.timing.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_macros::assert_matches;

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

    // ── snapshot / apply ─────────────────────────────────────────────

    use crate::db::{BellSoundCategory, ChartKind};

    /// Test fixture: an in-memory DB seeded with box-breath phase rows
    /// and one known sound + one known vibration pattern (so apply
    /// doesn't reject as sync-pending). Returns (db, sound_uuid,
    /// pattern_uuid) so tests can build configs that reference them.
    fn fresh_db() -> (Database, String, String) {
        let db = Database::open_in_memory().unwrap();
        db.seed_box_breath_phases().unwrap();
        let sound_uuid = "test-sound-uuid".to_string();
        db.insert_bell_sound_with_uuid(
            &sound_uuid,
            "Test Bell",
            "/test/bell.ogg",
            false,
            "audio/ogg",
            BellSoundCategory::General,
        )
        .unwrap();
        let pattern_uuid = "test-pattern-uuid".to_string();
        db.insert_vibration_pattern_with_uuid(
            &pattern_uuid,
            "Test Pattern",
            400,
            &[0.0, 1.0, 0.0],
            ChartKind::Line,
            false,
        )
        .unwrap();
        (db, sound_uuid, pattern_uuid)
    }

    fn cfg_with_known_uuids(
        timing: PresetTiming,
        sound_uuid: &str,
        pattern_uuid: &str,
    ) -> PresetConfig {
        PresetConfig {
            label: PresetLabel { enabled: true, uuid: Some("label-uuid".to_string()) },
            starting_bell: PresetStartingBell {
                enabled: true,
                sound_uuid: sound_uuid.to_string(),
                prep_time_enabled: true,
                prep_time_secs: 30,
                signal_mode: "both".to_string(),
                vibration_pattern_uuid: pattern_uuid.to_string(),
            },
            interval_bells: PresetIntervalBells {
                enabled: true,
                bells: vec![PresetIntervalBell {
                    kind: "interval".to_string(),
                    minutes: 5,
                    jitter_pct: 10,
                    sound_uuid: sound_uuid.to_string(),
                    enabled: true,
                    signal_mode: "sound".to_string(),
                    vibration_pattern_uuid: String::new(),
                }],
            },
            end_bell: PresetEndBell {
                enabled: true,
                sound_uuid: sound_uuid.to_string(),
                signal_mode: "vibration".to_string(),
                vibration_pattern_uuid: pattern_uuid.to_string(),
            },
            timing,
            cues_signal_mode: "both".to_string(),
            keep_screen_awake: true,
            box_breath_cues: PresetBoxBreathCues::default(),
        }
    }

    #[test]
    fn apply_writes_per_mode_settings_keyed_by_session_mode() {
        let (db, sound, pattern) = fresh_db();
        let cfg = cfg_with_known_uuids(
            PresetTiming::Timer { stopwatch: true, duration_secs: 600 },
            &sound,
            &pattern,
        );

        apply(&db, &cfg, SessionMode::Timer).expect("apply succeeds");

        // Per-mode keys land at the Timer-keyed setting names.
        assert_eq!(
            db.get_setting(stopwatch_key_for_mode(SessionMode::Timer), "false").unwrap(),
            "true"
        );
        assert_eq!(
            db.get_setting(label_active_key_for_mode(SessionMode::Timer), "false").unwrap(),
            "true"
        );
        assert_eq!(
            db.get_setting(signal_mode_key_for_mode(SessionMode::Timer), "").unwrap(),
            "both"
        );
        assert_eq!(
            db.get_setting(keep_screen_awake_key_for_mode(SessionMode::Timer), "false").unwrap(),
            "true"
        );

        // ...and don't bleed into the Box-Breath or Guided keys
        // (defaults still active there).
        assert_eq!(
            db.get_setting(stopwatch_key_for_mode(SessionMode::BoxBreath), "false").unwrap(),
            "false"
        );
        assert_eq!(
            db.get_setting(stopwatch_key_for_mode(SessionMode::Guided), "false").unwrap(),
            "false"
        );
    }

    #[test]
    fn apply_rejects_sync_pending_sound() {
        let (db, _sound, pattern) = fresh_db();
        let mut cfg = cfg_with_known_uuids(
            PresetTiming::Timer { stopwatch: false, duration_secs: 600 },
            "missing-sound-uuid",
            &pattern,
        );
        // Belt + braces: also point the interval bell at the missing
        // sound so the test exercises the fan-in over multiple slots.
        cfg.interval_bells.bells[0].sound_uuid = "missing-sound-uuid".to_string();

        let err = apply(&db, &cfg, SessionMode::Timer).unwrap_err();
        assert_matches!(
            err,
            ApplyError::SyncPending { missing_sounds, missing_patterns } => {
                assert_eq!(missing_sounds, vec!["missing-sound-uuid".to_string()]);
                assert!(missing_patterns.is_empty());
            }
        );
    }

    #[test]
    fn apply_rejects_sync_pending_pattern() {
        let (db, sound, _pattern) = fresh_db();
        let mut cfg = cfg_with_known_uuids(
            PresetTiming::Timer { stopwatch: false, duration_secs: 600 },
            &sound,
            "missing-pattern-uuid",
        );
        cfg.starting_bell.vibration_pattern_uuid = "missing-pattern-uuid".to_string();
        cfg.end_bell.vibration_pattern_uuid = "missing-pattern-uuid".to_string();

        let err = apply(&db, &cfg, SessionMode::Timer).unwrap_err();
        assert_matches!(
            err,
            ApplyError::SyncPending { missing_sounds, missing_patterns } => {
                assert!(missing_sounds.is_empty());
                assert_eq!(missing_patterns, vec!["missing-pattern-uuid".to_string()]);
            }
        );
    }

    #[test]
    fn apply_with_timer_preset_does_not_touch_box_breath_phases() {
        let (db, sound, pattern) = fresh_db();
        // Pre-set a known phase state so we can verify it survives the apply.
        db.set_box_breath_phase(
            BoxBreathPhaseId::In,
            true,
            SignalMode::Sound,
            &sound,
            &pattern,
        ).unwrap();

        let cfg = cfg_with_known_uuids(
            PresetTiming::Timer { stopwatch: false, duration_secs: 600 },
            &sound,
            &pattern,
        );
        apply(&db, &cfg, SessionMode::Timer).expect("apply succeeds");

        // The phase row must still hold our pre-applied state. Apply
        // wrote no box-breath phases because cfg.timing is Timer.
        let phase = db.get_box_breath_phase(BoxBreathPhaseId::In).unwrap().unwrap();
        assert!(phase.enabled);
        assert_eq!(phase.signal_mode, SignalMode::Sound);
        assert_eq!(phase.sound_uuid, sound);
    }

    #[test]
    fn apply_with_box_breath_preset_writes_phase_rows() {
        let (db, sound, pattern) = fresh_db();

        let cfg = PresetConfig {
            label: PresetLabel::default(),
            starting_bell: PresetStartingBell::default(),
            interval_bells: PresetIntervalBells::default(),
            end_bell: PresetEndBell::default(),
            timing: PresetTiming::BoxBreath {
                stopwatch: true,
                inhale_secs: 4,
                hold_full_secs: 4,
                exhale_secs: 4,
                hold_empty_secs: 4,
                duration_secs: 0,
            },
            cues_signal_mode: "sound".to_string(),
            keep_screen_awake: false,
            box_breath_cues: PresetBoxBreathCues {
                master_enabled: true,
                phases: vec![PresetBoxBreathPhase {
                    phase: "in".to_string(),
                    enabled: true,
                    signal_mode: "sound".to_string(),
                    sound_uuid: sound.clone(),
                    pattern_uuid: pattern.clone(),
                }],
            },
        };

        apply(&db, &cfg, SessionMode::BoxBreath).expect("apply succeeds");

        let phase = db.get_box_breath_phase(BoxBreathPhaseId::In).unwrap().unwrap();
        assert!(phase.enabled);
        assert_eq!(phase.sound_uuid, sound);
        assert_eq!(phase.pattern_uuid, pattern);
        assert_eq!(
            db.get_setting("boxbreath_cues_active", "false").unwrap(),
            "true"
        );
    }

    #[test]
    fn apply_replays_interval_bell_library_replacing_existing_rows() {
        let (db, sound, pattern) = fresh_db();
        // Pre-existing bell that should be wiped out by apply.
        db.insert_interval_bell(
            IntervalBellKind::Interval,
            10, 0, &sound, "", SignalMode::Sound,
        ).unwrap();
        assert_eq!(db.list_interval_bells().unwrap().len(), 1);

        let cfg = cfg_with_known_uuids(
            PresetTiming::Timer { stopwatch: false, duration_secs: 600 },
            &sound,
            &pattern,
        );
        // cfg has one interval bell at minutes=5, jitter=10 (set up
        // by cfg_with_known_uuids).
        apply(&db, &cfg, SessionMode::Timer).expect("apply succeeds");

        let bells = db.list_interval_bells().unwrap();
        assert_eq!(bells.len(), 1, "old bell should be deleted, only cfg's bell remains");
        assert_eq!(bells[0].minutes, 5);
        assert_eq!(bells[0].jitter_pct, 10);
    }

    #[test]
    fn snapshot_apply_round_trip_preserves_settings_and_bells() {
        let (db, sound, pattern) = fresh_db();
        let original = cfg_with_known_uuids(
            PresetTiming::Timer { stopwatch: false, duration_secs: 1200 },
            &sound,
            &pattern,
        );
        // Step 1: apply original to the DB.
        apply(&db, &original, SessionMode::Timer).expect("apply succeeds");

        // Step 2: snapshot back. Pass the same timing — the shell
        // would build it from its widget state at this moment.
        let round_tripped = snapshot(
            &db,
            SessionMode::Timer,
            original.timing.clone(),
        );

        // Settings + label should round-trip. Interval bells get fresh
        // row UUIDs on insert (delete-all + re-insert), so we compare
        // by structural fields rather than serializing.
        assert_eq!(round_tripped.label, original.label);
        assert_eq!(round_tripped.starting_bell, original.starting_bell);
        assert_eq!(round_tripped.end_bell, original.end_bell);
        assert_eq!(round_tripped.cues_signal_mode, original.cues_signal_mode);
        assert_eq!(round_tripped.keep_screen_awake, original.keep_screen_awake);
        assert_eq!(round_tripped.timing, original.timing);
        // interval-bell content (minus uuids that we don't capture
        // in PresetIntervalBell anyway).
        assert_eq!(
            round_tripped.interval_bells.enabled,
            original.interval_bells.enabled
        );
        assert_eq!(
            round_tripped.interval_bells.bells.len(),
            original.interval_bells.bells.len()
        );
        for (got, exp) in round_tripped
            .interval_bells
            .bells
            .iter()
            .zip(original.interval_bells.bells.iter())
        {
            assert_eq!(got.kind, exp.kind);
            assert_eq!(got.minutes, exp.minutes);
            assert_eq!(got.jitter_pct, exp.jitter_pct);
            assert_eq!(got.sound_uuid, exp.sound_uuid);
            assert_eq!(got.enabled, exp.enabled);
            assert_eq!(got.signal_mode, exp.signal_mode);
        }
    }
}
