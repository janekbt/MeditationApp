//! Date / weekday arithmetic shared between shells for stats roll-ups.
//!
//! Two kinds of helpers live here:
//! - Locale week-start detection (`locale_week_start_dow`) — a tiny
//!   libc bridge that survives the move to core because libc is
//!   portable enough; non-glibc platforms fall back to Monday.
//! - Pure arithmetic on integer day-of-week values
//!   (`days_since_week_start`) + the `week_over_week` aggregator
//!   that takes a `now_unix` reference moment and a pre-built
//!   date→seconds map. No glib datetime types in the API.

use std::collections::HashMap;

/// First day of the week per the active locale, in 1=Mon..7=Sun
/// numbering (compatible with `chrono::Weekday::number_from_monday()`
/// and `glib::DateTime::day_of_week()`).
///
/// On glibc-based Linux this queries `nl_langinfo(_NL_TIME_FIRST_WEEKDAY)`
/// — a POSIX extension whose returned byte is 1=Sun..7=Sat — and
/// translates into the 1=Mon..7=Sun convention. On any non-Linux
/// target (Android's bionic and any future shell platform) the
/// query returns Monday; locale-aware first-weekday on those
/// platforms needs a different bridge that's beyond this helper's
/// scope.
pub fn locale_week_start_dow() -> i32 {
    // libc-rs doesn't expose glibc-specific _NL_* enumerants as named
    // constants, so we reconstruct the value. _NL_ITEM(category,
    // index) is ((category << 16) | index); on glibc __LC_TIME == 2
    // and the nl_langinfo.h enum lands _NL_TIME_FIRST_WEEKDAY at
    // index 40 in the LC_TIME block, giving 0x20028 = 131176.
    #[cfg(target_os = "linux")]
    const NL_TIME_FIRST_WEEKDAY: libc::nl_item = 131176;

    #[cfg(not(target_os = "linux"))]
    return 1;

    #[cfg(target_os = "linux")]
    unsafe {
        let ptr = libc::nl_langinfo(NL_TIME_FIRST_WEEKDAY);
        if ptr.is_null() {
            return 1;
        }
        let byte = *ptr as u8;
        // 1 = Sun … 7 = Sat (POSIX)  →  1 = Mon … 7 = Sun (1=Mon).
        match byte {
            1 => 7,         // Sunday → 7 in 1=Mon convention
            2..=7 => (byte - 1) as i32,
            _ => 1,         // Unset / empty — default to Monday
        }
    }
}

/// Days between `today_dow` (1=Mon..7=Sun) and the most recent
/// start-of-week (`week_start_dow`, same numbering). Result is
/// `0..=6`, inclusive of today (0 means today is the first day of
/// the week). Pure modular arithmetic — caller supplies both
/// integers from its native datetime API.
pub fn days_since_week_start(today_dow: i32, week_start_dow: i32) -> i32 {
    (today_dow - week_start_dow + 7).rem_euclid(7)
}

/// Returns (seconds this calendar week so far, seconds in the same
/// portion of last week). Weeks start on `week_start_dow`
/// (1=Mon..7=Sun). The comparison is apples-to-apples: on
/// Wednesday we compare Mon–Wed of the current week against
/// Mon–Wed of the previous week.
///
/// `daily_totals` is a list of `(YYYY-MM-DD, seconds)` pairs in any
/// order; this helper maps it internally. `now_unix` is the
/// reference moment as a Unix timestamp (UTC seconds); chrono::Local
/// converts it to local-time day-of-week + date keys.
pub fn week_over_week(
    daily_totals: &[(String, i64)],
    now_unix: i64,
    week_start_dow: i32,
) -> (i64, i64) {
    use chrono::{Datelike, Duration, TimeZone};
    let map: HashMap<&str, i64> = daily_totals.iter().map(|(k, v)| (k.as_str(), *v)).collect();
    let Some(now) = chrono::Local.timestamp_opt(now_unix, 0).single() else {
        return (0, 0);
    };
    let today_dow = now.weekday().number_from_monday() as i32;
    let days_elapsed = days_since_week_start(today_dow, week_start_dow) + 1;
    let today = now.date_naive();
    let sum_range = |start_offset: i32| -> i64 {
        (0..days_elapsed)
            .filter_map(|i| {
                let offset = (start_offset - i) as i64;
                let dt = today.checked_add_signed(Duration::days(offset))?;
                let key = dt.format("%Y-%m-%d").to_string();
                Some(map.get(key.as_str()).copied().unwrap_or(0))
            })
            .sum()
    };
    (sum_range(0), sum_range(-7))
}

