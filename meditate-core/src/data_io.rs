//! CSV import / export for session data.
//!
//! Native format (the one `export_csv` writes + `parse_csv` reads):
//! ```csv
//! start_time_unix,duration_secs,mode,label,note,uuid,start_local,label_uuid,guided_file_uuid
//! 1712345678,600,timer,Morning,First sit of the day,<uuid>,2024-04-05T21:34:38,<uuid>,
//! ```
//! - `start_time_unix`: UTC seconds since epoch, read when a file has
//!   no `start_local`.
//! - `duration_secs`: integer seconds.
//! - `mode`: "timer" (countdowns + open-ended runs) or "box_breath".
//! - `label`: plain text — empty means no label. Matched by name on
//!   import when no label with `label_uuid` is here.
//! - `note`: optional free text (csv-quoted as needed).
//! - `uuid`, `start_local`, `label_uuid`, `guided_file_uuid`: the
//!   session as stored, so a restored backup is the same data sync
//!   brings, not a copy of it, in any time zone. Last, so older
//!   versions still find the first five where they read them.
//!
//! An import reads the whole file first (`parse_csv`,
//! `parse_insighttimer_csv`), so the shell can ask before importing
//! only part of a file, and then writes it in one go (`import_parsed`).

use crate::db::{Database, LabelUuid, Session, SessionMode};
use crate::time::local_iso_to_unix;
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
            DbError::Decode(msg) => DataIoError::Csv(csv::Error::from(std::io::Error::other(msg))),
            DbError::SchemaVersionTooNew { db, build } => DataIoError::Db(format!(
                "db schema_version={db} exceeds build schema_version={build}"
            )),
            DbError::DateOutOfRange => DataIoError::Db("date out of range".to_string()),
        }
    }
}

// ── Export ──────────────────────────────────────────────────────────────

/// OWASP-recommended CSV-injection guard. Excel / LibreOffice /
/// Sheets treat a cell whose first character is `=`, `+`, `-`, `@`,
/// or TAB as a formula — a malicious or unwary user-supplied label
/// like `=HYPERLINK("http://evil/", "click")` would execute on
/// open. Prefix any such cell with a literal single quote, which
/// the spreadsheet renders as plain text and strips on copy.
fn csv_inject_guard(s: &str) -> String {
    match s.chars().next() {
        Some('=') | Some('+') | Some('-') | Some('@') | Some('\t') => {
            let mut out = String::with_capacity(s.len() + 1);
            out.push('\'');
            out.push_str(s);
            out
        }
        _ => s.to_string(),
    }
}

/// Undo `csv_inject_guard` on import: drop the `'` it put before a
/// formula character.
/// ponytail: a note typed as `'=x` comes back as `=x`; guarding `'`
/// itself on export would fix it, at the cost of a changed format.
fn csv_unguard(s: &str) -> &str {
    match s.strip_prefix('\'') {
        Some(rest) if rest.starts_with(['=', '+', '-', '@', '\t']) => rest,
        _ => s,
    }
}

/// Write every session in the DB to `path` as CSV. Returns how many rows
/// were written.
pub fn export_csv(db: &Database, path: &Path) -> Result<usize, DataIoError> {
    let labels: std::collections::HashMap<i64, (String, String)> =
        crate::db::list_labels_from_db(db)?
            .into_iter()
            .map(|l| (l.id, (l.name, l.uuid.to_string())))
            .collect();

    let file = File::create(path)?;
    let mut wtr = csv::Writer::from_writer(file);
    wtr.write_record([
        "start_time_unix", "duration_secs", "mode", "label", "note",
        "uuid", "start_local", "label_uuid", "guided_file_uuid",
    ])?;

    // Start-time ascending, as a backup is read in chronological order.
    let mut sessions = crate::db::list_sessions_from_db(db)?;
    sessions.sort_by_key(|(_, s)| local_iso_to_unix(&s.start_iso));
    let mut n = 0usize;
    for (_id, s) in &sessions {
        let (label, label_uuid) = s
            .label_id
            .and_then(|id| labels.get(&id).cloned())
            .unwrap_or_default();
        let note = s.notes.clone().unwrap_or_default();
        let start_unix = local_iso_to_unix(&s.start_iso);
        wtr.write_record([
            start_unix.to_string(),
            s.duration_secs.to_string(),
            s.mode.as_db_str().to_string(),
            csv_inject_guard(&label),
            csv_inject_guard(&note),
            s.uuid.to_string(),
            s.start_iso.clone(),
            label_uuid,
            s.guided_file_uuid.as_ref().map(ToString::to_string).unwrap_or_default(),
        ])?;
        n += 1;
    }
    wtr.flush()?;
    // Reclaim the underlying File from csv::Writer and fsync it. A
    // power loss after `flush` returns can otherwise leave a
    // truncated or zero-byte CSV — the buffered pages reach the
    // kernel but not the disk. `sync_all` blocks until the data +
    // metadata are durable, the standard exported-file contract.
    let file = wtr.into_inner().map_err(csv::IntoInnerError::into_error)?;
    file.sync_all()?;
    Ok(n)
}

