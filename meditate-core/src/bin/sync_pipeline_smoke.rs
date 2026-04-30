//! End-to-end smoke test for Phase D: exercises the full `Sync::sync`
//! pipeline (PROPFIND + GET + PUT + MKCOL + replay_events) against a
//! shared `FakeWebDav` standing in for a personal Nextcloud.
//!
//! Counterpart to `sync_smoke` — that one tests the lower-level
//! `replay_events` machinery directly. This one tests the public
//! `Sync` orchestration as a real app would use it.
//!
//! Run with: `cargo run --bin sync_pipeline_smoke`

use meditate_core::db::{Database, Session, SessionMode};
use meditate_core::sync::{FakeWebDav, HttpWebDav, Sync, SyncError, WebDavError};

fn main() {
    println!("=== sync_pipeline_smoke: Sync::sync() end-to-end demo ===\n");

    round_1_basic_two_device_sync();
    round_2_concurrent_edits_converge_over_two_sync_passes();
    round_3_tombstone_propagation();
    round_4_three_device_chain_through_shared_remote();
    round_5_idempotency_under_repeated_calls();
    round_6_http_webdav_against_unreachable_server_surfaces_network_error();

    println!("\n=== sync_pipeline_smoke: ALL CHECKS PASSED ===");
}

fn round_1_basic_two_device_sync() {
    println!("--- Round 1: A authors, syncs; B syncs and pulls A's data ---");
    let phone = Database::open_in_memory().unwrap();
    let laptop = Database::open_in_memory().unwrap();
    let nc = FakeWebDav::new();

    insert_session(&phone, "2026-04-30T07:00:00", 600, Some("phone-authored"));
    let phone_stats = Sync::new(&phone, &nc, "Meditate").sync().unwrap();
    println!("Phone synced: pulled={}, pushed={}", phone_stats.pulled, phone_stats.pushed);
    assert_eq!(phone_stats.pulled, 0);
    assert_eq!(phone_stats.pushed, 1);

    let laptop_stats = Sync::new(&laptop, &nc, "Meditate").sync().unwrap();
    println!("Laptop synced: pulled={}, pushed={}", laptop_stats.pulled, laptop_stats.pushed);
    assert_eq!(laptop_stats.pulled, 1, "laptop must pick up phone's session");

    let laptop_sessions = laptop.list_sessions().unwrap();
    assert_eq!(laptop_sessions.len(), 1);
    assert_eq!(laptop_sessions[0].1.notes.as_deref(), Some("phone-authored"));
    println!("✓ laptop materialised phone's session through the WebDAV pipeline\n");
}

fn round_2_concurrent_edits_converge_over_two_sync_passes() {
    println!("--- Round 2: concurrent edits, conflict-resolved through sync ---");
    let phone = Database::open_in_memory().unwrap();
    let laptop = Database::open_in_memory().unwrap();
    let nc = FakeWebDav::new();

    // Both devices start with the same shared session.
    let phone_session_id = insert_session(&phone, "shared", 600, None);
    Sync::new(&phone, &nc, "Meditate").sync().unwrap();
    Sync::new(&laptop, &nc, "Meditate").sync().unwrap();
    let laptop_session_id = laptop.list_sessions().unwrap()[0].0;
    println!("Both devices have the shared session");

    // Concurrent edits — neither has seen the other's update yet.
    update_session_notes(&phone, phone_session_id, "from phone");
    update_session_notes(&laptop, laptop_session_id, "from laptop");

    // Two sync passes per device for full convergence:
    // - Pass 1 each pushes its update; pass 2 each pulls the other's.
    for _ in 0..2 {
        Sync::new(&phone,  &nc, "Meditate").sync().unwrap();
        Sync::new(&laptop, &nc, "Meditate").sync().unwrap();
    }

    let phone_notes  = phone.list_sessions().unwrap()[0].1.notes.clone();
    let laptop_notes = laptop.list_sessions().unwrap()[0].1.notes.clone();
    assert_eq!(phone_notes, laptop_notes,
        "both devices must converge on the same winning value");
    println!("✓ both devices converged on notes={:?} (one device's edit won deterministically)\n",
        phone_notes);
}

fn round_3_tombstone_propagation() {
    println!("--- Round 3: phone deletes a session, laptop syncs and the row is gone ---");
    let phone = Database::open_in_memory().unwrap();
    let laptop = Database::open_in_memory().unwrap();
    let nc = FakeWebDav::new();

    let phone_session_id = insert_session(&phone, "to-be-deleted", 600, None);
    Sync::new(&phone,  &nc, "Meditate").sync().unwrap();
    Sync::new(&laptop, &nc, "Meditate").sync().unwrap();
    assert_eq!(laptop.list_sessions().unwrap().len(), 1);

    phone.delete_session(phone_session_id).unwrap();
    Sync::new(&phone, &nc, "Meditate").sync().unwrap();
    println!("Phone synced after delete; remote has the tombstone event");

    Sync::new(&laptop, &nc, "Meditate").sync().unwrap();
    assert!(laptop.list_sessions().unwrap().is_empty(),
        "laptop must drop the row after pulling the tombstone");
    println!("✓ tombstone propagated through Sync::sync\n");
}

