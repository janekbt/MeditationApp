//! Single-device integration tests for `meditate-core`.
//!
//! Counterpart to `sync_roundtrip.rs`: these scenarios cross multiple
//! crate modules through the public surface (DB CRUD + preset_config +
//! CSV + the event-log-as-source-of-truth invariant) but never touch
//! the sync orchestrator or `FakeWebDav`. They cover the flows a user
//! exercises on their phone before any peer is involved — and the
//! recovery primitives that fire when something has gone wrong.
//!
//! Tests run on file-based databases under `tempfile::TempDir` so the
//! close-reopen scenario is testing real on-disk persistence, not
//! just the in-memory connection cache.

use std::io::Cursor;

use meditate_core::db::{
    list_labels_from_db, list_presets_for_mode_from_db, list_sessions_from_db,
    list_sessions_for_label_from_db, query_sessions_from_db, BellSoundCategory,
    ChartKind, Event, IntervalBellKind, Session, SessionFilter, SessionMode,
    SignalMode,
};
use meditate_core::preset_config::{
    PresetConfig, PresetLabel, PresetTiming,
};
use meditate_core::Database;

/// Open at `<tempdir>/<name>.db` + seed the non-audio defaults.
/// Returns the absolute path so the close-reopen test can reuse it.
fn open_test_db(
    tempdir: &tempfile::TempDir,
    name: &str,
) -> (Database, std::path::PathBuf) {
    let path = tempdir.path().join(format!("{name}.db"));
    let db = Database::open(&path).expect("open db");
    db.seed_all_non_audio().expect("seed");
    (db, path)
}

// ── Scenario 1: CSV export → wipe → import preserves every session ────────

#[test]
fn csv_export_then_wipe_then_import_preserves_every_session() {
    // Janek's backup-and-restore path: the user exports their
    // session history to a CSV, something destroys their local
    // sessions table (uninstall, factory reset, bug), and an
    // import restores everything. The format is 5 columns:
    // start_iso, duration_secs, label_name, notes, mode. Label
    // associations resolve by NAME (not UUID), so labels with the
    // same name on both sides round-trip even if their internal
    // rowids/UUIDs don't.
    let tempdir = tempfile::tempdir().expect("tempdir");
    let (db, _) = open_test_db(&tempdir, "csv");

    let practice = db.insert_label("Practice").expect("label 1");
    let evening = db.insert_label("Evening").expect("label 2");

    // Five sessions, varied along every column the CSV format
    // carries: labeled and unlabeled, with and without notes, Timer
    // and BoxBreath modes, distinct durations.
    let originals = [
        ("2026-04-01T07:30:00", 600u32, Some(practice), None, SessionMode::Timer),
        ("2026-04-02T08:15:00", 900, Some(practice), Some("focused"), SessionMode::Timer),
        ("2026-04-03T20:00:00", 300, Some(evening), None, SessionMode::BoxBreath),
        ("2026-04-04T20:30:00", 1200, Some(evening), Some("difficult night"), SessionMode::BoxBreath),
        ("2026-04-05T07:00:00", 450, None, Some("travel sit"), SessionMode::Timer),
    ];
    for (start, dur, label, notes, mode) in originals {
        db.insert_session(&Session {
            start_iso: start.into(),
            duration_secs: dur,
            label_id: label,
            notes: notes.map(str::to_string),
            mode,
            uuid: meditate_core::db::SessionUuid::new(""),
            guided_file_uuid: None,
        })
        .expect("insert original");
    }
    assert_eq!(list_sessions_from_db(&db).unwrap().len(), 5);

    // Export to bytes via the Write trait.
    let mut csv_bytes = Vec::<u8>::new();
    db.export_sessions_csv(&mut csv_bytes).expect("export");
    assert!(!csv_bytes.is_empty(), "CSV must have content");

    // Wipe — the "I lost my data" disaster the import is meant to
    // recover from. Labels deliberately survive (the CSV's label
    // column is by-name, so leaving labels intact is the realistic
    // path; the import-side `find_or_create_label` handles the
    // missing-label case too).
    let wiped = db.delete_all_sessions().expect("wipe sessions");
    assert_eq!(wiped, 5);
    assert_eq!(list_sessions_from_db(&db).unwrap().len(), 0);

    // Import back from the bytes we just exported.
    let imported = db
        .import_sessions_csv(Cursor::new(csv_bytes))
        .expect("import");
    assert_eq!(imported, 5, "every row must round-trip");

    // Every field must match the originals — order is preserved
    // because export sorts by rowid (insertion order) and import
    // walks the CSV top-to-bottom.
    let restored = list_sessions_from_db(&db).unwrap();
    assert_eq!(restored.len(), 5);
    for ((_id, s), (orig_start, orig_dur, orig_label, orig_notes, orig_mode))
        in restored.iter().zip(originals.iter())
    {
        assert_eq!(s.start_iso, *orig_start);
        assert_eq!(s.duration_secs, *orig_dur);
        assert_eq!(s.mode, *orig_mode);
        assert_eq!(s.notes.as_deref(), *orig_notes,
            "notes round-trip for start_iso={}", s.start_iso);

        // Label round-trips by NAME — the rowid may have changed
        // (insertion-order ids on import path), so look up by name.
        let expected_label_name = orig_label.map(|rowid| {
            list_labels_from_db(&db)
                .unwrap()
                .into_iter()
                .find(|l| l.id == rowid)
                .map(|l| l.name)
                .expect("original label still in DB")
        });
        let restored_label_name = s.label_id.map(|rowid| {
            list_labels_from_db(&db)
                .unwrap()
                .into_iter()
                .find(|l| l.id == rowid)
                .map(|l| l.name)
                .expect("restored label resolves")
        });
        assert_eq!(restored_label_name, expected_label_name,
            "label name must round-trip for start_iso={}", s.start_iso);
    }
}

