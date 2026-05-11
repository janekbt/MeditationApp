//! `sync_state` table — local-only sync-loop bookkeeping (server URL,
//! last-pull cursor, last-sync timestamp, last-sync-error). Separate
//! from `settings` because settings are event-sourced and would sync
//! to peers, whereas sync_state is device-private.

use rusqlite::params;

use super::{Database, DbError, Result};

impl Database {
    /// Read a sync-state value (server URL, last-pull cursor, …),
    /// returning `default` if the key has never been set. Mirrors
    /// `get_setting` but keyed against the `sync_state` namespace.
    pub fn get_sync_state(&self, key: &str, default: &str) -> Result<String> {
        match self.conn.query_row(
            "SELECT value FROM sync_state WHERE key = ?1",
            params![key],
            |row| row.get::<_, String>(0),
        ) {
            Ok(val) => Ok(val),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(default.to_string()),
            Err(e) => Err(DbError::Sqlite(e)),
        }
    }

    /// Upsert a sync-state value. Subsequent calls overwrite. Mirrors
    /// `set_setting`'s semantics in the `sync_state` namespace.
    pub fn set_sync_state(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO sync_state (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }
}
