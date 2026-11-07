# Backup and import

## Purpose
Backups export every session to a CSV file the user picks and import
such files again, so practice history survives a new device, a wipe
or another app.

## Requirements

### Requirement: An export never destroys the old backup
Exporting over an existing backup SHALL leave the old file whole until the new one is completely written.

#### Scenario: The export fails halfway
- **WHEN** a backup file exists and an export over it fails after it started writing
- **THEN** the old backup is unchanged and no temporary file is left
- **TEST** `meditate-core/src/data_io.rs::a_failed_export_keeps_the_old_backup`

#### Scenario: The export succeeds
- **WHEN** an export replaces an existing backup
- **THEN** the file holds the new rows and no temporary file is left
- **TEST** `meditate-core/src/data_io.rs::an_export_replaces_the_backup_and_leaves_no_temp_file`

### Requirement: A Meditation Assistant export imports
Both apps SHALL import the sessions of a Meditation Assistant CSV export, with its UTC start shown in local time, its length and its message as the note.

#### Scenario: The export from the issue
- **WHEN** the three-line export attached to issue #16 is read
- **THEN** it gives three sessions of 4, 7 and 5 minutes, starting at 21:11, 20:09 and 18:36 UTC on 2026-10-30, the last with the note "That's a column for notes", none with a label
- **TEST** `meditate-core/src/data_io.rs::a_meditation_assistant_export_reads_its_sessions`

#### Scenario: A line whose length or start can't be read
- **WHEN** an export has a line with the length "abc" and one with the start "yesterday"
- **THEN** both are reported by their line number as unreadable duration and start, and the other lines still import
- **TEST** `meditate-core/src/data_io.rs::a_meditation_assistant_line_that_cant_be_read_is_reported`

#### Scenario: A session under a minute
- **WHEN** an export has a line with the length "00:00"
- **THEN** it imports as a 1-minute session
- **TEST** `meditate-core/src/data_io.rs::a_meditation_assistant_session_under_a_minute_imports_as_one`

#### Scenario: The same export imported twice
- **WHEN** an export is imported and then imported again
- **THEN** the second import adds no session
- **TEST** `meditate-core/src/data_io.rs::a_meditation_assistant_export_imported_twice_adds_nothing`

#### Scenario: The Android pick reaches the right reader
- **WHEN** a file picked from the Meditation Assistant row lands
- **THEN** it is read as a Meditation Assistant export, a Meditate pick as a backup, an Insight Timer pick as an Insight Timer export
- **TEST** `meditate-android/src/app.rs::each_picked_csv_kind_reads_with_its_own_format`

#### Scenario: The row in both apps
- **WHEN** the user taps "Import from Meditation Assistant" in the Migrate group and picks the export
- **THEN** a toast says how many sessions were imported and they show in the Log at their local times
- **AND** on the phone in German the row's label shows in full, wrapped if needed
- **MANUAL** On the FP5 and on Linux with the issue's file: the file dialog, the Log and text at phone width can only be checked by a person.
