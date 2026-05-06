//! GTK-side `Database` — a thin wrapper around `meditate_core::db::Database`.
//!
//! All persistence logic lives in core. This module owns:
//! - The GTK-app's domain types (`Session`, `Label`, `SessionData`)
//!   which use i64-unix timestamps and the `note` field name for
//!   ergonomic UI integration. `SessionMode` is re-exported from
//!   core directly — no separate enum.
//! - Type translation at the API boundary (see `session_data_to_core` /
//!   `session_from_core` and `crate::time::unix_to_local_iso`).
//! - The `rusqlite::Result<T>` return type so callers can keep using `?`
//!   against this module without learning core's `DbError`.
//!
//! Core's schema is the on-disk reality; the existing app's old schema
//! is gone (one user, opted into a fresh DB).

use rusqlite::Result;
use std::path::Path;

// ── Models ────────────────────────────────────────────────────────────────────

/// `SessionMode` and `Label` are canonical core types — re-exported
/// here so call sites can keep importing from `crate::db` without
/// learning about meditate-core directly. Same for the bell-library
/// types added in B.3.1, and the `mint_uuid` helper added in B.5.
pub use meditate_core::db::{
    mint_uuid, BellSound, BellSoundCategory, ChartKind, GuidedFile, IntervalBell, IntervalBellKind, Label, Preset, SessionMode, SignalMode, VibrationPattern,
};

/// Bundled bell-sound seed: hardcoded (uuid, display name, GResource
/// path, MIME). UUIDs are STABLE across versions — never edit a row
/// here. New bundles get appended (never replace) so a peer that
/// already seeded the old set picks up the new ones via insert-or-
/// ignore. Adding a sound is a 1-tuple addition; no migration code.
const BUNDLED_BELL_SOUNDS: &[(&str, &str, &str, &str)] = &[
    (
        "f0c2e8a1-3a72-4d4f-9c8b-1b0e5d8c0001",
        "Singing Bowl",
        "/io/github/janekbt/Meditate/sounds/bowl.ogg",
        "audio/ogg",
    ),
    (
        "f0c2e8a1-3a72-4d4f-9c8b-1b0e5d8c0002",
        "Bell",
        "/io/github/janekbt/Meditate/sounds/bell.ogg",
        "audio/ogg",
    ),
    (
        "f0c2e8a1-3a72-4d4f-9c8b-1b0e5d8c0003",
        "Gong",
        "/io/github/janekbt/Meditate/sounds/gong.ogg",
        "audio/ogg",
    ),
    // ── B.* expansion: curated meditation-bell library. All OGG/Vorbis,
    // mono 48 kHz, EBU R128 loudness-normalised to -16 LUFS so the
    // 0.3 s woodblock click and the 30 s bonshō tail land at comparable
    // perceived volume. Sources + license attribution in
    // data/sounds/CREDITS.md.
    (
        "f0c2e8a1-3a72-4d4f-9c8b-1b0e5d8c0004",
        "Tibetan Singing Bowl",
        "/io/github/janekbt/Meditate/sounds/tibetan-bowl-medium.ogg",
        "audio/ogg",
    ),
    (
        "f0c2e8a1-3a72-4d4f-9c8b-1b0e5d8c0005",
        "Inkin",
        "/io/github/janekbt/Meditate/sounds/inkin.ogg",
        "audio/ogg",
    ),
    (
        "f0c2e8a1-3a72-4d4f-9c8b-1b0e5d8c0006",
        "Tingsha",
        "/io/github/janekbt/Meditate/sounds/tingsha.ogg",
        "audio/ogg",
    ),
    (
        "f0c2e8a1-3a72-4d4f-9c8b-1b0e5d8c0007",
        "Kanshō",
        "/io/github/janekbt/Meditate/sounds/kansho.ogg",
        "audio/ogg",
    ),
    (
        "f0c2e8a1-3a72-4d4f-9c8b-1b0e5d8c0008",
        "Burmese Brass Bell",
        "/io/github/janekbt/Meditate/sounds/burmese-brass.ogg",
        "audio/ogg",
    ),
    (
        "f0c2e8a1-3a72-4d4f-9c8b-1b0e5d8c0009",
        "Chau Gong",
        "/io/github/janekbt/Meditate/sounds/chau-gong.ogg",
        "audio/ogg",
    ),
    (
        "f0c2e8a1-3a72-4d4f-9c8b-1b0e5d8c000a",
        "Crystal Bowl",
        "/io/github/janekbt/Meditate/sounds/crystal-bowl.ogg",
        "audio/ogg",
    ),
    (
        "f0c2e8a1-3a72-4d4f-9c8b-1b0e5d8c000b",
        "Woodblock",
        "/io/github/janekbt/Meditate/sounds/woodblock.ogg",
        "audio/ogg",
    ),
];

/// Public so callers (B.4.4 migration site, etc.) can map old
/// "bowl" / "bell" / "gong" string keys to their bundled UUIDs
/// without re-deriving the table here.
pub const BUNDLED_BOWL_UUID: &str = "f0c2e8a1-3a72-4d4f-9c8b-1b0e5d8c0001";
pub const BUNDLED_BELL_UUID: &str = "f0c2e8a1-3a72-4d4f-9c8b-1b0e5d8c0002";
pub const BUNDLED_GONG_UUID: &str = "f0c2e8a1-3a72-4d4f-9c8b-1b0e5d8c0003";

/// Stable UUIDs for the two seeded default labels. The seed runs once
/// on first open (gated by `LABELS_SEEDED_KEY`) and never again — a
/// renamed default still resolves through the UUID, and a *deleted*
/// default stays deleted instead of resurrecting from the next open.
pub const DEFAULT_TIMER_LABEL_UUID: &str = "e2d5a4b8-7c91-4e3f-a826-d40f1c5b9001";
pub const DEFAULT_BREATHING_LABEL_UUID: &str = "e2d5a4b8-7c91-4e3f-a826-d40f1c5b9002";
pub const DEFAULT_GUIDED_LABEL_UUID: &str = "e2d5a4b8-7c91-4e3f-a826-d40f1c5b9003";

/// Seed list mirrors `BUNDLED_BELL_SOUNDS` — uuid + display name.
/// Append-only on UUID; the user can rename or delete the row from
/// the chooser like any other label.
const DEFAULT_LABELS: &[(&str, &str)] = &[
    (DEFAULT_TIMER_LABEL_UUID, "Meditation"),
    (DEFAULT_BREATHING_LABEL_UUID, "Box-Breathing"),
    (DEFAULT_GUIDED_LABEL_UUID, "Guided Meditation"),
];

/// One-shot seed flags in the `settings` table. Set to "1" after the
/// first successful seed; subsequent `open()` calls early-return
/// from the seed function. Without these, a deleted seed row would
/// resurrect on the next open (and re-emit an `*_insert` event that
/// overrides the user's delete on every synced peer).
const LABELS_SEEDED_KEY: &str = "default_labels_seeded";
const BELLS_SEEDED_KEY: &str = "bundled_bell_sounds_seeded";
const PRESETS_SEEDED_KEY: &str = "default_presets_seeded";
const VIBRATION_PATTERNS_SEEDED_KEY: &str = "bundled_vibration_patterns_seeded";

// ── Bundled vibration patterns ─────────────────────────────────────
// Stable hardcoded UUIDs in their own family (separate from the
// bell-sounds family for visual disambiguation in DB inspection) so
// that peers seeded independently end up with the same row identity
// per pattern and don't accumulate duplicates after sync.
pub const BUNDLED_PATTERN_PULSE_UUID:     &str = "7e9c4d2f-5a8b-4f1d-9e3c-2d6f7a8b0001";
pub const BUNDLED_PATTERN_HEARTBEAT_UUID: &str = "7e9c4d2f-5a8b-4f1d-9e3c-2d6f7a8b0002";
pub const BUNDLED_PATTERN_WAVE_UUID:      &str = "7e9c4d2f-5a8b-4f1d-9e3c-2d6f7a8b0003";
pub const BUNDLED_PATTERN_RIPPLE_UUID:    &str = "7e9c4d2f-5a8b-4f1d-9e3c-2d6f7a8b0004";
pub const BUNDLED_PATTERN_PYRAMID_UUID:   &str = "7e9c4d2f-5a8b-4f1d-9e3c-2d6f7a8b0005";

