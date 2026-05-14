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
/// `sounds_dir` + `guided_dir` are per-device — `<tempdir>/<name>/<kind>`.
/// Sync push reads files from here; sync pull writes here.
fn sync_for<'a, W: meditate_core::sync::WebDav>(
    db: &'a Database,
    remote: &'a W,
    tempdir: &tempfile::TempDir,
    name: &str,
) -> Sync<'a, W> {
    let sounds_dir = tempdir.path().join(name).join("sounds");
    let guided_dir = tempdir.path().join(name).join("guided");
    std::fs::create_dir_all(&sounds_dir).expect("mkdir sounds");
    std::fs::create_dir_all(&guided_dir).expect("mkdir guided");
    Sync::new(db, remote, "Meditate", sounds_dir, guided_dir)
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

// ── Scenario 4b: custom guided-file audio push + pull ─────────────────────

#[test]
fn custom_guided_file_audio_round_trips_through_webdav() {
    // The guided-file equivalent of scenario 4. Same shape: A
    // authors a guided_files row + places the .ogg locally, syncs;
    // B (clean device) syncs and ends up with both the DB row AND
    // the audio bytes at `<guided_dir>/<uuid>.ogg`. Without the
    // dedicated transport the row would arrive but the audio
    // wouldn't — the bug this commit fixes.
    let tempdir = tempfile::tempdir().expect("tempdir");
    let remote = FakeWebDav::new();

    let a = open_test_db(&tempdir, "device_a");
    let guided_dir_a = tempdir.path().join("device_a").join("guided");
    std::fs::create_dir_all(&guided_dir_a).expect("mkdir guided A");

    let guided_uuid = "88888888-aaaa-bbbb-cccc-999999999999";
    let guided_bytes = b"<vorbis-stream-payload>".to_vec();
    let file_name = format!("{guided_uuid}.ogg");
    std::fs::write(guided_dir_a.join(&file_name), &guided_bytes)
        .expect("write A's local guided audio");

    a.insert_guided_file_with_uuid(
        guided_uuid,
        "Morning Body Scan",
        &file_name,
        // 12 minutes — realistic guided session length.
        720,
        true,
    )
    .expect("insert guided file");

    sync_for(&a, &remote, &tempdir, "device_a")
        .sync()
        .expect("device A push");

    // Sanity-check the remote received the audio at the expected path.
    let remote_audio_path = format!("/Meditate/guided/{guided_uuid}.ogg");
    let remote_audio = remote
        .get(&remote_audio_path, u64::MAX)
        .expect("guided audio on remote");
    assert_eq!(remote_audio, guided_bytes,
        "remote guided bytes must match what A wrote");

    let b = open_test_db(&tempdir, "device_b");
    sync_for(&b, &remote, &tempdir, "device_b")
        .sync()
        .expect("device B sync");

    let b_guided = meditate_core::db::list_guided_files_from_db(&b)
        .expect("list guided on B")
        .into_iter()
        .find(|g| g.uuid.0 == guided_uuid)
        .expect("guided row materialised on B");
    assert_eq!(b_guided.name, "Morning Body Scan");
    assert_eq!(b_guided.duration_secs, 720);

    // The .ogg landed at <guided_dir>/<uuid>.ogg.
    let guided_dir_b = tempdir.path().join("device_b").join("guided");
    let local_audio_b = guided_dir_b.join(&file_name);
    let pulled_bytes = std::fs::read(&local_audio_b)
        .expect("guided audio landed on B's disk");
    assert_eq!(pulled_bytes, guided_bytes,
        "pulled guided bytes must match A's original");
}

// ── Scenario 5: last-writer-wins on a concurrent label rename ─────────────

