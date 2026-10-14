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
