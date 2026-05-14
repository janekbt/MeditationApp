//! Cross-module integration tests for the sync round-trip path.
//!
//! Tests in `meditate-core/tests/` are compiled as a separate crate
//! that consumes `meditate_core` as a library. They only see `pub`
//! items, which forces the test to exercise the same surface a real
//! shell does — there's no shortcut through `pub(crate)` helpers.
//!
//! Each scenario covers a cross-module flow that the inline
//! `#[cfg(test)] mod tests` blocks can't exercise on their own:
//! event-log push/pull across two `Database` instances, persistence
//! across a close-reopen cycle, audio-file transport alongside the
//! event log. Together they raise the bar against silent regressions
//! in `apply_event_inner`'s dispatch and the orchestrator's
//! per-entity reconcilation.
//!
//! `FakeWebDav` is the shared "remote" — no network, no filesystem
//! syscalls past `tempfile`, so each scenario runs in tens of
//! milliseconds.

use meditate_core::db::{
    list_labels_from_db, list_presets_for_mode_from_db, list_sessions_from_db,
    BellSoundCategory, ChartKind, Event, IntervalBellKind, Session, SessionMode,
    SignalMode,
};
use meditate_core::sync::{FakeWebDav, Sync, WebDav};
use meditate_core::Database;

/// Open a fresh `Database` at a unique sub-path of `tempdir`, run
/// the non-audio seeds (default labels + presets + vibration
/// patterns + box-breath phases), and return it. Mirrors what the
/// gtk shell's `Database::open` does at startup.
fn open_test_db(tempdir: &tempfile::TempDir, name: &str) -> Database {
    let path = tempdir.path().join(format!("{name}.db"));
    let db = Database::open(&path).expect("open db");
    db.seed_all_non_audio().expect("seed");
    db
}

/// Build a `Sync` against the device's database + a shared remote.
/// `sounds_dir` is per-device — `<tempdir>/<name>/sounds`. Sync push
/// reads files from here; sync pull writes here.
fn sync_for<'a, W: meditate_core::sync::WebDav>(
    db: &'a Database,
    remote: &'a W,
    tempdir: &tempfile::TempDir,
    name: &str,
) -> Sync<'a, W> {
    let sounds_dir = tempdir.path().join(name).join("sounds");
    std::fs::create_dir_all(&sounds_dir).expect("mkdir sounds");
    Sync::new(db, remote, "Meditate", sounds_dir)
}

// ── Scenario 1: cross-entity round-trip across two devices ────────────────

#[test]
fn label_then_preset_round_trip_keeps_uuid_reference_intact() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let remote = FakeWebDav::new();

    // Device A: author a label + a preset whose config_json embeds
    // the label's UUID, then push.
    let a = open_test_db(&tempdir, "device_a");
    let label_rowid = a.insert_label("Morning").expect("insert label");
    let label = list_labels_from_db(&a)
        .expect("list labels")
        .into_iter()
        .find(|l| l.id == label_rowid)
        .expect("label survives the round-trip");
    let preset_uuid = "11111111-aaaa-bbbb-cccc-222222222222";
    let preset_config = format!(r#"{{"label_uuid":"{}","duration_min":10}}"#, label.uuid.0);
    a.insert_preset_with_uuid(
        preset_uuid,
        "Quick sit",
        SessionMode::Timer,
        true,
        &preset_config,
    )
    .expect("insert preset");
    sync_for(&a, &remote, &tempdir, "device_a")
        .push()
        .expect("device A push");

    // Drop A's handle to prove persistence across a close-reopen
    // doesn't depend on lingering in-memory state.
    drop(a);

    // Device B (clean DB) pulls from the same remote.
    let b = open_test_db(&tempdir, "device_b");
    sync_for(&b, &remote, &tempdir, "device_b")
        .pull()
        .expect("device B pull");

    // The label exists locally on B, addressable by its
    // cross-device UUID — the row's local rowid will differ.
    let b_label = list_labels_from_db(&b)
        .expect("list labels on B")
        .into_iter()
        .find(|l| l.uuid.0 == label.uuid.0)
        .expect("label materialised on B with the same UUID");
    assert_eq!(b_label.name, "Morning");

    // The preset exists on B with its config_json intact — the
    // embedded label_uuid still resolves to a real label.
    let b_preset = list_presets_for_mode_from_db(&b, SessionMode::Timer)
        .expect("list presets")
        .into_iter()
        .find(|p| p.uuid.0 == preset_uuid)
        .expect("preset materialised on B");
    assert!(
        b_preset.config_json.contains(&label.uuid.0),
        "preset config_json must still reference the label UUID, got {:?}",
        b_preset.config_json,
    );
    assert_eq!(b_preset.name, "Quick sit");

    // Re-open A's DB — same assertions hold, proving the on-disk
    // state matches what B sees.
    let a2 = open_test_db(&tempdir, "device_a");
    let a2_label = list_labels_from_db(&a2)
        .expect("list labels on reopened A")
        .into_iter()
        .find(|l| l.uuid.0 == label.uuid.0)
        .expect("label survives close-reopen on A");
    assert_eq!(a2_label.name, "Morning");
}