#[test]
fn last_writer_wins_when_two_devices_rename_the_same_label() {
    // Both devices have label X (after an initial sync). A renames
    // first and pushes. B pulls — its lamport clock now observes
    // A's rename ts. B then renames at a strictly higher ts and
    // pushes. A pulls. The recompute_label query picks the
    // MAX(lamport_ts, device_id) event, so B's rename must win on
    // both devices' final state.
    //
    // The ping-pong (A push → B pull → B rename → B push → A pull)
    // is what makes the outcome deterministic — without B observing
    // A's ts first, both renames would land at the same local
    // counter value and the winner would depend on device_id lex
    // order, which is randomly generated per Database.
    let tempdir = tempfile::tempdir().expect("tempdir");
    let remote = FakeWebDav::new();

    // Initial state: A creates the label, both devices converge.
    let a = open_test_db(&tempdir, "device_a");
    let label_rowid_a = a.insert_label("Original").expect("insert label");
    let label_uuid = list_labels_from_db(&a)
        .expect("list labels A")
        .into_iter()
        .find(|l| l.id == label_rowid_a)
        .expect("label exists on A")
        .uuid
        .0;
    sync_for(&a, &remote, &tempdir, "device_a").sync().expect("A initial sync");

    let b = open_test_db(&tempdir, "device_b");
    sync_for(&b, &remote, &tempdir, "device_b").sync().expect("B initial pull");
    let label_rowid_b = list_labels_from_db(&b)
        .expect("list labels B")
        .into_iter()
        .find(|l| l.uuid.0 == label_uuid)
        .expect("label exists on B")
        .id;

    // A renames first, pushes.
    a.update_label(label_rowid_a, "A-Wins")
        .expect("A rename");
    sync_for(&a, &remote, &tempdir, "device_a").push().expect("A push rename");

    // B pulls A's rename — B's lamport clock advances past A's ts.
    // Then B renames at a strictly higher ts and pushes.
    sync_for(&b, &remote, &tempdir, "device_b").pull().expect("B pull A");
    b.update_label(label_rowid_b, "B-Wins-Last")
        .expect("B rename");
    sync_for(&b, &remote, &tempdir, "device_b").push().expect("B push rename");

    // A pulls B's later rename. Both devices must agree on "B-Wins-Last".
    sync_for(&a, &remote, &tempdir, "device_a").pull().expect("A pull B");

    let final_a = list_labels_from_db(&a)
        .unwrap()
        .into_iter()
        .find(|l| l.uuid.0 == label_uuid)
        .expect("label still on A");
    let final_b = list_labels_from_db(&b)
        .unwrap()
        .into_iter()
        .find(|l| l.uuid.0 == label_uuid)
        .expect("label still on B");
    assert_eq!(final_a.name, "B-Wins-Last",
        "A's recompute must adopt B's higher-ts rename");
    assert_eq!(final_b.name, "B-Wins-Last",
        "B keeps its own rename");
    assert_eq!(final_a.name, final_b.name,
        "both devices converge");
}

// ── Scenario 6: wipe-and-recover (disaster recovery from remote) ──────────

#[test]
fn fresh_device_recovers_full_state_from_remote_via_sync() {
    // Janek's documented disaster-recovery flow: a device's local
    // DB is gone (uninstall + reinstall, factory reset, new phone).
    // The fix is to open a fresh Database at a new path and call
    // sync() — the remote event log + sound files must reconstruct
    // everything the lost device used to have. This pins down the
    // contract: opening on an empty path + one sync() = a working
    // device.
    //
    // Touches every entity kind so a regression in one recompute_*
    // surfaces here even if the others stay healthy.
    let tempdir = tempfile::tempdir().expect("tempdir");
    let remote = FakeWebDav::new();

    // Source-of-truth device authors one row of each entity kind
    // (plus a custom audio file) and pushes.
    let source = open_test_db(&tempdir, "source");
    let sounds_dir_source = tempdir.path().join("source").join("sounds");
    std::fs::create_dir_all(&sounds_dir_source).expect("mkdir source sounds");

    source.insert_label("Recovery-Test").expect("label");
    source.insert_preset_with_uuid(
        "aaaa1111-bbbb-2222-cccc-333333333333",
        "Recovery preset",
        SessionMode::Timer,
        true,
        r#"{"duration_min":15}"#,
    )
    .expect("preset");
    source.insert_guided_file_with_uuid(
        "bbbb1111-cccc-2222-dddd-444444444444",
        "Recovery guided",
        "guided/recovery.ogg",
        420,
        false,
    )
    .expect("guided file");
    let pattern_uuid = source
        .insert_vibration_pattern(
            "Recovery pulse",
            300,
            &[0.0, 1.0, 0.5, 0.0],
            ChartKind::Line,
            false,
        )
        .expect("pattern");
    let sound_uuid = "cccc1111-dddd-2222-eeee-555555555555";
    let sound_filename = format!("{sound_uuid}.ogg");
    let sound_bytes = b"<recovery-audio-bytes>".to_vec();
    std::fs::write(sounds_dir_source.join(&sound_filename), &sound_bytes)
        .expect("write source audio");
    source.insert_bell_sound_with_uuid(
        sound_uuid,
        "Recovery chime",
        &sound_filename,
        false,
        "audio/ogg",
        BellSoundCategory::General,
    )
    .expect("bell sound");
    source.insert_interval_bell(
        IntervalBellKind::Interval,
        7,
        10,
        sound_uuid,
        &pattern_uuid,
        SignalMode::Sound,
    )
    .expect("interval bell");
    let source_label_rowid = list_labels_from_db(&source)
        .unwrap()
        .into_iter()
        .find(|l| l.name == "Recovery-Test")
        .unwrap()
        .id;
    source
        .insert_session(&Session {
            start_iso: "2026-04-30T09:00:00".into(),
            duration_secs: 900,
            label_id: Some(source_label_rowid),
            notes: Some("recovery session".into()),
            mode: SessionMode::Timer,
            uuid: meditate_core::db::SessionUuid::new(""),
            guided_file_uuid: None,
        })
        .expect("session");
    sync_for(&source, &remote, &tempdir, "source")
        .sync()
        .expect("source push");

    // Recovery device: brand-new DB path, brand-new sounds_dir,
    // single sync() call.
    let fresh = open_test_db(&tempdir, "fresh");
    sync_for(&fresh, &remote, &tempdir, "fresh")
        .sync()
        .expect("fresh sync");

    // Every authored entity must be present on the fresh device.
    assert!(
        list_labels_from_db(&fresh)
            .unwrap()
            .iter()
            .any(|l| l.name == "Recovery-Test"),
        "label recovered",
    );
    assert!(
        list_presets_for_mode_from_db(&fresh, SessionMode::Timer)
            .unwrap()
            .iter()
            .any(|p| p.name == "Recovery preset"),
        "preset recovered",
    );
    assert!(
        meditate_core::db::list_guided_files_from_db(&fresh)
            .unwrap()
            .iter()
            .any(|g| g.name == "Recovery guided"),
        "guided file recovered",
    );
    assert!(
        meditate_core::db::list_vibration_patterns_from_db(&fresh)
            .unwrap()
            .iter()
            .any(|p| p.name == "Recovery pulse"),
        "vibration pattern recovered",
    );
    assert!(
        fresh
            .list_bell_sounds()
            .unwrap()
            .iter()
            .any(|s| s.name == "Recovery chime"),
        "bell sound row recovered",
    );
    assert!(
        !fresh.list_interval_bells().unwrap().is_empty(),
        "interval bell recovered",
    );
    let recovered_sessions = list_sessions_from_db(&fresh).unwrap();
    assert_eq!(recovered_sessions.len(), 1, "session recovered");
    let recovered_label_id = list_labels_from_db(&fresh)
        .unwrap()
        .into_iter()
        .find(|l| l.name == "Recovery-Test")
        .unwrap()
        .id;
    assert_eq!(
        recovered_sessions[0].1.label_id,
        Some(recovered_label_id),
        "recovered session keeps its label link",
    );

    // The custom audio file must land in the fresh device's local
    // sounds_dir — no audio file == bell rings silently.
    let sounds_dir_fresh = tempdir.path().join("fresh").join("sounds");
    let local_audio = sounds_dir_fresh.join(&sound_filename);
    let pulled_bytes = std::fs::read(&local_audio).expect("audio file recovered");
    assert_eq!(pulled_bytes, sound_bytes, "audio bytes match source");
}

