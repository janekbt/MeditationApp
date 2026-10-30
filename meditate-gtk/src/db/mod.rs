//! GTK-side `Database` — a thin wrapper around `meditate_core::Database`.
//!
//! All persistence logic lives in core. This module owns:
//! - The GTK-app's domain types (`Session`, `Label`, `SessionData`)
//!   which use i64-unix timestamps and the `note` field name for
//!   ergonomic UI integration. `SessionMode` is re-exported from
//!   core directly — no separate enum.
//! - Type translation at the API boundary — the canonical
//!   unix↔ISO conversion lives in core via `Session::from_unix` /
//!   `Session::start_unix`; this module just renames `note` ↔ `notes`
//!   and threads the optional `id` through `session_from_core`.
//! - The `rusqlite::Result<T>` return type so callers can keep using `?`
//!   against this module without learning core's `DbError`.
//!
//! Core's schema is the on-disk reality; the existing app's old schema
//! is gone (one user, opted into a fresh DB).

use gtk::prelude::*;
use std::path::Path;

use crate::i18n::gettext;

/// Shell-side error type returned by every wrapper method on the
/// `Database` struct. Mirrors core's `meditate_core::db::DbError`
/// shape, preserving the typed `Duplicate*` variants so the four
/// "name already taken" toasts can render a real user-facing
/// message instead of a synthesized rusqlite `UNIQUE constraint
/// failed` string. `Other` wraps a genuine `rusqlite::Error` for
/// the long tail of SQL surface that doesn't need typed handling.
#[derive(Debug)]
pub enum DbError {
    DuplicateLabel(String),
    DuplicatePreset(String),
    DuplicateGuidedFile(String),
    DuplicateVibrationPattern(String),
    SchemaVersionTooNew { db: u32, build: u32 },
    DateOutOfRange,
    Decode(String),
    Other(rusqlite::Error),
}

impl std::fmt::Display for DbError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DuplicateLabel(name) => write!(f, "duplicate label: {name}"),
            Self::DuplicatePreset(name) => write!(f, "duplicate preset: {name}"),
            Self::DuplicateGuidedFile(name) => write!(f, "duplicate guided file: {name}"),
            Self::DuplicateVibrationPattern(name) =>
                write!(f, "duplicate vibration pattern: {name}"),
            Self::SchemaVersionTooNew { db, build } =>
                write!(f, "db schema_version={db} exceeds build schema_version={build}"),
            Self::DateOutOfRange => write!(f, "date out of range"),
            Self::Decode(s) => write!(f, "decode error: {s}"),
            Self::Other(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for DbError {}

impl From<rusqlite::Error> for DbError {
    fn from(e: rusqlite::Error) -> Self { Self::Other(e) }
}

impl DbError {
    /// Localized "«{name}» is already taken" toast text for the four
    /// duplicate variants. `None` for non-duplicate errors so the
    /// caller can decide whether to show a generic toast or stay
    /// silent. Phrasing is deliberately short — Librem 5 toast width
    /// truncates around 30 chars, and longer copy is what made the
    /// previous developer-language `UNIQUE constraint failed`
    /// rendering useless to users.
    pub fn duplicate_toast_text(&self) -> Option<String> {
        let name = match self {
            DbError::DuplicateLabel(n)
            | DbError::DuplicatePreset(n)
            | DbError::DuplicateGuidedFile(n)
            | DbError::DuplicateVibrationPattern(n) => n,
            _ => return None,
        };
        Some(gettext("«{name}» is already taken").replace("{name}", name))
    }
}

pub type Result<T> = std::result::Result<T, DbError>;

/// Which message a failed write shows.
#[derive(Clone, Copy)]
pub enum Failed {
    Save,
    Delete,
}

/// `true` when the write landed. Otherwise logs it under `tag` and
/// toasts "Save failed" or "Delete failed" (or "«X» is already taken"
/// for a name clash) on the toast overlay nearest to `anchor`. No database at
/// all counts as a failure.
pub fn write_ok<T, E: std::fmt::Debug + 'static>(
    anchor: &impl gtk::glib::object::IsA<gtk::Widget>,
    tag: &str,
    result: Option<std::result::Result<T, E>>,
    failed: Failed,
) -> bool {
    let err = match result {
        Some(Ok(_)) => return true,
        Some(Err(e)) => e,
        None => {
            meditate_core::log(tag, "failed: no database");
            show_write_failure(anchor, None, failed);
            return false;
        }
    };
    meditate_core::log(tag, &format!("failed: {err:?}"));
    show_write_failure(anchor, clash_text(&err), failed);
    false
}

/// "«X» is already taken" when the write failed on a name clash, from
/// this app's or core's error type.
fn clash_text(err: &dyn std::any::Any) -> Option<String> {
    use meditate_core::db::DbError as Core;
    if let Some(e) = err.downcast_ref::<DbError>() {
        return e.duplicate_toast_text();
    }
    let ours = match err.downcast_ref::<Core>()? {
        Core::DuplicateLabel(n) => DbError::DuplicateLabel(n.clone()),
        Core::DuplicatePreset(n) => DbError::DuplicatePreset(n.clone()),
        Core::DuplicateGuidedFile(n) => DbError::DuplicateGuidedFile(n.clone()),
        Core::DuplicateVibrationPattern(n) => DbError::DuplicateVibrationPattern(n.clone()),
        _ => return None,
    };
    ours.duplicate_toast_text()
}

fn show_write_failure(anchor: &impl gtk::glib::object::IsA<gtk::Widget>, text: Option<String>, failed: Failed) {
    let text = text.unwrap_or_else(|| match failed {
        Failed::Save => gettext("Save failed"),
        Failed::Delete => gettext("Delete failed"),
    });
    show_toast(anchor, text);
}