/// Seed list: (uuid, name, duration_ms, intensities, chart_kind).
/// Pulse/Heartbeat/Wave/Ripple are line patterns; Pyramid ships in
/// bar mode to demo the abrupt-step variant out of the box.
const BUNDLED_VIBRATION_PATTERNS: &[(&str, &str, u32, &[f32], ChartKind)] = &[
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

/// Stable UUIDs for the three seeded default presets. Bundled rows
/// have no special property at the schema level — they're regular
/// presets that the user can rename, restar, or delete just like
/// their own. The UUIDs let the one-shot seed know "we already did
/// this" without scanning by name.
pub const DEFAULT_SITTING_PRESET_UUID: &str
    = "b9e1c5a4-2d3f-4d8b-9c70-7a0e1d2c3001";
pub const DEFAULT_BOX_BREATH_4444_UUID: &str
    = "b9e1c5a4-2d3f-4d8b-9c70-7a0e1d2c3002";
pub const DEFAULT_BOX_BREATH_4780_UUID: &str
    = "b9e1c5a4-2d3f-4d8b-9c70-7a0e1d2c3003";

#[derive(Debug, Clone)]
pub struct Session {
    pub id: i64,
    /// Unix timestamp (seconds since epoch) of when the session started.
    pub start_time: i64,
    pub duration_secs: i64,
    pub mode: SessionMode,
    pub label_id: Option<i64>,
    pub note: Option<String>,
    /// Set on guided meditation rows that played a library-stored
    /// file. None for non-Guided modes and transient one-off plays.
    pub guided_file_uuid: Option<String>,
}

/// Parameters for creating or updating a session.
pub struct SessionData {
    pub start_time: i64,
    pub duration_secs: i64,
    pub mode: SessionMode,
    pub label_id: Option<i64>,
    pub note: Option<String>,
    pub guided_file_uuid: Option<String>,
}

/// `SessionFilter` is the canonical core type — re-exported here so
/// call sites can import from `crate::db` without learning about
/// meditate-core directly.
pub use meditate_core::db::SessionFilter;

// ── Translation: GTK-side ↔ meditate_core::db ─────────────────────────────────

/// Convert this app's `SessionData` (i64 unix, `note`) into core's
/// insert shape (ISO 8601 string, `notes`). Negative or overflowing
/// durations clamp to the u32 range.
fn session_data_to_core(s: &SessionData) -> meditate_core::db::Session {
    meditate_core::db::Session {
        start_iso: crate::time::unix_to_local_iso(s.start_time),
        duration_secs: s.duration_secs.clamp(0, u32::MAX as i64) as u32,
        label_id: s.label_id,
        notes: s.note.clone(),
        mode: s.mode,
        // Empty placeholder — core's `insert_session` overwrites this
        // with a freshly generated v4 uuid. Read paths see the real one.
        uuid: String::new(),
        guided_file_uuid: s.guided_file_uuid.clone(),
    }
}

/// Inverse of `session_data_to_core` for retrievals: takes core's
/// `(id, Session)` shape and produces the GTK-side `Session` with
/// embedded id and i64-unix `start_time`.
fn session_from_core(id: i64, core: &meditate_core::db::Session) -> Session {
    Session {
        id,
        start_time: crate::time::local_iso_to_unix(&core.start_iso),
        duration_secs: core.duration_secs as i64,
        mode: core.mode,
        label_id: core.label_id,
        note: core.notes.clone(),
        guided_file_uuid: core.guided_file_uuid.clone(),
    }
}

/// Map core's structured error to a `rusqlite::Error` so the GTK side
/// can keep its `Result = rusqlite::Result` alias. `DuplicateLabel`
/// becomes a synthesized UNIQUE-constraint failure, matching what
/// callers used to see when the GTK app talked to rusqlite directly.
fn map_core_err(e: meditate_core::db::DbError) -> rusqlite::Error {
    use meditate_core::db::DbError;
    match e {
        DbError::Sqlite(err) => err,
        DbError::DuplicateLabel(name) => rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error {
                code: rusqlite::ErrorCode::ConstraintViolation,
                extended_code: rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE,
            },
            Some(format!("UNIQUE constraint failed: labels.name (\"{name}\")")),
        ),
        DbError::DuplicatePreset(name) => rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error {
                code: rusqlite::ErrorCode::ConstraintViolation,
                extended_code: rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE,
            },
            Some(format!("UNIQUE constraint failed: presets.name (\"{name}\")")),
        ),
        DbError::DuplicateGuidedFile(name) => rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error {
                code: rusqlite::ErrorCode::ConstraintViolation,
                extended_code: rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE,
            },
            Some(format!("UNIQUE constraint failed: guided_files.name (\"{name}\")")),
        ),
        DbError::DuplicateVibrationPattern(name) => rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error {
                code: rusqlite::ErrorCode::ConstraintViolation,
                extended_code: rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE,
            },
            Some(format!("UNIQUE constraint failed: vibration_patterns.name (\"{name}\")")),
        ),
        DbError::Csv(s) => rusqlite::Error::ToSqlConversionFailure(Box::new(
            std::io::Error::new(std::io::ErrorKind::InvalidData, s),
        )),
    }
}

/// Today as a `chrono::NaiveDate` in the user's local timezone — used
/// for streak / running-average calculations that need a concrete
/// "today" boundary.
fn today_local_naive_date() -> chrono::NaiveDate {
    chrono::Local::now().date_naive()
}

// ── Database ──────────────────────────────────────────────────────────────────

pub struct Database {
    inner: meditate_core::db::Database,
}

impl std::fmt::Debug for Database {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Database").finish_non_exhaustive()
    }
}

