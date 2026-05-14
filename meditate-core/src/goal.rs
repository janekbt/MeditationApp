//! Weekly meditation goal: progress math + status partition.
//!
//! The Stats view's hero "ring" renders the user's progress toward a
//! weekly minutes target. The math (clamped fraction for the arc,
//! capped integer percent for the label, remaining minutes, reached
//! vs. in-progress decision) is the same on every shell; the
//! cairo / Skia / Compose drawing is shell-specific. This module
//! owns the math.

use crate::db::Database;

/// Lower bound (minutes per week) for the goal spinner. Lower would
/// degenerate the heatmap thresholds — `daily_expected_mins`
/// floor-divides by 7 and `minutes_to_level` would land everything
/// in level 4 from minute one.
pub const WEEKLY_GOAL_MIN: i64 = 30;

/// Upper bound. Above ~17 hours/week the spinner stops being a
/// realistic target and starts being a typo magnet.
pub const WEEKLY_GOAL_MAX: i64 = 1000;

/// Spinner step — matches the gtk shell's `Adjustment`.
pub const WEEKLY_GOAL_STEP: i64 = 15;

/// Default for a fresh user — chosen to map cleanly to `daily_
/// expected_mins == 21` (~3 minutes per day plus tolerance).
pub const WEEKLY_GOAL_DEFAULT: i64 = 150;

/// Read the user's persisted weekly goal in minutes, falling back to
/// `WEEKLY_GOAL_DEFAULT` when the row is missing, unparseable, or
/// non-positive (a zero goal would collapse the daily-share math).
pub fn weekly_goal_mins_from_db(db: &Database) -> i64 {
    db.get_setting("weekly_goal_mins", &WEEKLY_GOAL_DEFAULT.to_string())
        .ok()
        .and_then(|s| s.parse::<i64>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(WEEKLY_GOAL_DEFAULT)
}

/// Persist a new weekly goal. The shell typically calls this from
/// the preferences `SpinRow`'s value-changed notify.
pub fn write_weekly_goal_mins(db: &Database, mins: i64) -> crate::db::Result<()> {
    db.set_setting("weekly_goal_mins", &mins.to_string())
}

/// Daily share of the weekly goal, used as the heatmap-cell
/// threshold by `contrib::build_grid`. Floor at 1 so a degenerate
/// weekly-goal value can't divide-by-zero downstream.
pub fn daily_expected_mins(weekly_goal_mins: i64) -> i64 {
    ((weekly_goal_mins.max(0) as f64) / 7.0).round().max(1.0) as i64
}

/// Where the user currently stands against the weekly goal. The
/// shell maps `Reached` to a "✓" suffix on the sub-label and renders
/// the ring as a full circle; `InProgress` shows the "{duration} to
/// go this week" copy and renders the arc up to `arc_pct`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GoalStatus {
    /// Met or exceeded the weekly minutes target.
    Reached,
    /// Still under target — `remaining_mins > 0`.
    InProgress,
}

/// Full progress snapshot for the weekly-goal ring + its
/// surrounding labels. All fields are pre-resolved math; the shell
/// only renders. `display_pct` is the capped integer for the
/// big-number label ("99%" / "153%"); `arc_pct` is the 0.0..1.0
/// fraction the ring drawing should sweep (capped at 1.0 so the
/// arc never visually wraps around once goal is met).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WeeklyGoal {
    pub week_mins: i64,
    pub goal_mins: i64,
    /// 0.0..=1.0 — the arc-sweep fraction the ring should draw.
    /// Clamped so over-goal weeks render a full circle.
    pub arc_pct: f64,
    /// Integer percent for the big-number label, clamped to
    /// 0..=999 (matches the gtk shell's `clamp(0.0, 9.99) * 100`).
    pub display_pct: i32,
    /// Minutes remaining to reach the goal. Saturating-subtracted;
    /// zero once goal is met or exceeded.
    pub remaining_mins: i64,
    pub status: GoalStatus,
}

