# Log

## Purpose
The Log lists every session as a card, grouped by date, with its
length, label, note and, for guided sessions, the file it played.

## Requirements

### Requirement: A guided session's card names the file it played
The Log SHALL show the name of the file a guided session played, whether it came from the library, the local folder or Open File, on every device the session syncs to.

#### Scenario: A local folder session shows its file name on another device
- **WHEN** a session played from a local folder file "Body Scan" syncs to a second device
- **THEN** the second device's Log card names "Body Scan"
- **TEST** `meditate-core/src/db/sessions.rs::a_guided_session_carries_its_file_name_to_a_peer`

#### Scenario: A library file renamed later shows its new name
- **WHEN** a session played a library file that was renamed afterwards
- **THEN** the Log card shows the new name; a file not in the library shows the stored name
- **TEST** `meditate-core/src/db/guided_files.rs::the_log_name_prefers_the_library_then_the_stored_name`

#### Scenario: An older app edits the note of such a session
- **WHEN** a device on the last release changes the note and syncs
- **THEN** the card still names the file
- **TEST** `meditate-core/src/db/sessions.rs::an_edit_without_a_file_name_keeps_the_stored_one`

#### Scenario: A name that arrived before this device updated shows after the update
- **WHEN** a device still on the last release received a session with a file name, then updates
- **THEN** its Log names the file
- **TEST** `meditate-core/src/db/sessions.rs::a_name_received_before_the_update_shows_after_it`

#### Scenario: A session recovered after a crash keeps its file name
- **WHEN** the app was killed during a local file session and starts again
- **THEN** the recovered session names the file
- **TEST** `meditate-core/src/db/session_in_progress.rs::a_recovered_session_keeps_its_file_name`

#### Scenario: A restored backup keeps the file names
- **WHEN** a backup is exported and imported into an empty database
- **THEN** each guided session keeps its file name
- **TEST** `meditate-core/src/data_io.rs::a_restored_backup_keeps_guided_file_names`

#### Scenario: A database from the last release opens
- **WHEN** the app opens a database written by 26.10.1
- **THEN** it opens, its sessions are there and new sessions store a file name
- **TEST** `meditate-core/src/db/sessions.rs::a_database_from_the_last_release_gains_the_file_name_columns`

#### Scenario: The card shows the name on both apps
- **WHEN** the maintainer plays a local file on the phone and an Open File pick on the laptop, then syncs
- **THEN** both Logs on both devices name both files
- **MANUAL** card layout and sync between two real devices
