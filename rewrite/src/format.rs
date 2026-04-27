use std::time::Duration;

pub fn parse_hms_duration(s: &str) -> Option<Duration> {
    let parts: Vec<&str> = s.split(':').collect();
    let nums: Vec<u64> = parts
        .iter()
        .map(|p| p.parse::<u64>())
        .collect::<std::result::Result<_, _>>()
        .ok()?;
    match nums.as_slice() {
        [m, s] => Some(Duration::from_secs(m * 60 + s)),
        [h, m, s] => Some(Duration::from_secs(h * 3600 + m * 60 + s)),
        _ => None,
    }
}

pub fn format_hm_compact(d: Duration) -> String {
    let total_mins = d.as_secs() / 60;
    let h = total_mins / 60;
    let m = total_mins % 60;
    if h >= 100 {
        format!("{h}h")
    } else {
        match (h, m) {
            (0, _) => format!("{m}m"),
            (_, 0) => format!("{h}h"),
            _ => format!("{h}h{m}m"),
        }
    }
}

pub fn format_hm_mins(d: Duration) -> String {
    let total_mins = d.as_secs() / 60;
    let h = total_mins / 60;
    let m = total_mins % 60;
    match (h, m) {
        (0, _) => format!("{m}m"),
        (_, 0) => format!("{h}h"),
        _ => format!("{h}h {m}m"),
    }
}

pub fn format_hm_secs(d: Duration) -> String {
    let total = d.as_secs();
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    if h > 0 {
        format!("{h}h {m}m {s}s")
    } else if m > 0 {
        format!("{m}m {s}s")
    } else {
        format!("{s}s")
    }
}

pub fn format_time(d: Duration) -> String {
    let total = d.as_secs();
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    if h > 0 {
        format!("{h:02}:{m:02}:{s:02}")
    } else {
        format!("{m:02}:{s:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_time_zero_shows_double_zero() {
        assert_eq!(format_time(Duration::ZERO), "00:00");
    }

    #[test]
    fn format_time_pads_under_minute() {
        assert_eq!(format_time(Duration::from_secs(5)), "00:05");
    }

    #[test]
    fn format_time_under_hour_shows_minutes_seconds() {
        assert_eq!(format_time(Duration::from_secs(65)), "01:05");
    }

    #[test]
    fn format_time_at_hour_adds_hours_segment() {
        assert_eq!(format_time(Duration::from_secs(3661)), "01:01:01");
    }

    #[test]
    fn parse_hms_duration_accepts_minutes_seconds() {
        assert_eq!(parse_hms_duration("1:30"), Some(Duration::from_secs(90)));
    }

    #[test]
    fn parse_hms_duration_accepts_hours_minutes_seconds() {
        assert_eq!(
            parse_hms_duration("1:30:45"),
            Some(Duration::from_secs(5445))
        );
    }

    #[test]
    fn parse_hms_duration_rejects_garbage() {
        assert_eq!(parse_hms_duration("garbage"), None);
        assert_eq!(parse_hms_duration(""), None);
        assert_eq!(parse_hms_duration("60"), None); // single component is ambiguous
        assert_eq!(parse_hms_duration("1:30:45:00"), None);
        assert_eq!(parse_hms_duration(":30"), None);
        assert_eq!(parse_hms_duration("1:30.5"), None); // fractional seconds rejected
    }

    #[test]
    fn format_hm_secs_omits_unused_units() {
        assert_eq!(format_hm_secs(Duration::ZERO), "0s");
        assert_eq!(format_hm_secs(Duration::from_secs(30)), "30s");
        assert_eq!(format_hm_secs(Duration::from_secs(90)), "1m 30s");
        assert_eq!(format_hm_secs(Duration::from_secs(3665)), "1h 1m 5s");
    }

    #[test]
    fn format_hm_mins_drops_seconds_and_unused_units() {
        assert_eq!(format_hm_mins(Duration::ZERO), "0m");
        assert_eq!(format_hm_mins(Duration::from_secs(30)), "0m");
        assert_eq!(format_hm_mins(Duration::from_secs(90)), "1m");
        assert_eq!(format_hm_mins(Duration::from_secs(3600)), "1h");
        assert_eq!(format_hm_mins(Duration::from_secs(3661)), "1h 1m");
    }

    #[test]
    fn format_hm_compact_omits_spaces_and_clips_at_100h() {
        assert_eq!(format_hm_compact(Duration::ZERO), "0m");
        assert_eq!(format_hm_compact(Duration::from_secs(90)), "1m");
        assert_eq!(format_hm_compact(Duration::from_secs(3600)), "1h");
        assert_eq!(format_hm_compact(Duration::from_secs(3661)), "1h1m");
        // h >= 100 clips minutes — keeps the cell narrow in the heatmap.
        assert_eq!(
            format_hm_compact(Duration::from_secs(100 * 3600)),
            "100h"
        );
        assert_eq!(
            format_hm_compact(Duration::from_secs(100 * 3600 + 60)),
            "100h"
        );
    }
}
