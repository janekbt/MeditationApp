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
use crate::bells::{legacy_jitter_pct, legacy_minutes, resolve_bell_times, BellTime, TimeUnit};

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
    /// Per-mode Auto-Finish. Missing in presets saved before it
    /// existed: those apply it as off.
    #[serde(default)]
    pub auto_finish: bool,
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
    pub uuid: Option<crate::db::LabelUuid>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PresetStartingBell {
    pub enabled: bool,
    pub sound_uuid: crate::db::BellSoundUuid,
    pub prep_time_enabled: bool,
    pub prep_time_secs: u32,
    /// Per-bell sound/vibration/both mode (db-string form).
    pub signal_mode: String,
    /// Per-bell vibration pattern uuid.
    pub vibration_pattern_uuid: crate::db::VibrationPatternUuid,
    /// Bell volume (`crate::bell_volume`). Presets saved before
    /// per-bell volumes carry none and load at the middle.
    #[serde(default)]
    pub volume: crate::bell_volume::BellVolume,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PresetIntervalBells {
    pub enabled: bool,
    pub bells: Vec<PresetIntervalBell>,
}

/// Snapshot of one row from the `interval_bells` library. `kind` is the
/// db-string form ("interval", "fixed_from_start", "fixed_from_end")
/// to keep this file decoupled from `meditate_core::db::IntervalBellKind`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(from = "PresetIntervalBellJson", into = "PresetIntervalBellJson")]
pub struct PresetIntervalBell {
    pub kind: String,
    pub time: BellTime,
    pub jitter: BellTime,
    pub step: BellTime,
    pub sound_uuid: crate::db::BellSoundUuid,
    pub enabled: bool,
    /// Per-bell sound/vibration/both mode (db-string form).
    pub signal_mode: String,
    /// Per-bell vibration pattern uuid.
    pub vibration_pattern_uuid: crate::db::VibrationPatternUuid,
    /// Bell volume (`crate::bell_volume`). Presets saved before
    /// per-bell volumes carry none and load at the middle.
    pub volume: crate::bell_volume::BellVolume,
    /// The library row this is. Presets saved before it carry none;
    /// `apply` then finds the row by its settings.
    pub uuid: crate::db::IntervalBellUuid,
}

/// `PresetIntervalBell` as stored: the seconds fields next to the
/// `minutes` and `jitter_pct` that versions before seconds read.
#[derive(Serialize, Deserialize)]
struct PresetIntervalBellJson {
    kind: String,
    minutes: u32,
    jitter_pct: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    secs: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    secs_unit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    jitter_secs: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    jitter_unit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    step_secs: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    step_unit: Option<String>,
    sound_uuid: crate::db::BellSoundUuid,
    enabled: bool,
    signal_mode: String,
    vibration_pattern_uuid: crate::db::VibrationPatternUuid,
    #[serde(default)]
    volume: crate::bell_volume::BellVolume,
    #[serde(default)]
    uuid: crate::db::IntervalBellUuid,
}

impl From<PresetIntervalBellJson> for PresetIntervalBell {
    fn from(j: PresetIntervalBellJson) -> Self {
        let time = |secs: Option<u32>, unit: Option<String>| {
            Some(BellTime { secs: secs?, unit: TimeUnit::from_db_str(&unit?)? })
        };
        let stored = match (
            time(j.secs, j.secs_unit),
            time(j.jitter_secs, j.jitter_unit),
            time(j.step_secs, j.step_unit),
        ) {
            (Some(t), Some(ji), Some(st)) => Some((t, ji, st)),
            _ => None,
        };
        let (time, jitter, step) = resolve_bell_times(j.minutes, j.jitter_pct, stored);
        Self {
            kind: j.kind,
            time,
            jitter,
            step,
            sound_uuid: j.sound_uuid,
            enabled: j.enabled,
            signal_mode: j.signal_mode,
            vibration_pattern_uuid: j.vibration_pattern_uuid,
            volume: j.volume,
            uuid: j.uuid,
        }
    }
}

impl From<PresetIntervalBell> for PresetIntervalBellJson {
    fn from(b: PresetIntervalBell) -> Self {
        Self {
            kind: b.kind,
            minutes: legacy_minutes(b.time),
            jitter_pct: legacy_jitter_pct(b.time, b.jitter),
            secs: Some(b.time.secs),
            secs_unit: Some(b.time.unit.as_db_str().to_string()),
            jitter_secs: Some(b.jitter.secs),
            jitter_unit: Some(b.jitter.unit.as_db_str().to_string()),
            step_secs: Some(b.step.secs),
            step_unit: Some(b.step.unit.as_db_str().to_string()),
            sound_uuid: b.sound_uuid,
            enabled: b.enabled,
            signal_mode: b.signal_mode,
            vibration_pattern_uuid: b.vibration_pattern_uuid,
            volume: b.volume,
            uuid: b.uuid,
        }
    }
}

impl From<IntervalBell> for PresetIntervalBell {
    fn from(b: IntervalBell) -> Self {
        Self {
            kind: b.kind.as_db_str().to_string(),
            time: b.time,
            jitter: b.jitter,
            step: b.step,
            sound_uuid: b.sound_uuid,
            enabled: b.enabled,
            signal_mode: b.signal_mode.as_db_str().to_string(),
            vibration_pattern_uuid: b.vibration_pattern_uuid,
            volume: b.volume,
            uuid: b.uuid,
        }
    }
}

impl PresetIntervalBell {
    /// Same kind, timing, sound, pattern and volume, on or off.
    fn same_settings(&self, other: &Self) -> bool {
        Self { enabled: other.enabled, uuid: other.uuid.clone(), ..self.clone() } == *other
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PresetEndBell {
    pub enabled: bool,
    pub sound_uuid: crate::db::BellSoundUuid,
    /// Per-bell sound/vibration/both mode (db-string form).
    pub signal_mode: String,
    /// Per-bell vibration pattern uuid.
    pub vibration_pattern_uuid: crate::db::VibrationPatternUuid,
    /// Bell volume (`crate::bell_volume`). Presets saved before
    /// per-bell volumes carry none and load at the middle.
    #[serde(default)]
    pub volume: crate::bell_volume::BellVolume,
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
    pub sound_uuid: crate::db::BellSoundUuid,
    pub pattern_uuid: crate::db::VibrationPatternUuid,
    /// Bell volume (`crate::bell_volume`). Presets saved before
    /// per-bell volumes carry none and load at the middle.
    #[serde(default)]
    pub volume: crate::bell_volume::BellVolume,
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
// produce a `PresetConfig`. `apply` is the inverse: writes settings,
// switches library bells and writes phase rows; returns the cfg's
// `timing` so the shell can reflect it into widget state.
//
// Apply is NOT atomic — each underlying `db.set_setting` /
// `db.set_interval_bell_enabled` opens its own per-method transaction. A
// future infrastructure pass on core::db (item: nestable transactions)
// would let apply wrap everything in one outer transaction; until then
// behaviour matches the GTK shell's prior non-transactional walker.

use crate::db::{
    BoxBreathPhaseId, Database, IntervalBell, IntervalBellKind, SessionMode, SignalMode,
};
use crate::format::PREP_SECS_DEFAULT;
use crate::seeds::{BUNDLED_BOWL_UUID, BUNDLED_PATTERN_PULSE_UUID};
use crate::settings_keys::{
    end_bell_active_key_for_mode, end_bell_pattern_key_for_mode,
    end_bell_signal_mode_key_for_mode, end_bell_sound_key_for_mode,
    end_bell_volume_key_for_mode, keep_screen_awake_key_for_mode, label_active_key_for_mode, label_uuid_key_for_mode,
    signal_mode_key_for_mode, stopwatch_key_for_mode, STARTING_BELL_VOLUME_KEY,
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

/// The starred presets of `mode` whose settings are exactly Setup's
/// as stored: the ones Setup shows as active. Only what Setup shows
/// for the mode counts (a Timer preset's saved Box Breath cues don't).
/// Derived, never stored, so it follows every change, local or synced.
pub fn active_presets(db: &Database, mode: SessionMode) -> std::collections::HashSet<String> {
    use crate::settings_keys::{breathing_from_db, read_bool, stopwatch_key_for_mode, timer_session_secs_from_db};
    let stopwatch = read_bool(db, stopwatch_key_for_mode(mode), false);
    let timing = if mode == SessionMode::BoxBreath {
        let (p, duration_secs) = breathing_from_db(db);
        PresetTiming::BoxBreath {
            stopwatch,
            inhale_secs: p.in_secs,
            hold_full_secs: p.hold_in,
            exhale_secs: p.out_secs,
            hold_empty_secs: p.hold_out,
            duration_secs,
        }
    } else {
        PresetTiming::Timer { stopwatch, duration_secs: timer_session_secs_from_db(db) }
    };
    let current = snapshot(db, mode, timing);
    let shown = |mut c: PresetConfig| {
        let vis = setup_visibility(mode);
        if stopwatch {
            // Setup hides Auto-Finish while counting up.
            c.auto_finish = false;
        }
        if !vis.starting_bell {
            c.starting_bell = PresetStartingBell::default();
        }
        if !vis.interval_bells {
            c.interval_bells = PresetIntervalBells::default();
        } else {
            // What rings decides, not the switched-off bells the
            // library keeps, their order or their rows.
            let ib = &mut c.interval_bells;
            let master = ib.enabled;
            ib.bells.retain(|b| master && b.enabled);
            ib.bells.iter_mut().for_each(|b| b.uuid = Default::default());
            ib.bells.sort();
        }
        if !vis.boxbreath_phase {
            c.box_breath_cues = PresetBoxBreathCues::default();
        } else if c.box_breath_cues.phases.is_empty() {
            // A preset without phase cues leaves them as they are.
            c.box_breath_cues.phases.clone_from(&current.box_breath_cues.phases);
        }
        c
    };
    let current_shown = shown(current.clone());
    crate::db::list_starred_presets_for_mode_from_db(db, mode)
        .unwrap_or_default()
        .into_iter()
        .filter(|p| PresetConfig::from_json(&p.config_json).is_ok_and(|c| shown(c) == current_shown))
        .map(|p| p.uuid.to_string())
        .collect()
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
    let read_volume = |k: &str| crate::bell_volume::BellVolume::parse(&read_str(k, ""));

    let label = PresetLabel {
        enabled: read_bool(label_active_key_for_mode(mode), false),
        uuid: db
            .get_setting(label_uuid_key_for_mode(mode), "")
            .ok()
            .filter(|s| !s.is_empty())
            .map(crate::db::LabelUuid::new),
    };

    let starting_bell = PresetStartingBell {
        enabled: read_bool("starting_bell_active", false),
        sound_uuid: read_str("starting_bell_sound", BUNDLED_BOWL_UUID).into(),
        prep_time_enabled: read_bool("preparation_time_active", false),
        prep_time_secs: read_u32("preparation_time_secs", PREP_SECS_DEFAULT),
        signal_mode: read_str("starting_bell_signal_mode", "sound"),
        vibration_pattern_uuid: read_str("starting_bell_pattern", BUNDLED_PATTERN_PULSE_UUID).into(),
        volume: read_volume(STARTING_BELL_VOLUME_KEY),
    };

    let end_bell = PresetEndBell {
        enabled: read_bool(end_bell_active_key_for_mode(mode), true),
        sound_uuid: read_str(end_bell_sound_key_for_mode(mode), BUNDLED_BOWL_UUID).into(),
        signal_mode: read_str(end_bell_signal_mode_key_for_mode(mode), "sound"),
        vibration_pattern_uuid: read_str(end_bell_pattern_key_for_mode(mode), BUNDLED_PATTERN_PULSE_UUID).into(),
        volume: read_volume(end_bell_volume_key_for_mode(mode)),
    };

    let intervals_enabled = read_bool("interval_bells_active", false);
    let bells: Vec<PresetIntervalBell> = db
        .list_interval_bells()
        .unwrap_or_default()
        .into_iter()
        .map(PresetIntervalBell::from)
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
        volume: p.volume,
    })
    .collect();
    let box_breath_cues = PresetBoxBreathCues {
        master_enabled: read_bool("boxbreath_cues_active", false),
        phases,
    };

    let cues_signal_mode = read_str(signal_mode_key_for_mode(mode), "both");
    let keep_screen_awake = read_bool(keep_screen_awake_key_for_mode(mode), false);
    let auto_finish = read_bool(crate::settings_keys::auto_finish_key_for_mode(mode), false);

    PresetConfig {
        label,
        starting_bell,
        interval_bells,
        end_bell,
        timing,
        cues_signal_mode,
        keep_screen_awake,
        auto_finish,
        box_breath_cues,
    }
}

/// Apply a `PresetConfig` to the DB: writes the per-mode and bell
/// settings, switches the interval-bell library to the preset's bells,
/// and writes box-breath phase rows. The Timer-only parts (starting
/// bell, preparation, interval bells) are written for a Timer preset
/// only, the phase rows for a Box Breath preset only.
///
/// Sound and pattern references are written as they are. Sync brings
/// rows in order, so one that is missing was deleted, and waiting for
/// it would never end; Setup shows that bell as Missing.
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
) -> crate::db::Result<PresetTiming> {
    // 1. Per-mode + bell-related settings.
    let (stopwatch_active, is_timer) = match cfg.timing {
        PresetTiming::Timer { stopwatch, .. } => (stopwatch, true),
        PresetTiming::BoxBreath { stopwatch, .. } => (stopwatch, false),
    };
    let set = |k: &str, v: &str| db.set_setting(k, v);
    let bool_str = crate::settings_keys::format_bool;

    set(label_active_key_for_mode(mode), bool_str(cfg.label.enabled))?;
    if let Some(luuid) = cfg.label.uuid.as_ref() {
        set(label_uuid_key_for_mode(mode), luuid.as_str())?;
    }
    if is_timer {
        set("starting_bell_active", bool_str(cfg.starting_bell.enabled))?;
        if !cfg.starting_bell.sound_uuid.is_empty() {
            set("starting_bell_sound", cfg.starting_bell.sound_uuid.as_str())?;
        }
        set("preparation_time_active", bool_str(cfg.starting_bell.prep_time_enabled))?;
        set("preparation_time_secs", &cfg.starting_bell.prep_time_secs.to_string())?;
        set("starting_bell_signal_mode", &cfg.starting_bell.signal_mode)?;
        set("starting_bell_pattern", cfg.starting_bell.vibration_pattern_uuid.as_str())?;
        set(STARTING_BELL_VOLUME_KEY, &cfg.starting_bell.volume.percent().to_string())?;
        set("interval_bells_active", bool_str(cfg.interval_bells.enabled))?;
    }
    set(end_bell_active_key_for_mode(mode), bool_str(cfg.end_bell.enabled))?;
    if !cfg.end_bell.sound_uuid.is_empty() {
        set(end_bell_sound_key_for_mode(mode), cfg.end_bell.sound_uuid.as_str())?;
    }
    set(stopwatch_key_for_mode(mode), bool_str(stopwatch_active))?;
    set(end_bell_signal_mode_key_for_mode(mode), &cfg.end_bell.signal_mode)?;
    set(end_bell_volume_key_for_mode(mode), &cfg.end_bell.volume.percent().to_string())?;
    set(end_bell_pattern_key_for_mode(mode), cfg.end_bell.vibration_pattern_uuid.as_str())?;
    set(signal_mode_key_for_mode(mode), &cfg.cues_signal_mode)?;
    set(
        keep_screen_awake_key_for_mode(mode),
        bool_str(cfg.keep_screen_awake),
    )?;
    set(crate::settings_keys::auto_finish_key_for_mode(mode), bool_str(cfg.auto_finish))?;

    // 2. Box-breath phase rows — only when cfg is BoxBreath. Timer
    // presets carry the seed values; stamping them on apply would
    // wipe the user's box-breath authoring.
    if !is_timer {
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
                p.sound_uuid.as_str(),
                p.pattern_uuid.as_str(),
                p.volume,
            )?;
        }
        return Ok(cfg.timing.clone());
    }

    // 3. Interval bells: switch each library bell on or off and never
    // delete one, so bells set up for other presets stay. A preset bell
    // is its library row, or else (presets saved before bells carried
    // their row, or rows another app version re-made) a row with the
    // same settings. A bell the preset rings that the library lacks is
    // added back.
    let library: Vec<PresetIntervalBell> =
        db.list_interval_bells()?.into_iter().map(PresetIntervalBell::from).collect();
    let mut on: Vec<Option<bool>> = vec![None; library.len()];
    for s in &cfg.interval_bells.bells {
        let row = library
            .iter()
            .position(|b| !s.uuid.is_empty() && b.uuid == s.uuid)
            .or_else(|| (0..library.len()).find(|&i| on[i].is_none() && library[i].same_settings(s)));
        if let Some(i) = row {
            on[i] = Some(s.enabled);
            continue;
        }
        let (true, Some(kind), Some(signal_mode)) = (
            s.enabled,
            IntervalBellKind::from_db_str(&s.kind),
            SignalMode::from_db_str(&s.signal_mode),
        ) else {
            continue;
        };
        let rowid = db.insert_interval_bell(
            kind,
            s.time,
            s.jitter,
            s.step,
            s.sound_uuid.as_str(),
            s.vibration_pattern_uuid.as_str(),
            signal_mode,
        )?;
        // A fresh row is enabled and at the default volume.
        if s.volume != crate::bell_volume::BellVolume::default() {
            if let Some(b) = db.find_interval_bell_by_id(rowid)? {
                db.update_interval_bell(&IntervalBell { volume: s.volume, ..b })?;
            }
        }
    }
    for (b, on) in library.iter().zip(on) {
        if b.enabled != on.unwrap_or(false) {
            db.set_interval_bell_enabled(b.uuid.as_str(), !b.enabled)?;
        }
    }

    Ok(cfg.timing.clone())
}