// ── Scenario 2: out-of-order delivery within the same batch ────────────────

#[test]
fn session_arriving_before_its_label_in_a_batch_links_after_recompute() {
    // Real-world trigger: a peer authored two events (label_insert
    // at ts=1, session_insert referencing the label at ts=2). The
    // bundle file's serialisation order is lamport-ts ascending, so
    // a healthy pull replays label-then-session. The defence here:
    // even when something REVERSES the order (a corrupt batch, a
    // pending_events query bug, a test-only manipulation), the
    // recompute_label pass triggered when the label finally lands
    // must re-link sessions whose latest mutate event references
    // its UUID.
    let tempdir = tempfile::tempdir().expect("tempdir");

    // Device A authors the events through the normal CRUD path so
    // the event payloads have the production shape.
    let a = open_test_db(&tempdir, "device_a");
    // Seeding default labels/presets emits events too; remember the
    // baseline count so we can slice off just the two events we
    // care about for the out-of-order replay.
    let pre_insert_count = a.pending_events().expect("baseline pending").len();
    let label_rowid = a.insert_label("Pre-coffee").expect("insert label");
    let a_label = list_labels_from_db(&a)
        .expect("list labels")
        .into_iter()
        .find(|l| l.id == label_rowid)
        .expect("label exists");
    a.insert_session(&Session {
        start_iso: "2026-04-30T07:00:00".into(),
        duration_secs: 600,
        label_id: Some(label_rowid),
        notes: None,
        mode: SessionMode::Timer,
        uuid: meditate_core::db::SessionUuid::new(""),
        guided_file_uuid: None,
    })
    .expect("insert session");

    // Pull the two new pending events out (slicing off the seed
    // events), then reverse the order so the session-insert lands
    // BEFORE its label-insert.
    let pending = a.pending_events().expect("pending events");
    let new_events: Vec<Event> = pending
        .into_iter()
        .skip(pre_insert_count)
        .map(|(_, e)| e)
        .collect();
    assert_eq!(new_events.len(), 2, "label-insert + session-insert");
    let mut events = new_events;
    events.reverse();
    assert!(events[0].kind.contains("session"));
    assert!(events[1].kind.contains("label"));

    // Replay reversed onto device B. After the label finally lands,
    // recompute_label re-runs the per-session FK link via the
    // event log.
    let b = open_test_db(&tempdir, "device_b");
    b.replay_events(&events).expect("replay reversed batch");

    let b_label = list_labels_from_db(&b)
        .expect("list labels on B")
        .into_iter()
        .find(|l| l.uuid.0 == a_label.uuid.0)
        .expect("label materialised");
    let sessions = list_sessions_from_db(&b).expect("list sessions on B");
    assert_eq!(sessions.len(), 1);
    let session = &sessions[0].1;
    assert_eq!(
        session.label_id,
        Some(b_label.id),
        "session.label_id must re-link to B's local label rowid \
         even though the events arrived out of order",
    );
}

