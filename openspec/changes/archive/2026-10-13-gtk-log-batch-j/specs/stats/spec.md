## Purpose
Stats sums up the user's practice: today's goal progress, the streak,
the heatmap and the charts, counted by calendar day in local time.

## ADDED Requirements

### Requirement: Stats follows the calendar day while the app stays open
Stats SHALL show the new day as today within a few seconds of midnight, or of waking the device after midnight, without the user leaving the page.

#### Scenario: The day changes while the app stays open
- **WHEN** the app was opened yesterday and the clock passes midnight
- **THEN** the shared day watch reports a new day once, and not again on the same day
- **TEST** `meditate-core/src/time.rs::a_new_day_is_reported_once`

#### Scenario: The laptop wakes up the next morning on the Stats page
- **WHEN** the laptop was suspended overnight with Stats on screen and wakes up
- **THEN** the window poll asks the day watch and reloads Stats and the Log on a new day
- **TEST** `meditate-gtk/src/window/imp.rs::the_window_poll_reloads_both_pages_on_a_new_day`

#### Scenario: The phone comes back to the app the next morning
- **WHEN** the app stayed in memory overnight on Stats and is opened again
- **THEN** the tick loop asks the day watch and reloads Stats and the Log on a new day
- **TEST** `meditate-android/src/app.rs::the_tick_loop_reloads_both_pages_on_a_new_day`

#### Scenario: Seen on both devices
- **WHEN** the maintainer leaves both apps open on Stats over midnight (or sets the clock forward)
- **THEN** "today" and the heatmap's today move to the new day within a few seconds
- **MANUAL** needs a real clock change and a screen
