//! Test-only helpers shared across every entity submodule's `mod tests`
//! block. Synth-event constructors let tests hand-construct Event
//! values with pinned `lamport_ts` / `device_id` for tie-break and
//! out-of-order replay scenarios; the shared device + session uuid
//! constants keep cross-entity tests stable.
//!
//! All `pub(super)` — visible to db/* submodules' test code, never
//! to the wider crate.

#![cfg(test)]

use super::{Database, Event, SessionMode};

/// How many events `db` has authored or received and not yet pushed.
pub(crate) fn sent(db: &Database) -> usize {
    db.pending_events().unwrap().len()
}

/// `to` applies every event `from` has not pushed yet.
pub(crate) fn push(from: &Database, to: &Database) {
    let events: Vec<Event> = from.pending_events().unwrap().into_iter().map(|(_, e)| e).collect();
    to.replay_events(&events).unwrap();
}

/// Two devices sync both ways: each applies the other's events.
pub(crate) fn exchange(a: &Database, b: &Database) {
    push(a, b);
    push(b, a);
}

/// Hand-construct an event without going through a Database. Lets
/// tests pin specific lamport_ts / device_id values for tie-break
/// and out-of-order scenarios.
pub(super) fn synth_event(
    kind: &str,
    target_id: &str,
    lamport_ts: i64,
    device_id: &str,
    payload: serde_json::Value,
) -> Event {
    Event {
        event_uuid: uuid::Uuid::new_v4().to_string(),
        lamport_ts,
        device_id: device_id.to_string(),
        kind: kind.to_string(),
        target_id: target_id.to_string(),
        payload: payload.to_string(),
    }
}

pub(super) fn synth_session_insert(
    session_uuid: &str,
    lamport_ts: i64,
    device_id: &str,
    start_iso: &str,
    duration_secs: u32,
    label_uuid: Option<&str>,
    notes: Option<&str>,
    mode: SessionMode,
) -> Event {
    synth_event(
        "session_insert",
        session_uuid,
        lamport_ts,
        device_id,
        serde_json::json!({
            "uuid": session_uuid,
            "start_iso": start_iso,
            "duration_secs": duration_secs,
            "label_uuid": label_uuid,
            "notes": notes,
            "mode": mode.as_db_str(),
        }),
    )
}

pub(super) fn synth_session_update(
    session_uuid: &str,
    lamport_ts: i64,
    device_id: &str,
    start_iso: &str,
    duration_secs: u32,
    label_uuid: Option<&str>,
    notes: Option<&str>,
    mode: SessionMode,
) -> Event {
    synth_event(
        "session_update",
        session_uuid,
        lamport_ts,
        device_id,
        serde_json::json!({
            "uuid": session_uuid,
            "start_iso": start_iso,
            "duration_secs": duration_secs,
            "label_uuid": label_uuid,
            "notes": notes,
            "mode": mode.as_db_str(),
        }),
    )
}

pub(super) fn synth_session_delete(
    session_uuid: &str,
    lamport_ts: i64,
    device_id: &str,
) -> Event {
    synth_event(
        "session_delete",
        session_uuid,
        lamport_ts,
        device_id,
        serde_json::json!({ "uuid": session_uuid }),
    )
}

pub(super) const DEVICE_A: &str = "00000000-0000-4000-8000-aaaaaaaaaaaa";
pub(super) const DEVICE_B: &str = "00000000-0000-4000-8000-bbbbbbbbbbbb";
pub(super) const SESSION_X: &str = "11111111-1111-4111-8111-111111111111";
pub(super) const LABEL_X: &str = "22222222-2222-4222-8222-222222222222";

pub(super) fn synth_label_insert(
    label_uuid: &str,
    lamport_ts: i64,
    device: &str,
    name: &str,
) -> Event {
    synth_event(
        "label_insert",
        label_uuid,
        lamport_ts,
        device,
        serde_json::json!({ "uuid": label_uuid, "name": name }),
    )
}

pub(super) fn synth_label_rename(
    label_uuid: &str,
    lamport_ts: i64,
    device: &str,
    name: &str,
) -> Event {
    synth_event(
        "label_rename",
        label_uuid,
        lamport_ts,
        device,
        serde_json::json!({ "uuid": label_uuid, "name": name }),
    )
}

pub(super) fn synth_label_delete(label_uuid: &str, lamport_ts: i64, device: &str) -> Event {
    synth_event(
        "label_delete",
        label_uuid,
        lamport_ts,
        device,
        serde_json::json!({ "uuid": label_uuid }),
    )
}

pub(super) fn synth_setting_changed(
    key: &str,
    value: &str,
    lamport_ts: i64,
    device: &str,
) -> Event {
    synth_event(
        "setting_changed",
        key,
        lamport_ts,
        device,
        serde_json::json!({ "key": key, "value": value }),
    )
}

