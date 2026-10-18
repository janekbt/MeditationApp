## ADDED Requirements
### Requirement: A running session survives a display change
On the phone, a change of font size, bold text, screen size or keyboard during a session SHALL NOT end the session, and an app start SHALL stop any notification, track or bell left over from a session that is gone.

#### Scenario: Changing the font size during a session
- **WHEN** the user changes the system font size while a session runs
- **THEN** the session keeps running with its time
- **TEST** `meditate-android/src/app.rs::configuration_changes_keep_the_session`

#### Scenario: The screen restarts with a session's service still running
- **WHEN** the app's screen starts while a foreground service, guided track or bell from an earlier screen is still running, as after a language change mid-session
- **THEN** they are stopped, and no notification stays
- **TEST** `meditate-android/src/app.rs::startup_stops_what_an_old_screen_left_running`
