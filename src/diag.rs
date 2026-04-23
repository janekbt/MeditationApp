//! Tiny ring-buffer diagnostics log.
//!
//! Rationale: on a user's Librem 5 there's no terminal to tail stderr, so
//! issues the user reports come with zero local context. This module
//! appends boundary-event lines (startup, DB open result, import/export
//! result, panics) to `<data_dir>/diagnostics.log` and surfaces the file
//! via `AdwAboutDialog`'s Debug Info viewer (copy + save-to-file built in).
//!
//! Scope on purpose is tight:
//! - One severity level. `log(msg)` takes a string and writes `<ts> msg`.
//! - No structured fields, no per-frame logging, no per-session-save line.
//!   Frame-clock spam would make the tail useless for triage.
//! - Trim to the last 2000 lines on each `init()` — runs once per app
//!   launch, so the file grows within a session but can never exceed
//!   2000 lines across sessions. No timer-based rotation.
//! - Panic hook installed once at `init()`. Runs before the default hook
//!   (which prints to stderr and unwinds), so a crash shows up in the log
//!   even if the user never saw stderr.
//!
//! Thread safety: relies on Linux O_APPEND atomicity — each `writeln!` is
//! one atomic write at well under PIPE_BUF (4 KiB), so concurrent writes
//! from the main thread and the GIO blocking pool interleave at line
//! granularity without a mutex.

use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

const MAX_LINES: usize = 2000;

static LOG_PATH: OnceLock<PathBuf> = OnceLock::new();

/// Initialise the diag log at `<data_dir>/diagnostics.log`. Trims any
/// existing log to the last `MAX_LINES` lines, then installs a panic
/// hook that appends panic info before the default hook unwinds.
/// Idempotent — safe to call more than once (subsequent calls are no-ops).
pub fn init(data_dir: &Path) {
    if LOG_PATH.get().is_some() {
        return;
    }
    let _ = std::fs::create_dir_all(data_dir);
    let path = data_dir.join("diagnostics.log");
    trim_to_tail(&path, MAX_LINES);
    let _ = LOG_PATH.set(path);
    install_panic_hook();
}

/// Append a single line to the diag log. Cheap no-op if `init` hasn't
/// run or the file can't be opened — this is diagnostics, not business
/// logic, so failure to log must never fail the caller.
pub fn log(msg: &str) {
    let Some(path) = LOG_PATH.get() else { return; };
    let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) else {
        return;
    };
    let _ = writeln!(f, "{} {msg}", timestamp());
}

/// Return the full log as a single string, or an empty string if the
/// log isn't initialised / the file can't be read. Used to feed
/// `AdwAboutDialog::set_debug_info`.
pub fn read_all() -> String {
    let Some(path) = LOG_PATH.get() else { return String::new(); };
    std::fs::read_to_string(path).unwrap_or_default()
}

fn timestamp() -> String {
    glib::DateTime::now_local()
        .and_then(|dt| dt.format("%Y-%m-%d %H:%M:%S"))
        .map(|s| s.to_string())
        .unwrap_or_else(|_| {
            // Last-resort fallback when tzdata is missing / clock is broken:
            // raw unix seconds. Better to keep the line than skip it.
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| format!("@{}", d.as_secs()))
                .unwrap_or_else(|_| "?".to_string())
        })
}

/// Rewrite `path` to contain only its last `max_lines` lines. No-op if
/// the file doesn't exist yet or is already small enough. Writes via a
/// `.tmp` sibling + rename so a kill mid-trim doesn't corrupt the log.
fn trim_to_tail(path: &Path, max_lines: usize) {
    let Ok(file) = File::open(path) else { return; };
    let lines: Vec<String> = BufReader::new(file)
        .lines()
        .map_while(Result::ok)
        .collect();
    if lines.len() <= max_lines {
        return;
    }
    let keep_from = lines.len() - max_lines;
    let tmp = path.with_extension("log.tmp");
    let Ok(mut out) = File::create(&tmp) else { return; };
    for line in &lines[keep_from..] {
        if writeln!(out, "{line}").is_err() {
            return;
        }
    }
    let _ = std::fs::rename(&tmp, path);
}

fn install_panic_hook() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let loc = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_else(|| "<unknown>".to_string());
        let payload = info.payload();
        let msg = if let Some(s) = payload.downcast_ref::<&str>() {
            *s
        } else if let Some(s) = payload.downcast_ref::<String>() {
            s.as_str()
        } else {
            "<non-string panic payload>"
        };
        log(&format!("PANIC at {loc}: {msg}"));
        default(info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trim_tail_no_op_when_short() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.log");
        std::fs::write(&path, b"a\nb\nc\n").unwrap();
        trim_to_tail(&path, 100);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "a\nb\nc\n");
    }

    #[test]
    fn trim_tail_keeps_last_n_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.log");
        let mut body = String::new();
        for i in 0..50 {
            body.push_str(&format!("line {i}\n"));
        }
        std::fs::write(&path, body).unwrap();
        trim_to_tail(&path, 10);
        let kept = std::fs::read_to_string(&path).unwrap();
        let kept_lines: Vec<&str> = kept.lines().collect();
        assert_eq!(kept_lines.len(), 10);
        assert_eq!(kept_lines[0], "line 40");
        assert_eq!(kept_lines[9], "line 49");
    }

    #[test]
    fn log_without_init_is_noop() {
        // LOG_PATH unset → log() must silently drop. OnceLock is per-process,
        // so we can't robustly test the "set path then log" path here without
        // clobbering state shared with other tests. This test covers the
        // "never initialised" branch, which is what library consumers hit
        // when the data dir couldn't be created.
        log("should not panic and should not create a file");
    }
}