// ── Import ──────────────────────────────────────────────────────────────

/// What could not be read in a line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unreadable {
    StartTime,
    Duration,
}

/// A file read for import, before anything is written.
#[derive(Debug, Default)]
pub struct ParsedImport {
    /// Label names (case-insensitive, first spelling wins), with the
    /// label's id when the file is a backup.
    pub labels: Vec<(String, Option<LabelUuid>)>,
    /// Each readable line: its session and the index of its label.
    pub rows: Vec<(Session, Option<usize>)>,
    /// The lines that could not be read, by their line in the file.
    pub unreadable: Vec<(usize, Unreadable)>,
}

/// What to ask before importing a file with unreadable lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImportQuestion {
    /// The first unreadable line, and what in it.
    pub line: usize,
    pub what: Unreadable,
    /// How many more lines can't be read.
    pub more: usize,
    /// Whether any line can be imported at all.
    pub can_import: bool,
}

impl ParsedImport {
    /// `None` when every line was read: import without asking.
    pub fn question(&self) -> Option<ImportQuestion> {
        let &(line, what) = self.unreadable.first()?;
        Some(ImportQuestion {
            line,
            what,
            more: self.unreadable.len() - 1,
            can_import: !self.rows.is_empty(),
        })
    }

    /// The index of label `name`, added on first sight. Matched
    /// case-insensitively so the file can't split one label in two.
    fn label(&mut self, name: &str, uuid: Option<LabelUuid>) -> Option<usize> {
        if name.is_empty() {
            return None;
        }
        let lower = name.to_lowercase();
        let at = self.labels.iter().position(|(n, _)| n.to_lowercase() == lower);
        Some(at.unwrap_or_else(|| {
            self.labels.push((name.to_string(), uuid));
            self.labels.len() - 1
        }))
    }
}

/// The record's line in the file: a quoted note can span lines.
fn line_of(rec: &csv::StringRecord, index: usize) -> usize {
    rec.position().map_or(index + 2, |p| p.line() as usize)
}

/// A field that is a valid id, as written.
fn an_id(s: &str) -> Option<String> {
    uuid::Uuid::parse_str(s).is_ok().then(|| s.to_string())
}

/// A stored start time (`YYYY-MM-DDTHH:MM:SS`), as the DB keeps it.
fn a_local_iso(s: &str) -> Option<String> {
    use chrono::Datelike;
    chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S")
        .ok()
        .filter(|n| (0..=9999).contains(&n.year()))
        .map(|n| n.format("%Y-%m-%dT%H:%M:%S").to_string())
}

fn reader(path: &Path) -> Result<csv::Reader<BufReader<File>>, DataIoError> {
    // Flexible: a short line is an unreadable line, not a failed file.
    Ok(csv::ReaderBuilder::new().flexible(true).from_reader(BufReader::new(File::open(path)?)))
}

