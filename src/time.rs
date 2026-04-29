//! Defensive wrappers around the couple of glib datetime calls that can
//! fail on pathological systems (missing tzdata, exhausted clock).
//!
//! Also: the i64-unix ↔ local-naive ISO 8601 conversions used at the
//! database boundary. The DB stores ISO strings (meditate-core's choice,
//! to keep core GTK-free); the rest of the app uses unix timestamps for
//! ergonomics. These two functions are the only translation point.

use gtk::glib;

/// Current local time. Falls back to UTC if the tzdata lookup fails —
/// better a slightly wrong clock than a panic on the stats tab.
pub fn now_local() -> glib::DateTime {
    glib::DateTime::now_local()
        .or_else(|_| glib::DateTime::now_utc())
        .expect("system reports no working clock")
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
