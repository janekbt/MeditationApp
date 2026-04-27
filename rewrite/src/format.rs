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

pub fn parse_insighttimer_datetime(s: &str) -> Option<chrono::NaiveDateTime> {
    // InsightTimer export format, e.g. "10/15/2024 6:30:00 AM".
    chrono::NaiveDateTime::parse_from_str(s, "%m/%d/%Y %l:%M:%S %p").ok()
}

const SESSION_MILESTONES: &[u32] = &[10, 25, 50, 100, 250, 500, 1000, 2500, 5000];

pub fn next_session_milestone(count: u32) -> Option<u32> {
    SESSION_MILESTONES.iter().copied().find(|&m| count < m)
}

pub fn minutes_to_level(mins: u32) -> u8 {
    match mins {
        0 => 0,
        1..=32 => 1,
        33..=79 => 2,
        80..=119 => 3,
        _ => 4,
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

    #[test]
    fn minutes_to_level_buckets_at_thresholds_0_33_80_120() {
        // Level 0: no activity.
        assert_eq!(minutes_to_level(0), 0);
        // Level 1: any meditation up to 32 mins.
        assert_eq!(minutes_to_level(1), 1);
        assert_eq!(minutes_to_level(32), 1);
        // Level 2: 33..80.
        assert_eq!(minutes_to_level(33), 2);
        assert_eq!(minutes_to_level(79), 2);
        // Level 3: 80..120.
        assert_eq!(minutes_to_level(80), 3);
        assert_eq!(minutes_to_level(119), 3);
        // Level 4: 120+.
        assert_eq!(minutes_to_level(120), 4);
        assert_eq!(minutes_to_level(1000), 4);
    }

    #[test]
    fn next_session_milestone_steps_through_targets() {
        // Approaching the first milestone.
        assert_eq!(next_session_milestone(0), Some(10));
        assert_eq!(next_session_milestone(9), Some(10));
        // At a milestone — point to the next one, not back to the same.
        assert_eq!(next_session_milestone(10), Some(25));
        assert_eq!(next_session_milestone(25), Some(50));
        // Mid-range.
        assert_eq!(next_session_milestone(101), Some(250));
        assert_eq!(next_session_milestone(2499), Some(2500));
        // Last milestone.
        assert_eq!(next_session_milestone(4999), Some(5000));
        // Past 5000 — no further milestone.
        assert_eq!(next_session_milestone(5000), None);
        assert_eq!(next_session_milestone(10000), None);
    }

    #[test]
    fn parse_insighttimer_datetime_handles_am_and_pm() {
        let am = parse_insighttimer_datetime("10/15/2024 6:30:00 AM").unwrap();
        assert_eq!(am.to_string(), "2024-10-15 06:30:00");
        let pm = parse_insighttimer_datetime("10/15/2024 6:30:00 PM").unwrap();
        assert_eq!(pm.to_string(), "2024-10-15 18:30:00");
    }

    #[test]
    fn parse_insighttimer_datetime_rejects_garbage() {
        assert_eq!(parse_insighttimer_datetime(""), None);
        assert_eq!(parse_insighttimer_datetime("not a date"), None);
        // ISO format is rejected — this parser is for InsightTimer's specific shape.
        assert_eq!(parse_insighttimer_datetime("2024-10-15T06:30:00"), None);
    }
}