/// Mark every pending event synced. Many event-emission tests need
/// to "reset the event log" between assertions — emit something,
/// drain, emit again, observe the second emission in isolation.
pub(super) fn drain_events(db: &Database) {
    for (id, _) in db.pending_events().unwrap() {
        db.mark_events_synced(&[id]).unwrap();
    }
}

/// Parse an event's JSON payload into a generic `serde_json::Value`
/// for assertions. Avoids hardcoding a Rust struct per event kind in
/// the test surface — the payload contract IS the JSON shape.
pub(super) fn event_payload(event: &Event) -> serde_json::Value {
    serde_json::from_str(&event.payload)
        .unwrap_or_else(|e| panic!("payload `{}` is not valid JSON: {e}",
            event.payload))
}

/// A simple session-insert event built from one i64 seed. Used by
/// event-log tests that don't care about payload specifics — just
/// need distinct events with a known lamport_ts.
pub(super) fn sample_event(seed: i64) -> Event {
    let session_uuid = format!("00000000-0000-4000-9000-{seed:012x}");
    Event {
        event_uuid: format!("00000000-0000-4000-8000-{seed:012x}"),
        lamport_ts: seed,
        device_id: "00000000-0000-4000-8000-aaaaaaaaaaaa".to_string(),
        kind: "session_insert".to_string(),
        target_id: session_uuid.clone(),
        payload: format!("{{\"uuid\":\"{session_uuid}\",\"seed\":{seed}}}"),
    }
}

/// 8-4-4-4-12 hex with v4 marker and RFC 4122 variant. Cheap shape
/// check — used across entity tests to assert that a generated id
/// looks like the output of `Uuid::new_v4()` rather than (say) a
/// timestamp string or counter.
pub(super) fn looks_like_uuid_v4(s: &str) -> bool {
    if s.len() != 36 { return false; }
    let bytes = s.as_bytes();
    if bytes[8] != b'-' || bytes[13] != b'-' || bytes[18] != b'-' || bytes[23] != b'-' {
        return false;
    }
    if bytes[14] != b'4' { return false; }                 // version
    if !matches!(bytes[19], b'8' | b'9' | b'a' | b'b') {  // variant
        return false;
    }
    bytes.iter().enumerate().all(|(i, c)| {
        matches!(i, 8 | 13 | 18 | 23) || c.is_ascii_hexdigit()
    })
}

/// A database as v26.10.1 left it, with one session and one
/// library file.
pub(super) fn release_v26_10_1_database() -> rusqlite::Connection {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch(include_str!("fixtures/schema_v26_10_1.sql")).unwrap();
    conn.execute_batch(
        "PRAGMA user_version = 1;
         INSERT INTO sync_state (key, value) VALUES ('cache_schema_version', '2');
         INSERT INTO sessions (start_iso, duration_secs, mode, uuid)
             VALUES ('2026-10-01T07:00:00', 1200, 'guided', 's1');
         INSERT INTO guided_files (uuid, name, file_path, duration_secs, created_iso, updated_iso)
             VALUES ('g1', 'Body scan', 'guided/g1.ogg', 1380, '2026-10-01T00:00:00Z', '2026-10-01T00:00:00Z');",
    )
    .unwrap();
    conn
}

/// A database as v26.11.0 left it: library files g1 (imported first)
/// and g2, both starred, and the local folder /m with "b.ogg" and
/// "Sub/c.ogg" starred (by name: b, c; by path: Sub/c, b).
pub(crate) fn release_v26_11_0_database() -> super::Database {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch(include_str!("fixtures/schema_v26_11_0.sql")).unwrap();
    conn.execute_batch(
        "PRAGMA user_version = 1;
         INSERT INTO sync_state (key, value) VALUES ('cache_schema_version', '2');
         INSERT INTO guided_files (uuid, name, file_path, duration_secs, is_starred, created_iso, updated_iso)
             VALUES ('g2', 'Later', 'guided/g2.ogg', 600, 1, '2026-10-02T00:00:00Z', '2026-10-02T00:00:00Z'),
                    ('g1', 'Earlier', 'guided/g1.ogg', 900, 1, '2026-10-01T00:00:00Z', '2026-10-01T00:00:00Z');
         INSERT INTO sync_state (key, value) VALUES ('local_guided_folder', '/m');
         INSERT INTO local_guided_files (root, rel_path, size, mtime, is_starred)
             VALUES ('/m', 'b.ogg', 1, 1, 1), ('/m', 'Sub/c.ogg', 1, 1, 1);",
    )
    .unwrap();
    super::Database::init(conn).unwrap()
}

/// Run raw SQL on `db`, e.g. a trigger that makes a write fail.
pub(crate) fn sql(db: &Database, sql: &str) {
    db.conn.execute_batch(sql).unwrap();
}
