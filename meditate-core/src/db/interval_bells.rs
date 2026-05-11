//! `interval_bells` table — user-configured bells that fire during
//! Timer-mode sessions. Three kinds: periodic-with-jitter,
//! fixed-from-start, fixed-from-end. All CRUD ops emit sync events
//! so the library round-trips across devices.

use rusqlite::{params, OptionalExtension};

use super::{Database, DbError, Result, SignalMode};

/// One configured bell entry in the user's interval-bell library.
/// All enabled rows fire as bells during a Timer-mode session;
/// Box Breathing is exempt. Three kinds:
///
/// - `Interval` — every `minutes` ± `jitter_pct`% of itself, rerolled
///   on each ring. A 9-min ±30% bell fires somewhere in 6.3–11.7 min,
///   never settling into a predictable beat (defeats anticipation).
/// - `FixedFromStart` — at exactly `minutes` elapsed (e.g., switch
///   from metta to breath at 10:00). `jitter_pct` is ignored.
/// - `FixedFromEnd` — at `minutes` before session end (only meaningful
///   in countdown mode; stopwatch sessions skip these). `jitter_pct`
///   is ignored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntervalBell {
    pub id: i64,
    pub uuid: String,
    pub kind: IntervalBellKind,
    pub minutes: u32,
    pub jitter_pct: u32,
    pub sound: String,
    pub vibration_pattern_uuid: String,
    pub signal_mode: SignalMode,
    pub enabled: bool,
    pub created_iso: String,
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
        minutes: u32,
        jitter_pct: u32,
        sound: &str,
        vibration_pattern_uuid: &str,
        signal_mode: SignalMode,
    ) -> Result<i64> {
        let tx = self.conn.unchecked_transaction()?;
        let bell_uuid = uuid::Uuid::new_v4().to_string();
        let created_iso = chrono::Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO interval_bells
                (uuid, kind, minutes, jitter_pct, sound,
                 vibration_pattern_uuid, signal_mode, enabled, created_iso)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1, ?8)",
            params![
                bell_uuid,
                kind.as_db_str(),
                minutes,
                jitter_pct,
                sound,
                vibration_pattern_uuid,
                signal_mode.as_db_str(),
                created_iso,
            ],
        )?;
        let rowid = self.conn.last_insert_rowid();
        let payload = serde_json::json!({
            "uuid": bell_uuid,
            "kind": kind.as_db_str(),
            "minutes": minutes,
            "jitter_pct": jitter_pct,
            "sound": sound,
            "vibration_pattern_uuid": vibration_pattern_uuid,
            "signal_mode": signal_mode.as_db_str(),
            "enabled": true,
            "created_iso": created_iso,
        }).to_string();
        self.emit_event("interval_bell_insert", &bell_uuid, payload)?;
        tx.commit()?;
        Ok(rowid)
    }

    /// Overwrite every mutable field of the bell with `uuid`. UUID +
    /// created_iso are immutable. Unknown uuids are a silent no-op AND
    /// emit no event — peers receiving an update for a row they've
    /// already tombstoned should not be reflected back as "this bell
    /// is alive again". Mirrors `update_label`'s shape.
    pub fn update_interval_bell(
        &self,
        uuid: &str,
        kind: IntervalBellKind,
        minutes: u32,
        jitter_pct: u32,
        sound: &str,
        vibration_pattern_uuid: &str,
        signal_mode: SignalMode,
        enabled: bool,
    ) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        let created_iso: Option<String> = self.conn.query_row(
            "SELECT created_iso FROM interval_bells WHERE uuid = ?1",
            params![uuid],
            |row| row.get::<_, String>(0),
        ).optional()?;
        let Some(created_iso) = created_iso else {
            return Ok(());
        };
        self.conn.execute(
            "UPDATE interval_bells
                SET kind = ?1, minutes = ?2, jitter_pct = ?3, sound = ?4,
                    vibration_pattern_uuid = ?5, signal_mode = ?6, enabled = ?7
              WHERE uuid = ?8",
            params![
                kind.as_db_str(),
                minutes,
                jitter_pct,
                sound,
                vibration_pattern_uuid,
                signal_mode.as_db_str(),
                enabled as i64,
                uuid,
            ],
        )?;
        let payload = serde_json::json!({
            "uuid": uuid,
            "kind": kind.as_db_str(),
            "minutes": minutes,
            "jitter_pct": jitter_pct,
            "sound": sound,
            "vibration_pattern_uuid": vibration_pattern_uuid,
            "signal_mode": signal_mode.as_db_str(),
            "enabled": enabled,
            "created_iso": created_iso,
        }).to_string();
        self.emit_event("interval_bell_update", uuid, payload)?;
        tx.commit()?;
        Ok(())
    }

    /// Convenience for the common path — toggling enabled without the
    /// UI having to read the other fields back. Emits the same
    /// `interval_bell_update` event as a full-fields update so the
    /// sync replay code only has to handle one update kind.
    pub fn set_interval_bell_enabled(&self, uuid: &str, enabled: bool) -> Result<()> {
        let row: Option<(String, u32, u32, String, String, String)> = self.conn.query_row(
            "SELECT kind, minutes, jitter_pct, sound,
                    vibration_pattern_uuid, signal_mode
               FROM interval_bells WHERE uuid = ?1",
            params![uuid],
            |row| Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)? as u32,
                row.get::<_, i64>(2)? as u32,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
            )),
        ).optional()?;
        let Some((kind_str, minutes, jitter_pct, sound,
                  vibration_pattern_uuid, signal_mode_str)) = row else {
            return Ok(());
        };
        let kind = IntervalBellKind::from_db_str(&kind_str)
            .expect("interval_bells.kind violates CHECK constraint");
        let signal_mode = SignalMode::from_db_str(&signal_mode_str)
            .expect("interval_bells.signal_mode violates CHECK constraint");
        self.update_interval_bell(
            uuid, kind, minutes, jitter_pct, &sound,
            &vibration_pattern_uuid, signal_mode, enabled,
        )
    }

    /// Remove the bell row with `uuid` and emit a tombstone event.
    /// Unknown uuids are silent no-ops AND emit no event (peers
    /// shouldn't get a delete for a row they never knew existed).
    pub fn delete_interval_bell(&self, uuid: &str) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        let exists: Option<i64> = self.conn.query_row(
            "SELECT id FROM interval_bells WHERE uuid = ?1",
            params![uuid],
            |row| row.get::<_, i64>(0),
        ).optional()?;
        if exists.is_none() {
            return Ok(());
        }
        self.conn.execute(
            "DELETE FROM interval_bells WHERE uuid = ?1",
            params![uuid],
        )?;
        let payload = serde_json::json!({ "uuid": uuid }).to_string();
        self.emit_event("interval_bell_delete", uuid, payload)?;
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

    /// Every bell row in insert order. The B.3.3 list page renders this
    /// directly. Order is `id ASC` (rowid) — deterministic and stable
    /// across reads, matches the user's mental model of "first one I
    /// added is at the top".
    pub fn list_interval_bells(&self) -> Result<Vec<IntervalBell>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, uuid, kind, minutes, jitter_pct, sound,
                    vibration_pattern_uuid, signal_mode, enabled, created_iso
             FROM interval_bells
             ORDER BY id ASC",
        )?;
        let rows = stmt
            .query_map([], |row| {
                let kind_str: String = row.get(2)?;
                let signal_mode_str: String = row.get(7)?;
                Ok(IntervalBell {
                    id: row.get(0)?,
                    uuid: row.get(1)?,
                    kind: IntervalBellKind::from_db_str(&kind_str)
                        .expect("interval_bells.kind violates CHECK constraint"),
                    minutes: row.get::<_, i64>(3)? as u32,
                    jitter_pct: row.get::<_, i64>(4)? as u32,
                    sound: row.get(5)?,
                    vibration_pattern_uuid: row.get(6)?,
                    signal_mode: SignalMode::from_db_str(&signal_mode_str)
                        .expect("interval_bells.signal_mode violates CHECK constraint"),
                    enabled: row.get::<_, i64>(8)? != 0,
                    created_iso: row.get(9)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Recompute the `interval_bells` row for `bell_uuid` from the
    /// events table. Same precedence rules as labels: tombstone wins
    /// on tie/precedence, else the highest-(lamport, device_id) mutate
    /// event drives the row's values. Update events carry every
    /// mutable field plus created_iso so they're self-sufficient if
    /// the corresponding insert event hasn't arrived yet.
    pub(super) fn recompute_interval_bell(&self, bell_uuid: &str) -> Result<()> {
        let delete_ts: Option<i64> = self.conn.query_row(
            "SELECT MAX(lamport_ts) FROM events
             WHERE target_id = ?1 AND kind = 'interval_bell_delete'",
            params![bell_uuid],
            |row| row.get::<_, Option<i64>>(0),
        )?;
        let mutate: Option<(i64, String)> = self.conn.query_row(
            "SELECT lamport_ts, payload FROM events
             WHERE target_id = ?1
               AND kind IN ('interval_bell_insert', 'interval_bell_update')
             ORDER BY lamport_ts DESC, device_id DESC
             LIMIT 1",
            params![bell_uuid],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
        ).optional()?;

        let row_should_exist = match (mutate.as_ref(), delete_ts) {
            (Some(_), None) => true,
            (None, _) => false,
            (Some((m_ts, _)), Some(d_ts)) => *m_ts > d_ts,
        };

        if let Some((_, payload)) = mutate.filter(|_| row_should_exist) {
            let v: serde_json::Value = serde_json::from_str(&payload)
                .map_err(|e| DbError::Csv(
                    format!("interval_bell event payload not valid JSON: {e}")))?;
            let kind = v["kind"].as_str().unwrap_or("interval");
            let minutes = v["minutes"].as_u64().unwrap_or(0) as u32;
            let jitter_pct = v["jitter_pct"].as_u64().unwrap_or(0) as u32;
            let sound = v["sound"].as_str().unwrap_or("bowl");
            let vibration_pattern_uuid = v["vibration_pattern_uuid"]
                .as_str()
                .unwrap_or("7e9c4d2f-5a8b-4f1d-9e3c-2d6f7a8b0001");
            let signal_mode = v["signal_mode"].as_str().unwrap_or("sound");
            let enabled = v["enabled"].as_bool().unwrap_or(true);
            let created_iso = v["created_iso"].as_str().unwrap_or_default();
            self.conn.execute(
                "INSERT INTO interval_bells
                    (uuid, kind, minutes, jitter_pct, sound,
                     vibration_pattern_uuid, signal_mode, enabled, created_iso)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT(uuid) DO UPDATE SET
                    kind                   = excluded.kind,
                    minutes                = excluded.minutes,
                    jitter_pct             = excluded.jitter_pct,
                    sound                  = excluded.sound,
                    vibration_pattern_uuid = excluded.vibration_pattern_uuid,
                    signal_mode            = excluded.signal_mode,
                    enabled                = excluded.enabled,
                    created_iso            = excluded.created_iso",
                params![
                    bell_uuid,
                    kind,
                    minutes,
                    jitter_pct,
                    sound,
                    vibration_pattern_uuid,
                    signal_mode,
                    enabled as i64,
                    created_iso,
                ],
            )?;
        } else {
            self.conn.execute(
                "DELETE FROM interval_bells WHERE uuid = ?1",
                params![bell_uuid],
            )?;
        }
        Ok(())
    }
}
