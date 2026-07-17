//! Daily meditation goal: progress math + status partition.
//!
//! The Stats view's hero "ring" renders the user's progress toward a
//! daily minutes target (switched from a weekly target 2026-07-17 —
//! Janek wants "how am I doing *today*", and the heatmap threshold
//! becomes the goal itself instead of a derived weekly share). The
//! math (clamped fraction for the arc, capped integer percent for
//! the label, remaining minutes, reached vs. in-progress decision)
//! is the same on every shell; the cairo / Skia drawing is
//! shell-specific. This module owns the math.
//!
//! No legacy fallback to the old `weekly_goal_mins` key — solo
//! user, re-entering the goal once is the accepted migration.

use crate::db::Database;

/// Lower bound (minutes per day) for the goal input. Lower would
/// land every practised day in heatmap level 4 from minute one.
pub const DAILY_GOAL_MIN: i64 = 5;

/// Upper bound. Above 4 hours/day the input stops being a
/// realistic target and starts being a typo magnet.
pub const DAILY_GOAL_MAX: i64 = 240;

/// Spin step for shells that use a stepper (the gtk `SpinRow`).
pub const DAILY_GOAL_STEP: i64 = 5;

/// Default for a fresh user — 20 min/day, the same ballpark the
/// old weekly default (150/7 ≈ 21) resolved to.
pub const DAILY_GOAL_DEFAULT: i64 = 20;

/// Read the user's persisted daily goal in minutes, falling back to
/// `DAILY_GOAL_DEFAULT` when the row is missing, unparseable, or
/// non-positive (a zero goal would collapse the ring math).
pub fn daily_goal_mins_from_db(db: &Database) -> i64 {
    db.get_setting("daily_goal_mins", &DAILY_GOAL_DEFAULT.to_string())
        .ok()
        .and_then(|s| s.parse::<i64>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(DAILY_GOAL_DEFAULT)
}

/// Persist a new daily goal. The shell calls this from its
/// preferences goal input.
pub fn write_daily_goal_mins(db: &Database, mins: i64) -> crate::db::Result<()> {
    db.set_setting("daily_goal_mins", &mins.to_string())
}

/// Where the user currently stands against the daily goal. The
/// shell maps `Reached` to a "✓" suffix on the sub-label and renders
/// the ring as a full circle; `InProgress` shows the "{duration} to
/// go today" copy and renders the arc up to `arc_pct`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GoalStatus {
    /// Met or exceeded the daily minutes target.
    Reached,
    /// Still under target — `remaining_mins > 0`.
    InProgress,
}

/// Full progress snapshot for the daily-goal ring + its
/// surrounding labels. All fields are pre-resolved math; the shell
/// only renders. `display_pct` is the capped integer for the
/// big-number label ("99%" / "153%"); `arc_pct` is the 0.0..1.0
/// fraction the ring drawing should sweep (capped at 1.0 so the
/// arc never visually wraps around once goal is met).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GoalProgress {
    pub today_mins: i64,
    pub goal_mins: i64,
    /// 0.0..=1.0 — the arc-sweep fraction the ring should draw.
    /// Clamped so over-goal days render a full circle.
    pub arc_pct: f64,
    /// Integer percent for the big-number label, clamped to
    /// 0..=999 (matches the gtk shell's `clamp(0.0, 9.99) * 100`).
    pub display_pct: i32,
    /// Minutes remaining to reach the goal. Saturating-subtracted;
    /// zero once goal is met or exceeded.
    pub remaining_mins: i64,
    pub status: GoalStatus,
}

