use std::time::Duration;

pub fn parse_hms_duration(s: &str) -> Option<Duration> {
    let parts: Vec<&str> = s.split(':').collect();
    // Last component may be fractional ("30.5"); leading components must be integers.
    match parts.as_slice() {
        [m, sec] => {
            let m: u64 = m.parse().ok()?;
            let sec: f64 = sec.parse().ok()?;
            Some(Duration::from_secs(m * 60 + sec.round() as u64))
        }
        [h, m, sec] => {
            let h: u64 = h.parse().ok()?;
            let m: u64 = m.parse().ok()?;
            let sec: f64 = sec.parse().ok()?;
            Some(Duration::from_secs(h * 3600 + m * 60 + sec.round() as u64))
        }
        _ => None,
    }
}

pub fn parse_insighttimer_datetime(s: &str) -> Option<chrono::NaiveDateTime> {
    // InsightTimer export has shipped both shapes across versions/locales:
    //   "10/15/2024 6:30:00 AM"   (12-hour with AM/PM)
    //   "04/20/2026 08:21:14"     (24-hour)
    // Try both so a mixed import works either way.
    chrono::NaiveDateTime::parse_from_str(s, "%m/%d/%Y %l:%M:%S %p")
        .or_else(|_| chrono::NaiveDateTime::parse_from_str(s, "%m/%d/%Y %H:%M:%S"))
        .ok()
}

const SESSION_MILESTONES: &[i64] = &[10, 25, 50, 100, 250, 500, 1000, 2500, 5000];

/// Returns `(target, distance_to_target)` for the next session-count milestone,
/// or `None` past the highest milestone.
pub fn next_session_milestone(count: i64) -> Option<(i64, i64)> {
    SESSION_MILESTONES
        .iter()
        .copied()
        .find(|&t| t > count)
        .map(|t| (t, t - count))
}

/// Heatmap level (0–4) for a day's meditated minutes against a daily goal.
/// Bands are percentages of the goal: 0 / 1–32 / 33–79 / 80–119 / 120+.
/// `mins <= 0` → 0; `goal_mins <= 0` (no goal set) → max level on any activity.
pub fn minutes_to_level(mins: i64, goal_mins: i64) -> u8 {
    if mins <= 0 {
        return 0;
    }
    if goal_mins <= 0 {
        return 4;
    }
    let pct = mins.saturating_mul(100) / goal_mins;
    match pct {
        0..=32 => 1,
        33..=79 => 2,
        80..=119 => 3,
        _ => 4,
    }
}

