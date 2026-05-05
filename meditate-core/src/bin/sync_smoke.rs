//! End-to-end smoke test for the sync event log: simulates two devices
//! authoring sessions/labels independently, then cross-replays their
//! event logs and verifies both converge to the same state.
//!
//! Run with: `cargo run --bin sync_smoke`
//!
//! Prints a step-by-step trace of what's happening so we can eyeball
//! the events table and the materialized cache. Uses in-memory DBs —
//! no files touched.

use meditate_core::db::{Database, Event, Session, SessionMode};

fn main() {
    println!("=== sync_smoke: two-device convergence demo ===\n");

    // Two fresh in-memory databases standing in for two devices.
    let phone = Database::open_in_memory().expect("open phone db");
    let laptop = Database::open_in_memory().expect("open laptop db");

    println!("Phone  device_id: {}", phone.device_id().unwrap());
    println!("Laptop device_id: {}\n", laptop.device_id().unwrap());

    // ── Round 1: each device authors independently ──────────────────────
    println!("--- Round 1: each device authors independently ---");

    // Phone: insert a label and a session referencing it.
    let morning_id = phone.insert_label("Morning").unwrap();
    let phone_morning_uuid = phone.list_labels().unwrap()[0].uuid.clone();
    phone.insert_session(&Session {
        start_iso: "2026-04-30T07:00:00".into(),
        duration_secs: 600,
        label_id: Some(morning_id),
        notes: Some("clear and present".into()),
        mode: SessionMode::Timer,
        uuid: String::new(),
        guided_file_uuid: None,
    }).unwrap();
    println!("Phone authored: 1 label (Morning), 1 session at 07:00 (600s)");

    // Laptop: insert a different label and session.
    let evening_id = laptop.insert_label("Evening").unwrap();
    laptop.insert_session(&Session {
        start_iso: "2026-04-30T20:00:00".into(),
        duration_secs: 1200,
        label_id: Some(evening_id),
        notes: None,
        mode: SessionMode::Timer,
        uuid: String::new(),
        guided_file_uuid: None,
    }).unwrap();
    println!("Laptop authored: 1 label (Evening), 1 session at 20:00 (1200s)");

    print_state("Phone after round 1", &phone);
    print_state("Laptop after round 1", &laptop);

    // ── Sync ───────────────────────────────────────────────────────────────
    println!("--- Cross-replay: each device replays the other's events ---");

    let events_from_phone = drain(&phone);
    let events_from_laptop = drain(&laptop);
    println!("Phone -> {} events to ship", events_from_phone.len());
    println!("Laptop -> {} events to ship", events_from_laptop.len());

    laptop.replay_events(&events_from_phone).unwrap();
    phone.replay_events(&events_from_laptop).unwrap();

    print_state("Phone after sync", &phone);
    print_state("Laptop after sync", &laptop);
    assert_states_match(&phone, &laptop);
    println!("✓ both devices converged on the same {} sessions / {} labels",
        phone.list_sessions().unwrap().len(),
        phone.list_labels().unwrap().len(),
    );

    // ── Round 2: concurrent edits to the SAME session ────────────────────
    println!("\n--- Round 2: concurrent edits, conflict resolution ---");

    // Use the Morning-tagged session as the contested one. Both devices
    // see it (it just synced).
    let morning_session_id_phone = phone.list_sessions().unwrap()
        .iter()
        .find(|(_, s)| s.notes.as_deref() == Some("clear and present"))
        .map(|(id, _)| *id)
        .expect("phone should have the Morning session post-sync");
    let morning_session_id_laptop = laptop.list_sessions().unwrap()
        .iter()
        .find(|(_, s)| s.notes.as_deref() == Some("clear and present"))
        .map(|(id, _)| *id)
        .expect("laptop should have the Morning session post-sync");

    // Both devices update the same session at roughly the same time.
    // Each emits its own event with its own lamport_ts (which is now > 0
    // because both devices have authored some events already).
    let new_label_phone = phone.list_labels().unwrap()[0].uuid.clone();
    println!("Phone lamport before edit:  {}", phone.lamport_clock().unwrap());
    println!("Laptop lamport before edit: {}", laptop.lamport_clock().unwrap());

    phone.update_session(morning_session_id_phone, &Session {
        start_iso: "2026-04-30T07:00:00".into(),
        duration_secs: 900,  // bumped by 5 minutes
        label_id: phone.list_labels().unwrap()
            .iter().find(|l| l.name == "Morning").map(|l| l.id),
        notes: Some("phone says: I extended this".into()),
        mode: SessionMode::Timer,
        uuid: String::new(),
        guided_file_uuid: None,
    }).unwrap();
    println!("Phone updated session: notes='phone says: I extended this', lamport now {}",
        phone.lamport_clock().unwrap());

    laptop.update_session(morning_session_id_laptop, &Session {
        start_iso: "2026-04-30T07:00:00".into(),
        duration_secs: 1200,  // bumped to 20 minutes
        label_id: laptop.list_labels().unwrap()
            .iter().find(|l| l.name == "Morning").map(|l| l.id),
        notes: Some("laptop says: I went longer!".into()),
        mode: SessionMode::Timer,
        uuid: String::new(),
        guided_file_uuid: None,
    }).unwrap();
    println!("Laptop updated session: notes='laptop says: I went longer!', lamport now {}",
        laptop.lamport_clock().unwrap());

    println!("\n  ↓ each device replays the other's edit ↓");
    let phone_event_batch = drain(&phone);
    let laptop_event_batch = drain(&laptop);
    laptop.replay_events(&phone_event_batch).unwrap();
    phone.replay_events(&laptop_event_batch).unwrap();

    print_state("Phone after concurrent-edit sync", &phone);
    print_state("Laptop after concurrent-edit sync", &laptop);
    assert_states_match(&phone, &laptop);
    let winning_notes = phone.list_sessions().unwrap()
        .iter()
        .find(|(_, s)| s.start_iso == "2026-04-30T07:00:00")
        .and_then(|(_, s)| s.notes.clone());
    println!("✓ both devices agree the winning notes are: {:?}", winning_notes);
    println!("  (whichever device had the higher lamport_ts at edit time wins;");
    println!("   on tie, the lex-larger device_id wins)");

    // Suppress unused-variable warning for the stable id captured for parity.
    let _ = (new_label_phone, phone_morning_uuid);

    // ── Round 3: tombstone test ──────────────────────────────────────────
    println!("\n--- Round 3: delete on one device, sync, tombstone wins ---");
    let evening_session_id = phone.list_sessions().unwrap()
        .iter()
        .find(|(_, s)| s.start_iso == "2026-04-30T20:00:00")
        .map(|(id, _)| *id)
        .expect("phone should have the Evening session");
    phone.delete_session(evening_session_id).unwrap();
    println!("Phone deleted the Evening session");

    let phone_delete_batch = drain(&phone);
    laptop.replay_events(&phone_delete_batch).unwrap();

    print_state("Phone after delete", &phone);
    print_state("Laptop after sync", &laptop);
    assert_states_match(&phone, &laptop);
    let evening_still_there = laptop.list_sessions().unwrap()
        .iter().any(|(_, s)| s.start_iso == "2026-04-30T20:00:00");
    assert!(!evening_still_there, "tombstone failed: Evening session still on laptop");
    println!("✓ tombstone propagated; both devices have {} session(s)",
        laptop.list_sessions().unwrap().len());

    // ── Round 4: settings sync ────────────────────────────────────────────
    println!("\n--- Round 4: settings sync ---");
    phone.set_setting("daily_goal_minutes", "20").unwrap();
    laptop.set_setting("daily_goal_minutes", "30").unwrap();
    println!("Phone set daily_goal_minutes=20, lamport {}", phone.lamport_clock().unwrap());
    println!("Laptop set daily_goal_minutes=30, lamport {}", laptop.lamport_clock().unwrap());

    let phone_setting_batch = drain(&phone);
    let laptop_setting_batch = drain(&laptop);
    laptop.replay_events(&phone_setting_batch).unwrap();
    phone.replay_events(&laptop_setting_batch).unwrap();

    println!("Phone  daily_goal_minutes after sync: {}",
        phone.get_setting("daily_goal_minutes", "?").unwrap());
    println!("Laptop daily_goal_minutes after sync: {}",
        laptop.get_setting("daily_goal_minutes", "?").unwrap());
    assert_eq!(
        phone.get_setting("daily_goal_minutes", "?").unwrap(),
        laptop.get_setting("daily_goal_minutes", "?").unwrap(),
        "settings must converge",
    );
    println!("✓ settings converged");

    // ── Round 5: three-device propagation through a hop ──────────────────
    println!("\n--- Round 5: three-device scenario, events transit a hop ---");
    let device_a = Database::open_in_memory().unwrap();
    let device_b = Database::open_in_memory().unwrap();
    let device_c = Database::open_in_memory().unwrap();
    println!("Three fresh devices: A, B, C");

    // A authors something. Sync to B.
    device_a.insert_session(&Session {
        start_iso: "2026-05-01T08:00:00".into(),
        duration_secs: 600, label_id: None,
        notes: Some("from A".into()),
        mode: SessionMode::Timer, uuid: String::new(),
        guided_file_uuid: None,
    }).unwrap();
    let from_a = drain(&device_a);
    device_b.replay_events(&from_a).unwrap();
    println!("A authored 'from A' session, B synced. A pending={}, B pending={}",
        device_a.pending_events().unwrap().len(),
        device_b.pending_events().unwrap().len());

    // B authors. Then sync B → C. C should pick up BOTH A's and B's events
    // (B's pending now includes A's previously-applied events, since they
    // haven't been marked synced from B's perspective).
    device_b.insert_session(&Session {
        start_iso: "2026-05-01T12:00:00".into(),
        duration_secs: 1200, label_id: None,
        notes: Some("from B".into()),
        mode: SessionMode::Timer, uuid: String::new(),
        guided_file_uuid: None,
    }).unwrap();
    let from_b_to_c = drain(&device_b);
    device_c.replay_events(&from_b_to_c).unwrap();
    println!("B authored 'from B', synced to C ({} events transited)", from_b_to_c.len());

    // C should now have BOTH sessions — A's transited via B.
    let c_notes: std::collections::HashSet<_> = device_c.list_sessions().unwrap()
        .iter().filter_map(|(_, s)| s.notes.clone()).collect();
    let expected_c: std::collections::HashSet<_> = ["from A", "from B"]
        .iter().map(|s| s.to_string()).collect();
    assert_eq!(c_notes, expected_c,
        "C must have A's session even though A and C never talked directly");
    println!("✓ C received A's session through the B hop (event identity preserved across forwarding)");

    // C authors and closes the loop back to A.
    device_c.insert_session(&Session {
        start_iso: "2026-05-01T18:00:00".into(),
        duration_secs: 900, label_id: None,
        notes: Some("from C".into()),
        mode: SessionMode::Timer, uuid: String::new(),
        guided_file_uuid: None,
    }).unwrap();
    let from_c = drain(&device_c);
    device_a.replay_events(&from_c).unwrap();

    let a_notes: std::collections::HashSet<_> = device_a.list_sessions().unwrap()
        .iter().filter_map(|(_, s)| s.notes.clone()).collect();
    let expected_all: std::collections::HashSet<_> = ["from A", "from B", "from C"]
        .iter().map(|s| s.to_string()).collect();
    assert_eq!(a_notes, expected_all,
        "A must end up with all three devices' sessions");
    println!("✓ A received B's and C's sessions; all three devices converged through hops");

    // Apply duplicates: re-replay events on a device that's already seen them.
    // After the cycle, C has all three sessions (from A via B, from B, and
    // from C itself). Re-replaying its own drained batch must dedup cleanly.
    let n_sessions_before = device_c.list_sessions().unwrap().len();
    device_c.replay_events(&from_c).unwrap();
    let n_sessions_after = device_c.list_sessions().unwrap().len();
    assert_eq!(n_sessions_before, n_sessions_after,
        "duplicate replay must not duplicate sessions (event_uuid dedup)");
    println!("✓ duplicate replay was a no-op ({} sessions before and after)", n_sessions_after);

    // ── Round 6: persistence across DB reopens ────────────────────────────
    println!("\n--- Round 6: persistence — close and reopen the file-based DB ---");
    let path = std::path::PathBuf::from("/tmp/sync-smoke-persistence.db");
    // Clean slate.
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(format!("{}-wal", path.display()));
    let _ = std::fs::remove_file(format!("{}-shm", path.display()));

    let (saved_device_id, saved_lamport, saved_session_uuid, saved_event_uuid);
    {
        let db = Database::open(&path).unwrap();
        saved_device_id = db.device_id().unwrap();
        db.insert_label("Persistent").unwrap();
        db.insert_session(&Session {
            start_iso: "2026-05-02T09:00:00".into(),
            duration_secs: 600, label_id: db.list_labels().unwrap()
                .iter().find(|l| l.name == "Persistent").map(|l| l.id),
            notes: Some("survives a restart".into()),
            mode: SessionMode::Timer, uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        saved_session_uuid = db.list_sessions().unwrap()[0].1.uuid.clone();
        saved_lamport = db.lamport_clock().unwrap();
        saved_event_uuid = db.pending_events().unwrap()[0].1.event_uuid.clone();
        println!("Pre-close: device_id={}, lamport={}, sessions={}, pending_events={}",
            &saved_device_id[..8],
            saved_lamport,
            db.list_sessions().unwrap().len(),
            db.pending_events().unwrap().len(),
        );
        // db drops here, closing the connection.
    }

    let db = Database::open(&path).unwrap();
    println!("Post-reopen: device_id={}, lamport={}, sessions={}, pending_events={}",
        &db.device_id().unwrap()[..8],
        db.lamport_clock().unwrap(),
        db.list_sessions().unwrap().len(),
        db.pending_events().unwrap().len(),
    );
    assert_eq!(db.device_id().unwrap(), saved_device_id,
        "device_id must persist across reopens");
    assert_eq!(db.lamport_clock().unwrap(), saved_lamport,
        "lamport clock must persist across reopens");
    assert_eq!(db.list_sessions().unwrap().len(), 1);
    assert_eq!(db.list_sessions().unwrap()[0].1.uuid, saved_session_uuid,
        "session uuid must be preserved");
    assert_eq!(db.list_labels().unwrap()[0].name, "Persistent");
    let pending_uuids: Vec<_> = db.pending_events().unwrap()
        .iter().map(|(_, e)| e.event_uuid.clone()).collect();
    assert!(pending_uuids.contains(&saved_event_uuid),
        "the unflushed event must still be in pending_events post-reopen");

    // After reopen, a new local write must continue from the persisted
    // lamport — not reset to 0.
    let lamport_before_new_write = db.lamport_clock().unwrap();
    db.set_setting("after_reopen", "yes").unwrap();
    assert_eq!(db.lamport_clock().unwrap(), lamport_before_new_write + 1,
        "post-reopen writes must continue the lamport sequence");
    println!("✓ device_id, lamport, sessions, labels, AND pending events all survive the reopen");

    // Cleanup the file we created.
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(format!("{}-wal", path.display()));
    let _ = std::fs::remove_file(format!("{}-shm", path.display()));

    // ── Round 7: out-of-order replay convergence ─────────────────────────
    println!("\n--- Round 7: same events, three different orderings → same state ---");

    // Build a stress batch: a label, two sessions, an update on one, a
    // delete on the other, a settings change. Lamport timestamps strewn
    // around so a naive in-arrival-order applier would diverge.
    let session_p = "44444444-4444-4444-8444-444444444444";
    let session_q = "55555555-5555-4555-8555-555555555555";
    let label_y   = "66666666-6666-4666-8666-666666666666";
    let foreign = "00000000-0000-4000-8000-ccccccccccccc";  // synthetic third device
    let _ = foreign;
    let device = "00000000-0000-4000-8000-cccccccccccc";

    let stress_batch = vec![
        // Label first in lamport order (must exist before sessions reference it).
        synth_label_event_insert(label_y, 1, device, "Stress"),
        synth_session_event_insert(session_p, 2, device, "P-init", 100, Some(label_y), None),
        synth_session_event_insert(session_q, 3, device, "Q-init", 200, None, None),
        synth_session_event_update(session_p, 7, device, "P-edit", 150, Some(label_y), Some("longer")),
        synth_session_event_delete(session_q, 8, device),
        synth_setting_change(device, "stretch_minutes", "5", 4),
        synth_setting_change(device, "stretch_minutes", "10", 9),
    ];

    // DB1: in lamport order.
    let db1 = Database::open_in_memory().unwrap();
    db1.replay_events(&stress_batch).unwrap();

    // DB2: reverse order.
    let mut reversed = stress_batch.clone();
    reversed.reverse();
    let db2 = Database::open_in_memory().unwrap();
    db2.replay_events(&reversed).unwrap();

    // DB3: a concrete shuffle (not random — deterministic so test failures
    // are reproducible). Indices: 4, 0, 6, 2, 5, 1, 3.
    let perm: [usize; 7] = [4, 0, 6, 2, 5, 1, 3];
    let shuffled: Vec<Event> = perm.iter().map(|&i| stress_batch[i].clone()).collect();
    let db3 = Database::open_in_memory().unwrap();
    db3.replay_events(&shuffled).unwrap();

    let state = |db: &Database| {
        // Project SessionMode to its db_str so the tuple is Ord-comparable.
        let mut sessions: Vec<_> = db.list_sessions().unwrap().into_iter()
            .map(|(_, s)| (s.uuid.clone(), s.start_iso.clone(), s.duration_secs,
                s.label_id.is_some(), s.notes.clone(), s.mode.as_db_str().to_string()))
            .collect();
        sessions.sort();
        let mut labels: Vec<_> = db.list_labels().unwrap().into_iter()
            .map(|l| (l.uuid.clone(), l.name.clone())).collect();
        labels.sort();
        let stretch = db.get_setting("stretch_minutes", "?").unwrap();
        (sessions, labels, stretch)
    };
    let s1 = state(&db1);
    let s2 = state(&db2);
    let s3 = state(&db3);
    println!("In-order:  {} sessions, {} labels, stretch={:?}",
        s1.0.len(), s1.1.len(), s1.2);
    println!("Reversed:  {} sessions, {} labels, stretch={:?}",
        s2.0.len(), s2.1.len(), s2.2);
    println!("Shuffled:  {} sessions, {} labels, stretch={:?}",
        s3.0.len(), s3.1.len(), s3.2);
    assert_eq!(s1, s2,
        "in-order and reversed must converge to the same materialized state");
    assert_eq!(s2, s3,
        "reversed and shuffled must converge");
    assert_eq!(s1.2, "10",
        "settings.stretch_minutes: lamport 9 must beat lamport 4");
    assert_eq!(s1.0.len(), 1,
        "session_q tombstoned at lamport 8 (insert was at 3) must be absent");
    println!("✓ all three orderings produced byte-identical materialized state");

    println!("\n=== sync_smoke: ALL CHECKS PASSED ===");
}

fn synth_label_event_insert(label_uuid: &str, lamport_ts: i64, device: &str, name: &str) -> Event {
    Event {
        event_uuid: format!("ev-li-{label_uuid}-{lamport_ts}"),
        lamport_ts,
        device_id: device.to_string(),
        kind: "label_insert".to_string(),
        target_id: label_uuid.to_string(),
        payload: format!(r#"{{"uuid":"{label_uuid}","name":"{name}"}}"#),
    }
}

fn synth_session_event_insert(
    session_uuid: &str, lamport_ts: i64, device: &str,
    start_iso: &str, duration_secs: u32,
    label_uuid: Option<&str>, notes: Option<&str>,
) -> Event {
    let payload = serde_json::json!({
        "uuid": session_uuid,
        "start_iso": start_iso,
        "duration_secs": duration_secs,
        "label_uuid": label_uuid,
        "notes": notes,
        "mode": "timer",
    }).to_string();
    Event {
        event_uuid: format!("ev-si-{session_uuid}-{lamport_ts}"),
        lamport_ts,
        device_id: device.to_string(),
        kind: "session_insert".to_string(),
        target_id: session_uuid.to_string(),
        payload,
    }
}

fn synth_session_event_update(
    session_uuid: &str, lamport_ts: i64, device: &str,
    start_iso: &str, duration_secs: u32,
    label_uuid: Option<&str>, notes: Option<&str>,
) -> Event {
    let payload = serde_json::json!({
        "uuid": session_uuid,
        "start_iso": start_iso,
        "duration_secs": duration_secs,
        "label_uuid": label_uuid,
        "notes": notes,
        "mode": "timer",
    }).to_string();
    Event {
        event_uuid: format!("ev-su-{session_uuid}-{lamport_ts}"),
        lamport_ts,
        device_id: device.to_string(),
        kind: "session_update".to_string(),
        target_id: session_uuid.to_string(),
        payload,
    }
}

fn synth_session_event_delete(session_uuid: &str, lamport_ts: i64, device: &str) -> Event {
    Event {
        event_uuid: format!("ev-sd-{session_uuid}-{lamport_ts}"),
        lamport_ts,
        device_id: device.to_string(),
        kind: "session_delete".to_string(),
        target_id: session_uuid.to_string(),
        payload: format!(r#"{{"uuid":"{session_uuid}"}}"#),
    }
}

fn synth_setting_change(device: &str, key: &str, value: &str, lamport_ts: i64) -> Event {
    Event {
        event_uuid: format!("ev-sc-{key}-{lamport_ts}"),
        lamport_ts,
        device_id: device.to_string(),
        kind: "setting_changed".to_string(),
        target_id: key.to_string(),
        payload: serde_json::json!({"key": key, "value": value}).to_string(),
    }
}

/// Drain every pending event off a device, returning them as plain
/// `Event` values for shipping to the other device. Marks them synced
/// locally so the next `pending_events` call only returns NEW work.
fn drain(db: &Database) -> Vec<Event> {
    let pending = db.pending_events().unwrap();
    let events: Vec<Event> = pending.iter().map(|(_, e)| e.clone()).collect();
    for (id, _) in pending {
        db.mark_event_synced(id).unwrap();
    }
    events
}

fn print_state(label: &str, db: &Database) {
    let sessions = db.list_sessions().unwrap();
    let labels = db.list_labels().unwrap();
    println!("  [{label}]: {} session(s), {} label(s), lamport={}",
        sessions.len(), labels.len(), db.lamport_clock().unwrap());
}

fn assert_states_match(a: &Database, b: &Database) {
    let mut a_sessions: Vec<_> = a.list_sessions().unwrap().into_iter()
        .map(|(_, s)| (s.uuid.clone(), s.start_iso.clone(), s.duration_secs, s.notes.clone()))
        .collect();
    let mut b_sessions: Vec<_> = b.list_sessions().unwrap().into_iter()
        .map(|(_, s)| (s.uuid.clone(), s.start_iso.clone(), s.duration_secs, s.notes.clone()))
        .collect();
    a_sessions.sort();
    b_sessions.sort();
    assert_eq!(a_sessions, b_sessions, "session sets diverged");

    let mut a_labels: Vec<_> = a.list_labels().unwrap().into_iter()
        .map(|l| (l.uuid.clone(), l.name.clone())).collect();
    let mut b_labels: Vec<_> = b.list_labels().unwrap().into_iter()
        .map(|l| (l.uuid.clone(), l.name.clone())).collect();
    a_labels.sort();
    b_labels.sort();
    assert_eq!(a_labels, b_labels, "label sets diverged");
}
