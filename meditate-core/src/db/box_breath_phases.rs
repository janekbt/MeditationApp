//! `box_breath_phases` table — per-phase cue config (enabled +
//! signal_mode + sound_uuid + pattern_uuid). Always exactly four
//! rows (one per BoxBreathPhaseId), seeded on first open. No
//! insert/delete entry points — only the `box_breath_phase_update`
//! event mutates fields.

use rusqlite::{params, OptionalExtension};

use super::events::EventKind;
use super::{Database, DbError, Result};
use crate::bell_volume::BellVolume;
use crate::bells::SignalMode;
use crate::breath::{BoxBreathPhase, BoxBreathPhaseId};

const COLUMNS: &str = "phase, enabled, signal_mode, sound_uuid, pattern_uuid, volume_pct";

fn from_row(row: &rusqlite::Row) -> rusqlite::Result<BoxBreathPhase> {
    let phase: String = row.get(0)?;
    let signal_mode: String = row.get(2)?;
    Ok(BoxBreathPhase {
        phase: BoxBreathPhaseId::from_db_str(&phase).expect("box_breath_phases.phase violates CHECK constraint"),
        enabled: row.get::<_, i64>(1)? != 0,
        signal_mode: SignalMode::from_db_str(&signal_mode)
            .expect("box_breath_phases.signal_mode violates CHECK constraint"),
        sound_uuid: row.get(3)?,
        pattern_uuid: row.get(4)?,
        volume: BellVolume::from_percent(row.get::<_, i64>(5)? as f64),
    })
}

impl Database {
    /// Read every phase row in cycle order (in / hold-in / out /
    /// hold-out). Always returns exactly four rows after the seed.
    pub fn list_box_breath_phases(&self) -> Result<Vec<BoxBreathPhase>> {
        let mut phases: Vec<BoxBreathPhase> = self.query_rows(&format!("SELECT {COLUMNS} FROM box_breath_phases"), [], from_row)?;
        phases.sort_by_key(|p| BoxBreathPhaseId::all().iter().position(|&id| id == p.phase));
        Ok(phases)
    }

    /// Read a single phase row. Returns `None` if the phase hasn't
    /// been seeded yet (only happens during the seed cycle itself).
    pub fn get_box_breath_phase(&self, phase: BoxBreathPhaseId) -> Result<Option<BoxBreathPhase>> {
        self.query_one(&format!("SELECT {COLUMNS} FROM box_breath_phases WHERE phase = ?1"), params![phase.as_db_str()], from_row)
    }

    /// Change one phase as `edit` says, on the row as stored now, so a
    /// value saved since the caller last read it stays. Recorded in a
    /// `box_breath_phase_update` event carrying every column and stored
    /// as a peer stores it. A row `edit` leaves as it was writes
    /// nothing: a fresh event would undo an edit another device made in
    /// the meantime. A row not seeded yet is made first, as the seed
    /// makes it, so the change lands.
    pub fn edit_box_breath_phase(&self, phase: BoxBreathPhaseId, edit: impl Fn(&mut BoxBreathPhase)) -> Result<()> {
        // The edited row, or none when `edit` changes nothing.
        let edited = |stored: &BoxBreathPhase| {
            let mut row = stored.clone();
            edit(&mut row);
            (row != *stored).then_some(row)
        };
        // Nothing to change: a read, no lock (Setup echoes stored values).
        let seeded = match self.get_box_breath_phase(phase)? {
            Some(stored) if edited(&stored).is_none() => return Ok(()),
            stored => stored.is_some(),
        };
        let tx = self.tx()?;
        if !seeded {
            self.seed_box_breath_phases()?;
        }
        let Some(row) = edited(&self.get_box_breath_phase(phase)?.ok_or(rusqlite::Error::QueryReturnedNoRows)?) else {
            // Keeps a row just made, so the next unchanged write is a read.
            return tx.commit();
        };
        self.emit_and_rebuild_in(&tx, EventKind::BoxBreathPhaseUpdate, phase.as_db_str(), serde_json::json!({
            "phase": phase.as_db_str(),
            "enabled": row.enabled,
            "signal_mode": row.signal_mode.as_db_str(),
            "sound_uuid": row.sound_uuid,
            "pattern_uuid": row.pattern_uuid,
            "volume": row.volume.percent(),
        }))?;
        tx.commit()
    }