// ── Scenario 2: cache is a pure function of the event log ─────────────────

#[test]
fn wipe_then_replay_reconstructs_every_cache_row_from_the_event_log() {
    // The load-bearing invariant of the event-sourced cache: every
    // user-content table can be rebuilt by replaying its events.
    // `wipe_local_event_log` (via the pub `prepare_wipe_local_recovery`
    // wrapper) clears both the events table and the cache rows; if
    // the invariant holds, capturing pending_events first and
    // replaying them after the wipe must restore the exact same
    // cache state.
    //
    // This test guards against the class of bug where apply_event
    // does something the original CRUD path does NOT (or vice
    // versa) — a divergence would manifest as the rebuild ending
    // with a different state than the snapshot.
    let tempdir = tempfile::tempdir().expect("tempdir");
    let (db, _) = open_test_db(&tempdir, "rebuild");

    // Insert one row of each entity kind — the same coverage as
    // sync_roundtrip::replay_events_covers_every_entity_kind, but
    // here we're proving the entity's OWN events survive a wipe
    // and rebuild it on the same database.
    db.insert_label("Snapshot-Label").expect("label");
    db.insert_session(&Session {
        start_iso: "2026-04-15T10:00:00".into(),
        duration_secs: 480,
        label_id: None,
        notes: Some("rebuild me".into()),
        mode: SessionMode::Timer,
        uuid: meditate_core::db::SessionUuid::new(""),
        guided_file_uuid: None,
    })
    .expect("session");
    db.insert_bell_sound(
        "Snapshot-Bell",
        "snap.ogg",
        false,
        "audio/ogg",
        BellSoundCategory::General,
    )
    .expect("bell sound");
    let pattern_uuid = db
        .insert_vibration_pattern(
            "Snapshot-Pulse",
            250,
            &[0.0, 1.0, 0.0],
            ChartKind::Line,
            false,
        )
        .expect("pattern");
    let bell_sound_uuid = db
        .list_bell_sounds()
        .unwrap()
        .into_iter()
        .find(|s| s.name == "Snapshot-Bell")
        .unwrap()
        .uuid
        .0;
    db.insert_interval_bell(
        IntervalBellKind::Interval,
        3,
        15,
        &bell_sound_uuid,
        &pattern_uuid,
        SignalMode::Sound,
    )
    .expect("interval bell");
    db.insert_preset_with_uuid(
        "11111111-2222-3333-4444-555555555555",
        "Snapshot-Preset",
        SessionMode::Timer,
        true,
        r#"{"duration_min":8}"#,
    )
    .expect("preset");
    db.insert_guided_file_with_uuid(
        "22222222-3333-4444-5555-666666666666",
        "Snapshot-Guided",
        "guided/snap.ogg",
        480,
        false,
    )
    .expect("guided file");

    // Snapshot the cache state. Only the counts/identities the
    // post-replay assertions actually compare against need to be
    // captured here; the rest are checked with literal expected
    // values inline.
    let sessions_before_len = list_sessions_from_db(&db).unwrap().len();
    let interval_bells_before_len = db.list_interval_bells().unwrap().len();

    // Capture the event log before the wipe — this is the only
    // input the replay has access to.
    let events: Vec<Event> = db
        .pending_events()
        .expect("pending")
        .into_iter()
        .map(|(_, e)| e)
        .collect();
    assert!(!events.is_empty(), "events captured before wipe");

    // Wipe local: clears events + cache rows + sync state. After
    // this the DB looks like a brand-new device with seed defaults
    // gone too.
    meditate_core::sync::settings::prepare_wipe_local_recovery(&db)
        .expect("wipe");

    // Sanity-check the wipe actually happened — otherwise the
    // "rebuild" might just be observing pre-wipe state.
    assert!(list_labels_from_db(&db).unwrap()
        .iter().all(|l| l.name != "Snapshot-Label"));
    assert!(list_sessions_from_db(&db).unwrap().is_empty());
    assert!(db.list_bell_sounds().unwrap()
        .iter().all(|s| s.name != "Snapshot-Bell"));

    // Replay every captured event. After this the cache rows for
    // user-authored entities must match the pre-wipe snapshot.
    db.replay_events(&events).expect("replay");

    assert!(
        list_labels_from_db(&db).unwrap().iter()
            .any(|l| l.name == "Snapshot-Label"),
        "label rebuilt from event log",
    );
    assert_eq!(
        list_sessions_from_db(&db).unwrap().len(),
        sessions_before_len,
        "sessions rebuilt from event log",
    );
    assert!(
        db.list_bell_sounds().unwrap().iter().any(|s| s.name == "Snapshot-Bell"),
        "custom bell sound rebuilt",
    );
    assert_eq!(
        db.list_interval_bells().unwrap().len(),
        interval_bells_before_len,
        "interval bells rebuilt",
    );
    assert!(
        meditate_core::db::list_vibration_patterns_from_db(&db).unwrap()
            .iter().any(|p| p.name == "Snapshot-Pulse"),
        "vibration pattern rebuilt",
    );
    assert!(
        list_presets_for_mode_from_db(&db, SessionMode::Timer).unwrap()
            .iter().any(|p| p.name == "Snapshot-Preset"),
        "preset rebuilt",
    );
    assert!(
        meditate_core::db::list_guided_files_from_db(&db).unwrap()
            .iter().any(|g| g.name == "Snapshot-Guided"),
        "guided file rebuilt",
    );
}

