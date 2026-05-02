//! CSV import / export for session data.
//!
//! Native format (the one `export_csv` writes + `import_csv` reads):
//! ```csv
//! start_time_unix,duration_secs,mode,label,note
//! 1712345678,600,timer,Morning,First sit of the day
//! ```
//! - `start_time_unix`: UTC seconds since epoch.
//! - `duration_secs`: integer seconds.
//! - `mode`: "timer" (countdowns + open-ended runs) or "box_breath".
//! - `label`: plain text — empty means no label. Labels are looked up or
//!   created by name on import, so ids are not persisted.
//! - `note`: optional free text (csv-quoted as needed).
//!
//! Insight Timer format (what `import_insighttimer` reads):
//! ```csv
//! Started At,Duration,Preset,Activity
//! 04/20/2026 08:21:14,0:45:0,,Meditation
//! ```
//! - `Started At` is parsed as local time (no timezone in the file).
//! - `Duration` is `H:M:S`.
//! - `Activity` becomes the label name.

use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use crate::application::MeditateApplication;
use crate::db::{Database, SessionData, SessionFilter, SessionMode};

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

/// Suggested filename for an export, e.g. `meditate-backup-2026-04-20_142030.csv`.
pub fn suggested_export_filename() -> String {
    let now = crate::time::now_local();
    let ts  = now.format("%Y-%m-%d_%H%M%S")
        .map(|s| s.to_string())
        .unwrap_or_else(|_| "unknown".to_string());
    format!("meditate-backup-{ts}.csv")
}

// ── Export ────────────────────────────────────────────────────────────────────

/// Write every session in the DB to `path` as CSV. Returns how many rows
/// were written.
pub fn export_csv(app: &MeditateApplication, path: &Path) -> Result<usize, DataIoError> {
    let result = app.with_db(|db| export_csv_to_db(db, path))
        .ok_or(DataIoError::NoDatabase)?;
    match &result {
        Ok(n) => crate::diag::log(&format!("export_csv: wrote {n} sessions to {}", path.display())),
        Err(e) => crate::diag::log(&format!("export_csv FAILED to {}: {e}", path.display())),
    }
    result
}

/// Core of `export_csv` against an open `Database`. Split out so the full
/// export → import round-trip is testable without a live `GApplication`.
pub(crate) fn export_csv_to_db(db: &Database, path: &Path) -> Result<usize, DataIoError> {
    let labels: std::collections::HashMap<i64, String> = db.list_labels()?
        .into_iter()
        .map(|l| (l.id, l.name))
        .collect();

    let file = File::create(path)?;
    let mut wtr = csv::Writer::from_writer(file);
    wtr.write_record(["start_time_unix", "duration_secs", "mode", "label", "note"])?;

    // list_sessions returns start_iso DESC; reverse so the CSV is
    // start-time ascending, matching what users expect when opening
    // a backup file in chronological order.
    let mut sessions = db.list_sessions(&SessionFilter::default())?;
    sessions.reverse();
    let mut n = 0usize;
    for s in &sessions {
        let label = s.label_id
            .and_then(|id| labels.get(&id).cloned())
            .unwrap_or_default();
        let note = s.note.clone().unwrap_or_default();
        wtr.write_record([
            s.start_time.to_string(),
            s.duration_secs.to_string(),
            s.mode.as_db_str().to_string(),
            label,
            note,
        ])?;
        n += 1;
    }
    wtr.flush()?;
    Ok(n)
}

// ── Native-format import ──────────────────────────────────────────────────────

pub fn import_csv(app: &MeditateApplication, path: &Path) -> Result<usize, DataIoError> {
    let result = app.with_db_mut(|db| import_csv_to_db(db, path))
        .ok_or(DataIoError::NoDatabase)?;
    match &result {
        Ok(n) => crate::diag::log(&format!("import_csv: read {n} sessions from {}", path.display())),
        Err(e) => crate::diag::log(&format!("import_csv FAILED from {}: {e}", path.display())),
    }
    result
}

