## ADDED Requirements

### Requirement: A guided file write that fails says so and keeps the audio
A failed guided file write SHALL show "Save failed" or "Delete failed" and leave the file and its audio in place.

#### Scenario: Delete fails during a sync (laptop)
- **WHEN** the user confirms deleting a guided file and the write fails
- **THEN** "Delete failed" shows with no Undo, the file stays listed and still plays
- **TEST** `meditate-gtk/src/guided.rs::a_failed_delete_keeps_the_audio_file`

#### Scenario: Star, rename or Undo fails (laptop)
- **WHEN** starring a file or undoing its delete fails
- **THEN** "Save failed" shows
- **TEST** `meditate-gtk/src/db/mod.rs::no_write_result_is_dropped`

#### Scenario: Rename or delete fails on the phone
- **WHEN** a guided rename or delete fails
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

### Requirement: The selected guided file follows renames and deletes
When the selected guided file is renamed or deleted, here or on another device, Setup SHALL show its new name or clear the selection, outside a running or just-ended session.

#### Scenario: The selected file is deleted in Manage Files (laptop)
- **WHEN** "Body Scan" is selected and the user deletes it in Manage Files
- **THEN** Setup shows no file selected, and Start does not fail with a file error
- **TEST** `meditate-gtk/src/timer/imp.rs::manage_files_changes_follow_the_selected_file`

#### Scenario: The selected file is renamed in Manage Files (laptop)
- **WHEN** "Body Scan" is selected and renamed to "Evening Scan"
- **THEN** Setup shows "Evening Scan"
- **TEST** `meditate-gtk/src/timer/imp.rs::manage_files_changes_follow_the_selected_file`

#### Scenario: Another device deletes or renames it (laptop)
- **WHEN** a sync pulls a delete or rename of the selected file while Setup shows
- **THEN** the selection clears or shows the new name; during a session or on Done it stays as it is
- **TEST** `meditate-gtk/src/timer/imp.rs::manage_files_changes_follow_the_selected_file`

#### Scenario: Another device deletes or renames it (phone)
- **WHEN** a sync pulls a delete or rename of the selected file while Setup shows
- **THEN** the selection clears or shows the new name; during a session or on Done it stays as it is
- **TEST** `meditate-android/src/app.rs::a_pull_follows_the_guided_file_and_refills_choosers`

#### Scenario: The library can't be read for a moment
- **WHEN** a sync holds the database while Setup checks its selected file
- **THEN** the selection stays as it is, in both apps
- **TEST** `meditate-core/src/guided.rs::the_selected_file_follows_renames_and_deletes`

### Requirement: Undo of a deleted guided file selects it again
When the selected guided file is deleted and the delete is undone, Setup SHALL show that file selected again.

#### Scenario: Undo on the laptop
- **WHEN** "Body Scan" is selected, the user deletes it in Manage Files and taps Undo
- **THEN** Setup shows "Body Scan" selected
- **TEST** `meditate-gtk/src/timer/imp.rs::undo_of_a_deleted_guided_file_reselects_it`

#### Scenario: Undo on the phone
- **WHEN** "Body Scan" is selected, the user deletes it and taps Undo
- **THEN** Setup shows "Body Scan" selected
- **TEST** `meditate-android/src/app.rs::undo_of_a_deleted_guided_file_reselects_it`

#### Scenario: Something else was picked before Undo
- **WHEN** the selected file is deleted, the user picks another file, then taps Undo
- **THEN** the other file stays selected
- **TEST** `meditate-core/src/guided.rs::undo_of_a_delete_selects_the_file_again`
