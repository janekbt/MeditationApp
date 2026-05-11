use rusqlite::{params, Connection, OptionalExtension};
use std::io::{Read, Write};
use std::path::Path;

mod bell_sounds;
mod box_breath_phases;
mod device;
mod error;
mod events;
mod guided_files;
mod interval_bells;
mod known_remote;
mod labels;
mod presets;
mod schema;
mod seeds;
mod sessions;
mod settings;
mod sync_state;
mod vibration_patterns;

#[cfg(test)]
mod test_helpers;

pub use bell_sounds::{BellSound, BellSoundCategory};
pub use box_breath_phases::{BoxBreathPhase, BoxBreathPhaseId};
pub use events::Event;
pub use guided_files::GuidedFile;
pub use interval_bells::{IntervalBell, IntervalBellKind};
pub use labels::Label;
pub use presets::Preset;
pub use sessions::{Session, SessionFilter, SessionMode};
pub use vibration_patterns::{ChartKind, VibrationPattern};
pub use error::{target_id_is_well_formed_for, DbError, Result};
pub use schema::{CACHE_SCHEMA_VERSION, CACHE_SCHEMA_VERSION_KEY, SCHEMA_VERSION};
use error::{conflict_suffixed_name, is_unique_constraint_error};
use schema::SCHEMA;

/// One audio file in the bell-sound library — bundled CC0 sounds the
/// app ships with, plus user-imported custom files. Referenced by
/// every bell-fire site (starting bell, interval bells, completion
/// sound) via the `uuid` column. The `is_bundled` flag distinguishes
/// what the audio system does with `file_path`: bundled rows hold a
/// GResource path the binary contains; custom rows hold a filesystem
/// path under `$XDG_DATA_HOME`. Bundled rows ride sync (so a peer
/// without the bundle inherits the same UUIDs from the seeding device)
/// but the audio itself doesn't — peers compile in their own copy.
///
/// `category` partitions the library by usage context: General sounds
/// (bells / gongs / chimes) feed the Starting / Interval / End bell
/// choosers; BoxBreath sounds (voice cues / phase markers) feed the
/// One configured bell entry in the user's interval-bell library.
/// What channels a bell or phase plays through. Mirrors the
/// Sound / Vibration / Both `Adw.ToggleGroup` segments in the
/// timer setup. Used as the persisted enum behind every per-bell
/// signal-mode setting key + the `interval_bells.signal_mode`
/// column + the `box_breath_phases.signal_mode` column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalMode {
    Sound,
    Vibration,
    Both,
}

/// One row in `box_breath_phases` — the per-phase cue config for
impl SignalMode {
    pub fn as_db_str(self) -> &'static str {
        match self {
            SignalMode::Sound     => "sound",
            SignalMode::Vibration => "vibration",
            SignalMode::Both      => "both",
        }
    }
    pub fn from_db_str(s: &str) -> Option<Self> {
        match s {
            "sound"     => Some(SignalMode::Sound),
            "vibration" => Some(SignalMode::Vibration),
            "both"      => Some(SignalMode::Both),
            _           => None,
        }
    }

    /// Does this mode include the sound channel? `Sound | Both`.
    pub fn includes_sound(self) -> bool {
        matches!(self, SignalMode::Sound | SignalMode::Both)
    }

    /// Does this mode include the vibration channel? `Vibration | Both`.
    pub fn includes_vibration(self) -> bool {
        matches!(self, SignalMode::Vibration | SignalMode::Both)
    }
}


/// One entry in the append-only sync event log. A self-contained
/// description of a state-changing operation — sessions inserted /
/// updated / deleted, labels renamed, settings changed. Every field
/// is part of the cross-device identity or ordering contract:
///
/// - `event_uuid` is the dedup key. Receiving the same uuid twice
///   (retry, peer-forwarding) is a silent no-op.
/// - `lamport_ts` orders events; ties break on `device_id` per the
///   conflict-resolution rules.
/// - `device_id` records authorship.
pub struct Database {
    conn: Connection,
}

/// Mint a fresh v4 UUID. Exposed so the shell (which doesn't have
/// the `uuid` crate as a direct dep) can generate UUIDs for places
/// where the id has to be known before the row is created — e.g.,
/// the custom-bell-import path that needs a UUID for the destination
/// filename before the DB insert.
pub fn mint_uuid() -> String {
    uuid::Uuid::new_v4().to_string()
}


impl Database {
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        Self::init(conn)
    }

    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        // For on-disk databases, enable WAL with synchronous=NORMAL.
        // The default (rollback journal + synchronous=FULL) does a
        // full fsync on every commit — autocommit UPDATEs become
        // ~50–200 ms each on phone eMMC, which bottlenecks any
        // hot-loop write. WAL+NORMAL fsyncs only on checkpoint and
        // the WAL header on commit, two orders of magnitude cheaper.
        // Durability tradeoff: a power loss between commit and
        // checkpoint may roll back a small number of recently
        // committed transactions. Acceptable here — events are
        // append-only and idempotent on re-sync.
        //
        // In-memory `open_in_memory` skips this — WAL on `:memory:` is
        // a no-op (the journal is also in memory) and synchronous
        // doesn't apply.
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")?;
        Self::init(conn)
    }

    fn init(conn: Connection) -> Result<Self> {
        // Refuse to open a DB whose user_version exceeds what this
        // build knows how to read — a downgrade from a future build
        // could otherwise drop forward-only data silently. A fresh
        // DB reads user_version=0 and falls through to the stamp.
        let db_version: u32 = conn.query_row(
            "PRAGMA user_version", [], |row| row.get(0),
        )?;
        if db_version > SCHEMA_VERSION {
            return Err(DbError::SchemaVersionTooNew {
                db: db_version,
                build: SCHEMA_VERSION,
            });
        }
        // Explicit PRAGMAs — even when rusqlite enables them by default,
        // the intent is part of the source so it can't be silently
        // dropped by a dependency upgrade. The FK clause on
        // sessions.label_id only fires when this is ON.
        conn.execute_batch("PRAGMA foreign_keys=ON;")?;
        conn.execute_batch(SCHEMA)?;
        // Stamp the current version. `execute_batch` is required because
        // PRAGMA values aren't bindable via params.
        conn.execute_batch(&format!("PRAGMA user_version = {SCHEMA_VERSION};"))?;
        // One-shot integrity check. `quick_check` returns "ok" on a
        // clean DB or one or more issue lines on corruption — we log
        // the first non-ok line via diag and continue, since refusing
        // to open would lock the user out without a recovery path.
        let integrity: String = conn.query_row(
            "PRAGMA quick_check", [], |row| row.get(0),
        ).unwrap_or_else(|_| "ok".to_string());
        if integrity != "ok" {
            crate::diag::log(&format!("db_integrity_check_failed: {integrity}"));
        }
        let db = Self { conn };
        db.maybe_walk_events_for_cache_upgrade()?;
        Ok(db)
    }

}

#[cfg(test)]
mod tests {
    use super::*;
    use super::test_helpers::*;

    // ── Cache schema version + walk-on-upgrade ────────────────────────

    #[test]
    fn fresh_open_in_memory_stamps_cache_schema_version_to_current() {
        // First-ever open: no events, but the marker still lands so
        // subsequent opens take the fast path and skip the walk.
        let db = Database::open_in_memory().unwrap();
        let stored = db
            .get_sync_state(CACHE_SCHEMA_VERSION_KEY, "missing")
            .unwrap();
        assert_eq!(stored, CACHE_SCHEMA_VERSION.to_string());
    }

