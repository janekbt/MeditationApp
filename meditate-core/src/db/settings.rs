//! `settings` table — user-facing preference key/value store. Every
//! write emits a `setting_changed` event so peers converge via the
//! same Lamport-ts precedence rules as the entity rows.

use rusqlite::{params, OptionalExtension};

use super::events::EventKind;
use super::{Database, DbError, Result};

impl Database {
    /// Read the value of a settings key. Returns `default` (without
    /// inserting it) when the key has never been set.
    pub fn get_setting(&self, key: &str, default: &str) -> Result<String> {
        self.read_kv("settings", key, default)
    }

    /// Write a settings value. Upserts: subsequent calls overwrite.
    /// Each call emits its own `setting_changed` event — peers
    /// last-write-wins by Lamport ts, so collapsing two overwrites to
    /// one event would lose the intermediate ordering.
    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        self.write_kv("settings", key, value)?;
        let payload = serde_json::json!({
            "key": key,
            "value": value,
        }).to_string();
        self.emit_event(EventKind::SettingChanged, key, payload)?;
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
                .map_err(|e| DbError::Decode(
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_helpers::*;

    #[test]
    fn get_setting_returns_default_when_key_missing() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(
            db.get_setting("timer_presets", "5,10,15,20,30").unwrap(),
            "5,10,15,20,30",
        );
        // The key remained absent — getting it again returns the same default.
        assert_eq!(
            db.get_setting("timer_presets", "5,10,15,20,30").unwrap(),
            "5,10,15,20,30",
        );
    }

    #[test]
    fn set_setting_then_get_setting_round_trip() {
        let db = Database::open_in_memory().unwrap();
        db.set_setting("timer_presets", "3,7,12").unwrap();
        assert_eq!(
            db.get_setting("timer_presets", "5,10,15,20,30").unwrap(),
            "3,7,12",
        );
    }

    #[test]
    fn set_setting_overwrites_existing_value() {
        let db = Database::open_in_memory().unwrap();
        db.set_setting("daily_goal_mins", "20").unwrap();
        db.set_setting("daily_goal_mins", "25").unwrap();
        assert_eq!(db.get_setting("daily_goal_mins", "0").unwrap(), "25");
    }

    #[test]
    fn settings_keys_are_independent() {
        let db = Database::open_in_memory().unwrap();
        db.set_setting("daily_goal_mins", "20").unwrap();
        assert_eq!(db.get_setting("weekly_goal_mins", "150").unwrap(), "150");
        assert_eq!(db.get_setting("daily_goal_mins", "0").unwrap(), "20");
    }

    #[test]
    fn set_setting_accepts_empty_string_and_unicode() {
        let db = Database::open_in_memory().unwrap();
        db.set_setting("note_template", "").unwrap();
        assert_eq!(db.get_setting("note_template", "fallback").unwrap(), "");
        db.set_setting("greeting", "こんにちは ☀️").unwrap();
        assert_eq!(db.get_setting("greeting", "").unwrap(), "こんにちは ☀️");
    }

    #[test]
    fn set_setting_appends_a_setting_changed_event() {
        let db = Database::open_in_memory().unwrap();
        db.set_setting("daily_goal_minutes", "20").unwrap();
        let events = db.pending_events().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].1.kind, "setting_changed");
        let payload = event_payload(&events[0].1);
        assert_eq!(payload["key"], "daily_goal_minutes");
        assert_eq!(payload["value"], "20");
    }

    #[test]
    fn set_setting_overwrite_emits_a_second_event_with_the_new_value() {
        let db = Database::open_in_memory().unwrap();
        db.set_setting("daily_goal_minutes", "20").unwrap();
        db.set_setting("daily_goal_minutes", "30").unwrap();
        let events = db.pending_events().unwrap();
        assert_eq!(events.len(), 2);
        let last_payload = event_payload(&events[1].1);
        assert_eq!(last_payload["value"], "30");
    }

    #[test]
    fn set_setting_with_unicode_value_round_trips_through_payload() {
        let db = Database::open_in_memory().unwrap();
        db.set_setting("greeting", "🧘 こんにちは").unwrap();
        let payload = event_payload(&db.pending_events().unwrap()[0].1);
        assert_eq!(payload["value"], "🧘 こんにちは");
    }

    #[test]
    fn apply_event_setting_changed_writes_value_into_settings() {
        let db = Database::open_in_memory().unwrap();
        db.apply_event(&synth_setting_changed("daily_goal", "20", 5, DEVICE_A)).unwrap();
        assert_eq!(db.get_setting("daily_goal", "fallback").unwrap(), "20");
    }

    #[test]
    fn apply_event_higher_lamport_setting_overwrites_lower() {
        let db = Database::open_in_memory().unwrap();
        db.apply_event(&synth_setting_changed("daily_goal", "20", 5, DEVICE_A)).unwrap();
        db.apply_event(&synth_setting_changed("daily_goal", "30", 10, DEVICE_A)).unwrap();
        assert_eq!(db.get_setting("daily_goal", "x").unwrap(), "30");
    }

    #[test]
    fn apply_event_out_of_order_settings_converge_correctly() {
        let db = Database::open_in_memory().unwrap();
        db.apply_event(&synth_setting_changed("daily_goal", "30", 10, DEVICE_A)).unwrap();
        db.apply_event(&synth_setting_changed("daily_goal", "20", 5, DEVICE_A)).unwrap();
        assert_eq!(db.get_setting("daily_goal", "x").unwrap(), "30");
    }

    #[test]
    fn apply_event_setting_concurrent_writes_break_ties_on_device_id() {
        let db = Database::open_in_memory().unwrap();
        db.apply_event(&synth_setting_changed("daily_goal", "from A", 5, DEVICE_A)).unwrap();
        db.apply_event(&synth_setting_changed("daily_goal", "from B", 5, DEVICE_B)).unwrap();
        assert_eq!(db.get_setting("daily_goal", "x").unwrap(), "from B");
    }

    #[test]
    fn apply_event_settings_for_different_keys_do_not_collide() {
        let db = Database::open_in_memory().unwrap();
        db.apply_event(&synth_setting_changed("a", "alpha", 5, DEVICE_A)).unwrap();
        db.apply_event(&synth_setting_changed("b", "beta",  6, DEVICE_A)).unwrap();
        assert_eq!(db.get_setting("a", "x").unwrap(), "alpha");
        assert_eq!(db.get_setting("b", "x").unwrap(), "beta");
    }

    #[test]
    fn apply_event_setting_is_idempotent() {
        let db = Database::open_in_memory().unwrap();
        let event = synth_setting_changed("daily_goal", "20", 5, DEVICE_A);
        db.apply_event(&event).unwrap();
        db.apply_event(&event).unwrap();
        assert_eq!(db.get_setting("daily_goal", "x").unwrap(), "20");
    }
}
