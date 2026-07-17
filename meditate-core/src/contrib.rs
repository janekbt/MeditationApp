//! Contribution heatmap walker — the 13 × 7 grid the Stats view's
//! "last 12 weeks" card renders. Owns the per-cell classification
//! (Future / Today / Past) and the level-by-daily-goal decision so
//! every shell (GTK, Android, future Slint) reaches the same cells
//! from the same daily totals.
//!
//! The renderer stays in the shell: CSS classes, opacity, ★ glyph,
//! accessible-name composition. This module returns a typed
//! `Vec<ContribCell>` the shell iterates over.

use chrono::{Datelike, Duration, NaiveDate};
use std::collections::HashMap;

use crate::date_math::days_since_week_start;
use crate::format::minutes_to_level;

/// The grid's column count — 12 prior weeks plus the current week.
pub(crate) const CONTRIB_COLS: usize = 13;
/// The grid's row count — one row per weekday.
pub(crate) const CONTRIB_ROWS: usize = 7;

/// One cell of the contrib heatmap. The shell maps each variant /
/// field to its native rendering: future days dim, past/today days
/// pick a CSS level class from `level`, today gets a highlight, and
/// `level == 4` gets the goal-exceeded glyph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContribCell {
    /// ISO-formatted date string the cell represents. Carried so the
    /// shell's accessible-name composer can re-format via its native
    /// datetime API.
    pub date_iso: String,
    /// `true` for any date strictly later than `today` — the shell
    /// renders these dimmed and skips the level/glyph logic.
    pub is_future: bool,
    /// `true` for the cell whose date equals `today` — the shell
    /// adds its highlight CSS class on top of the level class.
    pub is_today: bool,
    /// Total minutes meditated on this date. Zero for future days
    /// and for past days with no sessions.
    pub mins: i64,
    /// Heatmap level `0..=4` from `format::minutes_to_level`. Zero
    /// for future days; non-zero past days bucket by daily share of
    /// the weekly goal.
    pub level: u8,
}

impl ContribCell {
    /// Convenience for the "show a goal-exceeded glyph" decision
    /// (GTK uses ★). Equivalent to `level == 4`; named so callers
    /// don't sprinkle the magic number.
    pub fn is_goal_exceeded(&self) -> bool {
        self.level == 4
    }
}

/// Build the 13 × 7 contrib grid for the Stats view. The cells are
/// returned in column-major order (oldest week first, then top-to-
/// bottom within each week starting at the locale's first weekday),
/// which matches the gtk grid layout and lets the shell index by
/// `col * 7 + row`.
///
/// - `today` anchors the rightmost column; the grid spans
///   `[today − 12 weeks aligned to week start, today]`.
/// - `locale_first_dow` is the locale's first weekday (1=Mon..7=Sun)
///   — typically `date_math::locale_week_start_dow()`. Determines
///   which weekday sits on row 0.
/// - `totals` is a date → seconds map. Days missing from the map
///   contribute zero.
/// - `daily_goal_mins` is the user's daily goal, driving the
///   level thresholds directly (so a 10-hour retreat day doesn't
///   make on-target days look washed-out).
pub fn build_grid(
    today: NaiveDate,
    locale_first_dow: i32,
    totals: &HashMap<NaiveDate, i64>,
    daily_goal_mins: i64,
) -> Vec<ContribCell> {
    let today_dow = today.weekday().number_from_monday() as i32;
    let cur_week_start = today
        - Duration::days(i64::from(days_since_week_start(today_dow, locale_first_dow)));

    let mut cells = Vec::with_capacity(CONTRIB_COLS * CONTRIB_ROWS);
    for col in 0..CONTRIB_COLS as i64 {
        let weeks_ago = (CONTRIB_COLS as i64 - 1) - col;
        let week_start = cur_week_start - Duration::weeks(weeks_ago);
        for row in 0..CONTRIB_ROWS as i64 {
            let date = week_start + Duration::days(row);
            let is_future = date > today;
            let is_today = date == today;
            let secs = totals.get(&date).copied().unwrap_or(0);
            let mins = if is_future { 0 } else { secs / 60 };
            let level = if is_future {
                0
            } else {
                minutes_to_level(mins, daily_goal_mins)
            };
            cells.push(ContribCell {
                date_iso: date.format("%Y-%m-%d").to_string(),
                is_future,
                is_today,
                mins,
                level,
            });
        }
    }
    cells
}

