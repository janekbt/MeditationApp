//! `interval_bells` table — user-configured bells that fire during
//! Timer-mode sessions. Three kinds: periodic-with-jitter,
//! fixed-from-start, fixed-from-end. All CRUD ops emit sync events
//! so the library round-trips across devices.

use crate::bell_volume::BellVolume;
use rusqlite::{params, OptionalExtension};

use super::events::EventKind;
use super::{Database, Result};
use crate::bells::{legacy_jitter_pct, legacy_minutes, resolve_bell_times, BellTime, SignalMode, TimeUnit};
use crate::seeds::{BUNDLED_BOWL_UUID, BUNDLED_PATTERN_PULSE_UUID};

/// One configured bell entry in the user's interval-bell library.
/// All enabled rows fire as bells during a Timer-mode session;
/// Box Breathing is exempt. Three kinds:
///
/// - `Interval` — every `time`, growing by `step` per gap, ± `jitter`
///   rerolled on each ring. A 9 min ±3 min bell fires somewhere in
///   6–12 min, never settling into a predictable beat.
/// - `FixedFromStart` — at exactly `time` elapsed (e.g., switch
///   from metta to breath at 10:00). `jitter` and `step` are ignored.
/// - `FixedFromEnd` — at `time` before session end (only meaningful
///   in countdown mode; stopwatch sessions skip these). `jitter` and
///   `step` are ignored.
///
/// Rows and events also carry `minutes` and `jitter_pct`, which
/// versions before seconds read; see `resolve_bell_times`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntervalBell {
    pub id: i64,
    pub uuid: super::IntervalBellUuid,
    pub kind: IntervalBellKind,
    pub time: BellTime,
    pub jitter: BellTime,
    pub step: BellTime,
    pub sound_uuid: crate::db::BellSoundUuid,
    pub vibration_pattern_uuid: crate::db::VibrationPatternUuid,
    pub signal_mode: SignalMode,
    pub enabled: bool,
    pub created_iso: String,
    pub volume: crate::bell_volume::BellVolume,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntervalBellKind {
    Interval,
    FixedFromStart,
    FixedFromEnd,
}

impl IntervalBellKind {
    pub fn as_db_str(self) -> &'static str {
        match self {
            IntervalBellKind::Interval => "interval",
            IntervalBellKind::FixedFromStart => "fixed_from_start",
            IntervalBellKind::FixedFromEnd => "fixed_from_end",
        }
    }
    pub fn from_db_str(s: &str) -> Option<Self> {
        match s {
            "interval" => Some(IntervalBellKind::Interval),
            "fixed_from_start" => Some(IntervalBellKind::FixedFromStart),
            "fixed_from_end" => Some(IntervalBellKind::FixedFromEnd),
            _ => None,
        }
    }
}

impl Database {
    /// Insert a new bell row. Mints a UUID + created_iso, records an
    /// `interval_bell_insert` event, returns the AUTOINCREMENT rowid.
    /// `enabled` defaults to true on a fresh insert — the user opts a
    /// bell out by toggling it off later, not at creation time.
    pub fn insert_interval_bell(
        &self,
        kind: IntervalBellKind,
        time: BellTime,
        jitter: BellTime,
        step: BellTime,
        sound_uuid: &str,
        vibration_pattern_uuid: &str,
        signal_mode: SignalMode,
    ) -> Result<i64> {
        let tx = self.conn.unchecked_transaction()?;
        let rowid = self.insert_bell_in(&tx, &IntervalBell {
            id: 0,
            uuid: uuid::Uuid::new_v4().to_string().into(),
            kind,
            time,
            jitter,
            step,
            sound_uuid: sound_uuid.into(),
            vibration_pattern_uuid: vibration_pattern_uuid.into(),
            signal_mode,
            enabled: true,
            created_iso: chrono::Utc::now().to_rfc3339(),
            volume: BellVolume::default(),
        })?;
        tx.commit()?;
        Ok(rowid)
    }

    /// The edit `edited` of bell `old` as new bell `uuid` beside it
    /// (same creation time), and `old` switched off, in one
    /// transaction. Returns the new bell's uuid.
    pub fn copy_interval_bell(&self, old: &IntervalBell, edited: &IntervalBell, uuid: &str) -> Result<super::IntervalBellUuid> {
        let tx = self.conn.unchecked_transaction()?;
        let uuid: super::IntervalBellUuid = uuid.into();
        self.insert_bell_in(&tx, &IntervalBell { uuid: uuid.clone(), created_iso: old.created_iso.clone(), ..edited.clone() })?;
        self.update_bell_in(&tx, &IntervalBell { enabled: false, ..old.clone() })?;
        tx.commit()?;
        Ok(uuid)
    }

