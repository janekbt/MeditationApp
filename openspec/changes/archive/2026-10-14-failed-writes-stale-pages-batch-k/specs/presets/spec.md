## Purpose
Presets store a mode's whole setup under a name, so the user can apply
it with one tap; they can be starred, renamed, overridden, deleted and
restored with Undo, and they sync between devices.

## ADDED Requirements

### Requirement: A preset write that fails says so
A failed preset write SHALL show "Save failed" or "Delete failed" in place of any success message or Undo.

#### Scenario: Override while a sync holds the database (laptop)
- **WHEN** the user overrides a preset and the write fails
- **THEN** "Save failed" shows, no "overridden" message, and the preset keeps its old setup
- **TEST** `meditate-gtk/src/db/mod.rs::no_write_result_is_dropped`

#### Scenario: Delete fails on the phone
- **WHEN** the user deletes a preset and the write fails
- **THEN** "Delete failed" shows without Undo, the dialog closes and the preset stays listed
- **TEST** `meditate-android/src/app.rs::failed_dialog_writes_say_so_and_close`

#### Scenario: Override or rename fails on the phone, then the user tries again
- **WHEN** an override or a rename fails and the user opens the dialog again
- **THEN** "Save failed" showed, and the dialog works on the second try
- **TEST** `meditate-android/src/app.rs::failed_dialog_writes_say_so_and_close`