/// Read a file in the native format (a backup) without writing.
pub fn parse_csv(path: &Path) -> Result<ParsedImport, DataIoError> {
    let mut out = ParsedImport::default();
    for (i, record) in reader(path)?.records().enumerate() {
        let rec = record?;
        let line = line_of(&rec, i);
        let field = |n: usize| rec.get(n).map_or("", str::trim);
        // A backup's stored start time is used as written; older files
        // only have the unix one.
        let start_iso = match field(6) {
            "" => field(0).parse().ok().and_then(crate::time::unix_to_local_iso_checked),
            local => a_local_iso(local),
        };
        let Some(start_iso) = start_iso else {
            out.unreadable.push((line, Unreadable::StartTime));
            continue;
        };
        let Ok(duration_secs) = field(1).parse::<u32>() else {
            out.unreadable.push((line, Unreadable::Duration));
            continue;
        };
        // A crash in a session's first minute left 0 s: no session.
        if duration_secs == 0 {
            continue;
        }
        // Unknown / typo'd mode values default to Timer — that
        // preserves the row rather than discarding it on import.
        let mode = SessionMode::from_db_str(field(2)).unwrap_or(SessionMode::Timer);
        let label_name = rec.get(3).map(|s| csv_unguard(s).trim().to_string()).unwrap_or_default();
        let label = out.label(&label_name, an_id(field(7)).map(LabelUuid::new));
        // Notes keep their whitespace; a blank one is stored as none.
        let note = rec.get(4).map(|s| csv_unguard(s).to_string()).unwrap_or_default();
        let session = Session {
            start_iso,
            duration_secs,
            label_id: None,
            notes: (!note.trim().is_empty()).then_some(note),
            mode,
            uuid: crate::db::SessionUuid::new(an_id(field(5)).unwrap_or_default()),
            guided_file_uuid: an_id(field(8)).map(crate::db::GuidedFileUuid::new),
        };
        out.rows.push((session, label));
    }
    Ok(out)
}

/// Write a read file in one go: all of it, or on an error nothing.
/// A line whose session is already in the log (by id, or by start and
/// duration) is skipped, so a backup imported twice, or restored next
/// to the synced log, never doubles it. Returns the sessions written.
pub fn import_parsed(db: &Database, parsed: &ParsedImport) -> Result<usize, DataIoError> {
    Ok(db.insert_imported(&parsed.labels, &parsed.rows)?)
}

// ── Insight Timer import ────────────────────────────────────────────────

/// An Insight Timer "Started At" cell → unix seconds, read as local
/// time on this device. Format detection is
/// `format::parse_insighttimer_datetime`; the local-time conversion is
/// `time::local_naive_to_unix`, so a row in a DST gap or overlap
/// still imports. Both shells pass this to `parse_insighttimer_csv`.
pub fn insighttimer_started_at(s: &str) -> Option<i64> {
    crate::format::parse_insighttimer_datetime(s).map(crate::time::local_naive_to_unix)
}

/// `insighttimer_started_at` with the time-zone lookup passed in.
#[cfg(test)]
fn insighttimer_started_at_with(
    s: &str,
    lookup: impl Fn(chrono::NaiveDateTime) -> chrono::LocalResult<chrono::DateTime<chrono::FixedOffset>>,
) -> Option<i64> {
    crate::format::parse_insighttimer_datetime(s)
        .map(|n| crate::time::naive_to_unix_with(n, lookup))
}

