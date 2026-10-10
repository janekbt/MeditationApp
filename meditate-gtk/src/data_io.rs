//! GTK-shell wrappers around `meditate_core::data_io`: the
//! `MeditateApplication` glue (DB access via `app.with_db*`), the
//! shell's localized error type, and logging. Parsing and writing,
//! Insight Timer's included, live in core.
//!
//! Native CSV format documented in `meditate_core::data_io`.

use std::path::Path;

use meditate_core::data_io::ParsedImport;

use crate::application::MeditateApplication;

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

// ── Import ────────────────────────────────────────────────────────────────────

/// Read a backup (native format) without writing anything.
pub fn parse_csv(path: &Path) -> Result<ParsedImport, DataIoError> {
    logged("import.csv", path, meditate_core::data_io::parse_csv(path))
}

/// Read an Insight Timer export without writing anything. Its
/// local-time conversion is core's, DST-safe.
pub fn parse_insighttimer(path: &Path) -> Result<ParsedImport, DataIoError> {
    logged(
        "import.insighttimer",
        path,
        meditate_core::data_io::parse_insighttimer_csv(path, meditate_core::data_io::insighttimer_started_at),
    )
}

fn logged(
    tag: &str,
    path: &Path,
    result: Result<ParsedImport, meditate_core::data_io::DataIoError>,
) -> Result<ParsedImport, DataIoError> {
    match &result {
        Ok(p) => meditate_core::log(
            tag,
            &format!("read rows={} unreadable={} path={}", p.rows.len(), p.unreadable.len(), path.display()),
        ),
        Err(e) => meditate_core::log(tag, &format!("FAILED path={} err={e}", path.display())),
    }
    Ok(result?)
}

/// Write a read file: all of it, or on an error nothing.
pub fn import_parsed(app: &MeditateApplication, parsed: &ParsedImport) -> Result<usize, DataIoError> {
    let result: Result<usize, DataIoError> = app
        .with_db_mut(|db| meditate_core::data_io::import_parsed(db.core(), parsed))
        .ok_or(DataIoError::NoDatabase)?
        .map_err(DataIoError::from);
    match &result {
        Ok(n) => meditate_core::log("import.write", &format!("wrote sessions={n}")),
        Err(e) => meditate_core::log("import.write", &format!("FAILED err={e}")),
    }
    result
}

// ── Delete all ────────────────────────────────────────────────────────────────

pub fn delete_all(app: &MeditateApplication) -> Result<usize, DataIoError> {
    app.with_db_mut(super::db::Database::delete_all_sessions)
        .ok_or(DataIoError::NoDatabase)?
        .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    /// Local-time conversion lives in core (`insighttimer_started_at`,
    /// built on `time::local_naive_to_unix`), which imports a row in a
    /// DST gap instead of rounding or failing; the shell must not keep
    /// its own conversion.
    /// A file is read before anything is written. One with unreadable
    /// lines is imported in part only when the user says so: the
    /// question names the first such line, and Cancel writes nothing.
    #[test]
    fn a_partly_unreadable_file_asks_before_importing() {
        let src = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/preferences.rs"),
        )
        .unwrap();
        let at = src.find("fn open_import_dialog<F>(").unwrap();
        let body = &src[at..at + src[at..].find("\n}\n").unwrap()];
        assert!(body.contains("let Some(question) = parsed.question() else {\n                write_import(&app, &dialog, &parsed);"));
        assert!(body.contains("alert.set_response_enabled(\"import\", question.can_import);"));
        assert!(body.contains("alert.connect_response(Some(\"import\")"));
        assert_eq!(src.matches("data_io::import_parsed(").count(), 1, "one write");
    }

    #[test]
    fn insight_timer_import_uses_the_core_time_conversion() {
        let src = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/data_io.rs"),
        )
        .unwrap();
        let code = src.split("#[cfg(test)]\nmod tests").next().unwrap();
        assert!(code.contains("meditate_core::data_io::insighttimer_started_at"));
        assert!(!code.contains("DateTime::new"), "no shell-side local-time conversion");
    }
}
