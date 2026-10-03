//! Lock-free count of local changes that need syncing.
//!
//! `emit_event` — the one place every local write records its sync
//! event — bumps it; events replayed from peers and device-local
//! writes (the running-session snapshot) don't. A shell holds a
//! `LocalChangeWatch` and asks it, from any thread and without the
//! DB lock, whether something new needs pushing, so no write path
//! can forget to start a sync.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Shared handle to one database's local-change count.
#[derive(Debug, Clone, Default)]
pub struct LocalChanges(Arc<AtomicU64>);

impl LocalChanges {
    pub fn count(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }

    pub(super) fn bump(&self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

/// Remembers the count it last reported, so each batch of changes
/// is reported once.
#[derive(Debug)]
pub struct LocalChangeWatch {
    changes: LocalChanges,
    seen: u64,
}

impl LocalChangeWatch {
    /// Starts from the current count: changes made before the watch
    /// existed are not reported.
    pub fn new(changes: LocalChanges) -> Self {
        let seen = changes.count();
        Self { changes, seen }
    }

    /// Whether anything changed since the last call.
    pub fn take_new(&mut self) -> bool {
        let now = self.changes.count();
        let new = now != self.seen;
        self.seen = now;
        new
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{
        BellSoundCategory, BoxBreathPhaseId, ChartKind, Database, IntervalBellKind, Session,
        SessionInProgress, SessionMode, SessionUuid, SignalMode, StarredState,
    };
    use crate::seeds::{BUNDLED_BELL_UUID, BUNDLED_PATTERN_PULSE_UUID};

    fn session(start_iso: &str) -> Session {
        Session {
            start_iso: start_iso.into(),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: SessionUuid::new(""),
            guided_file_uuid: None,
        }
    }

    fn pending(db: &Database) -> usize {
        db.pending_events().unwrap().len()
    }

    #[test]
    fn a_fresh_watch_sees_nothing() {
        let db = Database::open_in_memory().unwrap();
        let mut watch = LocalChangeWatch::new(db.local_changes());
        assert!(!watch.take_new());
    }

    #[test]
    fn a_watch_ignores_changes_made_before_it_existed() {
        // Startup writes are covered by the launch sync.
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Before").unwrap();
        let mut watch = LocalChangeWatch::new(db.local_changes());
        assert!(!watch.take_new());
    }

    #[test]
    fn one_change_is_seen_once() {
        let db = Database::open_in_memory().unwrap();
        let mut watch = LocalChangeWatch::new(db.local_changes());
        db.insert_label("Morning").unwrap();
        assert!(watch.take_new());
        assert!(!watch.take_new(), "already reported");
    }

    #[test]
    fn a_burst_of_changes_is_seen_as_one() {
        let db = Database::open_in_memory().unwrap();
        let mut watch = LocalChangeWatch::new(db.local_changes());
        for i in 0..5 {
            db.insert_session(&session(&format!("2026-10-0{}T07:00:00", i + 1))).unwrap();
        }
        assert!(watch.take_new());
        assert!(!watch.take_new());
    }

    #[test]
    fn the_handle_can_be_read_on_another_thread() {
        let db = Database::open_in_memory().unwrap();
        let handle = db.local_changes();
        let mut watch = LocalChangeWatch::new(handle.clone());
        db.insert_label("Evening").unwrap();
        let seen = std::thread::spawn(move || handle.count()).join().unwrap();
        assert!(seen >= 1);
        assert!(watch.take_new());
    }

    #[test]
    fn every_write_is_seen_exactly_when_it_records_a_sync_event() {
        // The counter sits where every local change is recorded, so
        // it moves iff the event log gained a local event. Walk one
        // write of every kind the shells make.
        let db = Database::open_in_memory().unwrap();
        let mut watch = LocalChangeWatch::new(db.local_changes());
        let mut check = |what: &str, op: &dyn Fn(&Database)| {
            let before = pending(&db);
            op(&db);
            let grew = pending(&db) > before;
            assert!(grew, "{what} should record a sync event");
            assert!(watch.take_new(), "{what} was not seen as a local change");
        };

        check("insert session", &|db| { db.insert_session(&session("2026-10-01T07:00:00")).unwrap(); });
        let id = crate::db::list_sessions_from_db(&db).unwrap()[0].0;
        check("update session", &|db| {
            let mut s = session("2026-10-01T08:00:00");
            s.notes = Some("edited".into());
            db.update_session(id, &s).unwrap();
        });
        check("bulk insert", &|db| { db.bulk_insert_sessions(&[session("2026-10-02T07:00:00")]).unwrap(); });
        check("csv import", &|db| {
            let csv = "start_iso,duration_secs,label,notes,mode\n2026-10-03T07:00:00,300,,,timer\n";
            db.import_sessions_csv(csv.as_bytes()).unwrap();
        });
        check("delete session", &|db| db.delete_session(id).unwrap());
        check("delete all sessions", &|db| { db.delete_all_sessions().unwrap(); });

        check("insert label", &|db| { db.insert_label("A").unwrap(); });
        let a = crate::db::find_label_by_name_from_db(&db, "A").unwrap().unwrap();
        check("rename label", &|db| db.update_label(a, "A2").unwrap());
        check("insert second label", &|db| { db.insert_label("B").unwrap(); });
        let b = crate::db::find_label_by_name_from_db(&db, "B").unwrap().unwrap();
        check("merge labels", &|db| { db.merge_labels(b, a).unwrap(); });
        check("delete label", &|db| db.delete_label(a).unwrap());

        check("insert preset", &|db| { db.insert_preset("P", SessionMode::Timer, false, "{}").unwrap(); });
        let preset = crate::db::list_presets_from_db(&db).unwrap()
            .into_iter().find(|p| p.name == "P").unwrap().uuid.to_string();
        check("rename preset", &|db| db.update_preset_name(&preset, "P2").unwrap());
        check("change preset", &|db| db.update_preset_config(&preset, r#"{"x":1}"#).unwrap());
        check("star preset", &|db| db.update_preset_starred(&preset, StarredState::Starred).unwrap());
        check("delete preset", &|db| db.delete_preset(&preset).unwrap());

        check("change setting", &|db| db.set_setting("daily_goal", "25").unwrap());

        check("import bell sound", &|db| {
            db.insert_bell_sound("Gong", "/x/gong.ogg", false, "audio/ogg", BellSoundCategory::General).unwrap();
        });

        check("add interval bell", &|db| {
            db.insert_interval_bell(
                IntervalBellKind::FixedFromStart, 10, 0, BUNDLED_BELL_UUID,
                BUNDLED_PATTERN_PULSE_UUID, SignalMode::Sound,
            ).unwrap();
        });
        let bell = db.list_interval_bells().unwrap().pop().unwrap();
        check("toggle interval bell", &|db| db.set_interval_bell_enabled(bell.uuid.as_str(), !bell.enabled).unwrap());
        check("delete interval bell", &|db| db.delete_interval_bell(bell.uuid.as_str()).unwrap());

        check("add vibration pattern", &|db| {
            db.insert_vibration_pattern("Wave", 1000, &[0.0, 1.0, 0.0], ChartKind::Line, false).unwrap();
        });
        let pattern = crate::db::list_vibration_patterns_from_db(&db).unwrap()
            .into_iter().find(|p| p.name == "Wave").unwrap().uuid.to_string();
        check("change vibration pattern", &|db| {
            db.update_vibration_pattern(&pattern, "Wave", 800, &[0.0, 0.5, 0.0], ChartKind::Bar).unwrap();
        });
        check("delete vibration pattern", &|db| db.delete_vibration_pattern(&pattern).unwrap());

        check("import guided file", &|db| {
            db.insert_guided_file_with_uuid(
                "8c0d5f2e-1111-4a2b-9c3d-000000000001", "Scan", "guided/x.ogg", 1200, false,
            ).unwrap();
        });
        let guided = "8c0d5f2e-1111-4a2b-9c3d-000000000001";
        check("rename guided file", &|db| db.rename_guided_file(guided, "Body Scan").unwrap());
        check("star guided file", &|db| db.set_guided_file_starred(guided, StarredState::Starred).unwrap());
        check("delete guided file", &|db| db.delete_guided_file(guided).unwrap());

        check("box breath phase", &|db| {
            db.set_box_breath_phase(
                BoxBreathPhaseId::In, true, SignalMode::Both, BUNDLED_BELL_UUID,
                BUNDLED_PATTERN_PULSE_UUID, crate::bell_volume::BellVolume::default(),
            ).unwrap();
        });
    }

    #[test]
    fn reads_are_not_changes() {
        let db = Database::open_in_memory().unwrap();
        db.insert_session(&session("2026-10-01T07:00:00")).unwrap();
        let mut watch = LocalChangeWatch::new(db.local_changes());
        crate::db::list_sessions_from_db(&db).unwrap();
        crate::db::list_labels_from_db(&db).unwrap();
        db.get_setting("daily_goal", "20").unwrap();
        db.list_interval_bells().unwrap();
        db.pending_events().unwrap();
        assert!(!watch.take_new());
    }

    #[test]
    fn an_unchanged_setting_is_not_a_change() {
        let db = Database::open_in_memory().unwrap();
        db.set_setting("timer_signal_mode", "sound").unwrap();
        let mut watch = LocalChangeWatch::new(db.local_changes());
        db.set_setting("timer_signal_mode", "sound").unwrap();
        assert!(!watch.take_new(), "a re-save must not start a sync");
    }

    #[test]
    fn the_running_session_snapshot_is_not_a_change() {
        // It is device-local and never synced.
        let db = Database::open_in_memory().unwrap();
        let mut watch = LocalChangeWatch::new(db.local_changes());
        let snapshot = SessionInProgress {
            start_iso: "2026-10-01T07:00:00".into(),
            accumulated_secs: 120,
            mode: SessionMode::Timer,
            mode_payload: "{}".into(),
            label_id: None,
            guided_file_uuid: None,
        };
        db.set_session_in_progress(&snapshot).unwrap();
        db.hold_ended_session(&snapshot).unwrap();
        db.clear_session_in_progress().unwrap();
        assert!(!watch.take_new());
    }

    #[test]
    fn changes_pulled_from_another_device_are_not_local_changes() {
        // Otherwise every pull would start another sync.
        let peer = Database::open_in_memory().unwrap();
        peer.insert_label("From peer").unwrap();
        peer.insert_session(&session("2026-10-01T07:00:00")).unwrap();
        peer.set_setting("daily_goal", "30").unwrap();
        let events: Vec<_> = peer.pending_events().unwrap().into_iter().map(|(_, e)| e).collect();

        let db = Database::open_in_memory().unwrap();
        let mut watch = LocalChangeWatch::new(db.local_changes());
        db.replay_events(&events).unwrap();
        assert_eq!(crate::db::list_sessions_from_db(&db).unwrap().len(), 1, "the pull applied");
        assert!(!watch.take_new());
    }

    #[test]
    fn a_rejected_write_is_not_a_change() {
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Same").unwrap();
        let mut watch = LocalChangeWatch::new(db.local_changes());
        assert!(db.insert_label("Same").is_err(), "duplicate names are refused");
        assert!(!watch.take_new());
    }

    #[test]
    fn two_databases_count_separately() {
        let one = Database::open_in_memory().unwrap();
        let two = Database::open_in_memory().unwrap();
        let mut watch_two = LocalChangeWatch::new(two.local_changes());
        one.insert_label("Only in one").unwrap();
        assert!(!watch_two.take_new());
    }
}
