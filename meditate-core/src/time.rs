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
/// NOTE: Rust's `Instant` is CLOCK_MONOTONIC on Linux *and Android*
/// (it does NOT switch to CLOCK_BOOTTIME) — an earlier comment here
/// claimed otherwise and the Android shell trusted `Instant`,
/// silently dropping suspended time (a ~34 min screen-off stopwatch
/// recorded ~15 min). Every shell — gtk and android — MUST source
/// `now` from this helper, never `Instant`.
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
        .map_or(1, |d| d.as_nanos() as u64)
        .max(1)
}

/// Format a unix timestamp (UTC seconds since epoch) as a local-naive
/// ISO 8601 string `YYYY-MM-DDTHH:MM:SS`. The string represents the
/// wall-clock time the user would see on their device — no timezone
/// suffix because the DB convention is "naive local".
///
/// On TZ ambiguity (DST fall-back), invalid input, or a year outside
/// 0000-9999, returns the unix epoch as ISO ("1970-01-01T00:00:00")
/// rather than panicking. Losing a session timestamp is bad, crashing
/// on the save path is worse — and emitting a year like "10000-..."
/// or "-262143-..." would silently break every SUBSTR-based stat
/// downstream (hour-of-day, day-of-week, etc. all assume the 19-char
/// canonical shape).
pub fn unix_to_local_iso(unix_secs: i64) -> String {
    use chrono::{Datelike, TimeZone};
    chrono::Local
        .timestamp_opt(unix_secs, 0)
        .single()
        .map(|dt| dt.naive_local())
        .filter(|naive| (0..=9999).contains(&naive.year())).map_or_else(|| "1970-01-01T00:00:00".to_string(), |naive| naive.format("%Y-%m-%dT%H:%M:%S").to_string())
}

/// Inverse of `unix_to_local_iso`: parse a local-naive ISO 8601 string
/// and return the corresponding unix timestamp.
///
/// On DST fall-back (a local time that occurs twice), the EARLIER of
/// the two unix candidates is returned — picking either is wrong by
/// up to one hour, but collapsing to the unix epoch (which `.single()`
/// would do) is wrong by decades and breaks every downstream stat.
///
/// A time in a DST spring-forward gap moves forward by the gap, like
/// `local_naive_to_unix`. Such a time is real data: a session recorded
/// in another time zone, or before the device's zone changed, can name
/// a wall-clock time that doesn't exist here. Turning it into the
/// epoch put the session on 1 Jan 1970, and a later edit saved it
/// there.
///
/// Returns 0 (the unix epoch) on parse failure or on a year outside
/// 0000-9999. Both indicate corrupt or fabricated input, since
/// `unix_to_local_iso` never produces either.
pub fn local_iso_to_unix(iso: &str) -> i64 {
    use chrono::TimeZone;
    iso_to_unix_with(iso, |n| {
        chrono::Local.from_local_datetime(&n).map(|dt| dt.fixed_offset())
    })
}

/// `local_iso_to_unix` with the time-zone lookup passed in, for tests.
pub(crate) fn iso_to_unix_with(
    iso: &str,
    lookup: impl Fn(chrono::NaiveDateTime) -> chrono::LocalResult<chrono::DateTime<chrono::FixedOffset>>,
) -> i64 {
    use chrono::Datelike;
    let Ok(naive) = chrono::NaiveDateTime::parse_from_str(iso, "%Y-%m-%dT%H:%M:%S")
        .or_else(|_| chrono::NaiveDateTime::parse_from_str(iso, "%Y-%m-%d %H:%M:%S"))
    else {
        return 0;
    };
    // Chrono accepts extreme years (-262144..=262143). A peer-authored
    // event with a 5-digit / negative year would parse cleanly here
    // and round-trip back through `unix_to_local_iso` as an unusable
    // string. Reject before computing the unix value so the caller
    // gets the same "corrupt input" signal as parse failure.
    if !(0..=9999).contains(&naive.year()) {
        return 0;
    }
    naive_to_unix_with(naive, lookup)
}

