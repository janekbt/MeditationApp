## ADDED Requirements
### Requirement: The session screen uses the dark colours in both themes
The black session screen SHALL draw its buttons and the Box Breath square in the dark scheme's colours, whatever the phone's light or dark mode.

#### Scenario: Discard during a session in light mode
- **WHEN** the phone is in light mode and a Timer session runs
- **THEN** Pause is a light accent fill with dark text, and Discard has light accent text and a visible outline on the black screen
- **MANUAL** on the FP5: colours on the phone screen need a person to judge.

#### Scenario: The rule cannot quietly come back
- **WHEN** someone gives the session screen or its buttons a colour from the current theme instead of the dark scheme
- **THEN** the build's tests fail
- **TEST** `meditate-android/src/app.rs::the_session_screen_takes_the_dark_roles`

#### Scenario: Box Breath in light mode
- **WHEN** the phone is in light mode and a Box Breath session runs
- **THEN** the square, its progress and the dot ring are in the light accent of the dark scheme, as in dark mode
- **MANUAL** on the FP5: colours on the phone screen need a person to judge.
