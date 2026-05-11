//! Time helpers shared between shells.
//!
//! Two concerns live here:
//! - `boot_time_now()` — suspend-resilient monotonic time.
//! - `unix_to_local_iso` / `local_iso_to_unix` — the boundary between
//!   the i64-unix-timestamp domain shells use for ergonomics and the
//!   ISO-string format the DB stores ("naive local").
//!
//! Glib-bound helpers (e.g. `now_local() -> glib::DateTime`) stay in
//! the GTK shell; this module is pure chrono + libc.

/// Suspend-resilient monotonic time. Linux's `std::time::Instant` uses
/// CLOCK_MONOTONIC, which freezes during system suspend — a 30s suspend
/// in the middle of a session would silently lose 30s of countdown.
/// CLOCK_BOOTTIME counts time including suspend, which is what a meditation
/// timer wants: real wall-clock progress regardless of OS power state.
///
/// On Android, Rust's `Instant::now()` already uses CLOCK_BOOTTIME natively
/// since 1.79 — but a single shared helper that callers depend on means
/// every shell stays consistent regardless of which platform's std happens
/// to do the right thing.
pub fn boot_time_now() -> std::time::Duration {
    let mut ts: libc::timespec = unsafe { std::mem::zeroed() };
    let rc = unsafe { libc::clock_gettime(libc::CLOCK_BOOTTIME, &mut ts) };
    debug_assert_eq!(rc, 0, "clock_gettime(CLOCK_BOOTTIME) failed");
    std::time::Duration::new(ts.tv_sec as u64, ts.tv_nsec as u32)
}

/// Current wall-clock time as unix seconds (UTC). Defensive: a
/// system clock that reports a timestamp before the unix epoch
/// (theoretically possible on a misconfigured RTC) collapses to 0
/// rather than panicking.
pub fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

/// Today's local-time date as a `chrono::NaiveDate`. Thin alias
/// over `chrono::Local::now().date_naive()` — present so callers
/// don't all duplicate the chrono path + so the Android shell's
/// "today" reads from the same source.
pub fn today_local() -> chrono::NaiveDate {
    chrono::Local::now().date_naive()
}

/// Wall-clock nanos as a `u64`, suitable as an xorshift64 seed for
/// per-session bell jitter. Always returns ≥ 1 (xorshift64 outputs
/// 0 forever from a 0 seed, so we collapse a clock that somehow
/// reports the unix epoch to 1).
pub fn seed_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(1)
        .max(1)
}

/// Format a unix timestamp (UTC seconds since epoch) as a local-naive
/// ISO 8601 string `YYYY-MM-DDTHH:MM:SS`. The string represents the
/// wall-clock time the user would see on their device — no timezone
/// suffix because the DB convention is "naive local".
///
/// On TZ ambiguity (DST fall-back) or invalid input, returns the unix
/// epoch as ISO ("1970-01-01T00:00:00") rather than panicking — losing
/// a session timestamp is bad, crashing on the save path is worse.
pub fn unix_to_local_iso(unix_secs: i64) -> String {
    use chrono::TimeZone;
    chrono::Local
        .timestamp_opt(unix_secs, 0)
        .single()
        .map(|dt| dt.naive_local().format("%Y-%m-%dT%H:%M:%S").to_string())
        .unwrap_or_else(|| "1970-01-01T00:00:00".to_string())
}

