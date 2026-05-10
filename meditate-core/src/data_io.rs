//! CSV import / export for session data.
//!
//! Native format (the one `export_csv` writes + `import_csv` reads):
//! ```csv
//! start_time_unix,duration_secs,mode,label,note
//! 1712345678,600,timer,Morning,First sit of the day
//! ```
//! - `start_time_unix`: UTC seconds since epoch — bridges core's
//!   ISO-string `Session::start_iso` and shells' i64-unix domain via
//!   `crate::time::{unix_to_local_iso, local_iso_to_unix}`.
//! - `duration_secs`: integer seconds.
//! - `mode`: "timer" (countdowns + open-ended runs) or "box_breath".
//! - `label`: plain text — empty means no label. Labels are looked up or
//!   created by name on import, so ids are not persisted.
//! - `note`: optional free text (csv-quoted as needed).
//!
//! Insight Timer import lives in the GTK shell because it needs a
//! local-time → unix-timestamp conversion that uses a host datetime
//! API (`glib::DateTime`); the parser primitives
//! (`parse_insighttimer_datetime`, `parse_hms_duration`) live in
//! `crate::format`, and `insert_sessions_with_labels` is exposed
//! `pub` so the shell-side importer shares the second pass.

use crate::db::{Database, Session, SessionMode};
use crate::time::{local_iso_to_unix, unix_to_local_iso};
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

/// Everything that can go wrong during import or export, collapsed into a
/// single error type so the shell can show one toast. Display is
/// English; user-facing display goes through each shell's own error
/// type, which can localize via its own i18n stack (gettext in the GTK
/// shell, etc.).
#[derive(Debug)]
pub enum DataIoError {
    Io(std::io::Error),
    Csv(csv::Error),
    Parse(String),
    Db(String),
}

impl std::fmt::Display for DataIoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DataIoError::Io(e) => write!(f, "File error: {e}"),
            DataIoError::Csv(e) => write!(f, "CSV error: {e}"),
            DataIoError::Parse(m) => write!(f, "Parse error: {m}"),
            DataIoError::Db(m) => write!(f, "Database error: {m}"),
        }
    }
}

impl From<std::io::Error> for DataIoError {
    fn from(e: std::io::Error) -> Self {
        DataIoError::Io(e)
    }
}
impl From<csv::Error> for DataIoError {
    fn from(e: csv::Error) -> Self {
        DataIoError::Csv(e)
    }
}
impl From<rusqlite::Error> for DataIoError {
    fn from(e: rusqlite::Error) -> Self {
        DataIoError::Db(e.to_string())
    }
}

impl From<crate::db::DbError> for DataIoError {
    fn from(e: crate::db::DbError) -> Self {
        use crate::db::DbError;
        match e {
            DbError::Sqlite(err) => DataIoError::Db(err.to_string()),
            DbError::DuplicateLabel(name) => DataIoError::Db(format!("duplicate label: {name}")),
            DbError::DuplicatePreset(name) => DataIoError::Db(format!("duplicate preset: {name}")),
            DbError::DuplicateGuidedFile(name) => {
                DataIoError::Db(format!("duplicate guided file: {name}"))
            }
            DbError::DuplicateVibrationPattern(name) => {
                DataIoError::Db(format!("duplicate vibration pattern: {name}"))
            }
            DbError::Csv(msg) => DataIoError::Csv(csv::Error::from(std::io::Error::other(msg))),
        }
    }
}

// ── Export ──────────────────────────────────────────────────────────────

