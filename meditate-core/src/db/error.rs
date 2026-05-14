//! Error types + collision/validation helpers used across the
//! per-entity modules.

#[derive(Debug)]
pub enum DbError {
    DuplicateLabel(String),
    DuplicatePreset(String),
    DuplicateGuidedFile(String),
    DuplicateVibrationPattern(String),
    Sqlite(rusqlite::Error),
    /// A structural data problem at a boundary between SQLite and
    /// Rust types — bad JSON in a `recompute_*` payload, malformed
    /// CSV row in `import_sessions_csv`, serde serialize/deserialize
    /// failure on a vibration-pattern intensities array, etc. The
    /// `String` is a human-readable description (originating from
    /// the underlying parser / serializer) intended for diag-log
    /// and developer-facing toast, not for translation.
    Decode(String),
    /// The on-disk DB was written by a newer build (its
    /// `user_version` exceeds this build's `SCHEMA_VERSION`).
    /// Opening would risk silent corruption.
    SchemaVersionTooNew { db: u32, build: u32 },
    /// A date computation walked past `chrono::NaiveDate::MAX` or
    /// `MIN` (year ±262144). Reachable via `import_csv` admitting a
    /// row with `start_iso` at the calendar boundary — without this
    /// variant the streak/aggregation code would panic via
    /// `succ_opt().expect()` / `pred_opt().expect()`.
    DateOutOfRange,
}

impl From<rusqlite::Error> for DbError {
    fn from(e: rusqlite::Error) -> Self {
        DbError::Sqlite(e)
    }
}

pub type Result<T> = std::result::Result<T, DbError>;

/// Compose a name that disambiguates a sync-merge collision between
/// two rows holding the same `name` value. Appends the row's uuid
/// short-prefix so the suffix is deterministic across replays (the
/// same event always produces the same name) and unique (UUID
/// collisions are negligible). English-only marker for now;
/// translatable when the cache-conflict UI sweep ships.
pub(super) fn conflict_suffixed_name(name: &str, uuid: &str) -> String {
    let short = uuid.chars().take(8).collect::<String>();
    format!("{name} (conflict-{short})")
}

/// Whether a rusqlite error is the UNIQUE-constraint failure shape
/// that our cache UPSERTs hit on a sync-merge name collision.
pub(super) fn is_unique_constraint_error(e: &rusqlite::Error) -> bool {
    matches!(
        e,
        rusqlite::Error::SqliteFailure(err, _)
            if err.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE
    )
}

/// Map a rusqlite error into a `DbError`, calling `on_unique` to
/// supply the typed duplicate variant when the error is a UNIQUE-
/// constraint failure. Collapses the recurring match shape
///
/// ```text
/// match res {
///     Err(rusqlite::Error::SqliteFailure(err, _))
///         if err.extended_code == SQLITE_CONSTRAINT_UNIQUE =>
///             Err(DbError::DuplicateLabel(name.to_string())),
///     Err(e) => Err(DbError::Sqlite(e)),
/// }
/// ```
///
/// into `Err(map_unique_err(e, || DbError::DuplicateLabel(name.into())))`.
/// `on_unique` is `FnOnce` so callers don't pay the allocation for
/// the duplicate variant's `String` body unless the lookup actually
/// hit.
pub(super) fn map_unique_err(
    e: rusqlite::Error,
    on_unique: impl FnOnce() -> DbError,
) -> DbError {
    if is_unique_constraint_error(&e) {
        on_unique()
    } else {
        DbError::Sqlite(e)
    }
}