/// Internal helper for `naive_to_unix_with`: collapses a chrono
/// `LocalResult` into an optional unix timestamp, picking the earlier
/// instant on fall-back ambiguity. Compares the instants rather than
/// trusting `earliest()`: chrono orders the two candidates by UTC
/// offset, which for a fall-back puts the LATER instant first.
fn disambiguate_local_result<Tz: chrono::TimeZone>(
    lr: chrono::LocalResult<chrono::DateTime<Tz>>,
) -> Option<i64> {
    match lr {
        chrono::LocalResult::Single(dt) => Some(dt.timestamp()),
        chrono::LocalResult::Ambiguous(a, b) => Some(a.timestamp().min(b.timestamp())),
        chrono::LocalResult::None => None,
    }
}

/// A wall-clock time the user picked (or an import row) → unix
/// seconds in the device's time zone. Never fails, unlike chrono's
/// `.single()` / `.earliest()`:
/// - a time that happens twice (DST fall-back) gives the first one;
/// - a time that never happens (DST spring-forward gap) moves forward
///   by the length of the gap, so 02:30 becomes 03:30.
///
/// Both shells convert picked dates and Insight Timer rows through
/// this instead of their own conversions.
pub fn local_naive_to_unix(naive: chrono::NaiveDateTime) -> i64 {
    use chrono::TimeZone;
    naive_to_unix_with(naive, |n| {
        chrono::Local.from_local_datetime(&n).map(|dt| dt.fixed_offset())
    })
}

/// `local_naive_to_unix` with the time-zone lookup passed in, so tests
/// can supply a DST day without touching the host's time zone.
pub(crate) fn naive_to_unix_with(
    naive: chrono::NaiveDateTime,
    lookup: impl Fn(chrono::NaiveDateTime) -> chrono::LocalResult<chrono::DateTime<chrono::FixedOffset>>,
) -> i64 {
    if let Some(unix) = disambiguate_local_result(lookup(naive)) {
        return unix;
    }
    // In a gap: read the time with the offset in force just before
    // it, which lands the same distance past the gap's end. Gaps are
    // at most a few hours; a day back is always before the gap.
    (1..=24)
        .find_map(|h| match lookup(naive - chrono::Duration::hours(h)) {
            chrono::LocalResult::Single(dt) | chrono::LocalResult::Ambiguous(dt, _) => {
                Some(dt.offset().local_minus_utc())
            }
            chrono::LocalResult::None => None,
        })
        .map_or_else(
            || naive.and_utc().timestamp(),
            |offset| naive.and_utc().timestamp() - i64::from(offset),
        )
}

