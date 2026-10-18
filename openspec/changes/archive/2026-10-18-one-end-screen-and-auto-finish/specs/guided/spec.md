## ADDED Requirements

### Requirement: The phone's Guided buttons sit side by side
On the phone, Manage Imports and Local Folder SHALL sit next to each other under the guided file, each showing its whole text on up to two lines in every language.

#### Scenario: Russian at 360 dp
- **WHEN** the phone is set to Russian and Guided is open at 360 dp
- **THEN** "Управление файлами" and "Локальная папка" sit side by side, wrapped, neither cut off, both buttons as tall as the taller one
- **TEST** `meditate-android/src/app.rs::the_guided_buttons_sit_side_by_side`

#### Scenario: How it looks
- **WHEN** Guided is opened on the phone in German and in Russian
- **THEN** both buttons are side by side, tall enough for their text, and tap like the other buttons
- **MANUAL** wrapping with the real font needs the phone

## MODIFIED Requirements

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
- **THEN** the selection clears or shows the new name; during a session, until it is saved or discarded, it stays as it is
- **TEST** `meditate-gtk/src/timer/imp.rs::manage_files_changes_follow_the_selected_file`

#### Scenario: Another device deletes or renames it (phone)
- **WHEN** a sync pulls a delete or rename of the selected file while Setup shows
- **THEN** the selection clears or shows the new name; during a session, until it is saved or discarded, it stays as it is
- **TEST** `meditate-android/src/app.rs::a_pull_follows_the_guided_file_and_refills_choosers`

#### Scenario: The library can't be read for a moment
- **WHEN** a sync holds the database while Setup checks its selected file
- **THEN** the selection stays as it is, in both apps
- **TEST** `meditate-core/src/guided.rs::the_selected_file_follows_renames_and_deletes`