    #[test]
    fn re_open_with_old_cache_version_walks_events_and_rematerialises_cache() {
        // Simulate a DB that was last opened by a build whose
        // apply_event_inner skipped some kind we now understand. The
        // walk-on-upgrade must re-apply every event so the cache
        // catches up to the current dispatch, then stamp the new
        // cache version to gate future fast paths.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("walk.db");
        {
            let db = Database::open(&path).unwrap();
            // Author some events the normal way.
            db.insert_label("Focus").unwrap();
            db.insert_label("Calm").unwrap();
            assert_eq!(db.list_labels().unwrap().len(), 2);
            // Manually delete the cache rows AND roll the cache
            // version back to 0 — pretending the previous build had
            // recorded these events without materialising them.
            db.conn.execute("DELETE FROM labels", []).unwrap();
            db.set_sync_state(CACHE_SCHEMA_VERSION_KEY, "0").unwrap();
            assert!(db.list_labels().unwrap().is_empty(),
                "labels cache must be empty before the re-open");
        }
        // Reopen. init must walk and re-materialise the labels.
        let db = Database::open(&path).unwrap();
        let labels = db.list_labels().unwrap();
        assert_eq!(labels.len(), 2, "labels must be re-materialised from event log");
        let mut names: Vec<_> = labels.iter().map(|l| l.name.clone()).collect();
        names.sort();
        assert_eq!(names, vec!["Calm".to_string(), "Focus".to_string()]);
        // Marker advances so the next open skips the walk.
        assert_eq!(
            db.get_sync_state(CACHE_SCHEMA_VERSION_KEY, "missing").unwrap(),
            CACHE_SCHEMA_VERSION.to_string(),
        );
    }

    #[test]
    fn re_open_at_current_cache_version_does_not_re_walk() {
        // Fast path: a DB already at the current cache version must
        // skip the walk so re-opens stay O(1) regardless of how many
        // events the log holds.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fast.db");
        {
            let db = Database::open(&path).unwrap();
            db.insert_label("Focus").unwrap();
            // Manually corrupt the cache and leave the marker at
            // current — a re-walk would fix this; the fast path must
            // therefore leave it broken (proving the walk was
            // skipped).
            db.conn.execute("DELETE FROM labels", []).unwrap();
            assert_eq!(
                db.get_sync_state(CACHE_SCHEMA_VERSION_KEY, "missing").unwrap(),
                CACHE_SCHEMA_VERSION.to_string(),
            );
        }
        let db = Database::open(&path).unwrap();
        assert!(
            db.list_labels().unwrap().is_empty(),
            "fast path must NOT have re-walked (labels stay empty)",
        );
    }

    // ── Schema version sentinel ───────────────────────────────────────

    #[test]
    fn open_in_memory_stamps_current_schema_version() {
        // A fresh DB starts at user_version=0; init must apply schema
        // and stamp SCHEMA_VERSION so a future reopen finds a matching
        // value rather than treating the DB as fresh again.
        let db = Database::open_in_memory().unwrap();
        let v: u32 = db.conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(v, SCHEMA_VERSION);
    }

    #[test]
    fn init_refuses_db_written_by_a_future_build() {
        // A DB whose user_version exceeds our SCHEMA_VERSION was written
        // by a build that may have added forward-only columns; opening
        // it risks silent corruption on subsequent writes.
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(&format!(
            "PRAGMA user_version = {};", SCHEMA_VERSION + 1
        )).unwrap();
        match Database::init(conn) {
            Err(DbError::SchemaVersionTooNew { db, build }) => {
                assert_eq!(db, SCHEMA_VERSION + 1);
                assert_eq!(build, SCHEMA_VERSION);
            }
            Err(other) => panic!("expected SchemaVersionTooNew, got {other:?}"),
            Ok(_) => panic!("expected error, init succeeded"),
        }
    }

    #[test]
    fn init_accepts_db_already_at_current_schema_version() {
        // Reopening a previously-stamped DB is the common case after
        // the first launch; init must accept it without re-stamping
        // or rejecting.
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(&format!(
            "PRAGMA user_version = {};", SCHEMA_VERSION
        )).unwrap();
        Database::init(conn).expect("init succeeds at current version");
    }

    // ── Name-collision suffix on sync merge ───────────────────────────

    #[test]
    fn label_name_collision_during_apply_renames_loser_with_uuid_suffix() {
        // Two peers offline both name a (different-uuid) label
        // "Morning". On sync merge, the UPSERT-on-uuid for the
        // second label hits UNIQUE-on-name. Without the retry,
        // recompute_label returns Err and replay_events rolls back
        // — sync hard-stuck on the poison event forever.
        // With the retry, the second row materialises under a
        // uuid-suffixed name and sync keeps moving.
        let db = Database::open_in_memory().unwrap();
        let l1 = "550e8400-e29b-41d4-a716-44665544aaaa";
        let l2 = "550e8400-e29b-41d4-a716-44665544bbbb";
        for (uuid, lamport, device) in [
            (l1, 5_i64, "dev-A"),
            (l2, 6_i64, "dev-B"),
        ] {
            db.apply_event(&Event {
                event_uuid: uuid::Uuid::new_v4().to_string(),
                lamport_ts: lamport,
                device_id: device.to_string(),
                kind: "label_insert".to_string(),
                target_id: uuid.to_string(),
                payload: serde_json::json!({
                    "uuid": uuid, "name": "Morning",
                }).to_string(),
            }).unwrap();
        }
        let labels = db.list_labels().unwrap();
        assert_eq!(labels.len(), 2,
            "both label rows must materialise — neither poisons the apply");
        let names: Vec<_> = labels.iter().map(|l| l.name.as_str()).collect();
        // The first-arriving label keeps the bare name; the second
        // gets the uuid-prefix suffix.
        assert!(names.contains(&"Morning"),
            "first label must keep its original name; got {names:?}");
        assert!(
            names.iter().any(|n| n.starts_with("Morning (conflict-")),
            "second label must carry the conflict suffix; got {names:?}"
        );
    }

    #[test]
    fn label_name_collision_is_idempotent_on_replay() {
        // Re-applying the same collision sequence must land on the
        // same suffixed name, not invent a different one each time
        // (the suffix is derived from the uuid prefix, which is
        // event-deterministic).
        let db1 = Database::open_in_memory().unwrap();
        let db2 = Database::open_in_memory().unwrap();
        let l1 = "550e8400-e29b-41d4-a716-44665544aaaa";
        let l2 = "550e8400-e29b-41d4-a716-44665544bbbb";
        let events: Vec<Event> = [(l1, 5_i64, "A"), (l2, 6_i64, "B")]
            .into_iter()
            .map(|(uuid, lamport, device)| Event {
                event_uuid: uuid::Uuid::new_v4().to_string(),
                lamport_ts: lamport,
                device_id: device.to_string(),
                kind: "label_insert".to_string(),
                target_id: uuid.to_string(),
                payload: serde_json::json!({
                    "uuid": uuid, "name": "Morning",
                }).to_string(),
            })
            .collect();
        for db in [&db1, &db2] {
            for e in &events { db.apply_event(e).unwrap(); }
        }
        let mut names1: Vec<_> = db1.list_labels().unwrap()
            .into_iter().map(|l| l.name).collect();
        let mut names2: Vec<_> = db2.list_labels().unwrap()
            .into_iter().map(|l| l.name).collect();
        names1.sort(); names2.sort();
        assert_eq!(names1, names2,
            "two devices applying the same events must converge on the same names");
    }

