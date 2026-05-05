//! End-to-end smoke test: runs a short countdown, records the session
//! to a real-file SQLite DB, then prints stats.
//!
//! Run with: `cargo run --bin record_smoke`
//! Override DB path: `MEDITATE_DB=/tmp/foo.db cargo run --bin record_smoke`

use meditate_core::db::{Database, Session, SessionMode};
use meditate_core::timer::{Countdown, CountdownTimer, Stopwatch};
use std::path::Path;
use std::thread::sleep;
use std::time::{Duration, Instant};

fn main() {
    let path =
        std::env::var("MEDITATE_DB").unwrap_or_else(|_| "/tmp/meditate-demo.db".to_string());
    let db = Database::open(Path::new(&path)).expect("open db");

    let session_start = chrono::Utc::now();
    let shell_origin = Instant::now();
    let now = || shell_origin.elapsed();

    let countdown = Countdown::new(
        CountdownTimer::new(Duration::from_secs(5)),
        Stopwatch::started_at(now()),
    );
    println!("Recording a 5-second meditation to {path}");
    println!();
    while !countdown.is_finished(now()) {
        let r = countdown.remaining(now());
        println!("  remaining: {}.{:03}s", r.as_secs(), r.subsec_millis());
        sleep(Duration::from_millis(500));
    }
    println!("  session complete");
    println!();

    let session = Session {
        start_iso: session_start.to_rfc3339(),
        duration_secs: 5,
        label_id: None,
        notes: None,
        mode: SessionMode::Timer,
        uuid: String::new(),
        guided_file_uuid: None,
    };
    db.insert_session(&session).expect("insert session");

    let today = chrono::Utc::now().naive_utc().date();
    println!("Stats:");
    println!("  Total sessions:  {}", db.count_sessions().unwrap());
    println!("  Total minutes:   {}", db.total_minutes().unwrap());
    println!("  Current streak:  {} days", db.get_streak(today).unwrap());
    println!("  Best streak:     {} days", db.get_best_streak().unwrap());

    let totals = db.get_daily_totals().unwrap();
    if !totals.is_empty() {
        println!("  Daily totals:");
        for (day, secs) in totals {
            println!("    {day}: {}m {}s", secs / 60, secs % 60);
        }
    }
}
