## ADDED Requirements

### Requirement: The Android date picker offers every year back to 2000
The Android edit dialog's year list SHALL offer every year from the current year back to 2000, newest first, and no future year, and picking one SHALL keep the day and month.

#### Scenario: The list in 2026
- **WHEN** the year list is built in 2026
- **THEN** it holds 27 years, 2026 first and 2000 last, with no gap
- **TEST** `meditate-android/src/app.rs::the_year_list_runs_from_this_year_back_to_2000`

#### Scenario: A session from 2022 is moved to another year
- **WHEN** the user edits a 2022 session on the phone and taps the year in the date picker
- **THEN** 2022 is marked as selected, this year as today, and picking 2019 saves the session in 2019
- **MANUAL** on the FP5: Slint layout and touch can't run on the host.

#### Scenario: Picking another year keeps the day and month
- **WHEN** the user picks 2019 in the year list for a session on 15 June 2022, or 2023 for one on 29 February 2024
- **THEN** the picker shows 15 June 2019, or 28 February 2023
- **TEST** `meditate-android/src/app.rs::picking_a_year_keeps_the_day_and_month_and_every_year_is_reachable`
- **TEST** `meditate-android/src/app.rs::a_month_has_its_calendar_length`
