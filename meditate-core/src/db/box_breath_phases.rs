//! `box_breath_phases` table — per-phase cue config (enabled +
//! signal_mode + sound_uuid + pattern_uuid). Always exactly four
//! rows (one per BoxBreathPhaseId), seeded on first open. No
//! insert/delete entry points — only the `box_breath_phase_update`
//! event mutates fields.

use rusqlite::{params, OptionalExtension};

use super::{Database, DbError, Result, SignalMode};

/// Box Breath. Identified by `phase` (PK), with the same
/// (enabled / signal_mode / sound_uuid / pattern_uuid) shape as
/// per-bell config. The four rows are fixed (no insert / delete) so
/// the table acts as a tiny key/value store with strong typing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoxBreathPhase {
    pub phase: BoxBreathPhaseId,
    pub enabled: bool,
    pub signal_mode: SignalMode,
    pub sound_uuid: String,
    pub pattern_uuid: String,
}

/// Mirrors `crate::timer::breathing::Phase` shapewise (In / HoldIn /
/// Out / HoldOut), but lives in core so the DB layer can use it
/// without depending on the timer module. The shell maps between the
/// two as needed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BoxBreathPhaseId {
    In,
    HoldIn,
    Out,
    HoldOut,
}

impl BoxBreathPhaseId {
    pub fn as_db_str(self) -> &'static str {
        match self {
            BoxBreathPhaseId::In      => "in",
            BoxBreathPhaseId::HoldIn  => "holdin",
            BoxBreathPhaseId::Out     => "out",
            BoxBreathPhaseId::HoldOut => "holdout",
        }
    }
    pub fn from_db_str(s: &str) -> Option<Self> {
        match s {
            "in"      => Some(BoxBreathPhaseId::In),
            "holdin"  => Some(BoxBreathPhaseId::HoldIn),
            "out"     => Some(BoxBreathPhaseId::Out),
            "holdout" => Some(BoxBreathPhaseId::HoldOut),
            _         => None,
        }
    }
    /// Iteration order for the seed + the UI list — matches the
    /// natural Box Breath cycle (in → hold → out → hold).
    pub fn all() -> &'static [BoxBreathPhaseId] {
        &[
            BoxBreathPhaseId::In,
            BoxBreathPhaseId::HoldIn,
            BoxBreathPhaseId::Out,
            BoxBreathPhaseId::HoldOut,
        ]
    }
}

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
                enabled as i64,
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
        self.emit_event("box_breath_phase_update", phase.as_db_str(), payload)?;
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
            .map_err(|e| DbError::Csv(
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
                enabled as i64,
                signal_mode,
                sound_uuid,
                pattern_uuid,
            ],
        )?;
        Ok(())
    }
}
