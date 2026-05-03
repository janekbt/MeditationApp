//! GTK-side `Database` — a thin wrapper around `meditate_core::db::Database`.
//!
//! All persistence logic lives in core. This module owns:
//! - The GTK-app's domain types (`Session`, `Label`, `SessionData`)
//!   which use i64-unix timestamps and the `note` field name for
//!   ergonomic UI integration. `SessionMode` is re-exported from
//!   core directly — no separate enum.
//! - Type translation at the API boundary (see `session_data_to_core` /
//!   `session_from_core` and `crate::time::unix_to_local_iso`).
//! - The `rusqlite::Result<T>` return type so callers can keep using `?`
//!   against this module without learning core's `DbError`.
//!
//! Core's schema is the on-disk reality; the existing app's old schema
//! is gone (one user, opted into a fresh DB).

use rusqlite::Result;
use std::path::Path;

// ── Models ────────────────────────────────────────────────────────────────────

/// `SessionMode` and `Label` are canonical core types — re-exported
/// here so call sites can keep importing from `crate::db` without
/// learning about meditate-core directly. Same for the bell-library
/// types added in B.3.1, and the `mint_uuid` helper added in B.5.
pub use meditate_core::db::{
    mint_uuid, BellSound, IntervalBell, IntervalBellKind, Label, SessionMode,
};

/// Bundled bell-sound seed: hardcoded (uuid, display name, GResource
/// path, MIME). UUIDs are STABLE across versions — never edit a row
/// here. New bundles get appended (never replace) so a peer that
/// already seeded the old set picks up the new ones via insert-or-
/// ignore. Adding a sound is a 1-tuple addition; no migration code.
const BUNDLED_BELL_SOUNDS: &[(&str, &str, &str, &str)] = &[
    (
        "f0c2e8a1-3a72-4d4f-9c8b-1b0e5d8c0001",
        "Singing Bowl",
        "/io/github/janekbt/Meditate/sounds/bowl.wav",
        "audio/wav",
    ),
    (
        "f0c2e8a1-3a72-4d4f-9c8b-1b0e5d8c0002",
        "Bell",
        "/io/github/janekbt/Meditate/sounds/bell.wav",
        "audio/wav",
    ),
    (
        "f0c2e8a1-3a72-4d4f-9c8b-1b0e5d8c0003",
        "Gong",
        "/io/github/janekbt/Meditate/sounds/gong.wav",
        "audio/wav",
    ),
];

/// Public so callers (B.4.4 migration site, etc.) can map old
/// "bowl" / "bell" / "gong" string keys to their bundled UUIDs
/// without re-deriving the table here.
pub const BUNDLED_BOWL_UUID: &str = "f0c2e8a1-3a72-4d4f-9c8b-1b0e5d8c0001";
pub const BUNDLED_BELL_UUID: &str = "f0c2e8a1-3a72-4d4f-9c8b-1b0e5d8c0002";
pub const BUNDLED_GONG_UUID: &str = "f0c2e8a1-3a72-4d4f-9c8b-1b0e5d8c0003";

#[derive(Debug, Clone)]
pub struct Session {
    pub id: i64,
    /// Unix timestamp (seconds since epoch) of when the session started.
    pub start_time: i64,
    pub duration_secs: i64,
    pub mode: SessionMode,
    pub label_id: Option<i64>,
    pub note: Option<String>,
}

/// Parameters for creating or updating a session.
pub struct SessionData {
    pub start_time: i64,
    pub duration_secs: i64,
    pub mode: SessionMode,
    pub label_id: Option<i64>,
    pub note: Option<String>,
}

/// `SessionFilter` is the canonical core type — re-exported here so
/// call sites can import from `crate::db` without learning about
/// meditate-core directly.
pub use meditate_core::db::SessionFilter;

// ── Translation: GTK-side ↔ meditate_core::db ─────────────────────────────────

/// Convert this app's `SessionData` (i64 unix, `note`) into core's
/// insert shape (ISO 8601 string, `notes`). Negative or overflowing
/// durations clamp to the u32 range.
fn session_data_to_core(s: &SessionData) -> meditate_core::db::Session {
    meditate_core::db::Session {
        start_iso: crate::time::unix_to_local_iso(s.start_time),
        duration_secs: s.duration_secs.clamp(0, u32::MAX as i64) as u32,
        label_id: s.label_id,
        notes: s.note.clone(),
        mode: s.mode,
        // Empty placeholder — core's `insert_session` overwrites this
        // with a freshly generated v4 uuid. Read paths see the real one.
        uuid: String::new(),
    }
}