/// A fake time zone for DST tests anywhere in the crate.
#[cfg(test)]
pub(crate) mod test_zone {
    /// Europe/Berlin around the 2026 transitions, built from fixed
    /// offsets so the tests don't depend on the host's time zone.
    /// Spring forward 2026-03-29 02:00 → 03:00 (02:xx never happens);
    /// fall back 2026-10-25 03:00 → 02:00 (02:xx happens twice).
    /// Ambiguous results come later-instant first, like chrono's.
    pub(crate) fn berlin(n: chrono::NaiveDateTime) -> chrono::LocalResult<chrono::DateTime<chrono::FixedOffset>> {
        use chrono::{NaiveDate, TimeZone};
        let at = |off: i32| chrono::FixedOffset::east_opt(off).unwrap().from_local_datetime(&n).unwrap();
        let (cet, cest) = (3600, 7200);
        let spring = NaiveDate::from_ymd_opt(2026, 3, 29).unwrap().and_hms_opt(2, 0, 0).unwrap();
        let fall = NaiveDate::from_ymd_opt(2026, 10, 25).unwrap().and_hms_opt(2, 0, 0).unwrap();
        let hour = chrono::Duration::hours(1);
        if n >= spring && n < spring + hour {
            chrono::LocalResult::None
        } else if n >= fall && n < fall + hour {
            chrono::LocalResult::Ambiguous(at(cet), at(cest))
        } else if n >= spring + hour && n < fall {
            chrono::LocalResult::Single(at(cest))
        } else {
            chrono::LocalResult::Single(at(cet))
        }
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
            after > before,
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
        // which several core queries (e.g. the stats date bounds) depend on.
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
    fn local_iso_to_unix_rejects_extreme_years() {
        // chrono parses 5-digit / negative years just fine — only the
        // explicit range check inside local_iso_to_unix stops them.
        // Without it, a peer-authored event with start_iso="10000-..."
        // would slip through and corrupt downstream SUBSTR-based stats.
        assert_eq!(local_iso_to_unix("10000-01-01T00:00:00"), 0);
        assert_eq!(local_iso_to_unix("-262143-01-01T00:00:00"), 0);
        // Year 0 and year 9999 are the boundary values and must pass.
        assert_ne!(local_iso_to_unix("0001-01-01T00:00:00"), 0,
            "year 0001 is inside the accepted range");
        assert_ne!(local_iso_to_unix("9999-12-31T23:59:59"), 0,
            "year 9999 is inside the accepted range");
    }

    #[test]
    fn unix_to_local_iso_falls_back_to_epoch_for_extreme_years() {
        // Symmetric to the local_iso_to_unix guard. Without the
        // range filter, chrono's %Y format would emit a year with
        // a sign or 5+ digits, breaking every SUBSTR-based stat
        // (hour-of-day, day-of-week, etc.) that assumes the 19-char
        // canonical shape.
        //
        // 253_402_300_800 unix seconds corresponds to year 10000.
        let year_10000 = 253_402_300_800;
        let s = unix_to_local_iso(year_10000);
        assert_eq!(s.len(), 19,
            "year-overflow case must fall back to a 19-char string, got {s:?}");
        assert!(!s.starts_with('-'),
            "year-overflow case must not produce a negative-year string");
    }

    #[test]
    fn unix_to_local_iso_falls_back_to_epoch_for_i64_extremes() {
        // i64::MIN / i64::MAX both saturate chrono's date range —
        // timestamp_opt returns None and the unwrap_or_else fires.
        // Belt-and-braces: even if a future chrono accepted these,
        // the year filter catches them too.
        for ts in [i64::MIN, i64::MAX] {
            let s = unix_to_local_iso(ts);
            assert_eq!(s.len(), 19,
                "i64 extreme {ts} must fall back, got {s:?}");
        }
    }

    // ── disambiguate_local_result ──────────────────────────────────────

    #[test]
    fn disambiguate_picks_earlier_unix_on_ambiguous_fall_back() {
        use chrono::TimeZone;
        // Simulate a DST fall-back: a local time that maps to two
        // distinct unix candidates one hour apart. Picking the earlier
        // matches how a user would describe "I meditated at 2:30" (the
        // first 2:30, before the clock rolled back).
        let earlier = chrono::Utc.timestamp_opt(1_700_000_000, 0).unwrap();
        let later = chrono::Utc.timestamp_opt(1_700_003_600, 0).unwrap();
        let lr = chrono::LocalResult::Ambiguous(earlier, later);
        assert_eq!(
            disambiguate_local_result(lr),
            Some(1_700_000_000),
            "must pick the earlier of two ambiguous unix candidates",
        );
    }

    #[test]
    fn disambiguate_picks_the_earlier_instant_whatever_the_order() {
        use chrono::TimeZone;
        // chrono orders the two candidates by UTC offset, not by
        // instant: for Europe/Berlin's fall-back it hands the LATER
        // instant (+01:00) first, so `earliest()` alone is wrong.
        let earlier = chrono::Utc.timestamp_opt(1_700_000_000, 0).unwrap();
        let later = chrono::Utc.timestamp_opt(1_700_003_600, 0).unwrap();
        let lr = chrono::LocalResult::Ambiguous(later, earlier);
        assert_eq!(disambiguate_local_result(lr), Some(1_700_000_000));
    }

    // ── local_naive_to_unix ────────────────────────────────────────────

    fn naive(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> chrono::NaiveDateTime {
        chrono::NaiveDate::from_ymd_opt(y, mo, d).unwrap().and_hms_opt(h, mi, 0).unwrap()
    }

    /// The unix seconds of a wall-clock time at a fixed UTC offset.
    fn at_offset(n: chrono::NaiveDateTime, offset_hours: i64) -> i64 {
        n.and_utc().timestamp() - offset_hours * 3600
    }

    #[test]
    fn an_ordinary_time_converts_with_its_offset() {
        let winter = naive(2026, 1, 10, 8, 0);
        let summer = naive(2026, 7, 10, 8, 0);
        assert_eq!(naive_to_unix_with(winter, test_zone::berlin), at_offset(winter, 1));
        assert_eq!(naive_to_unix_with(summer, test_zone::berlin), at_offset(summer, 2));
    }

    #[test]
    fn a_time_that_happens_twice_gives_the_first_one() {
        // 02:00, 02:30 and 02:59 on the fall-back night all exist in
        // summer time (+02, earlier) and again in winter time (+01).
        for m in [0, 30, 59] {
            let n = naive(2026, 10, 25, 2, m);
            assert_eq!(naive_to_unix_with(n, test_zone::berlin), at_offset(n, 2), "02:{m:02}");
        }
    }

    #[test]
    fn the_minutes_around_the_fall_back_hour_are_unambiguous() {
        let before = naive(2026, 10, 25, 1, 59);
        let after = naive(2026, 10, 25, 3, 0);
        assert_eq!(naive_to_unix_with(before, test_zone::berlin), at_offset(before, 2));
        assert_eq!(naive_to_unix_with(after, test_zone::berlin), at_offset(after, 1));
    }

    #[test]
    fn a_time_in_the_skipped_hour_moves_forward_by_the_gap() {
        // 02:xx doesn't exist on the spring-forward night; the result is
        // the same moment as 03:xx summer time, one hour later on the
        // clock, never "now".
        for m in [0, 30, 59] {
            let n = naive(2026, 3, 29, 2, m);
            let shifted = naive(2026, 3, 29, 3, m);
            assert_eq!(naive_to_unix_with(n, test_zone::berlin), at_offset(shifted, 2), "02:{m:02}");
        }
    }

    #[test]
    fn a_stored_start_in_the_skipped_hour_is_not_the_epoch() {
        // A session synced from another time zone can name 02:30 on
        // the spring-forward night. It reads as 03:30 summer time,
        // not as 1 Jan 1970.
        let unix = iso_to_unix_with("2026-03-29T02:30:00", test_zone::berlin);
        assert_eq!(unix, at_offset(naive(2026, 3, 29, 3, 30), 2));
        assert_eq!(iso_to_unix_with("garbage", test_zone::berlin), 0);
    }

    #[test]
    fn the_minutes_around_the_skipped_hour_are_unchanged() {
        let before = naive(2026, 3, 29, 1, 59);
        let after = naive(2026, 3, 29, 3, 0);
        assert_eq!(naive_to_unix_with(before, test_zone::berlin), at_offset(before, 1));
        assert_eq!(naive_to_unix_with(after, test_zone::berlin), at_offset(after, 2));
    }

    #[test]
    fn a_doubled_time_survives_a_round_trip_through_unix() {
        // The edit dialog seeds its pickers from the stored start and
        // converts them back on save: 02:30 on the fall-back night must
        // come back as 02:30, not as the moment of saving.
        let n = naive(2026, 10, 25, 2, 30);
        let unix = naive_to_unix_with(n, test_zone::berlin);
        let back = chrono::DateTime::from_timestamp(unix, 0).unwrap().naive_utc()
            + chrono::Duration::hours(2);
        assert_eq!(back, n);
    }

    #[test]
    fn disambiguate_returns_none_on_spring_forward_gap() {
        // A local time inside a DST spring-forward gap (a wall-clock
        // value that never occurred). `unix_to_local_iso` never produces
        // these — they only arrive as corrupt input — so dropping them
        // to None is fine; the public wrapper will land on the 0 sentinel.
        let lr: chrono::LocalResult<chrono::DateTime<chrono::Utc>> =
            chrono::LocalResult::None;
        assert_eq!(disambiguate_local_result(lr), None);
    }

    #[test]
    fn disambiguate_passes_through_unique_value() {
        use chrono::TimeZone;
        let dt = chrono::Utc.timestamp_opt(1_700_000_000, 0).unwrap();
        let lr = chrono::LocalResult::Single(dt);
        assert_eq!(disambiguate_local_result(lr), Some(1_700_000_000));
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
