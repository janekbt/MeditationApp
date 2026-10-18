# Vibration

## Purpose
Bells and Box Breath cues can vibrate on devices with a vibration
motor (the Librem 5 through feedbackd, Android phones). This area
says when a running vibration is kept, replaced or stopped.

## Requirements

### Requirement: A bell without vibration leaves a running vibration alone
On Linux, a bell that only plays a sound SHALL NOT stop a vibration that is already playing.

#### Scenario: A vibration bell and a sound-only bell at the same moment
- **WHEN** two bells are due at 10:00, one set to Vibration and one to Sound
- **THEN** the sound plays and the vibration plays in full
- **TEST** `meditate-gtk/src/vibration.rs::a_bell_without_vibration_keeps_the_running_one`

#### Scenario: Pausing during a vibration
- **WHEN** a long vibration is playing and the user pauses or stops the session
- **THEN** the vibration stops
- **MANUAL** on the Librem 5, start a 1-minute session whose starting bell has a long pattern, press Pause during it; the motor stops (feedbackd can't run on the host)

### Requirement: A replaced vibration sends nothing more
On Linux, when a new vibration replaces one that is still playing, the old one SHALL send no further parts of its pattern.

#### Scenario: A long pattern replaced by the next bell
- **WHEN** a pattern longer than one part is playing and another bell's vibration starts
- **THEN** only the new pattern plays to its end; no part of the old one interrupts it
- **TEST** `meditate-gtk/src/vibration.rs::a_replaced_pattern_sends_no_more_parts`

#### Scenario: Previewing another pattern
- **WHEN** a long pattern's preview is playing in the Vibrations list or the pattern editor and the user previews another one
- **THEN** only the second preview plays
- **TEST** `meditate-gtk/src/vibration.rs::a_replaced_pattern_sends_no_more_parts`

### Requirement: A pattern write that fails says so
A failed vibration pattern delete, rename Undo or delete Undo SHALL show "Delete failed" or "Save failed" in place of any success message or Undo.

#### Scenario: Delete fails on the laptop
- **WHEN** the user deletes a pattern and the write fails
- **THEN** "Delete failed" shows instead of "Deleted · Undo", and the pattern stays listed
- **TEST** `meditate-gtk/src/db/mod.rs::no_write_result_is_dropped`

#### Scenario: Delete fails on the phone
- **WHEN** the user confirms deleting a pattern and the write fails
- **THEN** "Delete failed" shows and the pattern stays listed
- **TEST** `meditate-android/src/app.rs::failed_dialog_writes_say_so_and_close`

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

### Requirement: Vibration bells get through Battery Saver
On the phone, a vibration bell SHALL be sent as an alarm vibration on every Android version whose Battery Saver lets alarm vibrations through (9 and newer).

#### Scenario: A Vibration end bell on Android 9 to 12 under Battery Saver
- **WHEN** a session with a Vibration end bell ends on Android 9 to 12 with Battery Saver on
- **THEN** the phone vibrates
- **TEST** `meditate-android/src/app.rs::vibrations_are_alarms_on_every_android`
