//! CSV import / export for session data.
//!
//! Native format (the one `export_csv` writes + `import_csv` reads):
//! ```csv
//! start_time_unix,duration_secs,mode,label,note
//! 1712345678,600,countdown,Morning,First sit of the day
//! ```
//! - `start_time_unix`: UTC seconds since epoch.
//! - `duration_secs`: integer seconds.
//! - `mode`: "countdown" or "stopwatch".
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
use crate::db::{Session, SessionData, SessionMode};

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
        match self {
            DataIoError::Io(e)    => write!(f, "File error: {e}"),
            DataIoError::Csv(e)   => write!(f, "CSV error: {e}"),
            DataIoError::Parse(m) => write!(f, "Parse error: {m}"),
            DataIoError::Db(m)    => write!(f, "Database error: {m}"),
            DataIoError::NoDatabase => write!(f, "Database unavailable"),
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
    let now = gtk::glib::DateTime::now_local().unwrap();
    let ts  = now.format("%Y-%m-%d_%H%M%S")
        .map(|s| s.to_string())
        .unwrap_or_else(|_| "unknown".to_string());
    format!("meditate-backup-{ts}.csv")
}

// ── Export ────────────────────────────────────────────────────────────────────

/// Write every session in the DB to `path` as CSV. Returns how many rows
/// were written.
pub fn export_csv(app: &MeditateApplication, path: &Path) -> Result<usize, DataIoError> {
    // Collect label names in one pass so the CSV can carry names, not ids.
    let labels: std::collections::HashMap<i64, String> = app
        .with_db(|db| db.list_labels())
        .ok_or(DataIoError::NoDatabase)??
        .into_iter()
        .map(|l| (l.id, l.name))
        .collect();

    let file = File::create(path)?;
    let mut wtr = csv::Writer::from_writer(file);
    wtr.write_record(["start_time_unix", "duration_secs", "mode", "label", "note"])?;

    let mut n = 0usize;
    let result: Result<(), DataIoError> = app
        .with_db(|db| -> Result<(), DataIoError> {
            db.for_each_session(|s: &Session| {
                let label = s.label_id
                    .and_then(|id| labels.get(&id).cloned())
                    .unwrap_or_default();
                let note = s.note.clone().unwrap_or_default();
                wtr.write_record([
                    s.start_time.to_string(),
                    s.duration_secs.to_string(),
                    s.mode.as_str().to_string(),
                    label,
                    note,
                ]).map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
                n += 1;
                Ok(())
            })?;
            Ok(())
        })
        .ok_or(DataIoError::NoDatabase)?;
    result?;
    wtr.flush()?;
    Ok(n)
}

// ── Native-format import ──────────────────────────────────────────────────────

pub fn import_csv(app: &MeditateApplication, path: &Path) -> Result<usize, DataIoError> {
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
        let mode = match rec.get(2).map(|s| s.trim()) {
            Some("stopwatch") => SessionMode::Stopwatch,
            _                 => SessionMode::Countdown,
        };
        let label_txt = rec.get(3).map(|s| s.trim().to_string()).unwrap_or_default();
        let note_txt = rec.get(4).map(|s| s.trim().to_string()).unwrap_or_default();
        let note = if note_txt.is_empty() { None } else { Some(note_txt) };

        // Resolve labels to ids in a second pass once we know the full set.
        let label_idx = if label_txt.is_empty() {
            usize::MAX
        } else {
            label_names.iter().position(|n| n == &label_txt).unwrap_or_else(|| {
                label_names.push(label_txt.clone());
                label_names.len() - 1
            })
        };
        rows.push((start_time, duration_secs, mode, note, label_idx));
    }

    insert_with_label_lookup(app, &label_names, &rows)
}

// ── Insight Timer import ──────────────────────────────────────────────────────

pub fn import_insighttimer(app: &MeditateApplication, path: &Path) -> Result<usize, DataIoError> {
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

        // Insight Timer doesn't record countdown-vs-stopwatch — treat
        // everything as countdown (the closer match: they picked a time).
        let label_idx = if activity.is_empty() {
            usize::MAX
        } else {
            label_names.iter().position(|n| n == &activity).unwrap_or_else(|| {
                label_names.push(activity.clone());
                label_names.len() - 1
            })
        };
        rows.push((start_time, duration_secs, SessionMode::Countdown, None, label_idx));
    }

    insert_with_label_lookup(app, &label_names, &rows)
}

// ── Delete all ────────────────────────────────────────────────────────────────

pub fn delete_all(app: &MeditateApplication) -> Result<usize, DataIoError> {
    app.with_db(|db| db.delete_all_sessions())
        .ok_or(DataIoError::NoDatabase)?
        .map_err(Into::into)
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Resolve the accumulated `label_names` to ids (creating missing labels)
/// and bulk-insert the `rows`. `usize::MAX` in the label-index column means
/// "no label".
fn insert_with_label_lookup(
    app: &MeditateApplication,
    label_names: &[String],
    rows: &[(i64, i64, SessionMode, Option<String>, usize)],
) -> Result<usize, DataIoError> {
    app.with_db(|db| -> Result<usize, DataIoError> {
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
    })
    .ok_or(DataIoError::NoDatabase)?
}

/// Parse `MM/DD/YYYY HH:MM:SS` as local time and return the unix timestamp.
/// Returns `None` on any format error.
fn parse_insighttimer_datetime(s: &str) -> Option<i64> {
    let (date_part, time_part) = s.split_once(' ')?;
    let mut date_bits = date_part.split('/');
    let month: i32 = date_bits.next()?.parse().ok()?;
    let day:   i32 = date_bits.next()?.parse().ok()?;
    let year:  i32 = date_bits.next()?.parse().ok()?;
    let mut time_bits = time_part.split(':');
    let hour:   i32 = time_bits.next()?.parse().ok()?;
    let minute: i32 = time_bits.next()?.parse().ok()?;
    let second: f64 = time_bits.next()?.parse().ok()?;
    let dt = gtk::glib::DateTime::new(
        &gtk::glib::TimeZone::local(),
        year, month, day, hour, minute, second,
    ).ok()?;
    Some(dt.to_unix())
}

/// Parse `H:M:S` (or `M:S`) into total seconds. Seconds can be fractional
/// — Insight Timer writes `0:45:0` / `1:50:0`.
fn parse_hms_duration(s: &str) -> Option<i64> {
    let parts: Vec<&str> = s.split(':').collect();
    match parts.len() {
        3 => {
            let h: i64 = parts[0].parse().ok()?;
            let m: i64 = parts[1].parse().ok()?;
            let sec: f64 = parts[2].parse().ok()?;
            Some(h * 3600 + m * 60 + sec.round() as i64)
        }
        2 => {
            let m: i64 = parts[0].parse().ok()?;
            let sec: f64 = parts[1].parse().ok()?;
            Some(m * 60 + sec.round() as i64)
        }
        _ => None,
    }
}