// ── Scenario 3: dispatch coverage for every entity-mutation kind ───────────

#[test]
fn replay_events_covers_every_entity_kind_in_one_batch() {
    // The 9-arm-ish `apply_event_inner` dispatch: each entity kind
    // (Session, Label, IntervalBell, BellSound, Preset, GuidedFile,
    // VibrationPattern) has its own recompute_*. Per-arm unit tests
    // catch individual breakages; this test catches a regression
    // where the dispatch table itself loses an arm — the kind would
    // be silently recorded but never materialised. Touch every
    // entity type via the production CRUD path so the event
    // payloads are exactly what the orchestrator emits in the wild.
    let tempdir = tempfile::tempdir().expect("tempdir");
    let a = open_test_db(&tempdir, "device_a");

    // Insert one of each. UUIDs come back via the row-listing paths.
    a.insert_label("Focus").expect("label");
    a.insert_session(&Session {
        start_iso: "2026-04-30T08:00:00".into(),
        duration_secs: 300,
        label_id: None,
        notes: Some("morning sit".into()),
        mode: SessionMode::Timer,
        uuid: meditate_core::db::SessionUuid::new(""),
        guided_file_uuid: None,
    })
    .expect("session");
    a.insert_bell_sound(
        "Custom Chime",
        "custom-chime.ogg",
        false,
        "audio/ogg",
        BellSoundCategory::General,
    )
    .expect("bell sound");
    // Use the freshly-inserted custom sound + a bundled pattern as
    // the interval bell's refs. is_bundled=false means it rides
    // sync's custom-file path; for this test we only care that the
    // EVENT materialises on B, so the file's absence is fine.
    let custom_sound_uuid = a
        .list_bell_sounds()
        .expect("list bell sounds")
        .into_iter()
        .find(|s| s.name == "Custom Chime")
        .expect("bell sound row")
        .uuid
        .0;
    let pattern_uuid = a
        .insert_vibration_pattern(
            "Test Pulse",
            500,
            &[0.0, 1.0, 0.0],
            ChartKind::Line,
            false,
        )
        .expect("vibration pattern");
    a.insert_interval_bell(
        IntervalBellKind::Interval,
        5,
        20,
        &custom_sound_uuid,
        &pattern_uuid,
        SignalMode::Sound,
    )
    .expect("interval bell");
    a.insert_preset_with_uuid(
        "33333333-aaaa-bbbb-cccc-444444444444",
        "Whole-batch preset",
        SessionMode::Timer,
        false,
        r#"{"duration_min":10}"#,
    )
    .expect("preset");
    a.insert_guided_file_with_uuid(
        "55555555-aaaa-bbbb-cccc-666666666666",
        "Body scan 10",
        "guided/body-scan-10.ogg",
        600,
        false,
    )
    .expect("guided file");

    // Snapshot every event A produced. Each entity type contributes
    // at least one insert event; the test asserts they all reach B.
    let events: Vec<Event> = a
        .pending_events()
        .expect("pending")
        .into_iter()
        .map(|(_, e)| e)
        .collect();
    let kinds: std::collections::HashSet<&str> =
        events.iter().map(|e| e.kind.as_str()).collect();
    for required in [
        "label_insert",
        "session_insert",
        "bell_sound_insert",
        "vibration_pattern_insert",
        "interval_bell_insert",
        "preset_insert",
        "guided_file_insert",
    ] {
        assert!(kinds.contains(required),
            "A must emit {required}; got {kinds:?}");
    }

    // Replay onto a clean device B and assert every entity table
    // got the row.
    let b = open_test_db(&tempdir, "device_b");
    b.replay_events(&events).expect("replay");

    assert!(
        list_labels_from_db(&b)
            .unwrap()
            .iter()
            .any(|l| l.name == "Focus"),
        "label row materialised",
    );
    assert!(
        !list_sessions_from_db(&b).unwrap().is_empty(),
        "session row materialised",
    );
    assert!(
        b.list_bell_sounds()
            .unwrap()
            .iter()
            .any(|s| s.name == "Custom Chime"),
        "bell_sound row materialised",
    );
    assert!(
        meditate_core::db::list_vibration_patterns_from_db(&b)
            .unwrap()
            .iter()
            .any(|p| p.name == "Test Pulse"),
        "vibration_pattern row materialised",
    );
    assert!(
        !b.list_interval_bells().unwrap().is_empty(),
        "interval_bell row materialised",
    );
    assert!(
        meditate_core::db::list_presets_for_mode_from_db(&b, SessionMode::Timer)
            .unwrap()
            .iter()
            .any(|p| p.name == "Whole-batch preset"),
        "preset row materialised",
    );
    assert!(
        meditate_core::db::list_guided_files_from_db(&b)
            .unwrap()
            .iter()
            .any(|g| g.name == "Body scan 10"),
        "guided_file row materialised",
    );
}

