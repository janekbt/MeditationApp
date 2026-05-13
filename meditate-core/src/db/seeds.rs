//! First-launch bundled-row seeders. Each fn is gated on a settings
//! flag so a re-open doesn't resurrect deleted seed rows (a fresh
//! `*_insert` event with a newer Lamport ts would otherwise override
//! the user's delete on every synced peer). `seed_all_non_audio`
//! composes the platform-agnostic seed steps; `seed_bell_sounds_with_paths`
//! takes platform-specific row data (gresource URIs on gtk, asset URIs
//! on Android).

use rusqlite::params;

use super::{BellSoundCategory, BoxBreathPhaseId, Database, DbError, Result};

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
        if self.get_setting(crate::seeds::PRESETS_SEEDED_KEY, "0")? == "1" {
            return Ok(());
        }
        for (uuid, name, mode, cfg) in crate::seeds::default_presets() {
            match self.insert_preset_with_uuid(uuid, name, mode, true, &cfg.to_json()) {
                Ok(_) => {}
                Err(DbError::DuplicatePreset(_)) => {}
                Err(e) => return Err(e),
            }
        }
        self.set_setting(crate::seeds::PRESETS_SEEDED_KEY, "1")?;
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

#[cfg(test)]
mod tests {
    use crate::db::{ChartKind, Database, SessionMode};
    use crate::preset_config::{PresetConfig, PresetTiming};
    use crate::seeds::{
        BUNDLED_BELL_UUID, BUNDLED_BOWL_UUID, BUNDLED_GONG_UUID,
        BUNDLED_PATTERN_HEARTBEAT_UUID, BUNDLED_PATTERN_PULSE_UUID,
        BUNDLED_PATTERN_PYRAMID_UUID, BUNDLED_PATTERN_RIPPLE_UUID,
        BUNDLED_PATTERN_WAVE_UUID, DEFAULT_BOX_BREATH_4444_UUID,
        DEFAULT_BOX_BREATH_4780_UUID, DEFAULT_BREATHING_LABEL_UUID,
        DEFAULT_LABELS, DEFAULT_SITTING_PRESET_UUID, DEFAULT_TIMER_LABEL_UUID,
    };

    /// Synthetic bell-sound seed list used by the tests that exercise
    /// `seed_bell_sounds_with_paths`. Shape mirrors the shell's
    /// production seed (tuples of uuid, name, file_path, mime_type) but
    /// the paths are nonsense — the seeding behaviour under test is
    /// path-agnostic, the per-shell BUNDLED_BELL_SOUNDS data is what
    /// makes the paths real on a particular target.
    const TEST_BELL_SOUNDS: &[(&str, &str, &str, &str)] = &[
        (BUNDLED_BOWL_UUID, "Singing Bowl", "/test/bowl.ogg", "audio/ogg"),
        (BUNDLED_BELL_UUID, "Bell",         "/test/bell.ogg", "audio/ogg"),
        (BUNDLED_GONG_UUID, "Gong",         "/test/gong.ogg", "audio/ogg"),
    ];

    // ── Default labels ─────────────────────────────────────────────────