    // ── recompute_label re-links orphaned sessions ────────────────────

    #[test]
    fn session_arriving_before_label_gets_relinked_when_label_arrives() {
        // Out-of-order event delivery: a peer pushes label_insert and
        // session_insert in separate batches. Pull processes the
        // session batch first; recompute_session looks up the label
        // uuid and finds nothing (label_id = None — orphan). Later
        // the label batch arrives; recompute_label must re-link the
        // orphan sessions or they stay label-less forever even after
        // the label exists locally.
        let db = Database::open_in_memory().unwrap();
        let label_uuid = "550e8400-e29b-41d4-a716-446655440001";
        let session_uuid = "550e8400-e29b-41d4-a716-446655440002";

        // Step 1: session_insert arrives, label doesn't exist yet.
        let session_payload = serde_json::json!({
            "uuid": session_uuid,
            "start_iso": "2026-05-01T10:00:00",
            "duration_secs": 600,
            "label_uuid": label_uuid,
            "notes": null,
            "mode": "timer",
            "guided_file_uuid": null,
        }).to_string();
        db.apply_event(&Event {
            event_uuid: uuid::Uuid::new_v4().to_string(),
            lamport_ts: 5,
            device_id: "peer".to_string(),
            kind: "session_insert".to_string(),
            target_id: session_uuid.to_string(),
            payload: session_payload,
        }).unwrap();

        // Session exists but label_id is None — the orphan state.
        let sessions = db.list_sessions().unwrap();
        assert_eq!(sessions.len(), 1);
        assert!(
            sessions[0].1.label_id.is_none(),
            "session must be orphaned before label arrives"
        );

        // Step 2: label_insert arrives. Re-link must happen inside
        // recompute_label so the orphan resolves.
        let label_payload = serde_json::json!({
            "uuid": label_uuid,
            "name": "Focus",
        }).to_string();
        db.apply_event(&Event {
            event_uuid: uuid::Uuid::new_v4().to_string(),
            lamport_ts: 6,
            device_id: "peer".to_string(),
            kind: "label_insert".to_string(),
            target_id: label_uuid.to_string(),
            payload: label_payload,
        }).unwrap();

        let sessions = db.list_sessions().unwrap();
        let s = &sessions[0].1;
        assert!(s.label_id.is_some(), "session must be re-linked after label arrives");
        let labels = db.list_labels().unwrap();
        let focus_id = labels.iter().find(|l| l.name == "Focus").unwrap().id;
        assert_eq!(s.label_id, Some(focus_id));
    }

    #[test]
    fn label_relink_only_targets_sessions_whose_latest_event_references_this_label() {
        // A session that was inserted with label_uuid=L1 and later
        // updated to label_uuid=L2 must stay linked to L2 when L1
        // arrives — the re-link query must check the LATEST event's
        // label_uuid, not just any event in history.
        let db = Database::open_in_memory().unwrap();
        let l1 = "550e8400-e29b-41d4-a716-44665544aaaa";
        let l2 = "550e8400-e29b-41d4-a716-44665544bbbb";
        let s = "550e8400-e29b-41d4-a716-44665544cccc";

        // session_insert references L1 at lamport 5
        db.apply_event(&Event {
            event_uuid: uuid::Uuid::new_v4().to_string(),
            lamport_ts: 5, device_id: "peer".to_string(),
            kind: "session_insert".to_string(),
            target_id: s.to_string(),
            payload: serde_json::json!({
                "uuid": s, "start_iso": "2026-05-01T10:00:00",
                "duration_secs": 600, "label_uuid": l1,
                "notes": null, "mode": "timer", "guided_file_uuid": null,
            }).to_string(),
        }).unwrap();
        // session_update points to L2 at lamport 7 (the latest)
        db.apply_event(&Event {
            event_uuid: uuid::Uuid::new_v4().to_string(),
            lamport_ts: 7, device_id: "peer".to_string(),
            kind: "session_update".to_string(),
            target_id: s.to_string(),
            payload: serde_json::json!({
                "uuid": s, "start_iso": "2026-05-01T10:00:00",
                "duration_secs": 600, "label_uuid": l2,
                "notes": null, "mode": "timer", "guided_file_uuid": null,
            }).to_string(),
        }).unwrap();
        // Now L2 arrives — should link the session.
        db.apply_event(&Event {
            event_uuid: uuid::Uuid::new_v4().to_string(),
            lamport_ts: 8, device_id: "peer".to_string(),
            kind: "label_insert".to_string(),
            target_id: l2.to_string(),
            payload: serde_json::json!({"uuid": l2, "name": "L2"}).to_string(),
        }).unwrap();
        let labels = db.list_labels().unwrap();
        let l2_id = labels.iter().find(|l| l.name == "L2").unwrap().id;
        let session_label = db.list_sessions().unwrap()[0].1.label_id;
        assert_eq!(session_label, Some(l2_id), "linked to L2 (latest)");

        // L1 arrives — must NOT re-steal the session away from L2.
        db.apply_event(&Event {
            event_uuid: uuid::Uuid::new_v4().to_string(),
            lamport_ts: 9, device_id: "peer".to_string(),
            kind: "label_insert".to_string(),
            target_id: l1.to_string(),
            payload: serde_json::json!({"uuid": l1, "name": "L1"}).to_string(),
        }).unwrap();
        let session_label = db.list_sessions().unwrap()[0].1.label_id;
        assert_eq!(session_label, Some(l2_id),
            "session must remain on L2, not re-linked to L1 by the stale event");
    }

    // ── target_id_is_well_formed_for ──────────────────────────────────

    #[test]
    fn target_id_validator_accepts_uuid_and_short_identifiers() {
        // The validator is path-traversal-focused, not UUID-strict —
        // legitimate non-UUID identifiers (short test IDs, opaque
        // keys) pass through unless they carry path separators.
        for kind in [
            "session_insert", "label_rename", "interval_bell_update",
            "bell_sound_insert", "preset_delete", "guided_file_insert",
            "vibration_pattern_update",
        ] {
            assert!(target_id_is_well_formed_for(
                kind, "550e8400-e29b-41d4-a716-446655440000"
            ));
            assert!(target_id_is_well_formed_for(kind, "bell-1"));
            assert!(target_id_is_well_formed_for(kind, "u-1"));
        }
    }

    #[test]
    fn target_id_validator_rejects_path_traversal() {
        // These are the strings that would let a peer write outside
        // sounds_dir when the value is later interpolated into a
        // filename component.
        for bad in [
            "../../../etc/passwd",
            "../sibling.wav",
            "/absolute/path",
            "with/slash",
            "back\\slash",
            "null\0byte",
            "",
        ] {
            assert!(
                !target_id_is_well_formed_for("bell_sound_insert", bad),
                "expected reject for target_id={bad:?}"
            );
        }
    }