// ── Scenario 7: session delete propagates across devices ──────────────────

#[test]
fn session_delete_on_a_propagates_to_b() {
    // The shell's delete-session button has to actually remove the
    // row on the user's other devices, not just the device they
    // tapped on. A `session_delete` event is emitted, and
    // recompute_session sees the delete is the MAX-ts event and
    // tombstones the row on replay.
    let tempdir = tempfile::tempdir().expect("tempdir");
    let remote = FakeWebDav::new();

    let a = open_test_db(&tempdir, "device_a");
    let session_rowid = a
        .insert_session(&Session {
            start_iso: "2026-04-30T10:00:00".into(),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: meditate_core::db::SessionUuid::new(""),
            guided_file_uuid: None,
        })
        .expect("insert session");
    sync_for(&a, &remote, &tempdir, "device_a").sync().expect("A push");

    // Confirm B sees the session after first sync — without this
    // assertion a later "session gone" check would pass trivially
    // even if the insert never reached B.
    let b = open_test_db(&tempdir, "device_b");
    sync_for(&b, &remote, &tempdir, "device_b").sync().expect("B initial pull");
    assert_eq!(
        list_sessions_from_db(&b).unwrap().len(),
        1,
        "session must land on B after first sync",
    );

    // A deletes, A pushes, B pulls. Session must be gone on B too.
    a.delete_session(session_rowid).expect("A delete");
    sync_for(&a, &remote, &tempdir, "device_a").push().expect("A push delete");
    sync_for(&b, &remote, &tempdir, "device_b").pull().expect("B pull delete");

    assert!(
        list_sessions_from_db(&b).unwrap().is_empty(),
        "session must be tombstoned on B",
    );
    // And A is unambiguously rid of it locally.
    assert!(
        list_sessions_from_db(&a).unwrap().is_empty(),
        "session must be gone on A",
    );
}

// ── Scenario 8: re-syncing with no local changes is a no-op ───────────────