/// Compute the daily-goal snapshot from today's elapsed seconds
/// and the user's persisted minute target. `goal_mins == 0` (or
/// negative) collapses to "InProgress with arc_pct=0" — the
/// shell's goal setting can't actually reach that state (it falls
/// back to a positive default), but the math is total either way.
pub fn compute(today_secs: i64, goal_mins: i64) -> GoalProgress {
    let today_mins = today_secs.max(0) / 60;
    if goal_mins <= 0 {
        return GoalProgress {
            today_mins,
            goal_mins,
            arc_pct: 0.0,
            display_pct: 0,
            remaining_mins: 0,
            status: GoalStatus::InProgress,
        };
    }
    let raw_pct = today_mins as f64 / goal_mins as f64;
    let arc_pct = raw_pct.clamp(0.0, 1.0);
    let display_pct = (raw_pct.clamp(0.0, 9.99) * 100.0).round() as i32;
    let remaining_mins = (goal_mins - today_mins).max(0);
    let status = if remaining_mins == 0 {
        GoalStatus::Reached
    } else {
        GoalStatus::InProgress
    };
    GoalProgress {
        today_mins,
        goal_mins,
        arc_pct,
        display_pct,
        remaining_mins,
        status,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_macros::assert_f64_eq;

    #[test]
    fn empty_day_is_in_progress_with_zero_pct() {
        let g = compute(0, 20);
        assert_eq!(g.today_mins, 0);
        assert_eq!(g.display_pct, 0);
        assert_f64_eq!(g.arc_pct, 0.0);
        assert_eq!(g.remaining_mins, 20);
        assert_eq!(g.status, GoalStatus::InProgress);
    }

    #[test]
    fn exact_target_is_reached() {
        let g = compute(20 * 60, 20);
        assert_eq!(g.display_pct, 100);
        assert_f64_eq!(g.arc_pct, 1.0);
        assert_eq!(g.remaining_mins, 0);
        assert_eq!(g.status, GoalStatus::Reached);
    }

    #[test]
    fn over_goal_caps_arc_at_one_but_label_keeps_climbing() {
        // 30 min vs 20 min goal — 150% of goal.
        let g = compute(30 * 60, 20);
        assert_eq!(g.today_mins, 30);
        assert_f64_eq!(g.arc_pct, 1.0, "arc must cap so it doesn't wrap");
        assert_eq!(g.display_pct, 150, "label keeps climbing");
        assert_eq!(g.remaining_mins, 0);
        assert_eq!(g.status, GoalStatus::Reached);
    }

    #[test]
    fn display_pct_caps_at_999_for_pathological_overshoot() {
        let g = compute(10_000 * 60, 1);
        assert_eq!(g.display_pct, 999);
    }

    #[test]
    fn negative_seconds_clamp_to_zero() {
        let g = compute(-5, 20);
        assert_eq!(g.today_mins, 0);
        assert_eq!(g.remaining_mins, 20);
    }

    #[test]
    fn zero_or_negative_goal_collapses_to_safe_zero() {
        let g = compute(60 * 60, 0);
        assert_eq!(g.display_pct, 0);
        assert_f64_eq!(g.arc_pct, 0.0);
        assert_eq!(g.status, GoalStatus::InProgress);
    }

    #[test]
    fn read_daily_goal_falls_back_to_default() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(daily_goal_mins_from_db(&db), DAILY_GOAL_DEFAULT);
        db.set_setting("daily_goal_mins", "garbage").unwrap();
        assert_eq!(daily_goal_mins_from_db(&db), DAILY_GOAL_DEFAULT);
        db.set_setting("daily_goal_mins", "0").unwrap();
        assert_eq!(daily_goal_mins_from_db(&db), DAILY_GOAL_DEFAULT, "zero filters out");
        db.set_setting("daily_goal_mins", "-10").unwrap();
        assert_eq!(daily_goal_mins_from_db(&db), DAILY_GOAL_DEFAULT, "negative filters out");
    }

    #[test]
    fn read_daily_goal_returns_persisted_value() {
        let db = Database::open_in_memory().unwrap();
        write_daily_goal_mins(&db, 45).unwrap();
        assert_eq!(daily_goal_mins_from_db(&db), 45);
    }

    #[test]
    fn stale_weekly_key_is_ignored_not_migrated() {
        // The old weekly_goal_mins row may still exist in synced
        // DBs; the daily reader must not consult it (no-compat
        // policy — re-entering the goal once is the migration).
        let db = Database::open_in_memory().unwrap();
        db.set_setting("weekly_goal_mins", "700").unwrap();
        assert_eq!(daily_goal_mins_from_db(&db), DAILY_GOAL_DEFAULT);
    }
}