impl Database {
    /// Open (or create) the database at `path`. Schema is core's; any
    /// pre-existing DB file written by an older version of this app
    /// will need to be deleted first (the user opted into that on
    /// 2026-04-29 — single-user repo).
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        }
        let inner = meditate_core::db::Database::open(path).map_err(map_core_err)?;
        let db = Self { inner };
        db.seed_bundled_bell_sounds()?;
        db.seed_default_labels()?;
        db.seed_default_presets()?;
        db.seed_bundled_vibration_patterns()?;
        Ok(db)
    }

    /// Insert the bundled bell-sound rows on first run, gated by a
    /// one-shot `bundled_bell_sounds_seeded` settings flag so a user
    /// who deletes a bundled row can't have it resurrect on the next
    /// open. (Without the flag, a deletion frees the seed UUID,
    /// `insert_bell_sound_with_uuid` re-INSERTs and emits a fresh
    /// `bell_sound_insert` event with a newer lamport ts, which on
    /// sync overrides the user's `bell_sound_delete` everywhere.)
    /// UUIDs are still hardcoded so every device — fresh seed or
    /// post-sync — converges on the same row identity per file.
    fn seed_bundled_bell_sounds(&self) -> Result<()> {
        if self.inner.get_setting(BELLS_SEEDED_KEY, "0").map_err(map_core_err)? == "1" {
            return Ok(());
        }
        for (uuid, name, path, mime) in BUNDLED_BELL_SOUNDS {
            // Bundled rows ship as `general` — bells, gongs, chimes
            // for the Starting / Interval / End bell choosers. Box
            // Breath voice cues land via separate seeds when those
            // ship (TODO entry: "Source bundled Box Breath voice-cue
            // sounds").
            self.inner
                .insert_bell_sound_with_uuid(
                    uuid, name, path, true, mime, BellSoundCategory::General,
                )
                .map_err(map_core_err)?;
        }
        self.inner.set_setting(BELLS_SEEDED_KEY, "1").map_err(map_core_err)?;
        Ok(())
    }

    /// Seed the five bundled vibration patterns (Pulse / Heartbeat /
    /// Wave / Ripple / Pyramid) under stable UUIDs. Same one-shot
    /// flag pattern as the other seeds — a deleted bundled pattern
    /// stays deleted instead of resurrecting on the next open.
    fn seed_bundled_vibration_patterns(&self) -> Result<()> {
        if self
            .inner
            .get_setting(VIBRATION_PATTERNS_SEEDED_KEY, "0")
            .map_err(map_core_err)?
            == "1"
        {
            return Ok(());
        }
        for &(uuid, name, duration_ms, intensities, chart_kind) in BUNDLED_VIBRATION_PATTERNS {
            self.inner
                .insert_vibration_pattern_with_uuid(
                    uuid, name, duration_ms, intensities, chart_kind, true,
                )
                .map_err(map_core_err)?;
        }
        self.inner
            .set_setting(VIBRATION_PATTERNS_SEEDED_KEY, "1")
            .map_err(map_core_err)?;
        Ok(())
    }

    /// Seed the two default labels ("Meditation", "Box-Breathing")
    /// with stable UUIDs, gated by a one-shot `default_labels_seeded`
    /// settings flag. Same resurrect-bug rationale as the bell-sound
    /// seed: deleting a seed label leaves its UUID free, and a
    /// re-seed without the flag would emit a fresh `label_insert`
    /// event that overrides the user's deletion via sync. A
    /// `DuplicateLabel` error (user already has a row with this
    /// *name* under a different uuid) is silently swallowed so we
    /// don't shadow user-managed rows.
    fn seed_default_labels(&self) -> Result<()> {
        if self.inner.get_setting(LABELS_SEEDED_KEY, "0").map_err(map_core_err)? == "1" {
            return Ok(());
        }
        for (uuid, name) in DEFAULT_LABELS {
            match self.inner.insert_label_with_uuid(uuid, name) {
                Ok(_) => {}
                Err(meditate_core::db::DbError::DuplicateLabel(_)) => {}
                Err(e) => return Err(map_core_err(e)),
            }
        }
        self.inner.set_setting(LABELS_SEEDED_KEY, "1").map_err(map_core_err)?;
        Ok(())
    }

    /// Seed the three bundled presets — one Timer ("Sitting") plus two
    /// Box-Breath patterns (4-4-4-4 and 4-7-8-0) — under stable UUIDs,
    /// all starred so they show in the home-screen chip list on first
    /// run. Mode-strict separation means the user always sees one of
    /// each kind regardless of which mode they start the app in. Per
    /// the design call (2026-05-04), these are *regular* presets:
    /// fully renamable / destarable / deletable, no special property
    /// at the schema level. Same one-shot flag pattern as the labels
    /// and bell-sounds seeds — deletion does not resurrect on the next
    /// open.
    fn seed_default_presets(&self) -> Result<()> {
        if self.inner.get_setting(PRESETS_SEEDED_KEY, "0").map_err(map_core_err)? == "1" {
            return Ok(());
        }

        use crate::preset_config::*;
        let sitting = PresetConfig {
            label: PresetLabel {
                enabled: true,
                uuid: Some(DEFAULT_TIMER_LABEL_UUID.to_string()),
            },
            starting_bell: PresetStartingBell {
                enabled: true,
                sound_uuid: BUNDLED_BOWL_UUID.to_string(),
                prep_time_enabled: false,
                prep_time_secs: 5,
            },
            interval_bells: PresetIntervalBells::default(),
            end_bell: PresetEndBell {
                enabled: true,
                sound_uuid: BUNDLED_BELL_UUID.to_string(),
            },
            timing: PresetTiming::Timer { stopwatch: false, duration_secs: 15 * 60 },
        };
        let box_4444 = PresetConfig {
            label: PresetLabel {
                enabled: true,
                uuid: Some(DEFAULT_BREATHING_LABEL_UUID.to_string()),
            },
            starting_bell: PresetStartingBell::default(),
            interval_bells: PresetIntervalBells::default(),
            end_bell: PresetEndBell {
                enabled: true,
                sound_uuid: BUNDLED_BELL_UUID.to_string(),
            },
            timing: PresetTiming::BoxBreath {
                inhale_secs: 4,
                hold_full_secs: 4,
                exhale_secs: 4,
                hold_empty_secs: 4,
                duration_secs: 10 * 60,
            },
        };
        let box_4780 = PresetConfig {
            timing: PresetTiming::BoxBreath {
                inhale_secs: 4,
                hold_full_secs: 7,
                exhale_secs: 8,
                hold_empty_secs: 0,
                duration_secs: 10 * 60,
            },
            ..box_4444.clone()
        };

        let seeds: &[(&str, &str, SessionMode, &PresetConfig)] = &[
            (DEFAULT_SITTING_PRESET_UUID, "Sitting", SessionMode::Timer, &sitting),
            (DEFAULT_BOX_BREATH_4444_UUID, "Box Breath 4-4-4-4",
                SessionMode::BoxBreath, &box_4444),
            (DEFAULT_BOX_BREATH_4780_UUID, "Box Breath 4-7-8-0",
                SessionMode::BoxBreath, &box_4780),
        ];

        for (uuid, name, mode, cfg) in seeds {
            match self.inner.insert_preset_with_uuid(
                uuid, name, *mode, true, &cfg.to_json(),
            ) {
                Ok(_) => {}
                Err(meditate_core::db::DbError::DuplicatePreset(_)) => {}
                Err(e) => return Err(map_core_err(e)),
            }
        }
        self.inner.set_setting(PRESETS_SEEDED_KEY, "1").map_err(map_core_err)?;
        Ok(())
    }

    // ── Labels ────────────────────────────────────────────────────────────────

    /// Insert a label with the exact name supplied. Returns a UNIQUE
    /// constraint error on collision (case-insensitive — column is
    /// COLLATE NOCASE). The UI is expected to pre-validate via
    /// `is_label_name_taken` so this error is a safety net, not the
    /// primary collision path.
    pub fn create_label(&self, name: &str) -> Result<Label> {
        let id = self.inner.insert_label(name).map_err(map_core_err)?;
        // Look the row back up so we return the DB-assigned uuid rather
        // than synthesising a half-populated Label. `list_labels` is the
        // only existing read path; it's O(n) over labels but n stays
        // small (~tens) and label creation isn't a hot path.
        self.inner
            .list_labels()
            .map_err(map_core_err)?
            .into_iter()
            .find(|l| l.id == id)
            .ok_or(rusqlite::Error::QueryReturnedNoRows)
    }

    pub fn list_labels(&self) -> Result<Vec<Label>> {
        self.inner.list_labels().map_err(map_core_err)
    }

    /// True iff any label other than `except_id` already uses `name`
    /// (case-insensitive — the column is COLLATE NOCASE).
    pub fn is_label_name_taken(&self, name: &str, except_id: i64) -> Result<bool> {
        self.inner.is_label_name_taken(name, except_id).map_err(map_core_err)
    }

    pub fn update_label(&self, id: i64, name: &str) -> Result<()> {
        self.inner.update_label(id, name).map_err(map_core_err)
    }

    pub fn label_session_count(&self, id: i64) -> Result<i64> {
        self.inner.label_session_count(id).map_err(map_core_err)
    }

    pub fn delete_label(&self, id: i64) -> Result<()> {
        self.inner.delete_label(id).map_err(map_core_err)
    }

    // ── Interval bells ────────────────────────────────────────────────────────
    // Thin pass-throughs onto core's CRUD. Domain types are re-exported
    // from core verbatim — no shell-side translation needed.

    pub fn list_interval_bells(&self) -> Result<Vec<meditate_core::db::IntervalBell>> {
        self.inner.list_interval_bells().map_err(map_core_err)
    }

    pub fn insert_interval_bell(
        &self,
        kind: meditate_core::db::IntervalBellKind,
        minutes: u32,
        jitter_pct: u32,
        sound: &str,
        vibration_pattern_uuid: &str,
        signal_mode: meditate_core::db::SignalMode,
    ) -> Result<i64> {
        self.inner
            .insert_interval_bell(
                kind, minutes, jitter_pct, sound,
                vibration_pattern_uuid, signal_mode,
            )
            .map_err(map_core_err)
    }

    pub fn update_interval_bell(
        &self,
        uuid: &str,
        kind: meditate_core::db::IntervalBellKind,
        minutes: u32,
        jitter_pct: u32,
        sound: &str,
        vibration_pattern_uuid: &str,
        signal_mode: meditate_core::db::SignalMode,
        enabled: bool,
    ) -> Result<()> {
        self.inner
            .update_interval_bell(
                uuid, kind, minutes, jitter_pct, sound,
                vibration_pattern_uuid, signal_mode, enabled,
            )
            .map_err(map_core_err)
    }

    pub fn set_interval_bell_enabled(&self, uuid: &str, enabled: bool) -> Result<()> {
        self.inner
            .set_interval_bell_enabled(uuid, enabled)
            .map_err(map_core_err)
    }

    pub fn delete_interval_bell(&self, uuid: &str) -> Result<()> {
        self.inner.delete_interval_bell(uuid).map_err(map_core_err)
    }

    // ── Bell sounds ───────────────────────────────────────────────────────────
    // Audio-file library shared by every bell-fire site. Pass-through
    // wrappers; domain types are re-exported from core.

    pub fn list_bell_sounds(&self) -> Result<Vec<BellSound>> {
        self.inner.list_bell_sounds().map_err(map_core_err)
    }

    pub fn list_bell_sounds_for_category(
        &self,
        category: BellSoundCategory,
    ) -> Result<Vec<BellSound>> {
        self.inner.list_bell_sounds_for_category(category).map_err(map_core_err)
    }

    pub fn insert_bell_sound(
        &self,
        name: &str,
        file_path: &str,
        is_bundled: bool,
        mime_type: &str,
        category: BellSoundCategory,
    ) -> Result<i64> {
        self.inner
            .insert_bell_sound(name, file_path, is_bundled, mime_type, category)
            .map_err(map_core_err)
    }

    pub fn insert_bell_sound_with_uuid(
        &self,
        uuid: &str,
        name: &str,
        file_path: &str,
        is_bundled: bool,
        mime_type: &str,
        category: BellSoundCategory,
    ) -> Result<i64> {
        self.inner
            .insert_bell_sound_with_uuid(uuid, name, file_path, is_bundled, mime_type, category)
            .map_err(map_core_err)
    }

    pub fn rename_bell_sound(&self, uuid: &str, name: &str) -> Result<()> {
        self.inner.rename_bell_sound(uuid, name).map_err(map_core_err)
    }

    pub fn delete_bell_sound(&self, uuid: &str) -> Result<()> {
        self.inner.delete_bell_sound(uuid).map_err(map_core_err)
    }

    // ── Presets ───────────────────────────────────────────────────────────────
    // Thin pass-throughs onto core's CRUD. The chooser UI (P.4) reaches
    // for these via the GTK shell's Database wrapper.

    pub fn list_presets(&self) -> Result<Vec<Preset>> {
        self.inner.list_presets().map_err(map_core_err)
    }

    pub fn list_presets_for_mode(&self, mode: SessionMode) -> Result<Vec<Preset>> {
        self.inner.list_presets_for_mode(mode).map_err(map_core_err)
    }

    pub fn list_starred_presets_for_mode(&self, mode: SessionMode) -> Result<Vec<Preset>> {
        self.inner.list_starred_presets_for_mode(mode).map_err(map_core_err)
    }

    pub fn insert_preset(
        &self,
        name: &str,
        mode: SessionMode,
        is_starred: bool,
        config_json: &str,
    ) -> Result<i64> {
        self.inner
            .insert_preset(name, mode, is_starred, config_json)
            .map_err(map_core_err)
    }

    pub fn insert_preset_with_uuid(
        &self,
        uuid: &str,
        name: &str,
        mode: SessionMode,
        is_starred: bool,
        config_json: &str,
    ) -> Result<i64> {
        self.inner
            .insert_preset_with_uuid(uuid, name, mode, is_starred, config_json)
            .map_err(map_core_err)
    }

    pub fn is_preset_name_taken(&self, name: &str, except_uuid: &str) -> Result<bool> {
        self.inner.is_preset_name_taken(name, except_uuid).map_err(map_core_err)
    }

    pub fn find_preset_by_uuid(&self, uuid: &str) -> Result<Option<Preset>> {
        self.inner.find_preset_by_uuid(uuid).map_err(map_core_err)
    }

    pub fn update_preset_name(&self, uuid: &str, name: &str) -> Result<()> {
        self.inner.update_preset_name(uuid, name).map_err(map_core_err)
    }

    pub fn update_preset_config(&self, uuid: &str, config_json: &str) -> Result<()> {
        self.inner.update_preset_config(uuid, config_json).map_err(map_core_err)
    }

    pub fn update_preset_starred(&self, uuid: &str, is_starred: bool) -> Result<()> {
        self.inner.update_preset_starred(uuid, is_starred).map_err(map_core_err)
    }

    pub fn delete_preset(&self, uuid: &str) -> Result<()> {
        self.inner.delete_preset(uuid).map_err(map_core_err)
    }

    // ── GuidedFiles — pass-through wrappers ───────────────────────────────────
    // Thin shells over the core CRUD; same Result = rusqlite::Result
    // shape as everything else here (DuplicateGuidedFile maps to a
    // synthesized UNIQUE-constraint failure via map_core_err).

    pub fn list_guided_files(&self) -> Result<Vec<GuidedFile>> {
        self.inner.list_guided_files().map_err(map_core_err)
    }

    pub fn insert_guided_file_with_uuid(
        &self,
        uuid: &str,
        name: &str,
        file_path: &str,
        duration_secs: u32,
        is_starred: bool,
    ) -> Result<i64> {
        self.inner
            .insert_guided_file_with_uuid(uuid, name, file_path, duration_secs, is_starred)
            .map_err(map_core_err)
    }

    pub fn find_guided_file_by_uuid(&self, uuid: &str) -> Result<Option<GuidedFile>> {
        self.inner.find_guided_file_by_uuid(uuid).map_err(map_core_err)
    }

    pub fn is_guided_file_name_taken(&self, name: &str, except_uuid: &str) -> Result<bool> {
        self.inner.is_guided_file_name_taken(name, except_uuid).map_err(map_core_err)
    }

    pub fn rename_guided_file(&self, uuid: &str, name: &str) -> Result<()> {
        self.inner.rename_guided_file(uuid, name).map_err(map_core_err)
    }

    pub fn set_guided_file_starred(&self, uuid: &str, is_starred: bool) -> Result<()> {
        self.inner.set_guided_file_starred(uuid, is_starred).map_err(map_core_err)
    }

    pub fn delete_guided_file(&self, uuid: &str) -> Result<()> {
        self.inner.delete_guided_file(uuid).map_err(map_core_err)
    }

    // ── VibrationPatterns — pass-through wrappers ─────────────────────────────
    // Thin shells over the core CRUD. DuplicateVibrationPattern maps
    // to a synthesized UNIQUE-constraint failure via map_core_err so
    // callers can treat the duplicate-name path uniformly with the
    // other UNIQUE-NOCASE name fields (labels, presets, guided files).

    pub fn list_vibration_patterns(&self) -> Result<Vec<VibrationPattern>> {
        self.inner.list_vibration_patterns().map_err(map_core_err)
    }

    pub fn find_vibration_pattern_by_uuid(
        &self, uuid: &str,
    ) -> Result<Option<VibrationPattern>> {
        self.inner.find_vibration_pattern_by_uuid(uuid).map_err(map_core_err)
    }

    pub fn insert_vibration_pattern(
        &self,
        name: &str,
        duration_ms: u32,
        intensities: &[f32],
        chart_kind: ChartKind,
        is_bundled: bool,
    ) -> Result<String> {
        self.inner
            .insert_vibration_pattern(name, duration_ms, intensities, chart_kind, is_bundled)
            .map_err(map_core_err)
    }

    pub fn insert_vibration_pattern_with_uuid(
        &self,
        uuid: &str,
        name: &str,
        duration_ms: u32,
        intensities: &[f32],
        chart_kind: ChartKind,
        is_bundled: bool,
    ) -> Result<i64> {
        self.inner
            .insert_vibration_pattern_with_uuid(
                uuid, name, duration_ms, intensities, chart_kind, is_bundled,
            )
            .map_err(map_core_err)
    }

    pub fn update_vibration_pattern(
        &self,
        uuid: &str,
        name: &str,
        duration_ms: u32,
        intensities: &[f32],
        chart_kind: ChartKind,
    ) -> Result<()> {
        self.inner
            .update_vibration_pattern(uuid, name, duration_ms, intensities, chart_kind)
            .map_err(map_core_err)
    }

    pub fn delete_vibration_pattern(&self, uuid: &str) -> Result<()> {
        self.inner.delete_vibration_pattern(uuid).map_err(map_core_err)
    }

    pub fn is_vibration_pattern_name_taken(
        &self, name: &str, except_uuid: &str,
    ) -> Result<bool> {
        self.inner
            .is_vibration_pattern_name_taken(name, except_uuid)
            .map_err(map_core_err)
    }

    // ── Sessions ──────────────────────────────────────────────────────────────

    pub fn create_session(&self, data: &SessionData) -> Result<Session> {
        let core = session_data_to_core(data);
        let id = self.inner.insert_session(&core).map_err(map_core_err)?;
        Ok(session_from_core(id, &core))
    }

    /// Insert many sessions inside a single core-side transaction.
    /// Atomic on error: a constraint violation rolls back the whole
    /// batch (see core's `bulk_insert_sessions` tests).
    pub fn bulk_insert_sessions(&self, sessions: &[SessionData]) -> Result<usize> {
        let core_rows: Vec<_> = sessions.iter().map(session_data_to_core).collect();
        self.inner.bulk_insert_sessions(&core_rows).map_err(map_core_err)
    }

    pub fn delete_all_sessions(&self) -> Result<usize> {
        self.inner.delete_all_sessions().map_err(map_core_err)
    }

    pub fn find_or_create_label(&self, name: &str) -> Result<i64> {
        self.inner.find_or_create_label(name).map_err(map_core_err)
    }

    pub fn list_sessions(&self, filter: &SessionFilter) -> Result<Vec<Session>> {
        let rows = self.inner.query_sessions(filter).map_err(map_core_err)?;
        Ok(rows.into_iter().map(|(id, c)| session_from_core(id, &c)).collect())
    }

    pub fn update_session(&self, id: i64, data: &SessionData) -> Result<()> {
        self.inner.update_session(id, &session_data_to_core(data)).map_err(map_core_err)
    }

    pub fn delete_session(&self, id: i64) -> Result<()> {
        self.inner.delete_session(id).map_err(map_core_err)
    }

    // ── Settings ──────────────────────────────────────────────────────────────

    pub fn get_setting(&self, key: &str, default: &str) -> Result<String> {
        self.inner.get_setting(key, default).map_err(map_core_err)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.inner.set_setting(key, value).map_err(map_core_err)
    }

    /// Read a sync-loop bookkeeping value (Nextcloud URL, username,
    /// last-sync timestamp, …). Separate namespace from `settings` so
    /// user-facing prefs and sync internals don't collide on a key.
    pub fn get_sync_state(&self, key: &str, default: &str) -> Result<String> {
        self.inner.get_sync_state(key, default).map_err(map_core_err)
    }

    /// Upsert a sync-loop bookkeeping value.
    pub fn set_sync_state(&self, key: &str, value: &str) -> Result<()> {
        self.inner.set_sync_state(key, value).map_err(map_core_err)
    }

    /// All remote file_uuids this device has ingested or pushed.
    /// Used by the remote-data-lost detection to recognise our own
    /// previous batches in the remote listing.
    pub fn known_remote_file_uuids(&self)
        -> Result<std::collections::HashSet<String>>
    {
        self.inner.known_remote_file_uuids().map_err(map_core_err)
    }

    /// Record a remote batch_uuid as ingested. Idempotent.
    pub fn record_known_remote_file(&self, file_uuid: &str) -> Result<()> {
        self.inner.record_known_remote_file(file_uuid).map_err(map_core_err)
    }

    /// Drop every recorded remote file_uuid. Called on account swap
    /// (URL or username change) and on the "push local up" recovery
    /// path after a remote-data-lost prompt.
    pub fn wipe_known_remote_files(&self) -> Result<()> {
        self.inner.wipe_known_remote_files().map_err(map_core_err)
    }

    // ── known_remote_sounds (B.6) ─────────────────────────────────────────────
    // Same lifecycle shape as known_remote_files but keyed on
    // bell_sounds.uuid for the audio-file sync layer.

    pub fn known_remote_sound_uuids(&self)
        -> Result<std::collections::HashSet<String>>
    {
        self.inner.known_remote_sound_uuids().map_err(map_core_err)
    }

    pub fn record_known_remote_sound(&self, bell_uuid: &str) -> Result<()> {
        self.inner.record_known_remote_sound(bell_uuid).map_err(map_core_err)
    }

    pub fn wipe_known_remote_sounds(&self) -> Result<()> {
        self.inner.wipe_known_remote_sounds().map_err(map_core_err)
    }

    /// Reset every event row's synced flag to 0, putting all of them
    /// back into pending. Used by the "push local up" recovery path.
    pub fn flag_all_events_unsynced(&self) -> Result<()> {
        self.inner.flag_all_events_unsynced().map_err(map_core_err)
    }

    /// Erase every user-content row plus the dedup tracker — events,
    /// sessions, labels, known_remote_files. Settings, sync_state,
    /// and device identity survive. Used by the "wipe local to match
    /// remote" recovery path.
    pub fn wipe_local_event_log(&self) -> Result<()> {
        self.inner.wipe_local_event_log().map_err(map_core_err)
    }

    /// How many events are currently pending push. Mostly a test-
    /// observability helper, but useful for any caller that wants to
    /// know "is there local work to sync?" without listing the full
    /// pending vector.
    pub fn pending_events_count(&self) -> Result<usize> {
        Ok(self.inner.pending_events().map_err(map_core_err)?.len())
    }

    // ── Stats queries ─────────────────────────────────────────────────────────

    /// Current streak of consecutive calendar days (ending today or
    /// yesterday) with at least one session. "Today" is computed in the
    /// user's local timezone here, then handed to core.
    pub fn get_streak(&self) -> Result<u32> {
        let today = today_local_naive_date();
        let n = self.inner.get_streak(today).map_err(map_core_err)?;
        Ok(n.max(0) as u32)
    }

    pub fn get_best_streak(&self) -> Result<u32> {
        let n = self.inner.get_best_streak().map_err(map_core_err)?;
        Ok(n.max(0) as u32)
    }

    pub fn total_seconds(&self) -> Result<i64> {
        self.inner.total_seconds().map_err(map_core_err)
    }

    /// Average daily duration over the last `days` days. Days with no
    /// sessions count as zero. Returns 0 for `days == 0` (guards against
    /// the underflow `days - 1` would cause).
    pub fn get_running_average_secs(&self, days: u32) -> Result<f64> {
        if days == 0 {
            return Ok(0.0);
        }
        let since = today_local_naive_date() - chrono::Duration::days((days - 1) as i64);
        let total = self.inner.total_secs_since(since).map_err(map_core_err)?;
        Ok(total as f64 / days as f64)
    }

    /// `(local-date "YYYY-MM-DD", total_secs)` for each day on or after
    /// `since_date`. Filters core's full daily-totals list in Rust;
    /// fine for the typical 30–90 day window the heatmap asks for.
    pub fn get_daily_totals(&self, since_date: &str) -> Result<Vec<(String, i64)>> {
        let since = chrono::NaiveDate::parse_from_str(since_date, "%Y-%m-%d")
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        let totals = self.inner.get_daily_totals().map_err(map_core_err)?;
        Ok(totals
            .into_iter()
            .filter(|(d, _)| *d >= since)
            .map(|(d, secs)| (d.format("%Y-%m-%d").to_string(), secs))
            .collect())
    }

    pub fn get_total_secs_since(&self, since_date: &str) -> Result<i64> {
        let since = chrono::NaiveDate::parse_from_str(since_date, "%Y-%m-%d")
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        self.inner.total_secs_since(since).map_err(map_core_err)
    }

    pub fn active_months(&self) -> Result<Vec<(i32, u32)>> {
        self.inner.active_months().map_err(map_core_err)
    }

    pub fn active_days_in_month(&self, year: i32, month: u32) -> Result<Vec<u32>> {
        self.inner.active_days_in_month(year, month).map_err(map_core_err)
    }

    pub fn count_sessions(&self) -> Result<i64> {
        self.inner.count_sessions().map_err(map_core_err)
    }

    /// Longest single session as `(duration_secs, start_time_unix)`,
    /// None on empty DB. The shape is a tuple for backward compat with
    /// existing UI sites; core returns the full Session.
    pub fn get_longest_session(&self) -> Result<Option<(i64, i64)>> {
        let row = self.inner.get_longest_session().map_err(map_core_err)?;
        Ok(row.map(|(_id, c)| {
            (c.duration_secs as i64, crate::time::local_iso_to_unix(&c.start_iso))
        }))
    }

    /// Median session duration, None on empty DB. Core's variant
    /// returns 0 for empty (lossy for the UI's "n/a" display); the
    /// wrapper checks `count_sessions` first to recover the None case.
    pub fn get_median_duration_secs(&self) -> Result<Option<i64>> {
        if self.inner.count_sessions().map_err(map_core_err)? == 0 {
            return Ok(None);
        }
        let secs = self.inner.get_median_duration_secs().map_err(map_core_err)?;
        Ok(Some(secs as i64))
    }

    pub fn hour_buckets(&self) -> Result<(i64, i64, i64)> {
        self.inner.hour_buckets().map_err(map_core_err)
    }

    pub fn get_label_totals(&self) -> Result<Vec<(String, i64, i64)>> {
        self.inner.label_totals_seconds().map_err(map_core_err)
    }

    pub fn month_total_secs(&self, year: i32, month: u32) -> Result<i64> {
        self.inner.month_total_secs(year, month).map_err(map_core_err)
    }
}