    #[test]
    fn target_id_validator_accepts_phase_strings_for_box_breath() {
        for phase in ["in", "holdin", "out", "holdout"] {
            assert!(target_id_is_well_formed_for("box_breath_phase_update", phase));
        }
    }

    #[test]
    fn target_id_validator_rejects_unknown_phase_or_traversal_for_box_breath() {
        for bad in ["inhale", "", "../etc", "IN"] {
            assert!(
                !target_id_is_well_formed_for("box_breath_phase_update", bad),
                "expected reject for box_breath target_id={bad:?}"
            );
        }
    }

    #[test]
    fn target_id_validator_passes_unknown_kinds_through() {
        // Forward-compat: a future entity type's event would be
        // recorded-not-applied; the validator must not over-block.
        assert!(target_id_is_well_formed_for("future_kind", "anything"));
        // ...except path-traversal, which is universal.
        assert!(!target_id_is_well_formed_for("future_kind", "../etc"));
        assert!(!target_id_is_well_formed_for("future_kind", ""));
    }

    #[test]
    fn apply_event_inner_skips_dispatch_on_invalid_target_id() {
        // Peer ships a bell_sound_insert with target_id that's a path-
        // traversal string. The event row records, but no bell_sounds
        // row materialises — preventing the downstream file-write
        // primitive in pull_custom_sound_files.
        let db = Database::open_in_memory().unwrap();
        let device_id = "peer-device".to_string();
        let evil = "../../../etc/passwd";
        let payload = serde_json::json!({
            "uuid": evil,
            "name": "Trojan",
            "file_path": "/p/x.wav",
            "is_bundled": false,
            "mime_type": "audio/wav",
            "category": "general",
            "created_iso": "2026-05-11T00:00:00",
        }).to_string();
        let event = Event {
            event_uuid: uuid::Uuid::new_v4().to_string(),
            lamport_ts: 1,
            device_id,
            kind: "bell_sound_insert".to_string(),
            target_id: evil.to_string(),
            payload,
        };
        db.apply_event(&event).unwrap();
        // Event row recorded for forward-compat, but no row in the
        // bell_sounds cache — the harm is the dispatch.
        let bells = db.list_bell_sounds().unwrap();
        assert!(
            bells.iter().all(|b| b.uuid != evil),
            "evil target_id must NOT land in bell_sounds.uuid"
        );
    }

    #[test]
    fn inserted_label_has_a_uuid() {
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        let labels = db.list_labels().unwrap();
        assert_eq!(labels.len(), 1);
        assert!(!labels[0].uuid.is_empty(), "uuid must be populated on read");
    }

    #[test]
    fn two_inserted_labels_get_distinct_uuids() {
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        db.insert_label("Evening").unwrap();
        let labels = db.list_labels().unwrap();
        assert_eq!(labels.len(), 2);
        assert_ne!(labels[0].uuid, labels[1].uuid);
    }

    #[test]
    fn inserted_label_uuid_is_v4_shaped() {
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        let uuid = &db.list_labels().unwrap()[0].uuid;
        assert!(looks_like_uuid_v4(uuid),
            "label uuid `{uuid}` doesn't match v4 shape");
    }


    // ── Event log: append + pending + mark_synced (A2.3) ─────────────────────
    //
    // The append-only event log is the single source of truth for all
    // mutations. `append_event` is idempotent on `event_uuid`: receiving
    // the same event twice (e.g. on retry, or from a peer that already
    // forwarded it) is a no-op rather than a constraint error escalated
    // to the caller. `pending_events` is the push-queue contract — sorted
    // by `lamport_ts` so peers see events in causal order.

    #[test]
    fn pending_events_is_empty_on_a_fresh_database() {
        let db = Database::open_in_memory().unwrap();
        assert!(db.pending_events().unwrap().is_empty());
    }

    #[test]
    fn append_event_then_read_back_via_pending_events() {
        let db = Database::open_in_memory().unwrap();
        let event = sample_event(7);
        db.append_event(&event).unwrap();
        let rows = db.pending_events().unwrap();
        assert_eq!(rows.len(), 1);
        let (_, got) = &rows[0];
        assert_eq!(got, &event,
            "appended event must round-trip every field unchanged");
    }

    #[test]
    fn append_event_returns_a_distinct_local_rowid_per_call() {
        // The local rowid is the cache key inside this device — distinct
        // from `event_uuid` (the cross-device identity). Two appends must
        // get two different rowids so callers can address them locally.
        let db = Database::open_in_memory().unwrap();
        let id_a = db.append_event(&sample_event(1)).unwrap();
        let id_b = db.append_event(&sample_event(2)).unwrap();
        assert_ne!(id_a, id_b);
    }

    #[test]
    fn append_event_with_duplicate_uuid_is_idempotent_no_op() {
        // `event_uuid` is UNIQUE — a second insert of the same uuid must
        // succeed silently and NOT create a second row. This makes
        // event delivery at-most-once on the local cache regardless of
        // how often the caller (or a sync retry) submits it.
        let db = Database::open_in_memory().unwrap();
        let event = sample_event(1);
        db.append_event(&event).unwrap();
        let res = db.append_event(&event);
        assert!(res.is_ok(),
            "duplicate-event_uuid append must be a silent no-op, got: {res:?}");
        assert_eq!(db.pending_events().unwrap().len(), 1,
            "duplicate append must not create a second row");
    }

    #[test]
    fn pending_events_orders_by_lamport_ts_ascending() {
        // Peers replay in lamport order to converge on a consistent
        // state. The push queue must hand events out in that same order
        // so a peer with a slow-then-fast connection still gets them
        // monotonically.
        let db = Database::open_in_memory().unwrap();
        // Insert out of order — ts 5, then 1, then 3.
        db.append_event(&sample_event(5)).unwrap();
        db.append_event(&sample_event(1)).unwrap();
        db.append_event(&sample_event(3)).unwrap();
        let timestamps: Vec<i64> = db.pending_events().unwrap()
            .iter().map(|(_, e)| e.lamport_ts).collect();
        assert_eq!(timestamps, vec![1, 3, 5]);
    }

    #[test]
    fn mark_event_synced_removes_it_from_pending_events() {
        let db = Database::open_in_memory().unwrap();
        let id_a = db.append_event(&sample_event(1)).unwrap();
        let _id_b = db.append_event(&sample_event(2)).unwrap();
        db.mark_event_synced(id_a).unwrap();
        let pending: Vec<i64> = db.pending_events().unwrap()
            .iter().map(|(_, e)| e.lamport_ts).collect();
        assert_eq!(pending, vec![2],
            "synced event must drop out of the pending list");
    }

    #[test]
    fn mark_event_synced_unknown_id_is_a_silent_no_op() {
        // Defensive: a stale id from a partial sync attempt must not
        // panic or surface an error. SQLite UPDATE on no-match is
        // already a no-op; the wrapper preserves that.
        let db = Database::open_in_memory().unwrap();
        db.append_event(&sample_event(1)).unwrap();
        let res = db.mark_event_synced(999);
        assert!(res.is_ok());
        assert_eq!(db.pending_events().unwrap().len(), 1,
            "the existing event must still be pending — nothing was marked");
    }