/// A four-second toast on `anchor`'s page, replacing the last one.
pub fn show_toast(anchor: &impl gtk::glib::object::IsA<gtk::Widget>, text: String) {
    // A plain toast title is one line, cut with "…" on a narrow window.
    let title = gtk::Label::builder().label(text).wrap(true).xalign(0.0).css_classes(["heading"]).build();
    let toast = adw::Toast::builder().custom_title(&title).timeout(4).build();
    // One at a time: a slider step that fails again replaces the last
    // one instead of queueing four seconds each.
    thread_local! {
        static LAST: std::cell::RefCell<Option<adw::Toast>> = const { std::cell::RefCell::new(None) };
    }
    if let Some(last) = LAST.replace(Some(toast.clone())) {
        last.dismiss();
    }
    // A pushed page has its own overlay; the window's sits on the root page.
    if let Some(overlay) = anchor.ancestor(adw::ToastOverlay::static_type()).and_downcast::<adw::ToastOverlay>() {
        overlay.add_toast(toast);
    } else if let Some(window) = anchor.root().and_downcast::<crate::window::MeditateWindow>() {
        window.add_toast(toast);
    }
}

// ── Models ────────────────────────────────────────────────────────────────────

/// `SessionMode` and `Label` are canonical core types — re-exported
/// here so call sites can keep importing from `crate::db` without
/// learning about meditate-core directly. Same for the bell-library
/// types added in B.3.1, and the `mint_uuid` helper added in B.5.
pub use meditate_core::db::{
    mint_uuid, BellSound, BellSoundCategory, BoxBreathPhase, BoxBreathPhaseId, ChartKind, GuidedFile, IntervalBell, IntervalBellKind, Label, Preset, SessionMode, SignalMode, VibrationPattern,
};

/// Bundled bell-sound seed: hardcoded (uuid, display name, GResource
/// path, MIME). UUIDs are STABLE across versions — never edit a row
/// here. New bundles get appended (never replace) so a peer that
/// already seeded the old set picks up the new ones via insert-or-
/// ignore. Adding a sound is a 1-tuple addition; no migration code.
/// The GResource path of a bundled bell this build ships, by uuid.
/// Bundled audio is always found here, never through the row's
/// stored `file_path` (sync can leave another platform's path there).
pub fn bundled_bell_resource(uuid: &str) -> Option<&'static str> {
    BUNDLED_BELL_SOUNDS
        .iter()
        .find(|(u, ..)| *u == uuid)
        .map(|(_, _, path, _)| *path)
}

const BUNDLED_BELL_SOUNDS: &[(&str, &str, &str, &str)] = &[
    (
        "f0c2e8a1-3a72-4d4f-9c8b-1b0e5d8c0001",
        "Singing Bowl",
        "/io/github/janekbt/Meditate/sounds/bowl.ogg",
        "audio/ogg",
    ),
    (
        "f0c2e8a1-3a72-4d4f-9c8b-1b0e5d8c0002",
        "Bell",
        "/io/github/janekbt/Meditate/sounds/bell.ogg",
        "audio/ogg",
    ),
    (
        "f0c2e8a1-3a72-4d4f-9c8b-1b0e5d8c0003",
        "Gong",
        "/io/github/janekbt/Meditate/sounds/gong.ogg",
        "audio/ogg",
    ),
    // ── B.* expansion: curated meditation-bell library. All OGG/Vorbis,
    // mono 48 kHz, EBU R128 loudness-normalised to -16 LUFS so the
    // 0.3 s woodblock click and the 30 s bonshō tail land at comparable
    // perceived volume. Sources + license attribution in
    // data/sounds/CREDITS.md.
    (
        "f0c2e8a1-3a72-4d4f-9c8b-1b0e5d8c0004",
        "Tibetan Singing Bowl",
        "/io/github/janekbt/Meditate/sounds/tibetan-bowl-medium.ogg",
        "audio/ogg",
    ),
    (
        "f0c2e8a1-3a72-4d4f-9c8b-1b0e5d8c0005",
        "Inkin",
        "/io/github/janekbt/Meditate/sounds/inkin.ogg",
        "audio/ogg",
    ),
    (
        "f0c2e8a1-3a72-4d4f-9c8b-1b0e5d8c0006",
        "Tingsha",
        "/io/github/janekbt/Meditate/sounds/tingsha.ogg",
        "audio/ogg",
    ),
    (
        "f0c2e8a1-3a72-4d4f-9c8b-1b0e5d8c0007",
        "Kanshō",
        "/io/github/janekbt/Meditate/sounds/kansho.ogg",
        "audio/ogg",
    ),
    (
        "f0c2e8a1-3a72-4d4f-9c8b-1b0e5d8c0008",
        "Burmese Brass Bell",
        "/io/github/janekbt/Meditate/sounds/burmese-brass.ogg",
        "audio/ogg",
    ),
    (
        "f0c2e8a1-3a72-4d4f-9c8b-1b0e5d8c0009",
        "Chau Gong",
        "/io/github/janekbt/Meditate/sounds/chau-gong.ogg",
        "audio/ogg",
    ),
    (
        "f0c2e8a1-3a72-4d4f-9c8b-1b0e5d8c000a",
        "Crystal Bowl",
        "/io/github/janekbt/Meditate/sounds/crystal-bowl.ogg",
        "audio/ogg",
    ),
    (
        "f0c2e8a1-3a72-4d4f-9c8b-1b0e5d8c000b",
        "Woodblock",
        "/io/github/janekbt/Meditate/sounds/woodblock.ogg",
        "audio/ogg",
    ),
];

