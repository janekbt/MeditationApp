//! GTK-shell wrappers around `meditate_core::data_io`.
//!
//! The pure CSV parse / write logic plus the `(label_idx, …) -> Session`
//! second pass live in core. This file is the `MeditateApplication`
//! glue (DB access via `app.with_db*`) plus the Insight Timer importer
//! (still here because it needs `gtk::glib::DateTime` for the
//! local-time → unix conversion — chrono can do the same, but the move
//! is on its own future migration).
//!
//! Native CSV format documented in `meditate_core::data_io`.

use std::path::Path;

use crate::application::MeditateApplication;
use crate::db::Database;

/// Everything that can go wrong during import or export, collapsed into a
/// single user-facing error type so the caller can just show a toast.
#[derive(Debug)]
pub enum DataIoError {
    Io(std::io::Error),
    Csv(csv::Error),
    Parse(String),
    Db(String),
    NoDatabase,
}

impl std::fmt::Display for DataIoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use crate::i18n::gettext;
        match self {
            DataIoError::Io(e)    => write!(f, "{}: {e}", gettext("File error")),
            DataIoError::Csv(e)   => write!(f, "{}: {e}", gettext("CSV error")),
            DataIoError::Parse(m) => write!(f, "{}: {m}", gettext("Parse error")),
            DataIoError::Db(m)    => write!(f, "{}: {m}", gettext("Database error")),
            DataIoError::NoDatabase => write!(f, "{}", gettext("Database unavailable")),
        }
    }
}

impl From<std::io::Error> for DataIoError {
    fn from(e: std::io::Error) -> Self { DataIoError::Io(e) }
}
impl From<csv::Error> for DataIoError {
    fn from(e: csv::Error) -> Self { DataIoError::Csv(e) }
}
impl From<rusqlite::Error> for DataIoError {
    fn from(e: rusqlite::Error) -> Self { DataIoError::Db(e.to_string()) }
}
impl From<crate::db::DbError> for DataIoError {
    fn from(e: crate::db::DbError) -> Self { DataIoError::Db(e.to_string()) }
}

/// Bridge from core's English-only error type to the gtk shell's
/// gettext-localized one. The variant mapping is one-to-one; the
/// `NoDatabase` arm is gtk-only.
impl From<meditate_core::data_io::DataIoError> for DataIoError {
    fn from(e: meditate_core::data_io::DataIoError) -> Self {
        use meditate_core::data_io::DataIoError as Core;
        match e {
            Core::Io(err) => DataIoError::Io(err),
            Core::Csv(err) => DataIoError::Csv(err),
            Core::Parse(m) => DataIoError::Parse(m),
            Core::Db(m) => DataIoError::Db(m),
        }
    }
}

/// Suggested filename for an export, e.g. `meditate-backup-2026-04-20_142030.csv`.
pub fn suggested_export_filename() -> String {
    let now = crate::time::now_local();
    let ts  = now.format("%Y-%m-%d_%H%M%S").map_or_else(|_| "unknown".to_string(), |s| s.to_string());
    format!("meditate-backup-{ts}.csv")
}

// ── Export ────────────────────────────────────────────────────────────────────

/// Write every session in the DB to `path` as CSV. Returns how many rows
/// were written.
pub fn export_csv(app: &MeditateApplication, path: &Path) -> Result<usize, DataIoError> {
    let result: Result<usize, DataIoError> = app
        .with_db(|db| meditate_core::data_io::export_csv(db.core(), path))
        .ok_or(DataIoError::NoDatabase)?
        .map_err(DataIoError::from);
    match &result {
        Ok(n) => meditate_core::log(
            "export.csv",
            &format!("wrote sessions={n} path={}", path.display()),
        ),
        Err(e) => meditate_core::log(
            "export.csv",
            &format!("FAILED path={} err={e}", path.display()),
        ),
    }
    result
}

// ── Native-format import ──────────────────────────────────────────────────────