// ── Scenario 3: preset → session → query happy-path loop ──────────────────

#[test]
fn preset_config_round_trips_through_db_and_drives_a_session() {
    // The main happy path: a user builds a preset, the shell saves
    // it as a JSON blob, later a session runs against that preset
    // and persists. Stats queries must then attribute the session
    // to the preset's label correctly.
    //
    // This test exercises three modules end-to-end:
    //   - `preset_config` (PresetConfig ↔ JSON serialization)
    //   - `db::presets` (insert_preset_with_uuid + read-back)
    //   - `db::sessions` (insert_session + list_sessions_for_label_from_db)
    let tempdir = tempfile::tempdir().expect("tempdir");
    let (db, _) = open_test_db(&tempdir, "preset_loop");

    // Author a label the preset will reference, then build a
    // PresetConfig pinned to it.
    let label_rowid = db.insert_label("Mindful").expect("label");
    let label_uuid = list_labels_from_db(&db)
        .unwrap()
        .into_iter()
        .find(|l| l.id == label_rowid)
        .unwrap()
        .uuid;
    let cfg = PresetConfig {
        label: PresetLabel { enabled: true, uuid: Some(label_uuid.clone()) },
        timing: PresetTiming::Timer { stopwatch: false, duration_secs: 900 },
        cues_signal_mode: "sound".into(),
        keep_screen_awake: true,
        ..Default::default()
    };
    let preset_uuid = "aaaa3333-bbbb-4444-cccc-555555555555";
    db.insert_preset_with_uuid(
        preset_uuid,
        "Mindful 15",
        SessionMode::Timer,
        true,
        &cfg.to_json(),
    )
    .expect("insert preset");

    // Read the preset back and deserialize the config. The blob in
    // `presets.config_json` is exactly what the shell would
    // serialize, so this proves serde round-trip without crossing
    // process boundaries.
    let stored = list_presets_for_mode_from_db(&db, SessionMode::Timer)
        .unwrap()
        .into_iter()
        .find(|p| p.uuid.0 == preset_uuid)
        .expect("preset stored");
    assert!(stored.is_starred);
    let restored = PresetConfig::from_json(&stored.config_json)
        .expect("config_json must deserialize");
    assert_eq!(restored.label.uuid, Some(label_uuid.clone()));
    assert_eq!(
        restored.timing,
        PresetTiming::Timer { stopwatch: false, duration_secs: 900 },
    );
    assert!(restored.keep_screen_awake);

    // Run a session using the preset's timing + label. Mirrors what
    // the shell does when the user taps "Start" on the preset card.
    let session_duration = match restored.timing {
        PresetTiming::Timer { duration_secs, .. } => duration_secs,
        PresetTiming::BoxBreath { duration_secs, .. } => duration_secs,
    };
    db.insert_session(&Session {
        start_iso: "2026-04-16T07:00:00".into(),
        duration_secs: session_duration,
        label_id: Some(label_rowid),
        notes: None,
        mode: SessionMode::Timer,
        uuid: meditate_core::db::SessionUuid::new(""),
        guided_file_uuid: None,
    })
    .expect("session");

    // Query the stats helpers — the session must show up against
    // its label, contribute to the longest-session record, and
    // appear in the by-label session list.
    let longest = meditate_core::db::get_longest_session_from_db(&db)
        .expect("longest");
    let (_id, longest_session) = longest.expect("at least one session");
    assert_eq!(longest_session.duration_secs, session_duration);
    let by_label = list_sessions_for_label_from_db(&db, label_rowid)
        .expect("by-label list");
    assert_eq!(by_label.len(), 1, "session attributed to label");
    let label_counts = meditate_core::db::count_sessions_by_label_from_db(&db)
        .expect("count by label");
    let mindful_count = label_counts
        .iter()
        .find(|(name, _)| name.as_deref() == Some("Mindful"))
        .map(|(_, c)| *c)
        .unwrap_or(0);
    assert_eq!(mindful_count, 1, "label aggregation reflects the new session");
}