#[test]
fn second_sync_with_no_local_changes_is_a_no_op() {
    // A bug class to guard against: sync() emits new events on every
    // call (re-inserting things it already knows about, or re-PUTing
    // bundles unnecessarily). Either would mean the event log grows
    // monotonically with sync calls regardless of user activity — a
    // slow leak that eventually pushes past size caps.
    let tempdir = tempfile::tempdir().expect("tempdir");
    let remote = FakeWebDav::new();

    let a = open_test_db(&tempdir, "device_a");
    a.insert_label("Idempotency").expect("label");
    a.insert_session(&Session {
        start_iso: "2026-04-30T11:00:00".into(),
        duration_secs: 300,
        label_id: None,
        notes: None,
        mode: SessionMode::Timer,
        uuid: meditate_core::db::SessionUuid::new(""),
        guided_file_uuid: None,
    })
    .expect("session");

    // First sync drains pending events and uploads the bundle.
    sync_for(&a, &remote, &tempdir, "device_a").sync().expect("first sync");
    let paths_after_first = remote.paths();
    let pending_after_first = a.pending_events().expect("pending").len();
    let labels_after_first = list_labels_from_db(&a).unwrap().len();
    let sessions_after_first = list_sessions_from_db(&a).unwrap().len();

    assert_eq!(pending_after_first, 0,
        "first sync must clear the pending-events queue");

    // Second sync with nothing changed locally.
    sync_for(&a, &remote, &tempdir, "device_a").sync().expect("second sync");

    assert_eq!(
        a.pending_events().expect("pending").len(),
        0,
        "second sync must not emit any new outgoing events",
    );
    assert_eq!(
        list_labels_from_db(&a).unwrap().len(),
        labels_after_first,
        "label count must not change across the no-op sync",
    );
    assert_eq!(
        list_sessions_from_db(&a).unwrap().len(),
        sessions_after_first,
        "session count must not change across the no-op sync",
    );
    assert_eq!(
        remote.paths(),
        paths_after_first,
        "remote file set must not grow on a no-op sync",
    );
}

// ── Scenario 9: A → B → C transitive propagation ──────────────────────────

#[test]
fn label_from_a_reaches_c_via_b_through_the_shared_remote() {
    // Confirms there's no "only my own events get rebroadcast" bug.
    // A authors a label and pushes; B pulls + authors a preset
    // referencing A's label uuid + pushes; C (which never spoke
    // directly to A) should see both rows after a single sync(),
    // with the preset's config_json still resolving to A's label
    // UUID — proving B forwarded A's event when B pushed its own
    // bundle.
    let tempdir = tempfile::tempdir().expect("tempdir");
    let remote = FakeWebDav::new();

    // A authors + pushes a label.
    let a = open_test_db(&tempdir, "device_a");
    let a_label_rowid = a.insert_label("Shared-Across-Chain").expect("A label");
    let label_uuid = list_labels_from_db(&a)
        .unwrap()
        .into_iter()
        .find(|l| l.id == a_label_rowid)
        .unwrap()
        .uuid
        .0;
    sync_for(&a, &remote, &tempdir, "device_a").sync().expect("A sync");

    // B pulls A's label, authors a preset referencing it, pushes.
    let b = open_test_db(&tempdir, "device_b");
    sync_for(&b, &remote, &tempdir, "device_b").sync().expect("B initial sync");
    let preset_uuid = "cccc2222-dddd-3333-eeee-444444444444";
    let preset_config = format!(r#"{{"label_uuid":"{}"}}"#, label_uuid);
    b.insert_preset_with_uuid(
        preset_uuid,
        "B-authored",
        SessionMode::Timer,
        false,
        &preset_config,
    )
    .expect("B preset");
    sync_for(&b, &remote, &tempdir, "device_b").sync().expect("B push preset");

    // C (which never talked to A) pulls everything via the remote.
    let c = open_test_db(&tempdir, "device_c");
    sync_for(&c, &remote, &tempdir, "device_c").sync().expect("C sync");

    // A's label reached C even though A never directly synced after
    // B's involvement — proving the bundle B pushed carried A's
    // event forward.
    assert!(
        list_labels_from_db(&c)
            .unwrap()
            .iter()
            .any(|l| l.uuid.0 == label_uuid && l.name == "Shared-Across-Chain"),
        "A's label must reach C via B's pushed bundle",
    );
    // B's preset reached C, AND its embedded label_uuid still
    // matches A's label — so the cross-device link is intact.
    let c_preset = list_presets_for_mode_from_db(&c, SessionMode::Timer)
        .unwrap()
        .into_iter()
        .find(|p| p.uuid.0 == preset_uuid)
        .expect("B's preset reaches C");
    assert!(
        c_preset.config_json.contains(&label_uuid),
        "preset config_json must still resolve to A's label UUID on C",
    );
}