pub fn format_hm_compact(d: Duration) -> String {
    let total_mins = d.as_secs() / 60;
    if total_mins == 0 {
        return "–".to_string();
    }
    let h = total_mins / 60;
    let m = total_mins % 60;
    if h >= 100 {
        return format!("{h}h");
    }
    match (h, m) {
        (0, _) => format!("{m}m"),
        (_, 0) => format!("{h}h"),
        _ => format!("{h}h {m}m"),
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

/// "h/m output from a seconds-precision input." Despite the name (which
/// reflects the input precision), this drops sub-minute remainder for
/// stats display where seconds are noise. Use `format_time` for live
/// session display where seconds matter.
pub fn format_hm_secs(d: Duration) -> String {
    let total = d.as_secs();
    if total == 0 {
        return "–".to_string();
    }
    let h = total / 3600;
    let m = (total % 3600) / 60;
    match (h, m) {
        (0, m) => format!("{m}m"),
        (h, 0) => format!("{h}h"),
        (h, m) => format!("{h}h {m}m"),
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

/// Bounds + default for the preparation-time silence in seconds.
///
/// Min 5 s — anything shorter feels accidental. Max 5 min — keeps the
/// SpinRow tractable and avoids a "the app froze" reading. Default 30 s
/// is long enough to settle, short enough to feel snappy.
pub const PREP_SECS_MIN: u32 = 5;
pub const PREP_SECS_MAX: u32 = 300;
pub const PREP_SECS_DEFAULT: u32 = 30;

/// Decide whether to enter the Preparing state at session start.
///
/// `Some(d)` means schedule a prep tick of `d` and play the starting
/// bell at the end of it; `None` means skip prep and go straight to
/// Running. A 0-second prep is treated as "no prep" — bouncing through
/// Preparing for an instant would just create a flicker.
pub fn prep_target_duration(prep_active: bool, prep_secs: u32) -> Option<Duration> {
    if prep_active && prep_secs > 0 {
        Some(Duration::from_secs(prep_secs as u64))
    } else {
        None
    }
}

/// Parse a settings-table preparation-time value into a clamped u32.
///
/// Returns `PREP_SECS_DEFAULT` for empty / non-numeric / negative input
/// (anything `u32::from_str` rejects), and clamps in-range integers to
/// `[PREP_SECS_MIN, PREP_SECS_MAX]`. The shell never has to think about
/// sanitising a raw string read from the DB.
pub fn parse_prep_secs(s: &str) -> u32 {
    s.parse::<u32>()
        .map(|n| n.clamp(PREP_SECS_MIN, PREP_SECS_MAX))
        .unwrap_or(PREP_SECS_DEFAULT)
}

/// Hero-label text for a running Timer-mode session.
///
/// `target = Some(d)` means the user picked a duration; the label counts
/// down (`format_time(target - elapsed)`). `target = None` means the
/// stopwatch toggle is on; the label counts up (`format_time(elapsed)`).
/// Saturating subtraction: if a tick lands a beat past `target`, the
/// caller gets `00:00` instead of an underflow panic.
pub fn running_text(target: Option<Duration>, elapsed: Duration) -> String {
    match target {
        Some(t) => format_time(t.saturating_sub(elapsed)),
        None => format_time(elapsed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_time_zero_shows_double_zero() {
        assert_eq!(format_time(Duration::ZERO), "00:00");
    }

    // ── running_text ──────────────────────────────────────────────────
    // The hero label on the running timer page. Two regimes folded into
    // one helper so the merged Timer mode (M.2 onwards) can branch on a
    // single Option<Duration> rather than a TimerMode variant.

    #[test]
    fn running_text_targeted_shows_remaining() {
        assert_eq!(
            running_text(Some(Duration::from_secs(60)), Duration::from_secs(10)),
            "00:50"
        );
    }

    #[test]
    fn running_text_open_ended_shows_elapsed() {
        assert_eq!(
            running_text(None, Duration::from_secs(10)),
            "00:10"
        );
    }

    #[test]
    fn running_text_targeted_clamps_to_zero_when_elapsed_overshoots() {
        // Tick scheduling sometimes lands one tick after target; saturating_sub
        // gives "00:00" instead of underflowing.
        assert_eq!(
            running_text(Some(Duration::from_secs(60)), Duration::from_secs(75)),
            "00:00"
        );
    }

    #[test]
    fn running_text_targeted_at_start_shows_full_target() {
        assert_eq!(
            running_text(Some(Duration::from_secs(600)), Duration::ZERO),
            "10:00"
        );
    }

    #[test]
    fn running_text_open_ended_at_start_shows_zero() {
        assert_eq!(running_text(None, Duration::ZERO), "00:00");
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
    }

    #[test]
    fn parse_hms_duration_rounds_fractional_seconds() {
        // 1:30.5 = 1m 30.5s → rounds to 91s
        assert_eq!(parse_hms_duration("1:30.5"), Some(Duration::from_secs(91)));
        // 1:30.4 → rounds down to 90s
        assert_eq!(parse_hms_duration("1:30.4"), Some(Duration::from_secs(90)));
        // Three-part with fractional last component.
        assert_eq!(
            parse_hms_duration("1:00:30.5"),
            Some(Duration::from_secs(3631))
        );
    }

    #[test]
    fn format_hm_secs_drops_sub_minute_and_uses_em_dash_for_zero() {
        // Stats display: seconds are noise; show "–" for empty.
        assert_eq!(format_hm_secs(Duration::ZERO), "–");
        assert_eq!(format_hm_secs(Duration::from_secs(30)), "0m");
        assert_eq!(format_hm_secs(Duration::from_secs(90)), "1m");
        assert_eq!(format_hm_secs(Duration::from_secs(3600)), "1h");
        assert_eq!(format_hm_secs(Duration::from_secs(3665)), "1h 1m");
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
    fn format_hm_compact_uses_em_dash_for_empty() {
        // Zero is empty, not "0m" — heatmap cells with no data render "–".
        assert_eq!(format_hm_compact(Duration::ZERO), "–");
    }

    #[test]
    fn format_hm_compact_clips_at_100h() {
        assert_eq!(format_hm_compact(Duration::from_secs(90)), "1m");
        assert_eq!(format_hm_compact(Duration::from_secs(3600)), "1h");
        assert_eq!(format_hm_compact(Duration::from_secs(3661)), "1h 1m");
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
    fn minutes_to_level_buckets_at_thresholds_0_33_80_120_percent_of_goal() {
        // Bands are percentages of the daily goal, not absolute minutes.
        // With goal=100, the percentage and the minutes happen to match.
        assert_eq!(minutes_to_level(0, 100), 0);
        assert_eq!(minutes_to_level(1, 100), 1);
        assert_eq!(minutes_to_level(32, 100), 1);
        assert_eq!(minutes_to_level(33, 100), 2);
        assert_eq!(minutes_to_level(79, 100), 2);
        assert_eq!(minutes_to_level(80, 100), 3);
        assert_eq!(minutes_to_level(119, 100), 3);
        assert_eq!(minutes_to_level(120, 100), 4);
        assert_eq!(minutes_to_level(1000, 100), 4);
    }

    #[test]
    fn minutes_to_level_scales_with_goal() {
        // 18 mins against a 15-min goal = 120% → level 4 (high achievement).
        assert_eq!(minutes_to_level(18, 15), 4);
        // Same 18 mins against a 100-min goal = 18% → level 1.
        assert_eq!(minutes_to_level(18, 100), 1);
    }

    #[test]
    fn minutes_to_level_handles_no_goal_and_negative() {
        // No goal set → any positive activity clips to max level.
        assert_eq!(minutes_to_level(60, 0), 4);
        // Negative goal also treated as no goal.
        assert_eq!(minutes_to_level(60, -1), 4);
        // Negative minutes → no activity.
        assert_eq!(minutes_to_level(-5, 100), 0);
    }

    #[test]
    fn next_session_milestone_returns_target_and_distance() {
        // (target, distance_to_target).
        assert_eq!(next_session_milestone(0), Some((10, 10)));
        assert_eq!(next_session_milestone(9), Some((10, 1)));
        assert_eq!(next_session_milestone(10), Some((25, 15)));
        assert_eq!(next_session_milestone(24), Some((25, 1)));
        assert_eq!(next_session_milestone(499), Some((500, 1)));
        assert_eq!(next_session_milestone(2499), Some((2500, 1)));
        assert_eq!(next_session_milestone(4999), Some((5000, 1)));
    }

    #[test]
    fn next_session_milestone_returns_none_past_ceiling() {
        assert_eq!(next_session_milestone(5000), None);
        assert_eq!(next_session_milestone(5001), None);
        assert_eq!(next_session_milestone(10_000), None);
    }

    #[test]
    fn parse_insighttimer_datetime_handles_am_and_pm() {
        let am = parse_insighttimer_datetime("10/15/2024 6:30:00 AM").unwrap();
        assert_eq!(am.to_string(), "2024-10-15 06:30:00");
        let pm = parse_insighttimer_datetime("10/15/2024 6:30:00 PM").unwrap();
        assert_eq!(pm.to_string(), "2024-10-15 18:30:00");
    }

    #[test]
    fn parse_insighttimer_datetime_handles_24_hour() {
        // Some InsightTimer exports are 24-hour without AM/PM.
        let dt = parse_insighttimer_datetime("04/20/2026 08:21:14").unwrap();
        assert_eq!(dt.to_string(), "2026-04-20 08:21:14");
        let evening = parse_insighttimer_datetime("04/20/2026 20:00:00").unwrap();
        assert_eq!(evening.to_string(), "2026-04-20 20:00:00");
    }

    #[test]
    fn parse_insighttimer_datetime_rejects_garbage() {
        assert_eq!(parse_insighttimer_datetime(""), None);
        assert_eq!(parse_insighttimer_datetime("not a date"), None);
        // ISO format is rejected — this parser is for InsightTimer's specific shape.
        assert_eq!(parse_insighttimer_datetime("2024-10-15T06:30:00"), None);
        // Month 13 is invalid in either format.
        assert_eq!(parse_insighttimer_datetime("13/01/2024 08:30:00"), None);
    }

    // ── parse_prep_secs ──────────────────────────────────────────────
    // Settings-table values for the Preparation-Time SpinRow round-trip
    // through this helper so the shell never has to think about garbage,
    // empty strings, or out-of-range values from a future hand-edit.

    #[test]
    fn parse_prep_secs_constants_have_expected_shape() {
        // Min / max bound a "settle in" silence — long enough to feel
        // intentional, short enough not to feel like a frozen UI.
        assert_eq!(PREP_SECS_MIN, 5);
        assert_eq!(PREP_SECS_MAX, 300);
        assert_eq!(PREP_SECS_DEFAULT, 30);
        // Default must lie in the allowed range.
        assert!(PREP_SECS_MIN <= PREP_SECS_DEFAULT && PREP_SECS_DEFAULT <= PREP_SECS_MAX);
    }

    #[test]
    fn parse_prep_secs_passes_through_in_range() {
        assert_eq!(parse_prep_secs("5"), 5);
        assert_eq!(parse_prep_secs("30"), 30);
        assert_eq!(parse_prep_secs("60"), 60);
        assert_eq!(parse_prep_secs("300"), 300);
    }

    #[test]
    fn parse_prep_secs_clamps_below_min() {
        assert_eq!(parse_prep_secs("0"), PREP_SECS_MIN);
        assert_eq!(parse_prep_secs("4"), PREP_SECS_MIN);
    }

    #[test]
    fn parse_prep_secs_clamps_above_max() {
        assert_eq!(parse_prep_secs("301"), PREP_SECS_MAX);
        assert_eq!(parse_prep_secs("100000"), PREP_SECS_MAX);
    }

    #[test]
    fn parse_prep_secs_falls_back_to_default_on_garbage() {
        assert_eq!(parse_prep_secs(""), PREP_SECS_DEFAULT);
        assert_eq!(parse_prep_secs("garbage"), PREP_SECS_DEFAULT);
        // Negative — u32 parse fails, default kicks in.
        assert_eq!(parse_prep_secs("-5"), PREP_SECS_DEFAULT);
        // Stray decimals — u32 parse fails, default kicks in.
        assert_eq!(parse_prep_secs("30.0"), PREP_SECS_DEFAULT);
    }

    // ── prep_target_duration ─────────────────────────────────────────
    // Decides whether the timer should enter the Preparing state. The
    // shell calls this once at on_start; Some(d) → schedule prep,
    // None → skip prep and go straight to Running.

    #[test]
    fn prep_target_duration_returns_some_only_when_active_and_positive() {
        assert_eq!(
            prep_target_duration(true, 30),
            Some(Duration::from_secs(30))
        );
        assert_eq!(
            prep_target_duration(true, PREP_SECS_MIN),
            Some(Duration::from_secs(PREP_SECS_MIN as u64))
        );
        assert_eq!(
            prep_target_duration(true, PREP_SECS_MAX),
            Some(Duration::from_secs(PREP_SECS_MAX as u64))
        );
    }

    #[test]
    fn prep_target_duration_is_none_when_inactive() {
        // Switch off → no prep, regardless of seconds value.
        assert_eq!(prep_target_duration(false, 30), None);
        assert_eq!(prep_target_duration(false, 0), None);
    }

    #[test]
    fn prep_target_duration_is_none_when_zero_seconds() {
        // A 0-second prep is just "no prep" — don't bounce through
        // the Preparing state for an instant.
        assert_eq!(prep_target_duration(true, 0), None);
    }
}