fn round_4_three_device_chain_through_shared_remote() {
    println!("--- Round 4: three devices via the same shared FakeWebDav ---");
    let a = Database::open_in_memory().unwrap();
    let b = Database::open_in_memory().unwrap();
    let c = Database::open_in_memory().unwrap();
    let nc = FakeWebDav::new();

    insert_session(&a, "from A", 100, None);
    Sync::new(&a, &nc, "Meditate").sync().unwrap();

    insert_session(&b, "from B", 200, None);
    Sync::new(&b, &nc, "Meditate").sync().unwrap();

    // C joins late and pulls everything that's accumulated upstream.
    let c_stats = Sync::new(&c, &nc, "Meditate").sync().unwrap();
    println!("C joined late, sync stats: pulled={}, pushed={}",
        c_stats.pulled, c_stats.pushed);
    assert_eq!(c_stats.pulled, 2,
        "C must pick up both A's and B's events on first sync");

    let c_starts: std::collections::HashSet<String> = c.list_sessions().unwrap()
        .iter().map(|(_, s)| s.start_iso.clone()).collect();
    let expected: std::collections::HashSet<_> =
        ["from A", "from B"].iter().map(|s| s.to_string()).collect();
    assert_eq!(c_starts, expected);

    // C authors something of its own and goes back online — both A
    // and B should pull it on their next sync.
    insert_session(&c, "from C", 300, None);
    Sync::new(&c, &nc, "Meditate").sync().unwrap();
    Sync::new(&a, &nc, "Meditate").sync().unwrap();
    Sync::new(&b, &nc, "Meditate").sync().unwrap();
    let a_starts: std::collections::HashSet<String> = a.list_sessions().unwrap()
        .iter().map(|(_, s)| s.start_iso.clone()).collect();
    let b_starts: std::collections::HashSet<String> = b.list_sessions().unwrap()
        .iter().map(|(_, s)| s.start_iso.clone()).collect();
    let three_way: std::collections::HashSet<_> =
        ["from A", "from B", "from C"].iter().map(|s| s.to_string()).collect();
    assert_eq!(a_starts, three_way);
    assert_eq!(b_starts, three_way);
    println!("✓ three-device chain converged: all three peers see all three sessions\n");
}

fn round_5_idempotency_under_repeated_calls() {
    println!("--- Round 5: repeat sync() calls leave state stable ---");
    let phone = Database::open_in_memory().unwrap();
    let laptop = Database::open_in_memory().unwrap();
    let nc = FakeWebDav::new();

    for i in 0..3 {
        insert_session(&phone, &format!("phone-{i}"), 100, None);
    }
    // Pump until convergence (4 rounds is plenty; this measures
    // stability AFTER convergence).
    for _ in 0..4 {
        Sync::new(&phone,  &nc, "Meditate").sync().unwrap();
        Sync::new(&laptop, &nc, "Meditate").sync().unwrap();
    }
    let phone_count_converged  = phone.list_sessions().unwrap().len();
    let laptop_count_converged = laptop.list_sessions().unwrap().len();
    let remote_files_converged = nc.file_count();
    println!("After convergence: phone={} sessions, laptop={} sessions, remote={} files",
        phone_count_converged, laptop_count_converged, remote_files_converged);

    // Another 3 rounds — nothing should change.
    for _ in 0..3 {
        Sync::new(&phone,  &nc, "Meditate").sync().unwrap();
        Sync::new(&laptop, &nc, "Meditate").sync().unwrap();
    }
    assert_eq!(phone.list_sessions().unwrap().len(), phone_count_converged);
    assert_eq!(laptop.list_sessions().unwrap().len(), laptop_count_converged);
    assert_eq!(nc.file_count(), remote_files_converged,
        "post-convergence syncs must not append phantom remote files");
    println!("✓ further sync() calls were no-ops; state stable\n");
}

fn round_6_http_webdav_against_unreachable_server_surfaces_network_error() {
    println!("--- Round 6: real HttpWebDav against unreachable server ---");
    let phone = Database::open_in_memory().unwrap();
    insert_session(&phone, "to-not-be-pushed", 600, None);
    let pending_before = phone.pending_events().unwrap().len();
    println!("Phone has {pending_before} pending event(s) before failed sync");

    // 127.0.0.1:1 is reserved (tcpmux) — typically nothing listening.
    let unreachable = HttpWebDav::new("http://127.0.0.1:1", "u", "p");
    let result = Sync::new(&phone, &unreachable, "Meditate").sync();
    match result {
        Err(SyncError::WebDav(WebDavError::Network(msg))) => {
            println!("Sync failed as expected: Network({msg})");
        }
        other => panic!("expected Network error, got {other:?}"),
    }

    // Crucially: local state is untouched. Events are still pending
    // and ready to be retried against a reachable server.
    let pending_after = phone.pending_events().unwrap().len();
    assert_eq!(pending_after, pending_before,
        "failed sync must not consume / mark events");
    println!("✓ Network error surfaced cleanly; {pending_after} pending events still ready for retry\n");
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn insert_session(db: &Database, start_iso: &str, secs: u32, notes: Option<&str>) -> i64 {
    db.insert_session(&Session {
        start_iso: start_iso.into(),
        duration_secs: secs,
        label_id: None,
        notes: notes.map(|s| s.to_string()),
        mode: SessionMode::Countdown,
        uuid: String::new(),
    }).unwrap()
}

fn update_session_notes(db: &Database, id: i64, notes: &str) {
    let current = db.list_sessions().unwrap()
        .into_iter()
        .find(|(rid, _)| *rid == id)
        .map(|(_, s)| s)
        .expect("session exists");
    db.update_session(id, &Session {
        start_iso: current.start_iso,
        duration_secs: current.duration_secs,
        label_id: current.label_id,
        notes: Some(notes.to_string()),
        mode: current.mode,
        uuid: String::new(),
    }).unwrap();
}