/// X-axis label decision for the stats chart at bar `i`. The shell
/// maps each variant to its native locale-aware rendering (gtk uses
/// `glib::DateTime::format`, Android uses its native formatter).
/// `Empty` means render nothing at this bar (label thinning).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XLabelKind {
    /// Localised weekday name (Mon / Tue / …). Used in the 7-day view.
    Weekday,
    /// Localised month abbreviation + day-of-month (e.g. "Apr 17").
    /// Used in the 28-day view at every 7th bar.
    MonthShortDay,
    /// Single-letter month code (J/F/M/A/M/J/J/A/S/O/N/D).
    /// Used on longer views when the month changes from the
    /// previous bar.
    MonthLetter,
    /// Render nothing at this bar.
    Empty,
}

/// Pick the right `XLabelKind` for bar `i` in a chart covering
/// `days` total. `months` is the month-number (1..=12) of each
/// bar, in display order — the longer-view branch consults it to
/// detect month-change transitions.
pub fn x_label_kind(i: usize, days: u32, months: &[u32]) -> XLabelKind {
    match days {
        7 => XLabelKind::Weekday,
        28 => {
            if i % 7 == 0 {
                XLabelKind::MonthShortDay
            } else {
                XLabelKind::Empty
            }
        }
        _ => {
            let cur = months.get(i).copied().unwrap_or(0);
            let prev = if i == 0 {
                0
            } else {
                months.get(i - 1).copied().unwrap_or(0)
            };
            if cur != prev {
                XLabelKind::MonthLetter
            } else {
                XLabelKind::Empty
            }
        }
    }
}

/// Period selector for the stats chart. `days()` returns how many
/// trailing calendar days the chart should cover. Persisted to
/// settings as the `as_db_str` form so the toggle survives restart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChartPeriod {
    Week,
    FourWeeks,
    ThreeMonths,
    OneYear,
}

impl ChartPeriod {
    /// How many trailing days this period covers.
    pub fn days(self) -> u32 {
        match self {
            ChartPeriod::Week => 7,
            ChartPeriod::FourWeeks => 28,
            ChartPeriod::ThreeMonths => 90,
            ChartPeriod::OneYear => 365,
        }
    }

    /// Stable string form for persisted settings + toggle-group
    /// active-name values. Order matches the gtk shell's existing
    /// ToggleGroup children so a DB written before this migration
    /// still parses.
    pub fn as_db_str(self) -> &'static str {
        match self {
            ChartPeriod::Week => "1w",
            ChartPeriod::FourWeeks => "4w",
            ChartPeriod::ThreeMonths => "3m",
            ChartPeriod::OneYear => "1y",
        }
    }

    /// Parse a persisted / toggle-group name back into the typed
    /// variant. Unknown / empty input falls back to `Week` — the
    /// default selection.
    pub fn from_db_str(s: &str) -> Self {
        match s {
            "4w" => ChartPeriod::FourWeeks,
            "3m" => ChartPeriod::ThreeMonths,
            "1y" => ChartPeriod::OneYear,
            _ => ChartPeriod::Week,
        }
    }
}

/// Roll a `(YYYY-MM-DD, secs)` daily series up into the granularity
/// the stats chart wants for `days`:
///
/// - `days >= 365`: collapse into months (same `YYYY-MM` prefix).
///   Each month-bar carries the sum of every day in that month
///   plus a representative date_str (the first day seen).
/// - `days >= 90`: collapse into chunks of 7 (weeks). The chunk's
///   first date_str represents the week.
/// - otherwise: daily, untouched.
///
/// Caller passes the dense daily series (with zero-filled gaps).
/// Returns owned `(String, i64)` rows in display order.
pub fn aggregate_for_chart_period(
    daily: &[(String, i64)],
    days: u32,
) -> Vec<(String, i64)> {
    if days >= 365 {
        let mut months: Vec<(String, i64)> = Vec::new();
        for (date_str, dur) in daily {
            // Month-key match — both keys are YYYY-MM-DD so the
            // first 7 chars are YYYY-MM.
            let same = months
                .last()
                .map(|(k, _)| k.len() >= 7 && date_str.len() >= 7 && k[..7] == date_str[..7])
                .unwrap_or(false);
            if same {
                months.last_mut().unwrap().1 += dur;
            } else {
                months.push((date_str.clone(), *dur));
            }
        }
        months
    } else if days >= 90 {
        daily
            .chunks(7)
            .map(|c| {
                let key = c.first().map(|(k, _)| k.clone()).unwrap_or_default();
                let sum = c.iter().map(|(_, d)| d).sum();
                (key, sum)
            })
            .collect()
    } else {
        daily.to_vec()
    }
}

