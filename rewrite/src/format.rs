use std::time::Duration;

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
}
