//! `presets` table — named, full-fidelity session templates. The
//! shell defines and serialises `config_json`; core only stores +
//! round-trips it through sync events.

use rusqlite::{params, OptionalExtension};

use super::events::EventKind;
use super::{
    map_unique_err, mint_uuid,
    Database, DbError, Result, SessionMode,
};

/// One named, full-fidelity session template. Captures the entire
/// Setup-view state (mode, duration / breath pattern, label, bells,
/// interval-bell snapshot, end bell) under a stable UUID. The shell
/// applies a preset by replaying its `config_json` into the live
/// Setup state. `is_starred` controls whether the preset appears in
/// the visible chip list above the Save / Manage buttons; `mode`
/// is denormalised into a column so the visible-list query can
/// filter without parsing JSON.
///
/// The shape of `config_json` is opaque at this layer — core only
/// stores and round-trips it. The shell defines and serialises the
/// concrete schema, same way `Event::payload` works.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preset {
    pub id: i64,
    pub uuid: super::PresetUuid,
    pub name: String,
    pub mode: SessionMode,
    pub is_starred: bool,
    pub config_json: String,
    pub created_iso: String,
    pub updated_iso: String,
}

/// Every preset, ordered by mode (timer first, then box_breath)
/// then created_iso ASC. Stable order: rows don't shuffle when a
/// star toggles or a config gets overwritten.
pub fn list_presets_from_db(db: &Database) -> Result<Vec<Preset>> {
    let mut stmt = db.conn.prepare(
        "SELECT id, uuid, name, mode, is_starred, config_json, created_iso, updated_iso
         FROM presets
         ORDER BY mode, created_iso ASC, uuid ASC",
    )?;
    let rows = stmt
        .query_map([], |row| {
            let mode_str: String = row.get(3)?;
            Ok(Preset {
                id: row.get(0)?,
                uuid: row.get(1)?,
                name: row.get(2)?,
                mode: SessionMode::from_db_str(&mode_str)
                    .expect("presets.mode violates CHECK constraint"),
                is_starred: row.get::<_, i64>(4)? != 0,
                config_json: row.get(5)?,
                created_iso: row.get(6)?,
                updated_iso: row.get(7)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}
/// Presets for one mode, ordered by created_iso ASC. Used by the
/// chooser pages (Save / Manage), both of which are mode-strict
/// per the design (the user shouldn't accidentally save a Timer
/// config into a Box-Breath preset, or see other-mode presets in
/// the management page).
pub fn list_presets_for_mode_from_db(db: &Database, mode: SessionMode) -> Result<Vec<Preset>> {
    let mut stmt = db.conn.prepare(
        "SELECT id, uuid, name, mode, is_starred, config_json, created_iso, updated_iso
         FROM presets
         WHERE mode = ?1
         ORDER BY created_iso ASC, uuid ASC",
    )?;
    let rows = stmt
        .query_map(params![mode.as_db_str()], |row| {
            let mode_str: String = row.get(3)?;
            Ok(Preset {
                id: row.get(0)?,
                uuid: row.get(1)?,
                name: row.get(2)?,
                mode: SessionMode::from_db_str(&mode_str)
                    .expect("presets.mode violates CHECK constraint"),
                is_starred: row.get::<_, i64>(4)? != 0,
                config_json: row.get(5)?,
                created_iso: row.get(6)?,
                updated_iso: row.get(7)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}
/// Starred presets for one mode, ordered by created_iso ASC.
/// Drives the visible chip list above the Save / Manage buttons
/// in the Setup view. When this list is empty, the chip section
/// hides entirely (just the two buttons remain).
pub fn list_starred_presets_for_mode_from_db(db: &Database, mode: SessionMode) -> Result<Vec<Preset>> {
    let mut stmt = db.conn.prepare(
        "SELECT id, uuid, name, mode, is_starred, config_json, created_iso, updated_iso
         FROM presets
         WHERE mode = ?1 AND is_starred = 1
         ORDER BY created_iso ASC, uuid ASC",
    )?;
    let rows = stmt
        .query_map(params![mode.as_db_str()], |row| {
            let mode_str: String = row.get(3)?;
            Ok(Preset {
                id: row.get(0)?,
                uuid: row.get(1)?,
                name: row.get(2)?,
                mode: SessionMode::from_db_str(&mode_str)
                    .expect("presets.mode violates CHECK constraint"),
                is_starred: row.get::<_, i64>(4)? != 0,
                config_json: row.get(5)?,
                created_iso: row.get(6)?,
                updated_iso: row.get(7)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}
/// True iff any preset other than `except_uuid` already uses
/// `name` (case-insensitive — the column is COLLATE NOCASE).
/// Used by the rename flow's live validation; pass the row's own
/// uuid as `except_uuid` so renaming to its current name (or a
/// case variant) doesn't false-positive.
pub fn is_preset_name_taken_from_db(db: &Database, name: &str, except_uuid: &str) -> Result<bool> {
    let n: i64 = db.conn.query_row(
        "SELECT COUNT(*) FROM presets WHERE name = ?1 AND uuid != ?2",
        params![name, except_uuid],
        |row| row.get(0),
    )?;
    Ok(n > 0)
}
pub fn find_preset_by_uuid_from_db(db: &Database, uuid_str: &str) -> Result<Option<Preset>> {
    let row = db.conn.query_row(
        "SELECT id, uuid, name, mode, is_starred, config_json, created_iso, updated_iso
         FROM presets WHERE uuid = ?1",
        params![uuid_str],
        |row| {
            let mode_str: String = row.get(3)?;
            Ok(Preset {
                id: row.get(0)?,
                uuid: row.get(1)?,
                name: row.get(2)?,
                mode: SessionMode::from_db_str(&mode_str)
                    .expect("presets.mode violates CHECK constraint"),
                is_starred: row.get::<_, i64>(4)? != 0,
                config_json: row.get(5)?,
                created_iso: row.get(6)?,
                updated_iso: row.get(7)?,
            })
        },
    ).optional()?;
    Ok(row)
}
impl Database {
    /// Create a preset under a freshly-minted v4 UUID. Convenience
    /// over `insert_preset_with_uuid` for the user-creates-from-Setup
    /// flow where the shell doesn't need a stable uuid up front.
    pub fn insert_preset(
        &self,
        name: &str,
        mode: SessionMode,
        is_starred: bool,
        config_json: &str,
    ) -> Result<i64> {
        self.insert_preset_with_uuid(
            &mint_uuid(),
            name,
            mode,
            is_starred,
            config_json,
        )
    }

    /// Insert a preset with a caller-supplied uuid. Idempotent on the
    /// uuid: an existing row with this uuid is returned without
    /// inserting or emitting. A duplicate *name* with a different uuid
    /// surfaces `DuplicatePreset` so unrelated callers don't silently
    /// shadow each other's rows. Emits a `preset_insert` event with
    /// the full row payload so a peer that's missed prior events can
    /// still materialise the row from this single message.
    pub fn insert_preset_with_uuid(
        &self,
        uuid_str: &str,
        name: &str,
        mode: SessionMode,
        is_starred: bool,
        config_json: &str,
    ) -> Result<i64> {
        let now_iso = chrono::Utc::now().to_rfc3339();
        self.insert_preset_at(&Preset {
            id: 0,
            uuid: super::PresetUuid::new(uuid_str),
            name: name.to_string(),
            mode,
            is_starred,
            config_json: config_json.to_string(),
            created_iso: now_iso.clone(),
            updated_iso: now_iso,
        })
    }

    /// Undo of a delete: the preset comes back with its uuid and its
    /// creation time, so it takes its old place in the list.
    pub fn restore_preset(&self, preset: &Preset) -> Result<i64> {
        self.insert_preset_at(preset)
    }

    /// Insert `preset` under its uuid and times (its id is ignored).
    fn insert_preset_at(&self, preset: &Preset) -> Result<i64> {
        let Preset { uuid, name, mode, is_starred, config_json, created_iso, updated_iso, .. } = preset;
        let (uuid_str, mode, is_starred) = (uuid.as_str(), *mode, *is_starred);
        let tx = self.conn.unchecked_transaction()?;
        if let Some(existing) = self.existing_rowid_by_uuid("presets", uuid_str)? {
            return Ok(existing);
        }
        self.conn
            .execute(
                "INSERT INTO presets (uuid, name, mode, is_starred, config_json, created_iso, updated_iso)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    uuid_str,
                    name,
                    mode.as_db_str(),
                    i64::from(is_starred),
                    config_json,
                    created_iso,
                    updated_iso,
                ],
            )
            .map_err(|e| map_unique_err(e, || DbError::DuplicatePreset(name.to_string())))?;
        let rowid = self.conn.last_insert_rowid();
        let payload = serde_json::json!({
            "uuid": uuid_str,
            "name": name,
            "mode": mode.as_db_str(),
            "is_starred": is_starred,
            "config_json": config_json,
            "created_iso": created_iso,
            "updated_iso": updated_iso,
        }).to_string();
        self.emit_event(&tx, EventKind::PresetInsert, uuid_str, payload)?;
        tx.commit()?;
        Ok(rowid)
    }







    /// Rename a preset. Unknown uuids are silent no-ops AND emit no
    /// event. If `name` collides with another preset (case-insensitive)
    /// returns `DuplicatePreset` and the transaction rolls back so no
    /// rename event leaks to peers. Renaming to the current name (or a
    /// case variant of itself) is allowed — SQLite's UNIQUE check
    /// excludes the row being updated.
    pub fn update_preset_name(&self, uuid_str: &str, name: &str) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        let stored: Option<String> = self.conn.query_row(
            "SELECT name FROM presets WHERE uuid = ?1",
            params![uuid_str],
            |row| row.get(0),
        ).optional()?;
        // Unknown, or unchanged: a fresh event would undo a rename
        // another device made in the meantime.
        if stored.map_or(true, |stored| stored == name) { return Ok(()); }
        let now_iso = chrono::Utc::now().to_rfc3339();
        self.conn
            .execute(
                "UPDATE presets SET name = ?1, updated_iso = ?2 WHERE uuid = ?3",
                params![name, now_iso, uuid_str],
            )
            .map_err(|e| map_unique_err(e, || DbError::DuplicatePreset(name.to_string())))?;
        let row = find_preset_by_uuid_from_db(self, uuid_str)?
            .expect("just confirmed exists");
        let payload = serde_json::json!({
            "uuid": uuid_str,
            "name": name,
            "mode": row.mode.as_db_str(),
            "is_starred": row.is_starred,
            "config_json": row.config_json,
            "created_iso": row.created_iso,
            "updated_iso": now_iso,
        }).to_string();
        self.emit_event(&tx, EventKind::PresetUpdate, uuid_str, payload)?;
        tx.commit()?;
        Ok(())
    }

    /// Replace the config JSON for a preset (the "Override" path in
    /// Save mode). Unknown uuids are silent no-ops with no event.
    /// Bumps `updated_iso`.
    pub fn update_preset_config(&self, uuid_str: &str, config_json: &str) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        let row = find_preset_by_uuid_from_db(self, uuid_str)?;
        let Some(row) = row else { return Ok(()); };
        let now_iso = chrono::Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE presets SET config_json = ?1, updated_iso = ?2 WHERE uuid = ?3",
            params![config_json, now_iso, uuid_str],
        )?;
        let payload = serde_json::json!({
            "uuid": uuid_str,
            "name": row.name,
            "mode": row.mode.as_db_str(),
            "is_starred": row.is_starred,
            "config_json": config_json,
            "created_iso": row.created_iso,
            "updated_iso": now_iso,
        }).to_string();
        self.emit_event(&tx, EventKind::PresetUpdate, uuid_str, payload)?;
        tx.commit()?;
        Ok(())
    }

    /// Star or unstar a preset. Unknown uuids are silent no-ops with
    /// no event. Bumps `updated_iso` so peers' last-write-wins
    /// resolution converges on the latest toggle.
    pub fn update_preset_starred(&self, uuid_str: &str, starred: crate::db::StarredState) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        let row = find_preset_by_uuid_from_db(self, uuid_str)?;
        let Some(row) = row else { return Ok(()); };
        let now_iso = chrono::Utc::now().to_rfc3339();
        let is_starred = starred.is_starred();
        self.conn.execute(
            "UPDATE presets SET is_starred = ?1, updated_iso = ?2 WHERE uuid = ?3",
            params![i64::from(is_starred), now_iso, uuid_str],
        )?;
        let payload = serde_json::json!({
            "uuid": uuid_str,
            "name": row.name,
            "mode": row.mode.as_db_str(),
            "is_starred": is_starred,
            "config_json": row.config_json,
            "created_iso": row.created_iso,
            "updated_iso": now_iso,
        }).to_string();
        self.emit_event(&tx, EventKind::PresetUpdate, uuid_str, payload)?;
        tx.commit()?;
        Ok(())
    }

    /// Move a preset to `to` in its mode's list (clamped to the end).
    /// The lists sort by `created_iso` as text (older versions too), so
    /// only the moved preset gets a new one, between its new
    /// neighbours: a sync can't touch the presets it passed. Only
    /// neighbours with equal text leave no room; then the mode's
    /// presets get fresh times 1 ms apart from now.
    /// Unknown uuids and a move onto the same place write nothing.
    pub fn move_preset(&self, uuid_str: &str, to: usize) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        let Some(row) = find_preset_by_uuid_from_db(self, uuid_str)? else { return Ok(()); };
        let mut list = list_presets_for_mode_from_db(self, row.mode)?;
        let from = list.iter().position(|p| p.uuid == row.uuid).expect("a preset is listed under its mode");
        let to = to.min(list.len() - 1);
        if from == to { return Ok(()); }
        let moved = list.remove(from);
        list.insert(to, moved);
        let mut times: Vec<String> = list.iter().map(|p| p.created_iso.clone()).collect();
        match key_between(to.checked_sub(1).map(|i| times[i].as_str()), times.get(to + 1).map(String::as_str), uuid_str) {
            Some(key) => times[to] = key,
            None => {
                let start = chrono::Utc::now();
                times = (0..list.len())
                    .map(|i| (start + chrono::Duration::milliseconds(i as i64))
                        .to_rfc3339_opts(chrono::SecondsFormat::Millis, false))
                    .collect();
            }
        }
        let now_iso = chrono::Utc::now().to_rfc3339();
        for (p, created_iso) in list.iter().zip(&times) {
            if &p.created_iso == created_iso { continue; }
            self.conn.execute(
                "UPDATE presets SET created_iso = ?1, updated_iso = ?2 WHERE uuid = ?3",
                params![created_iso, now_iso, p.uuid.as_str()],
            )?;
            let payload = serde_json::json!({
                "uuid": p.uuid.as_str(),
                "name": p.name,
                "mode": p.mode.as_db_str(),
                "is_starred": p.is_starred,
                "config_json": p.config_json,
                "created_iso": created_iso,
                "updated_iso": now_iso,
            }).to_string();
            self.emit_event(&tx, EventKind::PresetUpdate, p.uuid.as_str(), payload)?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Remove a preset row and emit a tombstone. Unknown uuids are
    /// silent no-ops with no event — peers would otherwise receive a
    /// tombstone for a preset they never knew existed.
    pub fn delete_preset(&self, uuid_str: &str) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        let exists: bool = self.conn.query_row(
            "SELECT 1 FROM presets WHERE uuid = ?1",
            params![uuid_str],
            |_| Ok(true),
        ).optional()?.unwrap_or(false);
        if !exists { return Ok(()); }
        self.conn.execute(
            "DELETE FROM presets WHERE uuid = ?1",
            params![uuid_str],
        )?;
        let payload = serde_json::json!({ "uuid": uuid_str }).to_string();
        self.emit_event(&tx, EventKind::PresetDelete, uuid_str, payload)?;
        tx.commit()?;
        Ok(())
    }

    /// Recompute the `presets` row for `preset_uuid` from the events
    /// table. Same precedence rules as labels / interval_bells:
    /// tombstone wins on tie, else the highest-(lamport, device_id)
    /// mutate event drives the row. Update events carry every field
    /// plus created_iso so they self-suffice if the corresponding
    /// insert event hasn't arrived yet (out-of-order delivery).
    pub(super) fn recompute_preset(&self, preset_uuid: &str) -> Result<()> {
        let Some(v) = self.winning_mutate(
            preset_uuid,
            [EventKind::PresetInsert, EventKind::PresetUpdate],
            EventKind::PresetDelete,
        )? else {
            self.conn.execute(
                "DELETE FROM presets WHERE uuid = ?1",
                params![preset_uuid],
            )?;
            return Ok(());
        };
        {
            let name = v["name"].as_str().unwrap_or_default();
            let mode = v["mode"].as_str().unwrap_or("timer");
            let is_starred = v["is_starred"].as_bool().unwrap_or(false);
            let config_json = v["config_json"].as_str().unwrap_or("{}");
            let created_iso = v["created_iso"].as_str().unwrap_or_default();
            let updated_iso = v["updated_iso"].as_str().unwrap_or_default();
            let upsert_sql = "INSERT INTO presets
                    (uuid, name, mode, is_starred, config_json, created_iso, updated_iso)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(uuid) DO UPDATE SET
                    name        = excluded.name,
                    mode        = excluded.mode,
                    is_starred  = excluded.is_starred,
                    config_json = excluded.config_json,
                    created_iso = excluded.created_iso,
                    updated_iso = excluded.updated_iso";
            self.claim_name("presets", preset_uuid, name, |name| self.conn.execute(upsert_sql, params![
                preset_uuid, name, mode, i64::from(is_starred),
                config_json, created_iso, updated_iso,
            ]))?;
        }
        Ok(())
    }
}

// ponytail: moves dropped again and again at the same edge of one gap
// add a digit every 5 to 10 moves (about 160 characters after 1000);
// count in the neighbour's tail, as count_up does, if that ever shows.
/// A `created_iso` that sorts as text strictly between `before` and
/// `after` (`None`: the list's end). Between two neighbours it extends
/// `before` with digits, so there is always room; a tail from the
/// moved preset's uuid keeps two devices' moves into one gap apart.
/// `None` only for neighbours with equal text, or a first preset with
/// no digit above 0.
fn key_between(before: Option<&str>, after: Option<&str>, uuid: &str) -> Option<String> {
    let core = match (before, after) {
        (Some(lo), Some(hi)) if lo < hi => match hi.strip_prefix(lo) {
            Some(rest) => format!("{lo}{}", digits_below(rest)?),
            None => just_above(lo, hi),
        },
        // The first digit above 0 lowered: below `hi` for any text. The
        // 9 after it gives the next moves to the top room to lower.
        (None, Some(hi)) => {
            let j = hi.bytes().position(|b| (b'1'..=b'9').contains(&b))?;
            format!("{}{}9", &hi[..j], char::from(hi.as_bytes()[j] - 1))
        }
        // Now, so a preset saved later still comes after it. With the
        // clock behind `lo`: `lo` without its tail, counted up by one in
        // its last digits, so it stays as long and as early as it is.
        (Some(lo), None) => {
            let now = chrono::Utc::now().to_rfc3339();
            if now.as_str() > lo { now } else { count_up(lo) }
        }
        _ => return None,
    };
    // The last 8 hex digits (the bundled ids share their first ones),
    // each as two digits 1-9: never ending in 0, so digits_below always
    // finds room, and two ids never give one tail.
    let tail: String = uuid.chars().rev().filter_map(|c| c.to_digit(16)).take(8)
        .flat_map(|v| [char::from(b'1' + (v / 9) as u8), char::from(b'1' + (v % 9) as u8)]).collect();
    let key = core + &tail;
    (before.map_or(true, |lo| lo < key.as_str()) && after.map_or(true, |hi| key.as_str() < hi)).then_some(key)
}

/// The shortest start of `lo`, with one digit raised, that sorts above
/// `lo` and below `hi`, where `hi` doesn't start with `lo`; so moves
/// back and forth don't grow the text.
fn just_above(lo: &str, hi: &str) -> String {
    let (l, h) = (lo.as_bytes(), hi.as_bytes());
    let split = l.iter().zip(h).position(|(a, b)| a != b).unwrap_or(l.len());
    (split..l.len())
        .find_map(|j| {
            let digit = l[j].saturating_add(1).max(b'1');
            let start = lo.get(..j).filter(|_| digit <= b'9' && (j > split || digit < h[j]))?;
            Some(format!("{start}{}", char::from(digit)))
        })
        .unwrap_or_else(|| format!("{lo}5"))
}

/// The next text above `lo`: its last digit below 9 raised and the
/// digits after it set to 0. The count runs in a move's 16-digit tail
/// (or a plain time's offset), never in the time before it, so the
/// place stays as early as it is; a second tail on top is dropped
/// first, so the text stays as long. Every tail holds a digit below 9.
fn count_up(lo: &str) -> String {
    let n = lo.len();
    let core = if n >= 32 && lo.as_bytes()[n - 32..].iter().all(u8::is_ascii_digit) { &lo[..n - 16] } else { lo };
    let mut bytes = core.as_bytes().to_vec();
    let Some(j) = bytes.iter().rposition(|b| (b'0'..=b'8').contains(b)) else { return format!("{lo}5"); };
    bytes[j] += 1;
    bytes[j + 1..].iter_mut().filter(|b| b.is_ascii_digit()).for_each(|b| *b = b'0');
    String::from_utf8(bytes).expect("only ASCII digits changed")
}

/// A digit string that sorts below `rest` and is not a start of it;
/// `rest` is digits not ending in 0, as `key_between` writes them.
fn digits_below(rest: &str) -> Option<String> {
    let zeros = rest.bytes().take_while(|&b| b == b'0').count();
    let low = match *rest.as_bytes().get(zeros)? {
        first @ b'2'..=b'9' => char::from(first - 1).to_string(),
        b'1' => "05".into(),
        _ => return None,
    };
    Some(format!("{}{low}", &rest[..zeros]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{test_helpers::*, Event};

    fn insert_basic_preset(db: &Database, name: &str, mode: SessionMode) -> i64 {
        db.insert_preset(
            name,
            mode,
            false,
            r#"{"placeholder":true}"#,
        ).unwrap()
    }

    #[test]
    fn insert_preset_round_trips_through_list() {
        let db = Database::open_in_memory().unwrap();
        let id = db.insert_preset(
            "Sitting",
            SessionMode::Timer,
            true,
            r#"{"duration_secs":900}"#,
        ).unwrap();

        let presets = list_presets_from_db(&db).unwrap();
        assert_eq!(presets.len(), 1);
        let p = &presets[0];
        assert_eq!(p.id, id);
        assert!(!p.uuid.is_empty(), "fresh insert mints a uuid");
        assert_eq!(p.name, "Sitting");
        assert_eq!(p.mode, SessionMode::Timer);
        assert!(p.is_starred);
        assert_eq!(p.config_json, r#"{"duration_secs":900}"#);
        assert!(!p.created_iso.is_empty());
        assert_eq!(p.created_iso, p.updated_iso,
            "fresh insert sets updated_iso = created_iso");
    }

    #[test]
    fn insert_preset_guided_mode_round_trips() {
        let db = Database::open_in_memory().unwrap();
        let id = db.insert_preset(
            "Headspace Track 1",
            SessionMode::Guided,
            true,
            r#"{"guided_file_uuid":"x"}"#,
        ).unwrap();
        let presets = list_presets_from_db(&db).unwrap();
        assert_eq!(presets.len(), 1);
        assert_eq!(presets[0].id, id);
        assert_eq!(presets[0].mode, SessionMode::Guided);
    }

    #[test]
    fn insert_preset_with_uuid_uses_supplied_uuid() {
        let db = Database::open_in_memory().unwrap();
        let _id = db.insert_preset_with_uuid(
            "abc-123",
            "Sitting",
            SessionMode::Timer,
            false,
            r#"{}"#,
        ).unwrap();
        let presets = list_presets_from_db(&db).unwrap();
        assert_eq!(presets[0].uuid, "abc-123");
    }

    #[test]
    fn insert_preset_with_existing_uuid_is_silent_noop() {
        let db = Database::open_in_memory().unwrap();
        let r1 = db.insert_preset_with_uuid(
            "u-1", "Sitting", SessionMode::Timer, false, r#"{}"#,
        ).unwrap();
        let r2 = db.insert_preset_with_uuid(
            "u-1", "Different Name", SessionMode::BoxBreath, true, r#"{"x":1}"#,
        ).unwrap();
        assert_eq!(r1, r2, "second insert returns existing rowid");
        assert_eq!(list_presets_from_db(&db).unwrap().len(), 1);
        // Original values stand — second insert is a pure no-op.
        let p = &list_presets_from_db(&db).unwrap()[0];
        assert_eq!(p.name, "Sitting");
        assert_eq!(p.mode, SessionMode::Timer);
        assert!(!p.is_starred);
    }

    #[test]
    fn insert_preset_duplicate_name_returns_duplicate_preset() {
        let db = Database::open_in_memory().unwrap();
        db.insert_preset("Sitting", SessionMode::Timer, false, r#"{}"#).unwrap();
        let r = db.insert_preset(
            "sitting",  // case-insensitive collision
            SessionMode::BoxBreath,
            false,
            r#"{}"#,
        );
        assert!(
            matches!(r, Err(DbError::DuplicatePreset(ref n)) if n == "sitting"),
            "expected DuplicatePreset, got {r:?}",
        );
        assert_eq!(list_presets_from_db(&db).unwrap().len(), 1, "row count unchanged");
    }

    #[test]
    fn insert_preset_emits_a_preset_insert_event() {
        let db = Database::open_in_memory().unwrap();
        db.insert_preset("Sitting", SessionMode::Timer, true, r#"{"dur":900}"#).unwrap();
        let events = db.pending_events().unwrap();
        let insert: Vec<_> = events.iter()
            .filter(|(_, e)| e.kind == "preset_insert")
            .collect();
        assert_eq!(insert.len(), 1);
        let payload: serde_json::Value =
            serde_json::from_str(&insert[0].1.payload).unwrap();
        assert_eq!(payload["name"], "Sitting");
        assert_eq!(payload["mode"], "timer");
        assert_eq!(payload["is_starred"], true);
        assert_eq!(payload["config_json"], r#"{"dur":900}"#);
    }

    #[test]
    fn list_presets_orders_by_mode_then_created_iso() {
        let db = Database::open_in_memory().unwrap();
        // Interleave insert order and modes; verify the SQL ORDER BY
        // groups by mode and orders by created_iso within each group.
        insert_basic_preset(&db, "BB-1", SessionMode::BoxBreath);
        std::thread::sleep(std::time::Duration::from_millis(2));
        insert_basic_preset(&db, "T-1", SessionMode::Timer);
        std::thread::sleep(std::time::Duration::from_millis(2));
        insert_basic_preset(&db, "BB-2", SessionMode::BoxBreath);
        std::thread::sleep(std::time::Duration::from_millis(2));
        insert_basic_preset(&db, "T-2", SessionMode::Timer);

        let names: Vec<String> = list_presets_from_db(&db).unwrap()
            .into_iter().map(|p| p.name).collect();
        // Mode ordering (timer < box_breath alphabetically as DB string —
        // 'box_breath' < 'timer' actually). Verify whichever way SQL sees it.
        // What matters is that within each mode block, created_iso is ASC.
        let bb_pos: Vec<usize> = names.iter().enumerate()
            .filter(|(_, n)| n.starts_with("BB-"))
            .map(|(i, _)| i).collect();
        let t_pos: Vec<usize> = names.iter().enumerate()
            .filter(|(_, n)| n.starts_with("T-"))
            .map(|(i, _)| i).collect();
        assert_eq!(bb_pos.len(), 2);
        assert_eq!(t_pos.len(), 2);
        // Within each mode, ordering follows insert order.
        assert!(bb_pos[0] < bb_pos[1]);
        assert!(t_pos[0] < t_pos[1]);
        assert_eq!(names[bb_pos[0]], "BB-1");
        assert_eq!(names[bb_pos[1]], "BB-2");
        assert_eq!(names[t_pos[0]], "T-1");
        assert_eq!(names[t_pos[1]], "T-2");
    }

    #[test]
    fn list_presets_for_mode_filters_correctly() {
        let db = Database::open_in_memory().unwrap();
        insert_basic_preset(&db, "T-1", SessionMode::Timer);
        insert_basic_preset(&db, "BB-1", SessionMode::BoxBreath);
        insert_basic_preset(&db, "T-2", SessionMode::Timer);

        let timers: Vec<String> = list_presets_for_mode_from_db(&db, SessionMode::Timer)
            .unwrap().into_iter().map(|p| p.name).collect();
        assert_eq!(timers, vec!["T-1", "T-2"]);

        let breaths: Vec<String> = list_presets_for_mode_from_db(&db, SessionMode::BoxBreath)
            .unwrap().into_iter().map(|p| p.name).collect();
        assert_eq!(breaths, vec!["BB-1"]);
    }

    #[test]
    fn list_starred_presets_for_mode_returns_only_starred_in_mode() {
        let db = Database::open_in_memory().unwrap();
        // Mix: Timer starred + unstarred; BoxBreath starred + unstarred.
        db.insert_preset("T-star", SessionMode::Timer, true, r#"{}"#).unwrap();
        db.insert_preset("T-no", SessionMode::Timer, false, r#"{}"#).unwrap();
        db.insert_preset("BB-star", SessionMode::BoxBreath, true, r#"{}"#).unwrap();
        db.insert_preset("BB-no", SessionMode::BoxBreath, false, r#"{}"#).unwrap();

        let timer_starred: Vec<String> =
            list_starred_presets_for_mode_from_db(&db, SessionMode::Timer).unwrap()
                .into_iter().map(|p| p.name).collect();
        assert_eq!(timer_starred, vec!["T-star"]);

        let breath_starred: Vec<String> =
            list_starred_presets_for_mode_from_db(&db, SessionMode::BoxBreath).unwrap()
                .into_iter().map(|p| p.name).collect();
        assert_eq!(breath_starred, vec!["BB-star"]);
    }

    #[test]
    fn is_preset_name_taken_excludes_self_uuid() {
        let db = Database::open_in_memory().unwrap();
        db.insert_preset_with_uuid(
            "u-1", "Sitting", SessionMode::Timer, false, r#"{}"#,
        ).unwrap();
        db.insert_preset_with_uuid(
            "u-2", "Walking", SessionMode::Timer, false, r#"{}"#,
        ).unwrap();
        // Collision with another preset's name.
        assert!(is_preset_name_taken_from_db(&db, "Walking", "u-1").unwrap());
        // Renaming to own current name — case-insensitive — is allowed.
        assert!(!is_preset_name_taken_from_db(&db, "sitting", "u-1").unwrap());
        // Brand-new name is fine.
        assert!(!is_preset_name_taken_from_db(&db, "Sleeping", "u-1").unwrap());
    }

    #[test]
    fn find_preset_by_uuid_returns_some_when_present() {
        let db = Database::open_in_memory().unwrap();
        db.insert_preset_with_uuid(
            "u-1", "Sitting", SessionMode::Timer, true, r#"{"x":1}"#,
        ).unwrap();
        let p = find_preset_by_uuid_from_db(&db, "u-1").unwrap().unwrap();
        assert_eq!(p.name, "Sitting");
        assert!(p.is_starred);
        assert_eq!(p.config_json, r#"{"x":1}"#);
    }

    #[test]
    fn find_preset_by_uuid_returns_none_when_absent() {
        let db = Database::open_in_memory().unwrap();
        assert!(find_preset_by_uuid_from_db(&db, "missing").unwrap().is_none());
    }

    #[test]
    fn update_preset_name_changes_the_name_and_bumps_updated_iso() {
        let db = Database::open_in_memory().unwrap();
        db.insert_preset_with_uuid(
            "u-1", "Sitting", SessionMode::Timer, false, r#"{}"#,
        ).unwrap();
        let before = find_preset_by_uuid_from_db(&db, "u-1").unwrap().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        db.update_preset_name("u-1", "Morning Sit").unwrap();
        let after = find_preset_by_uuid_from_db(&db, "u-1").unwrap().unwrap();
        assert_eq!(after.name, "Morning Sit");
        assert_eq!(after.created_iso, before.created_iso);
        assert!(after.updated_iso > before.updated_iso);
    }

    #[test]
    fn update_preset_name_duplicate_returns_error_and_rolls_back() {
        let db = Database::open_in_memory().unwrap();
        db.insert_preset_with_uuid(
            "u-1", "Sitting", SessionMode::Timer, false, r#"{}"#,
        ).unwrap();
        db.insert_preset_with_uuid(
            "u-2", "Walking", SessionMode::Timer, false, r#"{}"#,
        ).unwrap();
        let result = db.update_preset_name("u-1", "Walking");
        assert!(matches!(result, Err(DbError::DuplicatePreset(_))));
        // u-1 still has its original name.
        let p = find_preset_by_uuid_from_db(&db, "u-1").unwrap().unwrap();
        assert_eq!(p.name, "Sitting");
    }

    #[test]
    fn update_preset_name_unknown_uuid_is_silent_noop() {
        let db = Database::open_in_memory().unwrap();
        db.update_preset_name("missing", "Foo").unwrap();
        let events: Vec<_> = db.pending_events().unwrap()
            .into_iter()
            .filter(|(_, e)| e.kind == "preset_update")
            .collect();
        assert!(events.is_empty(), "unknown uuid emits no event");
    }

    #[test]
    fn update_preset_config_replaces_the_config_blob() {
        let db = Database::open_in_memory().unwrap();
        db.insert_preset_with_uuid(
            "u-1", "Sitting", SessionMode::Timer, false, r#"{"old":1}"#,
        ).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        db.update_preset_config("u-1", r#"{"new":2}"#).unwrap();
        let p = find_preset_by_uuid_from_db(&db, "u-1").unwrap().unwrap();
        assert_eq!(p.config_json, r#"{"new":2}"#);
    }

    #[test]
    fn update_preset_config_emits_one_preset_update_event() {
        let db = Database::open_in_memory().unwrap();
        db.insert_preset_with_uuid(
            "u-1", "Sitting", SessionMode::Timer, false, r#"{}"#,
        ).unwrap();
        db.update_preset_config("u-1", r#"{"x":1}"#).unwrap();
        let updates: Vec<_> = db.pending_events().unwrap()
            .into_iter()
            .filter(|(_, e)| e.kind == "preset_update")
            .collect();
        assert_eq!(updates.len(), 1);
    }

    #[test]
    fn update_preset_starred_toggles_the_flag() {
        let db = Database::open_in_memory().unwrap();
        db.insert_preset_with_uuid(
            "u-1", "Sitting", SessionMode::Timer, false, r#"{}"#,
        ).unwrap();
        db.update_preset_starred("u-1", crate::db::StarredState::Starred).unwrap();
        assert!(find_preset_by_uuid_from_db(&db, "u-1").unwrap().unwrap().is_starred);
        db.update_preset_starred("u-1", crate::db::StarredState::Unstarred).unwrap();
        assert!(!find_preset_by_uuid_from_db(&db, "u-1").unwrap().unwrap().is_starred);
    }

    #[test]
    fn delete_preset_removes_the_row_and_emits_tombstone() {
        let db = Database::open_in_memory().unwrap();
        db.insert_preset_with_uuid(
            "u-1", "Sitting", SessionMode::Timer, false, r#"{}"#,
        ).unwrap();
        db.delete_preset("u-1").unwrap();
        assert!(find_preset_by_uuid_from_db(&db, "u-1").unwrap().is_none());
        let tombstones: Vec<_> = db.pending_events().unwrap()
            .into_iter()
            .filter(|(_, e)| e.kind == "preset_delete")
            .collect();
        assert_eq!(tombstones.len(), 1);
    }

    #[test]
    fn delete_preset_unknown_uuid_emits_no_event() {
        let db = Database::open_in_memory().unwrap();
        db.delete_preset("never-existed").unwrap();
        let tombstones: Vec<_> = db.pending_events().unwrap()
            .into_iter()
            .filter(|(_, e)| e.kind == "preset_delete")
            .collect();
        assert!(tombstones.is_empty());
    }

    // ── Presets — sync replay (apply_event / replay_events) ───────────

    #[test]
    fn apply_event_preset_insert_creates_the_row_on_a_fresh_peer() {
        let peer = Database::open_in_memory().unwrap();
        let payload = serde_json::json!({
            "uuid": "u-1",
            "name": "Sitting",
            "mode": "timer",
            "is_starred": true,
            "config_json": "{\"dur\":900}",
            "created_iso": "2026-05-04T10:00:00Z",
            "updated_iso": "2026-05-04T10:00:00Z",
        });
        peer.apply_event(&synth_event("preset_insert", "u-1", 1, "dev-a", payload))
            .unwrap();
        let p = find_preset_by_uuid_from_db(&peer, "u-1").unwrap()
            .expect("row materialised on peer");
        assert_eq!(p.name, "Sitting");
        assert_eq!(p.mode, SessionMode::Timer);
        assert!(p.is_starred);
        assert_eq!(p.config_json, r#"{"dur":900}"#);
    }

    #[test]
    fn apply_event_preset_update_overwrites_fields_after_insert() {
        let peer = Database::open_in_memory().unwrap();
        peer.apply_event(&synth_event("preset_insert", "u-1", 1, "dev-a",
            serde_json::json!({
                "uuid": "u-1", "name": "Sitting", "mode": "timer",
                "is_starred": false, "config_json": "{\"v\":1}",
                "created_iso": "2026-05-04T10:00:00Z",
                "updated_iso": "2026-05-04T10:00:00Z",
            }))).unwrap();
        peer.apply_event(&synth_event("preset_update", "u-1", 5, "dev-a",
            serde_json::json!({
                "uuid": "u-1", "name": "Morning Sit", "mode": "timer",
                "is_starred": true, "config_json": "{\"v\":2}",
                "created_iso": "2026-05-04T10:00:00Z",
                "updated_iso": "2026-05-04T10:05:00Z",
            }))).unwrap();
        let p = find_preset_by_uuid_from_db(&peer, "u-1").unwrap().unwrap();
        assert_eq!(p.name, "Morning Sit");
        assert!(p.is_starred);
        assert_eq!(p.config_json, r#"{"v":2}"#);
    }

    #[test]
    fn apply_event_preset_delete_removes_the_row() {
        let peer = Database::open_in_memory().unwrap();
        peer.apply_event(&synth_event("preset_insert", "u-1", 1, "dev-a",
            serde_json::json!({
                "uuid": "u-1", "name": "Sitting", "mode": "timer",
                "is_starred": false, "config_json": "{}",
                "created_iso": "x", "updated_iso": "x",
            }))).unwrap();
        peer.apply_event(&synth_event("preset_delete", "u-1", 5, "dev-a",
            serde_json::json!({"uuid": "u-1"}))).unwrap();
        assert!(find_preset_by_uuid_from_db(&peer, "u-1").unwrap().is_none());
    }

    #[test]
    fn apply_event_preset_tombstone_resists_lower_lamport_insert() {
        // Out-of-order delivery: delete arrives first (ts=10), then a
        // late insert event with ts=5. Tombstone wins, row stays absent.
        let peer = Database::open_in_memory().unwrap();
        peer.apply_event(&synth_event("preset_delete", "u-1", 10, "dev-a",
            serde_json::json!({"uuid": "u-1"}))).unwrap();
        peer.apply_event(&synth_event("preset_insert", "u-1", 5, "dev-a",
            serde_json::json!({
                "uuid": "u-1", "name": "Sitting", "mode": "timer",
                "is_starred": false, "config_json": "{}",
                "created_iso": "x", "updated_iso": "x",
            }))).unwrap();
        assert!(find_preset_by_uuid_from_db(&peer, "u-1").unwrap().is_none(),
            "tombstone with higher ts wins over later-applied lower-ts insert");
    }

    #[test]
    fn replay_events_round_trips_a_preset_through_create_rename_delete() {
        // Full lifecycle on device A, replayed on device B from the
        // event log alone.
        let dev_a = Database::open_in_memory().unwrap();
        dev_a.insert_preset_with_uuid(
            "u-1", "Sitting", SessionMode::Timer, true, r#"{"dur":900}"#,
        ).unwrap();
        dev_a.update_preset_name("u-1", "Morning Sit").unwrap();
        dev_a.update_preset_starred("u-1", crate::db::StarredState::Unstarred).unwrap();

        let events: Vec<Event> = dev_a.pending_events().unwrap()
            .into_iter().map(|(_, e)| e).collect();

        let dev_b = Database::open_in_memory().unwrap();
        dev_b.replay_events(&events).unwrap();
        let p = find_preset_by_uuid_from_db(&dev_b, "u-1").unwrap()
            .expect("device B materialised the preset from events alone");
        assert_eq!(p.name, "Morning Sit");
        assert!(!p.is_starred);
        assert_eq!(p.config_json, r#"{"dur":900}"#);
    }

    #[test]
    fn replay_events_with_delete_at_the_end_leaves_the_row_absent() {
        let dev_a = Database::open_in_memory().unwrap();
        dev_a.insert_preset_with_uuid(
            "u-1", "Sitting", SessionMode::Timer, false, r#"{}"#,
        ).unwrap();
        dev_a.delete_preset("u-1").unwrap();
        let events: Vec<Event> = dev_a.pending_events().unwrap()
            .into_iter().map(|(_, e)| e).collect();

        let dev_b = Database::open_in_memory().unwrap();
        dev_b.replay_events(&events).unwrap();
        assert!(find_preset_by_uuid_from_db(&dev_b, "u-1").unwrap().is_none());
    }

    const SMALL: &str = "11111111-0000-4000-8000-000000000001";
    const LARGE: &str = "99999999-0000-4000-8000-000000000009";

    /// Same rule as labels: the larger uuid of a same-name pair takes
    /// the suffix, whichever arrives first.
    #[test]
    fn every_device_suffixes_the_same_preset_of_a_same_name_pair() {
        for (mine, theirs) in [(SMALL, LARGE), (LARGE, SMALL)] {
            let peer = Database::open_in_memory().unwrap();
            peer.insert_preset_with_uuid(theirs, "Morning", SessionMode::Timer, false, "{}").unwrap();
            let events: Vec<Event> = peer.pending_events().unwrap().into_iter()
                .map(|(_, e)| e).filter(|e| e.target_id == theirs).collect();
            let db = Database::open_in_memory().unwrap();
            db.insert_preset_with_uuid(mine, "Morning", SessionMode::Timer, false, "{}").unwrap();
            db.replay_events(&events).unwrap();
            assert_eq!(find_preset_by_uuid_from_db(&db, SMALL).unwrap().unwrap().name, "Morning");
            assert_eq!(find_preset_by_uuid_from_db(&db, LARGE).unwrap().unwrap().name, "Morning (conflict-99999999)");
        }
    }

    #[test]
    fn renaming_a_preset_to_its_own_name_writes_nothing() {
        let db = Database::open_in_memory().unwrap();
        db.insert_preset_with_uuid(SMALL, "Morning", SessionMode::Timer, false, "{}").unwrap();
        let before = db.pending_events().unwrap().len();
        db.update_preset_name(SMALL, "Morning").unwrap();
        assert_eq!(db.pending_events().unwrap().len(), before);
    }

    /// Undo after a delete put the preset at the end of its chips.
    #[test]
    fn an_undone_delete_puts_the_preset_back_in_its_place() {
        let db = Database::open_in_memory().unwrap();
        for name in ["Morning", "Evening", "Walk"] {
            insert_basic_preset(&db, name, SessionMode::Timer);
        }
        let names = |db: &Database| -> Vec<String> {
            list_presets_from_db(db).unwrap().into_iter().map(|p| p.name).collect()
        };
        let before = names(&db);
        let morning = list_presets_from_db(&db).unwrap().into_iter().find(|p| p.name == "Morning").unwrap();
        db.delete_preset(morning.uuid.as_str()).unwrap();
        db.restore_preset(&morning).unwrap();
        assert_eq!(names(&db), before);
    }

    fn order(db: &Database, mode: SessionMode) -> Vec<String> {
        list_presets_for_mode_from_db(db, mode).unwrap().into_iter().map(|p| p.name).collect()
    }

    fn uuid_of(db: &Database, name: &str) -> String {
        list_presets_from_db(db).unwrap().into_iter().find(|p| p.name == name).unwrap().uuid.0
    }

    fn timer_presets(db: &Database, names: &[&str]) {
        for name in names {
            db.insert_preset(name, SessionMode::Timer, true, "{}").unwrap();
        }
    }

    fn sync_both(one: &Database, two: &Database) {
        swap_events(one, two);
        swap_events(two, one);
    }

    fn longest_place(db: &Database) -> usize {
        list_presets_from_db(db).unwrap().into_iter().map(|p| p.created_iso.len()).max().unwrap()
    }

    fn swap_events(from: &Database, to: &Database) {
        let events: Vec<Event> = from.pending_events().unwrap().into_iter().map(|(_, e)| e).collect();
        to.replay_events(&events).unwrap();
    }

    #[test]
    fn a_moved_preset_takes_its_new_place() {
        let db = Database::open_in_memory().unwrap();
        timer_presets(&db, &["A"]);
        db.insert_preset("X", SessionMode::BoxBreath, true, "{}").unwrap();
        timer_presets(&db, &["B"]);
        db.insert_preset("Y", SessionMode::BoxBreath, true, "{}").unwrap();
        timer_presets(&db, &["C"]);
        db.move_preset(&uuid_of(&db, "C"), 0).unwrap();
        assert_eq!(order(&db, SessionMode::Timer), ["C", "A", "B"]);
        let starred: Vec<String> = list_starred_presets_for_mode_from_db(&db, SessionMode::Timer).unwrap().into_iter().map(|p| p.name).collect();
        assert_eq!(starred, ["C", "A", "B"]);
        assert_eq!(order(&db, SessionMode::BoxBreath), ["X", "Y"]);
    }

    #[test]
    fn a_preset_moved_down_lands_below_the_others() {
        let db = Database::open_in_memory().unwrap();
        timer_presets(&db, &["A", "B", "C"]);
        db.move_preset(&uuid_of(&db, "A"), 2).unwrap();
        assert_eq!(order(&db, SessionMode::Timer), ["B", "C", "A"]);
    }

    #[test]
    fn a_preset_dropped_on_its_place_writes_nothing() {
        let db = Database::open_in_memory().unwrap();
        timer_presets(&db, &["A", "B", "C"]);
        let before = db.pending_events().unwrap().len();
        db.move_preset(&uuid_of(&db, "B"), 1).unwrap();
        db.move_preset(&uuid_of(&db, "C"), 99).unwrap();
        db.move_preset("not-a-preset", 0).unwrap();
        assert_eq!(db.pending_events().unwrap().len(), before);
        assert_eq!(order(&db, SessionMode::Timer), ["A", "B", "C"]);
    }

    #[test]
    fn another_device_gets_the_new_order() {
        let one = Database::open_in_memory().unwrap();
        let two = Database::open_in_memory().unwrap();
        timer_presets(&one, &["A", "B", "C"]);
        one.move_preset(&uuid_of(&one, "C"), 0).unwrap();
        swap_events(&one, &two);
        assert_eq!(order(&two, SessionMode::Timer), ["C", "A", "B"]);
    }

    #[test]
    fn an_edit_elsewhere_to_a_preset_not_moved_keeps_the_order() {
        let one = Database::open_in_memory().unwrap();
        let two = Database::open_in_memory().unwrap();
        timer_presets(&one, &["A", "B", "C", "D"]);
        swap_events(&one, &two);
        one.move_preset(&uuid_of(&one, "B"), 0).unwrap();
        two.update_preset_name(&uuid_of(&two, "D"), "D2").unwrap();
        sync_both(&one, &two);
        assert_eq!(order(&one, SessionMode::Timer), ["B", "A", "C", "D2"]);
        assert_eq!(order(&two, SessionMode::Timer), ["B", "A", "C", "D2"]);
    }

    #[test]
    fn presets_with_the_same_time_can_still_be_moved() {
        let db = Database::open_in_memory().unwrap();
        timer_presets(&db, &["A", "B", "C", "D"]);
        db.conn.execute("UPDATE presets SET created_iso = '2026-01-01T00:00:00+00:00'", []).unwrap();
        let b = order(&db, SessionMode::Timer);
        db.move_preset(&uuid_of(&db, &b[3]), 1).unwrap();
        assert_eq!(order(&db, SessionMode::Timer), [b[0].clone(), b[3].clone(), b[1].clone(), b[2].clone()]);
    }

    #[test]
    fn an_edit_elsewhere_to_a_preset_the_move_passes_is_kept() {
        let one = Database::open_in_memory().unwrap();
        let two = Database::open_in_memory().unwrap();
        timer_presets(&one, &["A", "B", "C"]);
        swap_events(&one, &two);
        two.update_preset_name(&uuid_of(&two, "B"), "B2").unwrap();
        two.delete_preset(&uuid_of(&two, "A")).unwrap();
        // The phone has written more since, so its move is the later write.
        for starred in [false, true, false, true] {
            one.update_preset_starred(&uuid_of(&one, "C"), crate::db::StarredState::from_flag(starred)).unwrap();
        }
        one.move_preset(&uuid_of(&one, "C"), 0).unwrap();
        sync_both(&one, &two);
        assert_eq!(order(&one, SessionMode::Timer), ["C", "B2"]);
        assert_eq!(order(&two, SessionMode::Timer), ["C", "B2"]);
    }

    #[test]
    fn a_move_past_the_end_lands_last() {
        let db = Database::open_in_memory().unwrap();
        timer_presets(&db, &["A", "B", "C"]);
        db.move_preset(&uuid_of(&db, "A"), 99).unwrap();
        assert_eq!(order(&db, SessionMode::Timer), ["B", "C", "A"]);
    }

    #[test]
    fn two_devices_moving_before_they_sync_end_in_one_order() {
        let one = Database::open_in_memory().unwrap();
        let two = Database::open_in_memory().unwrap();
        timer_presets(&one, &["A", "B", "C", "D"]);
        swap_events(&one, &two);
        one.move_preset(&uuid_of(&one, "D"), 1).unwrap();
        two.move_preset(&uuid_of(&two, "C"), 1).unwrap();
        sync_both(&one, &two);
        let settled = order(&one, SessionMode::Timer);
        assert_eq!(order(&two, SessionMode::Timer), settled);
        assert_eq!((settled[0].as_str(), settled[3].as_str()), ("A", "B"));
    }

    #[test]
    fn many_moves_into_one_gap_write_only_the_moved_preset() {
        let db = Database::open_in_memory().unwrap();
        timer_presets(&db, &["A", "B", "C"]);
        for _ in 0..60 {
            let last = order(&db, SessionMode::Timer)[2].clone();
            let before = db.pending_events().unwrap().len();
            db.move_preset(&uuid_of(&db, &last), 1).unwrap();
            assert_eq!(db.pending_events().unwrap().len(), before + 1);
            assert_eq!(order(&db, SessionMode::Timer)[1], last);
        }
        // To the top, above a preset that was itself moved there.
        for _ in 0..2 {
            let last = order(&db, SessionMode::Timer)[2].clone();
            let before = db.pending_events().unwrap().len();
            db.move_preset(&uuid_of(&db, &last), 0).unwrap();
            assert_eq!(db.pending_events().unwrap().len(), before + 1);
            assert_eq!(order(&db, SessionMode::Timer)[0], last);
        }
    }

    #[test]
    fn moves_into_one_gap_on_two_devices_leave_room_between_them() {
        let one = Database::open_in_memory().unwrap();
        let two = Database::open_in_memory().unwrap();
        timer_presets(&one, &["A", "B", "C", "D"]);
        swap_events(&one, &two);
        one.move_preset(&uuid_of(&one, "D"), 1).unwrap();
        two.move_preset(&uuid_of(&two, "C"), 1).unwrap();
        sync_both(&one, &two);
        two.delete_preset(&uuid_of(&two, "A")).unwrap();
        for starred in [false, true, false, true] {
            one.update_preset_starred(&uuid_of(&one, "B"), crate::db::StarredState::from_flag(starred)).unwrap();
        }
        one.move_preset(&uuid_of(&one, "B"), 2).unwrap();
        sync_both(&one, &two);
        let settled = order(&one, SessionMode::Timer);
        assert_eq!(order(&two, SessionMode::Timer), settled);
        assert_eq!(settled.len(), 3, "A stays deleted: {settled:?}");
        assert_eq!(settled[1], "B");
    }

    #[test]
    fn a_preset_moved_to_the_end_stays_above_one_saved_later() {
        let db = Database::open_in_memory().unwrap();
        timer_presets(&db, &["A", "B"]);
        db.move_preset(&uuid_of(&db, "A"), 1).unwrap();
        timer_presets(&db, &["C"]);
        assert_eq!(order(&db, SessionMode::Timer), ["B", "A", "C"]);
    }

    #[test]
    fn moves_of_two_presets_into_one_gap_never_share_a_place() {
        let one = Database::open_in_memory().unwrap();
        let two = Database::open_in_memory().unwrap();
        timer_presets(&one, &["A", "B"]);
        // Ids whose first characters a lossy mapping would merge.
        one.insert_preset_with_uuid("00000000-0000-4000-8000-000000000001", "C", SessionMode::Timer, true, "{}").unwrap();
        one.insert_preset_with_uuid("99999999-0000-4000-8000-000000000002", "D", SessionMode::Timer, true, "{}").unwrap();
        swap_events(&one, &two);
        one.move_preset(&uuid_of(&one, "D"), 1).unwrap();
        two.move_preset(&uuid_of(&two, "C"), 1).unwrap();
        sync_both(&one, &two);
        let settled = order(&one, SessionMode::Timer);
        let before = one.pending_events().unwrap().len();
        one.move_preset(&uuid_of(&one, "B"), 2).unwrap();
        assert_eq!(one.pending_events().unwrap().len(), before + 1, "{settled:?}");
        assert_eq!(order(&one, SessionMode::Timer)[2], "B");
    }

    #[test]
    fn swapping_the_top_two_again_and_again_keeps_the_place_short() {
        let db = Database::open_in_memory().unwrap();
        timer_presets(&db, &["A", "B", "C"]);
        for _ in 0..200 {
            let first = order(&db, SessionMode::Timer)[0].clone();
            db.move_preset(&uuid_of(&db, &first), 1).unwrap();
        }
        assert!(longest_place(&db) < 120, "{}", longest_place(&db));
        for _ in 0..200 {
            let last = order(&db, SessionMode::Timer)[2].clone();
            db.move_preset(&uuid_of(&db, &last), 0).unwrap();
        }
        assert!(longest_place(&db) < 120, "to the top: {}", longest_place(&db));
    }

    #[test]
    fn a_move_to_the_top_after_moves_in_the_middle_writes_only_the_moved_preset() {
        let db = Database::open_in_memory().unwrap();
        timer_presets(&db, &["A", "B", "C"]);
        for (name, to) in [("C", 1), ("A", 2), ("B", 0)] {
            let before = db.pending_events().unwrap().len();
            db.move_preset(&uuid_of(&db, name), to).unwrap();
            assert_eq!(db.pending_events().unwrap().len(), before + 1, "{name}");
        }
        assert_eq!(order(&db, SessionMode::Timer), ["B", "C", "A"]);
    }

    /// Random moves on two devices that sync now and then: every move
    /// lands where it was dropped and writes only the moved preset, the
    /// places stay short, and both devices end in one order.
    #[test]
    fn random_moves_write_only_the_moved_preset() {
        let devices = [Database::open_in_memory().unwrap(), Database::open_in_memory().unwrap()];
        timer_presets(&devices[0], &["A", "B", "C", "D", "E", "F"]);
        swap_events(&devices[0], &devices[1]);
        let mut state = 7_u64;
        let mut pick = |n: usize| { let (unit, next) = crate::bells::xorshift64(state); state = next; (unit * n as f64) as usize };
        for step in 0..600 {
            let db = &devices[pick(2)];
            let mut expected = order(db, SessionMode::Timer);
            let (from, to) = (pick(6), pick(6));
            let name = expected.remove(from);
            expected.insert(to, name.clone());
            let before = db.pending_events().unwrap().len();
            db.move_preset(&uuid_of(db, &name), to).unwrap();
            assert_eq!(order(db, SessionMode::Timer), expected, "step {step}");
            assert_eq!(db.pending_events().unwrap().len(), before + usize::from(from != to), "step {step}");
            if step % 50 == 49 {
                sync_both(&devices[0], &devices[1]);
                assert_eq!(order(&devices[0], SessionMode::Timer), order(&devices[1], SessionMode::Timer), "step {step}");
            }
        }
        assert!(longest_place(&devices[0]) < 120, "{}", longest_place(&devices[0]));
    }

    #[test]
    fn the_bundled_box_breath_presets_moved_into_one_gap_on_two_devices_leave_room() {
        let one = Database::open_in_memory().unwrap();
        let two = Database::open_in_memory().unwrap();
        for name in ["P", "Q"] {
            one.insert_preset(name, SessionMode::BoxBreath, true, "{}").unwrap();
        }
        // The bundled ids differ only in their last digits.
        one.insert_preset_with_uuid(crate::seeds::DEFAULT_BOX_BREATH_4444_UUID, "4-4-4-4", SessionMode::BoxBreath, true, "{}").unwrap();
        one.insert_preset_with_uuid(crate::seeds::DEFAULT_BOX_BREATH_4780_UUID, "4-7-8-0", SessionMode::BoxBreath, true, "{}").unwrap();
        swap_events(&one, &two);
        one.move_preset(crate::seeds::DEFAULT_BOX_BREATH_4444_UUID, 1).unwrap();
        two.move_preset(crate::seeds::DEFAULT_BOX_BREATH_4780_UUID, 1).unwrap();
        sync_both(&one, &two);
        let before = one.pending_events().unwrap().len();
        one.move_preset(&uuid_of(&one, "Q"), 2).unwrap();
        assert_eq!(one.pending_events().unwrap().len(), before + 1);
        assert_eq!(order(&one, SessionMode::BoxBreath)[2], "Q");
    }

    #[test]
    fn swapping_the_last_two_while_the_clock_is_behind_keeps_the_place_short() {
        // Written by a device whose clock runs far ahead of this one: a
        // plain time, and a place a move between two presets cut short.
        for (start, before_next) in [
            ("2999-01-01T00:00:00+00:00", "2999-01-01T00:00:01"),
            ("2999-01-01T011234567812345678", "2999-01-01T02"),
        ] {
            let db = Database::open_in_memory().unwrap();
            timer_presets(&db, &["A", "B", "C"]);
            db.conn.execute("UPDATE presets SET created_iso = ?1 WHERE name = 'C'", [start]).unwrap();
            for _ in 0..200 {
                let mut expected = order(&db, SessionMode::Timer);
                let second = expected.remove(1);
                expected.push(second.clone());
                let before = db.pending_events().unwrap().len();
                db.move_preset(&uuid_of(&db, &second), 2).unwrap();
                assert_eq!(order(&db, SessionMode::Timer), expected);
                assert_eq!(db.pending_events().unwrap().len(), before + 1);
            }
            let places: Vec<String> = list_presets_from_db(&db).unwrap().into_iter().map(|p| p.created_iso).collect();
            assert!(places.iter().all(|p| p.len() < 120), "{places:?}");
            // The place counts up after the other device's, not hours ahead.
            assert!(places.iter().all(|p| p.as_str() < before_next), "{start}: {places:?}");
        }
    }
}