/// Inverse of `unix_to_local_iso`: parse a local-naive ISO 8601 string
/// and return the corresponding unix timestamp.
///
/// Returns 0 (the unix epoch) on parse failure or DST ambiguity. The
/// "drop on bad input" policy mirrors `unix_to_local_iso` — a corrupt
/// timestamp shouldn't take down the log feed.
pub fn local_iso_to_unix(iso: &str) -> i64 {
    use chrono::TimeZone;
    let parsed = chrono::NaiveDateTime::parse_from_str(iso, "%Y-%m-%dT%H:%M:%S")
        .or_else(|_| chrono::NaiveDateTime::parse_from_str(iso, "%Y-%m-%d %H:%M:%S"));
    match parsed {
        Ok(naive) => chrono::Local
            .from_local_datetime(&naive)
            .single()
            .map(|dt| dt.timestamp())
            .unwrap_or(0),
        Err(_) => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── boot_time_now ──────────────────────────────────────────────────

    #[test]
    fn boot_time_now_is_monotonically_non_decreasing() {
        // Two consecutive reads must not go backwards. We can't assert
        // anything about the absolute value (varies by host), only the
        // monotonic invariant that drives every caller.
        let a = boot_time_now();
        let b = boot_time_now();
        assert!(b >= a, "second read {b:?} preceded first {a:?}");
    }

    #[test]
    fn boot_time_now_advances_across_a_real_sleep() {
        let before = boot_time_now();
        std::thread::sleep(std::time::Duration::from_millis(10));
        let after = boot_time_now();
        assert!(
            after.saturating_sub(before) >= std::time::Duration::from_millis(5),
            "did not advance across a 10ms sleep: before={before:?} after={after:?}"
        );
    }

    // ── unix_now ───────────────────────────────────────────────────────

    #[test]
    fn unix_now_is_in_the_plausible_present() {
        // If the host clock is sane this is somewhere between 2024-01-01
        // and 2100-01-01 unix seconds. Loose bounds because the test
        // can run any time and we only want to catch "clock returned
        // 0 / negative" garbage.
        let now = unix_now();
        assert!(now > 1_700_000_000, "unix_now reported {now}, before 2023-11-14");
        assert!(now < 4_000_000_000, "unix_now reported {now}, past 2096");
    }

    #[test]
    fn unix_now_is_monotonic_across_a_real_sleep() {
        let before = unix_now();
        std::thread::sleep(std::time::Duration::from_secs(1));
        let after = unix_now();
        assert!(
            after >= before + 1,
            "unix_now did not advance across a 1s sleep: before={before} after={after}"
        );
    }

    // ── unix_to_local_iso / local_iso_to_unix ──────────────────────────

    #[test]
    fn unix_to_local_iso_round_trips_through_local_iso_to_unix() {
        // Two conversions must be exact inverses for well-formed unix
        // timestamps away from DST transitions. Pick a handful of
        // representative values rather than every i64.
        for &secs in &[0i64, 1_000_000, 1_700_000_000, 1_800_000_000] {
            let iso = unix_to_local_iso(secs);
            let back = local_iso_to_unix(&iso);
            assert_eq!(back, secs, "round-trip failed for {secs}: iso={iso}, back={back}");
        }
    }

    #[test]
    fn unix_to_local_iso_produces_iso_8601_shape() {
        // Fixed-width YYYY-MM-DDTHH:MM:SS — exactly 19 chars, 'T' between
        // date and time. Lexicographic ordering is then chronological,
        // which several core queries (e.g. total_secs_since) depend on.
        let iso = unix_to_local_iso(1_700_000_000);
        assert_eq!(iso.len(), 19);
        assert_eq!(&iso[10..11], "T");
        assert_eq!(&iso[4..5], "-");
        assert_eq!(&iso[7..8], "-");
        assert_eq!(&iso[13..14], ":");
        assert_eq!(&iso[16..17], ":");
    }

    #[test]
    fn local_iso_to_unix_accepts_t_separator_and_space_separator() {
        // ISO standard uses 'T'; chrono's NaiveDateTime::Display uses
        // a space. Accept both so callers don't have to normalise.
        let with_t = local_iso_to_unix("2026-04-27T10:00:00");
        let with_space = local_iso_to_unix("2026-04-27 10:00:00");
        assert_eq!(with_t, with_space);
        assert_ne!(with_t, 0, "well-formed input must not collapse to the epoch sentinel");
    }

    #[test]
    fn local_iso_to_unix_returns_zero_for_garbage() {
        // Defensive on bad input: 0 sentinel rather than panic. Losing
        // a corrupt timestamp is a smaller failure than crashing the
        // log feed.
        assert_eq!(local_iso_to_unix(""), 0);
        assert_eq!(local_iso_to_unix("not a date"), 0);
        assert_eq!(local_iso_to_unix("2026-13-01T00:00:00"), 0); // bad month
        assert_eq!(local_iso_to_unix("2026-04-31T00:00:00"), 0); // April has 30 days
    }

    #[test]
    fn unix_to_local_iso_advances_by_one_hour_when_unix_advances_by_3600() {
        // Adjacent timestamps round-trip with the expected delta even
        // though we can't pin the absolute value (depends on host TZ).
        // The picked timestamp is far from DST transitions in any TZ.
        let a = unix_to_local_iso(1_700_000_000);
        let b = unix_to_local_iso(1_700_000_000 + 3600);
        let hour_a: u32 = a[11..13].parse().unwrap();
        let hour_b: u32 = b[11..13].parse().unwrap();
        // Hour wraps 0..24 — handle midnight crossing.
        let diff = (hour_b + 24 - hour_a) % 24;
        assert_eq!(diff, 1);
    }
}