// ── Scenario 4: SessionFilter shapes return correct subsets ───────────────

#[test]
fn session_filter_returns_correct_subsets_for_label_notes_and_pagination() {
    // SessionFilter is the structured query that drives the stats
    // screen's filter panel. Its four axes — label_id, only_with_notes,
    // limit, offset — must compose: enabling one shouldn't change
    // how the others behave. This test builds a populated DB and
    // pins a representative grid of shapes.
    let tempdir = tempfile::tempdir().expect("tempdir");
    let (db, _) = open_test_db(&tempdir, "filter");

    let work = db.insert_label("Work").expect("label work");
    let home = db.insert_label("Home").expect("label home");

    // 8 sessions: 3 work (1 with notes), 3 home (2 with notes), 2 unlabeled.
    let plan: &[(&str, Option<i64>, Option<&str>)] = &[
        ("2026-04-20T07:00:00", Some(work), Some("kickoff")),
        ("2026-04-20T12:00:00", Some(work), None),
        ("2026-04-20T18:00:00", Some(work), None),
        ("2026-04-21T07:00:00", Some(home), Some("morning")),
        ("2026-04-21T12:00:00", Some(home), None),
        ("2026-04-21T18:00:00", Some(home), Some("evening")),
        ("2026-04-22T07:00:00", None, None),
        ("2026-04-22T18:00:00", None, Some("between things")),
    ];
    for (start, label, notes) in plan {
        db.insert_session(&Session {
            start_iso: (*start).into(),
            duration_secs: 600,
            label_id: *label,
            notes: notes.map(str::to_string),
            mode: SessionMode::Timer,
            uuid: meditate_core::db::SessionUuid::new(""),
            guided_file_uuid: None,
        })
        .expect("seed session");
    }

    // Default filter — every session, no pagination.
    let all = query_sessions_from_db(&db, &SessionFilter::default())
        .expect("default filter");
    assert_eq!(all.len(), 8);

    // label_id = Some(work) — 3 rows.
    let work_only = query_sessions_from_db(
        &db,
        &SessionFilter { label_id: Some(work), ..Default::default() },
    )
    .expect("work filter");
    assert_eq!(work_only.len(), 3);
    assert!(work_only.iter().all(|(_, s)| s.label_id == Some(work)));

    // only_with_notes — 4 rows: 1 work + 2 home + 1 unlabeled.
    let with_notes = query_sessions_from_db(
        &db,
        &SessionFilter { only_with_notes: true, ..Default::default() },
    )
    .expect("notes filter");
    assert_eq!(with_notes.len(), 4);
    assert!(with_notes.iter()
        .all(|(_, s)| s.notes.as_deref().is_some_and(|n| !n.is_empty())));

    // Compose label + notes — home + with_notes = 2 rows.
    let home_with_notes = query_sessions_from_db(
        &db,
        &SessionFilter {
            label_id: Some(home),
            only_with_notes: true,
            ..Default::default()
        },
    )
    .expect("home+notes filter");
    assert_eq!(home_with_notes.len(), 2);

    // Pagination: limit=3 caps the row count.
    let page = query_sessions_from_db(
        &db,
        &SessionFilter { limit: Some(3), ..Default::default() },
    )
    .expect("limit=3");
    assert_eq!(page.len(), 3);

    // Pagination: limit=3 offset=3 yields a different 3 rows than
    // limit=3 offset=0. (We don't pin the exact ordering — that's
    // an implementation detail — only that pagination advances.)
    let page2 = query_sessions_from_db(
        &db,
        &SessionFilter {
            limit: Some(3),
            offset: Some(3),
            ..Default::default()
        },
    )
    .expect("limit=3 offset=3");
    assert_eq!(page2.len(), 3);
    let ids_page1: std::collections::HashSet<i64> =
        page.iter().map(|(id, _)| *id).collect();
    let ids_page2: std::collections::HashSet<i64> =
        page2.iter().map(|(id, _)| *id).collect();
    assert!(
        ids_page1.is_disjoint(&ids_page2),
        "consecutive pages must contain different rows: \
         page1={ids_page1:?}, page2={ids_page2:?}",
    );

    // Empty result when no row matches — work + only_with_notes = 1
    // (only "kickoff"), but a non-existent label yields zero.
    let none = query_sessions_from_db(
        &db,
        &SessionFilter { label_id: Some(9999), ..Default::default() },
    )
    .expect("nonexistent label");
    assert!(none.is_empty());
}