/// Core of `import_csv` against an open `Database`. Split out for the
/// round-trip test.
pub(crate) fn import_csv_to_db(db: &Database, path: &Path) -> Result<usize, DataIoError> {
    let file = File::open(path)?;
    let mut rdr = csv::Reader::from_reader(BufReader::new(file));

    // Pull every row into memory first so the whole import happens inside
    // a single DB transaction.
    let mut label_names: Vec<String> = Vec::new();
    let mut rows: Vec<(i64, i64, SessionMode, Option<String>, usize)> = Vec::new();

    for (i, record) in rdr.records().enumerate() {
        let rec = record?;
        let line = i + 2;
        let start_time: i64 = rec.get(0)
            .and_then(|s| s.trim().parse().ok())
            .ok_or_else(|| DataIoError::Parse(format!("line {line}: bad start_time_unix")))?;
        let duration_secs: i64 = rec.get(1)
            .and_then(|s| s.trim().parse().ok())
            .ok_or_else(|| DataIoError::Parse(format!("line {line}: bad duration_secs")))?;
        if duration_secs <= 0 {
            return Err(DataIoError::Parse(
                format!("line {line}: duration_secs must be positive, got {duration_secs}")));
        }
        // Unknown / typo'd mode values default to Timer — that
        // preserves the row rather than discarding it on import.
        let mode = SessionMode::from_db_str(rec.get(2).map(|s| s.trim()).unwrap_or(""))
            .unwrap_or(SessionMode::Timer);
        let label_txt = rec.get(3).map(|s| s.trim().to_string()).unwrap_or_default();
        let note_txt = rec.get(4).map(|s| s.trim().to_string()).unwrap_or_default();
        let note = if note_txt.is_empty() { None } else { Some(note_txt) };

        // Resolve labels to ids in a second pass once we know the full set.
        // Match case-insensitively so the CSV can't split one logical label
        // into two DB rows.
        let label_idx = if label_txt.is_empty() {
            usize::MAX
        } else {
            let lower = label_txt.to_lowercase();
            label_names.iter().position(|n| n.to_lowercase() == lower).unwrap_or_else(|| {
                label_names.push(label_txt.clone());
                label_names.len() - 1
            })
        };
        rows.push((start_time, duration_secs, mode, note, label_idx));
    }

    insert_sessions_with_labels(db, &label_names, &rows)
}

// ── Insight Timer import ──────────────────────────────────────────────────────

pub fn import_insighttimer(app: &MeditateApplication, path: &Path) -> Result<usize, DataIoError> {
    let result = app.with_db_mut(|db| import_insighttimer_to_db(db, path))
        .ok_or(DataIoError::NoDatabase)?;
    match &result {
        Ok(n) => crate::diag::log(&format!("import_insighttimer: read {n} sessions from {}", path.display())),
        Err(e) => crate::diag::log(&format!("import_insighttimer FAILED from {}: {e}", path.display())),
    }
    result
}

pub(crate) fn import_insighttimer_to_db(db: &Database, path: &Path) -> Result<usize, DataIoError> {
    let file = File::open(path)?;
    let mut rdr = csv::Reader::from_reader(BufReader::new(file));

    let mut label_names: Vec<String> = Vec::new();
    let mut rows: Vec<(i64, i64, SessionMode, Option<String>, usize)> = Vec::new();

    for (i, record) in rdr.records().enumerate() {
        let rec = record?;
        let line = i + 2;
        let started_raw = rec.get(0).unwrap_or("").trim();
        let duration_raw = rec.get(1).unwrap_or("").trim();
        let activity = rec.get(3).unwrap_or("").trim().to_string();

        let start_time = parse_insighttimer_datetime(started_raw)
            .ok_or_else(|| DataIoError::Parse(
                format!("line {line}: can't parse 'Started At' {started_raw:?}")))?;
        let duration_secs = parse_hms_duration(duration_raw)
            .ok_or_else(|| DataIoError::Parse(
                format!("line {line}: can't parse 'Duration' {duration_raw:?}")))?;
        if duration_secs <= 0 {
            return Err(DataIoError::Parse(
                format!("line {line}: duration must be positive, got {duration_raw:?}")));
        }

        // Insight Timer doesn't record countdown-vs-stopwatch — treat
        // everything as countdown (the closer match: they picked a time).
        let label_idx = if activity.is_empty() {
            usize::MAX
        } else {
            let lower = activity.to_lowercase();
            label_names.iter().position(|n| n.to_lowercase() == lower).unwrap_or_else(|| {
                label_names.push(activity.clone());
                label_names.len() - 1
            })
        };
        rows.push((start_time, duration_secs, SessionMode::Timer, None, label_idx));
    }

    insert_sessions_with_labels(db, &label_names, &rows)
}

// ── Delete all ────────────────────────────────────────────────────────────────

