//! Test-only helpers shared across every entity submodule's `mod tests`
//! block. Synth-event constructors let tests hand-construct Event
//! values with pinned `lamport_ts` / `device_id` for tie-break and
//! out-of-order replay scenarios; the shared device + session uuid
//! constants keep cross-entity tests stable.
//!
//! All `pub(super)` — visible to db/* submodules' test code, never
//! to the wider crate.

#![cfg(test)]

use super::{Event, SessionMode};

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
/// Mirror of the shell-side BUNDLED_PATTERN_PULSE_UUID const. Kept
/// literal so the core tests don't have to plumb in the shell module.
pub(super) const BUNDLED_PATTERN_PULSE_UUID: &str =
    "7e9c4d2f-5a8b-4f1d-9e3c-2d6f7a8b0001";

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