/// Inverse of `session_data_to_core` for retrievals: takes core's
/// `(id, Session)` shape and produces the GTK-side `Session` with
/// embedded id and i64-unix `start_time`.
fn session_from_core(id: i64, core: &meditate_core::db::Session) -> Session {
    Session {
        id,
        start_time: crate::time::local_iso_to_unix(&core.start_iso),
        duration_secs: core.duration_secs as i64,
        mode: core.mode,
        label_id: core.label_id,
        note: core.notes.clone(),
    }
}

/// Map core's structured error to a `rusqlite::Error` so the GTK side
/// can keep its `Result = rusqlite::Result` alias. `DuplicateLabel`
/// becomes a synthesized UNIQUE-constraint failure, matching what
/// callers used to see when the GTK app talked to rusqlite directly.
fn map_core_err(e: meditate_core::db::DbError) -> rusqlite::Error {
    use meditate_core::db::DbError;
    match e {
        DbError::Sqlite(err) => err,
        DbError::DuplicateLabel(name) => rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error {
                code: rusqlite::ErrorCode::ConstraintViolation,
                extended_code: rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE,
            },
            Some(format!("UNIQUE constraint failed: labels.name (\"{name}\")")),
        ),
        DbError::Csv(s) => rusqlite::Error::ToSqlConversionFailure(Box::new(
            std::io::Error::new(std::io::ErrorKind::InvalidData, s),
        )),
    }
}

/// Today as a `chrono::NaiveDate` in the user's local timezone — used
/// for streak / running-average calculations that need a concrete
/// "today" boundary.
fn today_local_naive_date() -> chrono::NaiveDate {
    chrono::Local::now().date_naive()
}

// ── Database ──────────────────────────────────────────────────────────────────

pub struct Database {
    inner: meditate_core::db::Database,
}

impl std::fmt::Debug for Database {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Database").finish_non_exhaustive()
    }
}

