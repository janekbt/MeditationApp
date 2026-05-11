//! `settings` table — user-facing preference key/value store. Every
//! write emits a `setting_changed` event so peers converge via the
//! same Lamport-ts precedence rules as the entity rows.

use rusqlite::{params, OptionalExtension};

use super::{Database, DbError, Result};

impl Database {
    /// Read the value of a settings key. Returns `default` (without
    /// inserting it) when the key has never been set.
    pub fn get_setting(&self, key: &str, default: &str) -> Result<String> {
        match self.conn.query_row(
            "SELECT value FROM settings WHERE key = ?1",
            params![key],
            |row| row.get::<_, String>(0),
        ) {
            Ok(val) => Ok(val),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(default.to_string()),
            Err(e) => Err(DbError::Sqlite(e)),
        }
    }

    /// Write a settings value. Upserts: subsequent calls overwrite.
    /// Each call emits its own `setting_changed` event — peers
    /// last-write-wins by Lamport ts, so collapsing two overwrites to
    /// one event would lose the intermediate ordering.
    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        self.conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        let payload = serde_json::json!({
            "key": key,
            "value": value,
        }).to_string();
        self.emit_event("setting_changed", key, payload)?;
        tx.commit()?;
        Ok(())
    }

    /// Recompute the `settings` value for `key` from the events table.
    /// No tombstone — settings have no `setting_delete` kind, every
    /// write is a `setting_changed` event. Highest (lamport_ts,
    /// device_id) wins; if no events exist for the key the row is left
    /// alone (the local cache may have a value from a pre-event-log
    /// build, which we treat as already-converged).
    pub(super) fn recompute_setting(&self, key: &str) -> Result<()> {
        let mutate: Option<String> = self.conn.query_row(
            "SELECT payload FROM events
             WHERE target_id = ?1 AND kind = 'setting_changed'
             ORDER BY lamport_ts DESC, device_id DESC
             LIMIT 1",
            params![key],
            |row| row.get::<_, String>(0),
        ).optional()?;

        if let Some(payload) = mutate {
            let v: serde_json::Value = serde_json::from_str(&payload)
                .map_err(|e| DbError::Csv(
                    format!("setting_changed payload not valid JSON: {e}")))?;
            let value = v["value"].as_str().unwrap_or_default();
            self.conn.execute(
                "INSERT INTO settings (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )?;
        }
        Ok(())
    }
}