// ── Scenario 4: custom bell-sound audio file push + pull ───────────────────

#[test]
fn custom_bell_sound_audio_file_round_trips_through_webdav() {
    // The orchestrator's audio half: a custom (non-bundled)
    // bell_sound row triggers an audio-file PUT on push and a GET
    // on pull. Bundled rows skip the file transport entirely (every
    // device compiles them in via GResource). This scenario covers
    // the round-trip end-to-end: file on disk → remote → file on
    // disk again, with the local DB rows linked by UUID.
    let tempdir = tempfile::tempdir().expect("tempdir");
    let remote = FakeWebDav::new();

    // Device A: author the bell-sound row + place the file at the
    // path push() reads from. The relative file_path stored in the
    // DB is what the orchestrator uses when scanning for the
    // local-side bytes — `<sounds_dir>/<uuid>.<ext>`.
    let a = open_test_db(&tempdir, "device_a");
    let sounds_dir_a = tempdir.path().join("device_a").join("sounds");
    std::fs::create_dir_all(&sounds_dir_a).expect("mkdir sounds A");

    let sound_uuid = "77777777-aaaa-bbbb-cccc-888888888888";
    let sound_bytes = b"<wav-bytes-go-here>".to_vec();
    let file_name = format!("{sound_uuid}.wav");
    std::fs::write(sounds_dir_a.join(&file_name), &sound_bytes)
        .expect("write A's local audio file");

    a.insert_bell_sound_with_uuid(
        sound_uuid,
        "My Chime",
        &file_name,
        false, // is_bundled = false → rides the custom-file sync path
        "audio/wav",
        BellSoundCategory::General,
    )
    .expect("insert custom bell sound");

    // Push: event + audio file land on the remote.
    sync_for(&a, &remote, &tempdir, "device_a")
        .sync()
        .expect("device A push");

    // Sanity-check the remote actually got the audio file (not just
    // the event bundle).
    let remote_audio_path = format!("/Meditate/sounds/{sound_uuid}.wav");
    let remote_audio = remote
        .get(&remote_audio_path, u64::MAX)
        .expect("audio file on remote");
    assert_eq!(remote_audio, sound_bytes,
        "remote audio bytes must match what A wrote");

    // Device B: clean DB, syncs, pulls both the event AND the
    // audio file.
    let b = open_test_db(&tempdir, "device_b");
    sync_for(&b, &remote, &tempdir, "device_b")
        .sync()
        .expect("device B sync");

    // The bell_sound row materialised under the same UUID.
    let b_sound = b
        .list_bell_sounds()
        .expect("list bell sounds on B")
        .into_iter()
        .find(|s| s.uuid.0 == sound_uuid)
        .expect("bell sound row materialised on B");
    assert_eq!(b_sound.name, "My Chime");
    assert!(!b_sound.is_bundled);

    // The audio file landed in B's local sounds_dir under the same
    // <uuid>.<ext> filename.
    let sounds_dir_b = tempdir.path().join("device_b").join("sounds");
    let local_audio_b = sounds_dir_b.join(&file_name);
    let pulled_bytes = std::fs::read(&local_audio_b)
        .expect("audio file landed on B's disk");
    assert_eq!(pulled_bytes, sound_bytes,
        "pulled audio bytes must match A's original");
}