/// What `edit_interval_bell` did with an edit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BellEdit {
    InPlace,
    /// A saved preset rings the bell, so the edit went into this new
    /// bell next to it and the old one was switched off.
    Copied(crate::db::IntervalBellUuid),
}

/// Save an edit of an interval bell from Setup. A bell a saved preset
/// rings is never changed: the edit becomes a new bell, so the preset
/// keeps ringing the bell it was saved with.
pub fn edit_interval_bell(db: &Database, bell: &IntervalBell) -> crate::db::Result<BellEdit> {
    let library = db.list_interval_bells()?;
    if let Some(old) = library.iter().find(|b| b.uuid == bell.uuid) {
        let before = PresetIntervalBell::from(old.clone());
        if !before.same_settings(&PresetIntervalBell::from(bell.clone())) && rung_by_a_preset(db, &before, &library)? {
            // Two devices copying the same bell offline make the same
            // copy, so the later edit wins, as for any edit. Never the
            // uuid of a deleted copy, which a preset may still ring.
            let mut n = 0;
            let uuid = loop {
                let uuid = copy_uuid(old.uuid.as_str(), n);
                if library.iter().all(|b| b.uuid.as_str() != uuid) && !db.has_events_for(&uuid)? {
                    break uuid;
                }
                n += 1;
            };
            return Ok(BellEdit::Copied(db.copy_interval_bell(old, bell, &uuid)?));
        }
    }
    db.update_interval_bell(bell)?;
    Ok(BellEdit::InPlace)
}