// ── Scenario 5: every entity table survives a close-reopen at same path ───

#[test]
fn every_entity_table_survives_a_database_close_reopen_cycle() {
    // The on-disk persistence contract: dropping the Database
    // handle flushes the WAL, opening the same path picks up
    // everything that was written. A regression here (schema
    // migration breakage, sqlite WAL mode mis-configuration, a
    // botched cache-only insert that never hit disk) wipes user
    // data on next app launch — silently if the user doesn't
    // notice immediately.
    let tempdir = tempfile::tempdir().expect("tempdir");
    let (db, path) = open_test_db(&tempdir, "persistence");

    // Author one row of each user-content entity kind. UUIDs and
    // identifying names captured for the reopen assertions.
    let label_rowid = db.insert_label("Persistent").expect("label");
    let label_uuid = list_labels_from_db(&db)
        .unwrap()
        .into_iter()
        .find(|l| l.id == label_rowid)
        .unwrap()
        .uuid
        .0;

    let session_rowid = db
        .insert_session(&Session {
            start_iso: "2026-04-17T08:00:00".into(),
            duration_secs: 720,
            label_id: Some(label_rowid),
            notes: Some("survive me".into()),
            mode: SessionMode::Timer,
            uuid: meditate_core::db::SessionUuid::new(""),
            guided_file_uuid: None,
        })
        .expect("session");

    let preset_uuid = "bbbb4444-cccc-5555-dddd-666666666666";
    db.insert_preset_with_uuid(
        preset_uuid,
        "Persistent Preset",
        SessionMode::Timer,
        true,
        r#"{"duration_min":12}"#,
    )
    .expect("preset");

    let guided_uuid = "cccc4444-dddd-5555-eeee-666666666666";
    db.insert_guided_file_with_uuid(
        guided_uuid,
        "Persistent Guided",
        "guided/persist.ogg",
        720,
        false,
    )
    .expect("guided file");

    let pattern_uuid = db
        .insert_vibration_pattern(
            "Persistent Pulse",
            400,
            &[0.0, 0.5, 1.0, 0.0],
            ChartKind::Line,
            false,
        )
        .expect("pattern");

    let bell_sound_rowid = db
        .insert_bell_sound(
            "Persistent Chime",
            "persist.ogg",
            false,
            "audio/ogg",
            BellSoundCategory::General,
        )
        .expect("bell sound");
    let bell_sound_uuid = db
        .list_bell_sounds()
        .unwrap()
        .into_iter()
        .find(|s| s.name == "Persistent Chime")
        .unwrap()
        .uuid
        .0;

    db.insert_interval_bell(
        IntervalBellKind::Interval,
        4,
        25,
        &bell_sound_uuid,
        &pattern_uuid,
        SignalMode::Both,
    )
    .expect("interval bell");

    // Drop the handle — this is the close half of close-reopen.
    // Rust drops the Database (and the underlying rusqlite Connection),
    // which flushes the WAL and releases the file lock.
    drop(db);

    // Reopen the SAME path. No seeding this time — the seed is
    // idempotent so calling it would just no-op, but skipping it
    // proves the persisted state is sufficient on its own.
    let reopened = Database::open(&path).expect("reopen db at same path");

    // Every authored row must still be there with the same key fields.
    let labels = list_labels_from_db(&reopened).unwrap();
    let label = labels
        .iter()
        .find(|l| l.id == label_rowid)
        .expect("label survived reopen");
    assert_eq!(label.name, "Persistent");
    assert_eq!(label.uuid.0, label_uuid);

    let sessions = list_sessions_from_db(&reopened).unwrap();
    let (_, session) = sessions
        .iter()
        .find(|(id, _)| *id == session_rowid)
        .expect("session survived reopen");
    assert_eq!(session.duration_secs, 720);
    assert_eq!(session.notes.as_deref(), Some("survive me"));
    assert_eq!(session.label_id, Some(label_rowid));

    let preset = list_presets_for_mode_from_db(&reopened, SessionMode::Timer)
        .unwrap()
        .into_iter()
        .find(|p| p.uuid.0 == preset_uuid)
        .expect("preset survived");
    assert_eq!(preset.name, "Persistent Preset");
    assert!(preset.is_starred);

    let guided = meditate_core::db::list_guided_files_from_db(&reopened)
        .unwrap()
        .into_iter()
        .find(|g| g.uuid.0 == guided_uuid)
        .expect("guided file survived");
    assert_eq!(guided.name, "Persistent Guided");

    let pattern = meditate_core::db::list_vibration_patterns_from_db(&reopened)
        .unwrap()
        .into_iter()
        .find(|p| p.uuid.0 == pattern_uuid)
        .expect("vibration pattern survived");
    assert_eq!(pattern.name, "Persistent Pulse");

    let bell_sound = reopened
        .list_bell_sounds()
        .unwrap()
        .into_iter()
        .find(|s| s.id == bell_sound_rowid)
        .expect("bell sound survived");
    assert_eq!(bell_sound.name, "Persistent Chime");
    assert!(!bell_sound.is_bundled);

    assert!(
        !reopened.list_interval_bells().unwrap().is_empty(),
        "interval bell survived",
    );
}