// Bundled UUIDs + portable seed lists live in core so every shell
// agrees on canonical row identity. Re-exported here so existing
// callers continue importing them from `crate::db::*` unchanged.
pub use meditate_core::seeds::{
    BELLS_SEEDED_KEY,
    BUNDLED_BELL_UUID,
    BUNDLED_BOWL_UUID,
    BUNDLED_WOODBLOCK_UUID,
    BUNDLED_GONG_UUID,
    BUNDLED_PATTERN_HEARTBEAT_UUID,
    BUNDLED_PATTERN_PULSE_UUID,
    BUNDLED_PATTERN_PYRAMID_UUID,
    BUNDLED_PATTERN_RIPPLE_UUID,
    BUNDLED_PATTERN_WAVE_UUID,
    DEFAULT_BOX_BREATH_4444_UUID,
    DEFAULT_BOX_BREATH_4780_UUID,
    DEFAULT_BREATHING_LABEL_UUID,
    DEFAULT_GUIDED_LABEL_UUID,
    DEFAULT_LABELS,
    DEFAULT_SITTING_PRESET_UUID,
    DEFAULT_TIMER_LABEL_UUID,
};

#[derive(Debug, Clone)]
pub struct Session {
    pub id: i64,
    /// Unix timestamp (seconds since epoch) of when the session started.
    pub start_time: i64,
    pub duration_secs: i64,
    pub mode: SessionMode,
    pub label_id: Option<i64>,
    pub note: Option<String>,
    /// Set on guided meditation rows that played a library-stored
    /// file. None for non-Guided modes and transient one-off plays.
    pub guided_file_uuid: Option<String>,
    /// The played file's title, for the Log card (any guided session).
    pub guided_file_name: Option<String>,
}

/// Parameters for creating or updating a session.
pub struct SessionData {
    pub start_time: i64,
    pub duration_secs: i64,
    pub mode: SessionMode,
    pub label_id: Option<i64>,
    pub note: Option<String>,
    pub guided_file_uuid: Option<String>,
    pub guided_file_name: Option<String>,
}

/// `SessionFilter` is the canonical core type — re-exported here so
/// call sites can import from `crate::db` without learning about
/// meditate-core directly.
pub use meditate_core::db::SessionFilter;

// ── Translation: GTK-side ↔ meditate_core::db ─────────────────────────────────

/// Build core's `Session` from this shell's `SessionData`. The
/// scalar→ISO + clamp logic lives on `meditate_core::db::Session`
/// via `from_unix` so the Android shell can share the same canonical
/// translation; this helper is just a thin field-name renamer
/// (`note` → `notes`).
fn session_data_to_core(s: &SessionData) -> meditate_core::db::Session {
    meditate_core::db::Session::from_unix(
        s.start_time,
        s.duration_secs,
        s.label_id,
        s.note.clone(),
        s.mode,
        s.guided_file_uuid.clone().map(meditate_core::db::GuidedFileUuid::new),
        s.guided_file_name.clone(),
    )
}

/// Inverse of `session_data_to_core` for retrievals: takes core's
/// `(id, Session)` shape and produces the GTK-side `Session` with
/// embedded id and i64-unix `start_time`. Reads `start_unix()` off
/// core so the ISO-parse logic is shared with future shells.
fn session_from_core(id: i64, core: &meditate_core::db::Session) -> Session {
    Session {
        id,
        start_time: core.start_unix(),
        duration_secs: i64::from(core.duration_secs),
        mode: core.mode,
        label_id: core.label_id,
        note: core.notes.clone(),
        guided_file_uuid: core.guided_file_uuid.as_ref().map(|u| u.0.clone()),
        guided_file_name: core.guided_file_name.clone(),
    }
}

/// Map core's structured error to the shell's `DbError`. Typed
/// `Duplicate*` variants carry the offending name straight through
/// so the create / rename dialogs render a real "«{name}» is
/// already taken" toast rather than the developer-language UNIQUE-
/// constraint string they used to.
fn map_core_err(e: meditate_core::db::DbError) -> DbError {
    use meditate_core::db::DbError as Core;
    match e {
        Core::Sqlite(err) => DbError::Other(err),
        Core::DuplicateLabel(name) => DbError::DuplicateLabel(name),
        Core::DuplicatePreset(name) => DbError::DuplicatePreset(name),
        Core::DuplicateGuidedFile(name) => DbError::DuplicateGuidedFile(name),
        Core::DuplicateVibrationPattern(name) => DbError::DuplicateVibrationPattern(name),
        Core::Decode(s) => DbError::Decode(s),
        Core::SchemaVersionTooNew { db, build } => DbError::SchemaVersionTooNew { db, build },
        Core::DateOutOfRange => DbError::DateOutOfRange,
    }
}

/// Today as a `chrono::NaiveDate` in the user's local timezone — used
/// for streak / running-average calculations that need a concrete
/// "today" boundary. Thin re-export of the core helper so call sites
/// don't all spell out `meditate_core::time::today_local`.
fn today_local_naive_date() -> chrono::NaiveDate {
    meditate_core::time::today_local()
}

// ── Database ──────────────────────────────────────────────────────────────────

pub struct Database {
    inner: meditate_core::Database,
}

impl std::fmt::Debug for Database {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Database").finish_non_exhaustive()
    }
}

impl Database {
    /// Borrow the underlying core handle. Used by sibling modules in
    /// the gtk crate (e.g. `data_io`) that delegate work to functions
    /// in `meditate_core::*` taking a `&meditate_core::Database`.
    /// `pub(crate)` rather than `pub` so the inner handle never
    /// escapes the crate's wrapper layer.
    pub(crate) fn core(&self) -> &meditate_core::Database {
        &self.inner
    }

