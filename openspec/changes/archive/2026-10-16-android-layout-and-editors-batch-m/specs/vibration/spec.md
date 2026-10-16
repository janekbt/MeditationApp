## ADDED Requirements

### Requirement: Editing a pattern keeps what was stored
Opening a vibration pattern in the editor SHALL show its stored points and chart kind unchanged, also on a phone without amplitude control; only points the user drags there round to off or full.

#### Scenario: A Line pattern is opened and saved unchanged on a phone without amplitude control
- **WHEN** "Wave" (Line, 0.3, 0.6, 0.9) is opened in the phone's editor and saved without a change
- **THEN** it is still Line 0.3, 0.6, 0.9, on this phone and after a sync on every other device
- **TEST** `meditate-android/src/app.rs::editing_a_pattern_loads_its_stored_points_and_kind`

#### Scenario: The FP5 keeps a pattern through Edit and Save
- **WHEN** on the FP5 a custom Line pattern with soft points is opened, renamed and saved
- **THEN** reopening it shows the same soft curve and Line, under the new name
- **MANUAL** the FP5 is the phone without amplitude control; ui.rs only runs on Android

### Requirement: Save keeps a typed Duration and Points
Saving in the phone's vibration editor SHALL first apply a Duration or Points value that was typed but not yet confirmed, so the saved pattern is the one on screen.

#### Scenario: Points typed and Save tapped at once
- **WHEN** 12 is typed into Points of a 7-point pattern and Save is tapped without leaving the field
- **THEN** the pattern is saved with 12 points
- **TEST** `meditate-android/src/app.rs::vibration_save_applies_typed_duration_and_points_first`

#### Scenario: A shorter Duration typed and Save tapped at once
- **WHEN** 1.0 s is typed into Duration of a 20-point pattern and Save is tapped without leaving the field
- **THEN** the pattern is saved with no more points than 1.0 s allows
- **TEST** `meditate-android/src/app.rs::vibration_save_applies_typed_duration_and_points_first`

### Requirement: The vibration editor reaches every row
The phone's vibration editor SHALL scroll the part above the chart, so every row is reachable in landscape and split screen.

#### Scenario: The editor in landscape
- **WHEN** the vibration editor is opened with the phone in landscape
- **THEN** every row above the chart can be scrolled to and used
- **MANUAL** layout on a phone

### Requirement: Pattern chooser rows leave room for the name
Each row of the phone's pattern chooser SHALL show its controls as icon buttons in the laptop's order (check, play, edit, delete), so a long name in any language shows in full.

#### Scenario: A long German pattern name at 360 dp
- **WHEN** a custom pattern named "Sanftes Ansteigen am Morgen" is listed in German at 360 dp width
- **THEN** the full name shows, wrapped if needed, next to the play, edit and delete icons
- **MANUAL** layout on a phone
