## ADDED Requirements

### Requirement: The daily goal can be as small as one minute
Both apps SHALL accept any whole daily goal from 1 to 1440 minutes.

#### Scenario: A one-minute goal is saved and read back
- **WHEN** the daily goal is set to 1 minute
- **THEN** it is allowed, kept after a restart, and reached by a one-minute session
- **TEST** `meditate-core/src/goal.rs::a_one_minute_goal_is_allowed_and_kept`

#### Scenario: Setting one minute on the laptop
- **WHEN** the goal in Preferences is lowered with the minus button on Linux
- **THEN** it counts down by 1 to 1 and stops there; the Stats ring reads against 1 min
- **MANUAL** Janek on the laptop: the spin row only works in a running app

#### Scenario: Setting one minute on the phone
- **WHEN** 1 is typed into the Daily Goal dialog on Android
- **THEN** Set accepts it, the hint reads "Minutes (1–1440)", and 0 is still refused
- **MANUAL** Janek on the FP5: the dialog only runs on the device