/// In-memory DB for tests. Module-level so sibling files (e.g.
/// `data_io`) can construct a `Database` without needing a path.
/// Seeds the bundled bell sounds the same way `open()` does so tests
/// see post-seed state by default.
#[cfg(test)]
pub(crate) fn test_db_in_memory() -> Database {
    let db = Database {
        inner: meditate_core::db::Database::open_in_memory().unwrap(),
    };
    db.seed_bundled_bell_sounds().unwrap();
    db.seed_default_labels().unwrap();
    db.seed_default_presets().unwrap();
    db.seed_bundled_vibration_patterns().unwrap();
    db
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── core::Session translation helpers ─────────────────────────────────────

    #[test]
    fn session_data_to_core_preserves_every_field() {
        let sd = SessionData {
            start_time: 1_700_000_000,
            duration_secs: 1234,
            mode: SessionMode::BoxBreath,
            label_id: Some(42),
            note: Some("hello".to_string()),
            guided_file_uuid: None,
        };
        let core = session_data_to_core(&sd);
        assert_eq!(crate::time::local_iso_to_unix(&core.start_iso), 1_700_000_000);
        assert_eq!(core.duration_secs, 1234);
        assert_eq!(core.label_id, Some(42));
        assert_eq!(core.notes, Some("hello".to_string()));
        assert!(matches!(core.mode, meditate_core::db::SessionMode::BoxBreath));
    }

    #[test]
    fn session_data_to_core_clamps_negative_duration_to_zero() {
        let sd = SessionData {
            start_time: 1_700_000_000,
            duration_secs: -1,
            mode: SessionMode::Timer,
            label_id: None,
            note: None,
            guided_file_uuid: None,
        };
        assert_eq!(session_data_to_core(&sd).duration_secs, 0);
    }

    #[test]
    fn session_data_to_core_clamps_overflowing_duration_to_u32_max() {
        let sd = SessionData {
            start_time: 0,
            duration_secs: i64::MAX,
            mode: SessionMode::Timer,
            label_id: None,
            note: None,
            guided_file_uuid: None,
        };
        assert_eq!(session_data_to_core(&sd).duration_secs, u32::MAX);
    }

    #[test]
    fn session_from_core_preserves_every_field() {
        let core = meditate_core::db::Session {
            start_iso: crate::time::unix_to_local_iso(1_700_000_000),
            duration_secs: 600,
            label_id: Some(7),
            notes: Some("from core".to_string()),
            mode: meditate_core::db::SessionMode::BoxBreath,
            // Translation from core → shell drops the uuid (the GTK-side
            // Session doesn't carry one). This test pins the rest of the
            // mapping; uuid round-trip is covered in core's tests.
            uuid: "ignored-by-shell".to_string(),
            guided_file_uuid: None,
        };
        let s = session_from_core(99, &core);
        assert_eq!(s.id, 99);
        assert_eq!(s.start_time, 1_700_000_000);
        assert_eq!(s.duration_secs, 600);
        assert_eq!(s.label_id, Some(7));
        assert_eq!(s.note, Some("from core".to_string()));
        assert_eq!(s.mode, SessionMode::BoxBreath);
    }

    #[test]
    fn session_data_round_trips_through_core_and_back() {
        let original = SessionData {
            start_time: 1_700_000_000,
            duration_secs: 750,
            mode: SessionMode::Timer,
            label_id: Some(11),
            note: Some("noted".to_string()),
            guided_file_uuid: None,
        };
        let core = session_data_to_core(&original);
        let restored = session_from_core(123, &core);
        assert_eq!(restored.id, 123);
        assert_eq!(restored.start_time, original.start_time);
        assert_eq!(restored.duration_secs, original.duration_secs);
        assert_eq!(restored.mode, original.mode);
        assert_eq!(restored.label_id, original.label_id);
        assert_eq!(restored.note, original.note);
    }

    // ── Tier B: in-memory integration tests against the wrapper ──────────────

    fn fresh_db() -> Database { super::test_db_in_memory() }

    /// Unix timestamp at `hh:mm` local time, `days_ago` local-days back.
    /// Computed via chrono::Local — matches the wrapper's own
    /// `today_local_naive_date` so streak / running-average tests share
    /// the same "today" boundary.
    fn local_ts(days_ago: i64, hh: u32, mm: u32) -> i64 {
        use chrono::TimeZone;
        let date = chrono::Local::now().date_naive() - chrono::Duration::days(days_ago);
        let datetime = date.and_hms_opt(hh, mm, 0).unwrap();
        chrono::Local
            .from_local_datetime(&datetime)
            .single()
            .unwrap()
            .timestamp()
    }

    fn seed_session(db: &Database, start_time: i64, duration_secs: i64, label_id: Option<i64>) {
        db.create_session(&SessionData {
            start_time,
            duration_secs,
            mode: SessionMode::Timer,
            label_id,
            note: None,
            guided_file_uuid: None,
        }).unwrap();
    }

    // ── create_label collision behaviour ──────────────────────────────────────

    #[test]
    fn create_label_returns_unique_constraint_error_on_collision() {
        // create_label does NOT auto-rename. The UI pre-validates with
        // is_label_name_taken and shows a "label already exists" toast
        // before even calling this; the error here is the DB safety net
        // for any code path that bypassed that check.
        let db = fresh_db();
        let first = db.create_label("Morning").unwrap();
        assert_eq!(first.name, "Morning");

        let second = db.create_label("Morning");
        let err = second.expect_err("collision must surface as an error");
        assert!(
            matches!(
                err,
                rusqlite::Error::SqliteFailure(ref f, _)
                    if f.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE
            ),
            "expected UNIQUE constraint violation, got {err:?}",
        );
    }

    #[test]
    fn create_label_returns_unique_constraint_error_on_case_variant() {
        // Column is COLLATE NOCASE — different casings collide too.
        let db = fresh_db();
        db.create_label("Morning").unwrap();
        for variant in ["morning", "MORNING", "MoRnInG"] {
            let err = db.create_label(variant)
                .expect_err(&format!("'{variant}' must collide with 'Morning'"));
            assert!(matches!(
                err,
                rusqlite::Error::SqliteFailure(ref f, _)
                    if f.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE
            ));
        }
    }

    #[test]
    fn create_label_preserves_caller_provided_name_verbatim() {
        // No auto-suffix, no normalisation — the caller gets back the
        // exact name they passed in, plus the new id.
        let db = fresh_db();
        let label = db.create_label("Pre-coffee meditation").unwrap();
        assert_eq!(label.name, "Pre-coffee meditation");
        assert!(label.id > 0);
    }

    // ── Streak (gap-and-island via core) ──────────────────────────────────────

    #[test]
    fn streak_empty_db_is_zero() {
        let db = fresh_db();
        assert_eq!(db.get_streak().unwrap(), 0);
        assert_eq!(db.get_best_streak().unwrap(), 0);
    }

    #[test]
    fn streak_today_only() {
        let db = fresh_db();
        seed_session(&db, local_ts(0, 12, 0), 600, None);
        assert_eq!(db.get_streak().unwrap(), 1);
        assert_eq!(db.get_best_streak().unwrap(), 1);
    }

    #[test]
    fn streak_yesterday_only_still_counts() {
        let db = fresh_db();
        seed_session(&db, local_ts(1, 12, 0), 600, None);
        // Grace day: yesterday counts until end-of-today.
        assert_eq!(db.get_streak().unwrap(), 1);
    }

    #[test]
    fn streak_two_days_ago_is_broken() {
        let db = fresh_db();
        seed_session(&db, local_ts(2, 12, 0), 600, None);
        // Older than yesterday → current streak 0 even though best is 1.
        assert_eq!(db.get_streak().unwrap(), 0);
        assert_eq!(db.get_best_streak().unwrap(), 1);
    }

    #[test]
    fn streak_consecutive_run_of_five() {
        let db = fresh_db();
        for d in 0..5 {
            seed_session(&db, local_ts(d, 12, 0), 600, None);
        }
        assert_eq!(db.get_streak().unwrap(), 5);
        assert_eq!(db.get_best_streak().unwrap(), 5);
    }

    #[test]
    fn streak_gap_separates_current_from_best() {
        let db = fresh_db();
        for d in [30, 29, 28, 27, 26, 25] {
            seed_session(&db, local_ts(d, 12, 0), 600, None);
        }
        for d in [2, 1, 0] {
            seed_session(&db, local_ts(d, 12, 0), 600, None);
        }
        assert_eq!(db.get_streak().unwrap(), 3);
        assert_eq!(db.get_best_streak().unwrap(), 6);
    }

    #[test]
    fn streak_multiple_sessions_same_day_count_once() {
        let db = fresh_db();
        seed_session(&db, local_ts(0, 9, 0), 600, None);
        seed_session(&db, local_ts(0, 18, 0), 600, None);
        assert_eq!(db.get_streak().unwrap(), 1);
        assert_eq!(db.get_best_streak().unwrap(), 1);
    }

    // ── Running average ───────────────────────────────────────────────────────

    #[test]
    fn running_average_zero_days_returns_zero() {
        let db = fresh_db();
        assert_eq!(db.get_running_average_secs(0).unwrap(), 0.0);
    }

    #[test]
    fn running_average_empty_window() {
        let db = fresh_db();
        assert_eq!(db.get_running_average_secs(7).unwrap(), 0.0);
    }

    #[test]
    fn running_average_divides_by_window_not_session_count() {
        let db = fresh_db();
        seed_session(&db, local_ts(0, 12, 0), 600, None);
        seed_session(&db, local_ts(1, 12, 0), 600, None);
        let avg = db.get_running_average_secs(7).unwrap();
        assert!((avg - 1200.0 / 7.0).abs() < 1e-6, "avg was {avg}");
    }

    #[test]
    fn running_average_excludes_sessions_before_window() {
        let db = fresh_db();
        // In-window (6 days ago, inside the 7-day window incl. today).
        seed_session(&db, local_ts(6, 12, 0), 300, None);
        // Out-of-window (8 days ago).
        seed_session(&db, local_ts(8, 12, 0), 9999, None);
        let avg = db.get_running_average_secs(7).unwrap();
        assert!((avg - 300.0 / 7.0).abs() < 1e-6, "avg was {avg}");
    }

    // ── Daily totals (local-midnight grouping) ────────────────────────────────

    #[test]
    fn daily_totals_groups_by_local_date() {
        let db = fresh_db();
        // 23:55 local + 00:05 local on the same local day must collapse.
        seed_session(&db, local_ts(0, 23, 55), 300, None);
        seed_session(&db, local_ts(0, 0, 5), 300, None);
        // An earlier local day to prove the grouping actually separates.
        seed_session(&db, local_ts(3, 12, 0), 600, None);

        // Pick a 7-day window that includes both the today-bucket and
        // the 3-days-ago bucket.
        let since = (chrono::Local::now().date_naive() - chrono::Duration::days(7))
            .format("%Y-%m-%d")
            .to_string();
        let totals = db.get_daily_totals(&since).unwrap();
        assert_eq!(totals.len(), 2, "two distinct local dates expected, got {totals:?}");
        // Sorted ascending by day: older first.
        assert_eq!(totals[0].1, 600);
        assert_eq!(totals[1].1, 600); // 300 + 300 collapsed onto today
    }

    // ── Median ────────────────────────────────────────────────────────────────

    #[test]
    fn median_empty_db_is_none() {
        let db = fresh_db();
        assert_eq!(db.get_median_duration_secs().unwrap(), None);
    }

    #[test]
    fn median_single_row() {
        let db = fresh_db();
        seed_session(&db, local_ts(0, 12, 0), 600, None);
        assert_eq!(db.get_median_duration_secs().unwrap(), Some(600));
    }

    #[test]
    fn median_odd_count_is_middle() {
        let db = fresh_db();
        for (i, secs) in [100, 500, 700, 1000, 2000].iter().enumerate() {
            seed_session(&db, local_ts(i as i64, 12, 0), *secs, None);
        }
        assert_eq!(db.get_median_duration_secs().unwrap(), Some(700));
    }

    #[test]
    fn median_even_count_takes_lower_of_two_middles() {
        let db = fresh_db();
        for (i, secs) in [100, 500, 700, 1000].iter().enumerate() {
            seed_session(&db, local_ts(i as i64, 12, 0), *secs, None);
        }
        assert_eq!(db.get_median_duration_secs().unwrap(), Some(500));
    }

    // ── Label totals ──────────────────────────────────────────────────────────

    #[test]
    fn label_totals_groups_sums_and_excludes_empties() {
        let db = fresh_db();
        let morning = db.create_label("Morning").unwrap().id;
        let evening = db.create_label("Evening").unwrap().id;
        let _unused = db.create_label("Unused").unwrap().id;

        seed_session(&db, local_ts(0, 7, 0), 600, Some(morning));
        seed_session(&db, local_ts(1, 7, 0), 300, Some(morning));
        seed_session(&db, local_ts(0, 20, 0), 1200, Some(evening));
        seed_session(&db, local_ts(0, 12, 0), 500, None);

        let got = db.get_label_totals().unwrap();
        assert_eq!(got.len(), 2);
        assert_eq!(got[0], ("Evening".to_string(), 1200, 1));
        assert_eq!(got[1], ("Morning".to_string(), 900, 2));
    }

    #[test]
    fn label_totals_empty_db_is_empty() {
        let db = fresh_db();
        assert!(db.get_label_totals().unwrap().is_empty());
    }

    #[test]
    fn label_totals_ties_break_alphabetically() {
        let db = fresh_db();
        let zebra = db.create_label("Zebra").unwrap().id;
        let alpha = db.create_label("Alpha").unwrap().id;
        seed_session(&db, local_ts(0, 12, 0), 600, Some(zebra));
        seed_session(&db, local_ts(1, 12, 0), 600, Some(alpha));
        let got = db.get_label_totals().unwrap();
        assert_eq!(got[0].0, "Alpha");
        assert_eq!(got[1].0, "Zebra");
    }

    // ── Bundled bell-sound seeding (B.4.2) ────────────────────────

    #[test]
    fn open_seeds_bundled_bell_sounds_with_stable_uuids() {
        let db = test_db_in_memory();
        let sounds = db.list_bell_sounds().unwrap();
        assert_eq!(
            sounds.len(),
            BUNDLED_BELL_SOUNDS.len(),
            "every bundled row must be seeded on first open",
        );
        assert!(sounds.iter().all(|s| s.is_bundled));
        // Stable UUIDs the constants point at.
        assert!(sounds.iter().any(|s| s.uuid == BUNDLED_BOWL_UUID));
        assert!(sounds.iter().any(|s| s.uuid == BUNDLED_BELL_UUID));
        assert!(sounds.iter().any(|s| s.uuid == BUNDLED_GONG_UUID));
    }

    #[test]
    fn seeding_twice_is_idempotent() {
        let db = test_db_in_memory();
        // Helper already seeded once; do it again manually.
        db.seed_bundled_bell_sounds().unwrap();
        let sounds = db.list_bell_sounds().unwrap();
        assert_eq!(
            sounds.len(),
            BUNDLED_BELL_SOUNDS.len(),
            "second seed must not duplicate rows",
        );
    }

    #[test]
    fn open_seeds_default_labels_with_stable_uuids() {
        let db = test_db_in_memory();
        let labels = db.list_labels().unwrap();
        assert!(
            labels.iter().any(|l| l.uuid == DEFAULT_TIMER_LABEL_UUID
                && l.name == "Meditation"),
            "Meditation default seeded under stable uuid",
        );
        assert!(
            labels.iter().any(|l| l.uuid == DEFAULT_BREATHING_LABEL_UUID
                && l.name == "Box-Breathing"),
            "Box-Breathing default seeded under stable uuid",
        );
    }

    #[test]
    fn seeding_default_labels_twice_is_idempotent() {
        let db = test_db_in_memory();
        db.seed_default_labels().unwrap();
        let labels = db.list_labels().unwrap();
        assert_eq!(
            labels.iter().filter(|l| l.uuid == DEFAULT_TIMER_LABEL_UUID).count(),
            1,
            "second seed must not duplicate the Meditation row",
        );
        assert_eq!(
            labels.iter().filter(|l| l.uuid == DEFAULT_BREATHING_LABEL_UUID).count(),
            1,
            "second seed must not duplicate the Box-Breathing row",
        );
    }


    #[test]
    fn seeding_emits_one_insert_event_per_bundled_row_and_no_more_on_re_seed() {
        // Sync correctness: every seeded row produces exactly one
        // bell_sound_insert in the event log on the first device that
        // sees it. A re-seed on a device that's already done it must
        // not bump the event log — peers don't need a redundant insert.
        let db = test_db_in_memory();
        let after_first: Vec<_> = db
            .inner
            .pending_events()
            .unwrap()
            .into_iter()
            .filter(|(_, e)| e.kind == "bell_sound_insert")
            .collect();
        assert_eq!(after_first.len(), BUNDLED_BELL_SOUNDS.len());

        db.seed_bundled_bell_sounds().unwrap();
        let after_second: Vec<_> = db
            .inner
            .pending_events()
            .unwrap()
            .into_iter()
            .filter(|(_, e)| e.kind == "bell_sound_insert")
            .collect();
        assert_eq!(
            after_second.len(),
            BUNDLED_BELL_SOUNDS.len(),
            "no extra events on re-seed",
        );
    }

    // ── Seed-once flags: deletion must not resurrect on next open ─────
    // The seed functions used to run on every `open()`, INSERT-OR-IGNORE
    // by uuid. If the user deleted a seeded row, the next open re-
    // inserted it — and emitted a fresh `*_insert` event with a newer
    // lamport ts, undoing the user's delete on every synced peer. The
    // fix is a one-shot settings flag (`default_labels_seeded`,
    // `bundled_bell_sounds_seeded`) gating each seed.

    #[test]
    fn deleted_default_label_stays_deleted_after_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("meditate.db");
        let db = Database::open(&path).unwrap();
        let labels = db.list_labels().unwrap();
        let meditation = labels
            .iter()
            .find(|l| l.uuid == DEFAULT_TIMER_LABEL_UUID)
            .expect("Meditation seeded on first open");
        db.delete_label(meditation.id).unwrap();
        drop(db);

        let db2 = Database::open(&path).unwrap();
        let labels2 = db2.list_labels().unwrap();
        assert!(
            !labels2.iter().any(|l| l.uuid == DEFAULT_TIMER_LABEL_UUID),
            "deleted seed label must stay deleted across reopen",
        );
    }

    #[test]
    fn deleted_bundled_bell_sound_stays_deleted_after_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("meditate.db");
        let db = Database::open(&path).unwrap();
        db.delete_bell_sound(BUNDLED_BOWL_UUID).unwrap();
        drop(db);

        let db2 = Database::open(&path).unwrap();
        let sounds = db2.list_bell_sounds().unwrap();
        assert!(
            !sounds.iter().any(|s| s.uuid == BUNDLED_BOWL_UUID),
            "deleted seed bell sound must stay deleted across reopen",
        );
    }

    #[test]
    fn second_open_emits_no_seed_events() {
        // Belt-and-braces sync test: even if the deletes above are
        // mocked out, a vanilla second `open()` on a previously-seeded
        // DB must not append `bell_sound_insert` / `label_insert` /
        // `preset_insert` events — those would propagate to peers and
        // look like the local user just re-created the rows.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("meditate.db");
        {
            let _db = Database::open(&path).unwrap();
        }
        let db2 = Database::open(&path).unwrap();
        let pending = db2.inner.pending_events().unwrap();
        let bell_count = pending.iter()
            .filter(|(_, e)| e.kind == "bell_sound_insert").count();
        let label_count = pending.iter()
            .filter(|(_, e)| e.kind == "label_insert").count();
        let preset_count = pending.iter()
            .filter(|(_, e)| e.kind == "preset_insert").count();
        assert_eq!(bell_count, BUNDLED_BELL_SOUNDS.len(),
            "no extra bell_sound_insert events on reopen");
        assert_eq!(label_count, DEFAULT_LABELS.len(),
            "no extra label_insert events on reopen");
        assert_eq!(preset_count, 3,
            "no extra preset_insert events on reopen");
    }

    // ── Preset seeding ────────────────────────────────────────────────

    #[test]
    fn open_seeds_three_default_presets_with_stable_uuids_all_starred() {
        let db = test_db_in_memory();
        let presets = db.list_presets().unwrap();
        assert_eq!(presets.len(), 3);
        assert!(presets.iter().all(|p| p.is_starred), "all bundled starred");
        assert!(presets.iter().any(|p| p.uuid == DEFAULT_SITTING_PRESET_UUID
            && p.name == "Sitting" && p.mode == SessionMode::Timer));
        assert!(presets.iter().any(|p| p.uuid == DEFAULT_BOX_BREATH_4444_UUID
            && p.name == "Box Breath 4-4-4-4" && p.mode == SessionMode::BoxBreath));
        assert!(presets.iter().any(|p| p.uuid == DEFAULT_BOX_BREATH_4780_UUID
            && p.name == "Box Breath 4-7-8-0" && p.mode == SessionMode::BoxBreath));
    }

    #[test]
    fn seeded_preset_configs_round_trip_through_preset_config_schema() {
        let db = test_db_in_memory();
        for p in db.list_presets().unwrap() {
            let cfg = crate::preset_config::PresetConfig::from_json(&p.config_json)
                .unwrap_or_else(|e| panic!(
                    "preset '{}' config_json must round-trip: {e} — json={}",
                    p.name, p.config_json,
                ));
            // Mode invariant: column-level mode and timing variant agree.
            match (&p.mode, &cfg.timing) {
                (SessionMode::Timer,
                    crate::preset_config::PresetTiming::Timer { .. }) => {},
                (SessionMode::BoxBreath,
                    crate::preset_config::PresetTiming::BoxBreath { .. }) => {},
                _ => panic!("preset '{}' column mode {:?} disagrees with timing variant",
                    p.name, p.mode),
            }
        }
    }

    #[test]
    fn seeding_default_presets_twice_is_idempotent() {
        let db = test_db_in_memory();
        // test_db_in_memory already seeded once; do it again.
        db.seed_default_presets().unwrap();
        assert_eq!(db.list_presets().unwrap().len(), 3,
            "second seed must not duplicate rows");
    }

    #[test]
    fn deleted_default_preset_stays_deleted_after_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("meditate.db");
        let db = Database::open(&path).unwrap();
        db.delete_preset(DEFAULT_SITTING_PRESET_UUID).unwrap();
        drop(db);

        let db2 = Database::open(&path).unwrap();
        let presets = db2.list_presets().unwrap();
        assert!(
            !presets.iter().any(|p| p.uuid == DEFAULT_SITTING_PRESET_UUID),
            "deleted seed preset must stay deleted across reopen",
        );
        assert_eq!(presets.len(), 2, "the other two seeds remain");
    }

    // ── Bundled vibration patterns ────────────────────────────────────

    #[test]
    fn bundled_vibration_patterns_are_seeded_on_first_open() {
        let db = test_db_in_memory();
        let mut rows = db.inner.list_vibration_patterns().unwrap();
        // Stable order — sort by uuid so the assertion doesn't depend
        // on the seed-list ordering inside test_db_in_memory.
        rows.sort_by(|a, b| a.uuid.cmp(&b.uuid));
        let expected = [
            (BUNDLED_PATTERN_PULSE_UUID,     "Pulse",     400u32,  ChartKind::Line, 3),
            (BUNDLED_PATTERN_HEARTBEAT_UUID, "Heartbeat", 1500u32, ChartKind::Line, 6),
            (BUNDLED_PATTERN_WAVE_UUID,      "Wave",      2000u32, ChartKind::Line, 7),
            (BUNDLED_PATTERN_RIPPLE_UUID,    "Ripple",    2500u32, ChartKind::Line, 6),
            (BUNDLED_PATTERN_PYRAMID_UUID,   "Pyramid",   3000u32, ChartKind::Bar,  5),
        ];
        let mut expected_sorted: Vec<_> = expected.into_iter().collect();
        expected_sorted.sort_by(|a, b| a.0.cmp(b.0));
        assert_eq!(rows.len(), expected_sorted.len());
        for (row, (uuid, name, dur, kind, n)) in rows.iter().zip(expected_sorted.iter()) {
            assert_eq!(row.uuid, *uuid);
            assert_eq!(row.name, *name);
            assert_eq!(row.duration_ms, *dur);
            assert_eq!(row.chart_kind, *kind);
            assert_eq!(row.intensities.len(), *n,
                "{} should have {} intensity samples", name, n);
            assert!(row.is_bundled, "{} must be flagged as bundled", name);
        }
    }

    #[test]
    fn seeding_bundled_vibration_patterns_twice_is_idempotent() {
        let db = test_db_in_memory();
        db.seed_bundled_vibration_patterns().unwrap();
        assert_eq!(db.inner.list_vibration_patterns().unwrap().len(), 5,
            "second seed must not duplicate rows");
    }

    #[test]
    fn shell_vibration_pattern_wrappers_round_trip() {
        let db = test_db_in_memory();
        // Insert via the auto-uuid wrapper, retrieve via find.
        let uuid = db.insert_vibration_pattern(
            "Custom Wave", 1000, &[0.0, 0.5, 1.0, 0.5, 0.0],
            ChartKind::Line, false,
        ).unwrap();
        let row = db.find_vibration_pattern_by_uuid(&uuid).unwrap().unwrap();
        assert_eq!(row.name, "Custom Wave");
        assert_eq!(row.intensities, vec![0.0, 0.5, 1.0, 0.5, 0.0]);

        // Update via the wrapper.
        db.update_vibration_pattern(
            &uuid, "Slow Wave", 2000, &[0.0, 0.3, 0.0],
            ChartKind::Bar,
        ).unwrap();
        let row = db.find_vibration_pattern_by_uuid(&uuid).unwrap().unwrap();
        assert_eq!(row.name, "Slow Wave");
        assert_eq!(row.chart_kind, ChartKind::Bar);

        // is_name_taken sees it.
        assert!(db.is_vibration_pattern_name_taken("Slow Wave", "").unwrap());
        // Self-rename-no-op excluded via except_uuid.
        assert!(!db.is_vibration_pattern_name_taken("Slow Wave", &uuid).unwrap());

        // Delete via wrapper.
        db.delete_vibration_pattern(&uuid).unwrap();
        assert!(db.find_vibration_pattern_by_uuid(&uuid).unwrap().is_none());
    }

    #[test]
    fn shell_vibration_pattern_duplicate_name_maps_to_unique_constraint_err() {
        // The core-side DuplicateVibrationPattern variant must surface
        // through the shell as a synthesized UNIQUE-constraint failure
        // (same shape callers handle for guided files / presets / labels).
        let db = test_db_in_memory();
        // "Pulse" is already in the seed set — inserting a custom row
        // with the same name must fail through the wrapper.
        let result = db.insert_vibration_pattern(
            "Pulse", 500, &[0.0, 1.0, 0.0], ChartKind::Line, false,
        );
        let Err(rusqlite::Error::SqliteFailure(err, msg)) = result else {
            panic!("expected SqliteFailure(UNIQUE), got {result:?}");
        };
        assert_eq!(err.code, rusqlite::ErrorCode::ConstraintViolation);
        assert_eq!(err.extended_code, rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE);
        assert!(msg.unwrap_or_default().contains("vibration_patterns.name"),
            "error message should name the failing column");
    }

    #[test]
    fn deleted_bundled_vibration_pattern_stays_deleted_after_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("meditate.db");
        let db = Database::open(&path).unwrap();
        db.inner.delete_vibration_pattern(BUNDLED_PATTERN_PULSE_UUID).unwrap();
        drop(db);

        let db2 = Database::open(&path).unwrap();
        let rows = db2.inner.list_vibration_patterns().unwrap();
        assert!(
            !rows.iter().any(|p| p.uuid == BUNDLED_PATTERN_PULSE_UUID),
            "deleted bundled pattern must stay deleted across reopen",
        );
        assert_eq!(rows.len(), 4, "the other four seeds remain");
    }
}