/// Read an Insight Timer CSV export without writing. Columns:
///   col 0: "Started At", a local time read by `parse_dt` (both
///          shells pass `insighttimer_started_at`, tests a stand-in).
///   col 1: "Duration", an HMS string for `parse_hms_duration`.
///   col 3: "Activity", the session's label; empty means none.
/// Insight Timer doesn't record countdown-vs-stopwatch: everything is
/// a Timer session (the closer match: they picked a time).
pub fn parse_insighttimer_csv<F>(path: &Path, parse_dt: F) -> Result<ParsedImport, DataIoError>
where
    F: Fn(&str) -> Option<i64>,
{
    let mut out = ParsedImport::default();
    for (i, record) in reader(path)?.records().enumerate() {
        let rec = record?;
        let line = line_of(&rec, i);
        let field = |n: usize| rec.get(n).map_or("", str::trim);
        let Some(start_iso) = parse_dt(field(0)).and_then(crate::time::unix_to_local_iso_checked) else {
            out.unreadable.push((line, Unreadable::StartTime));
            continue;
        };
        let Some(duration_secs) = crate::format::parse_hms_duration(field(1))
            .and_then(|d| u32::try_from(d.as_secs()).ok())
        else {
            out.unreadable.push((line, Unreadable::Duration));
            continue;
        };
        if duration_secs == 0 {
            continue;
        }
        let label = out.label(field(3), None);
        let session = Session {
            start_iso,
            duration_secs,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
        };
        out.rows.push((session, label));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::unix_to_local_iso;
    use std::io::Write;

    fn write_csv(contents: &str) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(contents.as_bytes()).unwrap();
        f.flush().unwrap();
        f
    }

    #[test]
    fn an_insighttimer_row_in_the_skipped_dst_hour_still_imports() {
        // 02:30 on the spring-forward night never happened on the
        // clock; the row must import (shifted to 03:30), not abort
        // the whole file.
        let csv = "Started At,Duration,Type,Activity\n\
                   03/28/2026 22:00:00,00:10:00,T,Meditation\n\
                   03/29/2026 02:30:00,00:20:00,T,Meditation\n\
                   03/29/2026 08:00:00,00:15:00,T,Meditation\n";
        let f = write_csv(csv);
        let p = parse_insighttimer_csv(f.path(), |s| {
            insighttimer_started_at_with(s, crate::time::test_zone::berlin)
        })
        .unwrap();
        assert_eq!(p.rows.len(), 3);
        let shifted = chrono::NaiveDate::from_ymd_opt(2026, 3, 29)
            .unwrap()
            .and_hms_opt(3, 30, 0)
            .unwrap()
            .and_utc()
            .timestamp()
            - 7200;
        assert_eq!(p.rows[1].0.start_iso, unix_to_local_iso(shifted));
    }

    #[test]
    fn an_insighttimer_row_in_the_doubled_dst_hour_takes_the_first_one() {
        let csv = "Started At,Duration,Type,Activity\n\
                   10/25/2026 02:30:00,00:10:00,T,Meditation\n";
        let f = write_csv(csv);
        let p = parse_insighttimer_csv(f.path(), |s| {
            insighttimer_started_at_with(s, crate::time::test_zone::berlin)
        })
        .unwrap();
        let first = chrono::NaiveDate::from_ymd_opt(2026, 10, 25)
            .unwrap()
            .and_hms_opt(2, 30, 0)
            .unwrap()
            .and_utc()
            .timestamp()
            - 7200;
        assert_eq!(p.rows[0].0.start_iso, unix_to_local_iso(first));
    }

    #[test]
    fn insighttimer_started_at_reads_local_time_consistently() {
        // Exact value depends on the host zone; an ordinary time must
        // parse, repeat identically, and an hour later be 3600 s later.
        let a = insighttimer_started_at("04/21/2026 08:30:00").unwrap();
        assert_eq!(insighttimer_started_at("04/21/2026 08:30:00"), Some(a));
        assert_eq!(insighttimer_started_at("04/21/2026 09:30:00"), Some(a + 3600));
    }

    #[test]
    fn insighttimer_started_at_rejects_garbage() {
        for bad in ["", "04/21/2026", "2026-04-21 08:30:00", "xx/yy/zzzz 08:30:00", "04/21/2026 08:30", "13/21/2026 08:30:00"] {
            assert_eq!(insighttimer_started_at(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn an_unparseable_insighttimer_time_is_still_rejected() {
        assert_eq!(insighttimer_started_at("not a date"), None);
        assert_eq!(insighttimer_started_at_with("not a date", crate::time::test_zone::berlin), None);
    }

    /// A stub "Started At" reader: a distinct, valid time per text.
    fn stub_time(s: &str) -> Option<i64> {
        (!s.is_empty()).then(|| 1_700_000_000 + s.len() as i64 * 60)
    }

    #[test]
    fn parse_insighttimer_csv_dedupes_labels_case_insensitively() {
        // Two rows whose Activity differs only in case must land
        // under the same label name (the first spelling).
        let csv = "Started At,Duration,Type,Activity\n\
                   2024-04-17T08:00:00,00:10:00,T,Meditation\n\
                   2024-04-18T08:00:00,00:15:00,T,meditation\n";
        let f = write_csv(csv);
        let p = parse_insighttimer_csv(f.path(), stub_time).unwrap();
        assert_eq!(p.labels, vec![("Meditation".to_string(), None)]);
        assert_eq!(p.rows.len(), 2);
        assert_eq!(p.rows[0].1, Some(0));
        assert_eq!(p.rows[1].1, Some(0));
    }

    #[test]
    fn parse_insighttimer_csv_empty_activity_yields_no_label() {
        let csv = "Started At,Duration,Type,Activity\n\
                   2024-04-17T08:00:00,00:10:00,T,\n";
        let f = write_csv(csv);
        let p = parse_insighttimer_csv(f.path(), stub_time).unwrap();
        assert!(p.labels.is_empty());
        assert_eq!(p.rows[0].1, None);
    }

    #[test]
    fn zero_second_rows_are_skipped_not_fatal() {
        // A 0 s session (a crash in its first minute) failed the whole
        // import, and older backups contain them.
        let it = "Started At,Duration,Type,Activity\n\
                  2024-04-17T08:00:00,00:00:00,T,Meditation\n\
                  2024-04-18T08:00:00,00:10:00,T,Meditation\n";
        let p = parse_insighttimer_csv(write_csv(it).path(), stub_time).unwrap();
        assert_eq!((p.rows.len(), p.question()), (1, None));
        let ours = "start_time_unix,duration_secs,mode,label,note\n\
                    1700000000,0,timer,,\n\
                    1700003600,600,timer,,\n";
        let p = parse_csv(write_csv(ours).path()).unwrap();
        assert_eq!((p.rows.len(), p.question()), (1, None));
    }

    #[test]
    fn an_unreadable_insighttimer_time_is_listed_not_fatal() {
        let csv = "Started At,Duration,Type,Activity\n\
                   garbage,00:10:00,T,Meditation\n\
                   2024-04-18T08:00:00,00:10:00,T,Meditation\n";
        let p = parse_insighttimer_csv(write_csv(csv).path(), |s| (s != "garbage").then_some(1_700_000_000)).unwrap();
        assert_eq!(p.rows.len(), 1);
        assert_eq!(p.unreadable, [(2, Unreadable::StartTime)]);
    }

    fn fresh_db() -> Database {
        Database::open_in_memory().unwrap()
    }

    fn row(start_unix: i64, duration_secs: u32, mode: SessionMode, note: Option<&str>, label: Option<usize>) -> (Session, Option<usize>) {
        let session = Session {
            start_iso: unix_to_local_iso(start_unix),
            duration_secs,
            label_id: None,
            notes: note.map(str::to_string),
            mode,
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
        };
        (session, label)
    }

    fn parsed(labels: &[&str], rows: Vec<(Session, Option<usize>)>) -> ParsedImport {
        ParsedImport {
            labels: labels.iter().map(|n| ((*n).to_string(), None)).collect(),
            rows,
            unreadable: Vec::new(),
        }
    }

    /// Import a file that has no unreadable lines.
    fn import_file(db: &Database, path: &Path) -> usize {
        let p = parse_csv(path).unwrap();
        assert_eq!(p.question(), None);
        import_parsed(db, &p).unwrap()
    }

    #[test]
    fn a_restored_backup_keeps_ids_start_times_labels_and_guided_files() {
        // Rows got new ids on import, so a backup restored next to the
        // synced log doubled it (1,500 became 3,000), every label came
        // back as a "(conflict)" copy, and restored rows could wipe
        // the guided file shown under a session.
        let db = fresh_db();
        let morning = db.find_or_create_label("Morning").unwrap();
        let originals = [
            Session {
                start_iso: unix_to_local_iso(1_712_000_000),
                duration_secs: 600,
                mode: SessionMode::Timer,
                label_id: Some(morning),
                // Commas and a quote to exercise CSV escaping on the note column.
                notes: Some("first sit, \"nice\" focus".to_string()),
                uuid: crate::db::SessionUuid::new(""),
                guided_file_uuid: Some("6f1c6a54-3d8e-4c2a-9b1e-2f7d8a9c0b1d".into()),
            },
            Session {
                start_iso: unix_to_local_iso(1_712_086_400),
                duration_secs: 1200,
                mode: SessionMode::BoxBreath,
                label_id: None,
                notes: None,
                uuid: crate::db::SessionUuid::new(""),
                guided_file_uuid: None,
            },
        ];
        for s in &originals {
            db.insert_session(s).unwrap();
        }
        let tmp = tempfile::NamedTempFile::new().unwrap();
        assert_eq!(export_csv(&db, tmp.path()).unwrap(), originals.len());

        let fresh = fresh_db();
        assert_eq!(import_file(&fresh, tmp.path()), originals.len());
        let label_uuid = |db: &Database, id: Option<i64>| {
            id.map(|id| crate::db::list_labels_from_db(db).unwrap().into_iter().find(|l| l.id == id).unwrap().uuid)
        };
        let before = crate::db::list_sessions_from_db(&db).unwrap();
        let after = crate::db::list_sessions_from_db(&fresh).unwrap();
        assert_eq!(after.len(), before.len());
        for ((_, want), (_, got)) in before.iter().zip(&after) {
            assert_eq!(got.uuid, want.uuid);
            assert_eq!(got.start_iso, want.start_iso);
            assert_eq!(got.duration_secs, want.duration_secs);
            assert_eq!(got.mode, want.mode);
            assert_eq!(got.notes, want.notes);
            assert_eq!(got.guided_file_uuid, want.guided_file_uuid);
            assert_eq!(label_uuid(&fresh, got.label_id), label_uuid(&db, want.label_id));
        }
    }

    #[test]
    fn older_versions_still_find_their_five_columns_first() {
        let db = fresh_db();
        let tmp = tempfile::NamedTempFile::new().unwrap();
        export_csv(&db, tmp.path()).unwrap();
        let header = std::fs::read_to_string(tmp.path()).unwrap();
        assert!(header.starts_with("start_time_unix,duration_secs,mode,label,note,"), "{header}");
    }

    #[test]
    fn an_old_five_column_backup_still_imports() {
        let csv = "start_time_unix,duration_secs,mode,label,note\n\
                   1700000000,600,timer,Morning,calm\n";
        let db = fresh_db();
        assert_eq!(import_file(&db, write_csv(csv).path()), 1);
        let (_, s) = crate::db::list_sessions_from_db(&db).unwrap().remove(0);
        assert_eq!(s.start_iso, unix_to_local_iso(1_700_000_000));
        assert!(!s.uuid.is_empty(), "a new id");
    }

    #[test]
    fn the_stored_start_time_is_used_as_written() {
        // The unix column moved every start by the time-zone change
        // between export and import (07:00 in Berlin became 01:00).
        let csv = "start_time_unix,duration_secs,mode,label,note,uuid,start_local,label_uuid,guided_file_uuid\n\
                   1,600,timer,,,,2026-03-01T07:00:00,,\n";
        let db = fresh_db();
        assert_eq!(import_file(&db, write_csv(csv).path()), 1);
        assert_eq!(crate::db::list_sessions_from_db(&db).unwrap()[0].1.start_iso, "2026-03-01T07:00:00");
    }

    #[test]
    fn a_session_already_here_is_skipped_by_its_id() {
        // Even when it was edited since the backup: same session.
        let db = fresh_db();
        db.insert_session(&row(1_700_000_000, 600, SessionMode::Timer, None, None).0).unwrap();
        let tmp = tempfile::NamedTempFile::new().unwrap();
        export_csv(&db, tmp.path()).unwrap();
        let (id, mut s) = crate::db::list_sessions_from_db(&db).unwrap().remove(0);
        s.duration_secs = 900;
        db.update_session(id, &s).unwrap();
        assert_eq!(import_file(&db, tmp.path()), 0);
        assert_eq!(crate::db::count_sessions_from_db(&db).unwrap(), 1);
    }

    #[test]
    fn an_id_repeated_in_one_file_imports_once() {
        let id = "0c6b2f7e-7f4a-4d3e-9a51-3e2b1c0d9f8a";
        let csv = format!(
            "start_time_unix,duration_secs,mode,label,note,uuid,start_local,label_uuid,guided_file_uuid\n\
             1700000000,600,timer,,,{id},,,\n\
             1700003600,900,timer,,,{id},,,\n"
        );
        let db = fresh_db();
        assert_eq!(import_file(&db, write_csv(&csv).path()), 1);
    }

    #[test]
    fn a_millisecond_start_time_is_unreadable_not_1970() {
        // It became 1970-01-01, and the duplicate check then kept one
        // row per duration while reporting success.
        let csv = "start_time_unix,duration_secs,mode,label,note\n\
                   1700000000000,600,timer,,\n\
                   1700003600000,600,timer,,\n";
        let p = parse_csv(write_csv(csv).path()).unwrap();
        assert_eq!(p.unreadable, [(2, Unreadable::StartTime), (3, Unreadable::StartTime)]);
        assert!(p.rows.is_empty());
    }

    #[test]
    fn an_unreadable_line_is_named_by_its_line_in_the_file() {
        // The record index ignored the newline inside a quoted note.
        let csv = "start_time_unix,duration_secs,mode,label,note\n\
                   1700000000,600,timer,,\"two\nlines\"\n\
                   1700003600,abc,timer,,\n";
        let p = parse_csv(write_csv(csv).path()).unwrap();
        assert_eq!(p.unreadable, [(4, Unreadable::Duration)]);
    }

    #[test]
    fn readable_lines_wait_while_the_question_says_what_was_not() {
        let csv = "start_time_unix,duration_secs,mode,label,note\n\
                   1700000000,600,timer,,\n\
                   x,600,timer,,\n\
                   1700007200,,timer,,\n\
                   1700010800,600,timer,,\n";
        let p = parse_csv(write_csv(csv).path()).unwrap();
        assert_eq!(p.rows.len(), 2);
        assert_eq!(
            p.question(),
            Some(ImportQuestion { line: 3, what: Unreadable::StartTime, more: 1, can_import: true })
        );
        let none = "start_time_unix,duration_secs,mode,label,note\nx,600,timer,,\n";
        let p = parse_csv(write_csv(none).path()).unwrap();
        assert_eq!(p.question().map(|q| q.can_import), Some(false));
    }

    #[test]
    fn a_reimport_creates_no_label_for_rows_it_skips() {
        // Labels were made for every name in the file before the
        // duplicate check: a deleted "Work" came back empty.
        let db = fresh_db();
        import_parsed(&db, &parsed(&["Work"], vec![row(1_700_000_000, 600, SessionMode::Timer, None, Some(0))])).unwrap();
        let tmp = tempfile::NamedTempFile::new().unwrap();
        export_csv(&db, tmp.path()).unwrap();
        let work = crate::db::list_labels_from_db(&db).unwrap()[0].id;
        db.delete_label(work).unwrap();
        assert_eq!(import_file(&db, tmp.path()), 0);
        assert!(crate::db::list_labels_from_db(&db).unwrap().iter().all(|l| l.name != "Work"));
    }

    #[test]
    fn csv_inject_guard_prefixes_formula_starters() {
        // The five characters Excel / LibreOffice / Sheets treat as
        // formula starters when at the head of a cell.
        assert_eq!(csv_inject_guard("=HYPERLINK(\"a\",\"b\")"), "'=HYPERLINK(\"a\",\"b\")");
        assert_eq!(csv_inject_guard("+1+1"), "'+1+1");
        assert_eq!(csv_inject_guard("-2+2"), "'-2+2");
        assert_eq!(csv_inject_guard("@SUM(A1:A9)"), "'@SUM(A1:A9)");
        assert_eq!(csv_inject_guard("\tmischief"), "'\tmischief");
    }

    #[test]
    fn csv_inject_guard_leaves_normal_cells_alone() {
        // Plain text, leading digit, leading-space — all benign as
        // first chars; no prefix added.
        assert_eq!(csv_inject_guard("Morning sit"), "Morning sit");
        assert_eq!(csv_inject_guard("4 minutes in"), "4 minutes in");
        assert_eq!(csv_inject_guard(" leading space"), " leading space");
        assert_eq!(csv_inject_guard(""), "");
    }

    // ── Import dedupe ────────────────────────────────────────────

    #[test]
    fn reimporting_the_same_rows_inserts_nothing() {
        // The backup-restore foot-gun: importing rows that already
        // exist (same start + duration) must not duplicate them.
        let db = Database::open_in_memory().unwrap();
        let p = parsed(&[], vec![
            row(1_700_000_000, 600, SessionMode::Timer, None, None),
            row(1_700_010_000, 900, SessionMode::Timer, None, None),
        ]);
        assert_eq!(import_parsed(&db, &p).unwrap(), 2);
        assert_eq!(import_parsed(&db, &p).unwrap(), 0,
            "exact (start, duration) matches must be skipped");
        assert_eq!(crate::db::count_sessions_from_db(&db).unwrap(), 2);
    }

    #[test]
    fn duplicate_rows_within_one_batch_insert_once() {
        let db = Database::open_in_memory().unwrap();
        let p = parsed(&[], vec![
            row(1_700_000_000, 600, SessionMode::Timer, None, None),
            row(1_700_000_000, 600, SessionMode::Timer, None, None),
        ]);
        assert_eq!(import_parsed(&db, &p).unwrap(), 1);
    }

    #[test]
    fn same_start_different_duration_is_not_a_duplicate() {
        let db = Database::open_in_memory().unwrap();
        let p = parsed(&[], vec![
            row(1_700_000_000, 600, SessionMode::Timer, None, None),
            row(1_700_000_000, 601, SessionMode::Timer, None, None),
        ]);
        assert_eq!(import_parsed(&db, &p).unwrap(), 2);
    }

    #[test]
    fn guarded_cells_and_note_whitespace_survive_a_round_trip() {
        // The export's formula guard `'` came back as part of the
        // text, and import trimmed notes (#7).
        let db = fresh_db();
        let notes = ["- calm", "=x", "+y", "@z", "\tt", "  indented ", "plain"];
        let rows = notes
            .iter()
            .enumerate()
            .map(|(i, n)| row(1_700_000_000 + i as i64 * 3600, 600, SessionMode::Timer, Some(n), Some(0)))
            .collect();
        import_parsed(&db, &parsed(&["-Work"], rows)).unwrap();
        let f = tempfile::NamedTempFile::new().unwrap();
        export_csv(&db, f.path()).unwrap();

        let fresh = fresh_db();
        assert_eq!(import_file(&fresh, f.path()), notes.len());
        let mut got: Vec<String> = crate::db::list_sessions_from_db(&fresh)
            .unwrap()
            .into_iter()
            .map(|(_, s)| s.notes.unwrap_or_default())
            .collect();
        got.sort();
        let mut want: Vec<String> = notes.iter().map(|n| (*n).to_string()).collect();
        want.sort();
        assert_eq!(got, want);
        let labels = crate::db::list_labels_from_db(&fresh).unwrap();
        assert_eq!(labels.len(), 1, "one label, not a guarded copy");
        assert_eq!(labels[0].name, "-Work");
    }

    #[test]
    fn export_is_in_start_time_order() {
        // Rows came out by id, reversed (#36).
        let db = fresh_db();
        let rows = [1_700_020_000_i64, 1_700_000_000, 1_700_010_000]
            .iter()
            .map(|t| row(*t, 600, SessionMode::Timer, None, None))
            .collect();
        import_parsed(&db, &parsed(&[], rows)).unwrap();
        let f = tempfile::NamedTempFile::new().unwrap();
        export_csv(&db, f.path()).unwrap();
        let starts: Vec<i64> = csv::Reader::from_path(f.path())
            .unwrap()
            .records()
            .map(|r| r.unwrap()[0].parse().unwrap())
            .collect();
        assert_eq!(starts, [1_700_000_000, 1_700_010_000, 1_700_020_000]);
    }

    #[test]
    fn export_then_reimport_into_same_db_inserts_nothing() {
        // End-to-end user story: export your own log, re-import the
        // file into the same DB — the log must not double.
        let db = Database::open_in_memory().unwrap();
        let p = parsed(&["Sitting"], vec![
            row(1_700_000_000, 600, SessionMode::Timer, Some("note"), Some(0)),
            row(1_700_010_000, 1200, SessionMode::BoxBreath, None, None),
        ]);
        import_parsed(&db, &p).unwrap();
        let f = tempfile::NamedTempFile::new().unwrap();
        export_csv(&db, f.path()).unwrap();
        assert_eq!(import_file(&db, f.path()), 0,
            "re-import of an unmodified export must be a no-op");
        assert_eq!(crate::db::count_sessions_from_db(&db).unwrap(), 2);
    }
}
