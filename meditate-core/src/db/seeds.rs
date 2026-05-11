//! First-launch bundled-row seeders. Each fn is gated on a settings
//! flag so a re-open doesn't resurrect deleted seed rows (a fresh
//! `*_insert` event with a newer Lamport ts would otherwise override
//! the user's delete on every synced peer). `seed_all_non_audio`
//! composes the platform-agnostic seed steps; `seed_bell_sounds_with_paths`
//! takes platform-specific row data (gresource URIs on gtk, asset URIs
//! on Android).

use rusqlite::params;

use super::{BellSoundCategory, BoxBreathPhaseId, Database, DbError, Result, SessionMode};

impl Database {
    /// Seed-once bundled bell-sound orchestration: gated on
    /// `BELLS_SEEDED_KEY` so a repeat startup is a no-op, walks the
    /// caller-supplied rows (each `(uuid, name, file_path, mime)`),
    /// inserts each as a bundled General-category row, then flips
    /// the gate to "1". The platform-specific bit — the `file_path`,
    /// which is a GResource URI on gtk and an assets:// URI on
    /// Android — stays in the shell's row table; the orchestration
    /// (idempotency, ordering, emit-event behavior via the
    /// underlying `insert_bell_sound_with_uuid`) lives here.
    pub fn seed_bell_sounds_with_paths(
        &self,
        rows: &[(&str, &str, &str, &str)],
    ) -> Result<()> {
        if self.get_setting(crate::seeds::BELLS_SEEDED_KEY, "0")? == "1" {
            return Ok(());
        }
        for (uuid, name, file_path, mime) in rows {
            self.insert_bell_sound_with_uuid(
                uuid, name, file_path, true, mime, BellSoundCategory::General,
            )?;
        }
        self.set_setting(crate::seeds::BELLS_SEEDED_KEY, "1")?;
        Ok(())
    }

    /// Seed all four rows once on first open. Idempotent — re-seeding
    /// after the rows exist is a no-op (INSERT OR IGNORE on the
    /// `phase` PK). Doesn't emit events: the rows hold default values
    /// only, so a peer that already has them seeded would see a
    /// pointless overwrite.
    pub fn seed_box_breath_phases(&self) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        for phase in BoxBreathPhaseId::all() {
            self.conn.execute(
                "INSERT OR IGNORE INTO box_breath_phases (phase) VALUES (?1)",
                params![phase.as_db_str()],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Seed the two default labels ("Meditation", "Box-Breathing")
    /// with stable UUIDs, gated by the one-shot
    /// `default_labels_seeded` settings flag. Deleting a seed label
    /// must NOT resurrect it on the next open — a fresh
    /// `label_insert` with a newer lamport ts would override the
    /// user's deletion via sync. `DuplicateLabel` (the user already
    /// owns a label with the same name under a different UUID) is
    /// silently swallowed so we don't shadow user-managed rows.
    pub fn seed_default_labels(&self) -> Result<()> {
        if self.get_setting(crate::seeds::LABELS_SEEDED_KEY, "0")? == "1" {
            return Ok(());
        }
        for (uuid, name) in crate::seeds::DEFAULT_LABELS {
            match self.insert_label_with_uuid(uuid, name) {
                Ok(_) => {}
                Err(DbError::DuplicateLabel(_)) => {}
                Err(e) => return Err(e),
            }
        }
        self.set_setting(crate::seeds::LABELS_SEEDED_KEY, "1")?;
        Ok(())
    }

    /// Seed the five bundled vibration patterns (Pulse / Heartbeat
    /// / Wave / Ripple / Pyramid) under stable UUIDs, gated by the
    /// one-shot `bundled_vibration_patterns_seeded` settings flag.
    /// Same resurrect-bug guard as the labels seed.
    pub fn seed_bundled_vibration_patterns(&self) -> Result<()> {
        if self.get_setting(crate::seeds::VIBRATION_PATTERNS_SEEDED_KEY, "0")? == "1" {
            return Ok(());
        }
        for &(uuid, name, duration_ms, intensities, chart_kind) in
            crate::seeds::BUNDLED_VIBRATION_PATTERNS
        {
            self.insert_vibration_pattern_with_uuid(
                uuid, name, duration_ms, intensities, chart_kind, true,
            )?;
        }
        self.set_setting(crate::seeds::VIBRATION_PATTERNS_SEEDED_KEY, "1")?;
        Ok(())
    }

    /// Seed the three bundled presets — one Timer ("Sitting") plus
    /// two Box-Breath patterns (4-4-4-4 and 4-7-8-0) — under stable
    /// UUIDs, all starred so they show in the home-screen chip list
    /// on first run. Mode-strict separation means the user always
    /// sees one of each kind regardless of which mode they start
    /// the app in. Same resurrect-bug guard via the
    /// `default_presets_seeded` one-shot flag; `DuplicatePreset`
    /// (user has a preset with the same name under a different UUID)
    /// is silently swallowed.
    pub fn seed_default_presets(&self) -> Result<()> {
        use crate::preset_config::*;
        use crate::seeds::*;
        if self.get_setting(PRESETS_SEEDED_KEY, "0")? == "1" {
            return Ok(());
        }
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
                uuid: Some(DEFAULT_TIMER_LABEL_UUID.to_string()),
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
                uuid: Some(DEFAULT_BREATHING_LABEL_UUID.to_string()),
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
        let seeds: &[(&str, &str, SessionMode, &PresetConfig)] = &[
            (DEFAULT_SITTING_PRESET_UUID, "Sitting", SessionMode::Timer, &sitting),
            (
                DEFAULT_BOX_BREATH_4444_UUID,
                "Box Breath 4-4-4-4",
                SessionMode::BoxBreath,
                &box_4444,
            ),
            (
                DEFAULT_BOX_BREATH_4780_UUID,
                "Box Breath 4-7-8-0",
                SessionMode::BoxBreath,
                &box_4780,
            ),
        ];
        for (uuid, name, mode, cfg) in seeds {
            match self.insert_preset_with_uuid(uuid, name, *mode, true, &cfg.to_json()) {
                Ok(_) => {}
                Err(DbError::DuplicatePreset(_)) => {}
                Err(e) => return Err(e),
            }
        }
        self.set_setting(PRESETS_SEEDED_KEY, "1")?;
        Ok(())
    }

    /// Composition of all non-audio seed steps — box-breath phases,
    /// default labels, default presets, bundled vibration patterns.
    /// The bell-sound seed list stays in each shell because the
    /// audio files are platform-specific (gtk → gresource paths,
    /// Android → assets/raw paths); the shell calls
    /// `seed_bundled_bell_sounds` separately.
    pub fn seed_all_non_audio(&self) -> Result<()> {
        self.seed_box_breath_phases()?;
        self.seed_default_labels()?;
        self.seed_default_presets()?;
        self.seed_bundled_vibration_patterns()?;
        Ok(())
    }
}
