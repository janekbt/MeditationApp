## ADDED Requirements

### Requirement: Android's Log groups sessions by the day they started
The Android Log SHALL put each session under the day it started, at its start time in the phone's clock format, with the day's count and total, and its label, colour and guided file on the card.

#### Scenario: Two sessions on one day and one on the day before
- **WHEN** the Log loads sessions of 10:20 and 20:00 on one day and one of 5:00 the day before
- **THEN** it shows two days, the first with 2 sessions and 30 minutes (what its cards show), the second with 1 session and 5 minutes
- **TEST** `meditate-android/src/app.rs::log_groups_sessions_by_start_day`

#### Scenario: A card's time in 24-hour and 12-hour clocks
- **WHEN** sessions started at 21:05 and 09:05
- **THEN** their cards read "21:05" and "09:05" on a 24-hour phone, "9:05 PM" and "9:05 AM" on a 12-hour one
- **TEST** `meditate-android/src/app.rs::log_card_time_follows_the_clock_format`

#### Scenario: A session waiting for Undo
- **WHEN** one of a day's two sessions is deleted and Undo is still offered
- **THEN** the day shows 1 session and only the other one's minutes
- **TEST** `meditate-android/src/app.rs::log_hides_a_session_waiting_for_undo`

#### Scenario: A card with a label and a guided file
- **WHEN** a session has a label and played a guided file
- **THEN** its card shows the label's name and colour place and the file's name, and a session whose label was deleted shows no label and no colour
- **TEST** `meditate-android/src/app.rs::log_card_shows_label_colour_and_guided_name`