/// Single-letter month code (J/F/M/…) for month-number `1..=12`.
/// Locale-independent — the longer chart views use this to label
/// month boundaries without committing to a translated string.
pub fn month_letter(month: u32) -> &'static str {
    match month {
        1 => "J",
        2 => "F",
        3 => "M",
        4 => "A",
        5 => "M",
        6 => "J",
        7 => "J",
        8 => "A",
        9 => "S",
        10 => "O",
        11 => "N",
        _ => "D",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn days_since_week_start_zero_when_today_is_start() {
        // Week starts Monday (1), today is Monday (1) → 0.
        assert_eq!(days_since_week_start(1, 1), 0);
        // Week starts Sunday (7), today is Sunday (7) → 0.
        assert_eq!(days_since_week_start(7, 7), 0);
    }

    #[test]
    fn days_since_week_start_wraps_correctly() {
        // Week starts Monday, today is Sunday → 6 days in.
        assert_eq!(days_since_week_start(7, 1), 6);
        // Week starts Sunday, today is Monday → 1 day in.
        assert_eq!(days_since_week_start(1, 7), 1);
        // Week starts Sunday, today is Saturday → 6 days in.
        assert_eq!(days_since_week_start(6, 7), 6);
    }

    #[test]
    fn days_since_week_start_midweek() {
        // Week starts Monday (1), today is Wednesday (3) → 2 days in.
        assert_eq!(days_since_week_start(3, 1), 2);
        // Week starts Sunday (7), today is Wednesday (3) → 3 days in.
        assert_eq!(days_since_week_start(3, 7), 3);
    }

    #[test]
    fn x_label_kind_seven_day_view_always_weekday() {
        for i in 0..7 {
            assert_eq!(x_label_kind(i, 7, &[]), XLabelKind::Weekday);
        }
    }

    #[test]
    fn x_label_kind_28_day_view_every_seventh_bar() {
        assert_eq!(x_label_kind(0, 28, &[]), XLabelKind::MonthShortDay);
        assert_eq!(x_label_kind(6, 28, &[]), XLabelKind::Empty);
        assert_eq!(x_label_kind(7, 28, &[]), XLabelKind::MonthShortDay);
        assert_eq!(x_label_kind(14, 28, &[]), XLabelKind::MonthShortDay);
        assert_eq!(x_label_kind(15, 28, &[]), XLabelKind::Empty);
    }

    #[test]
    fn x_label_kind_long_view_letters_on_month_change() {
        // 90-day view (or any other != 7, 28). Month numbers per bar.
        // April for the first three bars, then May.
        let months = [4u32, 4, 4, 5, 5, 5, 5];
        assert_eq!(x_label_kind(0, 90, &months), XLabelKind::MonthLetter);
        assert_eq!(x_label_kind(1, 90, &months), XLabelKind::Empty);
        assert_eq!(x_label_kind(2, 90, &months), XLabelKind::Empty);
        // Month change at bar 3.
        assert_eq!(x_label_kind(3, 90, &months), XLabelKind::MonthLetter);
        assert_eq!(x_label_kind(4, 90, &months), XLabelKind::Empty);
    }

    #[test]
    fn x_label_kind_long_view_first_bar_always_labelled() {
        // The prev-month sentinel (0) at i==0 always trips a "month
        // change", so the first bar of a long view always gets the
        // month-letter — caller doesn't need a special case.
        let months = [4u32, 4, 4];
        assert_eq!(x_label_kind(0, 365, &months), XLabelKind::MonthLetter);
    }

    #[test]
    fn month_letter_covers_full_year() {
        let letters: Vec<&'static str> = (1..=12).map(month_letter).collect();
        assert_eq!(letters, vec!["J","F","M","A","M","J","J","A","S","O","N","D"]);
    }

    // ── ChartPeriod ───────────────────────────────────────────────────

    #[test]
    fn chart_period_days_per_variant() {
        assert_eq!(ChartPeriod::Week.days(), 7);
        assert_eq!(ChartPeriod::FourWeeks.days(), 28);
        assert_eq!(ChartPeriod::ThreeMonths.days(), 90);
        assert_eq!(ChartPeriod::OneYear.days(), 365);
    }

    #[test]
    fn chart_period_round_trip_via_db_str() {
        for p in [
            ChartPeriod::Week,
            ChartPeriod::FourWeeks,
            ChartPeriod::ThreeMonths,
            ChartPeriod::OneYear,
        ] {
            assert_eq!(ChartPeriod::from_db_str(p.as_db_str()), p);
        }
    }

    #[test]
    fn chart_period_unknown_input_falls_back_to_week() {
        assert_eq!(ChartPeriod::from_db_str(""), ChartPeriod::Week);
        assert_eq!(ChartPeriod::from_db_str("nonsense"), ChartPeriod::Week);
        // The "1w" alias matches the default-not-explicit case.
        assert_eq!(ChartPeriod::from_db_str("1w"), ChartPeriod::Week);
    }

    // ── aggregate_for_chart_period ────────────────────────────────────

    fn daily(rows: &[(&str, i64)]) -> Vec<(String, i64)> {
        rows.iter().map(|(d, v)| (d.to_string(), *v)).collect()
    }

    #[test]
    fn aggregate_short_periods_stay_daily() {
        let rows = daily(&[
            ("2024-04-15", 10),
            ("2024-04-16", 20),
            ("2024-04-17", 30),
        ]);
        assert_eq!(aggregate_for_chart_period(&rows, 7), rows);
        assert_eq!(aggregate_for_chart_period(&rows, 28), rows);
    }

    #[test]
    fn aggregate_three_month_view_chunks_into_weeks_of_seven() {
        let mut rows: Vec<(String, i64)> = Vec::new();
        for i in 0..14 {
            rows.push((format!("2024-04-{:02}", i + 1), 10));
        }
        let out = aggregate_for_chart_period(&rows, 90);
        // 14 days → 2 weeks → 2 rows of 70 each.
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].1, 70);
        assert_eq!(out[1].1, 70);
        // Key of each chunk is the first day of the week.
        assert_eq!(out[0].0, "2024-04-01");
        assert_eq!(out[1].0, "2024-04-08");
    }

    #[test]
    fn aggregate_one_year_view_collapses_into_months() {
        let mut rows: Vec<(String, i64)> = Vec::new();
        // 5 days in April + 3 days in May.
        for d in 28..=30 {
            rows.push((format!("2024-04-{d}"), 10));
        }
        for d in 1..=3 {
            rows.push((format!("2024-05-0{d}"), 20));
        }
        let out = aggregate_for_chart_period(&rows, 365);
        assert_eq!(out.len(), 2);
        // First month: 3 × 10 = 30. Second: 3 × 20 = 60.
        assert_eq!(out[0].1, 30);
        assert_eq!(out[1].1, 60);
    }

    #[test]
    fn month_letter_falls_back_to_december_for_out_of_range() {
        // Defensive — match the pre-migration gtk behaviour.
        assert_eq!(month_letter(0), "D");
        assert_eq!(month_letter(13), "D");
        assert_eq!(month_letter(99), "D");
    }

    // ── week_over_week ─────────────────────────────────────────────────

    #[test]
    fn week_over_week_returns_zero_when_no_data() {
        let (this_week, last_week) = week_over_week(&[], 1_700_000_000, 1);
        assert_eq!(this_week, 0);
        assert_eq!(last_week, 0);
    }

    #[test]
    fn week_over_week_sums_only_within_current_week_window() {
        // Pin a Wednesday: 2024-04-17 12:00 local. Build daily totals
        // for that whole week + the previous one, and check both
        // sides of the comparison.
        // Local-tz-aware via chrono::Local; we just need a moment
        // whose local-time weekday is Wednesday on the test machine.
        // 2024-04-17 12:00 UTC is Wednesday in every timezone from
        // -12 to +12 hours away.
        use chrono::TimeZone;
        let now = chrono::Local.with_ymd_and_hms(2024, 4, 17, 12, 0, 0).single();
        let Some(now) = now else { return; }; // skip on exotic tz
        let now_unix = now.timestamp();

        let totals = vec![
            ("2024-04-15".into(), 100),   // Monday this week
            ("2024-04-16".into(), 200),   // Tuesday this week
            ("2024-04-17".into(), 300),   // Wednesday this week (today)
            ("2024-04-18".into(), 999),   // Thursday — should NOT count
            ("2024-04-08".into(),  10),   // Monday last week
            ("2024-04-09".into(),  20),   // Tuesday last week
            ("2024-04-10".into(),  30),   // Wednesday last week
            ("2024-04-11".into(), 888),   // Thursday last week — should NOT count
        ];
        let (this_week, last_week) = week_over_week(&totals, now_unix, 1);
        assert_eq!(this_week, 600);
        assert_eq!(last_week, 60);
    }
}