impl Database {
    /// Open (or create) the database at `path`. Schema is core's; any
    /// pre-existing DB file written by an older version of this app
    /// will need to be deleted first (the user opted into that on
    /// 2026-04-29 — single-user repo).
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        }
        let inner = meditate_core::db::Database::open(path).map_err(map_core_err)?;
        let db = Self { inner };
        db.seed_bundled_bell_sounds()?;
        Ok(db)
    }

    /// Insert the bundled bell-sound rows on first run. Idempotent:
    /// `insert_bell_sound_with_uuid` is a no-op when the uuid row
    /// already exists, so subsequent runs (or runs that already had
    /// the bundle from a peer sync) skip without emitting duplicate
    /// events. UUIDs are hardcoded so every device — fresh seed or
    /// post-sync — ends up with the same row identity per file.
    fn seed_bundled_bell_sounds(&self) -> Result<()> {
        for (uuid, name, path, mime) in BUNDLED_BELL_SOUNDS {
            self.inner
                .insert_bell_sound_with_uuid(uuid, name, path, true, mime)
                .map_err(map_core_err)?;
        }
        Ok(())
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
        self.inner
            .list_labels()
            .map_err(map_core_err)?
            .into_iter()
            .find(|l| l.id == id)
            .ok_or(rusqlite::Error::QueryReturnedNoRows)
    }

    pub fn list_labels(&self) -> Result<Vec<Label>> {
        self.inner.list_labels().map_err(map_core_err)
    }

    /// True iff any label other than `except_id` already uses `name`
    /// (case-insensitive — the column is COLLATE NOCASE).
    pub fn is_label_name_taken(&self, name: &str, except_id: i64) -> Result<bool> {
        self.inner.is_label_name_taken(name, except_id).map_err(map_core_err)
    }

    pub fn update_label(&self, id: i64, name: &str) -> Result<()> {
        self.inner.update_label(id, name).map_err(map_core_err)
    }

    pub fn label_session_count(&self, id: i64) -> Result<i64> {
        self.inner.label_session_count(id).map_err(map_core_err)
    }

    pub fn delete_label(&self, id: i64) -> Result<()> {
        self.inner.delete_label(id).map_err(map_core_err)
    }

    // ── Interval bells ────────────────────────────────────────────────────────
    // Thin pass-throughs onto core's CRUD. Domain types are re-exported
    // from core verbatim — no shell-side translation needed.

    pub fn list_interval_bells(&self) -> Result<Vec<meditate_core::db::IntervalBell>> {
        self.inner.list_interval_bells().map_err(map_core_err)
    }

    pub fn insert_interval_bell(
        &self,
        kind: meditate_core::db::IntervalBellKind,
        minutes: u32,
        jitter_pct: u32,
        sound: &str,
    ) -> Result<i64> {
        self.inner
            .insert_interval_bell(kind, minutes, jitter_pct, sound)
            .map_err(map_core_err)
    }

    pub fn update_interval_bell(
        &self,
        uuid: &str,
        kind: meditate_core::db::IntervalBellKind,
        minutes: u32,
        jitter_pct: u32,
        sound: &str,
        enabled: bool,
    ) -> Result<()> {
        self.inner
            .update_interval_bell(uuid, kind, minutes, jitter_pct, sound, enabled)
            .map_err(map_core_err)
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

    pub fn insert_bell_sound(
        &self,
        name: &str,
        file_path: &str,
        is_bundled: bool,
        mime_type: &str,
    ) -> Result<i64> {
        self.inner
            .insert_bell_sound(name, file_path, is_bundled, mime_type)
            .map_err(map_core_err)
    }

    pub fn insert_bell_sound_with_uuid(
        &self,
        uuid: &str,
        name: &str,
        file_path: &str,
        is_bundled: bool,
        mime_type: &str,
    ) -> Result<i64> {
        self.inner
            .insert_bell_sound_with_uuid(uuid, name, file_path, is_bundled, mime_type)
            .map_err(map_core_err)
    }

    pub fn rename_bell_sound(&self, uuid: &str, name: &str) -> Result<()> {
        self.inner.rename_bell_sound(uuid, name).map_err(map_core_err)
    }

    pub fn delete_bell_sound(&self, uuid: &str) -> Result<()> {
        self.inner.delete_bell_sound(uuid).map_err(map_core_err)
    }

    // ── Sessions ──────────────────────────────────────────────────────────────

    pub fn create_session(&self, data: &SessionData) -> Result<Session> {
        let core = session_data_to_core(data);
        let id = self.inner.insert_session(&core).map_err(map_core_err)?;
        Ok(session_from_core(id, &core))
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
        let rows = self.inner.query_sessions(filter).map_err(map_core_err)?;
        Ok(rows.into_iter().map(|(id, c)| session_from_core(id, &c)).collect())
    }

    pub fn update_session(&self, id: i64, data: &SessionData) -> Result<()> {
        self.inner.update_session(id, &session_data_to_core(data)).map_err(map_core_err)
    }

    pub fn delete_session(&self, id: i64) -> Result<()> {
        self.inner.delete_session(id).map_err(map_core_err)
    }

    // ── Settings ──────────────────────────────────────────────────────────────

    pub fn get_presets(&self) -> Result<Vec<u32>> {
        let s = self.get_setting("timer_presets", "5,10,15,20,30")?;
        let vals: Vec<u32> = s.split(',')
            .filter_map(|v| v.trim().parse::<u32>().ok())
            .filter(|&v| v > 0)
            .collect();
        if vals.is_empty() { Ok(vec![5, 10, 15, 20, 30]) } else { Ok(vals) }
    }

    pub fn set_presets(&self, presets: &[u32]) -> Result<()> {
        let s = presets.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(",");
        self.set_setting("timer_presets", &s)
    }

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

    /// All remote file_uuids this device has ingested or pushed.
    /// Used by the remote-data-lost detection to recognise our own
    /// previous batches in the remote listing.
    pub fn known_remote_file_uuids(&self)
        -> Result<std::collections::HashSet<String>>
    {
        self.inner.known_remote_file_uuids().map_err(map_core_err)
    }

    /// Record a remote batch_uuid as ingested. Idempotent.
    pub fn record_known_remote_file(&self, file_uuid: &str) -> Result<()> {
        self.inner.record_known_remote_file(file_uuid).map_err(map_core_err)
    }

    /// Drop every recorded remote file_uuid. Called on account swap
    /// (URL or username change) and on the "push local up" recovery
    /// path after a remote-data-lost prompt.
    pub fn wipe_known_remote_files(&self) -> Result<()> {
        self.inner.wipe_known_remote_files().map_err(map_core_err)
    }

    // ── known_remote_sounds (B.6) ─────────────────────────────────────────────
    // Same lifecycle shape as known_remote_files but keyed on
    // bell_sounds.uuid for the audio-file sync layer.

    pub fn known_remote_sound_uuids(&self)
        -> Result<std::collections::HashSet<String>>
    {
        self.inner.known_remote_sound_uuids().map_err(map_core_err)
    }

    pub fn record_known_remote_sound(&self, bell_uuid: &str) -> Result<()> {
        self.inner.record_known_remote_sound(bell_uuid).map_err(map_core_err)
    }

    pub fn wipe_known_remote_sounds(&self) -> Result<()> {
        self.inner.wipe_known_remote_sounds().map_err(map_core_err)
    }

    /// Reset every event row's synced flag to 0, putting all of them
    /// back into pending. Used by the "push local up" recovery path.
    pub fn flag_all_events_unsynced(&self) -> Result<()> {
        self.inner.flag_all_events_unsynced().map_err(map_core_err)
    }

    /// Erase every user-content row plus the dedup tracker — events,
    /// sessions, labels, known_remote_files. Settings, sync_state,
    /// and device identity survive. Used by the "wipe local to match
    /// remote" recovery path.
    pub fn wipe_local_event_log(&self) -> Result<()> {
        self.inner.wipe_local_event_log().map_err(map_core_err)
    }

    /// How many events are currently pending push. Mostly a test-
    /// observability helper, but useful for any caller that wants to
    /// know "is there local work to sync?" without listing the full
    /// pending vector.
    pub fn pending_events_count(&self) -> Result<usize> {
        Ok(self.inner.pending_events().map_err(map_core_err)?.len())
    }

    // ── Stats queries ─────────────────────────────────────────────────────────

    /// Current streak of consecutive calendar days (ending today or
    /// yesterday) with at least one session. "Today" is computed in the
    /// user's local timezone here, then handed to core.
    pub fn get_streak(&self) -> Result<u32> {
        let today = today_local_naive_date();
        let n = self.inner.get_streak(today).map_err(map_core_err)?;
        Ok(n.max(0) as u32)
    }

    pub fn get_best_streak(&self) -> Result<u32> {
        let n = self.inner.get_best_streak().map_err(map_core_err)?;
        Ok(n.max(0) as u32)
    }

    pub fn total_seconds(&self) -> Result<i64> {
        self.inner.total_seconds().map_err(map_core_err)
    }

    /// Average daily duration over the last `days` days. Days with no
    /// sessions count as zero. Returns 0 for `days == 0` (guards against
    /// the underflow `days - 1` would cause).
    pub fn get_running_average_secs(&self, days: u32) -> Result<f64> {
        if days == 0 {
            return Ok(0.0);
        }
        let since = today_local_naive_date() - chrono::Duration::days((days - 1) as i64);
        let total = self.inner.total_secs_since(since).map_err(map_core_err)?;
        Ok(total as f64 / days as f64)
    }

    /// `(local-date "YYYY-MM-DD", total_secs)` for each day on or after
    /// `since_date`. Filters core's full daily-totals list in Rust;
    /// fine for the typical 30–90 day window the heatmap asks for.
    pub fn get_daily_totals(&self, since_date: &str) -> Result<Vec<(String, i64)>> {
        let since = chrono::NaiveDate::parse_from_str(since_date, "%Y-%m-%d")
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        let totals = self.inner.get_daily_totals().map_err(map_core_err)?;
        Ok(totals
            .into_iter()
            .filter(|(d, _)| *d >= since)
            .map(|(d, secs)| (d.format("%Y-%m-%d").to_string(), secs))
            .collect())
    }

    pub fn get_total_secs_since(&self, since_date: &str) -> Result<i64> {
        let since = chrono::NaiveDate::parse_from_str(since_date, "%Y-%m-%d")
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        self.inner.total_secs_since(since).map_err(map_core_err)
    }

    pub fn active_months(&self) -> Result<Vec<(i32, u32)>> {
        self.inner.active_months().map_err(map_core_err)
    }

    pub fn active_days_in_month(&self, year: i32, month: u32) -> Result<Vec<u32>> {
        self.inner.active_days_in_month(year, month).map_err(map_core_err)
    }

    pub fn count_sessions(&self) -> Result<i64> {
        self.inner.count_sessions().map_err(map_core_err)
    }

    /// Longest single session as `(duration_secs, start_time_unix)`,
    /// None on empty DB. The shape is a tuple for backward compat with
    /// existing UI sites; core returns the full Session.
    pub fn get_longest_session(&self) -> Result<Option<(i64, i64)>> {
        let row = self.inner.get_longest_session().map_err(map_core_err)?;
        Ok(row.map(|(_id, c)| {
            (c.duration_secs as i64, crate::time::local_iso_to_unix(&c.start_iso))
        }))
    }

    /// Median session duration, None on empty DB. Core's variant
    /// returns 0 for empty (lossy for the UI's "n/a" display); the
    /// wrapper checks `count_sessions` first to recover the None case.
    pub fn get_median_duration_secs(&self) -> Result<Option<i64>> {
        if self.inner.count_sessions().map_err(map_core_err)? == 0 {
            return Ok(None);
        }
        let secs = self.inner.get_median_duration_secs().map_err(map_core_err)?;
        Ok(Some(secs as i64))
    }

    pub fn hour_buckets(&self) -> Result<(i64, i64, i64)> {
        self.inner.hour_buckets().map_err(map_core_err)
    }

    pub fn get_label_totals(&self) -> Result<Vec<(String, i64, i64)>> {
        self.inner.label_totals_seconds().map_err(map_core_err)
    }

    pub fn month_total_secs(&self, year: i32, month: u32) -> Result<i64> {
        self.inner.month_total_secs(year, month).map_err(map_core_err)
    }
}