    #[test]
    fn seed_default_labels_creates_each_default_under_its_stable_uuid() {
        let db = Database::open_in_memory().unwrap();
        db.seed_default_labels().unwrap();
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
    fn seed_default_labels_twice_is_idempotent() {
        let db = Database::open_in_memory().unwrap();
        db.seed_default_labels().unwrap();
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
    fn deleted_default_label_stays_deleted_after_reopen() {
        // The LABELS_SEEDED_KEY gate prevents re-seeding on subsequent
        // opens. Without it, a user who deleted a seeded label would
        // see it resurrect every launch — and the re-insert would
        // tombstone-override the user's delete on every synced peer.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("seeds_labels.db");
        {
            let db = Database::open(&path).unwrap();
            db.seed_default_labels().unwrap();
            let labels = db.list_labels().unwrap();
            let meditation = labels.iter()
                .find(|l| l.uuid == DEFAULT_TIMER_LABEL_UUID)
                .expect("Meditation seeded on first open");
            db.delete_label(meditation.id).unwrap();
        }
        let db2 = Database::open(&path).unwrap();
        db2.seed_default_labels().unwrap();
        let labels2 = db2.list_labels().unwrap();
        assert!(
            !labels2.iter().any(|l| l.uuid == DEFAULT_TIMER_LABEL_UUID),
            "deleted seed label must stay deleted across reopen",
        );
    }

    // ── Default presets ────────────────────────────────────────────────

    #[test]
    fn seed_default_presets_creates_three_starred_rows_with_stable_uuids() {
        let db = Database::open_in_memory().unwrap();
        db.seed_default_presets().unwrap();
        let presets = db.list_presets().unwrap();
        assert_eq!(presets.len(), 3);
        assert!(presets.iter().all(|p| p.is_starred),
            "all bundled presets ship starred");
        assert!(presets.iter().any(|p| p.uuid == DEFAULT_SITTING_PRESET_UUID
            && p.name == "Sitting" && p.mode == SessionMode::Timer));
        assert!(presets.iter().any(|p| p.uuid == DEFAULT_BOX_BREATH_4444_UUID
            && p.name == "Box Breath 4-4-4-4" && p.mode == SessionMode::BoxBreath));
        assert!(presets.iter().any(|p| p.uuid == DEFAULT_BOX_BREATH_4780_UUID
            && p.name == "Box Breath 4-7-8-0" && p.mode == SessionMode::BoxBreath));
    }

    #[test]
    fn seeded_preset_config_json_round_trips_through_preset_config_schema() {
        // Each seeded preset's config_json must parse back into a
        // PresetConfig and the timing-variant has to agree with the
        // column-level mode — drift would break the Setup view's
        // apply path (and silently for any new build that adds a
        // shape).
        let db = Database::open_in_memory().unwrap();
        db.seed_default_presets().unwrap();
        for p in db.list_presets().unwrap() {
            let cfg = PresetConfig::from_json(&p.config_json)
                .unwrap_or_else(|e| panic!(
                    "preset '{}' config_json must round-trip: {e} — json={}",
                    p.name, p.config_json,
                ));
            match (&p.mode, &cfg.timing) {
                (SessionMode::Timer, PresetTiming::Timer { .. }) => {},
                (SessionMode::BoxBreath, PresetTiming::BoxBreath { .. }) => {},
                _ => panic!("preset '{}' column mode {:?} disagrees with timing variant",
                    p.name, p.mode),
            }
        }
    }

    #[test]
    fn seed_default_presets_twice_is_idempotent() {
        let db = Database::open_in_memory().unwrap();
        db.seed_default_presets().unwrap();
        db.seed_default_presets().unwrap();
        assert_eq!(db.list_presets().unwrap().len(), 3,
            "second seed must not duplicate rows");
    }

    #[test]
    fn deleted_default_preset_stays_deleted_after_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("seeds_presets.db");
        {
            let db = Database::open(&path).unwrap();
            db.seed_default_presets().unwrap();
            db.delete_preset(DEFAULT_SITTING_PRESET_UUID).unwrap();
        }
        let db2 = Database::open(&path).unwrap();
        db2.seed_default_presets().unwrap();
        let presets = db2.list_presets().unwrap();
        assert!(
            !presets.iter().any(|p| p.uuid == DEFAULT_SITTING_PRESET_UUID),
            "deleted seed preset must stay deleted across reopen",
        );
        assert_eq!(presets.len(), 2, "the other two seeds remain");
    }

    // ── Bundled vibration patterns ─────────────────────────────────────

    #[test]
    fn seed_bundled_vibration_patterns_inserts_each_pattern_with_expected_shape() {
        let db = Database::open_in_memory().unwrap();
        db.seed_bundled_vibration_patterns().unwrap();
        let mut rows = db.list_vibration_patterns().unwrap();
        // Stable order — sort by uuid so the assertion doesn't depend
        // on the seed-list ordering.
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
    fn seed_bundled_vibration_patterns_twice_is_idempotent() {
        let db = Database::open_in_memory().unwrap();
        db.seed_bundled_vibration_patterns().unwrap();
        db.seed_bundled_vibration_patterns().unwrap();
        assert_eq!(db.list_vibration_patterns().unwrap().len(), 5,
            "second seed must not duplicate rows");
    }

    #[test]
    fn deleted_bundled_vibration_pattern_stays_deleted_after_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("seeds_vibration.db");
        {
            let db = Database::open(&path).unwrap();
            db.seed_bundled_vibration_patterns().unwrap();
            db.delete_vibration_pattern(BUNDLED_PATTERN_PULSE_UUID).unwrap();
        }
        let db2 = Database::open(&path).unwrap();
        db2.seed_bundled_vibration_patterns().unwrap();
        let rows = db2.list_vibration_patterns().unwrap();
        assert!(
            !rows.iter().any(|p| p.uuid == BUNDLED_PATTERN_PULSE_UUID),
            "deleted bundled pattern must stay deleted across reopen",
        );
    }

    // ── Bell sounds (seed list lives shell-side; tests use synthetic data) ──

    #[test]
    fn seed_bell_sounds_with_paths_creates_one_row_per_tuple() {
        let db = Database::open_in_memory().unwrap();
        db.seed_bell_sounds_with_paths(TEST_BELL_SOUNDS).unwrap();
        let sounds = db.list_bell_sounds().unwrap();
        assert_eq!(sounds.len(), TEST_BELL_SOUNDS.len());
        assert!(sounds.iter().all(|s| s.is_bundled),
            "every row inserted via the seed helper is flagged bundled");
        assert!(sounds.iter().any(|s| s.uuid == BUNDLED_BOWL_UUID));
        assert!(sounds.iter().any(|s| s.uuid == BUNDLED_BELL_UUID));
        assert!(sounds.iter().any(|s| s.uuid == BUNDLED_GONG_UUID));
    }

    #[test]
    fn seed_bell_sounds_with_paths_twice_is_idempotent() {
        let db = Database::open_in_memory().unwrap();
        db.seed_bell_sounds_with_paths(TEST_BELL_SOUNDS).unwrap();
        db.seed_bell_sounds_with_paths(TEST_BELL_SOUNDS).unwrap();
        assert_eq!(db.list_bell_sounds().unwrap().len(), TEST_BELL_SOUNDS.len(),
            "second seed must not duplicate rows");
    }

    #[test]
    fn deleted_seeded_bell_sound_stays_deleted_after_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("seeds_bell_sounds.db");
        {
            let db = Database::open(&path).unwrap();
            db.seed_bell_sounds_with_paths(TEST_BELL_SOUNDS).unwrap();
            db.delete_bell_sound(BUNDLED_BOWL_UUID).unwrap();
        }
        let db2 = Database::open(&path).unwrap();
        db2.seed_bell_sounds_with_paths(TEST_BELL_SOUNDS).unwrap();
        let sounds = db2.list_bell_sounds().unwrap();
        assert!(
            !sounds.iter().any(|s| s.uuid == BUNDLED_BOWL_UUID),
            "deleted seed bell sound must stay deleted across reopen",
        );
    }