/// Whether `target_id` is safe to use as part of a filesystem path
/// component for the given event `kind`. The attack this defends
/// against is a peer-authored `bell_sound_insert` whose target_id is
/// e.g. `"../../../etc/passwd"` — without rejection, the value lands
/// in `bell_sounds.uuid` and later gets interpolated into
/// `sounds_dir.join(format!("{uuid}.{ext}"))` by
/// `pull_custom_sound_files`, yielding a write outside the sounds
/// directory.
///
/// Policy: reject any target_id that contains a path separator,
/// null byte, or is empty. `box_breath_phase_update` additionally
/// demands one of the four canonical phase strings (its `target_id`
/// is enum-shaped). Unknown kinds pass through — the dispatch in
/// `apply_event_inner` records but does not act on them, so an
/// over-strict validator here would block future entity types.
pub fn target_id_is_well_formed_for(kind: &str, target_id: &str) -> bool {
    if target_id.is_empty()
        || target_id.contains('/')
        || target_id.contains('\\')
        || target_id.contains('\0')
    {
        return false;
    }
    if kind == "box_breath_phase_update" {
        return matches!(target_id, "in" | "holdin" | "out" | "holdout");
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{Database, Event};

    #[test]
    fn target_id_validator_accepts_uuid_and_short_identifiers() {
        // The validator is path-traversal-focused, not UUID-strict —
        // legitimate non-UUID identifiers (short test IDs, opaque
        // keys) pass through unless they carry path separators.
        for kind in [
            "session_insert", "label_rename", "interval_bell_update",
            "bell_sound_insert", "preset_delete", "guided_file_insert",
            "vibration_pattern_update",
        ] {
            assert!(target_id_is_well_formed_for(
                kind, "550e8400-e29b-41d4-a716-446655440000"
            ));
            assert!(target_id_is_well_formed_for(kind, "bell-1"));
            assert!(target_id_is_well_formed_for(kind, "u-1"));
        }
    }

    #[test]
    fn target_id_validator_rejects_path_traversal() {
        // These are the strings that would let a peer write outside
        // sounds_dir when the value is later interpolated into a
        // filename component.
        for bad in [
            "../../../etc/passwd",
            "../sibling.wav",
            "/absolute/path",
            "with/slash",
            "back\\slash",
            "null\0byte",
            "",
        ] {
            assert!(
                !target_id_is_well_formed_for("bell_sound_insert", bad),
                "expected reject for target_id={bad:?}"
            );
        }
    }

    #[test]
    fn target_id_validator_accepts_phase_strings_for_box_breath() {
        for phase in ["in", "holdin", "out", "holdout"] {
            assert!(target_id_is_well_formed_for("box_breath_phase_update", phase));
        }
    }

    #[test]
    fn target_id_validator_rejects_unknown_phase_or_traversal_for_box_breath() {
        for bad in ["inhale", "", "../etc", "IN"] {
            assert!(
                !target_id_is_well_formed_for("box_breath_phase_update", bad),
                "expected reject for box_breath target_id={bad:?}"
            );
        }
    }

    #[test]
    fn target_id_validator_passes_unknown_kinds_through() {
        // Forward-compat: a future entity type's event would be
        // recorded-not-applied; the validator must not over-block.
        assert!(target_id_is_well_formed_for("future_kind", "anything"));
        // ...except path-traversal, which is universal.
        assert!(!target_id_is_well_formed_for("future_kind", "../etc"));
        assert!(!target_id_is_well_formed_for("future_kind", ""));
    }

    #[test]
    fn apply_event_inner_skips_dispatch_on_invalid_target_id() {
        // Peer ships a bell_sound_insert with target_id that's a path-
        // traversal string. The event row records, but no bell_sounds
        // row materialises — preventing the downstream file-write
        // primitive in pull_custom_sound_files.
        let db = Database::open_in_memory().unwrap();
        let device_id = "peer-device".to_string();
        let evil = "../../../etc/passwd";
        let payload = serde_json::json!({
            "uuid": evil,
            "name": "Trojan",
            "file_path": "/p/x.wav",
            "is_bundled": false,
            "mime_type": "audio/wav",
            "category": "general",
            "created_iso": "2026-05-11T00:00:00",
        }).to_string();
        let event = Event {
            event_uuid: uuid::Uuid::new_v4().to_string(),
            lamport_ts: 1,
            device_id,
            kind: "bell_sound_insert".to_string(),
            target_id: evil.to_string(),
            payload,
        };
        db.apply_event(&event).unwrap();
        // Event row recorded for forward-compat, but no row in the
        // bell_sounds cache — the harm is the dispatch.
        let bells = db.list_bell_sounds().unwrap();
        assert!(
            bells.iter().all(|b| b.uuid != evil),
            "evil target_id must NOT land in bell_sounds.uuid"
        );
    }
}
