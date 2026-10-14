## ADDED Requirements

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