    #[test]
    fn mark_events_synced_batch_marks_every_provided_id() {
        // The batch variant must produce the same end state as N calls
        // to `mark_event_synced`. Used by the bulk-push path to flip
        // every event in a successful batch in a single transaction.
        let db = Database::open_in_memory().unwrap();
        let id_a = db.append_event(&sample_event(1)).unwrap();
        let id_b = db.append_event(&sample_event(2)).unwrap();
        let id_c = db.append_event(&sample_event(3)).unwrap();
        db.mark_events_synced(&[id_a, id_c]).unwrap();
        let pending = db.pending_events().unwrap();
        assert_eq!(pending.len(), 1, "only the un-marked event remains pending");
        assert_eq!(pending[0].0, id_b,
            "the un-marked event is the one whose id wasn't in the batch");
    }

    #[test]
    fn mark_events_synced_empty_slice_is_a_silent_no_op() {
        // Don't crash on the no-work path. The bulk push only calls
        // this when at least one event was pushed, but defending
        // against the empty input is cheap and removes a footgun.
        let db = Database::open_in_memory().unwrap();
        db.append_event(&sample_event(1)).unwrap();
        db.mark_events_synced(&[]).unwrap();
        assert_eq!(db.pending_events().unwrap().len(), 1,
            "the existing event must remain pending — nothing was asked of us");
    }

    #[test]
    fn mark_events_synced_is_atomic_across_the_batch() {
        // The batch runs inside one transaction. Verifies that the
        // mid-batch state isn't visible to a concurrent reader: either
        // all rows are marked or none. Hard to test fully without a
        // second connection — we check the post-condition.
        let db = Database::open_in_memory().unwrap();
        let ids: Vec<i64> = (1..=10)
            .map(|i| db.append_event(&sample_event(i)).unwrap())
            .collect();
        db.mark_events_synced(&ids).unwrap();
        assert!(db.pending_events().unwrap().is_empty(),
            "every event in the batch must be marked synced");
    }

    #[test]
    fn mark_events_synced_ignores_unknown_ids_among_known_ones() {
        // Same defensive shape as the single-id variant: a stale id
        // mixed in with valid ones doesn't poison the batch.
        let db = Database::open_in_memory().unwrap();
        let id_real = db.append_event(&sample_event(1)).unwrap();
        let result = db.mark_events_synced(&[id_real, 99_999]);
        assert!(result.is_ok());
        assert!(db.pending_events().unwrap().is_empty(),
            "the real event must still be marked synced");
    }

    // ── flag_all_events_unsynced — "push local" recovery primitive ─────

    #[test]
    fn flag_all_events_unsynced_marks_every_synced_event_pending() {
        // The "push local up" recovery path needs every authored
        // event to be re-pushed as a single fresh batch. Flipping
        // synced=0 across the table puts them all back into
        // pending_events.
        let db = Database::open_in_memory().unwrap();
        let id_a = db.append_event(&sample_event(1)).unwrap();
        let id_b = db.append_event(&sample_event(2)).unwrap();
        db.mark_events_synced(&[id_a, id_b]).unwrap();
        assert!(db.pending_events().unwrap().is_empty());

        db.flag_all_events_unsynced().unwrap();
        let pending = db.pending_events().unwrap();
        assert_eq!(pending.len(), 2,
            "every authored event must be back in pending");
    }

    #[test]
    fn flag_all_events_unsynced_is_a_no_op_on_already_pending_events() {
        // Already-pending rows must stay pending — the operation is
        // idempotent. (SQLite UPDATE WHERE matches no rows is fine,
        // but we shouldn't accidentally clobber other state.)
        let db = Database::open_in_memory().unwrap();
        let _ = db.append_event(&sample_event(1)).unwrap();
        let _ = db.append_event(&sample_event(2)).unwrap();
        let count_before = db.pending_events().unwrap().len();
        db.flag_all_events_unsynced().unwrap();
        assert_eq!(db.pending_events().unwrap().len(), count_before);
    }

    #[test]
    fn flag_all_events_unsynced_on_an_empty_log_is_a_silent_no_op() {
        // Defensive: never-synced device, empty events table. Don't
        // crash; subsequent assertions about pending_events stay valid.
        let db = Database::open_in_memory().unwrap();
        db.flag_all_events_unsynced().unwrap();
        assert!(db.pending_events().unwrap().is_empty());
    }

    #[test]
    fn flag_all_events_unsynced_does_not_touch_other_tables() {
        // Defensive: the operation is scoped to the synced flag.
        // Sessions, labels, settings, and known_remote_files must
        // survive untouched — only the events table changes.
        let db = Database::open_in_memory().unwrap();
        db.append_event(&sample_event(1)).unwrap();
        let label_id = db.insert_label("focus").unwrap();
        db.set_setting("k", "v").unwrap();
        db.record_known_remote_file("a").unwrap();
        let labels_before = db.list_labels().unwrap().len();

        db.flag_all_events_unsynced().unwrap();

        assert_eq!(db.list_labels().unwrap().len(), labels_before);
        assert!(db.list_labels().unwrap().iter().any(|l| l.id == label_id));
        assert_eq!(db.get_setting("k", "default").unwrap(), "v");
        assert!(db.known_remote_file_uuids().unwrap().contains("a"),
            "known_remote_files must be left alone — the caller wipes it \
             explicitly when needed");
    }

    // ── wipe_local_event_log — "wipe local" recovery primitive ─────────