    /// Insert `bell` as it is (its `id` is ignored) and record it.
    fn insert_bell_in(&self, tx: &rusqlite::Transaction<'_>, bell: &IntervalBell) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO interval_bells
                (uuid, kind, minutes, jitter_pct, sound_uuid,
                 vibration_pattern_uuid, signal_mode, enabled, created_iso, volume_pct,
                 secs, secs_unit, jitter_secs, jitter_unit, step_secs, step_unit)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
            params![
                bell.uuid,
                bell.kind.as_db_str(),
                legacy_minutes(bell.time),
                legacy_jitter_pct(bell.time, bell.jitter),
                bell.sound_uuid,
                bell.vibration_pattern_uuid,
                bell.signal_mode.as_db_str(),
                i64::from(bell.enabled),
                bell.created_iso,
                bell.volume.percent(),
                bell.time.secs,
                bell.time.unit.as_db_str(),
                bell.jitter.secs,
                bell.jitter.unit.as_db_str(),
                bell.step.secs,
                bell.step.unit.as_db_str(),
            ],
        )?;
        let rowid = self.conn.last_insert_rowid();
        let mut payload = serde_json::json!({
            "uuid": bell.uuid,
            "kind": bell.kind.as_db_str(),
            "sound_uuid": bell.sound_uuid,
            "vibration_pattern_uuid": bell.vibration_pattern_uuid,
            "signal_mode": bell.signal_mode.as_db_str(),
            "enabled": bell.enabled,
            "created_iso": bell.created_iso,
            "volume": bell.volume.percent(),
        });
        add_time_fields(&mut payload, bell.time, bell.jitter, bell.step);
        self.emit_event(tx, EventKind::IntervalBellInsert, bell.uuid.as_str(), payload.to_string())?;
        Ok(rowid)
    }

    /// Overwrite every mutable field of the bell. UUID + created_iso
    /// are immutable — the `bell.uuid` field is the row identity, the
    /// stored `created_iso` is preserved. Unknown uuids are a silent
    /// no-op AND emit no event — peers receiving an update for a row
    /// they've already tombstoned should not be reflected back as
    /// "this bell is alive again". Mirrors `update_label`'s shape.
    pub fn update_interval_bell(&self, bell: &IntervalBell) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        self.update_bell_in(&tx, bell)?;
        tx.commit()?;
        Ok(())
    }

    fn update_bell_in(&self, tx: &rusqlite::Transaction<'_>, bell: &IntervalBell) -> Result<()> {
        let created_iso: Option<String> = self.conn.query_row(
            "SELECT created_iso FROM interval_bells WHERE uuid = ?1",
            params![bell.uuid],
            |row| row.get::<_, String>(0),
        ).optional()?;
        let Some(created_iso) = created_iso else {
            return Ok(());
        };
        self.conn.execute(
            "UPDATE interval_bells
                SET kind = ?1, minutes = ?2, jitter_pct = ?3, sound_uuid = ?4,
                    vibration_pattern_uuid = ?5, signal_mode = ?6, enabled = ?7,
                    volume_pct = ?8, secs = ?10, secs_unit = ?11, jitter_secs = ?12,
                    jitter_unit = ?13, step_secs = ?14, step_unit = ?15
              WHERE uuid = ?9",
            params![
                bell.kind.as_db_str(),
                legacy_minutes(bell.time),
                legacy_jitter_pct(bell.time, bell.jitter),
                bell.sound_uuid,
                bell.vibration_pattern_uuid,
                bell.signal_mode.as_db_str(),
                i64::from(bell.enabled),
                bell.volume.percent(),
                bell.uuid,
                bell.time.secs,
                bell.time.unit.as_db_str(),
                bell.jitter.secs,
                bell.jitter.unit.as_db_str(),
                bell.step.secs,
                bell.step.unit.as_db_str(),
            ],
        )?;
        let mut payload = serde_json::json!({
            "uuid": bell.uuid,
            "kind": bell.kind.as_db_str(),
            "sound_uuid": bell.sound_uuid,
            "vibration_pattern_uuid": bell.vibration_pattern_uuid,
            "signal_mode": bell.signal_mode.as_db_str(),
            "enabled": bell.enabled,
            "created_iso": created_iso,
            "volume": bell.volume.percent(),
        });
        add_time_fields(&mut payload, bell.time, bell.jitter, bell.step);
        self.emit_event(tx, EventKind::IntervalBellUpdate, bell.uuid.as_str(), payload.to_string())?;
        Ok(())
    }

    /// Convenience for the common path — toggling enabled without the
    /// UI having to read the other fields back. Emits the same
    /// `interval_bell_update` event as a full-fields update so the
    /// sync replay code only has to handle one update kind.
    pub fn set_interval_bell_enabled(&self, uuid: &str, enabled: bool) -> Result<()> {
        let Some(bell) = self.list_interval_bells()?
            .into_iter()
            .find(|b| b.uuid == uuid)
        else {
            return Ok(());
        };
        self.update_interval_bell(&IntervalBell { enabled, ..bell })
    }

    /// Remove the bell row with `uuid` and emit a tombstone event.
    /// Unknown uuids are silent no-ops AND emit no event (peers
    /// shouldn't get a delete for a row they never knew existed).
    pub fn delete_interval_bell(&self, uuid: &str) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        if self.existing_rowid_by_uuid("interval_bells", uuid)?.is_none() {
            return Ok(());
        }
        self.conn.execute(
            "DELETE FROM interval_bells WHERE uuid = ?1",
            params![uuid],
        )?;
        let payload = serde_json::json!({ "uuid": uuid }).to_string();
        self.emit_event(&tx, EventKind::IntervalBellDelete, uuid, payload)?;
        tx.commit()?;
        Ok(())
    }

    /// Look up an interval bell by its primary-key rowid. Used by
    /// the shell's "drill into the row I just inserted" flow —
    /// `insert_interval_bell` returns rowid and this resolves it to
    /// the full row (including the generated uuid) without
    /// re-listing the whole library.
    pub fn find_interval_bell_by_id(&self, rowid: i64) -> Result<Option<IntervalBell>> {
        // Cheap to delegate to list_interval_bells then filter — the
        // library is small (typically <20 rows). Avoids duplicating
        // the row→IntervalBell decoder.
        Ok(self.list_interval_bells()?.into_iter().find(|b| b.id == rowid))
    }

    /// Every bell row in creation order: the first one added is at the
    /// top, on every device (`created_iso` syncs; rowids follow arrival
    /// order). The B.3.3 list page renders this directly.
    pub fn list_interval_bells(&self) -> Result<Vec<IntervalBell>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, uuid, kind, minutes, jitter_pct, sound_uuid,
                    vibration_pattern_uuid, signal_mode, enabled, created_iso,
                    volume_pct, secs, secs_unit, jitter_secs, jitter_unit,
                    step_secs, step_unit
             FROM interval_bells
             ORDER BY created_iso ASC, uuid ASC",
        )?;
        let rows = stmt
            .query_map([], |row| {
                let kind_str: String = row.get(2)?;
                let signal_mode_str: String = row.get(7)?;
                let time = |secs: usize| -> rusqlite::Result<Option<BellTime>> {
                    let n: Option<i64> = row.get(secs)?;
                    let unit: Option<String> = row.get(secs + 1)?;
                    Ok(stored_time(n, unit.as_deref()))
                };
                let stored = match (time(11)?, time(13)?, time(15)?) {
                    (Some(t), Some(j), Some(s)) => Some((t, j, s)),
                    _ => None,
                };
                let (time, jitter, step) = resolve_bell_times(
                    row.get::<_, i64>(3)? as u32,
                    row.get::<_, i64>(4)? as u32,
                    stored,
                );
                Ok(IntervalBell {
                    id: row.get(0)?,
                    uuid: row.get(1)?,
                    kind: IntervalBellKind::from_db_str(&kind_str)
                        .expect("interval_bells.kind violates CHECK constraint"),
                    time,
                    jitter,
                    step,
                    sound_uuid: row.get(5)?,
                    vibration_pattern_uuid: row.get(6)?,
                    signal_mode: SignalMode::from_db_str(&signal_mode_str)
                        .expect("interval_bells.signal_mode violates CHECK constraint"),
                    enabled: row.get::<_, i64>(8)? != 0,
                    created_iso: row.get(9)?,
                    volume: BellVolume::from_percent(row.get::<_, i64>(10)? as f64),
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Rebuild every bell row from its winning event, on each open. A
    /// version before seconds keeps only minutes and percent in the row
    /// when it applies a newer device's event (before this device
    /// updated, or after going back to it); the event keeps the rest.
    /// Only existing rows that have events: a deleted bell is already
    /// gone, a row without events has nothing to rebuild from (and
    /// would be deleted), and the library is a handful of rows.
    pub(super) fn recompute_interval_bells_from_events(&self) -> Result<()> {
        let uuids: Vec<String> = self
            .conn
            .prepare(
                "SELECT uuid FROM interval_bells b
                 WHERE EXISTS (SELECT 1 FROM events WHERE target_id = b.uuid)",
            )?
            .query_map([], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        let tx = self.conn.unchecked_transaction()?;
        for uuid in uuids {
            self.recompute_interval_bell(&uuid)?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Recompute the `interval_bells` row for `bell_uuid` from the
    /// events table. Same precedence rules as labels: tombstone wins
    /// on tie/precedence, else the highest-(lamport, device_id) mutate
    /// event drives the row's values. Update events carry every
    /// mutable field plus created_iso so they're self-sufficient if
    /// the corresponding insert event hasn't arrived yet.
    pub(super) fn recompute_interval_bell(&self, bell_uuid: &str) -> Result<()> {
        let Some(v) = self.winning_mutate(
            bell_uuid,
            [EventKind::IntervalBellInsert, EventKind::IntervalBellUpdate],
            EventKind::IntervalBellDelete,
        )? else {
            self.conn.execute(
                "DELETE FROM interval_bells WHERE uuid = ?1",
                params![bell_uuid],
            )?;
            return Ok(());
        };
        {
            let kind = v["kind"].as_str().unwrap_or("interval");
            let minutes = v["minutes"].as_u64().unwrap_or(0) as u32;
            let jitter_pct = v["jitter_pct"].as_u64().unwrap_or(0) as u32;
            // Events from builds before seconds carry only minutes and percent.
            let [time, jitter, step] =
                TIME_KEYS.map(|(secs, unit)| stored_time(v[secs].as_i64(), v[unit].as_str()));
            let stored = match (time, jitter, step) {
                (Some(t), Some(j), Some(s)) => Some((t, j, s)),
                _ => None,
            };
            let (time, jitter, step) = resolve_bell_times(minutes, jitter_pct, stored);
            let sound_uuid = v["sound_uuid"].as_str().unwrap_or(BUNDLED_BOWL_UUID);
            let vibration_pattern_uuid = v["vibration_pattern_uuid"]
                .as_str()
                .unwrap_or(BUNDLED_PATTERN_PULSE_UUID);
            let signal_mode = v["signal_mode"].as_str().unwrap_or("sound");
            let enabled = v["enabled"].as_bool().unwrap_or(true);
            let created_iso = v["created_iso"].as_str().unwrap_or_default();
            // Events from builds before per-bell volumes carry none.
            let volume = v["volume"].as_f64().map_or_else(BellVolume::default, BellVolume::from_percent);
            self.conn.execute(
                "INSERT INTO interval_bells
                    (uuid, kind, minutes, jitter_pct, sound_uuid,
                     vibration_pattern_uuid, signal_mode, enabled, created_iso,
                     volume_pct, secs, secs_unit, jitter_secs, jitter_unit,
                     step_secs, step_unit)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
                 ON CONFLICT(uuid) DO UPDATE SET
                    kind                   = excluded.kind,
                    minutes                = excluded.minutes,
                    jitter_pct             = excluded.jitter_pct,
                    sound_uuid             = excluded.sound_uuid,
                    vibration_pattern_uuid = excluded.vibration_pattern_uuid,
                    signal_mode            = excluded.signal_mode,
                    enabled                = excluded.enabled,
                    created_iso            = excluded.created_iso,
                    volume_pct             = excluded.volume_pct,
                    secs                   = excluded.secs,
                    secs_unit              = excluded.secs_unit,
                    jitter_secs            = excluded.jitter_secs,
                    jitter_unit            = excluded.jitter_unit,
                    step_secs              = excluded.step_secs,
                    step_unit              = excluded.step_unit",
                params![
                    bell_uuid,
                    kind,
                    minutes,
                    jitter_pct,
                    sound_uuid,
                    vibration_pattern_uuid,
                    signal_mode,
                    i64::from(enabled),
                    created_iso,
                    volume.percent(),
                    time.secs,
                    time.unit.as_db_str(),
                    jitter.secs,
                    jitter.unit.as_db_str(),
                    step.secs,
                    step.unit.as_db_str(),
                ],
            )?;
        }
        Ok(())
    }
}

/// The event keys of a bell's time, jitter and step, each with its unit.
const TIME_KEYS: [(&str, &str); 3] =
    [("secs", "secs_unit"), ("jitter_secs", "jitter_unit"), ("step_secs", "step_unit")];

/// A stored seconds field and its unit, `None` when either is missing
/// or not understood.
fn stored_time(secs: Option<i64>, unit: Option<&str>) -> Option<BellTime> {
    let secs = u32::try_from(secs?).ok()?;
    Some(BellTime { secs, unit: TimeUnit::from_db_str(unit?)? })
}

/// The seconds fields of a bell event, next to the `minutes` and
/// `jitter_pct` that versions before seconds read.
fn add_time_fields(payload: &mut serde_json::Value, time: BellTime, jitter: BellTime, step: BellTime) {
    for ((secs, unit), t) in TIME_KEYS.into_iter().zip([time, jitter, step]) {
        payload[secs] = t.secs.into();
        payload[unit] = t.unit.as_db_str().into();
    }
    payload["minutes"] = legacy_minutes(time).into();
    payload["jitter_pct"] = legacy_jitter_pct(time, jitter).into();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Event;
    use crate::seeds::{BUNDLED_BELL_UUID, BUNDLED_GONG_UUID};

    fn synth_interval_bell_insert(
        bell_uuid: &str,
        lamport_ts: i64,
        device: &str,
        kind: IntervalBellKind,
        minutes: u32,
        jitter_pct: u32,
        sound_uuid: &str,
    ) -> Event {
        Event {
            event_uuid: format!("ev-insert-{bell_uuid}-{lamport_ts}-{device}"),
            lamport_ts,
            device_id: device.to_string(),
            kind: "interval_bell_insert".to_string(),
            target_id: bell_uuid.to_string(),
            payload: serde_json::json!({
                "uuid": bell_uuid,
                "kind": kind.as_db_str(),
                "minutes": minutes,
                "jitter_pct": jitter_pct,
                "sound_uuid": sound_uuid,
                "enabled": true,
                "created_iso": "2026-05-03T12:00:00Z",
            }).to_string(),
        }
    }

    fn synth_interval_bell_update(
        bell_uuid: &str,
        lamport_ts: i64,
        device: &str,
        minutes: u32,
        enabled: bool,
    ) -> Event {
        Event {
            event_uuid: format!("ev-update-{bell_uuid}-{lamport_ts}-{device}"),
            lamport_ts,
            device_id: device.to_string(),
            kind: "interval_bell_update".to_string(),
            target_id: bell_uuid.to_string(),
            payload: serde_json::json!({
                "uuid": bell_uuid,
                "kind": "interval",
                "minutes": minutes,
                "jitter_pct": 0,
                "sound_uuid": BUNDLED_BOWL_UUID,
                "enabled": enabled,
                "created_iso": "2026-05-03T12:00:00Z",
            }).to_string(),
        }
    }

    fn synth_interval_bell_delete(
        bell_uuid: &str,
        lamport_ts: i64,
        device: &str,
    ) -> Event {
        Event {
            event_uuid: format!("ev-delete-{bell_uuid}-{lamport_ts}-{device}"),
            lamport_ts,
            device_id: device.to_string(),
            kind: "interval_bell_delete".to_string(),
            target_id: bell_uuid.to_string(),
            payload: serde_json::json!({ "uuid": bell_uuid }).to_string(),
        }
    }

    #[test]
    fn interval_bell_kind_round_trips_through_db_strings() {
        assert_eq!(IntervalBellKind::Interval.as_db_str(), "interval");
        assert_eq!(IntervalBellKind::FixedFromStart.as_db_str(), "fixed_from_start");
        assert_eq!(IntervalBellKind::FixedFromEnd.as_db_str(), "fixed_from_end");
        assert_eq!(IntervalBellKind::from_db_str("interval"), Some(IntervalBellKind::Interval));
        assert_eq!(IntervalBellKind::from_db_str("fixed_from_start"), Some(IntervalBellKind::FixedFromStart));
        assert_eq!(IntervalBellKind::from_db_str("fixed_from_end"), Some(IntervalBellKind::FixedFromEnd));
    }

    #[test]
    fn interval_bell_kind_from_db_str_rejects_unknown() {
        assert_eq!(IntervalBellKind::from_db_str(""), None);
        assert_eq!(IntervalBellKind::from_db_str("INTERVAL"), None);
        assert_eq!(IntervalBellKind::from_db_str("from_start"), None);
        assert_eq!(IntervalBellKind::from_db_str("garbage"), None);
    }

    #[test]
    fn insert_interval_bell_inserts_a_row_with_uuid_and_returns_rowid() {
        let db = Database::open_in_memory().unwrap();
        let rowid = db
            .insert_interval_bell(
                IntervalBellKind::Interval, BellTime::new(9, TimeUnit::Min), BellTime::new(162, TimeUnit::Sec), BellTime::ZERO, BUNDLED_BOWL_UUID,
                BUNDLED_PATTERN_PULSE_UUID, SignalMode::Sound,
            )
            .unwrap();
        assert!(rowid > 0);
        let bells = db.list_interval_bells().unwrap();
        assert_eq!(bells.len(), 1);
        let b = &bells[0];
        assert_eq!(b.id, rowid);
        assert!(!b.uuid.is_empty());
        assert_eq!(b.kind, IntervalBellKind::Interval);
        assert_eq!(b.time, BellTime::new(9, TimeUnit::Min));
        assert_eq!(b.jitter, BellTime::new(162, TimeUnit::Sec));
        assert_eq!(b.sound_uuid, BUNDLED_BOWL_UUID);
        assert!(b.enabled);
        assert!(!b.created_iso.is_empty());
    }

    #[test]
    fn insert_interval_bell_emits_an_interval_bell_insert_event() {
        let db = Database::open_in_memory().unwrap();
        db.insert_interval_bell(
            IntervalBellKind::FixedFromStart, BellTime::new(10, TimeUnit::Min), BellTime::ZERO, BellTime::ZERO, BUNDLED_BELL_UUID,
            BUNDLED_PATTERN_PULSE_UUID, SignalMode::Sound,
        ).unwrap();
        let events = db.pending_events().unwrap();
        let mine: Vec<_> = events
            .iter()
            .filter(|(_, e)| e.kind == "interval_bell_insert")
            .collect();
        assert_eq!(mine.len(), 1);
        let payload: serde_json::Value = serde_json::from_str(&mine[0].1.payload).unwrap();
        assert_eq!(payload["kind"], "fixed_from_start");
        assert_eq!(payload["minutes"], 10);
        assert_eq!(payload["jitter_pct"], 0);
        assert_eq!(payload["sound_uuid"], BUNDLED_BELL_UUID);
        assert_eq!(payload["enabled"], true);
        assert!(payload["uuid"].is_string());
        assert!(payload["created_iso"].is_string());
    }

    #[test]
    fn list_interval_bells_returns_rows_in_insert_order() {
        let db = Database::open_in_memory().unwrap();
        db.insert_interval_bell(IntervalBellKind::Interval, BellTime::new(5, TimeUnit::Min), BellTime::ZERO, BellTime::ZERO, BUNDLED_BOWL_UUID, BUNDLED_PATTERN_PULSE_UUID, SignalMode::Sound).unwrap();
        db.insert_interval_bell(IntervalBellKind::FixedFromStart, BellTime::new(10, TimeUnit::Min), BellTime::ZERO, BellTime::ZERO, BUNDLED_BELL_UUID, BUNDLED_PATTERN_PULSE_UUID, SignalMode::Sound).unwrap();
        db.insert_interval_bell(IntervalBellKind::FixedFromEnd, BellTime::new(5, TimeUnit::Min), BellTime::ZERO, BellTime::ZERO, BUNDLED_GONG_UUID, BUNDLED_PATTERN_PULSE_UUID, SignalMode::Sound).unwrap();
        let bells = db.list_interval_bells().unwrap();
        assert_eq!(bells.len(), 3);
        assert_eq!(bells[0].kind, IntervalBellKind::Interval);
        assert_eq!(bells[1].kind, IntervalBellKind::FixedFromStart);
        assert_eq!(bells[2].kind, IntervalBellKind::FixedFromEnd);
    }

    #[test]
    fn list_interval_bells_returns_empty_when_none_inserted() {
        let db = Database::open_in_memory().unwrap();
        assert!(db.list_interval_bells().unwrap().is_empty());
    }

    #[test]
    fn update_interval_bell_overwrites_every_mutable_field() {
        let db = Database::open_in_memory().unwrap();
        db.insert_interval_bell(IntervalBellKind::Interval, BellTime::new(5, TimeUnit::Min), BellTime::ZERO, BellTime::ZERO, BUNDLED_BOWL_UUID, BUNDLED_PATTERN_PULSE_UUID, SignalMode::Sound).unwrap();
        let original = db.list_interval_bells().unwrap()[0].clone();

        db.update_interval_bell(&IntervalBell {
            kind: IntervalBellKind::FixedFromStart,
            time: BellTime::new(12, TimeUnit::Min),
            jitter: BellTime::new(180, TimeUnit::Sec),
            step: BellTime::ZERO,
            sound_uuid: BUNDLED_BELL_UUID.into(),
            enabled: false,
            ..original.clone()
        }).unwrap();

        let b = &db.list_interval_bells().unwrap()[0];
        assert_eq!(b.kind, IntervalBellKind::FixedFromStart);
        assert_eq!(b.time, BellTime::new(12, TimeUnit::Min));
        assert_eq!(b.jitter, BellTime::new(180, TimeUnit::Sec));
        assert_eq!(b.sound_uuid, BUNDLED_BELL_UUID);
        assert!(!b.enabled);
        assert_eq!(b.uuid, original.uuid);
    }

    #[test]
    fn update_interval_bell_emits_an_interval_bell_update_event() {
        let db = Database::open_in_memory().unwrap();
        db.insert_interval_bell(IntervalBellKind::Interval, BellTime::new(5, TimeUnit::Min), BellTime::ZERO, BellTime::ZERO, BUNDLED_BOWL_UUID, BUNDLED_PATTERN_PULSE_UUID, SignalMode::Sound).unwrap();
        let original = db.list_interval_bells().unwrap()[0].clone();

        db.update_interval_bell(&IntervalBell {
            time: BellTime::new(9, TimeUnit::Min),
            jitter: BellTime::new(162, TimeUnit::Sec),
            step: BellTime::ZERO,
            sound_uuid: BUNDLED_GONG_UUID.into(),
            ..original.clone()
        }).unwrap();

        let events = db.pending_events().unwrap();
        let updates: Vec<_> = events
            .iter()
            .filter(|(_, e)| e.kind == "interval_bell_update")
            .collect();
        assert_eq!(updates.len(), 1);
        let payload: serde_json::Value = serde_json::from_str(&updates[0].1.payload).unwrap();
        assert_eq!(payload["uuid"], original.uuid.0);
        assert_eq!(payload["kind"], "interval");
        assert_eq!(payload["minutes"], 9);
        assert_eq!(payload["jitter_pct"], 30);
        assert_eq!(payload["sound_uuid"], BUNDLED_GONG_UUID);
        assert_eq!(payload["enabled"], true);
    }

    #[test]
    fn update_interval_bell_unknown_uuid_is_silent_noop() {
        let db = Database::open_in_memory().unwrap();
        db.update_interval_bell(&IntervalBell {
            id: 0,
            uuid: "non-existent-uuid".into(),
            kind: IntervalBellKind::Interval,
            time: BellTime::new(5, TimeUnit::Min),
            jitter: BellTime::ZERO,
            step: BellTime::ZERO,
            sound_uuid: BUNDLED_BOWL_UUID.into(),
            vibration_pattern_uuid: BUNDLED_PATTERN_PULSE_UUID.into(),
            signal_mode: SignalMode::Sound,
            enabled: true,
            created_iso: String::new(),
            volume: Default::default(),
        }).unwrap();
        let updates: Vec<_> = db.pending_events().unwrap()
            .into_iter()
            .filter(|(_, e)| e.kind == "interval_bell_update")
            .collect();
        assert!(updates.is_empty());
    }

    #[test]
    fn delete_interval_bell_removes_the_row() {
        let db = Database::open_in_memory().unwrap();
        db.insert_interval_bell(IntervalBellKind::Interval, BellTime::new(5, TimeUnit::Min), BellTime::ZERO, BellTime::ZERO, BUNDLED_BOWL_UUID, BUNDLED_PATTERN_PULSE_UUID, SignalMode::Sound).unwrap();
        let uuid = db.list_interval_bells().unwrap()[0].uuid.clone();
        db.delete_interval_bell(uuid.as_str()).unwrap();
        assert!(db.list_interval_bells().unwrap().is_empty());
    }

    #[test]
    fn delete_interval_bell_emits_a_delete_event_with_uuid_target() {
        let db = Database::open_in_memory().unwrap();
        db.insert_interval_bell(IntervalBellKind::Interval, BellTime::new(5, TimeUnit::Min), BellTime::ZERO, BellTime::ZERO, BUNDLED_BOWL_UUID, BUNDLED_PATTERN_PULSE_UUID, SignalMode::Sound).unwrap();
        let uuid = db.list_interval_bells().unwrap()[0].uuid.clone();
        db.delete_interval_bell(uuid.as_str()).unwrap();
        let deletes: Vec<_> = db.pending_events().unwrap()
            .into_iter()
            .filter(|(_, e)| e.kind == "interval_bell_delete")
            .collect();
        assert_eq!(deletes.len(), 1);
        assert_eq!(deletes[0].1.target_id, uuid.0);
        let payload: serde_json::Value = serde_json::from_str(&deletes[0].1.payload).unwrap();
        assert_eq!(payload["uuid"], uuid.0);
    }

    #[test]
    fn delete_interval_bell_unknown_uuid_is_silent_noop() {
        let db = Database::open_in_memory().unwrap();
        db.delete_interval_bell("non-existent-uuid").unwrap();
        let deletes: Vec<_> = db.pending_events().unwrap()
            .into_iter()
            .filter(|(_, e)| e.kind == "interval_bell_delete")
            .collect();
        assert!(deletes.is_empty());
    }

    #[test]
    fn set_interval_bell_enabled_toggles_the_flag_only() {
        let db = Database::open_in_memory().unwrap();
        db.insert_interval_bell(IntervalBellKind::Interval, BellTime::new(9, TimeUnit::Min), BellTime::new(162, TimeUnit::Sec), BellTime::ZERO, BUNDLED_BELL_UUID, BUNDLED_PATTERN_PULSE_UUID, SignalMode::Sound).unwrap();
        let uuid = db.list_interval_bells().unwrap()[0].uuid.clone();

        db.set_interval_bell_enabled(uuid.as_str(), false).unwrap();
        let b = &db.list_interval_bells().unwrap()[0];
        assert!(!b.enabled);
        assert_eq!(b.kind, IntervalBellKind::Interval);
        assert_eq!(b.time, BellTime::new(9, TimeUnit::Min));
        assert_eq!(b.jitter, BellTime::new(162, TimeUnit::Sec));
        assert_eq!(b.sound_uuid, BUNDLED_BELL_UUID);

        db.set_interval_bell_enabled(uuid.as_str(), true).unwrap();
        assert!(db.list_interval_bells().unwrap()[0].enabled);
    }

    #[test]
    fn set_interval_bell_enabled_emits_an_update_event_with_new_state() {
        let db = Database::open_in_memory().unwrap();
        db.insert_interval_bell(IntervalBellKind::Interval, BellTime::new(9, TimeUnit::Min), BellTime::new(162, TimeUnit::Sec), BellTime::ZERO, BUNDLED_BELL_UUID, BUNDLED_PATTERN_PULSE_UUID, SignalMode::Sound).unwrap();
        let uuid = db.list_interval_bells().unwrap()[0].uuid.clone();
        db.set_interval_bell_enabled(uuid.as_str(), false).unwrap();
        let updates: Vec<_> = db.pending_events().unwrap()
            .into_iter()
            .filter(|(_, e)| e.kind == "interval_bell_update")
            .collect();
        assert_eq!(updates.len(), 1);
        let payload: serde_json::Value = serde_json::from_str(&updates[0].1.payload).unwrap();
        assert_eq!(payload["enabled"], false);
        assert_eq!(payload["minutes"], 9);
    }

    #[test]
    fn apply_event_interval_bell_insert_creates_the_row() {
        let db = Database::open_in_memory().unwrap();
        db.apply_event(&synth_interval_bell_insert(
            "bell-1", 5, "dev-A",
            IntervalBellKind::Interval, 9, 30, BUNDLED_BELL_UUID,
        )).unwrap();
        let bells = db.list_interval_bells().unwrap();
        assert_eq!(bells.len(), 1);
        assert_eq!(bells[0].uuid, "bell-1");
        assert_eq!(bells[0].kind, IntervalBellKind::Interval);
        assert_eq!(bells[0].time, BellTime::new(9, TimeUnit::Min));
        assert_eq!(bells[0].jitter, BellTime::new(162, TimeUnit::Sec));
        assert_eq!(bells[0].sound_uuid, BUNDLED_BELL_UUID);
        assert!(bells[0].enabled);
    }

    #[test]
    fn apply_event_interval_bell_update_applies_after_insert() {
        let db = Database::open_in_memory().unwrap();
        db.apply_event(&synth_interval_bell_insert(
            "bell-1", 5, "dev-A",
            IntervalBellKind::Interval, 9, 30, BUNDLED_BELL_UUID,
        )).unwrap();
        db.apply_event(&synth_interval_bell_update(
            "bell-1", 7, "dev-A", 12, false,
        )).unwrap();
        let b = &db.list_interval_bells().unwrap()[0];
        assert_eq!(b.time, BellTime::new(12, TimeUnit::Min));
        assert!(!b.enabled);
    }

    #[test]
    fn apply_event_interval_bell_delete_removes_the_row() {
        let db = Database::open_in_memory().unwrap();
        db.apply_event(&synth_interval_bell_insert(
            "bell-1", 5, "dev-A",
            IntervalBellKind::Interval, 9, 30, BUNDLED_BELL_UUID,
        )).unwrap();
        db.apply_event(&synth_interval_bell_delete("bell-1", 6, "dev-A")).unwrap();
        assert!(db.list_interval_bells().unwrap().is_empty());
    }

    #[test]
    fn apply_event_interval_bell_tombstone_resists_lower_lamport_insert() {
        let db = Database::open_in_memory().unwrap();
        db.apply_event(&synth_interval_bell_delete("bell-1", 10, "dev-A")).unwrap();
        db.apply_event(&synth_interval_bell_insert(
            "bell-1", 5, "dev-A",
            IntervalBellKind::Interval, 9, 30, BUNDLED_BELL_UUID,
        )).unwrap();
        assert!(db.list_interval_bells().unwrap().is_empty());
    }

    #[test]
    fn apply_event_interval_bell_higher_lamport_update_supersedes_lower_one() {
        let db = Database::open_in_memory().unwrap();
        db.apply_event(&synth_interval_bell_insert(
            "bell-1", 5, "dev-A",
            IntervalBellKind::Interval, 9, 30, BUNDLED_BELL_UUID,
        )).unwrap();
        db.apply_event(&synth_interval_bell_update("bell-1", 7, "dev-A", 12, true)).unwrap();
        db.apply_event(&synth_interval_bell_update("bell-1", 8, "dev-B", 18, true)).unwrap();
        let b = &db.list_interval_bells().unwrap()[0];
        assert_eq!(b.time, BellTime::new(18, TimeUnit::Min));
    }

    #[test]
    fn apply_event_interval_bell_replay_round_trip_across_peers() {
        let dev_a = Database::open_in_memory().unwrap();
        dev_a.insert_interval_bell(IntervalBellKind::Interval, BellTime::new(9, TimeUnit::Min), BellTime::new(162, TimeUnit::Sec), BellTime::ZERO, BUNDLED_BELL_UUID, BUNDLED_PATTERN_PULSE_UUID, SignalMode::Sound).unwrap();
        let original = dev_a.list_interval_bells().unwrap()[0].clone();
        dev_a.update_interval_bell(&IntervalBell {
            kind: IntervalBellKind::FixedFromStart,
            time: BellTime::new(10, TimeUnit::Min),
            jitter: BellTime::ZERO,
            step: BellTime::ZERO,
            sound_uuid: BUNDLED_GONG_UUID.into(),
            ..original.clone()
        }).unwrap();
        let uuid = original.uuid;

        let events: Vec<Event> = dev_a.pending_events().unwrap()
            .into_iter()
            .filter(|(_, e)| e.kind.starts_with("interval_bell_"))
            .map(|(_, e)| e)
            .collect();

        let dev_b = Database::open_in_memory().unwrap();
        dev_b.replay_events(&events).unwrap();
        let bells_b = dev_b.list_interval_bells().unwrap();
        assert_eq!(bells_b.len(), 1);
        let b = &bells_b[0];
        assert_eq!(b.uuid, uuid);
        assert_eq!(b.kind, IntervalBellKind::FixedFromStart);
        assert_eq!(b.time, BellTime::new(10, TimeUnit::Min));
        assert_eq!(b.sound_uuid, BUNDLED_GONG_UUID);
    }

    // ── Volume ─────────────────────────────────────────────────────

    use crate::bell_volume::BellVolume;

    fn only_bell(db: &Database) -> IntervalBell {
        let bells = db.list_interval_bells().unwrap();
        assert_eq!(bells.len(), 1);
        bells.into_iter().next().unwrap()
    }

    fn insert_one(db: &Database) -> IntervalBell {
        db.insert_interval_bell(
            IntervalBellKind::Interval, BellTime::new(5, TimeUnit::Min), BellTime::ZERO, BellTime::ZERO, BUNDLED_BOWL_UUID,
            BUNDLED_PATTERN_PULSE_UUID, SignalMode::Sound,
        )
        .unwrap();
        only_bell(db)
    }

    #[test]
    fn a_new_bell_starts_at_the_middle_volume() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(insert_one(&db).volume, BellVolume::default());
        let insert = db.pending_events().unwrap().into_iter().map(|(_, e)| e)
            .find(|e| e.kind == "interval_bell_insert").unwrap();
        let payload: serde_json::Value = serde_json::from_str(&insert.payload).unwrap();
        assert_eq!(payload["volume"], 50);
    }

    #[test]
    fn update_stores_the_volume_and_syncs_it() {
        let db = Database::open_in_memory().unwrap();
        let bell = insert_one(&db);
        let volume = BellVolume::from_percent(30.0);
        db.update_interval_bell(&IntervalBell { volume, ..bell }).unwrap();
        assert_eq!(only_bell(&db).volume, volume);
        let update = db.pending_events().unwrap().into_iter().map(|(_, e)| e)
            .find(|e| e.kind == "interval_bell_update").unwrap();
        let payload: serde_json::Value = serde_json::from_str(&update.payload).unwrap();
        assert_eq!(payload["volume"], 30);
    }

    #[test]
    fn toggling_enabled_keeps_the_volume() {
        let db = Database::open_in_memory().unwrap();
        let bell = insert_one(&db);
        let volume = BellVolume::from_percent(80.0);
        db.update_interval_bell(&IntervalBell { volume, ..bell.clone() }).unwrap();
        db.set_interval_bell_enabled(bell.uuid.as_str(), false).unwrap();
        assert_eq!(only_bell(&db).volume, volume);
    }

    #[test]
    fn a_synced_volume_lands_in_the_row() {
        let db = Database::open_in_memory().unwrap();
        let mut event = synth_interval_bell_update("b1", 5, "peer", 5, true);
        let mut payload: serde_json::Value = serde_json::from_str(&event.payload).unwrap();
        payload["volume"] = 70.into();
        event.payload = payload.to_string();
        db.apply_event(&event).unwrap();
        assert_eq!(only_bell(&db).volume, BellVolume::from_percent(70.0));
    }

    #[test]
    fn a_synced_bell_from_an_older_version_gets_the_middle_volume() {
        // Events written before per-bell volumes carry no "volume".
        let db = Database::open_in_memory().unwrap();
        db.apply_event(&synth_interval_bell_update("b1", 5, "peer", 5, true)).unwrap();
        assert_eq!(only_bell(&db).volume, BellVolume::default());
    }

    #[test]
    fn a_bad_synced_volume_is_repaired() {
        let db = Database::open_in_memory().unwrap();
        let mut event = synth_interval_bell_update("b1", 5, "peer", 5, true);
        let mut payload: serde_json::Value = serde_json::from_str(&event.payload).unwrap();
        payload["volume"] = 333.into();
        event.payload = payload.to_string();
        db.apply_event(&event).unwrap();
        assert_eq!(only_bell(&db).volume.percent(), 100);
    }

    // ── Times in seconds ───────────────────────────────────────────

    fn s(n: u32) -> BellTime { BellTime::new(n, TimeUnit::Sec) }
    fn m(n: u32) -> BellTime { BellTime::new(n, TimeUnit::Min) }

    #[test]
    fn a_time_keeps_the_unit_it_was_set_in() {
        let db = Database::open_in_memory().unwrap();
        for time in [s(60), m(1)] {
            db.insert_interval_bell(
                IntervalBellKind::Interval, time, BellTime::ZERO, BellTime::ZERO,
                BUNDLED_BOWL_UUID, BUNDLED_PATTERN_PULSE_UUID, SignalMode::Sound,
            )
            .unwrap();
        }
        let times: Vec<_> = db.list_interval_bells().unwrap().iter().map(|b| b.time).collect();
        assert_eq!(times, [s(60), m(1)]);
        // And on the other device.
        let peer = Database::open_in_memory().unwrap();
        for (_, e) in db.pending_events().unwrap() {
            peer.apply_event(&e).unwrap();
        }
        let times: Vec<_> = peer.list_interval_bells().unwrap().iter().map(|b| b.time).collect();
        assert_eq!(times, [s(60), m(1)]);
        let titles: Vec<_> = peer.list_interval_bells().unwrap().iter().map(crate::bells::bell_title_key).collect();
        let every = |time| crate::bells::BellTitleKey::Every { time, jitter: None, step: None };
        assert_eq!(titles, [every(s(60)), every(m(1))]);
    }

    #[test]
    fn a_synced_bell_from_an_older_version_reads_in_seconds() {
        let db = Database::open_in_memory().unwrap();
        db.apply_event(&synth_interval_bell_insert(
            "b1", 5, "peer", IntervalBellKind::Interval, 5, 20, BUNDLED_BELL_UUID,
        ))
        .unwrap();
        let b = only_bell(&db);
        assert_eq!((b.time, b.jitter, b.step), (m(5), m(1), BellTime::ZERO));
    }

    #[test]
    fn a_bell_in_seconds_also_writes_minutes_and_percent() {
        let db = Database::open_in_memory().unwrap();
        db.insert_interval_bell(
            IntervalBellKind::Interval, s(90), s(30), s(5),
            BUNDLED_BOWL_UUID, BUNDLED_PATTERN_PULSE_UUID, SignalMode::Sound,
        )
        .unwrap();
        let events = db.pending_events().unwrap();
        let insert = events.iter().find(|(_, e)| e.kind == "interval_bell_insert").unwrap();
        let payload: serde_json::Value = serde_json::from_str(&insert.1.payload).unwrap();
        assert_eq!((payload["minutes"].as_u64(), payload["jitter_pct"].as_u64()), (Some(2), Some(33)));
        let (minutes, pct): (i64, i64) = db.conn
            .query_row("SELECT minutes, jitter_pct FROM interval_bells", [], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap();
        assert_eq!((minutes, pct), (2, 33));
    }

    #[test]
    fn a_row_an_older_version_changed_reads_from_its_minutes() {
        let db = Database::open_in_memory().unwrap();
        db.insert_interval_bell(
            IntervalBellKind::Interval, s(90), BellTime::ZERO, s(5),
            BUNDLED_BOWL_UUID, BUNDLED_PATTERN_PULSE_UUID, SignalMode::Sound,
        )
        .unwrap();
        let uuid = only_bell(&db).uuid;
        // What 26.10.1 writes when it changes the bell to 3 min: its row
        // update and an event, neither knowing the seconds fields.
        db.conn.execute("UPDATE interval_bells SET minutes = 3", []).unwrap();
        let mut event = synth_interval_bell_update(uuid.as_str(), 99, "zz-this-device", 3, true);
        event.event_uuid = "release-edit".into();
        db.conn
            .execute(
                "INSERT INTO events (event_uuid, lamport_ts, device_id, kind, target_id, payload)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![event.event_uuid, event.lamport_ts, event.device_id, event.kind, event.target_id, event.payload],
            )
            .unwrap();
        let b = only_bell(&db);
        assert_eq!((b.time, b.step), (m(3), BellTime::ZERO), "read before the rebuild");
        let Database { conn, .. } = db;
        let db = Database::init(conn).unwrap();
        let b = only_bell(&db);
        assert_eq!((b.time, b.step), (m(3), BellTime::ZERO), "and after it");
        // The reopen rebuilt the stored seconds from the release's event.
        let stored: (i64, String, i64) = db.conn
            .query_row("SELECT secs, secs_unit, step_secs FROM interval_bells", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .unwrap();
        assert_eq!(stored, (180, "min".to_string(), 0));
    }
}
