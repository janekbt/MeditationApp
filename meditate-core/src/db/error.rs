//! Error types + collision/validation helpers used across the
//! per-entity modules.

#[derive(Debug)]
pub enum DbError {
    DuplicateLabel(String),
    DuplicatePreset(String),
    DuplicateGuidedFile(String),
    DuplicateVibrationPattern(String),
    Sqlite(rusqlite::Error),
    Csv(String),
    /// The on-disk DB was written by a newer build (its
    /// `user_version` exceeds this build's `SCHEMA_VERSION`).
    /// Opening would risk silent corruption.
    SchemaVersionTooNew { db: u32, build: u32 },
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
