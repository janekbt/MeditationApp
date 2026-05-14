//! `box_breath_phases` table — per-phase cue config (enabled +
//! signal_mode + sound_uuid + pattern_uuid). Always exactly four
//! rows (one per BoxBreathPhaseId), seeded on first open. No
//! insert/delete entry points — only the `box_breath_phase_update`
//! event mutates fields.

use rusqlite::{params, OptionalExtension};

use super::events::EventKind;
use super::{Database, DbError, Result};
use crate::bells::SignalMode;
use crate::breath::{BoxBreathPhase, BoxBreathPhaseId};

impl Database {
    /// Read every phase row in cycle order (in / hold-in / out /
    /// hold-out). Always returns exactly four rows after the seed.
    pub fn list_box_breath_phases(&self) -> Result<Vec<BoxBreathPhase>> {
        let mut stmt = self.conn.prepare(
            "SELECT phase, enabled, signal_mode, sound_uuid, pattern_uuid
             FROM box_breath_phases",
        )?;
        let mut by_id: std::collections::HashMap<BoxBreathPhaseId, BoxBreathPhase> =
            std::collections::HashMap::new();
        let rows = stmt.query_map([], |row| {
            let phase_str: String = row.get(0)?;
            let signal_mode_str: String = row.get(2)?;
            Ok(BoxBreathPhase {
                phase: BoxBreathPhaseId::from_db_str(&phase_str)
                    .expect("box_breath_phases.phase violates CHECK constraint"),
                enabled: row.get::<_, i64>(1)? != 0,
                signal_mode: SignalMode::from_db_str(&signal_mode_str)
                    .expect("box_breath_phases.signal_mode violates CHECK constraint"),
                sound_uuid: row.get(3)?,
                pattern_uuid: row.get(4)?,
            })
        })?;
        for r in rows {
            let p = r?;
            by_id.insert(p.phase, p);
        }
        // Return in cycle order regardless of how SQLite returned them.
        Ok(BoxBreathPhaseId::all()
            .iter()
            .filter_map(|id| by_id.remove(id))
            .collect())
    }

    /// Read a single phase row. Returns `None` if the phase hasn't
    /// been seeded yet (only happens during the seed cycle itself).
    pub fn get_box_breath_phase(
        &self,
        phase: BoxBreathPhaseId,
    ) -> Result<Option<BoxBreathPhase>> {
        let row = self.conn.query_row(
            "SELECT phase, enabled, signal_mode, sound_uuid, pattern_uuid
             FROM box_breath_phases WHERE phase = ?1",
            params![phase.as_db_str()],
            |row| {
                let phase_str: String = row.get(0)?;
                let signal_mode_str: String = row.get(2)?;
                Ok(BoxBreathPhase {
                    phase: BoxBreathPhaseId::from_db_str(&phase_str)
                        .expect("box_breath_phases.phase violates CHECK constraint"),
                    enabled: row.get::<_, i64>(1)? != 0,
                    signal_mode: SignalMode::from_db_str(&signal_mode_str)
                        .expect("box_breath_phases.signal_mode violates CHECK constraint"),
                    sound_uuid: row.get(3)?,
                    pattern_uuid: row.get(4)?,
                })
            },
        ).optional()?;
        Ok(row)
    }

    /// Update every mutable field on a single phase row. Emits a
    /// `box_breath_phase_update` event carrying every column so peers
    /// can replay in any order. Idempotent: writing the same values
    /// twice still emits two events but converges on the same state.
    pub fn set_box_breath_phase(
        &self,
        phase: BoxBreathPhaseId,
        enabled: bool,
        signal_mode: SignalMode,
        sound_uuid: &str,
        pattern_uuid: &str,
    ) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        self.conn.execute(
            "UPDATE box_breath_phases
                SET enabled = ?1, signal_mode = ?2,
                    sound_uuid = ?3, pattern_uuid = ?4
              WHERE phase = ?5",
            params![
                i64::from(enabled),
                signal_mode.as_db_str(),
                sound_uuid,
                pattern_uuid,
                phase.as_db_str(),
            ],
        )?;
        let payload = serde_json::json!({
            "phase": phase.as_db_str(),
            "enabled": enabled,
            "signal_mode": signal_mode.as_db_str(),
            "sound_uuid": sound_uuid,
            "pattern_uuid": pattern_uuid,
        }).to_string();
        self.emit_event(EventKind::BoxBreathPhaseUpdate, phase.as_db_str(), payload)?;
        tx.commit()?;
        Ok(())
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

        // INSERT OR REPLACE so peers replaying an event for a row
        // they haven't seeded yet still materialise it.
        self.conn.execute(
            "INSERT INTO box_breath_phases
                (phase, enabled, signal_mode, sound_uuid, pattern_uuid)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(phase) DO UPDATE SET
                enabled      = excluded.enabled,
                signal_mode  = excluded.signal_mode,
                sound_uuid   = excluded.sound_uuid,
                pattern_uuid = excluded.pattern_uuid",
            params![
                phase_id_str,
                i64::from(enabled),
                signal_mode,
                sound_uuid,
                pattern_uuid,
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
}
