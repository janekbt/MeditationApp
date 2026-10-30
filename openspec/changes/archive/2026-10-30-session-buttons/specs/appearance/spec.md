## MODIFIED Requirements
### Requirement: Actions that throw data away are red
Buttons that discard a session or delete data SHALL be drawn in the error colour, on the session screen and in their confirm dialogs.

#### Scenario: Discard on the session screen
- **WHEN** a session runs, in light or dark mode
- **THEN** Discard is a flat pill with red text on a faint red tint, as on the laptop, and Pause stays in the accent colour
- **MANUAL** on the FP5: colours on the phone screen need a person to judge.

#### Scenario: A delete dialog
- **WHEN** the user deletes a label, preset, sound, vibration pattern or bell, all sessions, discards a session with a note, or is asked to wipe local data after a sync problem
- **THEN** the dialog's Delete, Discard or Wipe button is red and Cancel stays in the accent colour
- **MANUAL** on the FP5: colours on the phone screen need a person to judge.

#### Scenario: A new destructive dialog cannot stay accent-coloured
- **WHEN** someone adds a confirm dialog whose button says Delete, Delete All, Discard or Wipe Local Data without marking it destructive, or the recovery dialog's Wipe Local loses its red, or the red is dropped from the buttons
- **THEN** the build's tests fail
- **TEST** `meditate-android/src/app.rs::destructive_buttons_are_red`