/// Write every session in the DB to `path` as CSV. Returns how many rows
/// were written.
pub fn export_csv(db: &Database, path: &Path) -> Result<usize, DataIoError> {
    let labels: std::collections::HashMap<i64, String> = db
        .list_labels()?
        .into_iter()
        .map(|l| (l.id, l.name))
        .collect();

    let file = File::create(path)?;
    let mut wtr = csv::Writer::from_writer(file);
    wtr.write_record(["start_time_unix", "duration_secs", "mode", "label", "note"])?;

    // list_sessions returns DESC start; reverse so the CSV is
    // start-time ascending, matching what users expect when opening
    // a backup file in chronological order.
    let mut sessions = db.list_sessions()?;
    sessions.reverse();
    let mut n = 0usize;
    for (_id, s) in &sessions {
        let label = s
            .label_id
            .and_then(|id| labels.get(&id).cloned())
            .unwrap_or_default();
        let note = s.notes.clone().unwrap_or_default();
        let start_unix = local_iso_to_unix(&s.start_iso);
        wtr.write_record([
            start_unix.to_string(),
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

// ── Import ──────────────────────────────────────────────────────────────

pub fn import_csv(db: &Database, path: &Path) -> Result<usize, DataIoError> {
    let file = File::open(path)?;
    let mut rdr = csv::Reader::from_reader(BufReader::new(file));

    // Pull every row into memory first so the whole import happens inside
    // a single DB transaction.
    let mut label_names: Vec<String> = Vec::new();
    let mut rows: Vec<(i64, u32, SessionMode, Option<String>, usize)> = Vec::new();

    for (i, record) in rdr.records().enumerate() {
        let rec = record?;
        let line = i + 2;
        let start_unix: i64 = rec
            .get(0)
            .and_then(|s| s.trim().parse().ok())
            .ok_or_else(|| DataIoError::Parse(format!("line {line}: bad start_time_unix")))?;
        let duration_secs: u32 = rec
            .get(1)
            .and_then(|s| s.trim().parse().ok())
            .ok_or_else(|| DataIoError::Parse(format!("line {line}: bad duration_secs")))?;
        if duration_secs == 0 {
            return Err(DataIoError::Parse(format!(
                "line {line}: duration_secs must be positive"
            )));
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
            label_names
                .iter()
                .position(|n| n.to_lowercase() == lower)
                .unwrap_or_else(|| {
                    label_names.push(label_txt.clone());
                    label_names.len() - 1
                })
        };
        rows.push((start_unix, duration_secs, mode, note, label_idx));
    }

    insert_sessions_with_labels(db, &label_names, &rows)
}

// ── Helpers ─────────────────────────────────────────────────────────────

/// Resolve the accumulated `label_names` to ids (creating missing labels)
/// and bulk-insert the `rows`. `usize::MAX` in the label-index column means
/// "no label". Each tuple in `rows` is
/// `(start_time_unix, duration_secs, mode, note, label_idx)`.
///
/// `pub` so the gtk shell's Insight Timer importer (which stays
/// shell-side because it needs a host datetime API for the local-time
/// conversion) shares the second pass without duplicating the vec walk.
pub fn insert_sessions_with_labels(
    db: &Database,
    label_names: &[String],
    rows: &[(i64, u32, SessionMode, Option<String>, usize)],
) -> Result<usize, DataIoError> {
    let mut label_ids: Vec<i64> = Vec::with_capacity(label_names.len());
    for name in label_names {
        label_ids.push(db.find_or_create_label(name)?);
    }
    let sessions: Vec<Session> = rows
        .iter()
        .map(|(start_unix, duration_secs, mode, note, label_idx)| Session {
            start_iso: unix_to_local_iso(*start_unix),
            duration_secs: *duration_secs,
            label_id: (*label_idx != usize::MAX).then(|| label_ids[*label_idx]),
            notes: note.clone(),
            mode: *mode,
            uuid: String::new(),
            guided_file_uuid: None,
        })
        .collect();
    Ok(db.bulk_insert_sessions(&sessions)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh_db() -> Database {
        Database::open_in_memory().unwrap()
    }

    #[test]
    fn csv_export_import_roundtrip_preserves_sessions() {
        let db = fresh_db();

        let morning = db.find_or_create_label("Morning").unwrap();
        let evening = db.find_or_create_label("Evening").unwrap();

        // Three sessions covering the shape matrix:
        //   1) labeled + note     — normal case, plus CSV quoting on the note
        //   2) labeled + no note  — covers the `None` → empty-string branch
        //   3) no label + note    — covers the `Option<label_id>` None branch
        let originals = [
            Session {
                start_iso: unix_to_local_iso(1_712_000_000),
                duration_secs: 600,
                mode: SessionMode::Timer,
                label_id: Some(morning),
                // Commas and a quote to exercise CSV escaping on the note column.
                notes: Some("first sit, \"nice\" focus".to_string()),
                uuid: String::new(),
                guided_file_uuid: None,
            },
            Session {
                start_iso: unix_to_local_iso(1_712_086_400),
                duration_secs: 1200,
                mode: SessionMode::Timer,
                label_id: Some(evening),
                notes: None,
                uuid: String::new(),
                guided_file_uuid: None,
            },
            Session {
                start_iso: unix_to_local_iso(1_712_172_800),
                duration_secs: 300,
                mode: SessionMode::Timer,
                label_id: None,
                notes: Some("no label on this one".to_string()),
                uuid: String::new(),
                guided_file_uuid: None,
            },
        ];
        for s in &originals {
            db.insert_session(s).unwrap();
        }

        // Export to a tempfile, wipe sessions (keeping the labels so the
        // import's case-insensitive lookup resolves back to the same ids),
        // then import.
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let written = export_csv(&db, tmp.path()).unwrap();
        assert_eq!(written, originals.len());

        db.delete_all_sessions().unwrap();
        assert_eq!(db.count_sessions().unwrap(), 0);

        let imported = import_csv(&db, tmp.path()).unwrap();
        assert_eq!(imported, originals.len());

        // Pull the sessions back and compare. list_sessions returns them in
        // descending start_iso order — reverse so we can index parallel to
        // `originals` which is ascending.
        let mut rows = db.list_sessions().unwrap();
        rows.reverse();
        assert_eq!(rows.len(), originals.len());

        for (orig, (_id, got)) in originals.iter().zip(rows.iter()) {
            // UUIDs are regenerated on import; everything else should
            // round-trip byte-for-byte through the unix↔ISO bridge.
            assert_eq!(got.start_iso, orig.start_iso);
            assert_eq!(got.duration_secs, orig.duration_secs);
            assert_eq!(got.mode, orig.mode);
            assert_eq!(got.notes, orig.notes);
            assert_eq!(
                got.label_id, orig.label_id,
                "label_id mismatch: import should have resolved case-insensitively back to the same row"
            );
        }
    }
}
