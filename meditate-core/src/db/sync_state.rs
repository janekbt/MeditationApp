//! `sync_state` table — local-only sync-loop bookkeeping (server URL,
//! last-pull cursor, last-sync timestamp, last-sync-error). Separate
//! from `settings` because settings are event-sourced and would sync
//! to peers, whereas sync_state is device-private.

use super::{Database, Result};

impl Database {
    /// Read a sync-state value (server URL, last-pull cursor, …),
    /// returning `default` if the key has never been set. Mirrors
    /// `get_setting` but keyed against the `sync_state` namespace —
    /// sync_state is device-local and doesn't emit events, so writes
    /// go through `write_kv` directly without the `set_setting`
    /// transaction + emit_event step.
    pub fn get_sync_state(&self, key: &str, default: &str) -> Result<String> {
        self.read_kv("sync_state", key, default)
    }

    /// Upsert a sync-state value. Subsequent calls overwrite. Mirrors
    /// `set_setting`'s semantics in the `sync_state` namespace.
    pub fn set_sync_state(&self, key: &str, value: &str) -> Result<()> {
        self.write_kv("sync_state", key, value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_sync_state_returns_default_on_a_fresh_database() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(db.get_sync_state("server_url", "fallback").unwrap(),
                   "fallback");
    }

    #[test]
    fn get_sync_state_returns_default_for_unknown_key_after_other_keys_set() {
        let db = Database::open_in_memory().unwrap();
        db.set_sync_state("server_url", "https://nc.example").unwrap();
        assert_eq!(db.get_sync_state("missing", "fallback").unwrap(),
                   "fallback");
    }

    #[test]
    fn set_then_get_sync_state_round_trips_the_value() {
        let db = Database::open_in_memory().unwrap();
        db.set_sync_state("server_url", "https://nc.example").unwrap();
        assert_eq!(db.get_sync_state("server_url", "fallback").unwrap(),
                   "https://nc.example");
    }

    #[test]
    fn set_sync_state_overwrites_an_existing_value() {
        let db = Database::open_in_memory().unwrap();
        db.set_sync_state("interval_seconds", "1800").unwrap();
        db.set_sync_state("interval_seconds", "300").unwrap();
        assert_eq!(db.get_sync_state("interval_seconds", "0").unwrap(),
                   "300");
    }

    #[test]
    fn sync_state_persists_across_database_reopens() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sync_state.db");
        {
            let db = Database::open(&path).unwrap();
            db.set_sync_state("server_url", "https://nc.example").unwrap();
        }
        let db = Database::open(&path).unwrap();
        assert_eq!(db.get_sync_state("server_url", "x").unwrap(),
                   "https://nc.example");
    }

    #[test]
    fn sync_state_and_settings_are_separate_namespaces() {
        // Same key in both tables must NOT collide — they're conceptually
        // independent stores. Pinning this makes future "let's just merge
        // them" refactors visible in CI.
        let db = Database::open_in_memory().unwrap();
        db.set_setting("foo", "from-settings").unwrap();
        db.set_sync_state("foo", "from-sync-state").unwrap();
        assert_eq!(db.get_setting("foo", "x").unwrap(), "from-settings");
        assert_eq!(db.get_sync_state("foo", "x").unwrap(), "from-sync-state");
    }
}