    #[test]
    fn wipe_local_event_log_clears_every_event_sourced_table() {
        // The "wipe local to match remote" recovery deletes every
        // user-content table whose source-of-truth is the event log,
        // plus both dedup trackers. After the wipe, the local DB
        // looks like a freshly-initialised one minus settings/device.
        let db = Database::open_in_memory().unwrap();
        db.append_event(&sample_event(1)).unwrap();
        db.insert_label("focus").unwrap();
        db.insert_session(&Session {
            start_iso: "2026-04-30T10:00:00".into(),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        db.insert_interval_bell(IntervalBellKind::Interval, 5, 0, "bowl", BUNDLED_PATTERN_PULSE_UUID, SignalMode::Sound).unwrap();
        db.insert_bell_sound("Custom", "/p/c.wav", false, "audio/wav", BellSoundCategory::General).unwrap();
        db.insert_preset("Sitting", SessionMode::Timer, true, r#"{}"#).unwrap();
        db.insert_guided_file_with_uuid("gf-1", "Track", "/p/t.ogg", 300, false).unwrap();
        db.insert_vibration_pattern("Custom Pulse", 200, &[1.0, 0.0], ChartKind::Bar, false).unwrap();
        db.set_box_breath_phase(BoxBreathPhaseId::In, false, SignalMode::Sound, "x", "y").unwrap();
        db.record_known_remote_file("a").unwrap();
        db.record_known_remote_sound("bs-1").unwrap();
        // Sanity: rows present before wipe.
        assert!(!db.pending_events().unwrap().is_empty());
        assert!(!db.list_labels().unwrap().is_empty());
        assert!(!db.list_sessions().unwrap().is_empty());
        assert!(!db.list_interval_bells().unwrap().is_empty());
        assert!(!db.list_bell_sounds().unwrap().is_empty());
        assert!(!db.list_presets().unwrap().is_empty());
        assert!(!db.list_guided_files().unwrap().is_empty());
        assert!(!db.list_vibration_patterns().unwrap().is_empty());
        assert!(!db.known_remote_file_uuids().unwrap().is_empty());
        assert!(!db.known_remote_sound_uuids().unwrap().is_empty());

        db.wipe_local_event_log().unwrap();

        assert!(db.pending_events().unwrap().is_empty(),
            "events table must be empty");
        assert!(db.list_labels().unwrap().is_empty(),
            "labels table must be empty");
        assert!(db.list_sessions().unwrap().is_empty(),
            "sessions table must be empty");
        assert!(db.list_interval_bells().unwrap().is_empty(),
            "interval_bells table must be empty");
        assert!(db.list_bell_sounds().unwrap().is_empty(),
            "bell_sounds table must be empty");
        assert!(db.list_presets().unwrap().is_empty(),
            "presets table must be empty");
        assert!(db.list_guided_files().unwrap().is_empty(),
            "guided_files table must be empty");
        assert!(db.list_vibration_patterns().unwrap().is_empty(),
            "vibration_patterns table must be empty");
        assert!(db.known_remote_file_uuids().unwrap().is_empty(),
            "file dedup tracker must be empty");
        assert!(db.known_remote_sound_uuids().unwrap().is_empty(),
            "sound dedup tracker must be empty");
    }

    #[test]
    fn wipe_local_event_log_keeps_box_breath_phases_seeded_at_defaults() {
        // Box-Breath mode requires the 4 phase rows to render. Wipe
        // clears any user-customised rows but the seed re-runs inline
        // so the mode stays usable post-wipe even before sync replay.
        let db = Database::open_in_memory().unwrap();
        db.seed_box_breath_phases().unwrap();
        db.set_box_breath_phase(
            BoxBreathPhaseId::In, true, SignalMode::Vibration, "u-x", "u-y",
        ).unwrap();

        db.wipe_local_event_log().unwrap();

        let phases = db.list_box_breath_phases().unwrap();
        assert_eq!(phases.len(), 4,
            "all four phases re-seeded with defaults after wipe");
        let in_phase = phases.iter()
            .find(|p| p.phase == BoxBreathPhaseId::In).unwrap();
        assert!(!in_phase.enabled,
            "default enabled=false overwrote user's customised enabled=true");
        assert_eq!(in_phase.signal_mode, SignalMode::Sound,
            "default signal_mode overwrote user's customisation");
    }

    #[test]
    fn wipe_local_event_log_preserves_settings() {
        // User preferences (end_sound, weekly_goal, vibrate, etc.) are
        // independent of the event log we're discarding. The user
        // explicitly chose "wipe content"; their UI prefs should not
        // surprise-reset.
        let db = Database::open_in_memory().unwrap();
        db.set_setting("end_sound", "bowl").unwrap();
        db.set_setting("weekly_goal_mins", "150").unwrap();

        db.wipe_local_event_log().unwrap();

        assert_eq!(db.get_setting("end_sound", "fallback").unwrap(), "bowl");
        assert_eq!(db.get_setting("weekly_goal_mins", "0").unwrap(), "150");
    }

    #[test]
    fn wipe_local_event_log_preserves_sync_state() {
        // The configured Nextcloud account (URL, username) must
        // survive — the user is wiping local state to converge with
        // the same remote. Re-entering the URL would be a friction
        // surprise.
        let db = Database::open_in_memory().unwrap();
        db.set_sync_state("nextcloud_url", "https://nc.example/").unwrap();
        db.set_sync_state("nextcloud_username", "alice").unwrap();

        db.wipe_local_event_log().unwrap();

        assert_eq!(
            db.get_sync_state("nextcloud_url", "").unwrap(),
            "https://nc.example/");
        assert_eq!(
            db.get_sync_state("nextcloud_username", "").unwrap(),
            "alice");
    }

    #[test]
    fn wipe_local_event_log_preserves_device_id_and_lamport() {
        // Device identity persists across wipes. Resetting device_id
        // would create a new identity for the same physical device,
        // confusing peers' replay; resetting lamport could in theory
        // produce duplicate (lamport, device_id) tuples, though
        // monotonicity of the next emit_event would still prevent
        // collisions. Conservative: leave the device row alone.
        let db = Database::open_in_memory().unwrap();
        let device_before = db.device_id().unwrap();
        for _ in 0..5 { db.bump_lamport_clock().unwrap(); }
        let lamport_before = db.lamport_clock().unwrap();

        db.wipe_local_event_log().unwrap();

        assert_eq!(db.device_id().unwrap(), device_before,
            "device_id must survive wipe — it's this device's identity");
        assert_eq!(db.lamport_clock().unwrap(), lamport_before,
            "lamport_clock must survive wipe — keeps causal correctness");
    }

    #[test]
    fn wipe_local_event_log_is_idempotent_on_an_empty_database() {
        // Defensive: never-authored device, fresh DB. Don't crash.
        let db = Database::open_in_memory().unwrap();
        db.wipe_local_event_log().unwrap();
        db.wipe_local_event_log().unwrap();
        assert!(db.pending_events().unwrap().is_empty());
    }

    #[test]
    fn wipe_local_event_log_followed_by_authoring_creates_a_fresh_event() {
        // After wipe, normal authoring must work. The empty events
        // table accepts new inserts; pending_events sees the new row.
        let db = Database::open_in_memory().unwrap();
        db.append_event(&sample_event(1)).unwrap();
        db.wipe_local_event_log().unwrap();

        db.insert_session(&Session {
            start_iso: "2026-04-30T11:00:00".into(),
            duration_secs: 300,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        assert_eq!(db.list_sessions().unwrap().len(), 1);
        assert!(!db.pending_events().unwrap().is_empty(),
            "the new authoring must produce a pending event");
    }

    #[test]
    fn pending_events_excludes_synced_rows() {
        // After every event has been synced, pending_events is empty
        // again. Documents the boundary case of "fully caught up".
        let db = Database::open_in_memory().unwrap();
        let id_a = db.append_event(&sample_event(1)).unwrap();
        let id_b = db.append_event(&sample_event(2)).unwrap();
        db.mark_event_synced(id_a).unwrap();
        db.mark_event_synced(id_b).unwrap();
        assert!(db.pending_events().unwrap().is_empty());
    }


    #[test]
    fn append_event_persists_across_database_reopens() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("events.db");
        let event = sample_event(42);
        {
            let db = Database::open(&path).unwrap();
            db.append_event(&event).unwrap();
        }
        let db = Database::open(&path).unwrap();
        let rows = db.pending_events().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(&rows[0].1, &event);
    }


    // ── B1.0: events carry target_id for fast lookup ─────────────────────────
    //
    // Replay queries need to find "all events affecting target X" cheaply.
    // Parsing the JSON payload in SQL is awkward, so each event also
    // stores the affected row's identity in a denormalised `target_id`
    // column — for sessions/labels the cross-device uuid, for settings
    // the key.

    #[test]
    fn session_insert_event_target_id_is_the_session_uuid() {
        let db = Database::open_in_memory().unwrap();
        db.insert_session(&Session {
            start_iso: "2026-04-30T10:00:00".to_string(),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        let row_uuid = db.list_sessions().unwrap()[0].1.uuid.clone();
        let events = db.pending_events().unwrap();
        assert_eq!(events[0].1.target_id, row_uuid);
    }

    #[test]
    fn session_delete_event_target_id_is_the_session_uuid() {
        let db = Database::open_in_memory().unwrap();
        let id = db.insert_session(&Session {
            start_iso: "2026-04-30T10:00:00".to_string(),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        let row_uuid = db.list_sessions().unwrap()[0].1.uuid.clone();
        drain_events(&db);
        db.delete_session(id).unwrap();
        let events = db.pending_events().unwrap();
        assert_eq!(events[0].1.target_id, row_uuid);
    }

    #[test]
    fn label_insert_event_target_id_is_the_label_uuid() {
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        let row_uuid = db.list_labels().unwrap()[0].uuid.clone();
        let events = db.pending_events().unwrap();
        assert_eq!(events[0].1.target_id, row_uuid);
    }

    #[test]
    fn setting_changed_event_target_id_is_the_setting_key() {
        let db = Database::open_in_memory().unwrap();
        db.set_setting("daily_goal_minutes", "20").unwrap();
        let events = db.pending_events().unwrap();
        assert_eq!(events[0].1.target_id, "daily_goal_minutes",
            "settings have no uuid; the key acts as cross-device identity");
    }

    // ── B2: replay_events ─────────────────────────────────────────────────────
    //
    // Bulk applier for incoming sync batches. Sorts the slice by
    // (lamport_ts ASC, device_id ASC, event_uuid ASC) for a stable
    // deterministic order, then dispatches each through apply_event's
    // recompute path. Idempotent on event_uuid, order-independent
    // because apply_event itself is.

    #[test]
    fn replay_events_with_empty_slice_is_a_noop() {
        let db = Database::open_in_memory().unwrap();
        db.replay_events(&[]).unwrap();
        assert!(db.list_sessions().unwrap().is_empty());
        assert!(db.list_labels().unwrap().is_empty());
        assert!(db.pending_events().unwrap().is_empty());
    }

    #[test]
    fn replay_events_with_one_event_matches_apply_event_alone() {
        let db = Database::open_in_memory().unwrap();
        let event = synth_session_insert(
            SESSION_X, 5, DEVICE_A,
            "2026-04-30T10:00:00", 600,
            None, None, SessionMode::Timer,
        );
        db.replay_events(std::slice::from_ref(&event)).unwrap();
        let rows = db.list_sessions().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].1.uuid, SESSION_X);
    }

    #[test]
    fn replay_events_converges_regardless_of_input_order() {
        // The same event set in two different orders must produce the
        // same final cache state. This is the core convergence property.
        let session_b = "33333333-3333-4333-8333-333333333333";
        let events = vec![
            synth_session_insert(SESSION_X, 1, DEVICE_A,
                "S-X", 100, None, None, SessionMode::Timer),
            synth_session_insert(session_b, 2, DEVICE_A,
                "S-B", 200, None, None, SessionMode::Timer),
            synth_session_update(SESSION_X, 5, DEVICE_A,
                "S-X-edited", 150, None, Some("edit"), SessionMode::Timer),
            synth_session_delete(session_b, 6, DEVICE_A),
        ];

        let db_in_order = Database::open_in_memory().unwrap();
        db_in_order.replay_events(&events).unwrap();

        let mut shuffled = events.clone();
        shuffled.reverse();
        let db_reversed = Database::open_in_memory().unwrap();
        db_reversed.replay_events(&shuffled).unwrap();

        let in_order = db_in_order.list_sessions().unwrap();
        let reversed = db_reversed.list_sessions().unwrap();
        assert_eq!(in_order.len(), 1, "session_b must be tombstoned away");
        assert_eq!(in_order.len(), reversed.len(),
            "convergence: same event set yields same row count regardless of order");
        assert_eq!(in_order[0].1.uuid, reversed[0].1.uuid);
        assert_eq!(in_order[0].1.start_iso, reversed[0].1.start_iso);
        assert_eq!(in_order[0].1.duration_secs, reversed[0].1.duration_secs);
        assert_eq!(in_order[0].1.notes, reversed[0].1.notes);
    }

    #[test]
    fn replay_events_dedups_duplicate_event_uuids() {
        // Same Event present twice in the input slice must be applied
        // only once — no double row, no error. Real-world cause:
        // overlapping pull windows or peer-forwarded duplicates.
        let db = Database::open_in_memory().unwrap();
        let event = synth_session_insert(
            SESSION_X, 5, DEVICE_A,
            "2026-04-30T10:00:00", 600,
            None, None, SessionMode::Timer,
        );
        db.replay_events(&[event.clone(), event]).unwrap();
        assert_eq!(db.list_sessions().unwrap().len(), 1);
    }

    #[test]
    fn replay_events_two_devices_authoring_independently_merges_both() {
        // Realistic scenario: two devices author concurrently, then each
        // pulls the other's events. After cross-replay both DBs have the
        // union of both devices' inserts.
        let device_a = Database::open_in_memory().unwrap();
        let device_b = Database::open_in_memory().unwrap();

        device_a.insert_session(&Session {
            start_iso: "2026-04-30T10:00:00".to_string(),
            duration_secs: 600, label_id: None, notes: Some("from A".to_string()),
            mode: SessionMode::Timer, uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        device_b.insert_session(&Session {
            start_iso: "2026-04-30T18:00:00".to_string(),
            duration_secs: 1200, label_id: None, notes: Some("from B".to_string()),
            mode: SessionMode::Timer, uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();

        let events_a: Vec<Event> = device_a.pending_events().unwrap()
            .into_iter().map(|(_, e)| e).collect();
        let events_b: Vec<Event> = device_b.pending_events().unwrap()
            .into_iter().map(|(_, e)| e).collect();

        device_a.replay_events(&events_b).unwrap();
        device_b.replay_events(&events_a).unwrap();

        let sessions_a = device_a.list_sessions().unwrap();
        let sessions_b = device_b.list_sessions().unwrap();
        assert_eq!(sessions_a.len(), 2);
        assert_eq!(sessions_b.len(), 2);

        let notes_a: std::collections::HashSet<_> = sessions_a.iter()
            .filter_map(|(_, s)| s.notes.clone()).collect();
        let notes_b: std::collections::HashSet<_> = sessions_b.iter()
            .filter_map(|(_, s)| s.notes.clone()).collect();
        let expected: std::collections::HashSet<_> = ["from A", "from B"]
            .iter().map(|s| s.to_string()).collect();
        assert_eq!(notes_a, expected);
        assert_eq!(notes_b, expected,
            "after cross-replay, both devices must hold the same union of events");
    }

    #[test]
    fn replay_events_idempotent_under_repeat_application() {
        // Replaying the same batch twice produces the same state as
        // replaying it once. Important for sync reliability — a partial
        // sync that retries the whole batch must not corrupt state.
        let device_a = Database::open_in_memory().unwrap();
        let device_b = Database::open_in_memory().unwrap();
        for i in 0..3 {
            device_a.insert_session(&Session {
                start_iso: format!("2026-04-3{i}T10:00:00"),
                duration_secs: 600, label_id: None, notes: None,
                mode: SessionMode::Timer, uuid: String::new(),
                guided_file_uuid: None,
            }).unwrap();
        }
        let events: Vec<Event> = device_a.pending_events().unwrap()
            .into_iter().map(|(_, e)| e).collect();
        device_b.replay_events(&events).unwrap();
        let after_first = device_b.list_sessions().unwrap();
        device_b.replay_events(&events).unwrap();
        let after_second = device_b.list_sessions().unwrap();
        assert_eq!(after_first.len(), after_second.len());
        assert_eq!(after_first, after_second,
            "second replay of the same batch must be a no-op on the cache");
    }

    // ── Lamport observation rule on apply_event (regression) ────────────────
    //
    // Per Nextcloud-Sync.md: "on remote event observation: lamport =
    // max(lamport, remote.lamport) + 1". apply_event must advance the
    // local clock for fresh remote events so a follow-up local write
    // strictly orders after what we just observed. Skipped for our own
    // device's events (idempotency) and for duplicates (only first
    // observation counts).

    #[test]
    fn apply_event_advances_local_lamport_when_observing_a_higher_remote_event() {
        // Local clock starts at 0. We see a remote event tagged
        // lamport=10 from a different device. After applying, our
        // local clock must be max(0,10)+1 = 11 — so any event we
        // author next will sort strictly after the observed one.
        let db = Database::open_in_memory().unwrap();
        assert_eq!(db.lamport_clock().unwrap(), 0);
        db.apply_event(&synth_session_insert(
            SESSION_X, 10, DEVICE_A,
            "2026-04-30T10:00:00", 600,
            None, None, SessionMode::Timer,
        )).unwrap();
        assert_eq!(db.lamport_clock().unwrap(), 11,
            "observation rule: local must jump to max(local, remote)+1");
    }

    #[test]
    fn apply_event_advances_local_lamport_even_when_local_is_already_ahead() {
        // Local has done lots of work (clock at 50). Remote observation
        // at lamport=10 must still advance to max(50,10)+1=51 — every
        // observation strictly increases the clock so no two events
        // ever share a (lamport, device_id) pair on the same device.
        let db = Database::open_in_memory().unwrap();
        for _ in 0..50 { db.bump_lamport_clock().unwrap(); }
        assert_eq!(db.lamport_clock().unwrap(), 50);
        db.apply_event(&synth_session_insert(
            SESSION_X, 10, DEVICE_A,
            "_", 1, None, None, SessionMode::Timer,
        )).unwrap();
        assert_eq!(db.lamport_clock().unwrap(), 51);
    }

    #[test]
    fn apply_event_does_not_advance_local_lamport_for_our_own_device_events() {
        // Re-applying an event we authored locally (idempotency retry,
        // or pulling our own event back from remote storage) must not
        // shift the clock. Otherwise a "harmless retry" would silently
        // mutate clock state and break ordering invariants.
        let db = Database::open_in_memory().unwrap();
        let our_device_id = db.device_id().unwrap();
        db.bump_lamport_clock().unwrap();
        db.bump_lamport_clock().unwrap();
        let before = db.lamport_clock().unwrap();
        // Author an event "from us" with a very high lamport value.
        let our_event = synth_session_insert(
            SESSION_X, 999, &our_device_id,
            "_", 1, None, None, SessionMode::Timer,
        );
        db.apply_event(&our_event).unwrap();
        assert_eq!(db.lamport_clock().unwrap(), before,
            "apply_event with our own device_id must not bump the clock");
    }

    #[test]
    fn apply_event_does_not_advance_local_lamport_on_duplicate_remote_observation() {
        // Receiving the same event twice — e.g. overlapping pull
        // windows or peer-forwarded duplicates — must only bump the
        // clock once. The bump is per *new observation*, not per call.
        let db = Database::open_in_memory().unwrap();
        let event = synth_session_insert(
            SESSION_X, 10, DEVICE_A,
            "_", 1, None, None, SessionMode::Timer,
        );
        db.apply_event(&event).unwrap();
        let after_first = db.lamport_clock().unwrap();
        db.apply_event(&event).unwrap();
        let after_second = db.lamport_clock().unwrap();
        assert_eq!(after_first, after_second,
            "second observation of the same event_uuid must not bump");
    }

    #[test]
    fn local_writes_after_observing_a_remote_event_strictly_order_after_it() {
        // The end-to-end correctness property: a write authored after
        // observing a remote event must have a strictly larger
        // lamport_ts than the remote event. Without the observation
        // rule, a slow local clock would author "in the past" and
        // peers would resolve it as the older write — wrong.
        let db = Database::open_in_memory().unwrap();
        // Remote event at lamport=20 lands on a fresh local DB.
        db.apply_event(&synth_session_insert(
            SESSION_X, 20, DEVICE_A,
            "remote", 100, None, None, SessionMode::Timer,
        )).unwrap();
        // Now author a local session. Its event must have lamport > 20.
        db.insert_session(&Session {
            start_iso: "local".into(),
            duration_secs: 200,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        let local_event = db.pending_events().unwrap()
            .into_iter()
            .find(|(_, e)| e.kind == "session_insert" && e.device_id == db.device_id().unwrap())
            .map(|(_, e)| e)
            .expect("local session_insert must be in pending events");
        assert!(local_event.lamport_ts > 20,
            "local event at lamport {} must order strictly after observed remote at 20",
            local_event.lamport_ts);
    }

    #[test]
    fn replay_events_advances_lamport_through_the_observation_rule() {
        // replay_events processes a batch via apply_event_inner, which
        // includes the observation step. After replaying a batch from
        // a peer whose highest lamport was N, our local clock must be
        // ≥ N+1 so subsequent local writes order after the batch.
        let db = Database::open_in_memory().unwrap();
        let batch = vec![
            synth_session_insert(SESSION_X, 5, DEVICE_A,
                "_", 1, None, None, SessionMode::Timer),
            synth_session_update(SESSION_X, 12, DEVICE_A,
                "_", 1, None, None, SessionMode::Timer),
        ];
        db.replay_events(&batch).unwrap();
        assert!(db.lamport_clock().unwrap() >= 13,
            "after replaying a batch up to lamport 12, local clock must be >= 13, got {}",
            db.lamport_clock().unwrap());
    }

    #[test]
    fn replay_events_handles_mixed_kinds_in_one_batch() {
        // A realistic batch: an insert label, an insert session that
        // references the label, an update session, a delete label, and
        // a settings change. Apply all together and the final cache
        // reflects every conflict-resolution rule.
        let db = Database::open_in_memory().unwrap();
        let events = vec![
            synth_label_insert(LABEL_X, 1, DEVICE_A, "Morning"),
            synth_session_insert(
                SESSION_X, 2, DEVICE_A,
                "10:00", 600, Some(LABEL_X), None, SessionMode::Timer,
            ),
            synth_session_update(
                SESSION_X, 3, DEVICE_A,
                "10:00", 900, Some(LABEL_X), Some("longer"), SessionMode::Timer,
            ),
            synth_label_delete(LABEL_X, 4, DEVICE_A),
            synth_setting_changed("daily_goal", "20", 5, DEVICE_A),
        ];
        db.replay_events(&events).unwrap();

        // Label is gone (deleted at lamport 4 after insert at 1).
        assert!(db.list_labels().unwrap().is_empty());
        // Session is present with the lamport-3 update's values, but
        // its label_id is NULL because the label has been deleted.
        let s = &db.list_sessions().unwrap()[0].1;
        assert_eq!(s.duration_secs, 900);
        assert_eq!(s.notes.as_deref(), Some("longer"));
        assert_eq!(s.label_id, None,
            "session keeps its data but loses the label link when the label tombstones");
        assert_eq!(db.get_setting("daily_goal", "x").unwrap(), "20");
    }

}
