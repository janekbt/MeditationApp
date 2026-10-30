## ADDED Requirements
### Requirement: System bar symbols stay readable in both themes
The navigation bar and status bar symbols SHALL be dark over a light page and light over a dark or black page, also after the theme changes while the app is open.

#### Scenario: Three-button navigation in the light theme
- **WHEN** the phone is in light mode with three-button navigation and Meditate is opened
- **THEN** the back, home and recent buttons and the status bar clock and battery are dark on the light page
- **MANUAL** on the FP5: Android draws the system bars, so only a phone shows them.

#### Scenario: Three-button navigation in the dark theme
- **WHEN** the phone is in dark mode with three-button navigation and Meditate is opened
- **THEN** the buttons and the status bar symbols are light, as before
- **MANUAL** on the FP5: Android draws the system bars, so only a phone shows them.

#### Scenario: The theme changes while Meditate is open
- **WHEN** Meditate is open and the user switches the phone from dark to light mode in the quick settings, and back
- **THEN** each time the bar symbols switch with the page, without restarting the app
- **MANUAL** on the FP5: Android draws the system bars, so only a phone shows them.

#### Scenario: A session runs in the light theme
- **WHEN** the phone is in light mode and a session is started, then stopped and the session saved
- **THEN** on the black session screen the bar symbols are light, and back on the timer page they are dark again
- **MANUAL** on the FP5: Android draws the system bars, so only a phone shows them.