/// Compute the weekly-goal snapshot from this-week's elapsed
/// seconds and the user's persisted minute target. `goal_mins == 0`
/// (or negative) collapses to "InProgress with arc_pct=0" — the
/// shell's goal setting can't actually reach that state (it falls
/// back to a positive default), but the math is total either way.
pub fn compute(week_secs: i64, goal_mins: i64) -> WeeklyGoal {
    let week_mins = week_secs.max(0) / 60;
    if goal_mins <= 0 {
        return WeeklyGoal {
            week_mins,
            goal_mins,
            arc_pct: 0.0,
            display_pct: 0,
            remaining_mins: 0,
            status: GoalStatus::InProgress,
        };
    }
    let raw_pct = week_mins as f64 / goal_mins as f64;
    let arc_pct = raw_pct.clamp(0.0, 1.0);
    let display_pct = (raw_pct.clamp(0.0, 9.99) * 100.0).round() as i32;
    let remaining_mins = (goal_mins - week_mins).max(0);
    let status = if remaining_mins == 0 {
        GoalStatus::Reached
    } else {
        GoalStatus::InProgress
    };
    WeeklyGoal {
        week_mins,
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
    fn empty_week_is_in_progress_with_zero_pct() {
        let g = compute(0, 150);
        assert_eq!(g.week_mins, 0);
        assert_eq!(g.display_pct, 0);
        assert_f64_eq!(g.arc_pct, 0.0);
        assert_eq!(g.remaining_mins, 150);
        assert_eq!(g.status, GoalStatus::InProgress);
    }

    #[test]
    fn exact_target_is_reached() {
        let g = compute(150 * 60, 150);
        assert_eq!(g.display_pct, 100);
        assert_f64_eq!(g.arc_pct, 1.0);
        assert_eq!(g.remaining_mins, 0);
        assert_eq!(g.status, GoalStatus::Reached);
    }

    #[test]
    fn over_goal_caps_arc_at_one_but_label_keeps_climbing() {
        // 3 hours 45 min vs 150 min goal — 150% of goal.
        let g = compute(225 * 60, 150);
        assert_eq!(g.week_mins, 225);
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
        let g = compute(-5, 150);
        assert_eq!(g.week_mins, 0);
        assert_eq!(g.remaining_mins, 150);
    }

    #[test]
    fn zero_or_negative_goal_collapses_to_safe_zero() {
        let g = compute(60 * 60, 0);
        assert_eq!(g.display_pct, 0);
        assert_f64_eq!(g.arc_pct, 0.0);
        assert_eq!(g.status, GoalStatus::InProgress);
    }

    #[test]
    fn read_weekly_goal_falls_back_to_default() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(weekly_goal_mins_from_db(&db), WEEKLY_GOAL_DEFAULT);
        db.set_setting("weekly_goal_mins", "garbage").unwrap();
        assert_eq!(weekly_goal_mins_from_db(&db), WEEKLY_GOAL_DEFAULT);
        db.set_setting("weekly_goal_mins", "0").unwrap();
        assert_eq!(weekly_goal_mins_from_db(&db), WEEKLY_GOAL_DEFAULT, "zero filters out");
        db.set_setting("weekly_goal_mins", "-10").unwrap();
        assert_eq!(weekly_goal_mins_from_db(&db), WEEKLY_GOAL_DEFAULT, "negative filters out");
    }

    #[test]
    fn read_weekly_goal_returns_persisted_value() {
        let db = Database::open_in_memory().unwrap();
        write_weekly_goal_mins(&db, 240).unwrap();
        assert_eq!(weekly_goal_mins_from_db(&db), 240);
    }

    #[test]
    fn daily_expected_mins_floors_at_one() {
        assert_eq!(daily_expected_mins(0), 1);
        assert_eq!(daily_expected_mins(-10), 1);
        assert_eq!(daily_expected_mins(7), 1, "exactly 1/day");
    }

    #[test]
    fn daily_expected_mins_rounds_to_nearest() {
        assert_eq!(daily_expected_mins(150), 21); // 21.43 → 21
        assert_eq!(daily_expected_mins(70), 10);
        assert_eq!(daily_expected_mins(140), 20);
    }
}
