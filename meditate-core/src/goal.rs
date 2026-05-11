//! Weekly meditation goal: progress math + status partition.
//!
//! The Stats view's hero "ring" renders the user's progress toward a
//! weekly minutes target. The math (clamped fraction for the arc,
//! capped integer percent for the label, remaining minutes, reached
//! vs. in-progress decision) is the same on every shell; the
//! cairo / Skia / Compose drawing is shell-specific. This module
//! owns the math.

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

    #[test]
    fn empty_week_is_in_progress_with_zero_pct() {
        let g = compute(0, 150);
        assert_eq!(g.week_mins, 0);
        assert_eq!(g.display_pct, 0);
        assert_eq!(g.arc_pct, 0.0);
        assert_eq!(g.remaining_mins, 150);
        assert_eq!(g.status, GoalStatus::InProgress);
    }

    #[test]
    fn exact_target_is_reached() {
        let g = compute(150 * 60, 150);
        assert_eq!(g.display_pct, 100);
        assert_eq!(g.arc_pct, 1.0);
        assert_eq!(g.remaining_mins, 0);
        assert_eq!(g.status, GoalStatus::Reached);
    }

    #[test]
    fn over_goal_caps_arc_at_one_but_label_keeps_climbing() {
        // 3 hours 45 min vs 150 min goal — 150% of goal.
        let g = compute(225 * 60, 150);
        assert_eq!(g.week_mins, 225);
        assert_eq!(g.arc_pct, 1.0, "arc must cap so it doesn't wrap");
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
        assert_eq!(g.arc_pct, 0.0);
        assert_eq!(g.status, GoalStatus::InProgress);
    }
}