    /// Open (or create) the database at `path`. Schema is core's; any
    /// pre-existing DB file written by an older version of this app
    /// will need to be deleted first (the user opted into that on
    /// 2026-04-29 — single-user repo).
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                DbError::Decode(format!("mkdir {}: {e}", parent.display()))
            })?;
        }
        let inner = meditate_core::Database::open(path).map_err(map_core_err)?;
        let db = Self { inner };
        // Bell sounds stay shell-side: the seed list carries gresource
        // paths that don't exist on Android.
        db.seed_bundled_bell_sounds()?;
        db.inner.seed_all_non_audio().map_err(map_core_err)?;
        Ok(db)
    }

    /// Insert the bundled bell-sound rows on first run, gated by a
    /// one-shot `bundled_bell_sounds_seeded` settings flag so a user
    /// who deletes a bundled row can't have it resurrect on the next
    /// open. (Without the flag, a deletion frees the seed UUID,
    /// `insert_bell_sound_with_uuid` re-INSERTs and emits a fresh
    /// `bell_sound_insert` event with a newer lamport ts, which on
    /// sync overrides the user's `bell_sound_delete` everywhere.)
    /// UUIDs are still hardcoded so every device — fresh seed or
    /// post-sync — converges on the same row identity per file.
    fn seed_bundled_bell_sounds(&self) -> Result<()> {
        // Orchestration (idempotency gate, ordering, event emission)
        // lives in core; only the gtk-specific row table (GResource
        // URIs as file_path) stays here. Bundled rows ship as
        // `general` — bells, gongs, chimes for the Starting /
        // Interval / End bell choosers. Box Breath voice cues land
        // via separate seeds when those ship (TODO entry: "Source
        // bundled Box Breath voice-cue sounds").
        let _ = BellSoundCategory::General;
        self.inner
            .seed_bell_sounds_with_paths(BUNDLED_BELL_SOUNDS)
            .map_err(map_core_err)
    }

    // ── Labels ────────────────────────────────────────────────────────────────

    /// Insert a label with the exact name supplied. Returns a UNIQUE
    /// constraint error on collision (case-insensitive — column is
    /// COLLATE NOCASE). The UI is expected to pre-validate via
    /// `is_label_name_taken` so this error is a safety net, not the
    /// primary collision path.
    pub fn create_label(&self, name: &str) -> Result<Label> {
        let id = self.inner.insert_label(name).map_err(map_core_err)?;
        // Look the row back up so we return the DB-assigned uuid rather
        // than synthesising a half-populated Label. `list_labels` is the
        // only existing read path; it's O(n) over labels but n stays
        // small (~tens) and label creation isn't a hot path.
        meditate_core::db::list_labels_from_db(&self.inner)
            .map_err(map_core_err)?
            .into_iter()
            .find(|l| l.id == id)
            .ok_or(DbError::Other(rusqlite::Error::QueryReturnedNoRows))
    }

    pub fn list_labels(&self) -> Result<Vec<Label>> {
        meditate_core::db::list_labels_from_db(&self.inner).map_err(map_core_err)
    }

    /// True iff any label other than `except_id` already uses `name`
    /// (case-insensitive — the column is COLLATE NOCASE).
    pub fn is_label_name_taken(&self, name: &str, except_id: i64) -> Result<bool> {
        meditate_core::db::is_label_name_taken_from_db(&self.inner, name, except_id)
            .map_err(map_core_err)
    }

    pub fn update_label(&self, id: i64, name: &str) -> Result<()> {
        self.inner.update_label(id, name).map_err(map_core_err)
    }

    pub fn label_session_count(&self, id: i64) -> Result<i64> {
        meditate_core::db::label_session_count_from_db(&self.inner, id).map_err(map_core_err)
    }

    pub fn delete_label(&self, id: i64) -> Result<()> {
        self.inner.delete_label(id).map_err(map_core_err)
    }

    // ── Interval bells ────────────────────────────────────────────────────────
    // Thin pass-throughs onto core's CRUD. Domain types are re-exported
    // from core verbatim — no shell-side translation needed.

    pub fn list_interval_bells(&self) -> Result<Vec<meditate_core::IntervalBell>> {
        self.inner.list_interval_bells().map_err(map_core_err)
    }

    pub fn find_interval_bell_by_id(
        &self, rowid: i64,
    ) -> Result<Option<meditate_core::IntervalBell>> {
        self.inner.find_interval_bell_by_id(rowid).map_err(map_core_err)
    }

    pub fn insert_interval_bell(
        &self,
        kind: meditate_core::db::IntervalBellKind,
        time: meditate_core::bells::BellTime,
        jitter: meditate_core::bells::BellTime,
        step: meditate_core::bells::BellTime,
        sound: &str,
        vibration_pattern_uuid: &str,
        signal_mode: meditate_core::SignalMode,
    ) -> Result<i64> {
        self.inner
            .insert_interval_bell(
                kind, time, jitter, step, sound,
                vibration_pattern_uuid, signal_mode,
            )
            .map_err(map_core_err)
    }

    pub fn edit_interval_bell(
        &self,
        bell: &meditate_core::IntervalBell,
    ) -> Result<meditate_core::preset_config::BellEdit> {
        meditate_core::preset_config::edit_interval_bell(&self.inner, bell).map_err(map_core_err)
    }

    pub fn set_interval_bell_enabled(&self, uuid: &str, enabled: bool) -> Result<()> {
        self.inner
            .set_interval_bell_enabled(uuid, enabled)
            .map_err(map_core_err)
    }

    pub fn delete_interval_bell(&self, uuid: &str) -> Result<()> {
        self.inner.delete_interval_bell(uuid).map_err(map_core_err)
    }

    // ── Bell sounds ───────────────────────────────────────────────────────────
    // Audio-file library shared by every bell-fire site. Pass-through
    // wrappers; domain types are re-exported from core.

    pub fn list_bell_sounds(&self) -> Result<Vec<BellSound>> {
        self.inner.list_bell_sounds().map_err(map_core_err)
    }

    pub fn list_bell_sounds_for_category(
        &self,
        category: BellSoundCategory,
    ) -> Result<Vec<BellSound>> {
        self.inner.list_bell_sounds_for_category(category).map_err(map_core_err)
    }

    pub fn insert_bell_sound(
        &self,
        name: &str,
        file_path: &str,
        is_bundled: bool,
        mime_type: &str,
        category: BellSoundCategory,
    ) -> Result<i64> {
        self.inner
            .insert_bell_sound(name, file_path, is_bundled, mime_type, category)
            .map_err(map_core_err)
    }

    pub fn insert_bell_sound_with_uuid(
        &self,
        uuid: &str,
        name: &str,
        file_path: &str,
        is_bundled: bool,
        mime_type: &str,
        category: BellSoundCategory,
    ) -> Result<i64> {
        self.inner
            .insert_bell_sound_with_uuid(uuid, name, file_path, is_bundled, mime_type, category)
            .map_err(map_core_err)
    }

    pub fn rename_bell_sound(&self, uuid: &str, name: &str) -> Result<()> {
        self.inner.rename_bell_sound(uuid, name).map_err(map_core_err)
    }

    pub fn delete_bell_sound(&self, uuid: &str) -> Result<()> {
        self.inner.delete_bell_sound(uuid).map_err(map_core_err)
    }

    // ── Presets ───────────────────────────────────────────────────────────────
    // Thin pass-throughs onto core's CRUD. The chooser UI (P.4) reaches
    // for these via the GTK shell's Database wrapper.

    pub fn list_presets(&self) -> Result<Vec<Preset>> {
        meditate_core::db::list_presets_from_db(&self.inner).map_err(map_core_err)
    }

    pub fn list_presets_for_mode(&self, mode: SessionMode) -> Result<Vec<Preset>> {
        meditate_core::db::list_presets_for_mode_from_db(&self.inner, mode).map_err(map_core_err)
    }

    pub fn list_starred_presets_for_mode(&self, mode: SessionMode) -> Result<Vec<Preset>> {
        meditate_core::db::list_starred_presets_for_mode_from_db(&self.inner, mode).map_err(map_core_err)
    }

    pub fn insert_preset(
        &self,
        name: &str,
        mode: SessionMode,
        is_starred: bool,
        config_json: &str,
    ) -> Result<i64> {
        self.inner
            .insert_preset(name, mode, is_starred, config_json)
            .map_err(map_core_err)
    }


    pub fn is_preset_name_taken(&self, name: &str, except_uuid: &str) -> Result<bool> {
        meditate_core::db::is_preset_name_taken_from_db(&self.inner, name, except_uuid).map_err(map_core_err)
    }

    pub fn find_preset_by_uuid(&self, uuid: &str) -> Result<Option<Preset>> {
        meditate_core::db::find_preset_by_uuid_from_db(&self.inner, uuid).map_err(map_core_err)
    }

    pub fn update_preset_name(&self, uuid: &str, name: &str) -> Result<()> {
        self.inner.update_preset_name(uuid, name).map_err(map_core_err)
    }

    pub fn update_preset_config(&self, uuid: &str, config_json: &str) -> Result<()> {
        self.inner.update_preset_config(uuid, config_json).map_err(map_core_err)
    }

    pub fn update_preset_starred(
        &self,
        uuid: &str,
        starred: meditate_core::db::StarredState,
    ) -> Result<()> {
        self.inner.update_preset_starred(uuid, starred).map_err(map_core_err)
    }

    pub fn delete_preset(&self, uuid: &str) -> Result<()> {
        self.inner.delete_preset(uuid).map_err(map_core_err)
    }

    // ── GuidedFiles — pass-through wrappers ───────────────────────────────────
    // Thin shells over the core CRUD; same Result = rusqlite::Result
    // shape as everything else here (DuplicateGuidedFile maps to a
    // synthesized UNIQUE-constraint failure via map_core_err).

    pub fn list_guided_files(&self) -> Result<Vec<GuidedFile>> {
        meditate_core::db::list_guided_files_from_db(&self.inner).map_err(map_core_err)
    }

    pub fn insert_guided_file_with_uuid(
        &self,
        uuid: &str,
        name: &str,
        file_path: &str,
        duration_secs: u32,
        is_starred: bool,
    ) -> Result<i64> {
        self.inner
            .insert_guided_file_with_uuid(uuid, name, file_path, duration_secs, is_starred)
            .map_err(map_core_err)
    }

    pub fn find_guided_file_by_uuid(&self, uuid: &str) -> Result<Option<GuidedFile>> {
        meditate_core::db::find_guided_file_by_uuid_from_db(&self.inner, uuid).map_err(map_core_err)
    }

    pub fn is_guided_file_name_taken(&self, name: &str, except_uuid: &str) -> Result<bool> {
        meditate_core::db::is_guided_file_name_taken_from_db(&self.inner, name, except_uuid).map_err(map_core_err)
    }

    pub fn rename_guided_file(&self, uuid: &str, name: &str) -> Result<()> {
        self.inner.rename_guided_file(uuid, name).map_err(map_core_err)
    }

    pub fn set_guided_file_starred(
        &self,
        uuid: &str,
        starred: meditate_core::db::StarredState,
    ) -> Result<()> {
        self.inner.set_guided_file_starred(uuid, starred).map_err(map_core_err)
    }

    pub fn delete_guided_file(&self, uuid: &str) -> Result<()> {
        self.inner.delete_guided_file(uuid).map_err(map_core_err)
    }

    // ── VibrationPatterns — pass-through wrappers ─────────────────────────────
    // Thin shells over the core CRUD. DuplicateVibrationPattern maps
    // to a synthesized UNIQUE-constraint failure via map_core_err so
    // callers can treat the duplicate-name path uniformly with the
    // other UNIQUE-NOCASE name fields (labels, presets, guided files).

    pub fn list_vibration_patterns(&self) -> Result<Vec<VibrationPattern>> {
        meditate_core::db::list_vibration_patterns_from_db(&self.inner).map_err(map_core_err)
    }

    pub fn find_vibration_pattern_by_uuid(
        &self, uuid: &str,
    ) -> Result<Option<VibrationPattern>> {
        meditate_core::db::find_vibration_pattern_by_uuid_from_db(&self.inner, uuid).map_err(map_core_err)
    }

    pub fn insert_vibration_pattern(
        &self,
        name: &str,
        duration_ms: u32,
        intensities: &[f32],
        chart_kind: ChartKind,
        is_bundled: bool,
    ) -> Result<String> {
        self.inner
            .insert_vibration_pattern(name, duration_ms, intensities, chart_kind, is_bundled)
            .map_err(map_core_err)
    }


    pub fn update_vibration_pattern(
        &self,
        uuid: &str,
        name: &str,
        duration_ms: u32,
        intensities: &[f32],
        chart_kind: ChartKind,
    ) -> Result<()> {
        self.inner
            .update_vibration_pattern(uuid, name, duration_ms, intensities, chart_kind)
            .map_err(map_core_err)
    }

    pub fn rename_vibration_pattern(&self, uuid: &str, new_name: &str) -> Result<bool> {
        self.inner.rename_vibration_pattern(uuid, new_name).map_err(map_core_err)
    }

    pub fn delete_vibration_pattern(&self, uuid: &str) -> Result<()> {
        self.inner.delete_vibration_pattern(uuid).map_err(map_core_err)
    }

    pub fn is_vibration_pattern_name_taken(
        &self, name: &str, except_uuid: &str,
    ) -> Result<bool> {
        meditate_core::db::is_vibration_pattern_name_taken_from_db(
            &self.inner, name, except_uuid,
        )
        .map_err(map_core_err)
    }

    // ── BoxBreath phases — pass-through wrappers ──────────────────────────────
    // Always-existing fixed-key rows (seeded in open). Only updates,
    // no insert / delete.

    pub fn list_box_breath_phases(&self) -> Result<Vec<BoxBreathPhase>> {
        self.inner.list_box_breath_phases().map_err(map_core_err)
    }

    pub fn get_box_breath_phase(
        &self, phase: BoxBreathPhaseId,
    ) -> Result<Option<BoxBreathPhase>> {
        self.inner.get_box_breath_phase(phase).map_err(map_core_err)
    }

    pub fn set_box_breath_phase(
        &self,
        phase: BoxBreathPhaseId,
        enabled: bool,
        signal_mode: SignalMode,
        sound_uuid: &str,
        pattern_uuid: &str,
        volume: meditate_core::bell_volume::BellVolume,
    ) -> Result<()> {
        self.inner
            .set_box_breath_phase(phase, enabled, signal_mode, sound_uuid, pattern_uuid, volume)
            .map_err(map_core_err)
    }

    // ── Sessions ──────────────────────────────────────────────────────────────

    pub fn create_session(&self, data: &SessionData) -> Result<Session> {
        let core = session_data_to_core(data);
        let id = self.inner.insert_session(&core).map_err(map_core_err)?;
        Ok(session_from_core(id, &core))
    }

    /// Save a just-ended session and drop its recovery snapshot in
    /// one write, counting the play of its Local Folder file (see
    /// core's `insert_session_counting_local_play`).
    pub fn save_ended_session(&self, data: &SessionData, local_file_id: Option<i64>) -> Result<Session> {
        let core = session_data_to_core(data);
        let (id, stored) = self
            .inner
            .insert_session_counting_local_play(&core, local_file_id)
            .map_err(map_core_err)?;
        Ok(session_from_core(id, &stored))
    }

    /// Insert many sessions inside a single core-side transaction.
    /// Atomic on error: a constraint violation rolls back the whole
    /// batch (see core's `bulk_insert_sessions` tests).
    pub fn bulk_insert_sessions(&self, sessions: &[SessionData]) -> Result<usize> {
        let core_rows: Vec<_> = sessions.iter().map(session_data_to_core).collect();
        self.inner.bulk_insert_sessions(&core_rows).map_err(map_core_err)
    }

    pub fn delete_all_sessions(&self) -> Result<usize> {
        self.inner.delete_all_sessions().map_err(map_core_err)
    }

    pub fn find_or_create_label(&self, name: &str) -> Result<i64> {
        self.inner.find_or_create_label(name).map_err(map_core_err)
    }

    pub fn list_sessions(&self, filter: &SessionFilter) -> Result<Vec<Session>> {
        let rows = meditate_core::db::query_sessions_from_db(&self.inner, filter).map_err(map_core_err)?;
        Ok(rows.into_iter().map(|(id, c)| session_from_core(id, &c)).collect())
    }

    pub fn update_session(&self, id: i64, data: &SessionData) -> Result<()> {
        self.inner.update_session(id, &session_data_to_core(data)).map_err(map_core_err)
    }

    pub fn delete_session(&self, id: i64) -> Result<()> {
        self.inner.delete_session(id).map_err(map_core_err)
    }

    /// Delete a session row by its cross-device uuid. The crash-
    /// recovery Undo flow uses this — the toast carries the uuid
    /// from `finalize_session_in_progress`, not the local rowid.
    pub fn delete_session_by_uuid(&self, uuid: &str) -> Result<()> {
        self.inner.delete_session_by_uuid(uuid).map_err(map_core_err)
    }

    // ── Session-in-progress (crash recovery) ──────────────────────────────────

    /// Write the device-local in-flight snapshot. Called by the timer
    /// view on session start and on the ~60s tick cadence. Does NOT
    /// emit a sync event — the in-progress row lives outside the
    /// event log so a session-in-progress stays private to this
    /// device.
    pub fn set_session_in_progress(
        &self,
        snapshot: &meditate_core::db::SessionInProgress,
    ) -> Result<()> {
        self.inner.set_session_in_progress(snapshot).map_err(map_core_err)
    }

    /// Read the in-flight snapshot, or `None` when no session is in
    /// progress (the common case on a clean shutdown).
    pub fn get_session_in_progress(
        &self,
    ) -> Result<Option<meditate_core::db::SessionInProgress>> {
        self.inner.get_session_in_progress().map_err(map_core_err)
    }

    /// Drop the in-flight snapshot. Idempotent; safe to call from
    /// every session-reset path.
    pub fn clear_session_in_progress(&self) -> Result<()> {
        self.inner.clear_session_in_progress().map_err(map_core_err)
    }

    /// Atomically convert any in-flight snapshot into a recorded
    /// session (one `session_insert` event emitted) and clear the
    /// snapshot row. Returns `None` when no session was in flight.
    /// Called from `application::startup` so a crash mid-session
    /// shows up as a saved row + an Undo toast on the next launch.
    pub fn finalize_session_in_progress(
        &self,
    ) -> Result<Option<meditate_core::db::FinalizedSession>> {
        self.inner.finalize_session_in_progress().map_err(map_core_err)
    }

    // ── Settings ──────────────────────────────────────────────────────────────

    pub fn get_setting(&self, key: &str, default: &str) -> Result<String> {
        self.inner.get_setting(key, default).map_err(map_core_err)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.inner.set_setting(key, value).map_err(map_core_err)
    }

    /// Read a sync-loop bookkeeping value (Nextcloud URL, username,
    /// last-sync timestamp, …). Separate namespace from `settings` so
    /// user-facing prefs and sync internals don't collide on a key.
    pub fn get_sync_state(&self, key: &str, default: &str) -> Result<String> {
        self.inner.get_sync_state(key, default).map_err(map_core_err)
    }

    /// Upsert a sync-loop bookkeeping value.
    pub fn set_sync_state(&self, key: &str, value: &str) -> Result<()> {
        self.inner.set_sync_state(key, value).map_err(map_core_err)
    }

    // ── Stats queries ─────────────────────────────────────────────────────────

    /// Current streak of consecutive calendar days (ending today or
    /// yesterday) with at least one session. "Today" is computed in the
    /// user's local timezone here, then handed to core.
    pub fn get_streak(&self) -> Result<u32> {
        let today = today_local_naive_date();
        meditate_core::db::get_streak_from_db(&self.inner, today).map_err(map_core_err)
    }

    pub fn get_best_streak(&self) -> Result<u32> {
        meditate_core::db::get_best_streak_from_db(&self.inner, today_local_naive_date()).map_err(map_core_err)
    }

    pub fn total_seconds(&self) -> Result<i64> {
        meditate_core::db::total_seconds_from_db(&self.inner).map_err(map_core_err)
    }

    pub fn active_months(&self) -> Result<Vec<(i32, u32)>> {
        meditate_core::db::active_months_from_db(&self.inner).map_err(map_core_err)
    }

    pub fn active_days_in_month(&self, year: i32, month: u32) -> Result<Vec<u32>> {
        meditate_core::db::active_days_in_month_from_db(&self.inner, year, month).map_err(map_core_err)
    }

    pub fn count_sessions(&self) -> Result<i64> {
        meditate_core::db::count_sessions_from_db(&self.inner).map_err(map_core_err)
    }

    pub fn get_label_totals(&self) -> Result<Vec<(String, i64, i64)>> {
        meditate_core::db::label_totals_seconds_from_db(&self.inner).map_err(map_core_err)
    }

}

