## ADDED Requirements

### Requirement: A bell or sound write that fails says so
A failed write of an interval bell or a bell sound SHALL show "Save failed" or "Delete failed" and leave any dialog usable.

#### Scenario: Toggling, editing or deleting an interval bell fails (laptop)
- **WHEN** an interval bell's switch, edit or delete fails to save
- **THEN** "Save failed" or "Delete failed" shows
- **TEST** `meditate-gtk/src/db/mod.rs::no_write_result_is_dropped`

#### Scenario: Renaming or deleting a sound fails (both apps)
- **WHEN** a sound rename or delete fails
- **THEN** the message shows, the sound keeps its name and file, and on the phone the dialog closes and opens again normally
- **TEST** `meditate-android/src/app.rs::failed_dialog_writes_say_so_and_close`

### Requirement: Bell rows show the current sound and pattern names
After a sound or pattern is renamed or deleted, the Interval Bells list, the bell edit page and Setup's bell rows SHALL show its current name, or "Missing" when it is gone.

#### Scenario: A sound is renamed while the Interval Bells list is open (laptop)
- **WHEN** the user opens the sound chooser from a bell, renames the sound and goes back
- **THEN** the bell edit page and the Interval Bells list show the new name
- **TEST** `meditate-gtk/src/bells.rs::bell_pages_reread_names_when_shown`

#### Scenario: A pattern used by Setup is deleted (laptop)
- **WHEN** the user deletes the end bell's pattern in the chooser and goes back to Setup
- **THEN** Setup's end bell pattern row shows "Missing"
- **TEST** `meditate-gtk/src/timer/imp.rs::setup_rereads_bell_rows_when_shown_again`

#### Scenario: A sound is renamed or deleted on the phone
- **WHEN** the user renames or deletes a sound used by an interval bell
- **THEN** the Interval Bells list shows the new name or "Missing"
- **TEST** `meditate-android/src/app.rs::label_and_sound_renames_reach_widget_and_bell_list`

#### Scenario: A chooser stays open across a sync on the phone
- **WHEN** a bell or pattern chooser is open, the app resumes, and the sync pulls another device's delete
- **THEN** the chooser no longer lists the deleted item
- **TEST** `meditate-android/src/app.rs::a_pull_follows_the_guided_file_and_refills_choosers`

### Requirement: A bell preview stops when its editor closes and never shows a silent Stop
On the phone, a bell preview SHALL play only while its editor is open and a sound actually plays, with "Playback failed" when it cannot.

#### Scenario: Back pressed during a volume preview
- **WHEN** the user drags Volume in the interval-bell editor and presses the phone's Back
- **THEN** the bell stops and the editor closes as with Cancel
- **TEST** `meditate-android/src/app.rs::back_from_the_interval_editor_cancels_it`

#### Scenario: A sound whose file hasn't arrived yet
- **WHEN** the user taps Play on a sound or pattern that cannot play
- **THEN** "Playback failed" shows and the row keeps its Play button
- **TEST** `meditate-android/src/app.rs::a_preview_that_cannot_play_says_so`

#### Scenario: Seen on the phone
- **WHEN** the maintainer drags Volume and presses Back
- **THEN** the sound stops at once
- **MANUAL** sound needs a speaker