#[cfg(test)]
mod tests {
    use super::*;

    fn iso(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn grid_has_91_cells() {
        let totals = HashMap::new();
        let cells = build_grid(iso("2026-05-11"), 1, &totals, 21);
        assert_eq!(cells.len(), CONTRIB_COLS * CONTRIB_ROWS);
    }

    #[test]
    fn first_cell_is_12_weeks_before_current_week_start() {
        // 2026-05-11 is a Monday. Monday-start locale (1) → week_start
        // is 2026-05-11 itself; 12 weeks earlier is 2026-02-16.
        let totals = HashMap::new();
        let cells = build_grid(iso("2026-05-11"), 1, &totals, 21);
        assert_eq!(cells[0].date_iso, "2026-02-16");
    }

    #[test]
    fn last_cell_in_current_week_includes_today() {
        // Monday week start → today (Mon 2026-05-11) is row 0 of the
        // rightmost column. That column is index COLS-1 = 12, so cell
        // index = 12 * 7 + 0 = 84.
        let totals = HashMap::new();
        let cells = build_grid(iso("2026-05-11"), 1, &totals, 21);
        assert!(cells[84].is_today, "today must be at expected slot");
        assert_eq!(cells[84].date_iso, "2026-05-11");
    }

    #[test]
    fn future_cells_are_marked_future_and_have_zero_level() {
        // Today = Mon. Locale Monday-start. Today is at index 84;
        // indices 85..=90 are Tue..Sun of the same week — future.
        let totals = HashMap::new();
        let cells = build_grid(iso("2026-05-11"), 1, &totals, 21);
        for cell in &cells[85..=90] {
            assert!(cell.is_future, "{} should be future", cell.date_iso);
            assert_eq!(cell.level, 0);
            assert_eq!(cell.mins, 0);
        }
    }

    #[test]
    fn past_cell_with_totals_buckets_via_minutes_to_level() {
        let mut totals = HashMap::new();
        // 30 min meditation on 2026-02-16 (the oldest cell).
        totals.insert(iso("2026-02-16"), 30 * 60);
        let cells = build_grid(iso("2026-05-11"), 1, &totals, 21);
        assert_eq!(cells[0].mins, 30);
        // 30 min against a 21-min daily expected is ~143% — level 4
        // per minutes_to_level (>= 120% of goal).
        assert_eq!(cells[0].level, 4);
        assert!(cells[0].is_goal_exceeded());
    }

    #[test]
    fn past_cell_no_sessions_lands_at_level_zero() {
        let totals = HashMap::new();
        let cells = build_grid(iso("2026-05-11"), 1, &totals, 21);
        // First past cell (2026-02-16) — no totals entry.
        assert_eq!(cells[0].mins, 0);
        assert_eq!(cells[0].level, 0);
        assert!(!cells[0].is_future);
        assert!(!cells[0].is_today);
    }

    #[test]
    fn sunday_start_locale_shifts_row_zero() {
        // 2026-05-11 Monday. Sunday-start locale (7) → days_since_week
        // _start = (1 - 7 + 7) % 7 = 1; week_start = 2026-05-10 (Sun).
        // Today (Mon) is row 1 of the current week (index COLS-1 * 7 + 1 = 85).
        let totals = HashMap::new();
        let cells = build_grid(iso("2026-05-11"), 7, &totals, 21);
        assert!(cells[85].is_today);
        assert_eq!(cells[84].date_iso, "2026-05-10"); // Sunday at row 0
    }
}
