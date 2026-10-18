## ADDED Requirements

### Requirement: Minutes on Stats round to the nearest minute, like the Log
Every Stats figure that turns seconds into minutes SHALL round to the nearest minute, as the Log does.

#### Scenario: A 19:30 session against a 20 min goal
- **WHEN** today has one session of 19 min 30 s and the daily goal is 20 min
- **THEN** the goal ring reads 20 min and shows the goal as reached
- **TEST** `meditate-core/src/goal.rs::a_session_of_19_min_30_reaches_a_20_min_goal`

#### Scenario: A 40 second day in the heatmap
- **WHEN** a past day holds only a 40 s session and the goal is 20 min
- **THEN** its heatmap cell is coloured at the lowest level, not left empty
- **TEST** `meditate-core/src/contrib.rs::a_40_second_day_colours_its_heatmap_cell`

#### Scenario: Short totals in tiles, rows and on the chart axis
- **WHEN** a total of 40 s, 29 s or 59 min 30 s is shown
- **THEN** 40 s reads 1m, 29 s reads as the dash in a tile and as 0m in a row or on the axis, and 59 min 30 s reads 1h
- **TEST** `meditate-core/src/format.rs::stats_totals_round_to_the_nearest_minute`

### Requirement: The week card compares like with like
The week card SHALL compare the same days of both weeks and count today only once today has practice.

#### Scenario: The first morning of the week, nothing yet today
- **WHEN** it is the first day of the week, today has no session, and the same day last week had 30 min
- **THEN** no week card shows
- **TEST** `meditate-core/src/insights.rs::the_week_card_waits_on_the_first_morning_of_the_week`

#### Scenario: Midweek, nothing yet today
- **WHEN** it is the third day of the week and today has no session
- **THEN** the card compares the first two days of this week with the first two of last week
- **TEST** `meditate-core/src/date_math.rs::week_over_week_leaves_out_today_until_it_has_practice`

#### Scenario: Midweek after a session today
- **WHEN** it is the third day of the week and today has a session
- **THEN** the card compares the first three days of both weeks, today included
- **TEST** `meditate-core/src/date_math.rs::week_over_week_counts_today_once_it_has_practice`

### Requirement: The year chart shows twelve calendar months
The year chart SHALL show one bar per calendar month, from the same month last year plus one through the current month, in both apps.

#### Scenario: Seen on 2026-10-09
- **WHEN** the year chart is shown on 2026-10-09
- **THEN** it has 12 bars, November 2025 to October 2026, each the sum of its month, empty months as zero
- **TEST** `meditate-core/src/date_math.rs::the_year_chart_has_twelve_calendar_months`

#### Scenario: The shorter periods are unchanged
- **WHEN** the week chart is shown
- **THEN** it has 7 daily bars ending today, empty days as zero
- **TEST** `meditate-core/src/date_math.rs::the_week_chart_has_seven_days_ending_today`

#### Scenario: Seen on both devices
- **WHEN** the maintainer opens the year chart in both apps
- **THEN** both show 12 bars with one letter per month and no repeated month
- **MANUAL** the bars and their labels are drawn by each app

### Requirement: The best streak counts no future days
The best streak SHALL count only days up to today.

#### Scenario: A session dated tomorrow after a 5-day run
- **WHEN** the last 5 days through today each have a session and one more is dated tomorrow
- **THEN** the best streak is 5
- **TEST** `meditate-core/src/db/sessions.rs::best_streak_ignores_days_after_today`

### Requirement: The best streak tile says it is the best streak
The Stats tile with the best streak SHALL be captioned "Best streak", in full in every language at phone width.

#### Scenario: The tile in every language
- **WHEN** the maintainer opens Stats on a 360 px wide phone in each of the ten languages, and on the laptop
- **THEN** the tile reads "Best streak" or its translation in full, on one or two lines
- **MANUAL** a layout check by eye; the caption wraps instead of clipping
