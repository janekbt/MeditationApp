## MODIFIED Requirements

### Requirement: A guided file write that fails says so and keeps the audio
A failed guided file write SHALL show "Save failed" or "Delete failed" and leave the file and its audio in place.

#### Scenario: Delete fails during a sync (laptop)
- **WHEN** the user confirms deleting a guided file and the write fails
- **THEN** "Delete failed" shows with no Undo, the file stays listed and still plays
- **TEST** `meditate-gtk/src/guided.rs::a_failed_delete_keeps_the_audio_file`

#### Scenario: Star or Undo fails (laptop)
- **WHEN** starring a file or undoing its delete fails
- **THEN** "Save failed" shows
- **TEST** `meditate-gtk/src/db/mod.rs::no_write_result_is_dropped`

#### Scenario: Delete fails on the phone
- **WHEN** a guided delete fails
- **THEN** the message shows and the dialog opens normally next time
- **TEST** `meditate-android/src/app.rs::failed_dialog_writes_say_so_and_close`

#### Scenario: Undo of a delete fails (laptop)
- **WHEN** the user deletes a guided file, taps Undo, and the restore fails
- **THEN** "Save failed" shows with Undo again, and the audio stays until that offer ends
- **TEST** `meditate-gtk/src/guided.rs::a_failed_delete_keeps_the_audio_file`

#### Scenario: Undo of a delete fails (phone)
- **WHEN** the user deletes a guided file, taps Undo, and the restore fails
- **THEN** "Save failed" shows with Undo again, and the audio stays until that offer ends
- **TEST** `meditate-android/src/app.rs::a_failed_guided_undo_is_offered_again`
