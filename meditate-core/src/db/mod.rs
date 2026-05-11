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

    // ── SessionMode serialization ─────────────────────────────────────────────

    #[test]
    fn session_mode_as_db_str_returns_canonical_strings() {
        // These are the values that go into the sessions.mode column AND
        // the CSV mode column — pinning them so a refactor that quietly
        // changes one (e.g. 'box_breath' → 'breath') gets caught.
        assert_eq!(SessionMode::Timer.as_db_str(), "timer");
        assert_eq!(SessionMode::BoxBreath.as_db_str(), "box_breath");
        assert_eq!(SessionMode::Guided.as_db_str(), "guided");
    }

    #[test]
    fn session_mode_from_db_str_parses_canonical_strings() {
        assert_eq!(SessionMode::from_db_str("timer"), Some(SessionMode::Timer));
        assert_eq!(SessionMode::from_db_str("box_breath"), Some(SessionMode::BoxBreath));
        assert_eq!(SessionMode::from_db_str("guided"), Some(SessionMode::Guided));
    }

    #[test]
    fn session_mode_from_db_str_returns_none_for_unknown() {
        // No legacy fallback — "countdown" and "stopwatch" deliberately
        // map to None. Callers decide what to do (existing data_io /
        // log paths default to Timer via unwrap_or, which makes legacy
        // rows readable without us adding a compat shim).
        assert_eq!(SessionMode::from_db_str(""), None);
        assert_eq!(SessionMode::from_db_str("countdown"), None);
        assert_eq!(SessionMode::from_db_str("stopwatch"), None);
        assert_eq!(SessionMode::from_db_str("TIMER"), None);  // case-sensitive
        assert_eq!(SessionMode::from_db_str("breathing"), None);  // old name
        assert_eq!(SessionMode::from_db_str("box-breath"), None); // dash, not underscore
        assert_eq!(SessionMode::from_db_str("Guided"), None);     // case-sensitive
        assert_eq!(SessionMode::from_db_str("garbage"), None);
    }

    #[test]
    fn session_mode_db_str_round_trip() {
        for &mode in &[SessionMode::Timer, SessionMode::BoxBreath, SessionMode::Guided] {
            assert_eq!(SessionMode::from_db_str(mode.as_db_str()), Some(mode));
        }
    }

    // ── label_totals_seconds (name, secs, count) ─────────────────────────────

    #[test]
    fn label_totals_seconds_is_empty_for_empty_db() {
        let db = Database::open_in_memory().unwrap();
        assert!(db.label_totals_seconds().unwrap().is_empty());
    }

    #[test]
    fn label_totals_seconds_groups_secs_and_counts_per_label() {
        // (name, total_secs, session_count) per label. Unlabeled sessions
        // and labels with zero sessions are excluded — INNER JOIN drops
        // them at the SQL level. Sort: total_secs DESC, name ASC NOCASE.
        let db = Database::open_in_memory().unwrap();
        let morning = db.insert_label("Morning").unwrap();
        let evening = db.insert_label("Evening").unwrap();
        // An extra label with no sessions — must NOT appear in output.
        let _unused = db.insert_label("Unused").unwrap();

        // Morning: 2 sessions, 900s total.
        db.insert_session(&Session {
            start_iso: "2026-04-27T07:00:00".to_string(),
            duration_secs: 600, label_id: Some(morning), notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        db.insert_session(&Session {
            start_iso: "2026-04-28T07:00:00".to_string(),
            duration_secs: 300, label_id: Some(morning), notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        // Evening: 1 session, 1200s total — larger total, should sort first.
        db.insert_session(&Session {
            start_iso: "2026-04-27T20:00:00".to_string(),
            duration_secs: 1200, label_id: Some(evening), notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        // Unlabeled session — must NOT appear.
        db.insert_session(&Session {
            start_iso: "2026-04-27T12:00:00".to_string(),
            duration_secs: 500, label_id: None, notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();

        let got = db.label_totals_seconds().unwrap();
        assert_eq!(got.len(), 2,
            "Unused label and unlabeled session must be excluded: {got:?}");
        assert_eq!(got[0], ("Evening".to_string(), 1200, 1));
        assert_eq!(got[1], ("Morning".to_string(), 900, 2));
    }

    #[test]
    fn label_totals_seconds_ties_break_case_insensitive_alphabetic() {
        // Same total ⇒ secondary sort by name, NOCASE.
        let db = Database::open_in_memory().unwrap();
        let zebra = db.insert_label("Zebra").unwrap();
        let alpha = db.insert_label("alpha").unwrap();
        db.insert_session(&Session {
            start_iso: "2026-04-27T12:00:00".to_string(),
            duration_secs: 600, label_id: Some(zebra), notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        db.insert_session(&Session {
            start_iso: "2026-04-28T12:00:00".to_string(),
            duration_secs: 600, label_id: Some(alpha), notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        let got = db.label_totals_seconds().unwrap();
        // 'alpha' (lowercase) sorts before 'Zebra' under NOCASE collation.
        assert_eq!(got[0].0, "alpha");
        assert_eq!(got[1].0, "Zebra");
    }

    #[test]
    fn label_totals_seconds_preserves_full_seconds_precision() {
        // total_minutes_by_label returns minutes (lossy integer division).
        // This variant must NOT lose sub-minute precision.
        let db = Database::open_in_memory().unwrap();
        let lid = db.insert_label("Morning").unwrap();
        // 90s + 45s = 135s — would round to 2 minutes (=120s) under
        // the minutes-then-converted approach.
        db.insert_session(&Session {
            start_iso: "2026-04-27T10:00:00".to_string(),
            duration_secs: 90, label_id: Some(lid), notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        db.insert_session(&Session {
            start_iso: "2026-04-28T10:00:00".to_string(),
            duration_secs: 45, label_id: Some(lid), notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        let got = db.label_totals_seconds().unwrap();
        assert_eq!(got[0], ("Morning".to_string(), 135, 2));
    }

    // ── hour_buckets ─────────────────────────────────────────────────────────

    #[test]
    fn hour_buckets_is_zero_zero_zero_for_empty_db() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(db.hour_buckets().unwrap(), (0, 0, 0));
    }

    #[test]
    fn hour_buckets_assigns_each_session_to_exactly_one_bucket() {
        // Boundaries: morning < 12 (00:00–11:59), afternoon 12–17,
        // evening ≥ 18 (18:00–23:59). Pin every boundary explicitly.
        let db = Database::open_in_memory().unwrap();
        let make = |hh: u32, mm: u32| Session {
            start_iso: format!("2026-04-27T{hh:02}:{mm:02}:00"),
            duration_secs: 600, label_id: None, notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        };
        // Morning (5 sessions, hours 0, 6, 11:00, 11:59).
        db.insert_session(&make(0, 0)).unwrap();
        db.insert_session(&make(6, 30)).unwrap();
        db.insert_session(&make(11, 0)).unwrap();
        db.insert_session(&make(11, 59)).unwrap();
        db.insert_session(&make(8, 15)).unwrap();
        // Afternoon (3 sessions, hours 12:00, 15:30, 17:59).
        db.insert_session(&make(12, 0)).unwrap();  // boundary into afternoon
        db.insert_session(&make(15, 30)).unwrap();
        db.insert_session(&make(17, 59)).unwrap(); // last minute of afternoon
        // Evening (2 sessions, hours 18:00, 23:59).
        db.insert_session(&make(18, 0)).unwrap();  // boundary into evening
        db.insert_session(&make(23, 59)).unwrap();

        let (morning, afternoon, evening) = db.hour_buckets().unwrap();
        assert_eq!(morning, 5, "five sessions in 00:00–11:59");
        assert_eq!(afternoon, 3, "three sessions in 12:00–17:59");
        assert_eq!(evening, 2, "two sessions in 18:00–23:59");
    }

    #[test]
    fn hour_buckets_total_equals_session_count() {
        // Defensive: every session lands in exactly one bucket, no
        // sessions are dropped or double-counted.
        let db = Database::open_in_memory().unwrap();
        let hours = [3u32, 7, 11, 12, 13, 17, 18, 22];
        for &h in &hours {
            db.insert_session(&Session {
                start_iso: format!("2026-04-27T{h:02}:00:00"),
                duration_secs: 600, label_id: None, notes: None,
                mode: SessionMode::Timer,
                uuid: String::new(),
                guided_file_uuid: None,
            }).unwrap();
        }
        let (m, a, e) = db.hour_buckets().unwrap();
        assert_eq!(m + a + e, hours.len() as i64);
        assert_eq!(m + a + e, db.count_sessions().unwrap());
    }

    // ── active_months ────────────────────────────────────────────────────────

    #[test]
    fn active_months_is_empty_for_empty_db() {
        let db = Database::open_in_memory().unwrap();
        assert!(db.active_months().unwrap().is_empty());
    }

    #[test]
    fn active_months_returns_distinct_year_month_pairs_descending() {
        // Each session contributes its (year, month) — duplicates within
        // the same month collapse to one entry. Order is most-recent first
        // (the calendar picker shows latest months at the top).
        let db = Database::open_in_memory().unwrap();
        // Three sessions in 2026-04, two in 2026-03, one in 2025-12.
        for d in 1..=3 {
            db.insert_session(&Session {
                start_iso: format!("2026-04-{d:02}T10:00:00"),
                duration_secs: 600, label_id: None, notes: None,
                mode: SessionMode::Timer,
                uuid: String::new(),
                guided_file_uuid: None,
            }).unwrap();
        }
        for d in 5..=6 {
            db.insert_session(&Session {
                start_iso: format!("2026-03-{d:02}T10:00:00"),
                duration_secs: 600, label_id: None, notes: None,
                mode: SessionMode::Timer,
                uuid: String::new(),
                guided_file_uuid: None,
            }).unwrap();
        }
        db.insert_session(&Session {
            start_iso: "2025-12-25T10:00:00".to_string(),
            duration_secs: 600, label_id: None, notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();

        let got = db.active_months().unwrap();
        // Three distinct months, newest first.
        assert_eq!(got, vec![(2026, 4), (2026, 3), (2025, 12)]);
    }

    #[test]
    fn active_months_orders_correctly_across_year_boundary() {
        // 2025-12 must sort BEFORE 2026-01 in newest-first ordering.
        let db = Database::open_in_memory().unwrap();
        db.insert_session(&Session {
            start_iso: "2026-01-15T10:00:00".to_string(),
            duration_secs: 600, label_id: None, notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        db.insert_session(&Session {
            start_iso: "2025-12-15T10:00:00".to_string(),
            duration_secs: 600, label_id: None, notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        let got = db.active_months().unwrap();
        assert_eq!(got, vec![(2026, 1), (2025, 12)]);
    }

    // ── active_days_in_month ─────────────────────────────────────────────────

    #[test]
    fn active_days_in_month_is_empty_for_silent_month() {
        let db = Database::open_in_memory().unwrap();
        assert!(db.active_days_in_month(2026, 4).unwrap().is_empty());
    }

    #[test]
    fn active_days_in_month_returns_distinct_days_ascending() {
        // Each day with at least one session contributes once. Multiple
        // sessions on the same day collapse to one entry. Returned in
        // ascending order (1, 2, 3, …) so callers can directly map to
        // calendar cells.
        let db = Database::open_in_memory().unwrap();
        // Two sessions on day 5, one on day 12, one on day 28.
        for hr in 9..=10 {
            db.insert_session(&Session {
                start_iso: format!("2026-04-05T{hr:02}:00:00"),
                duration_secs: 600, label_id: None, notes: None,
                mode: SessionMode::Timer,
                uuid: String::new(),
                guided_file_uuid: None,
            }).unwrap();
        }
        db.insert_session(&Session {
            start_iso: "2026-04-12T10:00:00".to_string(),
            duration_secs: 600, label_id: None, notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        db.insert_session(&Session {
            start_iso: "2026-04-28T10:00:00".to_string(),
            duration_secs: 600, label_id: None, notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        // A session in March — must NOT appear in April's days.
        db.insert_session(&Session {
            start_iso: "2026-03-15T10:00:00".to_string(),
            duration_secs: 600, label_id: None, notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();

        let got = db.active_days_in_month(2026, 4).unwrap();
        assert_eq!(got, vec![5u32, 12, 28]);
    }

    #[test]
    fn active_days_in_month_handles_december() {
        // The 'next month' boundary in code must roll to next-year-Jan
        // for December queries.
        let db = Database::open_in_memory().unwrap();
        db.insert_session(&Session {
            start_iso: "2026-12-31T23:00:00".to_string(),
            duration_secs: 600, label_id: None, notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        // Jan 1 next year — must NOT contribute.
        db.insert_session(&Session {
            start_iso: "2027-01-01T00:30:00".to_string(),
            duration_secs: 600, label_id: None, notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        let got = db.active_days_in_month(2026, 12).unwrap();
        assert_eq!(got, vec![31u32]);
    }

    // ── month_total_secs ─────────────────────────────────────────────────────

    #[test]
    fn month_total_secs_is_zero_for_empty_month() {
        let db = Database::open_in_memory().unwrap();
        // Far past — guaranteed empty.
        assert_eq!(db.month_total_secs(1999, 1).unwrap(), 0);
        // Mid-future — also empty.
        assert_eq!(db.month_total_secs(2099, 12).unwrap(), 0);
    }

    #[test]
    fn month_total_secs_sums_only_target_month() {
        // Adjacent-month boundary edges: last second of March and first
        // second of May must NOT count toward April.
        let db = Database::open_in_memory().unwrap();
        // March 31, very late.
        db.insert_session(&Session {
            start_iso: "2026-03-31T23:59:59".to_string(),
            duration_secs: 9999, label_id: None, notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        // April 1, midnight — INCLUDED in April.
        db.insert_session(&Session {
            start_iso: "2026-04-01T00:00:00".to_string(),
            duration_secs: 600, label_id: None, notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        // April 30, late evening — INCLUDED.
        db.insert_session(&Session {
            start_iso: "2026-04-30T23:59:59".to_string(),
            duration_secs: 1200, label_id: None, notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        // May 1, midnight — EXCLUDED.
        db.insert_session(&Session {
            start_iso: "2026-05-01T00:00:00".to_string(),
            duration_secs: 8888, label_id: None, notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();

        assert_eq!(db.month_total_secs(2026, 4).unwrap(), 600 + 1200);
    }

    #[test]
    fn month_total_secs_handles_december_year_rollover() {
        // The "next month" boundary is built in code; December must
        // roll to next-year-January cleanly.
        let db = Database::open_in_memory().unwrap();
        db.insert_session(&Session {
            start_iso: "2026-12-15T10:00:00".to_string(),
            duration_secs: 600, label_id: None, notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        // Jan 1, 2027 — must NOT count toward Dec 2026.
        db.insert_session(&Session {
            start_iso: "2027-01-01T00:00:00".to_string(),
            duration_secs: 9999, label_id: None, notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        assert_eq!(db.month_total_secs(2026, 12).unwrap(), 600);
    }

    // ── total_secs_since: weekly goal ring etc. ──────────────────────────────

    #[test]
    fn total_secs_since_is_zero_for_empty_db() {
        let db = Database::open_in_memory().unwrap();
        let since = chrono::NaiveDate::from_ymd_opt(2026, 4, 27).unwrap();
        assert_eq!(db.total_secs_since(since).unwrap(), 0);
    }

    #[test]
    fn total_secs_since_includes_sessions_on_or_after_date() {
        // Cut-off is at the START of the local-naive `since` date — a
        // session at 00:00:00 on `since` IS included.
        let db = Database::open_in_memory().unwrap();
        // On the cut-off date.
        db.insert_session(&Session {
            start_iso: "2026-04-27T00:00:00".to_string(),
            duration_secs: 600, label_id: None, notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        // Later that day.
        db.insert_session(&Session {
            start_iso: "2026-04-27T18:00:00".to_string(),
            duration_secs: 1200, label_id: None, notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        // Following day.
        db.insert_session(&Session {
            start_iso: "2026-04-28T10:00:00".to_string(),
            duration_secs: 300, label_id: None, notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        let since = chrono::NaiveDate::from_ymd_opt(2026, 4, 27).unwrap();
        assert_eq!(db.total_secs_since(since).unwrap(), 600 + 1200 + 300);
    }

    #[test]
    fn total_secs_since_excludes_sessions_before_date() {
        let db = Database::open_in_memory().unwrap();
        // Day before the cut-off.
        db.insert_session(&Session {
            start_iso: "2026-04-26T23:59:59".to_string(),
            duration_secs: 9999, label_id: None, notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        // On / after cut-off — counted.
        db.insert_session(&Session {
            start_iso: "2026-04-27T00:00:00".to_string(),
            duration_secs: 600, label_id: None, notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        let since = chrono::NaiveDate::from_ymd_opt(2026, 4, 27).unwrap();
        assert_eq!(db.total_secs_since(since).unwrap(), 600);
    }

    #[test]
    fn total_secs_since_far_future_date_returns_zero() {
        // Asking for a date past every session's start returns 0.
        let db = Database::open_in_memory().unwrap();
        db.insert_session(&Session {
            start_iso: "2026-04-27T10:00:00".to_string(),
            duration_secs: 600, label_id: None, notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        let since = chrono::NaiveDate::from_ymd_opt(2099, 1, 1).unwrap();
        assert_eq!(db.total_secs_since(since).unwrap(), 0);
    }

    // ── get_longest_session ──────────────────────────────────────────────────

    #[test]
    fn get_longest_session_is_none_for_empty_db() {
        let db = Database::open_in_memory().unwrap();
        assert!(db.get_longest_session().unwrap().is_none());
    }

    #[test]
    fn get_longest_session_returns_only_session_for_single_row_db() {
        let db = Database::open_in_memory().unwrap();
        let mut session = Session {
            start_iso: "2026-04-27T10:00:00Z".to_string(),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        };
        let id = db.insert_session(&session).unwrap();
        let (got_id, got) = db.get_longest_session().unwrap().unwrap();
        assert!(looks_like_uuid_v4(&got.uuid),
            "longest-session result must carry a v4 uuid");
        session.uuid = got.uuid.clone();
        assert_eq!((got_id, got), (id, session));
    }

    #[test]
    fn get_longest_session_returns_largest_duration() {
        // The longest among many — every other session must be shorter,
        // and the returned Session is the LONG one with all its fields
        // intact (not just the duration).
        let db = Database::open_in_memory().unwrap();
        for &secs in &[300u32, 600, 900, 1200, 450] {
            db.insert_session(&Session {
                start_iso: format!("2026-04-2{secs}T10:00:00Z"),
                duration_secs: secs,
                label_id: None,
                notes: None,
                mode: SessionMode::Timer,
                uuid: String::new(),
                guided_file_uuid: None,
            }).unwrap();
        }
        let mut longest_session = Session {
            start_iso: "2026-04-30T20:00:00Z".to_string(),
            duration_secs: 3600,
            label_id: None,
            notes: Some("the long one".to_string()),
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        };
        let longest_id = db.insert_session(&longest_session).unwrap();
        // Add one more shorter after — the order of insertion must not
        // affect which row wins.
        db.insert_session(&Session {
            start_iso: "2026-05-01T10:00:00Z".to_string(),
            duration_secs: 700,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();

        let (got_id, got) = db.get_longest_session().unwrap().unwrap();
        assert!(looks_like_uuid_v4(&got.uuid));
        longest_session.uuid = got.uuid.clone();
        assert_eq!(got_id, longest_id);
        assert_eq!(got, longest_session,
            "the returned Session must have every field of the long row, not just duration");
    }

    // ── total_seconds: precision-preserving aggregate ─────────────────────────

    #[test]
    fn total_seconds_is_zero_for_empty_db() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(db.total_seconds().unwrap(), 0);
    }

    #[test]
    fn total_seconds_sums_all_durations() {
        // Sums every session, regardless of label / mode / notes.
        let db = Database::open_in_memory().unwrap();
        db.insert_session(&Session {
            start_iso: "2026-04-27T10:00:00Z".to_string(),
            duration_secs: 600, label_id: None, notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        db.insert_session(&Session {
            start_iso: "2026-04-27T11:00:00Z".to_string(),
            duration_secs: 1245, label_id: None, notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        // Sub-minute remainder must NOT be lost — the whole point of
        // having a seconds aggregate alongside total_minutes.
        db.insert_session(&Session {
            start_iso: "2026-04-27T12:00:00Z".to_string(),
            duration_secs: 17, label_id: None, notes: None,
            mode: SessionMode::BoxBreath,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        assert_eq!(db.total_seconds().unwrap(), 600 + 1245 + 17);
    }

    #[test]
    fn total_minutes_agrees_with_total_seconds_div_60() {
        // After refactoring total_minutes to delegate to total_seconds,
        // the contract is: minutes = seconds / 60 (integer division).
        let db = Database::open_in_memory().unwrap();
        for &secs in &[59i64, 60, 61, 119, 120, 600, 1245] {
            db.insert_session(&Session {
                start_iso: format!("2026-04-27T10:{:02}:00Z", secs % 60),
                duration_secs: secs as u32, label_id: None, notes: None,
                mode: SessionMode::Timer,
                uuid: String::new(),
                guided_file_uuid: None,
            }).unwrap();
        }
        let secs = db.total_seconds().unwrap();
        let mins = db.total_minutes().unwrap();
        assert_eq!(mins, secs / 60);
    }

    // ── query_sessions: rich filter for the log feed ──────────────────────────

    #[test]
    fn query_sessions_default_filter_returns_all_newest_first() {
        // Default-constructed SessionFilter: no filter, no pagination —
        // every session, ordered start_iso DESC (newest first), to match
        // the log feed UX.
        let db = Database::open_in_memory().unwrap();
        let make = |iso: &str| Session {
            start_iso: iso.to_string(),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        };
        let _id_old = db.insert_session(&make("2026-04-25T10:00:00Z")).unwrap();
        let _id_new = db.insert_session(&make("2026-04-27T10:00:00Z")).unwrap();
        let _id_mid = db.insert_session(&make("2026-04-26T10:00:00Z")).unwrap();

        let rows = db.query_sessions(&SessionFilter::default()).unwrap();
        let isos: Vec<&str> = rows.iter().map(|(_, s)| s.start_iso.as_str()).collect();
        assert_eq!(
            isos,
            vec!["2026-04-27T10:00:00Z", "2026-04-26T10:00:00Z", "2026-04-25T10:00:00Z"],
            "rows must be ordered start_iso DESC",
        );
    }

    #[test]
    fn query_sessions_empty_db_returns_empty_vec() {
        // No rows — not an error, just an empty Vec.
        let db = Database::open_in_memory().unwrap();
        let rows = db.query_sessions(&SessionFilter::default()).unwrap();
        assert!(rows.is_empty());
    }

    #[test]
    fn query_sessions_limit_caps_result_count() {
        // limit=N returns at most N rows; the cap applies AFTER ordering,
        // so the newest N are returned.
        let db = Database::open_in_memory().unwrap();
        for d in 20..28 {
            db.insert_session(&Session {
                start_iso: format!("2026-04-{d:02}T10:00:00Z"),
                duration_secs: 600,
                label_id: None,
                notes: None,
                mode: SessionMode::Timer,
                uuid: String::new(),
                guided_file_uuid: None,
            }).unwrap();
        }
        let rows = db.query_sessions(&SessionFilter {
            limit: Some(3), ..Default::default()
        }).unwrap();
        let isos: Vec<&str> = rows.iter().map(|(_, s)| s.start_iso.as_str()).collect();
        assert_eq!(
            isos,
            vec!["2026-04-27T10:00:00Z", "2026-04-26T10:00:00Z", "2026-04-25T10:00:00Z"],
            "limit=3 must return the newest 3",
        );
    }

    #[test]
    fn query_sessions_offset_skips_initial_rows() {
        // offset=N skips the first N (in DESC order). Combined with
        // limit, this is the pagination contract: "give me page p of size s"
        // is offset = (p-1)*s, limit = s.
        let db = Database::open_in_memory().unwrap();
        for d in 20..28 {
            db.insert_session(&Session {
                start_iso: format!("2026-04-{d:02}T10:00:00Z"),
                duration_secs: 600,
                label_id: None,
                notes: None,
                mode: SessionMode::Timer,
                uuid: String::new(),
                guided_file_uuid: None,
            }).unwrap();
        }
        // Page 2 of size 3: skip 3, take 3.
        let rows = db.query_sessions(&SessionFilter {
            limit: Some(3),
            offset: Some(3),
            ..Default::default()
        }).unwrap();
        let isos: Vec<&str> = rows.iter().map(|(_, s)| s.start_iso.as_str()).collect();
        assert_eq!(
            isos,
            vec!["2026-04-24T10:00:00Z", "2026-04-23T10:00:00Z", "2026-04-22T10:00:00Z"],
            "page 2 of size 3 must be rows 4-6 in DESC order",
        );
    }

    #[test]
    fn query_sessions_offset_past_total_returns_empty() {
        // Asking for a page past the end is not an error.
        let db = Database::open_in_memory().unwrap();
        db.insert_session(&Session {
            start_iso: "2026-04-27T10:00:00Z".to_string(),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        let rows = db.query_sessions(&SessionFilter {
            offset: Some(100),
            ..Default::default()
        }).unwrap();
        assert!(rows.is_empty());
    }

    #[test]
    fn query_sessions_label_id_filters_by_label() {
        // label_id=Some(id) keeps only sessions referencing that label.
        let db = Database::open_in_memory().unwrap();
        let morning = db.insert_label("Morning").unwrap();
        let evening = db.insert_label("Evening").unwrap();
        // 2 Morning, 1 Evening, 1 unlabeled.
        db.insert_session(&Session {
            start_iso: "2026-04-27T10:00:00Z".to_string(),
            duration_secs: 600, label_id: Some(morning),
            notes: None, mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        db.insert_session(&Session {
            start_iso: "2026-04-27T11:00:00Z".to_string(),
            duration_secs: 600, label_id: Some(morning),
            notes: None, mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        db.insert_session(&Session {
            start_iso: "2026-04-27T19:00:00Z".to_string(),
            duration_secs: 600, label_id: Some(evening),
            notes: None, mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        db.insert_session(&Session {
            start_iso: "2026-04-27T20:00:00Z".to_string(),
            duration_secs: 600, label_id: None,
            notes: None, mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();

        let rows = db.query_sessions(&SessionFilter {
            label_id: Some(morning), ..Default::default()
        }).unwrap();
        assert_eq!(rows.len(), 2);
        for (_, s) in &rows {
            assert_eq!(s.label_id, Some(morning));
        }
    }

    #[test]
    fn query_sessions_only_with_notes_excludes_empty_and_null() {
        // only_with_notes=true matches when notes IS NOT NULL AND notes != ''.
        // Both None (NULL in DB) and Some("") must be excluded.
        let db = Database::open_in_memory().unwrap();
        // With note.
        db.insert_session(&Session {
            start_iso: "2026-04-27T10:00:00Z".to_string(),
            duration_secs: 600, label_id: None,
            notes: Some("kept focus".to_string()),
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        // Without note (None).
        db.insert_session(&Session {
            start_iso: "2026-04-27T11:00:00Z".to_string(),
            duration_secs: 600, label_id: None,
            notes: None, mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        // Empty-string note — also excluded.
        db.insert_session(&Session {
            start_iso: "2026-04-27T12:00:00Z".to_string(),
            duration_secs: 600, label_id: None,
            notes: Some("".to_string()),
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();

        let rows = db.query_sessions(&SessionFilter {
            only_with_notes: true, ..Default::default()
        }).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].1.notes, Some("kept focus".to_string()));
    }

    #[test]
    fn query_sessions_combines_label_filter_and_notes_filter() {
        // Compound filter: label_id AND only_with_notes both apply.
        let db = Database::open_in_memory().unwrap();
        let morning = db.insert_label("Morning").unwrap();
        // Morning + note → kept.
        db.insert_session(&Session {
            start_iso: "2026-04-27T10:00:00Z".to_string(),
            duration_secs: 600, label_id: Some(morning),
            notes: Some("yes".to_string()),
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        // Morning, no note → dropped (notes filter).
        db.insert_session(&Session {
            start_iso: "2026-04-27T11:00:00Z".to_string(),
            duration_secs: 600, label_id: Some(morning),
            notes: None, mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        // No label, with note → dropped (label filter).
        db.insert_session(&Session {
            start_iso: "2026-04-27T12:00:00Z".to_string(),
            duration_secs: 600, label_id: None,
            notes: Some("orphan".to_string()),
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();

        let rows = db.query_sessions(&SessionFilter {
            label_id: Some(morning),
            only_with_notes: true,
            ..Default::default()
        }).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].1.notes, Some("yes".to_string()));
    }

    #[test]
    fn query_sessions_pagination_walks_all_rows_without_overlap() {
        // Walking pages of size N covers every row exactly once.
        let db = Database::open_in_memory().unwrap();
        for d in 1..=10 {
            db.insert_session(&Session {
                start_iso: format!("2026-04-{d:02}T10:00:00Z"),
                duration_secs: 600, label_id: None,
                notes: None, mode: SessionMode::Timer,
                uuid: String::new(),
                guided_file_uuid: None,
            }).unwrap();
        }
        let mut seen: Vec<i64> = Vec::new();
        let mut offset = 0u32;
        loop {
            let page = db.query_sessions(&SessionFilter {
                limit: Some(3),
                offset: Some(offset),
                ..Default::default()
            }).unwrap();
            if page.is_empty() { break; }
            for (id, _) in &page { seen.push(*id); }
            offset += page.len() as u32;
        }
        assert_eq!(seen.len(), 10);
        // No duplicates.
        let mut sorted = seen.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), 10);
    }


    #[test]
    fn empty_database_has_zero_sessions() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(db.count_sessions().unwrap(), 0);
    }

    #[test]
    fn insert_session_increases_count() {
        let db = Database::open_in_memory().unwrap();
        let session = Session {
            start_iso: "2026-04-27T10:00:00Z".to_string(),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        };
        db.insert_session(&session).unwrap();
        assert_eq!(db.count_sessions().unwrap(), 1);
    }

    #[test]
    fn insert_session_with_mode_guided_is_accepted_by_check_constraint() {
        // Sessions saved at the end of a guided meditation carry
        // mode='guided'. The schema's CHECK clause must accept it
        // alongside 'timer' and 'box_breath' or insert fails.
        let db = Database::open_in_memory().unwrap();
        let session = Session {
            start_iso: "2026-05-05T20:30:00Z".to_string(),
            duration_secs: 1200,
            label_id: None,
            notes: None,
            mode: SessionMode::Guided,
            uuid: String::new(),
            guided_file_uuid: None,
        };
        db.insert_session(&session).unwrap();
        assert_eq!(db.count_sessions().unwrap(), 1);
    }

    #[test]
    fn insert_session_with_guided_file_uuid_round_trips() {
        // A guided session that played a starred imported file carries
        // the file's uuid so the log / stats can show per-file aggregates
        // later. Verifies the column is actually persisted + read back.
        let db = Database::open_in_memory().unwrap();
        let file_uuid = "deadbeef-1234-5678-9abc-def012345678";
        let session = Session {
            start_iso: "2026-05-05T20:30:00Z".to_string(),
            duration_secs: 1200,
            label_id: None,
            notes: None,
            mode: SessionMode::Guided,
            uuid: String::new(),
            guided_file_uuid: Some(file_uuid.to_string()),
        };
        db.insert_session(&session).unwrap();
        let rows = db.query_sessions(&SessionFilter::default()).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].1.guided_file_uuid.as_deref(), Some(file_uuid));
    }

    #[test]
    fn insert_session_without_guided_file_uuid_round_trips_as_none() {
        // Transient one-off guided sessions don't reference a
        // library-stored file; the column must accept NULL.
        let db = Database::open_in_memory().unwrap();
        let session = Session {
            start_iso: "2026-05-05T21:00:00Z".to_string(),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Guided,
            uuid: String::new(),
            guided_file_uuid: None,
        };
        db.insert_session(&session).unwrap();
        let rows = db.query_sessions(&SessionFilter::default()).unwrap();
        assert_eq!(rows.len(), 1);
        assert!(rows[0].1.guided_file_uuid.is_none());
    }

    #[test]
    fn list_sessions_for_label_filters_by_label_id() {
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        let morning = db.find_label_by_name("Morning").unwrap().unwrap();
        let mut labeled = Session {
            start_iso: "2026-04-27T10:00:00Z".to_string(),
            duration_secs: 600,
            label_id: Some(morning),
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        };
        let unlabeled = Session {
            start_iso: "2026-04-27T19:00:00Z".to_string(),
            duration_secs: 300,
            label_id: None,
            notes: None,
            mode: SessionMode::BoxBreath,
            uuid: String::new(),
            guided_file_uuid: None,
        };
        let labeled_id = db.insert_session(&labeled).unwrap();
        db.insert_session(&unlabeled).unwrap();
        let rows = db.list_sessions_for_label(morning).unwrap();
        assert_eq!(rows.len(), 1, "only the labeled session must be returned");
        assert!(looks_like_uuid_v4(&rows[0].1.uuid));
        labeled.uuid = rows[0].1.uuid.clone();
        assert_eq!(rows, vec![(labeled_id, labeled)]);
    }

    #[test]
    fn list_sessions_round_trips_inserted_session() {
        let db = Database::open_in_memory().unwrap();
        let mut session = Session {
            start_iso: "2026-04-27T10:00:00Z".to_string(),
            duration_secs: 600,
            label_id: None,
            notes: Some("felt clear today".to_string()),
            mode: SessionMode::BoxBreath,
            uuid: String::new(),
            guided_file_uuid: None,
        };
        let id = db.insert_session(&session).unwrap();
        let rows = db.list_sessions().unwrap();
        assert_eq!(rows.len(), 1);
        assert!(looks_like_uuid_v4(&rows[0].1.uuid),
            "round-tripped session must carry a v4 uuid");
        // Adopt the DB-assigned uuid into the expected value so the full
        // struct comparison below covers every other field exactly.
        session.uuid = rows[0].1.uuid.clone();
        assert_eq!(rows, vec![(id, session)]);
    }

    #[test]
    fn list_sessions_returns_id_per_row_in_insert_order() {
        // Each retrieved row carries its DB rowid so callers can address it
        // for update / delete. Ids are SQLite AUTOINCREMENT, so they
        // increase strictly and start at 1 on a fresh DB.
        let db = Database::open_in_memory().unwrap();
        let make = |start: &str| Session {
            start_iso: start.to_string(),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        };
        let id1 = db.insert_session(&make("2026-04-27T10:00:00Z")).unwrap();
        let id2 = db.insert_session(&make("2026-04-27T11:00:00Z")).unwrap();
        let id3 = db.insert_session(&make("2026-04-27T12:00:00Z")).unwrap();
        assert_eq!(id1, 1);
        assert_eq!(id2, 2);
        assert_eq!(id3, 3);
        let rows = db.list_sessions().unwrap();
        let got_ids: Vec<i64> = rows.iter().map(|(id, _)| *id).collect();
        assert_eq!(got_ids, vec![id1, id2, id3]);
    }

    #[test]
    fn update_session_replaces_all_fields() {
        // Update is destructive: every field of the new Session value
        // overwrites the row, identified by id. The other rows stay
        // untouched.
        let db = Database::open_in_memory().unwrap();
        let original = Session {
            start_iso: "2026-04-27T10:00:00Z".to_string(),
            duration_secs: 600,
            label_id: None,
            notes: Some("first take".to_string()),
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        };
        let id = db.insert_session(&original).unwrap();

        // Insert a sibling that must remain untouched.
        let other_id = db.insert_session(&Session {
            start_iso: "2026-04-27T11:00:00Z".to_string(),
            duration_secs: 300,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();

        db.insert_label("Evening").unwrap();
        let evening = db.find_label_by_name("Evening").unwrap().unwrap();
        let mut updated = Session {
            start_iso: "2026-04-28T19:00:00Z".to_string(),
            duration_secs: 1500,
            label_id: Some(evening),
            notes: Some("after dinner".to_string()),
            mode: SessionMode::BoxBreath,
            uuid: String::new(),
            guided_file_uuid: None,
        };
        db.update_session(id, &updated).unwrap();

        let rows = db.list_sessions().unwrap();
        assert_eq!(rows.len(), 2);
        // Updated row reflects every new field. Its uuid is whatever the
        // DB assigned at insert time and must survive an update unchanged
        // — bind it into `updated.uuid` for the full struct comparison.
        let updated_row = rows.iter().find(|(rid, _)| *rid == id).unwrap();
        assert!(looks_like_uuid_v4(&updated_row.1.uuid));
        updated.uuid = updated_row.1.uuid.clone();
        assert_eq!(updated_row.1, updated);
        // Sibling row untouched.
        let other_row = rows.iter().find(|(rid, _)| *rid == other_id).unwrap();
        assert_eq!(other_row.1.start_iso, "2026-04-27T11:00:00Z");
        assert_eq!(other_row.1.duration_secs, 300);
        assert_eq!(other_row.1.mode, SessionMode::Timer);
        // Each row must carry its own distinct uuid.
        assert!(looks_like_uuid_v4(&other_row.1.uuid));
        assert_ne!(updated_row.1.uuid, other_row.1.uuid);
    }

    #[test]
    fn update_session_can_clear_label_and_notes() {
        // Optional fields go round-trip in both directions: a session
        // with a label/note can have them cleared by update.
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        let morning = db.find_label_by_name("Morning").unwrap().unwrap();
        let id = db.insert_session(&Session {
            start_iso: "2026-04-27T10:00:00Z".to_string(),
            duration_secs: 600,
            label_id: Some(morning),
            notes: Some("had a label".to_string()),
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        db.update_session(id, &Session {
            start_iso: "2026-04-27T10:00:00Z".to_string(),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        let row = &db.list_sessions().unwrap()[0].1;
        assert_eq!(row.label_id, None);
        assert_eq!(row.notes, None);
    }

    #[test]
    fn update_session_unknown_id_is_noop() {
        // Updating a non-existent row is silent — matches SQLite's
        // UPDATE-by-id behaviour. The DB stays unchanged.
        let db = Database::open_in_memory().unwrap();
        let id = db.insert_session(&Session {
            start_iso: "2026-04-27T10:00:00Z".to_string(),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        db.update_session(id + 999, &Session {
            start_iso: "2099-01-01T00:00:00Z".to_string(),
            duration_secs: 9999,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        // Original row is intact.
        let rows = db.list_sessions().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].1.duration_secs, 600);
        assert_eq!(rows[0].1.start_iso, "2026-04-27T10:00:00Z");
    }

    #[test]
    fn delete_session_removes_only_the_addressed_row() {
        // Delete addresses one row by id; siblings are untouched.
        let db = Database::open_in_memory().unwrap();
        let make = |start: &str| Session {
            start_iso: start.to_string(),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        };
        let id1 = db.insert_session(&make("2026-04-27T10:00:00Z")).unwrap();
        let id2 = db.insert_session(&make("2026-04-27T11:00:00Z")).unwrap();
        let id3 = db.insert_session(&make("2026-04-27T12:00:00Z")).unwrap();

        db.delete_session(id2).unwrap();

        let surviving_ids: Vec<i64> =
            db.list_sessions().unwrap().into_iter().map(|(i, _)| i).collect();
        assert_eq!(surviving_ids, vec![id1, id3]);
        assert_eq!(db.count_sessions().unwrap(), 2);
    }

    #[test]
    fn delete_session_unknown_id_is_noop() {
        // Matches SQLite DELETE semantics: missing id is silent.
        let db = Database::open_in_memory().unwrap();
        let id = db.insert_session(&Session {
            start_iso: "2026-04-27T10:00:00Z".to_string(),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        db.delete_session(id + 999).unwrap();
        // Original row still there.
        assert_eq!(db.count_sessions().unwrap(), 1);
    }

    #[test]
    fn delete_session_does_not_remove_referenced_label() {
        // Labels survive their sessions — the FK is set-null on the
        // sessions side, not cascade-delete on the labels side.
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        let morning = db.find_label_by_name("Morning").unwrap().unwrap();
        let id = db.insert_session(&Session {
            start_iso: "2026-04-27T10:00:00Z".to_string(),
            duration_secs: 600,
            label_id: Some(morning),
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();

        db.delete_session(id).unwrap();

        // Label outlives the session.
        let names: Vec<String> =
            db.list_labels().unwrap().into_iter().map(|l| l.name).collect();
        assert_eq!(names, vec!["Morning"]);
        assert_eq!(db.count_labels().unwrap(), 1);
    }

    #[test]
    fn insert_session_with_unknown_label_id_is_rejected_by_fk() {
        // The labels.id ↔ sessions.label_id link is an enforced foreign key,
        // not just documentation. Inserting a session that points at a
        // non-existent label fails — the DB is the last line of defense
        // against UI bugs that pass through bad ids.
        let db = Database::open_in_memory().unwrap();
        // Sanity: the PRAGMA must be on for the FK clause to actually fire.
        let pragma: i64 = db.conn.query_row("PRAGMA foreign_keys", [], |r| r.get(0)).unwrap();
        assert_eq!(pragma, 1, "PRAGMA foreign_keys must be ON");

        let bad_id = 9999i64;
        let result = db.insert_session(&Session {
            start_iso: "2026-04-27T10:00:00Z".to_string(),
            duration_secs: 600,
            label_id: Some(bad_id),
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        });
        assert!(result.is_err(), "expected FK violation, got {result:?}");
        // No row landed.
        assert_eq!(db.count_sessions().unwrap(), 0);
    }

    #[test]
    fn bulk_insert_sessions_inserts_every_row_and_returns_count() {
        // Bulk insert is the import-CSV path's transactional API: every
        // row in the slice goes in (or none on error — see rollback test).
        // Returns the count for "imported N sessions" toasts.
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        let morning = db.find_label_by_name("Morning").unwrap().unwrap();

        let to_insert = vec![
            Session {
                start_iso: "2026-04-27T10:00:00Z".to_string(),
                duration_secs: 600,
                label_id: Some(morning),
                notes: Some("first".to_string()),
                mode: SessionMode::Timer,
                uuid: String::new(),
                guided_file_uuid: None,
            },
            Session {
                start_iso: "2026-04-27T11:00:00Z".to_string(),
                duration_secs: 1200,
                label_id: None,
                notes: None,
                mode: SessionMode::Timer,
                uuid: String::new(),
                guided_file_uuid: None,
            },
            Session {
                start_iso: "2026-04-27T12:00:00Z".to_string(),
                duration_secs: 300,
                label_id: Some(morning),
                notes: None,
                mode: SessionMode::BoxBreath,
                uuid: String::new(),
                guided_file_uuid: None,
            },
        ];

        let n = db.bulk_insert_sessions(&to_insert).unwrap();
        assert_eq!(n, 3);
        assert_eq!(db.count_sessions().unwrap(), 3);

        // Every row round-trips through the DB unchanged. The DB assigns
        // each row a fresh v4 uuid that the input doesn't carry — verify
        // each is well-formed, then graft it onto the expected value
        // before comparing the rest of the fields.
        let mut stored: Vec<Session> = db.list_sessions()
            .unwrap()
            .into_iter()
            .map(|(_, s)| s)
            .collect();
        let mut expected = to_insert.clone();
        for (got, want) in stored.iter().zip(expected.iter_mut()) {
            assert!(looks_like_uuid_v4(&got.uuid),
                "bulk-inserted row missing v4 uuid: {got:?}");
            want.uuid = got.uuid.clone();
        }
        // All uuids must also be distinct.
        let unique: std::collections::HashSet<_> =
            stored.iter().map(|s| s.uuid.clone()).collect();
        assert_eq!(unique.len(), stored.len(), "bulk insert must give unique uuids");
        // Strip nothing here: we've populated `expected.uuid` to match.
        let _ = stored.iter_mut(); // silence "doesn't need mut" if linter trips
        assert_eq!(stored, expected);
    }

    #[test]
    fn bulk_insert_sessions_empty_slice_is_zero_and_no_op() {
        // Empty input is not an error; the DB is unchanged.
        let db = Database::open_in_memory().unwrap();
        let n = db.bulk_insert_sessions(&[]).unwrap();
        assert_eq!(n, 0);
        assert_eq!(db.count_sessions().unwrap(), 0);
    }

    #[test]
    fn bulk_insert_sessions_rolls_back_on_constraint_violation() {
        // If any row in the batch violates a constraint (here: a foreign-key
        // pointing at a non-existent label), the WHOLE batch is reverted —
        // the caller never gets a half-imported DB.
        let db = Database::open_in_memory().unwrap();
        let pre_id = db.insert_session(&Session {
            start_iso: "2026-04-27T09:00:00Z".to_string(),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        assert_eq!(db.count_sessions().unwrap(), 1);

        let bad_label = 9999i64; // No label has this id.
        let batch = vec![
            Session {
                start_iso: "2026-04-27T10:00:00Z".to_string(),
                duration_secs: 600,
                label_id: None, // OK
                notes: None,
                mode: SessionMode::Timer,
                uuid: String::new(),
                guided_file_uuid: None,
            },
            Session {
                start_iso: "2026-04-27T11:00:00Z".to_string(),
                duration_secs: 600,
                label_id: Some(bad_label), // FK violation
                notes: None,
                mode: SessionMode::Timer,
                uuid: String::new(),
                guided_file_uuid: None,
            },
        ];
        let result = db.bulk_insert_sessions(&batch);
        assert!(result.is_err(), "expected FK violation, got {result:?}");

        // No rows from the failed batch landed; the pre-existing row is intact.
        assert_eq!(db.count_sessions().unwrap(), 1);
        let rows = db.list_sessions().unwrap();
        assert_eq!(rows[0].0, pre_id);
    }

    #[test]
    fn bulk_insert_sessions_is_atomic_with_no_partial_state_visible() {
        // Atomic-on-error: even after a failed bulk insert, count_sessions
        // and list_sessions agree on the pre-batch state. (This pins the
        // contract: "rolled back" means no observable side effect, not
        // just "rows aren't there".)
        let db = Database::open_in_memory().unwrap();
        let bad_label = 9999i64;
        let batch = vec![
            Session {
                start_iso: "2026-04-27T10:00:00Z".to_string(),
                duration_secs: 600,
                label_id: Some(bad_label), // fails immediately
                notes: None,
                mode: SessionMode::Timer,
                uuid: String::new(),
                guided_file_uuid: None,
            },
        ];
        let _ = db.bulk_insert_sessions(&batch);
        assert_eq!(db.count_sessions().unwrap(), 0);
        assert!(db.list_sessions().unwrap().is_empty());
    }

    #[test]
    fn delete_all_sessions_returns_count_and_clears_table() {
        // Wipe-all returns the row count so the caller can show "deleted N
        // sessions" toasts. Labels survive (this is a sessions-only nuke).
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        let morning = db.find_label_by_name("Morning").unwrap().unwrap();
        for i in 0..3 {
            db.insert_session(&Session {
                start_iso: format!("2026-04-2{i}T10:00:00Z"),
                duration_secs: 600,
                label_id: Some(morning),
                notes: None,
                mode: SessionMode::Timer,
                uuid: String::new(),
                guided_file_uuid: None,
            }).unwrap();
        }
        assert_eq!(db.count_sessions().unwrap(), 3);

        let removed = db.delete_all_sessions().unwrap();
        assert_eq!(removed, 3);
        assert_eq!(db.count_sessions().unwrap(), 0);
        assert!(db.list_sessions().unwrap().is_empty());

        // Labels untouched.
        let names: Vec<String> =
            db.list_labels().unwrap().into_iter().map(|l| l.name).collect();
        assert_eq!(names, vec!["Morning"]);
    }

    #[test]
    fn delete_all_sessions_on_empty_db_returns_zero() {
        // Idempotent: nothing to delete is not an error.
        let db = Database::open_in_memory().unwrap();
        assert_eq!(db.delete_all_sessions().unwrap(), 0);
        assert_eq!(db.count_sessions().unwrap(), 0);
    }

    #[test]
    fn list_sessions_for_label_returns_id_per_row() {
        // Filtered list must also carry ids — same contract.
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        let morning = db.find_label_by_name("Morning").unwrap().unwrap();
        let mut labeled = Session {
            start_iso: "2026-04-27T10:00:00Z".to_string(),
            duration_secs: 600,
            label_id: Some(morning),
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        };
        let id = db.insert_session(&labeled).unwrap();
        // Insert a second, unlabeled session — must not appear.
        db.insert_session(&Session {
            start_iso: "2026-04-27T11:00:00Z".to_string(),
            duration_secs: 300,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        let rows = db.list_sessions_for_label(morning).unwrap();
        assert_eq!(rows.len(), 1, "only the labeled session must be returned");
        assert!(looks_like_uuid_v4(&rows[0].1.uuid));
        labeled.uuid = rows[0].1.uuid.clone();
        assert_eq!(rows, vec![(id, labeled)]);
    }

    #[test]
    fn total_minutes_sums_durations_across_sessions() {
        let db = Database::open_in_memory().unwrap();
        let session_with_dur = |dur_secs| Session {
            start_iso: "2026-04-27T10:00:00Z".to_string(),
            duration_secs: dur_secs,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        };
        db.insert_session(&session_with_dur(600)).unwrap(); // 10 min
        db.insert_session(&session_with_dur(900)).unwrap(); // 15 min
        assert_eq!(db.total_minutes().unwrap(), 25);
    }

    #[test]
    fn total_minutes_is_zero_for_empty_db() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(db.total_minutes().unwrap(), 0);
    }

    #[test]
    fn total_minutes_by_label_groups_per_label() {
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Evening").unwrap();
        db.insert_label("Morning").unwrap();
        let evening = db.find_label_by_name("Evening").unwrap();
        let morning = db.find_label_by_name("Morning").unwrap();
        // Morning: 600 + 1200 = 1800s = 30m
        db.insert_session(&Session {
            duration_secs: 600,
            label_id: morning,
            ..session_on("2026-04-27")
        })
        .unwrap();
        db.insert_session(&Session {
            duration_secs: 1200,
            label_id: morning,
            ..session_on("2026-04-26")
        })
        .unwrap();
        // Evening: 300s = 5m
        db.insert_session(&Session {
            duration_secs: 300,
            label_id: evening,
            ..session_on("2026-04-27")
        })
        .unwrap();
        // SQLite default ORDER BY name puts ASCII "Evening" before "Morning".
        assert_eq!(
            db.total_minutes_by_label().unwrap(),
            vec![
                (Some("Evening".to_string()), 5),
                (Some("Morning".to_string()), 30),
            ]
        );
    }

    #[test]
    fn total_minutes_by_label_includes_unlabeled_as_none() {
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        let morning = db.find_label_by_name("Morning").unwrap();
        db.insert_session(&Session {
            duration_secs: 600,
            label_id: morning,
            ..session_on("2026-04-27")
        })
        .unwrap();
        db.insert_session(&Session {
            duration_secs: 300,
            label_id: None,
            ..session_on("2026-04-27")
        })
        .unwrap();
        // SQLite ORDER BY ASC sorts NULL first.
        assert_eq!(
            db.total_minutes_by_label().unwrap(),
            vec![(None, 5), (Some("Morning".to_string()), 10)]
        );
    }

    #[test]
    fn total_minutes_by_label_is_empty_for_empty_db() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(db.total_minutes_by_label().unwrap(), vec![]);
    }

    #[test]
    fn count_sessions_by_label_groups_per_label() {
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        let morning = db.find_label_by_name("Morning").unwrap();
        db.insert_session(&Session {
            label_id: morning,
            ..session_on("2026-04-27")
        })
        .unwrap();
        db.insert_session(&Session {
            label_id: morning,
            ..session_on("2026-04-26")
        })
        .unwrap();
        db.insert_session(&Session {
            label_id: None,
            ..session_on("2026-04-25")
        })
        .unwrap();
        assert_eq!(
            db.count_sessions_by_label().unwrap(),
            vec![(None, 1), (Some("Morning".to_string()), 2)]
        );
    }

    fn date(y: i32, m: u32, d: u32) -> chrono::NaiveDate {
        chrono::NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn streak_is_zero_for_empty_db() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(db.get_streak(date(2026, 4, 27)).unwrap(), 0);
    }

    fn session_on(day: &str) -> Session {
        Session {
            start_iso: format!("{day}T10:00:00Z"),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }
    }

    #[test]
    fn streak_is_one_with_single_session_today() {
        let db = Database::open_in_memory().unwrap();
        db.insert_session(&session_on("2026-04-27")).unwrap();
        assert_eq!(db.get_streak(date(2026, 4, 27)).unwrap(), 1);
    }

    #[test]
    fn streak_counts_consecutive_days_back_from_today() {
        let db = Database::open_in_memory().unwrap();
        db.insert_session(&session_on("2026-04-27")).unwrap();
        db.insert_session(&session_on("2026-04-26")).unwrap();
        db.insert_session(&session_on("2026-04-25")).unwrap();
        assert_eq!(db.get_streak(date(2026, 4, 27)).unwrap(), 3);
    }

    #[test]
    fn streak_breaks_at_first_gap() {
        let db = Database::open_in_memory().unwrap();
        db.insert_session(&session_on("2026-04-27")).unwrap();
        // gap on 2026-04-26
        db.insert_session(&session_on("2026-04-25")).unwrap();
        db.insert_session(&session_on("2026-04-24")).unwrap();
        assert_eq!(db.get_streak(date(2026, 4, 27)).unwrap(), 1);
    }

    #[test]
    fn streak_includes_yesterday_when_no_session_today() {
        // Forgiving variant: streak still alive if you meditated yesterday.
        let db = Database::open_in_memory().unwrap();
        db.insert_session(&session_on("2026-04-26")).unwrap();
        db.insert_session(&session_on("2026-04-25")).unwrap();
        assert_eq!(db.get_streak(date(2026, 4, 27)).unwrap(), 2);
    }

    #[test]
    fn streak_is_zero_when_most_recent_session_is_older_than_yesterday() {
        let db = Database::open_in_memory().unwrap();
        db.insert_session(&session_on("2026-04-24")).unwrap();
        assert_eq!(db.get_streak(date(2026, 4, 27)).unwrap(), 0);
    }

    #[test]
    fn streak_counts_each_day_once_even_with_multiple_sessions() {
        let db = Database::open_in_memory().unwrap();
        db.insert_session(&Session {
            start_iso: "2026-04-27T08:00:00Z".to_string(),
            ..session_on("2026-04-27")
        })
        .unwrap();
        db.insert_session(&Session {
            start_iso: "2026-04-27T19:00:00Z".to_string(),
            ..session_on("2026-04-27")
        })
        .unwrap();
        assert_eq!(db.get_streak(date(2026, 4, 27)).unwrap(), 1);
    }

    #[test]
    fn best_streak_is_zero_for_empty_db() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(db.get_best_streak().unwrap(), 0);
    }

    #[test]
    fn streak_for_label_only_counts_sessions_with_that_label() {
        let db = Database::open_in_memory().unwrap();
        let today = date(2026, 4, 27);
        db.insert_label("Morning").unwrap();
        db.insert_label("Evening").unwrap();
        let morning = db.find_label_by_name("Morning").unwrap().unwrap();
        let evening = db.find_label_by_name("Evening").unwrap().unwrap();
        // Today: Morning + Evening sessions.
        db.insert_session(&Session {
            label_id: Some(morning),
            ..session_on("2026-04-27")
        })
        .unwrap();
        db.insert_session(&Session {
            label_id: Some(evening),
            ..session_on("2026-04-27")
        })
        .unwrap();
        // Yesterday: Morning only.
        db.insert_session(&Session {
            label_id: Some(morning),
            ..session_on("2026-04-26")
        })
        .unwrap();
        // 2 days ago: Evening only.
        db.insert_session(&Session {
            label_id: Some(evening),
            ..session_on("2026-04-25")
        })
        .unwrap();
        // Morning streak: today + yesterday = 2 (gap on day-2).
        assert_eq!(db.get_streak_for_label(today, morning).unwrap(), 2);
        // Evening streak: today only (gap on yesterday).
        assert_eq!(db.get_streak_for_label(today, evening).unwrap(), 1);
        // Overall streak (no filter): today + yesterday + day-2 = 3.
        assert_eq!(db.get_streak(today).unwrap(), 3);
    }

    #[test]
    fn streak_and_best_streak_diverge_when_current_run_is_shorter() {
        // Mirrors `streak_gap_separates_current_from_best` from the existing app:
        // an old 6-day run, a gap, then a recent 3-day run ending today.
        let db = Database::open_in_memory().unwrap();
        let today = date(2026, 4, 27);
        // Old run: 30..25 days ago (6 days).
        for offset in 25..=30 {
            let day = today - chrono::Duration::days(offset);
            db.insert_session(&session_on(&day.format("%Y-%m-%d").to_string()))
                .unwrap();
        }
        // Current run: 0..2 days ago (3 days).
        for offset in 0..=2 {
            let day = today - chrono::Duration::days(offset);
            db.insert_session(&session_on(&day.format("%Y-%m-%d").to_string()))
                .unwrap();
        }
        assert_eq!(db.get_streak(today).unwrap(), 3, "current streak");
        assert_eq!(db.get_best_streak().unwrap(), 6, "best historical streak");
    }

    #[test]
    fn best_streak_for_label_only_counts_sessions_with_that_label() {
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        db.insert_label("Evening").unwrap();
        let morning = db.find_label_by_name("Morning").unwrap().unwrap();
        let evening = db.find_label_by_name("Evening").unwrap().unwrap();
        // Morning has a 3-day run.
        for d in ["2026-04-25", "2026-04-26", "2026-04-27"] {
            db.insert_session(&Session {
                label_id: Some(morning),
                ..session_on(d)
            })
            .unwrap();
        }
        // Evening has a 5-day run (longer overall, but for Morning it's irrelevant).
        for d in [
            "2026-04-01", "2026-04-02", "2026-04-03", "2026-04-04", "2026-04-05",
        ] {
            db.insert_session(&Session {
                label_id: Some(evening),
                ..session_on(d)
            })
            .unwrap();
        }
        assert_eq!(db.get_best_streak_for_label(morning).unwrap(), 3);
        assert_eq!(db.get_best_streak_for_label(evening).unwrap(), 5);
        // Overall best ignores label and finds the longest run anywhere.
        assert_eq!(db.get_best_streak().unwrap(), 5);
    }

    #[test]
    fn best_streak_finds_longest_run_across_history() {
        let db = Database::open_in_memory().unwrap();
        // Run of 2: Apr 1-2
        db.insert_session(&session_on("2026-04-01")).unwrap();
        db.insert_session(&session_on("2026-04-02")).unwrap();
        // Run of 4: Apr 10-13 (the best)
        db.insert_session(&session_on("2026-04-10")).unwrap();
        db.insert_session(&session_on("2026-04-11")).unwrap();
        db.insert_session(&session_on("2026-04-12")).unwrap();
        db.insert_session(&session_on("2026-04-13")).unwrap();
        // Run of 1: Apr 20
        db.insert_session(&session_on("2026-04-20")).unwrap();
        assert_eq!(db.get_best_streak().unwrap(), 4);
    }

    #[test]
    fn daily_totals_groups_durations_by_day() {
        let db = Database::open_in_memory().unwrap();
        // Two sessions same day → summed.
        db.insert_session(&Session {
            duration_secs: 600,
            ..session_on("2026-04-26")
        })
        .unwrap();
        db.insert_session(&Session {
            duration_secs: 300,
            ..session_on("2026-04-26")
        })
        .unwrap();
        // Different day, distinct entry.
        db.insert_session(&Session {
            duration_secs: 1200,
            ..session_on("2026-04-27")
        })
        .unwrap();
        assert_eq!(
            db.get_daily_totals().unwrap(),
            vec![(date(2026, 4, 26), 900), (date(2026, 4, 27), 1200)]
        );
    }

    #[test]
    fn daily_totals_is_empty_for_empty_db() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(db.get_daily_totals().unwrap(), vec![]);
    }

    #[test]
    fn daily_totals_for_label_filters_per_day() {
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        let morning = db.find_label_by_name("Morning").unwrap().unwrap();
        // Morning on Apr 26 (600s) and Apr 27 (1200s).
        db.insert_session(&Session {
            duration_secs: 600,
            label_id: Some(morning),
            ..session_on("2026-04-26")
        })
        .unwrap();
        db.insert_session(&Session {
            duration_secs: 1200,
            label_id: Some(morning),
            ..session_on("2026-04-27")
        })
        .unwrap();
        // Unlabeled on Apr 27 — must NOT show up in Morning's totals.
        db.insert_session(&Session {
            duration_secs: 9999,
            label_id: None,
            ..session_on("2026-04-27")
        })
        .unwrap();
        assert_eq!(
            db.get_daily_totals_for_label(morning).unwrap(),
            vec![(date(2026, 4, 26), 600), (date(2026, 4, 27), 1200)]
        );
    }

    #[test]
    fn open_creates_database_at_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.db");
        let db = Database::open(&path).unwrap();
        db.insert_label("Morning").unwrap();
        assert_eq!(db.count_labels().unwrap(), 1);
    }

    #[test]
    fn inserting_session_with_unknown_label_id_is_rejected() {
        let db = Database::open_in_memory().unwrap();
        let result = db.insert_session(&Session {
            start_iso: "2026-04-27T10:00:00Z".to_string(),
            duration_secs: 600,
            label_id: Some(999), // does not exist
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        });
        assert!(result.is_err(), "FK constraint should reject unknown label");
    }

    #[test]
    fn data_persists_across_reopens() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.db");
        {
            let db = Database::open(&path).unwrap();
            db.insert_label("Morning").unwrap();
            db.insert_session(&session_on("2026-04-27")).unwrap();
        }
        let db = Database::open(&path).unwrap();
        let names: Vec<String> =
            db.list_labels().unwrap().into_iter().map(|l| l.name).collect();
        assert_eq!(names, vec!["Morning"]);
        assert_eq!(db.count_sessions().unwrap(), 1);
    }

    #[test]
    fn running_average_is_zero_for_empty_db() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(
            db.get_running_average_secs(date(2026, 4, 27), 7).unwrap(),
            0.0
        );
    }

    #[test]
    fn running_average_handles_zero_days_without_divide_by_zero() {
        let db = Database::open_in_memory().unwrap();
        db.insert_session(&session_on("2026-04-27")).unwrap();
        assert_eq!(
            db.get_running_average_secs(date(2026, 4, 27), 0).unwrap(),
            0.0
        );
    }

    #[test]
    fn running_average_divides_total_by_window_days() {
        let db = Database::open_in_memory().unwrap();
        // 600s today, window of 1 day → average = 600.
        db.insert_session(&Session {
            duration_secs: 600,
            ..session_on("2026-04-27")
        })
        .unwrap();
        assert_eq!(
            db.get_running_average_secs(date(2026, 4, 27), 1).unwrap(),
            600.0
        );
        // Same data, window of 2 days → average = 300.
        assert_eq!(
            db.get_running_average_secs(date(2026, 4, 27), 2).unwrap(),
            300.0
        );
    }

    #[test]
    fn running_average_excludes_sessions_outside_window() {
        let db = Database::open_in_memory().unwrap();
        // Today: 600s — inside any window.
        db.insert_session(&Session {
            duration_secs: 600,
            ..session_on("2026-04-27")
        })
        .unwrap();
        // 10 days ago: 1200s — outside a 7-day window.
        db.insert_session(&Session {
            duration_secs: 1200,
            ..session_on("2026-04-17")
        })
        .unwrap();
        // Window of 7 days = today and 6 prior days; only today's 600s counts.
        let avg = db.get_running_average_secs(date(2026, 4, 27), 7).unwrap();
        assert!((avg - (600.0 / 7.0)).abs() < 1e-9, "got {avg}");
    }

    #[test]
    fn median_duration_is_none_for_empty_db() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(db.get_median_duration_secs().unwrap(), None);
    }

    #[test]
    fn median_duration_returns_middle_for_odd_count() {
        let db = Database::open_in_memory().unwrap();
        for d in [300u32, 600, 900, 1200, 1500] {
            db.insert_session(&Session {
                duration_secs: d,
                ..session_on("2026-04-27")
            })
            .unwrap();
        }
        assert_eq!(db.get_median_duration_secs().unwrap(), Some(900));
    }

    #[test]
    fn median_duration_uses_lower_median_for_even_count() {
        let db = Database::open_in_memory().unwrap();
        // Sorted: [300, 600, 900, 1200]. Lower median = 600.
        for d in [600u32, 1200, 300, 900] {
            db.insert_session(&Session {
                duration_secs: d,
                ..session_on("2026-04-27")
            })
            .unwrap();
        }
        assert_eq!(db.get_median_duration_secs().unwrap(), Some(600));
    }

    #[test]
    fn csv_round_trips_sessions_with_labels() {
        let src = Database::open_in_memory().unwrap();
        src.insert_label("Morning").unwrap();
        let morning_id = src.find_label_by_name("Morning").unwrap();
        src.insert_session(&Session {
            start_iso: "2026-04-27T10:00:00Z".to_string(),
            duration_secs: 600,
            label_id: morning_id,
            notes: Some("clear, focused".to_string()), // comma forces CSV quoting
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        })
        .unwrap();
        src.insert_session(&Session {
            start_iso: "2026-04-27T19:00:00Z".to_string(),
            duration_secs: 1200,
            label_id: None,
            notes: None,
            mode: SessionMode::BoxBreath,
            uuid: String::new(),
            guided_file_uuid: None,
        })
        .unwrap();

        let mut buf = Vec::new();
        src.export_sessions_csv(&mut buf).unwrap();

        let dst = Database::open_in_memory().unwrap();
        let imported = dst.import_sessions_csv(&buf[..]).unwrap();
        assert_eq!(imported, 2);

        // Label was created on import.
        let dst_names: Vec<String> =
            dst.list_labels().unwrap().into_iter().map(|l| l.name).collect();
        assert_eq!(dst_names, vec!["Morning"]);
        let dst_morning_id = dst.find_label_by_name("Morning").unwrap();

        // CSV import generates fresh v4 uuids on the destination DB
        // (uuids aren't part of the CSV format). Verify each row carries
        // one, then bind it into the expected struct so the full
        // comparison below also covers the rest of the fields.
        let sessions = dst.list_sessions().unwrap();
        assert_eq!(sessions.len(), 2);
        assert!(looks_like_uuid_v4(&sessions[0].1.uuid));
        assert!(looks_like_uuid_v4(&sessions[1].1.uuid));
        assert_ne!(sessions[0].1.uuid, sessions[1].1.uuid);
        assert_eq!(
            sessions[0].1,
            Session {
                start_iso: "2026-04-27T10:00:00Z".to_string(),
                duration_secs: 600,
                label_id: dst_morning_id,
                notes: Some("clear, focused".to_string()),
                mode: SessionMode::Timer,
                uuid: sessions[0].1.uuid.clone(),
                guided_file_uuid: None,
            }
        );
        assert_eq!(
            sessions[1].1,
            Session {
                start_iso: "2026-04-27T19:00:00Z".to_string(),
                duration_secs: 1200,
                label_id: None,
                notes: None,
                mode: SessionMode::BoxBreath,
                uuid: sessions[1].1.uuid.clone(),
                guided_file_uuid: None,
            }
        );
    }

    #[test]
    fn export_csv_writes_header_and_session_with_label_name() {
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        let label_id = db.find_label_by_name("Morning").unwrap();
        db.insert_session(&Session {
            start_iso: "2026-04-27T10:00:00Z".to_string(),
            duration_secs: 600,
            label_id,
            notes: Some("clear mind".to_string()),
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        })
        .unwrap();

        let mut buf = Vec::new();
        db.export_sessions_csv(&mut buf).unwrap();
        let csv = String::from_utf8(buf).unwrap();

        assert!(
            csv.contains("start_iso,duration_secs,label,notes,mode"),
            "missing header in:\n{csv}"
        );
        assert!(csv.contains("2026-04-27T10:00:00Z"));
        assert!(csv.contains("Morning"));
        assert!(csv.contains("clear mind"));
        assert!(csv.contains("timer"));
    }

    // ── UUIDs on sessions and labels (Nextcloud-Sync phase A1) ───────────────
    //
    // Every session and label row must carry a stable cross-device UUID.
    // The DB generates it at insert time — the value the caller puts in
    // the struct's `uuid` field is ignored. Reads round-trip the stored
    // UUID into the returned struct so the rest of the app (including
    // the future event log) can address rows by it.

    #[test]
    fn inserted_session_has_a_uuid_in_query_results() {
        let db = Database::open_in_memory().unwrap();
        db.insert_session(&Session {
            start_iso: "2026-04-27T10:00:00".to_string(),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
                        uuid: String::new(),  // ignored — DB assigns
                        guided_file_uuid: None,
        })
        .unwrap();
        let rows = db.list_sessions().unwrap();
        assert_eq!(rows.len(), 1);
        assert!(!rows[0].1.uuid.is_empty(), "uuid must be populated on read");
    }

    #[test]
    fn two_inserted_sessions_get_distinct_uuids() {
        let db = Database::open_in_memory().unwrap();
        for i in 0..2 {
            db.insert_session(&Session {
                start_iso: format!("2026-04-2{}T10:00:00", 7 + i),
                duration_secs: 600,
                label_id: None,
                notes: None,
                mode: SessionMode::Timer,
                                uuid: String::new(),
                                guided_file_uuid: None,
            })
            .unwrap();
        }
        let rows = db.list_sessions().unwrap();
        assert_eq!(rows.len(), 2);
        assert_ne!(rows[0].1.uuid, rows[1].1.uuid,
            "two inserts must produce distinct uuids");
    }

    #[test]
    fn inserted_session_uuid_is_v4_shaped() {
        let db = Database::open_in_memory().unwrap();
        db.insert_session(&Session {
            start_iso: "2026-04-27T10:00:00".to_string(),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
                        uuid: String::new(),
                        guided_file_uuid: None,
        })
        .unwrap();
        let uuid = &db.list_sessions().unwrap()[0].1.uuid;
        assert!(looks_like_uuid_v4(uuid),
            "session uuid `{uuid}` doesn't match v4 shape");
    }

    #[test]
    fn caller_supplied_session_uuid_is_ignored_in_favour_of_a_fresh_one() {
        // Documents that uuid is DB-assigned, not caller-controlled.
        // Belt-and-braces: if a caller accidentally reuses a uuid string
        // the DB still produces fresh, unique values — no collision risk.
        let db = Database::open_in_memory().unwrap();
        let bogus = "00000000-0000-4000-8000-000000000000".to_string();
        for i in 0..2 {
            db.insert_session(&Session {
                start_iso: format!("2026-04-2{}T10:00:00", 7 + i),
                duration_secs: 600,
                label_id: None,
                notes: None,
                mode: SessionMode::Timer,
                                uuid: bogus.clone(),
                                guided_file_uuid: None,
            })
            .unwrap();
        }
        let rows = db.list_sessions().unwrap();
        assert_ne!(rows[0].1.uuid, bogus, "DB must override caller's uuid");
        assert_ne!(rows[1].1.uuid, bogus, "DB must override caller's uuid");
        assert_ne!(rows[0].1.uuid, rows[1].1.uuid);
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


    // ── Event emission on mutations (A3) ─────────────────────────────────────
    //
    // Every state-changing operation appends a self-contained event to
    // `events` so peers can replay it. The local DB (`sessions`,
    // `labels`, `settings`) is the materialized cache derived from
    // those events; if the cache and the log disagree, the log wins on
    // every other device.


    // ── A3.1: insert_session emits a session_insert event ────────────────────

    #[test]
    fn insert_session_appends_exactly_one_session_insert_event() {
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
        let events = db.pending_events().unwrap();
        assert_eq!(events.len(), 1, "one insert must produce exactly one event");
        assert_eq!(events[0].1.kind, "session_insert");
    }

    #[test]
    fn session_insert_event_payload_contains_the_rows_uuid() {
        // The event's session uuid must match the row's uuid — that's
        // how peers cross-reference events to materialized rows.
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
        let payload = event_payload(&events[0].1);
        assert_eq!(payload["uuid"], serde_json::Value::String(row_uuid));
    }

    #[test]
    fn session_insert_event_payload_carries_every_relevant_field() {
        // Every column that a peer needs to reconstruct the row must be
        // present in the payload — start_iso, duration_secs, notes, mode.
        // label_uuid is null here (label_id is None); covered separately
        // when the session does have a label.
        let db = Database::open_in_memory().unwrap();
        db.insert_session(&Session {
            start_iso: "2026-04-30T10:00:00".to_string(),
            duration_secs: 1234,
            label_id: None,
            notes: Some("note text".to_string()),
            mode: SessionMode::BoxBreath,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        let payload = event_payload(&db.pending_events().unwrap()[0].1);
        assert_eq!(payload["start_iso"], "2026-04-30T10:00:00");
        assert_eq!(payload["duration_secs"], 1234);
        assert_eq!(payload["notes"], "note text");
        assert_eq!(payload["mode"], "box_breath");
        assert_eq!(payload["label_uuid"], serde_json::Value::Null);
    }

    #[test]
    fn session_insert_event_payload_label_uuid_resolves_from_label_id() {
        // sessions reference labels by rowid locally, but the event must
        // carry the label's UUID — the cross-device identity. The
        // resolution `label_id → label_uuid` happens at event-emission
        // time so a peer can apply the event without needing this
        // device's rowid space.
        let db = Database::open_in_memory().unwrap();
        let label_id = db.insert_label("Morning").unwrap();
        let label_uuid = db.list_labels().unwrap()[0].uuid.clone();
        // insert_label also emits an event — drain it before the session
        // insert so we can assert on a single event below.
        for (id, _) in db.pending_events().unwrap() {
            db.mark_event_synced(id).unwrap();
        }
        db.insert_session(&Session {
            start_iso: "2026-04-30T10:00:00".to_string(),
            duration_secs: 600,
            label_id: Some(label_id),
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        let payload = event_payload(&db.pending_events().unwrap()[0].1);
        assert_eq!(payload["label_uuid"], serde_json::Value::String(label_uuid));
    }

    #[test]
    fn session_insert_event_payload_serializes_notes_null_when_absent() {
        // `notes: None` round-trips through the payload as JSON null —
        // not an empty string, which would lose the "no notes" vs "empty
        // notes" distinction on a peer that re-applies the event.
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
        let payload = event_payload(&db.pending_events().unwrap()[0].1);
        assert_eq!(payload["notes"], serde_json::Value::Null);
    }

    #[test]
    fn session_insert_event_carries_this_devices_id() {
        let db = Database::open_in_memory().unwrap();
        let device_id = db.device_id().unwrap();
        db.insert_session(&Session {
            start_iso: "2026-04-30T10:00:00".to_string(),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        let events = db.pending_events().unwrap();
        assert_eq!(events[0].1.device_id, device_id,
            "event must be attributed to the authoring device");
    }

    #[test]
    fn session_insert_event_advances_the_lamport_clock() {
        // Bumping the clock on every authored event is what gives the
        // log a total order. After one insert, lamport must be ≥ 1; the
        // event's own ts must equal that bumped value.
        let db = Database::open_in_memory().unwrap();
        assert_eq!(db.lamport_clock().unwrap(), 0);
        db.insert_session(&Session {
            start_iso: "2026-04-30T10:00:00".to_string(),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        let lamport = db.lamport_clock().unwrap();
        assert!(lamport >= 1, "lamport must advance past zero");
        let events = db.pending_events().unwrap();
        assert_eq!(events[0].1.lamport_ts, lamport,
            "event ts must equal the post-bump clock value");
    }

    #[test]
    fn two_inserts_produce_two_distinct_events_in_lamport_order() {
        let db = Database::open_in_memory().unwrap();
        for i in 0..2 {
            db.insert_session(&Session {
                start_iso: format!("2026-04-3{}T10:00:00", i),
                duration_secs: 600,
                label_id: None,
                notes: None,
                mode: SessionMode::Timer,
                uuid: String::new(),
                guided_file_uuid: None,
            }).unwrap();
        }
        let events = db.pending_events().unwrap();
        assert_eq!(events.len(), 2);
        assert!(events[0].1.lamport_ts < events[1].1.lamport_ts,
            "events must be sorted ASC by lamport_ts");
        assert_ne!(events[0].1.event_uuid, events[1].1.event_uuid);
    }

    /// Drain every currently-pending event (mark them all synced) so
    /// follow-up assertions can focus on the events produced by a
    /// specific subsequent mutation. Returns nothing — callers don't
    /// care about the drained content, only that what comes next is
    /// observable in isolation.

    // ── A3.2: update_session and delete_session emit events ──────────────────

    #[test]
    fn update_session_appends_a_session_update_event() {
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
        drain_events(&db);

        db.update_session(id, &Session {
            start_iso: "2026-05-01T11:00:00".to_string(),
            duration_secs: 1800,
            label_id: None,
            notes: Some("revised".to_string()),
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        let events = db.pending_events().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].1.kind, "session_update");
    }

    #[test]
    fn session_update_event_payload_carries_the_rows_uuid_unchanged() {
        // The session's uuid is stable — update changes every other field
        // but the cross-device identity of the session is fixed at insert
        // time. The event must reference that same uuid so peers can
        // locate the row to update.
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
        let original_uuid = db.list_sessions().unwrap()[0].1.uuid.clone();
        drain_events(&db);

        db.update_session(id, &Session {
            start_iso: "2026-05-01T11:00:00".to_string(),
            duration_secs: 1800,
            label_id: None,
            notes: Some("revised".to_string()),
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        let payload = event_payload(&db.pending_events().unwrap()[0].1);
        assert_eq!(payload["uuid"], serde_json::Value::String(original_uuid));
    }

    #[test]
    fn session_update_event_payload_reflects_the_new_field_values() {
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
        drain_events(&db);

        db.update_session(id, &Session {
            start_iso: "2026-05-01T11:00:00".to_string(),
            duration_secs: 1800,
            label_id: None,
            notes: Some("revised".to_string()),
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        let payload = event_payload(&db.pending_events().unwrap()[0].1);
        assert_eq!(payload["start_iso"], "2026-05-01T11:00:00");
        assert_eq!(payload["duration_secs"], 1800);
        assert_eq!(payload["notes"], "revised");
        assert_eq!(payload["mode"], "timer");
    }

    #[test]
    fn session_update_event_payload_label_uuid_resolves_from_new_label() {
        // Updates can change the label — the event payload must reflect
        // the *new* label's uuid, not the old one or the rowid.
        let db = Database::open_in_memory().unwrap();
        let label_id = db.insert_label("Evening").unwrap();
        let label_uuid = db.list_labels().unwrap()[0].uuid.clone();
        let id = db.insert_session(&Session {
            start_iso: "2026-04-30T10:00:00".to_string(),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        drain_events(&db);

        db.update_session(id, &Session {
            start_iso: "2026-04-30T10:00:00".to_string(),
            duration_secs: 600,
            label_id: Some(label_id),
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        let payload = event_payload(&db.pending_events().unwrap()[0].1);
        assert_eq!(payload["label_uuid"], serde_json::Value::String(label_uuid));
    }

    #[test]
    fn update_session_unknown_id_emits_no_event() {
        // Defensive: an UPDATE that affects zero rows must NOT log a
        // ghost event referencing a uuid we don't know. Otherwise peers
        // would receive an update for a session they've never seen.
        let db = Database::open_in_memory().unwrap();
        drain_events(&db);
        db.update_session(9999, &Session {
            start_iso: "2026-05-01T11:00:00".to_string(),
            duration_secs: 1800,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        assert!(db.pending_events().unwrap().is_empty(),
            "no-match update must produce no event");
    }

    #[test]
    fn delete_session_appends_a_session_delete_event() {
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
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].1.kind, "session_delete");

        // Payload is just the uuid — peers don't need any other field
        // since the tombstone semantics is "drop the row by this id".
        let payload = event_payload(&events[0].1);
        assert_eq!(payload["uuid"], serde_json::Value::String(row_uuid));
    }

    #[test]
    fn delete_session_unknown_id_emits_no_event() {
        let db = Database::open_in_memory().unwrap();
        drain_events(&db);
        db.delete_session(9999).unwrap();
        assert!(db.pending_events().unwrap().is_empty(),
            "no-match delete must produce no event");
    }

    // ── A3.3: bulk operations emit one event per row ─────────────────────────

    #[test]
    fn bulk_insert_sessions_emits_one_event_per_row() {
        // Each row crosses the network as its own SessionInserted event —
        // the cross-device replay model has no concept of "bulk insert",
        // every row is independent. So N inputs must yield N events.
        let db = Database::open_in_memory().unwrap();
        let to_insert: Vec<Session> = (0..3).map(|i| Session {
            start_iso: format!("2026-04-3{i}T10:00:00"),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).collect();
        db.bulk_insert_sessions(&to_insert).unwrap();
        let events = db.pending_events().unwrap();
        assert_eq!(events.len(), 3,
            "three input rows must yield three events");
        for (_, e) in &events {
            assert_eq!(e.kind, "session_insert");
        }
    }

    #[test]
    fn bulk_insert_sessions_event_uuids_match_inserted_rows() {
        // Each event's session uuid must correspond to a stored row's
        // uuid — the set must be equal. Otherwise a peer would receive
        // events for rows we don't have, or skip rows we do.
        let db = Database::open_in_memory().unwrap();
        let to_insert: Vec<Session> = (0..3).map(|i| Session {
            start_iso: format!("2026-04-3{i}T10:00:00"),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).collect();
        db.bulk_insert_sessions(&to_insert).unwrap();
        let row_uuids: std::collections::HashSet<String> = db.list_sessions()
            .unwrap()
            .iter().map(|(_, s)| s.uuid.clone()).collect();
        let event_uuids: std::collections::HashSet<String> = db
            .pending_events()
            .unwrap()
            .iter()
            .map(|(_, e)| event_payload(e)["uuid"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(row_uuids, event_uuids,
            "every stored row must have a matching event, and vice versa");
    }

    #[test]
    fn bulk_insert_sessions_with_empty_slice_emits_no_events() {
        let db = Database::open_in_memory().unwrap();
        drain_events(&db);
        db.bulk_insert_sessions(&[]).unwrap();
        assert!(db.pending_events().unwrap().is_empty());
    }

    #[test]
    fn bulk_insert_session_events_have_strictly_increasing_lamport_ts() {
        // Replay order is determined by lamport_ts. Even within a bulk
        // op, each row gets its own ts so peers can apply them in a
        // consistent order across devices.
        let db = Database::open_in_memory().unwrap();
        let to_insert: Vec<Session> = (0..3).map(|i| Session {
            start_iso: format!("2026-04-3{i}T10:00:00"),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).collect();
        db.bulk_insert_sessions(&to_insert).unwrap();
        let timestamps: Vec<i64> = db.pending_events().unwrap()
            .iter().map(|(_, e)| e.lamport_ts).collect();
        let mut sorted = timestamps.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(timestamps.len(), sorted.len(),
            "every bulk-inserted event must have a unique lamport_ts: {timestamps:?}");
        assert_eq!(timestamps, sorted,
            "events must be returned in ascending lamport_ts order");
    }

    #[test]
    fn delete_all_sessions_emits_one_delete_event_per_existing_row() {
        let db = Database::open_in_memory().unwrap();
        for i in 0..3 {
            db.insert_session(&Session {
                start_iso: format!("2026-04-3{i}T10:00:00"),
                duration_secs: 600,
                label_id: None,
                notes: None,
                mode: SessionMode::Timer,
                uuid: String::new(),
                guided_file_uuid: None,
            }).unwrap();
        }
        let row_uuids: std::collections::HashSet<String> = db.list_sessions()
            .unwrap()
            .iter().map(|(_, s)| s.uuid.clone()).collect();
        drain_events(&db);

        let removed = db.delete_all_sessions().unwrap();
        assert_eq!(removed, 3);

        let events = db.pending_events().unwrap();
        assert_eq!(events.len(), 3,
            "delete_all must emit one delete event per row that was present");
        for (_, e) in &events {
            assert_eq!(e.kind, "session_delete");
        }
        let event_uuids: std::collections::HashSet<String> = events.iter()
            .map(|(_, e)| event_payload(e)["uuid"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(row_uuids, event_uuids,
            "every previously-present row must show up in a tombstone event");
    }

    #[test]
    fn delete_all_sessions_on_empty_database_emits_no_events() {
        let db = Database::open_in_memory().unwrap();
        drain_events(&db);
        let removed = db.delete_all_sessions().unwrap();
        assert_eq!(removed, 0);
        assert!(db.pending_events().unwrap().is_empty());
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

    // ── B1.1: apply_event for session events ─────────────────────────────────
    //
    // apply_event consumes a remote-authored event and updates the local
    // materialized cache. The model: record the event in `events`, then
    // recompute the cache row for its target_id from the events table —
    // tombstone wins on tie/precedence, otherwise the highest-lamport
    // mutate event drives the row's values. This makes apply_event
    // idempotent (re-applying same event_uuid is a no-op via INSERT OR
    // IGNORE) and order-independent (out-of-order delivery converges).

    use super::test_helpers::*;

    #[test]
    fn apply_event_session_insert_creates_the_row() {
        // Apply a single insert event from a peer; the cache row appears
        // with all the event's values.
        let db = Database::open_in_memory().unwrap();
        let event = synth_session_insert(
            SESSION_X, 5, DEVICE_A,
            "2026-04-30T10:00:00", 600,
            None, Some("from peer"), SessionMode::BoxBreath,
        );
        db.apply_event(&event).unwrap();
        let rows = db.list_sessions().unwrap();
        assert_eq!(rows.len(), 1);
        let s = &rows[0].1;
        assert_eq!(s.uuid, SESSION_X);
        assert_eq!(s.start_iso, "2026-04-30T10:00:00");
        assert_eq!(s.duration_secs, 600);
        assert_eq!(s.notes.as_deref(), Some("from peer"));
        assert_eq!(s.mode, SessionMode::BoxBreath);
    }

    #[test]
    fn apply_event_session_insert_with_guided_file_uuid_round_trips() {
        // A guided session synced from a peer carries the file's uuid
        // in the event payload so per-file stats stay consistent across
        // devices. recompute_session must lift `guided_file_uuid` out
        // of the JSON payload and write it to the column.
        let db = Database::open_in_memory().unwrap();
        let file_uuid = "fffffff0-0000-4000-8000-cccccccccccc";
        let event = synth_event(
            "session_insert",
            SESSION_X,
            7,
            DEVICE_A,
            serde_json::json!({
                "uuid": SESSION_X,
                "start_iso": "2026-05-05T20:30:00",
                "duration_secs": 1200,
                "label_uuid": serde_json::Value::Null,
                "notes": serde_json::Value::Null,
                "mode": "guided",
                "guided_file_uuid": file_uuid,
            }),
        );
        db.apply_event(&event).unwrap();
        let rows = db.list_sessions().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].1.guided_file_uuid.as_deref(), Some(file_uuid));
    }

    #[test]
    fn apply_event_session_insert_without_guided_file_uuid_leaves_column_null() {
        // Old-shape event payloads (no guided_file_uuid key) must
        // continue to work — recompute_session reads the field as
        // optional and writes NULL when missing.
        let db = Database::open_in_memory().unwrap();
        let event = synth_session_insert(
            SESSION_X, 5, DEVICE_A,
            "2026-04-30T10:00:00", 600,
            None, None, SessionMode::Timer,
        );
        db.apply_event(&event).unwrap();
        let rows = db.list_sessions().unwrap();
        assert_eq!(rows.len(), 1);
        assert!(rows[0].1.guided_file_uuid.is_none());
    }

    #[test]
    fn apply_event_is_idempotent_on_event_uuid() {
        // Applying the exact same Event twice must not double-insert
        // and must not error. The events table's UNIQUE(event_uuid)
        // is the dedup key.
        let db = Database::open_in_memory().unwrap();
        let event = synth_session_insert(
            SESSION_X, 5, DEVICE_A,
            "2026-04-30T10:00:00", 600,
            None, None, SessionMode::Timer,
        );
        db.apply_event(&event).unwrap();
        db.apply_event(&event).unwrap();
        assert_eq!(db.list_sessions().unwrap().len(), 1,
            "duplicate event_uuid must not create a second row");
    }

    #[test]
    fn apply_event_session_update_after_insert_updates_the_row() {
        let db = Database::open_in_memory().unwrap();
        db.apply_event(&synth_session_insert(
            SESSION_X, 5, DEVICE_A,
            "2026-04-30T10:00:00", 600,
            None, None, SessionMode::Timer,
        )).unwrap();
        db.apply_event(&synth_session_update(
            SESSION_X, 10, DEVICE_A,
            "2026-05-01T11:00:00", 1200,
            None, Some("revised"), SessionMode::Timer,
        )).unwrap();
        let s = &db.list_sessions().unwrap()[0].1;
        assert_eq!(s.start_iso, "2026-05-01T11:00:00");
        assert_eq!(s.duration_secs, 1200);
        assert_eq!(s.notes.as_deref(), Some("revised"));
        assert_eq!(s.mode, SessionMode::Timer);
    }

    #[test]
    fn apply_event_session_delete_removes_the_row() {
        let db = Database::open_in_memory().unwrap();
        db.apply_event(&synth_session_insert(
            SESSION_X, 5, DEVICE_A,
            "2026-04-30T10:00:00", 600,
            None, None, SessionMode::Timer,
        )).unwrap();
        db.apply_event(&synth_session_delete(SESSION_X, 10, DEVICE_A)).unwrap();
        assert!(db.list_sessions().unwrap().is_empty());
    }

    #[test]
    fn apply_event_tombstone_resists_later_applied_lower_lamport_insert() {
        // Out-of-order delivery: peer's delete arrives first (lamport=10),
        // then their insert at lamport=5 lands. The row must stay gone —
        // delete tombstones beat earlier inserts.
        let db = Database::open_in_memory().unwrap();
        db.apply_event(&synth_session_delete(SESSION_X, 10, DEVICE_A)).unwrap();
        db.apply_event(&synth_session_insert(
            SESSION_X, 5, DEVICE_A,
            "2026-04-30T10:00:00", 600,
            None, None, SessionMode::Timer,
        )).unwrap();
        assert!(db.list_sessions().unwrap().is_empty(),
            "tombstone with lamport 10 must beat insert at lamport 5");
    }

    #[test]
    fn apply_event_higher_lamport_update_supersedes_lower_one() {
        // Two updates from different devices on the same uuid; whichever
        // has the higher lamport_ts wins, regardless of arrival order.
        let db = Database::open_in_memory().unwrap();
        // Device A's update at lamport 10, Device B's at lamport 7 —
        // A wins. Apply B first (out of order), then A.
        db.apply_event(&synth_session_insert(
            SESSION_X, 1, DEVICE_A,
            "initial", 100, None, None, SessionMode::Timer,
        )).unwrap();
        db.apply_event(&synth_session_update(
            SESSION_X, 7, DEVICE_B,
            "B's edit", 700, None, Some("from B"), SessionMode::Timer,
        )).unwrap();
        db.apply_event(&synth_session_update(
            SESSION_X, 10, DEVICE_A,
            "A's edit", 1000, None, Some("from A"), SessionMode::BoxBreath,
        )).unwrap();
        let s = &db.list_sessions().unwrap()[0].1;
        assert_eq!(s.notes.as_deref(), Some("from A"),
            "A's lamport-10 update must win over B's lamport-7");
        assert_eq!(s.duration_secs, 1000);
    }

    #[test]
    fn apply_event_concurrent_updates_break_ties_on_device_id() {
        // Two updates with the SAME lamport_ts but different device_ids.
        // Lex-larger device_id wins (consistent across all peers per the
        // plan's tie-break rule).
        let db = Database::open_in_memory().unwrap();
        db.apply_event(&synth_session_insert(
            SESSION_X, 1, DEVICE_A,
            "initial", 100, None, None, SessionMode::Timer,
        )).unwrap();
        db.apply_event(&synth_session_update(
            SESSION_X, 5, DEVICE_A,
            "A wrote this", 500, None, Some("from A"), SessionMode::Timer,
        )).unwrap();
        db.apply_event(&synth_session_update(
            SESSION_X, 5, DEVICE_B,
            "B wrote this", 500, None, Some("from B"), SessionMode::Timer,
        )).unwrap();
        let s = &db.list_sessions().unwrap()[0].1;
        assert_eq!(s.notes.as_deref(), Some("from B"),
            "DEVICE_B is lex-larger than DEVICE_A; B's update wins on tie");
    }

    #[test]
    fn apply_event_records_the_event_in_the_log() {
        // After apply_event, the event must be in the events table so
        // future recomputes see it. Sync's push phase will pick it up
        // via pending_events (since `synced=0` by default).
        let db = Database::open_in_memory().unwrap();
        let event = synth_session_insert(
            SESSION_X, 5, DEVICE_A,
            "2026-04-30T10:00:00", 600,
            None, None, SessionMode::Timer,
        );
        let event_uuid = event.event_uuid.clone();
        db.apply_event(&event).unwrap();
        let pending = db.pending_events().unwrap();
        assert!(pending.iter().any(|(_, e)| e.event_uuid == event_uuid),
            "applied event must appear in events table");
    }

    #[test]
    fn apply_event_with_unknown_kind_is_a_silent_record_only() {
        // Forwards-compat: a future event kind we don't understand must
        // not panic or error. Record it — a future build can replay —
        // but don't try to mutate the cache from it.
        let db = Database::open_in_memory().unwrap();
        let weird = synth_event(
            "future_kind_not_yet_invented",
            SESSION_X, 5, DEVICE_A,
            serde_json::json!({"some": "future-data"}),
        );
        db.apply_event(&weird).unwrap();
        // Cache is empty (the event affected nothing it understood),
        // but the event was recorded.
        assert!(db.list_sessions().unwrap().is_empty());
        assert_eq!(db.pending_events().unwrap().len(), 1);
    }

    #[test]
    fn apply_event_session_insert_resolves_label_uuid_to_local_label_id() {
        // The peer's event references a label by label_uuid. If we have
        // a local label with that uuid, the materialized session must
        // link to it via local label_id. (Ensures cross-device
        // referential integrity survives the rowid-to-uuid translation.)
        let db = Database::open_in_memory().unwrap();
        let local_label_id = db.insert_label("Morning").unwrap();
        let label_uuid = db.list_labels().unwrap()[0].uuid.clone();
        drain_events(&db);

        db.apply_event(&synth_session_insert(
            SESSION_X, 5, DEVICE_A,
            "2026-04-30T10:00:00", 600,
            Some(&label_uuid), None, SessionMode::Timer,
        )).unwrap();
        let s = &db.list_sessions().unwrap()[0].1;
        assert_eq!(s.label_id, Some(local_label_id),
            "label_uuid must round-trip back to the local label_id");
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