/// In-memory DB for tests. Module-level so sibling files (e.g.
/// `data_io`) can construct a `Database` without needing a path.
/// Seeds the bundled bell sounds the same way `open()` does so tests
/// see post-seed state by default.
#[cfg(test)]
pub(crate) fn test_db_in_memory() -> Database {
    let db = Database {
        inner: meditate_core::db::Database::open_in_memory().unwrap(),
    };
    db.seed_bundled_bell_sounds().unwrap();
    db
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── core::Session translation helpers ─────────────────────────────────────

    #[test]
    fn session_data_to_core_preserves_every_field() {
        let sd = SessionData {
            start_time: 1_700_000_000,
            duration_secs: 1234,
            mode: SessionMode::BoxBreath,
            label_id: Some(42),
            note: Some("hello".to_string()),
        };
        let core = session_data_to_core(&sd);
        assert_eq!(crate::time::local_iso_to_unix(&core.start_iso), 1_700_000_000);
        assert_eq!(core.duration_secs, 1234);
        assert_eq!(core.label_id, Some(42));
        assert_eq!(core.notes, Some("hello".to_string()));
        assert!(matches!(core.mode, meditate_core::db::SessionMode::BoxBreath));
    }

    #[test]
    fn session_data_to_core_clamps_negative_duration_to_zero() {
        let sd = SessionData {
            start_time: 1_700_000_000,
            duration_secs: -1,
            mode: SessionMode::Timer,
            label_id: None,
            note: None,
        };
        assert_eq!(session_data_to_core(&sd).duration_secs, 0);
    }

    #[test]
    fn session_data_to_core_clamps_overflowing_duration_to_u32_max() {
        let sd = SessionData {
            start_time: 0,
            duration_secs: i64::MAX,
            mode: SessionMode::Timer,
            label_id: None,
            note: None,
        };
        assert_eq!(session_data_to_core(&sd).duration_secs, u32::MAX);
    }

    #[test]
    fn session_from_core_preserves_every_field() {
        let core = meditate_core::db::Session {
            start_iso: crate::time::unix_to_local_iso(1_700_000_000),
            duration_secs: 600,
            label_id: Some(7),
            notes: Some("from core".to_string()),
            mode: meditate_core::db::SessionMode::BoxBreath,
            // Translation from core → shell drops the uuid (the GTK-side
            // Session doesn't carry one). This test pins the rest of the
            // mapping; uuid round-trip is covered in core's tests.
            uuid: "ignored-by-shell".to_string(),
        };
        let s = session_from_core(99, &core);
        assert_eq!(s.id, 99);
        assert_eq!(s.start_time, 1_700_000_000);
        assert_eq!(s.duration_secs, 600);
        assert_eq!(s.label_id, Some(7));
        assert_eq!(s.note, Some("from core".to_string()));
        assert_eq!(s.mode, SessionMode::BoxBreath);
    }

    #[test]
    fn session_data_round_trips_through_core_and_back() {
        let original = SessionData {
            start_time: 1_700_000_000,
            duration_secs: 750,
            mode: SessionMode::Timer,
            label_id: Some(11),
            note: Some("noted".to_string()),
        };
        let core = session_data_to_core(&original);
        let restored = session_from_core(123, &core);
        assert_eq!(restored.id, 123);
        assert_eq!(restored.start_time, original.start_time);
        assert_eq!(restored.duration_secs, original.duration_secs);
        assert_eq!(restored.mode, original.mode);
        assert_eq!(restored.label_id, original.label_id);
        assert_eq!(restored.note, original.note);
    }

    // ── Tier B: in-memory integration tests against the wrapper ──────────────

    fn fresh_db() -> Database { super::test_db_in_memory() }

    /// Unix timestamp at `hh:mm` local time, `days_ago` local-days back.
    /// Computed via chrono::Local — matches the wrapper's own
    /// `today_local_naive_date` so streak / running-average tests share
    /// the same "today" boundary.
    fn local_ts(days_ago: i64, hh: u32, mm: u32) -> i64 {
        use chrono::TimeZone;
        let date = chrono::Local::now().date_naive() - chrono::Duration::days(days_ago);
        let datetime = date.and_hms_opt(hh, mm, 0).unwrap();
        chrono::Local
            .from_local_datetime(&datetime)
            .single()
            .unwrap()
            .timestamp()
    }

    fn seed_session(db: &Database, start_time: i64, duration_secs: i64, label_id: Option<i64>) {
        db.create_session(&SessionData {
            start_time,
            duration_secs,
            mode: SessionMode::Timer,
            label_id,
            note: None,
        }).unwrap();
    }

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
            matches!(
                err,
                rusqlite::Error::SqliteFailure(ref f, _)
                    if f.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE
            ),
            "expected UNIQUE constraint violation, got {err:?}",
        );
    }

    #[test]
    fn create_label_returns_unique_constraint_error_on_case_variant() {
        // Column is COLLATE NOCASE — different casings collide too.
        let db = fresh_db();
        db.create_label("Morning").unwrap();
        for variant in ["morning", "MORNING", "MoRnInG"] {
            let err = db.create_label(variant)
                .expect_err(&format!("'{variant}' must collide with 'Morning'"));
            assert!(matches!(
                err,
                rusqlite::Error::SqliteFailure(ref f, _)
                    if f.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE
            ));
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

    // ── Streak (gap-and-island via core) ──────────────────────────────────────

    #[test]
    fn streak_empty_db_is_zero() {
        let db = fresh_db();
        assert_eq!(db.get_streak().unwrap(), 0);
        assert_eq!(db.get_best_streak().unwrap(), 0);
    }

    #[test]
    fn streak_today_only() {
        let db = fresh_db();
        seed_session(&db, local_ts(0, 12, 0), 600, None);
        assert_eq!(db.get_streak().unwrap(), 1);
        assert_eq!(db.get_best_streak().unwrap(), 1);
    }

    #[test]
    fn streak_yesterday_only_still_counts() {
        let db = fresh_db();
        seed_session(&db, local_ts(1, 12, 0), 600, None);
        // Grace day: yesterday counts until end-of-today.
        assert_eq!(db.get_streak().unwrap(), 1);
    }

    #[test]
    fn streak_two_days_ago_is_broken() {
        let db = fresh_db();
        seed_session(&db, local_ts(2, 12, 0), 600, None);
        // Older than yesterday → current streak 0 even though best is 1.
        assert_eq!(db.get_streak().unwrap(), 0);
        assert_eq!(db.get_best_streak().unwrap(), 1);
    }

    #[test]
    fn streak_consecutive_run_of_five() {
        let db = fresh_db();
        for d in 0..5 {
            seed_session(&db, local_ts(d, 12, 0), 600, None);
        }
        assert_eq!(db.get_streak().unwrap(), 5);
        assert_eq!(db.get_best_streak().unwrap(), 5);
    }

    #[test]
    fn streak_gap_separates_current_from_best() {
        let db = fresh_db();
        for d in [30, 29, 28, 27, 26, 25] {
            seed_session(&db, local_ts(d, 12, 0), 600, None);
        }
        for d in [2, 1, 0] {
            seed_session(&db, local_ts(d, 12, 0), 600, None);
        }
        assert_eq!(db.get_streak().unwrap(), 3);
        assert_eq!(db.get_best_streak().unwrap(), 6);
    }

    #[test]
    fn streak_multiple_sessions_same_day_count_once() {
        let db = fresh_db();
        seed_session(&db, local_ts(0, 9, 0), 600, None);
        seed_session(&db, local_ts(0, 18, 0), 600, None);
        assert_eq!(db.get_streak().unwrap(), 1);
        assert_eq!(db.get_best_streak().unwrap(), 1);
    }

    // ── Running average ───────────────────────────────────────────────────────

    #[test]
    fn running_average_zero_days_returns_zero() {
        let db = fresh_db();
        assert_eq!(db.get_running_average_secs(0).unwrap(), 0.0);
    }

    #[test]
    fn running_average_empty_window() {
        let db = fresh_db();
        assert_eq!(db.get_running_average_secs(7).unwrap(), 0.0);
    }

    #[test]
    fn running_average_divides_by_window_not_session_count() {
        let db = fresh_db();
        seed_session(&db, local_ts(0, 12, 0), 600, None);
        seed_session(&db, local_ts(1, 12, 0), 600, None);
        let avg = db.get_running_average_secs(7).unwrap();
        assert!((avg - 1200.0 / 7.0).abs() < 1e-6, "avg was {avg}");
    }

    #[test]
    fn running_average_excludes_sessions_before_window() {
        let db = fresh_db();
        // In-window (6 days ago, inside the 7-day window incl. today).
        seed_session(&db, local_ts(6, 12, 0), 300, None);
        // Out-of-window (8 days ago).
        seed_session(&db, local_ts(8, 12, 0), 9999, None);
        let avg = db.get_running_average_secs(7).unwrap();
        assert!((avg - 300.0 / 7.0).abs() < 1e-6, "avg was {avg}");
    }

    // ── Daily totals (local-midnight grouping) ────────────────────────────────

    #[test]
    fn daily_totals_groups_by_local_date() {
        let db = fresh_db();
        // 23:55 local + 00:05 local on the same local day must collapse.
        seed_session(&db, local_ts(0, 23, 55), 300, None);
        seed_session(&db, local_ts(0, 0, 5), 300, None);
        // An earlier local day to prove the grouping actually separates.
        seed_session(&db, local_ts(3, 12, 0), 600, None);

        // Pick a 7-day window that includes both the today-bucket and
        // the 3-days-ago bucket.
        let since = (chrono::Local::now().date_naive() - chrono::Duration::days(7))
            .format("%Y-%m-%d")
            .to_string();
        let totals = db.get_daily_totals(&since).unwrap();
        assert_eq!(totals.len(), 2, "two distinct local dates expected, got {totals:?}");
        // Sorted ascending by day: older first.
        assert_eq!(totals[0].1, 600);
        assert_eq!(totals[1].1, 600); // 300 + 300 collapsed onto today
    }

    // ── Median ────────────────────────────────────────────────────────────────

    #[test]
    fn median_empty_db_is_none() {
        let db = fresh_db();
        assert_eq!(db.get_median_duration_secs().unwrap(), None);
    }

    #[test]
    fn median_single_row() {
        let db = fresh_db();
        seed_session(&db, local_ts(0, 12, 0), 600, None);
        assert_eq!(db.get_median_duration_secs().unwrap(), Some(600));
    }

    #[test]
    fn median_odd_count_is_middle() {
        let db = fresh_db();
        for (i, secs) in [100, 500, 700, 1000, 2000].iter().enumerate() {
            seed_session(&db, local_ts(i as i64, 12, 0), *secs, None);
        }
        assert_eq!(db.get_median_duration_secs().unwrap(), Some(700));
    }

    #[test]
    fn median_even_count_takes_lower_of_two_middles() {
        let db = fresh_db();
        for (i, secs) in [100, 500, 700, 1000].iter().enumerate() {
            seed_session(&db, local_ts(i as i64, 12, 0), *secs, None);
        }
        assert_eq!(db.get_median_duration_secs().unwrap(), Some(500));
    }

    // ── Label totals ──────────────────────────────────────────────────────────

    #[test]
    fn label_totals_groups_sums_and_excludes_empties() {
        let db = fresh_db();
        let morning = db.create_label("Morning").unwrap().id;
        let evening = db.create_label("Evening").unwrap().id;
        let _unused = db.create_label("Unused").unwrap().id;

        seed_session(&db, local_ts(0, 7, 0), 600, Some(morning));
        seed_session(&db, local_ts(1, 7, 0), 300, Some(morning));
        seed_session(&db, local_ts(0, 20, 0), 1200, Some(evening));
        seed_session(&db, local_ts(0, 12, 0), 500, None);

        let got = db.get_label_totals().unwrap();
        assert_eq!(got.len(), 2);
        assert_eq!(got[0], ("Evening".to_string(), 1200, 1));
        assert_eq!(got[1], ("Morning".to_string(), 900, 2));
    }

    #[test]
    fn label_totals_empty_db_is_empty() {
        let db = fresh_db();
        assert!(db.get_label_totals().unwrap().is_empty());
    }

    #[test]
    fn label_totals_ties_break_alphabetically() {
        let db = fresh_db();
        let zebra = db.create_label("Zebra").unwrap().id;
        let alpha = db.create_label("Alpha").unwrap().id;
        seed_session(&db, local_ts(0, 12, 0), 600, Some(zebra));
        seed_session(&db, local_ts(1, 12, 0), 600, Some(alpha));
        let got = db.get_label_totals().unwrap();
        assert_eq!(got[0].0, "Alpha");
        assert_eq!(got[1].0, "Zebra");
    }

    // ── Bundled bell-sound seeding (B.4.2) ────────────────────────

    #[test]
    fn open_seeds_bundled_bell_sounds_with_stable_uuids() {
        let db = test_db_in_memory();
        let sounds = db.list_bell_sounds().unwrap();
        assert_eq!(sounds.len(), 3, "three bundled rows seeded on first open");
        assert!(sounds.iter().all(|s| s.is_bundled));
        // Stable UUIDs the constants point at.
        assert!(sounds.iter().any(|s| s.uuid == BUNDLED_BOWL_UUID));
        assert!(sounds.iter().any(|s| s.uuid == BUNDLED_BELL_UUID));
        assert!(sounds.iter().any(|s| s.uuid == BUNDLED_GONG_UUID));
    }

    #[test]
    fn seeding_twice_is_idempotent() {
        let db = test_db_in_memory();
        // Helper already seeded once; do it again manually.
        db.seed_bundled_bell_sounds().unwrap();
        let sounds = db.list_bell_sounds().unwrap();
        assert_eq!(sounds.len(), 3, "second seed must not duplicate rows");
    }

    #[test]
    fn seeding_emits_one_insert_event_per_bundled_row_and_no_more_on_re_seed() {
        // Sync correctness: every seeded row produces exactly one
        // bell_sound_insert in the event log on the first device that
        // sees it. A re-seed on a device that's already done it must
        // not bump the event log — peers don't need a redundant insert.
        let db = test_db_in_memory();
        let after_first: Vec<_> = db
            .inner
            .pending_events()
            .unwrap()
            .into_iter()
            .filter(|(_, e)| e.kind == "bell_sound_insert")
            .collect();
        assert_eq!(after_first.len(), 3);

        db.seed_bundled_bell_sounds().unwrap();
        let after_second: Vec<_> = db
            .inner
            .pending_events()
            .unwrap()
            .into_iter()
            .filter(|(_, e)| e.kind == "bell_sound_insert")
            .collect();
        assert_eq!(after_second.len(), 3, "no extra events on re-seed");
    }
}