/// In-memory DB for tests. Module-level so sibling files (e.g.
/// `data_io`) can construct a `Database` without needing a path.
/// Seeds the bundled bell sounds the same way `open()` does so tests
/// see post-seed state by default.
#[cfg(test)]
pub(crate) fn test_db_in_memory() -> Database {
    let db = Database {
        inner: meditate_core::Database::open_in_memory().unwrap(),
    };
    db.seed_bundled_bell_sounds().unwrap();
    db.inner.seed_default_labels().unwrap();
    db.inner.seed_default_presets().unwrap();
    db.inner.seed_bundled_vibration_patterns().unwrap();
    db.inner.seed_box_breath_phases().unwrap();
    db
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Tier B: in-memory integration tests against the wrapper ──────────────

    fn fresh_db() -> Database { super::test_db_in_memory() }

    // ── create_label collision behaviour ──────────────────────────────────────

    #[test]
    fn create_label_returns_unique_constraint_error_on_collision() {
        // create_label does NOT auto-rename. The UI pre-validates with
        // is_label_name_taken and shows a "label already exists" toast
        // before even calling this; the error here is the DB safety net
        // for any code path that bypassed that check.
        let db = fresh_db();
        let first = db.create_label("Morning").unwrap();
        assert_eq!(first.name, "Morning");

        let second = db.create_label("Morning");
        let err = second.expect_err("collision must surface as an error");
        assert!(
            matches!(err, DbError::DuplicateLabel(ref n) if n == "Morning"),
            "expected DuplicateLabel(\"Morning\"), got {err:?}",
        );
    }

    #[test]
    fn create_label_returns_duplicate_label_error_on_case_variant() {
        // Column is COLLATE NOCASE — different casings collide too.
        let db = fresh_db();
        db.create_label("Morning").unwrap();
        for variant in ["morning", "MORNING", "MoRnInG"] {
            let err = db.create_label(variant)
                .expect_err(&format!("'{variant}' must collide with 'Morning'"));
            // The name in the error is the colliding one the caller
            // typed — that's what the toast renders, not the
            // canonical row.
            assert!(
                matches!(err, DbError::DuplicateLabel(ref n) if n == variant),
                "expected DuplicateLabel(\"{variant}\"), got {err:?}",
            );
        }
    }

    #[test]
    fn create_label_preserves_caller_provided_name_verbatim() {
        // No auto-suffix, no normalisation — the caller gets back the
        // exact name they passed in, plus the new id.
        let db = fresh_db();
        let label = db.create_label("Pre-coffee meditation").unwrap();
        assert_eq!(label.name, "Pre-coffee meditation");
        assert!(label.id > 0);
    }

    #[test]
    fn shell_vibration_pattern_wrappers_round_trip() {
        let db = test_db_in_memory();
        // Insert via the auto-uuid wrapper, retrieve via find.
        let uuid = db.insert_vibration_pattern(
            "Custom Wave", 1000, &[0.0, 0.5, 1.0, 0.5, 0.0],
            ChartKind::Line, false,
        ).unwrap();
        let row = db.find_vibration_pattern_by_uuid(&uuid).unwrap().unwrap();
        assert_eq!(row.name, "Custom Wave");
        assert_eq!(row.intensities, vec![0.0, 0.5, 1.0, 0.5, 0.0]);

        // Update via the wrapper.
        db.update_vibration_pattern(
            &uuid, "Slow Wave", 2000, &[0.0, 0.3, 0.0],
            ChartKind::Bar,
        ).unwrap();
        let row = db.find_vibration_pattern_by_uuid(&uuid).unwrap().unwrap();
        assert_eq!(row.name, "Slow Wave");
        assert_eq!(row.chart_kind, ChartKind::Bar);

        // is_name_taken sees it.
        assert!(db.is_vibration_pattern_name_taken("Slow Wave", "").unwrap());
        // Self-rename-no-op excluded via except_uuid.
        assert!(!db.is_vibration_pattern_name_taken("Slow Wave", &uuid).unwrap());

        // Delete via wrapper.
        db.delete_vibration_pattern(&uuid).unwrap();
        assert!(db.find_vibration_pattern_by_uuid(&uuid).unwrap().is_none());
    }

    #[test]
    fn shell_vibration_pattern_duplicate_name_surfaces_typed_variant() {
        // The core-side DuplicateVibrationPattern variant must surface
        // through the shell with the colliding name preserved, so the
        // editor's create / rename flow can render a localised toast.
        let db = test_db_in_memory();
        // "Pulse" is already in the seed set — inserting a custom row
        // with the same name must fail through the wrapper.
        let result = db.insert_vibration_pattern(
            "Pulse", 500, &[0.0, 1.0, 0.0], ChartKind::Line, false,
        );
        let err = result.expect_err("duplicate name must surface as Err");
        assert!(
            matches!(err, DbError::DuplicateVibrationPattern(ref n) if n == "Pulse"),
            "expected DuplicateVibrationPattern(\"Pulse\"), got {err:?}",
        );
    }

    // ── #52: no write result is thrown away ───────────────────────────────────

    /// The places in `src` where a write's result is dropped: a
    /// `let _ =` on a database call, a `with_db_mut(..)` statement, or
    /// one read as nothing. Test modules are not scanned.
    fn dropped_writes(file: &str) -> Vec<String> {
        let path = format!("{}/src/{file}", env!("CARGO_MANIFEST_DIR"));
        let full = std::fs::read_to_string(&path).unwrap();
        let src = full.split("#[cfg(test)]\nmod").next().unwrap();
        let mut found = Vec::new();
        for (n, line) in src.lines().enumerate() {
            // Says nothing unless the name clashed; write_ok covers both.
            if line.contains("surface_duplicate_toast(") {
                found.push(format!("{file}:{}: {}", n + 1, line.trim()));
            }
            let t = line.trim_start();
            // #191: a setting write whose error only reaches the log.
            let settings = ["set_setting(", "bell_volume::write(", "set_box_breath_phase(", "settings_keys::set_", "labels::set_", "labels::persist_"];
            if t.starts_with("if let Err(") && settings.iter().any(|s| t.contains(s)) {
                found.push(format!("{file}:{}: {t}", n + 1));
            }
            let dropped = t.split("let _ =").skip(1).any(|rest| rest.contains("db.") || rest.contains("with_db"));
            if dropped {
                found.push(format!("{file}:{}: {t}", n + 1));
            }
        }
        let mut from = 0;
        while let Some(i) = src[from..].find(".with_db_mut(") {
            let at = from + i;
            from = at + 1;
            let line_start = src[..at].rfind('\n').map_or(0, |p| p + 1);
            let head = src[line_start..at].trim_start();
            let open = at + ".with_db_mut".len();
            let mut depth = 0;
            let mut close = open;
            for (j, c) in src[open..].char_indices() {
                match c {
                    '(' | '{' | '[' => depth += 1,
                    ')' | '}' | ']' => {
                        depth -= 1;
                        if depth == 0 { close = open + j; break; }
                    }
                    _ => {}
                }
            }
            let after = src[close + 1..].trim_start();
            if after.starts_with(".and_then(") || after.starts_with(".ok()") || after.starts_with(".unwrap_or") {
                let n = src[..at].lines().count();
                found.push(format!("{file}:{n}: a failed write read as nothing"));
                continue;
            }
            if !head.chars().all(|c| c.is_alphanumeric() || "_.()".contains(c)) {
                continue; // the value is used: `let x =`, `if`, `match`, an argument
            }
            if !after.starts_with(';') {
                continue; // a tail expression: the caller gets the result
            }
            // Even a closure that handles its own error drops the `None`
            // of no database at all.
            let n = src[..at].lines().count();
            found.push(format!("{file}:{n}: {}", src[line_start..].lines().next().unwrap().trim()));
        }
        found
    }

    #[test]
    fn no_write_result_is_dropped() {
        // #52, #191: a failed write must reach the user, never a success
        // toast or only the log.
        let files = [
            "presets.rs", "labels.rs", "log/imp.rs", "vibrations.rs",
            "bells.rs", "sounds.rs", "guided.rs", "preferences.rs",
            "vibration_editor.rs", "timer/imp.rs",
        ];
        let found: Vec<String> = files.iter().flat_map(|f| dropped_writes(f)).collect();
        assert!(found.is_empty(), "dropped write results:\n{}", found.join("\n"));
    }

    #[test]
    fn a_core_name_clash_keeps_its_text() {
        // Undo writes go through core and return its error type; a
        // name clash there still names the name, not "Save failed".
        let core = meditate_core::db::DbError::DuplicatePreset("Calm".into());
        assert!(clash_text(&core).is_some_and(|t| t.contains("Calm")));
        assert!(clash_text(&DbError::DuplicateLabel("Zen".into())).is_some_and(|t| t.contains("Zen")));
        assert_eq!(clash_text(&meditate_core::db::DbError::DateOutOfRange), None);
    }
}