pub fn import_csv(app: &MeditateApplication, path: &Path) -> Result<usize, DataIoError> {
    let result: Result<usize, DataIoError> = app
        .with_db_mut(|db| meditate_core::data_io::import_csv(db.core(), path))
        .ok_or(DataIoError::NoDatabase)?
        .map_err(DataIoError::from);
    match &result {
        Ok(n) => meditate_core::log(
            "import.csv",
            &format!("read sessions={n} path={}", path.display()),
        ),
        Err(e) => meditate_core::log(
            "import.csv",
            &format!("FAILED path={} err={e}", path.display()),
        ),
    }
    result
}

// ── Insight Timer import ──────────────────────────────────────────────────────

pub fn import_insighttimer(app: &MeditateApplication, path: &Path) -> Result<usize, DataIoError> {
    let result = app.with_db_mut(|db| import_insighttimer_to_db(db, path))
        .ok_or(DataIoError::NoDatabase)?;
    match &result {
        Ok(n) => meditate_core::log(
            "import.insighttimer",
            &format!("read sessions={n} path={}", path.display()),
        ),
        Err(e) => meditate_core::log(
            "import.insighttimer",
            &format!("FAILED path={} err={e}", path.display()),
        ),
    }
    result
}

pub(crate) fn import_insighttimer_to_db(db: &Database, path: &Path) -> Result<usize, DataIoError> {
    // CSV parsing + label dedup + duration validation live in core;
    // gtk supplies its glib-based datetime parser via the closure
    // (Android passes its own chrono-based one).
    let (label_names, rows) = meditate_core::data_io::parse_insighttimer_csv(
        path,
        parse_insighttimer_datetime,
    )?;
    meditate_core::data_io::insert_sessions_with_labels(db.core(), &label_names, &rows)
        .map_err(DataIoError::from)
}

// ── Delete all ────────────────────────────────────────────────────────────────

pub fn delete_all(app: &MeditateApplication) -> Result<usize, DataIoError> {
    app.with_db_mut(super::db::Database::delete_all_sessions)
        .ok_or(DataIoError::NoDatabase)?
        .map_err(Into::into)
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Parse an InsightTimer "Started At" cell as local time and return the
/// unix timestamp. Format detection (12-hour AM/PM vs 24-hour) lives in
/// `meditate_core::format::parse_insighttimer_datetime`; this shim only
/// owns the local-tz → unix conversion that needs glib.
fn parse_insighttimer_datetime(s: &str) -> Option<i64> {
    use chrono::{Datelike, Timelike};
    let dt = meditate_core::format::parse_insighttimer_datetime(s)?;
    let glib_dt = gtk::glib::DateTime::new(
        &gtk::glib::TimeZone::local(),
        dt.year(), dt.month() as i32, dt.day() as i32,
        dt.hour() as i32, dt.minute() as i32, f64::from(dt.second()),
    ).ok()?;
    Some(glib_dt.to_unix())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_insighttimer_datetime_valid() {
        // MM/DD/YYYY HH:MM:SS, interpreted as local time — assert the parse
        // succeeds; the exact unix value depends on the host TZ, so we only
        // check round-trip consistency (same input → same output) rather than
        // a fixed number.
        let a = parse_insighttimer_datetime("04/21/2026 08:30:00");
        let b = parse_insighttimer_datetime("04/21/2026 08:30:00");
        assert!(a.is_some());
        assert_eq!(a, b);
        // One hour later → exactly 3600s later.
        let c = parse_insighttimer_datetime("04/21/2026 09:30:00").unwrap();
        assert_eq!(c - a.unwrap(), 3600);
    }

    #[test]
    fn parse_insighttimer_datetime_garbage() {
        assert_eq!(parse_insighttimer_datetime(""), None);
        assert_eq!(parse_insighttimer_datetime("04/21/2026"), None); // missing time
        assert_eq!(parse_insighttimer_datetime("2026-04-21 08:30:00"), None); // ISO, wrong fmt
        assert_eq!(parse_insighttimer_datetime("xx/yy/zzzz 08:30:00"), None);
        assert_eq!(parse_insighttimer_datetime("04/21/2026 08:30"), None); // missing seconds
        assert_eq!(parse_insighttimer_datetime("13/21/2026 08:30:00"), None); // month 13
    }
}