pub fn delete_all(app: &MeditateApplication) -> Result<usize, DataIoError> {
    app.with_db_mut(|db| db.delete_all_sessions())
        .ok_or(DataIoError::NoDatabase)?
        .map_err(Into::into)
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Resolve the accumulated `label_names` to ids (creating missing labels)
/// and bulk-insert the `rows`. `usize::MAX` in the label-index column means
/// "no label".
fn insert_sessions_with_labels(
    db: &Database,
    label_names: &[String],
    rows: &[(i64, i64, SessionMode, Option<String>, usize)],
) -> Result<usize, DataIoError> {
    let mut label_ids: Vec<i64> = Vec::with_capacity(label_names.len());
    for name in label_names {
        label_ids.push(db.find_or_create_label(name)?);
    }
    let sessions: Vec<SessionData> = rows.iter()
        .map(|(start_time, duration_secs, mode, note, label_idx)| SessionData {
            start_time:    *start_time,
            duration_secs: *duration_secs,
            mode:          mode.clone(),
            label_id:      (*label_idx != usize::MAX).then(|| label_ids[*label_idx]),
            note:          note.clone(),
        })
        .collect();
    Ok(db.bulk_insert_sessions(&sessions)?)
}

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
        dt.hour() as i32, dt.minute() as i32, dt.second() as f64,
    ).ok()?;
    Some(glib_dt.to_unix())
}

/// Parse `H:M:S` (or `M:S`) into total seconds. Thin shim around
/// `meditate_core::format::parse_hms_duration` which handles the parsing
/// (incl. fractional seconds Insight Timer writes as `0:45:0` / `1:50:0`).
fn parse_hms_duration(s: &str) -> Option<i64> {
    meditate_core::format::parse_hms_duration(s).map(|d| d.as_secs() as i64)
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

    // ── Native CSV round-trip ────────────────────────────────────────────────

    #[test]
    fn csv_export_import_roundtrip_preserves_sessions() {
        use crate::db::{SessionFilter, test_db_in_memory};

        let db = test_db_in_memory();

        // Seed labels and three sessions covering the shape matrix:
        //   1) labeled + note      — normal case, plus CSV quoting on the note
        //   2) labeled + no note   — covers the `None` → empty-string branch
        //   3) no label + note     — covers the `Option<label_id>` None branch
        let morning = db.create_label("Morning").unwrap().id;
        let evening = db.create_label("Evening").unwrap().id;

        let originals = [
            SessionData {
                start_time: 1_712_000_000,
                duration_secs: 600,
                mode: SessionMode::Timer,
                label_id: Some(morning),
                // Commas and a quote to exercise CSV escaping on the note column.
                note: Some("first sit, \"nice\" focus".to_string()),
            },
            SessionData {
                start_time: 1_712_086_400,
                duration_secs: 1200,
                mode: SessionMode::Timer,
                label_id: Some(evening),
                note: None,
            },
            SessionData {
                start_time: 1_712_172_800,
                duration_secs: 300,
                mode: SessionMode::Timer,
                label_id: None,
                note: Some("no label on this one".to_string()),
            },
        ];
        for s in &originals {
            db.create_session(s).unwrap();
        }

        // Export to a tempfile, wipe sessions (keeping the labels so the
        // import's case-insensitive lookup resolves back to the same ids),
        // then import.
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let written = export_csv_to_db(&db, tmp.path()).unwrap();
        assert_eq!(written, originals.len());

        db.delete_all_sessions().unwrap();
        assert_eq!(db.count_sessions().unwrap(), 0);

        let imported = import_csv_to_db(&db, tmp.path()).unwrap();
        assert_eq!(imported, originals.len());

        // Pull the sessions back and compare. list_sessions returns them in
        // descending start_time order — reverse so we can index parallel to
        // `originals` which is ascending.
        let mut rows = db.list_sessions(&SessionFilter::default()).unwrap();
        rows.reverse();
        assert_eq!(rows.len(), originals.len());

        for (orig, got) in originals.iter().zip(rows.iter()) {
            assert_eq!(got.start_time, orig.start_time);
            assert_eq!(got.duration_secs, orig.duration_secs);
            assert_eq!(got.mode, orig.mode);
            assert_eq!(got.note, orig.note);
            assert_eq!(got.label_id, orig.label_id,
                "label_id mismatch: import should have resolved case-insensitively back to the same row");
        }
    }
}