    // ── Cross-entity invariants ────────────────────────────────────────

    #[test]
    fn seed_bell_sounds_emits_one_insert_event_per_row_and_none_on_re_seed() {
        // Every seeded bell-sound row must produce exactly one
        // bell_sound_insert in the event log so peers materialise the
        // same set on first sync. A re-seed on a device that's
        // already done it must NOT emit additional events — peers
        // don't need a redundant insert.
        let db = Database::open_in_memory().unwrap();
        db.seed_bell_sounds_with_paths(TEST_BELL_SOUNDS).unwrap();
        let after_first: Vec<_> = db
            .pending_events()
            .unwrap()
            .into_iter()
            .filter(|(_, e)| e.kind == "bell_sound_insert")
            .collect();
        assert_eq!(after_first.len(), TEST_BELL_SOUNDS.len());

        db.seed_bell_sounds_with_paths(TEST_BELL_SOUNDS).unwrap();
        let after_second: Vec<_> = db
            .pending_events()
            .unwrap()
            .into_iter()
            .filter(|(_, e)| e.kind == "bell_sound_insert")
            .collect();
        assert_eq!(
            after_second.len(),
            TEST_BELL_SOUNDS.len(),
            "no extra events on re-seed",
        );
    }

    #[test]
    fn second_open_emits_no_seed_events() {
        // Belt-and-braces sync test: even if every per-entity seed gate
        // is doing its job, a vanilla second `open()` followed by the
        // seed calls on a previously-seeded DB must not append
        // bell_sound_insert / label_insert / preset_insert events —
        // those would propagate to peers and look like the local user
        // just re-created the rows.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("seeds_second_open.db");
        {
            let db = Database::open(&path).unwrap();
            db.seed_bell_sounds_with_paths(TEST_BELL_SOUNDS).unwrap();
            db.seed_all_non_audio().unwrap();
        }
        let db2 = Database::open(&path).unwrap();
        db2.seed_bell_sounds_with_paths(TEST_BELL_SOUNDS).unwrap();
        db2.seed_all_non_audio().unwrap();
        let pending = db2.pending_events().unwrap();
        let bell_count = pending.iter()
            .filter(|(_, e)| e.kind == "bell_sound_insert").count();
        let label_count = pending.iter()
            .filter(|(_, e)| e.kind == "label_insert").count();
        let preset_count = pending.iter()
            .filter(|(_, e)| e.kind == "preset_insert").count();
        assert_eq!(bell_count, TEST_BELL_SOUNDS.len(),
            "no extra bell_sound_insert events on reopen");
        assert_eq!(label_count, DEFAULT_LABELS.len(),
            "no extra label_insert events on reopen");
        assert_eq!(preset_count, 3,
            "no extra preset_insert events on reopen");
    }
}
