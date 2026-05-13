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
    pub(crate) fn bump_lamport_clock(&self) -> Result<i64> {
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

    /// Apply the Lamport observation rule: set `local = max(local,
    /// remote) + 1`, returning the new local value. Always strictly
    /// increases the clock, so any event authored after observation
    /// sorts after the remote one we just witnessed.
    pub(crate) fn observe_remote_lamport(&self, remote_ts: i64) -> Result<i64> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_helpers::*;

    #[test]
    fn device_id_is_a_v4_uuid() {
        let db = Database::open_in_memory().unwrap();
        let id = db.device_id().unwrap();
        assert!(looks_like_uuid_v4(&id),
            "device_id `{id}` doesn't match v4 shape");
    }

    #[test]
    fn device_id_is_stable_across_calls_within_one_process() {
        let db = Database::open_in_memory().unwrap();
        let a = db.device_id().unwrap();
        let b = db.device_id().unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn device_id_is_stable_across_database_reopens() {
        // Persistence: closing the DB and reopening the same file must
        // yield the same device_id. This is the actual cross-restart
        // contract; the in-memory variant above only proves "same call,
        // same answer".
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("device_id.db");
        let id_first = {
            let db = Database::open(&path).unwrap();
            db.device_id().unwrap()
        };
        let id_second = {
            let db = Database::open(&path).unwrap();
            db.device_id().unwrap()
        };
        assert_eq!(id_first, id_second);
    }

    #[test]
    fn two_separate_databases_get_different_device_ids() {
        let db_a = Database::open_in_memory().unwrap();
        let db_b = Database::open_in_memory().unwrap();
        assert_ne!(db_a.device_id().unwrap(), db_b.device_id().unwrap());
    }

    #[test]
    fn lamport_clock_is_zero_on_a_fresh_database() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(db.lamport_clock().unwrap(), 0);
    }

    #[test]
    fn lamport_clock_starts_at_zero_even_before_device_id_is_minted() {
        let db = Database::open_in_memory().unwrap();
        let _ = db.lamport_clock().unwrap();
        assert_eq!(db.lamport_clock().unwrap(), 0);
    }

    #[test]
    fn bump_lamport_clock_returns_post_increment_value() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(db.bump_lamport_clock().unwrap(), 1);
        assert_eq!(db.bump_lamport_clock().unwrap(), 2);
        assert_eq!(db.bump_lamport_clock().unwrap(), 3);
    }

    #[test]
    fn bump_lamport_clock_persists_the_increment() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(db.bump_lamport_clock().unwrap(), 1);
        assert_eq!(db.lamport_clock().unwrap(), 1);
    }

    #[test]
    fn bump_lamport_clock_persists_across_database_reopens() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lamport.db");
        let mid = {
            let db = Database::open(&path).unwrap();
            db.bump_lamport_clock().unwrap();
            db.bump_lamport_clock().unwrap()
        };
        assert_eq!(mid, 2);
        let after_reopen = {
            let db = Database::open(&path).unwrap();
            db.lamport_clock().unwrap()
        };
        assert_eq!(after_reopen, 2);
        let bumped = {
            let db = Database::open(&path).unwrap();
            db.bump_lamport_clock().unwrap()
        };
        assert_eq!(bumped, 3);
    }

    #[test]
    fn observe_remote_lamport_advances_when_remote_is_ahead() {
        let db = Database::open_in_memory().unwrap();
        let new_local = db.observe_remote_lamport(42).unwrap();
        assert_eq!(new_local, 43);
        assert_eq!(db.lamport_clock().unwrap(), 43);
    }

    #[test]
    fn observe_remote_lamport_keeps_advancing_when_local_is_already_ahead() {
        let db = Database::open_in_memory().unwrap();
        for _ in 0..100 { db.bump_lamport_clock().unwrap(); }
        let new_local = db.observe_remote_lamport(7).unwrap();
        assert_eq!(new_local, 101);
        assert_eq!(db.lamport_clock().unwrap(), 101);
    }

    #[test]
    fn observe_remote_lamport_treats_equal_as_max_plus_one() {
        let db = Database::open_in_memory().unwrap();
        for _ in 0..5 { db.bump_lamport_clock().unwrap(); }
        let new_local = db.observe_remote_lamport(5).unwrap();
        assert_eq!(new_local, 6);
    }

    #[test]
    fn observe_remote_lamport_handles_zero() {
        let db = Database::open_in_memory().unwrap();
        let new_local = db.observe_remote_lamport(0).unwrap();
        assert_eq!(new_local, 1);
    }
}

