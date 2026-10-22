## ADDED Requirements

### Requirement: The longest session names its year when it is from an earlier year
The longest-session insight SHALL show the year of its date when the session is from an earlier calendar year, and no year otherwise.

#### Scenario: A longest session from last year
- **WHEN** the longest session started on 3 March 2025 and today is 22 October 2026, or it started on 3 March 2026
- **THEN** the first date carries its year and the second does not
- **TEST** `meditate-core/src/format.rs::the_longest_session_names_its_year_only_from_an_earlier_year`

#### Scenario: Seen in both apps
- **WHEN** the maintainer's longest session is from an earlier year
- **THEN** the insight on Linux and on the phone shows the date with its year
- **MANUAL** the insight text is rendered by each app

## MODIFIED Requirements

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
- **THEN** both show 12 bars with one letter per month, in Chinese the month's number, and no repeated month
- **MANUAL** the bars and their labels are drawn by each app

