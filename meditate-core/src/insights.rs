//! Stats-view insight computation: the typed enum a shell renders
//! as a vertical list of insight cards on the Stats tab.
//!
//! Pure decision logic. The shell collects the DB-driven inputs into
//! an `InsightInput`, calls `compute`, gets back a `Vec<InsightKey>`,
//! and maps each variant to gettext-translated card text. Threshold
//! rules ("preferred time only if ≥10 sessions", "typical only if
//! ≥5", milestone gate, etc.) all live here so the Android shell
//! shows the same cards under the same conditions.

use crate::date_math;
use crate::format::next_session_milestone;

/// Hour-of-day bucket the user practises most in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HourBucket {
    Morning,
    Afternoon,
    Evening,
}

/// Every signal the insight compute needs, in one struct. The shell
/// fills it from a single DB borrow at refresh time.
#[derive(Debug, Default, Clone)]
pub struct InsightInput {
    pub current_streak: u32,
    pub best_streak: u32,
    pub this_month_secs: i64,
    pub last_month_secs: i64,
    /// 14 days of `(YYYY-MM-DD, secs)` rows, oldest first. Used by
    /// the week-over-week comparison.
    pub daily_totals: Vec<(String, i64)>,
    /// Longest single session ever: `(duration_secs, start_unix)`.
    /// `None` when no sessions are stored.
    pub longest: Option<(i64, i64)>,
    /// Typical (median) session duration in seconds. `0` means
    /// unknown — the typical-session card hides in that case.
    pub typical_secs: i64,
    /// Running average over the last 7 days, in seconds. Zero-days
    /// included.
    pub avg_secs_7d: i64,
    /// Session-count buckets: `(morning, afternoon, evening)`.
    pub hour_buckets: (i64, i64, i64),
    pub session_count: i64,
}

/// Typed key the shell renders into a translated card. Variants
/// carry the substitution values; the shell composes the gettext
/// template + interpolates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InsightKey {
    /// `is_record` = current streak matches or exceeds best AND
    /// is > 1 day (a single-day streak isn't a record worth
    /// celebrating, even if it equals the best).
    CurrentStreak {
        days: u32,
        is_record: bool,
        best: u32,
    },
    /// Up if pct >= 0, down otherwise. Always shows the
    /// percentage's absolute value alongside the icon.
    WeekOverWeek {
        pct: i32,
        this_secs: i64,
        last_secs: i64,
    },
    /// Same shape for month-over-month — distinct variant so the
    /// shell can use a longer-horizon title.
    MonthTrend {
        pct: i32,
        this_secs: i64,
        last_secs: i64,
    },
    PreferredTime {
        bucket: HourBucket,
        pct: i32,
    },
    TypicalSession {
        duration_secs: i64,
    },
    LongestSession {
        duration_secs: i64,
        start_unix: i64,
    },
    NextMilestone {
        target: i64,
        remaining: i64,
    },
    DailyRhythm {
        avg_secs: i64,
    },
    /// Fallback when no other variant fires — typically a fresh
    /// install with no sessions yet.
    NoData,
}

impl InsightKey {
    /// Single-character glyph the shell uses as the card's icon. The
    /// mapping is portable so Android renders the same icons; only
    /// the up/down trend arrows vary with the variant's own data.
    pub fn glyph(&self) -> &'static str {
        match self {
            InsightKey::CurrentStreak { .. } => "●",
            InsightKey::WeekOverWeek { pct, .. } | InsightKey::MonthTrend { pct, .. } => {
                if *pct >= 0 { "↗" } else { "↘" }
            }
            InsightKey::PreferredTime { .. } => "◔",
            InsightKey::TypicalSession { .. } => "≈",
            InsightKey::LongestSession { .. } => "◆",
            InsightKey::NextMilestone { .. } => "⚑",
            InsightKey::DailyRhythm { .. } => "◷",
            InsightKey::NoData => "✦",
        }
    }

    /// Whether the shell should accent (gtk: `accent` CSS class)
    /// this card. Reserved for variants that signal a noteworthy
    /// milestone — a current-streak record and the lifetime-longest
    /// session today. Trend / threshold cards stay neutral.
    pub fn is_accent(&self) -> bool {
        match self {
            InsightKey::CurrentStreak { is_record, .. } => *is_record,
            InsightKey::LongestSession { .. } => true,
            _ => false,
        }
    }
}