/// The `n`th copy's uuid of bell `old`: the same on every device.
fn copy_uuid(old: &str, n: u32) -> String {
    // FNV-1a 128: the same on every build, unlike std's hasher.
    let hash = format!("{old}/{n}").bytes().fold(0x6c62_272e_07bb_0142_62b8_2175_6295_c58d_u128, |h, b| {
        (h ^ u128::from(b)).wrapping_mul(0x0000_0000_0100_0000_0000_0000_0000_013b)
    });
    uuid::Builder::from_custom_bytes(hash.to_be_bytes()).into_uuid().to_string()
}

/// Whether a saved Timer preset rings `bell`: by its row, or, as `apply`
/// finds it when the preset's row is gone (or it was saved before bells
/// carried their row), by its settings.
fn rung_by_a_preset(db: &Database, bell: &PresetIntervalBell, library: &[IntervalBell]) -> crate::db::Result<bool> {
    Ok(crate::db::list_presets_from_db(db)?
        .iter()
        .filter(|p| p.mode == SessionMode::Timer)
        .filter_map(|p| PresetConfig::from_json(&p.config_json).ok())
        .filter(|c| c.interval_bells.enabled)
        .flat_map(|c| c.interval_bells.bells)
        .any(|s| {
            s.enabled
                && (s.uuid == bell.uuid
                    || (library.iter().all(|b| b.uuid != s.uuid) && s.same_settings(bell)))
        }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn timer_config() -> PresetConfig {
        PresetConfig {
            label: PresetLabel {
                enabled: true,
                uuid: Some("label-uuid".into()),
            },
            starting_bell: PresetStartingBell {
                enabled: true,
                sound_uuid: "bell-uuid".into(),
                prep_time_enabled: false,
                prep_time_secs: 5,
                signal_mode: "both".to_string(),
                vibration_pattern_uuid: "pulse-uuid".into(),
                volume: Default::default(),
            },
            interval_bells: PresetIntervalBells {
                enabled: false,
                bells: vec![],
            },
            end_bell: PresetEndBell {
                enabled: true,
                sound_uuid: "end-uuid".into(),
                signal_mode: "vibration".to_string(),
                vibration_pattern_uuid: "heartbeat-uuid".into(),
                volume: Default::default(),
            },
            timing: PresetTiming::Timer {
                stopwatch: false,
                duration_secs: 900,
            },
            cues_signal_mode: "both".to_string(),
            keep_screen_awake: true,
            auto_finish: false,
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
                sound_uuid: "end-uuid".into(),
                signal_mode: "sound".to_string(),
                vibration_pattern_uuid: crate::db::VibrationPatternUuid::default(),
                volume: Default::default(),
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
            auto_finish: false,
            box_breath_cues: PresetBoxBreathCues {
                master_enabled: true,
                phases: vec![
                    PresetBoxBreathPhase {
                        phase: "in".to_string(),
                        enabled: true,
                        signal_mode: "vibration".to_string(),
                        sound_uuid: crate::db::BellSoundUuid::default(),
                        pattern_uuid: "pulse-uuid".into(),
                        volume: Default::default(),
                    },
                    PresetBoxBreathPhase {
                        phase: "holdin".to_string(),
                        enabled: false,
                        signal_mode: "sound".to_string(),
                        sound_uuid: "voice-uuid".into(),
                        pattern_uuid: "pulse-uuid".into(),
                        volume: Default::default(),
                    },
                    PresetBoxBreathPhase {
                        phase: "out".to_string(),
                        enabled: true,
                        signal_mode: "both".to_string(),
                        sound_uuid: "voice-uuid".into(),
                        pattern_uuid: "wave-uuid".into(),
                        volume: Default::default(),
                    },
                    PresetBoxBreathPhase {
                        phase: "holdout".to_string(),
                        enabled: false,
                        signal_mode: "sound".to_string(),
                        sound_uuid: crate::db::BellSoundUuid::default(),
                        pattern_uuid: crate::db::VibrationPatternUuid::default(),
                        volume: Default::default(),
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
                        time: BellTime::new(5, TimeUnit::Min),
                        jitter: BellTime::new(30, TimeUnit::Sec),
                        step: BellTime::ZERO,
                        sound_uuid: "ping-uuid".into(),
                        enabled: true,
                        signal_mode: "vibration".to_string(),
                        vibration_pattern_uuid: "wave-uuid".into(),
                        volume: Default::default(),
                        uuid: "row-1".into(),
                    },
                    PresetIntervalBell {
                        kind: "fixed_from_start".to_string(),
                        time: BellTime::new(1, TimeUnit::Min),
                        jitter: BellTime::ZERO,
                        step: BellTime::ZERO,
                        sound_uuid: "tick-uuid".into(),
                        enabled: false,
                        signal_mode: "sound".to_string(),
                        vibration_pattern_uuid: crate::db::VibrationPatternUuid::default(),
                        volume: Default::default(),
                        uuid: Default::default(),
                    },
                ],
            },
            end_bell: PresetEndBell::default(),
            timing: PresetTiming::Timer { stopwatch: false, duration_secs: 600 },
            cues_signal_mode: String::new(),
            keep_screen_awake: false,
            auto_finish: false,
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
            label: PresetLabel { enabled: true, uuid: Some("label-uuid".into()) },
            starting_bell: PresetStartingBell {
                enabled: true,
                sound_uuid: sound_uuid.into(),
                prep_time_enabled: true,
                prep_time_secs: 30,
                signal_mode: "both".to_string(),
                vibration_pattern_uuid: pattern_uuid.into(),
                volume: Default::default(),
            },
            interval_bells: PresetIntervalBells {
                enabled: true,
                bells: vec![PresetIntervalBell {
                    kind: "interval".to_string(),
                    time: BellTime::new(5, TimeUnit::Min),
                    jitter: BellTime::new(30, TimeUnit::Sec),
                    step: BellTime::ZERO,
                    sound_uuid: sound_uuid.into(),
                    enabled: true,
                    signal_mode: "sound".to_string(),
                    vibration_pattern_uuid: crate::db::VibrationPatternUuid::default(),
                    volume: Default::default(),
                    uuid: Default::default(),
                }],
            },
            end_bell: PresetEndBell {
                enabled: true,
                sound_uuid: sound_uuid.into(),
                signal_mode: "vibration".to_string(),
                vibration_pattern_uuid: pattern_uuid.into(),
                volume: Default::default(),
            },
            timing,
            cues_signal_mode: "both".to_string(),
            keep_screen_awake: true,
            auto_finish: false,
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
    fn a_preset_whose_sound_or_pattern_is_gone_still_applies() {
        // Sync brings rows in order, so a missing one was deleted and
        // waiting for it never ends; Setup shows that bell as Missing.
        let (db, _sound, _pattern) = fresh_db();
        let cfg = cfg_with_known_uuids(
            PresetTiming::Timer { stopwatch: false, duration_secs: 600 },
            "deleted-sound",
            "deleted-pattern",
        );
        apply(&db, &cfg, SessionMode::Timer).expect("apply succeeds");
        assert_eq!(db.get_setting(end_bell_sound_key_for_mode(SessionMode::Timer), "").unwrap(), "deleted-sound");
        assert_eq!(db.get_setting("starting_bell_pattern", "").unwrap(), "deleted-pattern");
    }

    #[test]
    fn a_box_breath_preset_leaves_the_timer_bells_alone() {
        // Starting bell, preparation and interval bells are Timer-only
        // settings; the bundled Box Breath preset switched them off.
        let (db, sound, pattern) = fresh_db();
        let timer = cfg_with_known_uuids(
            PresetTiming::Timer { stopwatch: false, duration_secs: 600 },
            &sound,
            &pattern,
        );
        apply(&db, &timer, SessionMode::Timer).unwrap();
        let before = snapshot(&db, SessionMode::Timer, timer.timing.clone());
        let [.., (_, _, mode, box_breath)] = crate::seeds::default_presets();
        assert_eq!(mode, SessionMode::BoxBreath);
        apply(&db, &box_breath, SessionMode::BoxBreath).unwrap();
        let after = snapshot(&db, SessionMode::Timer, timer.timing.clone());
        assert_eq!(after.starting_bell, before.starting_bell);
        assert_eq!(after.interval_bells, before.interval_bells);
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
            crate::bell_volume::BellVolume::default(),
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
            auto_finish: false,
            box_breath_cues: PresetBoxBreathCues {
                master_enabled: true,
                phases: vec![PresetBoxBreathPhase {
                    phase: "in".to_string(),
                    enabled: true,
                    signal_mode: "sound".to_string(),
                    sound_uuid: sound.clone().into(),
                    pattern_uuid: pattern.clone().into(),
                    volume: Default::default(),
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

    fn bell(minutes: u32, enabled: bool) -> PresetIntervalBell {
        PresetIntervalBell {
            kind: "interval".to_string(),
            time: BellTime::new(minutes, TimeUnit::Min),
            jitter: BellTime::ZERO,
            step: BellTime::ZERO,
            sound_uuid: "s".into(),
            enabled,
            signal_mode: "sound".to_string(),
            vibration_pattern_uuid: "p".into(),
            volume: Default::default(),
            uuid: Default::default(),
        }
    }

    fn with_bells(bells: Vec<PresetIntervalBell>) -> PresetConfig {
        PresetConfig {
            interval_bells: PresetIntervalBells { enabled: true, bells },
            ..Default::default()
        }
    }

    fn bell_rows(db: &Database) -> Vec<(u32, bool)> {
        db.list_interval_bells().unwrap().iter().map(|b| (b.time.number(), b.enabled)).collect()
    }

    #[test]
    fn a_preset_switches_the_library_bells_and_never_deletes_one() {
        // Deleting the other bells lost the ones set up for other
        // presets, and two devices applying offline doubled them.
        let db = Database::open_in_memory().unwrap();
        apply(&db, &with_bells(vec![bell(5, true)]), SessionMode::Timer).unwrap();
        apply(&db, &with_bells(vec![bell(10, true)]), SessionMode::Timer).unwrap();
        assert_eq!(bell_rows(&db), [(5, false), (10, true)], "the other preset's bell stays, switched off");
        apply(&db, &with_bells(vec![bell(5, true)]), SessionMode::Timer).unwrap();
        assert_eq!(bell_rows(&db), [(5, true), (10, false)], "and is found again, not added twice");
    }

    #[test]
    fn a_preset_finds_its_own_bell_after_the_bell_was_edited() {
        let db = Database::open_in_memory().unwrap();
        apply(&db, &with_bells(vec![bell(5, true)]), SessionMode::Timer).unwrap();
        let saved = snapshot(&db, SessionMode::Timer, PresetTiming::default());
        let row = db.list_interval_bells().unwrap().remove(0);
        db.update_interval_bell(&IntervalBell { time: BellTime::new(7, TimeUnit::Min), enabled: false, ..row }).unwrap();
        apply(&db, &saved, SessionMode::Timer).unwrap();
        assert_eq!(bell_rows(&db), [(7, true)]);
    }

    /// Apply `bells`, save them as a starred Timer preset, return it.
    fn saved_preset(db: &Database, bells: Vec<PresetIntervalBell>) -> PresetConfig {
        apply(db, &with_bells(bells), SessionMode::Timer).unwrap();
        let saved = snapshot(db, SessionMode::Timer, PresetTiming::default());
        db.insert_preset("P", SessionMode::Timer, true, &saved.to_json()).unwrap();
        saved
    }

    fn sync(from: &Database, to: &Database) {
        let events: Vec<_> = from.pending_events().unwrap().into_iter().map(|(_, e)| e).collect();
        to.replay_events(&events).unwrap();
    }

    fn row_at(db: &Database, minutes: u32) -> IntervalBell {
        db.list_interval_bells().unwrap().into_iter().find(|b| b.time.number() == minutes).unwrap()
    }

    fn sorted_rows(db: &Database) -> Vec<(u32, bool)> {
        let mut rows = bell_rows(db);
        rows.sort();
        rows
    }

    fn at(minutes: u32, b: IntervalBell) -> IntervalBell {
        IntervalBell { time: BellTime::new(minutes, TimeUnit::Min), ..b }
    }

    #[test]
    fn editing_a_preset_bell_makes_a_new_bell() {
        let db = Database::open_in_memory().unwrap();
        let quiet = PresetIntervalBell { volume: crate::bell_volume::BellVolume::from_percent(30.0), ..bell(5, true) };
        let preset = saved_preset(&db, vec![quiet]);
        let old = row_at(&db, 5);
        let edited = IntervalBell { sound_uuid: "t".into(), volume: crate::bell_volume::BellVolume::from_percent(80.0), ..at(7, old.clone()) };
        let BellEdit::Copied(new) = edit_interval_bell(&db, &edited).unwrap() else { panic!("not copied") };
        assert_ne!(new, old.uuid);
        assert_eq!(sorted_rows(&db), [(5, false), (7, true)]);
        assert_eq!((row_at(&db, 7).uuid, row_at(&db, 7).sound_uuid.as_str(), row_at(&db, 7).volume.percent()), (new, "t", 80));
        apply(&db, &preset, SessionMode::Timer).unwrap();
        assert_eq!(sorted_rows(&db), [(5, true), (7, false)]);
        assert_eq!((row_at(&db, 5).sound_uuid.as_str(), row_at(&db, 5).volume.percent()), ("s", 30));
    }

    #[test]
    fn a_save_without_changes_copies_nothing() {
        let db = Database::open_in_memory().unwrap();
        saved_preset(&db, vec![bell(5, true)]);
        assert_eq!(edit_interval_bell(&db, &row_at(&db, 5)).unwrap(), BellEdit::InPlace);
        assert_eq!(bell_rows(&db), [(5, true)]);
    }

    #[test]
    fn only_bells_a_preset_rings_are_copied() {
        let db = Database::open_in_memory().unwrap();
        apply(&db, &with_bells(vec![bell(10, true)]), SessionMode::Timer).unwrap();
        saved_preset(&db, vec![bell(5, true)]);
        let mut bells_off = snapshot(&db, SessionMode::Timer, PresetTiming::default());
        bells_off.interval_bells.enabled = false;
        bells_off.interval_bells.bells.retain(|b| b.time.number() == 5);
        bells_off.interval_bells.bells[0].uuid = row_at(&db, 10).uuid;
        db.insert_preset("Bells off", SessionMode::Timer, true, &bells_off.to_json()).unwrap();
        let breath = PresetConfig {
            timing: PresetTiming::BoxBreath { stopwatch: false, inhale_secs: 4, hold_full_secs: 4, exhale_secs: 4, hold_empty_secs: 4, duration_secs: 300 },
            interval_bells: PresetIntervalBells { enabled: true, bells: vec![PresetIntervalBell { enabled: true, ..PresetIntervalBell::from(row_at(&db, 10)) }] },
            ..Default::default()
        };
        db.insert_preset("Breath", SessionMode::BoxBreath, true, &breath.to_json()).unwrap();
        assert_eq!(edit_interval_bell(&db, &at(12, row_at(&db, 10))).unwrap(), BellEdit::InPlace);
        assert_eq!(bell_rows(&db), [(12, false), (5, true)]);
    }

    #[test]
    fn the_new_bell_takes_further_edits_in_place() {
        let db = Database::open_in_memory().unwrap();
        saved_preset(&db, vec![bell(5, true)]);
        let BellEdit::Copied(new) = edit_interval_bell(&db, &at(7, row_at(&db, 5))).unwrap() else { panic!("not copied") };
        assert_eq!(edit_interval_bell(&db, &at(9, row_at(&db, 7))).unwrap(), BellEdit::InPlace);
        assert_eq!(sorted_rows(&db), [(5, false), (9, true)]);
        assert_eq!(row_at(&db, 9).uuid, new);
    }

    #[test]
    fn a_bell_no_preset_uses_is_edited_in_place() {
        let db = Database::open_in_memory().unwrap();
        apply(&db, &with_bells(vec![bell(5, true)]), SessionMode::Timer).unwrap();
        assert_eq!(edit_interval_bell(&db, &at(7, row_at(&db, 5))).unwrap(), BellEdit::InPlace);
        assert_eq!(bell_rows(&db), [(7, true)]);
    }

    #[test]
    fn the_new_bell_sits_next_to_the_old_one() {
        let db = Database::open_in_memory().unwrap();
        saved_preset(&db, vec![bell(3, true), bell(5, true), bell(10, true)]);
        edit_interval_bell(&db, &at(7, row_at(&db, 5))).unwrap();
        let order: Vec<u32> = bell_rows(&db).iter().map(|r| r.0).collect();
        assert_eq!((order.len(), order[0], order[3]), (4, 3, 10), "{order:?}");
    }

    #[test]
    fn another_device_gets_the_copy_as_it_is() {
        let db = Database::open_in_memory().unwrap();
        saved_preset(&db, vec![bell(3, true), bell(5, true), bell(10, true)]);
        let loud = IntervalBell { volume: crate::bell_volume::BellVolume::from_percent(80.0), ..at(7, row_at(&db, 5)) };
        edit_interval_bell(&db, &loud).unwrap();
        let peer = Database::open_in_memory().unwrap();
        sync(&db, &peer);
        let rows = |db: &Database| -> Vec<(String, u32, bool, u8, String)> {
            db.list_interval_bells().unwrap().into_iter()
                .map(|b| (b.uuid.as_str().to_owned(), b.time.number(), b.enabled, b.volume.percent(), b.created_iso))
                .collect()
        };
        assert_eq!(rows(&peer), rows(&db));
    }

    #[test]
    fn two_devices_editing_the_same_bell_keep_one_copy() {
        let phone = Database::open_in_memory().unwrap();
        saved_preset(&phone, vec![bell(5, true)]);
        let laptop = Database::open_in_memory().unwrap();
        sync(&phone, &laptop);
        edit_interval_bell(&phone, &at(7, row_at(&phone, 5))).unwrap();
        edit_interval_bell(&laptop, &at(9, row_at(&laptop, 5))).unwrap();
        sync(&phone, &laptop);
        sync(&laptop, &phone);
        assert_eq!(sorted_rows(&phone).len(), 2, "{:?}", sorted_rows(&phone));
        assert_eq!(sorted_rows(&phone), sorted_rows(&laptop));
        assert_eq!(sorted_rows(&phone)[0], (5, false));
        assert!([(7, true), (9, true)].contains(&sorted_rows(&phone)[1]), "one of the two edits");
    }

    #[test]
    fn a_second_copy_of_the_same_bell_is_a_bell_of_its_own() {
        let db = Database::open_in_memory().unwrap();
        saved_preset(&db, vec![bell(5, true)]);
        edit_interval_bell(&db, &at(7, row_at(&db, 5))).unwrap();
        let old = row_at(&db, 5);
        db.set_interval_bell_enabled(old.uuid.as_str(), true).unwrap();
        edit_interval_bell(&db, &at(9, row_at(&db, 5))).unwrap();
        assert_eq!(sorted_rows(&db), [(5, false), (7, true), (9, true)]);
    }

    #[test]
    fn a_deleted_copy_is_not_brought_back_by_the_next_edit() {
        let db = Database::open_in_memory().unwrap();
        let a = saved_preset(&db, vec![bell(5, true)]);
        edit_interval_bell(&db, &at(7, row_at(&db, 5))).unwrap();
        let b = snapshot(&db, SessionMode::Timer, PresetTiming::default());
        db.insert_preset("B", SessionMode::Timer, true, &b.to_json()).unwrap();
        db.delete_interval_bell(row_at(&db, 7).uuid.as_str()).unwrap();
        apply(&db, &a, SessionMode::Timer).unwrap();
        edit_interval_bell(&db, &at(12, row_at(&db, 5))).unwrap();
        apply(&db, &b, SessionMode::Timer).unwrap();
        assert_eq!(sorted_rows(&db), [(5, false), (7, true), (12, false)]);
    }

    #[test]
    fn a_preset_whose_bell_was_re_made_keeps_it_too() {
        let db = Database::open_in_memory().unwrap();
        let preset = saved_preset(&db, vec![bell(5, true)]);
        db.delete_interval_bell(row_at(&db, 5).uuid.as_str()).unwrap();
        apply(&db, &preset, SessionMode::Timer).unwrap();
        assert!(matches!(edit_interval_bell(&db, &at(7, row_at(&db, 5))).unwrap(), BellEdit::Copied(_)));
        assert_eq!(sorted_rows(&db), [(5, false), (7, true)]);
    }

    #[test]
    fn an_old_preset_keeps_its_bell_too() {
        let db = Database::open_in_memory().unwrap();
        apply(&db, &with_bells(vec![bell(5, true)]), SessionMode::Timer).unwrap();
        let mut old = snapshot(&db, SessionMode::Timer, PresetTiming::default());
        old.interval_bells.bells[0].uuid = Default::default();
        db.insert_preset("Old", SessionMode::Timer, true, &old.to_json()).unwrap();
        assert!(matches!(edit_interval_bell(&db, &at(7, row_at(&db, 5))).unwrap(), BellEdit::Copied(_)));
        assert_eq!(sorted_rows(&db), [(5, false), (7, true)]);
    }

    #[test]
    fn a_deleted_bell_comes_back_when_a_preset_rings_it() {
        let db = Database::open_in_memory().unwrap();
        let preset = with_bells(vec![bell(5, true), bell(10, false)]);
        apply(&db, &preset, SessionMode::Timer).unwrap();
        assert_eq!(bell_rows(&db), [(5, true)], "a bell the preset keeps off is not added");
        let row = db.list_interval_bells().unwrap().remove(0);
        db.delete_interval_bell(row.uuid.as_str()).unwrap();
        apply(&db, &preset, SessionMode::Timer).unwrap();
        assert_eq!(bell_rows(&db), [(5, true)]);
    }

    #[test]
    fn applying_the_same_preset_again_writes_no_bell_events() {
        let db = Database::open_in_memory().unwrap();
        let bell_events = |db: &Database| {
            db.all_events().unwrap().iter().filter(|e| e.kind.starts_with("interval_bell")).count()
        };
        let preset = with_bells(vec![bell(5, true), bell(10, true)]);
        apply(&db, &preset, SessionMode::Timer).unwrap();
        let after_first = bell_events(&db);
        apply(&db, &preset, SessionMode::Timer).unwrap();
        assert_eq!(bell_events(&db), after_first);
    }

    #[test]
    fn switched_off_bells_and_their_order_do_not_change_the_active_preset() {
        let db = Database::open_in_memory().unwrap();
        db.set_setting("timer_session_secs", "600").unwrap();
        apply(&db, &with_bells(vec![bell(5, true), bell(10, true)]), SessionMode::Timer).unwrap();
        let mut saved = snapshot(&db, SessionMode::Timer, PresetTiming::default());
        saved.interval_bells.bells.reverse();
        db.insert_preset("Two bells", SessionMode::Timer, true, &saved.to_json()).unwrap();
        assert_eq!(active_presets(&db, SessionMode::Timer).len(), 1, "saved in another order");
        db.insert_interval_bell(IntervalBellKind::Interval, BellTime::new(3, TimeUnit::Min), BellTime::ZERO, BellTime::ZERO, "s", "p", SignalMode::Sound).unwrap();
        let extra = db.list_interval_bells().unwrap().into_iter().find(|b| b.time == BellTime::new(3, TimeUnit::Min)).unwrap();
        assert_eq!(active_presets(&db, SessionMode::Timer).len(), 0, "a third bell rings");
        db.set_interval_bell_enabled(extra.uuid.as_str(), false).unwrap();
        assert_eq!(active_presets(&db, SessionMode::Timer).len(), 1, "switched off, it doesn't count");
    }

    #[test]
    fn the_active_preset_is_the_one_matching_setup() {
        let (db, sound, pattern) = fresh_db();
        let timing = PresetTiming::Timer { stopwatch: false, duration_secs: 1200 };
        let config = cfg_with_known_uuids(timing.clone(), &sound, &pattern);
        apply(&db, &config, SessionMode::Timer).unwrap();
        db.set_setting("timer_session_secs", "1200").unwrap();
        let quiet = snapshot(&db, SessionMode::Timer, timing.clone());
        db.insert_preset("Quiet", SessionMode::Timer, true, &quiet.to_json()).unwrap();
        db.set_setting("starting_bell_active", if quiet.starting_bell.enabled { "false" } else { "true" }).unwrap();
        let other = snapshot(&db, SessionMode::Timer, timing.clone());
        db.insert_preset("Other", SessionMode::Timer, true, &other.to_json()).unwrap();
        db.insert_preset("Unstarred", SessionMode::Timer, false, &other.to_json()).unwrap();
        let names = || -> Vec<String> {
            let uuids = active_presets(&db, SessionMode::Timer);
            let mut names: Vec<String> = crate::db::list_presets_for_mode_from_db(&db, SessionMode::Timer)
                .unwrap()
                .into_iter()
                .filter(|p| uuids.contains(p.uuid.as_str()))
                .map(|p| p.name)
                .collect();
            names.sort();
            names
        };
        assert_eq!(names(), ["Other"], "only starred ones show");
        db.set_setting("starting_bell_active", if quiet.starting_bell.enabled { "true" } else { "false" }).unwrap();
        assert_eq!(names(), ["Quiet"], "set back, it matches again");
        db.set_setting("timer_session_secs", "1500").unwrap();
        assert!(names().is_empty(), "another duration matches none");
        db.set_setting("timer_session_secs", "1200").unwrap();
        db.set_setting("timer_stopwatch_active", "true").unwrap();
        assert!(names().is_empty(), "the stopwatch toggle counts too");
    }

    #[test]
    fn only_what_setup_shows_decides_the_active_preset() {
        // On the phone no preset ever matched: Timer presets carry the
        // Box Breath cues as they were when saved, and the bundled
        // Box Breath presets carry no phase cues at all.
        let (db, sound, pattern) = fresh_db();
        let timing = PresetTiming::Timer { stopwatch: false, duration_secs: 1200 };
        apply(&db, &cfg_with_known_uuids(timing.clone(), &sound, &pattern), SessionMode::Timer).unwrap();
        db.set_setting("timer_session_secs", "1200").unwrap();
        let timer = snapshot(&db, SessionMode::Timer, timing);
        db.insert_preset("Sit", SessionMode::Timer, true, &timer.to_json()).unwrap();
        let mut breath = snapshot(&db, SessionMode::BoxBreath, PresetTiming::BoxBreath {
            stopwatch: false, inhale_secs: 4, hold_full_secs: 4, exhale_secs: 4, hold_empty_secs: 4, duration_secs: 300,
        });
        breath.box_breath_cues.phases.clear();
        db.insert_preset("Box", SessionMode::BoxBreath, true, &breath.to_json()).unwrap();
        db.set_box_breath_phase(
            crate::db::BoxBreathPhaseId::In, false, SignalMode::Vibration, &sound, &pattern, Default::default(),
        )
        .unwrap();
        db.set_setting("interval_bells_active", if timer.interval_bells.enabled { "false" } else { "true" }).unwrap();
        db.set_setting("breathing_session_secs", "300").unwrap();

        assert!(active_presets(&db, SessionMode::Timer).is_empty(), "the interval bells show in Timer");
        db.set_setting("interval_bells_active", if timer.interval_bells.enabled { "true" } else { "false" }).unwrap();
        assert_eq!(active_presets(&db, SessionMode::Timer).len(), 1, "changed Box Breath cues don't count in Timer");
        db.set_setting("interval_bells_active", if timer.interval_bells.enabled { "false" } else { "true" }).unwrap();
        assert_eq!(active_presets(&db, SessionMode::BoxBreath).len(), 1, "hidden bells and unset phase cues don't count");
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

        // Settings + label should round-trip. Added interval bells get
        // fresh row UUIDs, so we compare by structural fields rather
        // than serializing.
        assert_eq!(round_tripped.label, original.label);
        assert_eq!(round_tripped.starting_bell, original.starting_bell);
        assert_eq!(round_tripped.end_bell, original.end_bell);
        assert_eq!(round_tripped.cues_signal_mode, original.cues_signal_mode);
        assert_eq!(round_tripped.keep_screen_awake, original.keep_screen_awake);
        assert_eq!(round_tripped.timing, original.timing);
        // interval-bell content (minus the row uuids).
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
            assert_eq!(got.time, exp.time);
            assert_eq!(got.jitter, exp.jitter);
            assert_eq!(got.step, exp.step);
            assert_eq!(got.sound_uuid, exp.sound_uuid);
            assert_eq!(got.enabled, exp.enabled);
            assert_eq!(got.signal_mode, exp.signal_mode);
        }
    }

    // ── Bell volumes ───────────────────────────────────────────────

    use crate::bell_volume::BellVolume;

    fn pct(p: f64) -> BellVolume {
        BellVolume::from_percent(p)
    }

    /// A Timer preset with distinct volumes on every bell, plus a
    /// Box Breath preset with distinct volumes on each phase cue.
    fn presets_with_volumes(sound: &str, pattern: &str) -> (PresetConfig, PresetConfig) {
        let mut timer = cfg_with_known_uuids(
            PresetTiming::Timer { stopwatch: false, duration_secs: 1200 },
            sound,
            pattern,
        );
        timer.starting_bell.volume = pct(15.0);
        timer.end_bell.volume = pct(85.0);
        let bell = |minutes, volume| PresetIntervalBell {
            kind: "interval".to_string(),
            time: BellTime::new(minutes, TimeUnit::Min),
            jitter: BellTime::ZERO,
            step: BellTime::ZERO,
            sound_uuid: sound.into(),
            enabled: true,
            signal_mode: "sound".to_string(),
            vibration_pattern_uuid: pattern.into(),
            volume: pct(volume),
            uuid: Default::default(),
        };
        timer.interval_bells = PresetIntervalBells {
            enabled: true,
            bells: vec![bell(5, 25.0), bell(10, 70.0)],
        };
        let phase = |name: &str, volume| PresetBoxBreathPhase {
            phase: name.to_string(),
            enabled: true,
            signal_mode: "sound".to_string(),
            sound_uuid: sound.into(),
            pattern_uuid: pattern.into(),
            volume: pct(volume),
        };
        let box_breath = PresetConfig {
            timing: PresetTiming::BoxBreath {
                stopwatch: false,
                inhale_secs: 4,
                hold_full_secs: 4,
                exhale_secs: 4,
                hold_empty_secs: 4,
                duration_secs: 600,
            },
            end_bell: PresetEndBell { volume: pct(35.0), ..timer.end_bell.clone() },
            box_breath_cues: PresetBoxBreathCues {
                master_enabled: true,
                phases: vec![
                    phase("in", 10.0),
                    phase("holdin", 30.0),
                    phase("out", 60.0),
                    phase("holdout", 90.0),
                ],
            },
            ..timer.clone()
        };
        (timer, box_breath)
    }

    #[test]
    fn applying_a_preset_sets_every_bells_volume() {
        let (db, sound, pattern) = fresh_db();
        let (timer, box_breath) = presets_with_volumes(&sound, &pattern);
        apply(&db, &timer, SessionMode::Timer).unwrap();
        apply(&db, &box_breath, SessionMode::BoxBreath).unwrap();

        let starting = crate::bells::starting_bell_cue_from_db(&db, SessionMode::Timer).unwrap();
        assert_eq!(starting.volume, pct(15.0));
        let end = |mode| {
            crate::bells::end_bell_cue_from_db(&db, crate::bells::DisplayMode::Countdown, mode)
                .unwrap()
                .volume
        };
        assert_eq!(end(SessionMode::Timer), pct(85.0));
        assert_eq!(end(SessionMode::BoxBreath), pct(35.0));
        let bells: Vec<BellVolume> =
            db.list_interval_bells().unwrap().iter().map(|b| b.volume).collect();
        assert_eq!(bells, vec![pct(25.0), pct(70.0)]);
        let phases: Vec<BellVolume> =
            db.list_box_breath_phases().unwrap().iter().map(|p| p.volume).collect();
        assert_eq!(phases, vec![pct(10.0), pct(30.0), pct(60.0), pct(90.0)]);
    }

    #[test]
    fn a_switched_off_bell_keeps_its_volume_through_a_preset() {
        let (db, sound, pattern) = fresh_db();
        let (mut timer, _) = presets_with_volumes(&sound, &pattern);
        apply(&db, &timer, SessionMode::Timer).unwrap();
        timer.interval_bells.bells[1].enabled = false;
        apply(&db, &timer, SessionMode::Timer).unwrap();
        let rows = db.list_interval_bells().unwrap();
        assert!(!rows[1].enabled);
        assert_eq!(rows[1].volume, pct(70.0));
    }

    #[test]
    fn a_preset_snapshot_saves_every_bells_volume() {
        let (db, sound, pattern) = fresh_db();
        let (timer, box_breath) = presets_with_volumes(&sound, &pattern);
        apply(&db, &timer, SessionMode::Timer).unwrap();
        let saved = snapshot(&db, SessionMode::Timer, timer.timing.clone());
        assert_eq!(saved.starting_bell.volume, pct(15.0));
        assert_eq!(saved.end_bell.volume, pct(85.0));
        let bells: Vec<BellVolume> = saved.interval_bells.bells.iter().map(|b| b.volume).collect();
        assert_eq!(bells, vec![pct(25.0), pct(70.0)]);

        apply(&db, &box_breath, SessionMode::BoxBreath).unwrap();
        let saved = snapshot(&db, SessionMode::BoxBreath, box_breath.timing.clone());
        assert_eq!(saved.end_bell.volume, pct(35.0));
        let phases: Vec<BellVolume> =
            saved.box_breath_cues.phases.iter().map(|p| p.volume).collect();
        assert_eq!(phases, vec![pct(10.0), pct(30.0), pct(60.0), pct(90.0)]);
    }

    #[test]
    fn a_preset_keeps_auto_finish() {
        let (db, sound, pattern) = fresh_db();
        let timing = PresetTiming::Timer { stopwatch: false, duration_secs: 1200 };
        apply(&db, &cfg_with_known_uuids(timing.clone(), &sound, &pattern), SessionMode::Timer).unwrap();
        db.set_setting("timer_auto_finish", "true").unwrap();
        let saved = snapshot(&db, SessionMode::Timer, timing);
        db.set_setting("timer_auto_finish", "false").unwrap();
        apply(&db, &saved, SessionMode::Timer).unwrap();
        assert_eq!(db.get_setting("timer_auto_finish", "").unwrap(), "true");
        let back = PresetConfig::from_json(&saved.to_json()).unwrap();
        assert!(back.auto_finish, "it survives the JSON");
    }

    #[test]
    fn a_preset_from_before_auto_finish_applies_it_off() {
        // config_json as 26.10.1 wrote it: no "auto_finish".
        let json = r#"{
            "label": {"enabled": false, "uuid": null},
            "starting_bell": {"enabled": true, "sound_uuid": "s", "prep_time_enabled": false,
                "prep_time_secs": 5, "signal_mode": "sound", "vibration_pattern_uuid": "p", "volume": 50},
            "interval_bells": {"enabled": true, "bells": [{"kind": "interval", "minutes": 5,
                "jitter_pct": 0, "sound_uuid": "s", "enabled": true, "signal_mode": "sound",
                "vibration_pattern_uuid": "p", "volume": 50}]},
            "end_bell": {"enabled": true, "sound_uuid": "s", "signal_mode": "sound",
                "vibration_pattern_uuid": "p", "volume": 50},
            "timing": {"mode": "timer", "stopwatch": false, "duration_secs": 600},
            "cues_signal_mode": "both",
            "keep_screen_awake": false,
            "box_breath_cues": {"master_enabled": false, "phases": []}
        }"#;
        let cfg = PresetConfig::from_json(json).expect("old presets still load");
        let db = Database::open_in_memory().unwrap();
        db.set_setting("timer_auto_finish", "true").unwrap();
        apply(&db, &cfg, SessionMode::Timer).unwrap();
        assert_eq!(db.get_setting("timer_auto_finish", "").unwrap(), "false");
    }

    #[test]
    fn a_hidden_auto_finish_does_not_change_the_active_preset() {
        let db = Database::open_in_memory().unwrap();
        db.set_setting("timer_session_secs", "600").unwrap();
        db.set_setting("timer_stopwatch_active", "true").unwrap();
        let saved = snapshot(&db, SessionMode::Timer, PresetTiming::Timer { stopwatch: true, duration_secs: 600 });
        db.insert_preset("Open", SessionMode::Timer, true, &saved.to_json()).unwrap();
        db.set_setting("timer_auto_finish", "true").unwrap();
        assert_eq!(active_presets(&db, SessionMode::Timer).len(), 1, "hidden while counting up");
        db.set_setting("timer_stopwatch_active", "false").unwrap();
        let saved = snapshot(&db, SessionMode::Timer, PresetTiming::Timer { stopwatch: false, duration_secs: 600 });
        db.insert_preset("Countdown", SessionMode::Timer, true, &saved.to_json()).unwrap();
        db.set_setting("timer_auto_finish", "false").unwrap();
        assert!(active_presets(&db, SessionMode::Timer).is_empty(), "shown for a countdown, it counts");
    }

    #[test]
    fn a_preset_saved_before_bell_volumes_loads_at_the_middle() {
        // config_json as v26.8.5 wrote it: no "volume" anywhere.
        let json = r#"{
            "label": {"enabled": false, "uuid": null},
            "starting_bell": {"enabled": true, "sound_uuid": "s", "prep_time_enabled": false,
                "prep_time_secs": 5, "signal_mode": "sound", "vibration_pattern_uuid": "p"},
            "interval_bells": {"enabled": true, "bells": [{"kind": "interval", "minutes": 5,
                "jitter_pct": 0, "sound_uuid": "s", "enabled": true, "signal_mode": "sound",
                "vibration_pattern_uuid": "p"}]},
            "end_bell": {"enabled": true, "sound_uuid": "s", "signal_mode": "sound",
                "vibration_pattern_uuid": "p"},
            "timing": {"mode": "timer", "stopwatch": false, "duration_secs": 600},
            "cues_signal_mode": "both",
            "keep_screen_awake": false,
            "box_breath_cues": {"master_enabled": true, "phases": [{"phase": "in", "enabled": true,
                "signal_mode": "sound", "sound_uuid": "s", "pattern_uuid": "p"}]}
        }"#;
        let cfg = PresetConfig::from_json(json).expect("old presets still load");
        let middle = BellVolume::default();
        assert_eq!(cfg.starting_bell.volume, middle);
        assert_eq!(cfg.end_bell.volume, middle);
        assert_eq!(cfg.interval_bells.bells[0].volume, middle);
        assert_eq!(cfg.box_breath_cues.phases[0].volume, middle);
        assert!(cfg.interval_bells.bells[0].uuid.is_empty(), "no library row yet: matched by settings");
    }

    #[test]
    fn a_preset_bell_without_seconds_reads_from_minutes() {
        use crate::bells::{BellTime, TimeUnit};
        let mut json: serde_json::Value =
            serde_json::from_str(&with_bells(vec![bell(5, true)]).to_json()).unwrap();
        let old = &mut json["interval_bells"]["bells"][0];
        for key in ["secs", "secs_unit", "jitter_secs", "jitter_unit", "step_secs", "step_unit"] {
            old.as_object_mut().unwrap().remove(key);
        }
        old["jitter_pct"] = 20.into();
        let cfg = PresetConfig::from_json(&json.to_string()).unwrap();
        let b = &cfg.interval_bells.bells[0];
        assert_eq!(
            (b.time, b.jitter, b.step),
            (BellTime::new(5, TimeUnit::Min), BellTime::new(1, TimeUnit::Min), BellTime::ZERO)
        );

        // It matches the library bell it was saved from: no second row.
        let db = Database::open_in_memory().unwrap();
        db.insert_interval_bell(
            IntervalBellKind::Interval, b.time, b.jitter, b.step, "s", "p", SignalMode::Sound,
        )
        .unwrap();
        apply(&db, &cfg, SessionMode::Timer).unwrap();
        assert_eq!(db.list_interval_bells().unwrap().len(), 1);

        // Saved now, it still carries what 26.10.1 reads.
        let saved: serde_json::Value = serde_json::from_str(&cfg.to_json()).unwrap();
        let bell = &saved["interval_bells"]["bells"][0];
        assert_eq!((bell["minutes"].as_u64(), bell["jitter_pct"].as_u64()), (Some(5), Some(20)));
    }

    #[test]
    fn a_bell_field_this_version_does_not_know_is_ignored() {
        // How 26.10.1 and older read a bell's "uuid": same derives, no
        // deny_unknown_fields, so presets saved here still load there.
        let mut json: serde_json::Value = serde_json::from_str(&with_bells(vec![bell(5, true)]).to_json()).unwrap();
        json["interval_bells"]["bells"][0]["from_the_future"] = "x".into();
        let cfg = PresetConfig::from_json(&json.to_string()).expect("unknown keys are skipped");
        assert_eq!(cfg.interval_bells.bells[0].time, BellTime::new(5, TimeUnit::Min));
    }

    #[test]
    fn preset_volumes_round_trip_through_json() {
        let (timer, box_breath) = presets_with_volumes("s", "p");
        for cfg in [timer, box_breath] {
            assert_eq!(PresetConfig::from_json(&cfg.to_json()).unwrap(), cfg);
        }
    }
}

