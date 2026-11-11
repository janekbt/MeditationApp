## MODIFIED Requirements

### Requirement: A bell or sound write that fails says so
A failed write of an interval bell or a bell sound SHALL show "Save failed" or "Delete failed" and leave any dialog usable.

#### Scenario: Toggling, editing or deleting an interval bell fails (laptop)
- **WHEN** an interval bell's switch, edit or delete fails to save
- **THEN** "Save failed" or "Delete failed" shows
- **TEST** `meditate-gtk/src/db/mod.rs::no_write_result_is_dropped`

#### Scenario: Deleting a sound fails (both apps)
- **WHEN** a sound delete fails
- **THEN** the message shows, the sound keeps its name and file, and on the phone the dialog closes and opens again normally
- **TEST** `meditate-android/src/app.rs::failed_dialog_writes_say_so_and_close`