/// Run every insight branch and return the variants worth showing.
/// Order matters: the shell renders cards top-to-bottom in the
/// returned order, which matches the gtk shell's prior layout.
///
/// `now_unix` is the reference moment; `week_start_dow` is the
/// locale's first weekday (1=Mon..7=Sun, see
/// `meditate_core::date_math::locale_week_start_dow`).
pub fn compute(input: &InsightInput, now_unix: i64, week_start_dow: i32) -> Vec<InsightKey> {
    let mut out: Vec<InsightKey> = Vec::new();

    // 1. Current streak.
    if input.current_streak > 0 {
        let is_record = input.current_streak >= input.best_streak && input.current_streak > 1;
        out.push(InsightKey::CurrentStreak {
            days: input.current_streak,
            is_record,
            best: input.best_streak,
        });
    }

    // 2. Week-over-week (only when last week has data).
    let (this_week, last_week) =
        date_math::week_over_week(&input.daily_totals, now_unix, week_start_dow);
    if last_week > 0 {
        let delta = this_week - last_week;
        let pct = (delta as f64 / last_week as f64 * 100.0).round() as i32;
        out.push(InsightKey::WeekOverWeek {
            pct,
            this_secs: this_week,
            last_secs: last_week,
        });
    }

    // 3. Month trend (only when last month has data).
    if input.last_month_secs > 0 {
        let delta = input.this_month_secs - input.last_month_secs;
        let pct = (delta as f64 / input.last_month_secs as f64 * 100.0).round() as i32;
        out.push(InsightKey::MonthTrend {
            pct,
            this_secs: input.this_month_secs,
            last_secs: input.last_month_secs,
        });
    }

    // 4. Preferred time of day — only meaningful with ≥10 sessions
    // in the bucket total. Tie-break: morning > evening > afternoon
    // (matches the gtk shell's prior `>=` order).
    let (morn, afte, even) = input.hour_buckets;
    let bucket_total = morn + afte + even;
    if bucket_total >= 10 {
        let (bucket, count) = if morn >= afte && morn >= even {
            (HourBucket::Morning, morn)
        } else if even >= afte {
            (HourBucket::Evening, even)
        } else {
            (HourBucket::Afternoon, afte)
        };
        let pct = (count as f64 / bucket_total as f64 * 100.0).round() as i32;
        out.push(InsightKey::PreferredTime { bucket, pct });
    }

    // 5. Typical (median) session — needs ≥5 sessions to stop
    // being dominated by outliers.
    if input.session_count >= 5 && input.typical_secs > 0 {
        out.push(InsightKey::TypicalSession {
            duration_secs: input.typical_secs,
        });
    }

    // 6. Longest session ever (if any).
    if let Some((duration_secs, start_unix)) = input.longest {
        out.push(InsightKey::LongestSession {
            duration_secs,
            start_unix,
        });
    }

    // 7. Next milestone — needs ≥5 sessions so "12 until your 5th"
    // doesn't feel patronising to a fresh user.
    if input.session_count >= 5 {
        if let Some((target, remaining)) = next_session_milestone(input.session_count) {
            out.push(InsightKey::NextMilestone { target, remaining });
        }
    }

    // 8. Daily rhythm (7-day running average) — complements
    // TypicalSession by including zero-days.
    if input.avg_secs_7d > 0 {
        out.push(InsightKey::DailyRhythm {
            avg_secs: input.avg_secs_7d,
        });
    }

    if out.is_empty() {
        out.push(InsightKey::NoData);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn baseline() -> InsightInput {
        InsightInput::default()
    }

    #[test]
    fn no_data_when_everything_is_empty() {
        let out = compute(&baseline(), 1_700_000_000, 1);
        assert_eq!(out, vec![InsightKey::NoData]);
    }

    #[test]
    fn current_streak_one_day_is_not_a_record() {
        let mut input = baseline();
        input.current_streak = 1;
        input.best_streak = 5;
        let out = compute(&input, 1_700_000_000, 1);
        assert!(out.contains(&InsightKey::CurrentStreak {
            days: 1,
            is_record: false,
            best: 5
        }));
    }

    #[test]
    fn current_streak_matches_best_marks_record() {
        let mut input = baseline();
        input.current_streak = 7;
        input.best_streak = 7;
        let out = compute(&input, 1_700_000_000, 1);
        assert!(out.contains(&InsightKey::CurrentStreak {
            days: 7,
            is_record: true,
            best: 7,
        }));
    }

    #[test]
    fn preferred_time_only_with_at_least_ten_sessions() {
        let mut input = baseline();
        // 9 total sessions: no PreferredTime card.
        input.hour_buckets = (5, 2, 2);
        let out = compute(&input, 1_700_000_000, 1);
        assert!(
            !out
                .iter()
                .any(|k| matches!(k, InsightKey::PreferredTime { .. })),
            "PreferredTime must hide at <10 sessions in buckets"
        );
        // 10 sessions: card appears, picks morning.
        input.hour_buckets = (5, 2, 3);
        let out = compute(&input, 1_700_000_000, 1);
        assert!(out.contains(&InsightKey::PreferredTime {
            bucket: HourBucket::Morning,
            pct: 50,
        }));
    }

    #[test]
    fn preferred_time_picks_evening_over_afternoon_on_tie() {
        let mut input = baseline();
        // Equal evening + afternoon, morning lower → evening wins
        // because the gtk shell used `even >= afte` after the
        // morning branch failed.
        input.hour_buckets = (1, 5, 5);
        let out = compute(&input, 1_700_000_000, 1);
        assert!(out.contains(&InsightKey::PreferredTime {
            bucket: HourBucket::Evening,
            pct: 45,
        }));
    }

    #[test]
    fn typical_session_only_with_at_least_five_sessions() {
        let mut input = baseline();
        input.typical_secs = 600;
        input.session_count = 4;
        let out = compute(&input, 1_700_000_000, 1);
        assert!(!out
            .iter()
            .any(|k| matches!(k, InsightKey::TypicalSession { .. })));
        input.session_count = 5;
        let out = compute(&input, 1_700_000_000, 1);
        assert!(out.contains(&InsightKey::TypicalSession {
            duration_secs: 600
        }));
    }

    #[test]
    fn longest_session_appears_only_when_some() {
        let mut input = baseline();
        let out = compute(&input, 1_700_000_000, 1);
        assert!(!out
            .iter()
            .any(|k| matches!(k, InsightKey::LongestSession { .. })));
        input.longest = Some((3600, 1_700_000_000));
        let out = compute(&input, 1_700_000_000, 1);
        assert!(out.contains(&InsightKey::LongestSession {
            duration_secs: 3600,
            start_unix: 1_700_000_000,
        }));
    }

    #[test]
    fn next_milestone_only_with_at_least_five_sessions() {
        let mut input = baseline();
        input.session_count = 4;
        let out = compute(&input, 1_700_000_000, 1);
        assert!(!out
            .iter()
            .any(|k| matches!(k, InsightKey::NextMilestone { .. })));
        input.session_count = 7;
        let out = compute(&input, 1_700_000_000, 1);
        // First milestone past 7 is 10. 10 - 7 = 3 remaining.
        assert!(out.contains(&InsightKey::NextMilestone {
            target: 10,
            remaining: 3
        }));
    }

    #[test]
    fn daily_rhythm_only_when_avg_positive() {
        let mut input = baseline();
        input.avg_secs_7d = 0;
        let out = compute(&input, 1_700_000_000, 1);
        assert!(!out
            .iter()
            .any(|k| matches!(k, InsightKey::DailyRhythm { .. })));
        input.avg_secs_7d = 600;
        let out = compute(&input, 1_700_000_000, 1);
        assert!(out.contains(&InsightKey::DailyRhythm { avg_secs: 600 }));
    }

    #[test]
    fn order_is_streak_first_then_week_then_month_etc() {
        let mut input = baseline();
        input.current_streak = 3;
        input.best_streak = 5;
        input.this_month_secs = 4 * 3600;
        input.last_month_secs = 3 * 3600;
        input.session_count = 12;
        input.typical_secs = 600;
        input.longest = Some((1800, 1_700_000_000));
        input.avg_secs_7d = 700;
        input.hour_buckets = (5, 3, 4);
        let out = compute(&input, 1_700_000_000, 1);
        // First card must be CurrentStreak; LongestSession before
        // NextMilestone before DailyRhythm.
        assert!(matches!(out[0], InsightKey::CurrentStreak { .. }));
        let pos = |k: &InsightKey| out.iter().position(|x| x == k).unwrap();
        let longest = InsightKey::LongestSession {
            duration_secs: 1800,
            start_unix: 1_700_000_000,
        };
        let milestone = InsightKey::NextMilestone {
            target: 25,
            remaining: 13,
        };
        let rhythm = InsightKey::DailyRhythm { avg_secs: 700 };
        assert!(out.contains(&longest));
        assert!(out.contains(&milestone));
        assert!(out.contains(&rhythm));
        assert!(pos(&longest) < pos(&milestone));
        assert!(pos(&milestone) < pos(&rhythm));
    }

    #[test]
    fn glyph_per_variant_is_stable_or_pct_signed() {
        assert_eq!(
            InsightKey::CurrentStreak { days: 1, is_record: false, best: 1 }.glyph(),
            "●"
        );
        assert_eq!(
            InsightKey::WeekOverWeek { pct: 12, this_secs: 0, last_secs: 0 }.glyph(),
            "↗"
        );
        assert_eq!(
            InsightKey::WeekOverWeek { pct: -3, this_secs: 0, last_secs: 0 }.glyph(),
            "↘"
        );
        assert_eq!(
            InsightKey::MonthTrend { pct: 0, this_secs: 0, last_secs: 0 }.glyph(),
            "↗"
        );
        assert_eq!(InsightKey::PreferredTime { bucket: HourBucket::Morning, pct: 50 }.glyph(), "◔");
        assert_eq!(InsightKey::TypicalSession { duration_secs: 600 }.glyph(), "≈");
        assert_eq!(InsightKey::LongestSession { duration_secs: 1, start_unix: 0 }.glyph(), "◆");
        assert_eq!(InsightKey::NextMilestone { target: 10, remaining: 1 }.glyph(), "⚑");
        assert_eq!(InsightKey::DailyRhythm { avg_secs: 600 }.glyph(), "◷");
        assert_eq!(InsightKey::NoData.glyph(), "✦");
    }

    #[test]
    fn is_accent_only_for_streak_record_and_longest_session() {
        assert!(InsightKey::CurrentStreak { days: 7, is_record: true, best: 7 }.is_accent());
        assert!(!InsightKey::CurrentStreak { days: 3, is_record: false, best: 7 }.is_accent());
        assert!(InsightKey::LongestSession { duration_secs: 1, start_unix: 0 }.is_accent());
        // Every other variant is neutral.
        assert!(!InsightKey::WeekOverWeek { pct: 0, this_secs: 0, last_secs: 0 }.is_accent());
        assert!(!InsightKey::DailyRhythm { avg_secs: 600 }.is_accent());
        assert!(!InsightKey::NoData.is_accent());
    }

    #[test]
    fn no_data_fallback_does_not_fire_when_any_card_present() {
        let mut input = baseline();
        input.current_streak = 1;
        input.best_streak = 1;
        let out = compute(&input, 1_700_000_000, 1);
        assert!(!out.iter().any(|k| matches!(k, InsightKey::NoData)));
    }
}