    /// Set every mutable field of one phase (see `edit_box_breath_phase`).
    pub fn set_box_breath_phase(
        &self,
        phase: BoxBreathPhaseId,
        enabled: bool,
        signal_mode: SignalMode,
        sound_uuid: &str,
        pattern_uuid: &str,
        volume: BellVolume,
    ) -> Result<()> {
        self.edit_box_breath_phase(phase, |p| {
            *p = BoxBreathPhase { phase, enabled, signal_mode, volume, sound_uuid: sound_uuid.into(), pattern_uuid: pattern_uuid.into() };
        })
    }

    /// Recompute the `box_breath_phases` row for `phase_id_str`
    /// from the events table. Always-existing fixed-key row: only
    /// `box_breath_phase_update` events drive its mutable fields,
    /// no insert / delete. Highest-(lamport, device_id) update wins.
    /// If no event exists, the row stays at its seeded defaults.
    pub(super) fn recompute_box_breath_phase(&self, phase_id_str: &str) -> Result<()> {
        let mutate: Option<String> = self.conn.query_row(
            "SELECT payload FROM events
             WHERE target_id = ?1 AND kind = 'box_breath_phase_update'
             ORDER BY lamport_ts DESC, device_id DESC
             LIMIT 1",
            params![phase_id_str],
            |row| row.get::<_, String>(0),
        ).optional()?;

        let Some(payload) = mutate else { return Ok(()); };

        let v: serde_json::Value = serde_json::from_str(&payload)
            .map_err(|e| DbError::Decode(
                format!("box_breath_phase event payload not valid JSON: {e}")))?;
        let enabled = v["enabled"].as_bool().unwrap_or(false);
        let signal_mode = v["signal_mode"].as_str().unwrap_or("sound");
        let sound_uuid = v["sound_uuid"].as_str()
            .unwrap_or("f0c2e8a1-3a72-4d4f-9c8b-1b0e5d8c0001");
        let pattern_uuid = v["pattern_uuid"].as_str()
            .unwrap_or("7e9c4d2f-5a8b-4f1d-9e3c-2d6f7a8b0001");
        // Events from builds before per-bell volumes carry none.
        let volume = v["volume"].as_f64().map_or_else(BellVolume::default, BellVolume::from_percent);

        // INSERT OR REPLACE so peers replaying an event for a row
        // they haven't seeded yet still materialise it.
        self.conn.execute(
            "INSERT INTO box_breath_phases
                (phase, enabled, signal_mode, sound_uuid, pattern_uuid, volume_pct)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(phase) DO UPDATE SET
                enabled      = excluded.enabled,
                signal_mode  = excluded.signal_mode,
                sound_uuid   = excluded.sound_uuid,
                pattern_uuid = excluded.pattern_uuid,
                volume_pct   = excluded.volume_pct",
            params![
                phase_id_str,
                i64::from(enabled),
                signal_mode,
                sound_uuid,
                pattern_uuid,
                volume.percent(),
            ],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{test_helpers::*, Event};

    #[test]
    fn editing_one_field_keeps_a_newer_volume() {
        // #325: the chooser opened on the old row; the volume was
        // dragged since; picking a sound keeps the dragged volume.
        let db = Database::open_in_memory().unwrap();
        db.seed_box_breath_phases().unwrap();
        let louder = crate::bell_volume::BellVolume::parse("80");
        crate::bell_volume::write(&db, crate::bell_volume::BellSlot::BoxBreathCue(BoxBreathPhaseId::In), louder).unwrap();
        db.edit_box_breath_phase(BoxBreathPhaseId::In, |p| p.sound_uuid = "gong".into()).unwrap();
        let p = db.get_box_breath_phase(BoxBreathPhaseId::In).unwrap().unwrap();
        assert_eq!(p.sound_uuid.as_str(), "gong");
        assert_eq!(p.volume, louder);
    }

    #[test]
    fn an_unchanged_phase_write_takes_no_lock() {
        // Setup echoes stored values back on a refresh; while a sync
        // writes, that must neither wait nor say "Save failed".
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("phase.db");
        let app = Database::open(&path).unwrap();
        app.seed_box_breath_phases().unwrap();
        let stored = app.get_box_breath_phase(BoxBreathPhaseId::In).unwrap().unwrap();
        let sync = Database::open(&path).unwrap();
        sql(&sync, "BEGIN IMMEDIATE; INSERT INTO sync_state (key, value) VALUES ('busy', '1');");
        app.set_busy_timeout(std::time::Duration::ZERO).unwrap();
        app.edit_box_breath_phase(BoxBreathPhaseId::In, |p| p.signal_mode = stored.signal_mode)
            .expect("nothing to write takes no lock");
        sql(&sync, "COMMIT;");
    }

    #[test]
    fn an_unchanged_write_to_a_phase_not_seeded_yet_seeds_it() {
        // Setup echoes the defaults back; the row is then made once, so
        // the next echo is a read again.
        let db = Database::open_in_memory().unwrap();
        sql(&db, "DELETE FROM box_breath_phases");
        db.edit_box_breath_phase(BoxBreathPhaseId::In, |_| {}).unwrap();
        assert!(db.get_box_breath_phase(BoxBreathPhaseId::In).unwrap().is_some());
        assert_eq!(sent(&db), 0, "a seeded row is no change to send");
    }

    #[test]
    fn editing_a_phase_not_seeded_yet_keeps_the_change() {
        // A first start whose seed failed: the pick still lands, on the
        // row the seed would have made.
        let db = Database::open_in_memory().unwrap();
        sql(&db, "DELETE FROM box_breath_phases");
        db.edit_box_breath_phase(BoxBreathPhaseId::In, |p| p.enabled = true).unwrap();
        assert!(db.get_box_breath_phase(BoxBreathPhaseId::In).unwrap().unwrap().enabled);
        assert_eq!(sent(&db), 1, "the change goes to other devices");
    }

    #[test]
    fn an_unchanged_phase_write_keeps_a_peers_edit() {
        let laptop = Database::open_in_memory().unwrap();
        let phone = Database::open_in_memory().unwrap();
        for db in [&laptop, &phone] { db.seed_box_breath_phases().unwrap(); }
        let old = laptop.get_box_breath_phase(BoxBreathPhaseId::In).unwrap().unwrap();
        phone.set_box_breath_phase(BoxBreathPhaseId::In, old.enabled, old.signal_mode,
            crate::seeds::BUNDLED_GONG_UUID, old.pattern_uuid.as_str(), old.volume).unwrap();
        let before = sent(&laptop);
        laptop.set_box_breath_phase(BoxBreathPhaseId::In, old.enabled, old.signal_mode,
            old.sound_uuid.as_str(), old.pattern_uuid.as_str(), old.volume).unwrap();
        assert_eq!(sent(&laptop), before, "an unchanged phase write sends nothing");
        exchange(&laptop, &phone);
        for db in [&laptop, &phone] {
            assert_eq!(db.get_box_breath_phase(BoxBreathPhaseId::In).unwrap().unwrap().sound_uuid.as_str(),
                crate::seeds::BUNDLED_GONG_UUID);
        }
    }

    #[test]
    fn box_breath_phase_id_round_trips_through_db_str() {
        for p in BoxBreathPhaseId::all() {
            assert_eq!(BoxBreathPhaseId::from_db_str(p.as_db_str()), Some(*p));
        }
        assert_eq!(BoxBreathPhaseId::from_db_str("typo"), None);
    }

    #[test]
    fn box_breath_phases_table_check_constraint_rejects_unknown_phase() {
        let db = Database::open_in_memory().unwrap();
        let bad = db.conn.execute(
            "INSERT INTO box_breath_phases (phase) VALUES ('paused')",
            [],
        );
        assert!(bad.is_err(),
            "phase = 'paused' must be rejected by the CHECK constraint");
    }

    #[test]
    fn list_box_breath_phases_is_empty_before_seeding() {
        let db = Database::open_in_memory().unwrap();
        assert!(db.list_box_breath_phases().unwrap().is_empty());
    }

    #[test]
    fn seed_box_breath_phases_creates_four_rows_with_defaults() {
        let db = Database::open_in_memory().unwrap();
        db.seed_box_breath_phases().unwrap();
        let rows = db.list_box_breath_phases().unwrap();
        assert_eq!(rows.len(), 4);
        let expected_order = [
            BoxBreathPhaseId::In,
            BoxBreathPhaseId::HoldIn,
            BoxBreathPhaseId::Out,
            BoxBreathPhaseId::HoldOut,
        ];
        for (got, want) in rows.iter().zip(expected_order.iter()) {
            assert_eq!(got.phase, *want);
            assert!(!got.enabled, "fresh seed: enabled defaults to false");
            assert_eq!(got.signal_mode, SignalMode::Sound);
            assert!(!got.sound_uuid.is_empty());
            assert!(!got.pattern_uuid.is_empty());
        }
    }

    #[test]
    fn seed_box_breath_phases_is_idempotent() {
        let db = Database::open_in_memory().unwrap();
        db.seed_box_breath_phases().unwrap();
        db.seed_box_breath_phases().unwrap();
        assert_eq!(db.list_box_breath_phases().unwrap().len(), 4);
    }

    #[test]
    fn set_box_breath_phase_round_trips_through_get() {
        let db = Database::open_in_memory().unwrap();
        db.seed_box_breath_phases().unwrap();
        db.set_box_breath_phase(
            BoxBreathPhaseId::In,
            true,
            SignalMode::Both,
            "sound-uuid-x",
            "pattern-uuid-y",
            crate::bell_volume::BellVolume::default(),
        ).unwrap();
        let row = db.get_box_breath_phase(BoxBreathPhaseId::In).unwrap().unwrap();
        assert!(row.enabled);
        assert_eq!(row.signal_mode, SignalMode::Both);
        assert_eq!(row.sound_uuid, "sound-uuid-x");
        assert_eq!(row.pattern_uuid, "pattern-uuid-y");
        let other = db.get_box_breath_phase(BoxBreathPhaseId::HoldIn).unwrap().unwrap();
        assert!(!other.enabled);
    }

    #[test]
    fn replay_box_breath_phase_event_round_trips_to_a_fresh_peer() {
        let dev_a = Database::open_in_memory().unwrap();
        dev_a.seed_box_breath_phases().unwrap();
        dev_a.set_box_breath_phase(
            BoxBreathPhaseId::Out,
            true, SignalMode::Both, "s-uuid", "p-uuid",
            crate::bell_volume::BellVolume::default(),
        ).unwrap();
        let events: Vec<Event> = dev_a.pending_events().unwrap()
            .into_iter().map(|(_, e)| e).collect();

        let dev_b = Database::open_in_memory().unwrap();
        dev_b.seed_box_breath_phases().unwrap();
        dev_b.replay_events(&events).unwrap();

        let row = dev_b.get_box_breath_phase(BoxBreathPhaseId::Out)
            .unwrap().unwrap();
        assert!(row.enabled);
        assert_eq!(row.signal_mode, SignalMode::Both);
        assert_eq!(row.sound_uuid, "s-uuid");
        assert_eq!(row.pattern_uuid, "p-uuid");
    }

    #[test]
    fn apply_event_box_breath_phase_creates_row_on_unseeded_peer() {
        // Edge case: a peer receives the update event before seeding.
        // The recompute path INSERT-OR-UPDATEs so the row materialises.
        let peer = Database::open_in_memory().unwrap();
        peer.apply_event(&synth_event(
            "box_breath_phase_update", "in", 5, DEVICE_A,
            serde_json::json!({
                "phase": "in",
                "enabled": true,
                "signal_mode": "vibration",
                "sound_uuid": "x",
                "pattern_uuid": "y",
            }),
        )).unwrap();
        let row = peer.get_box_breath_phase(BoxBreathPhaseId::In)
            .unwrap().unwrap();
        assert!(row.enabled);
        assert_eq!(row.signal_mode, SignalMode::Vibration);
    }

    #[test]
    fn set_box_breath_phase_emits_a_box_breath_phase_update_event() {
        let db = Database::open_in_memory().unwrap();
        db.seed_box_breath_phases().unwrap();
        db.set_box_breath_phase(
            BoxBreathPhaseId::Out,
            true, SignalMode::Vibration, "s-uuid", "p-uuid",
            crate::bell_volume::BellVolume::default(),
        ).unwrap();
        let updates: Vec<_> = db.pending_events().unwrap()
            .into_iter()
            .filter(|(_, e)| e.kind == "box_breath_phase_update")
            .collect();
        assert_eq!(updates.len(), 1);
        let payload: serde_json::Value =
            serde_json::from_str(&updates[0].1.payload).unwrap();
        assert_eq!(payload["phase"], "out");
        assert_eq!(payload["enabled"], true);
        assert_eq!(payload["signal_mode"], "vibration");
        assert_eq!(payload["sound_uuid"], "s-uuid");
        assert_eq!(payload["pattern_uuid"], "p-uuid");
    }

    // ── Volume ─────────────────────────────────────────────────────

    use crate::bell_volume::BellVolume;

    fn phase_in(db: &Database) -> BoxBreathPhase {
        db.get_box_breath_phase(BoxBreathPhaseId::In).unwrap().unwrap()
    }

    #[test]
    fn every_phase_starts_at_the_middle_volume() {
        let db = Database::open_in_memory().unwrap();
        db.seed_box_breath_phases().unwrap();
        for p in db.list_box_breath_phases().unwrap() {
            assert_eq!(p.volume, BellVolume::default(), "{:?}", p.phase);
        }
    }

    #[test]
    fn set_stores_the_volume_and_syncs_it() {
        let db = Database::open_in_memory().unwrap();
        db.seed_box_breath_phases().unwrap();
        let volume = BellVolume::from_percent(25.0);
        db.set_box_breath_phase(
            BoxBreathPhaseId::In, true, SignalMode::Sound,
            "f0c2e8a1-3a72-4d4f-9c8b-1b0e5d8c0001", "7e9c4d2f-5a8b-4f1d-9e3c-2d6f7a8b0001",
            volume,
        )
        .unwrap();
        assert_eq!(phase_in(&db).volume, volume);
        let listed = db.list_box_breath_phases().unwrap();
        assert_eq!(listed.iter().find(|p| p.phase == BoxBreathPhaseId::In).unwrap().volume, volume);
        let update = db.pending_events().unwrap().into_iter().map(|(_, e)| e)
            .find(|e| e.kind == "box_breath_phase_update").unwrap();
        let payload: serde_json::Value = serde_json::from_str(&update.payload).unwrap();
        assert_eq!(payload["volume"], 25);
    }

    fn synced_phase_in(volume: Option<serde_json::Value>) -> Event {
        let mut payload = serde_json::json!({
            "phase": "in",
            "enabled": true,
            "signal_mode": "sound",
            "sound_uuid": "f0c2e8a1-3a72-4d4f-9c8b-1b0e5d8c0001",
            "pattern_uuid": "7e9c4d2f-5a8b-4f1d-9e3c-2d6f7a8b0001",
        });
        if let Some(v) = volume {
            payload["volume"] = v;
        }
        Event {
            event_uuid: "ev-phase-in".into(),
            lamport_ts: 5,
            device_id: "peer".into(),
            kind: "box_breath_phase_update".into(),
            target_id: "in".into(),
            payload: payload.to_string(),
        }
    }

    #[test]
    fn a_synced_volume_lands_in_the_row() {
        let db = Database::open_in_memory().unwrap();
        db.seed_box_breath_phases().unwrap();
        db.apply_event(&synced_phase_in(Some(65.into()))).unwrap();
        assert_eq!(phase_in(&db).volume, BellVolume::from_percent(65.0));
    }

    #[test]
    fn a_synced_phase_from_an_older_version_gets_the_middle_volume() {
        let db = Database::open_in_memory().unwrap();
        db.seed_box_breath_phases().unwrap();
        db.apply_event(&synced_phase_in(None)).unwrap();
        assert_eq!(phase_in(&db).volume, BellVolume::default());
    }
}
