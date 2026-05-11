//! `device` table: stable device UUID + Lamport clock.

use rusqlite::{params, OptionalExtension};

use super::{Database, Result};

impl Database {
    /// This database's stable device UUID. Generated lazily on first call
    /// after a fresh DB and persisted in the single-row `device` table —
    /// every subsequent call (including after the process restarts and
    /// reopens the file) returns the same value. The id tags every
    /// locally-authored event so devices can attribute writes during
    /// merge.
    pub fn device_id(&self) -> Result<String> {
        if let Some(existing) = self
            .conn
            .query_row("SELECT device_id FROM device LIMIT 1", [], |row| {
                row.get::<_, String>(0)
            })
            .optional()?
        {
            return Ok(existing);
        }
        // First call on a fresh DB — mint a new id and remember it.
        let new_id = uuid::Uuid::new_v4().to_string();
        self.conn.execute(
            "INSERT INTO device (device_id) VALUES (?1)",
            params![new_id],
        )?;
        Ok(new_id)
    }

    /// Current Lamport clock value (0 on a fresh DB). Returns even before
    /// `device_id()` has been called: an empty `device` table reads back
    /// the column default rather than failing.
    pub fn lamport_clock(&self) -> Result<i64> {
        let v: Option<i64> = self
            .conn
            .query_row("SELECT lamport_clock FROM device LIMIT 1", [], |row| {
                row.get::<_, i64>(0)
            })
            .optional()?;
        Ok(v.unwrap_or(0))
    }

    /// Increment the Lamport clock by 1; return the new value (so the
    /// caller can stamp the event they're about to author with it). On
    /// a fresh DB this also seeds the single `device` row.
    pub fn bump_lamport_clock(&self) -> Result<i64> {
        // Make sure a row exists — sharing the existing seed path with
        // `device_id` keeps the device_id and lamport_clock in the same
        // single row, as the schema requires (device_id is PRIMARY KEY).
        let _ = self.device_id()?;
        self.conn.execute(
            "UPDATE device SET lamport_clock = lamport_clock + 1",
            [],
        )?;
        self.lamport_clock()
    }

    /// Apply the Lamport observation rule: set local = max(local, remote)
    /// + 1. Returns the new local value. Always strictly increases the
    /// clock, so any event authored after observation sorts after the
    /// remote one we just witnessed.
    pub fn observe_remote_lamport(&self, remote_ts: i64) -> Result<i64> {
        let _ = self.device_id()?;
        // Single statement, no read-modify-write race: SQL computes
        // max(stored, ?) + 1 inline.
        self.conn.execute(
            "UPDATE device SET lamport_clock = MAX(lamport_clock, ?1) + 1",
            params![remote_ts],
        )?;
        self.lamport_clock()
    }
}
