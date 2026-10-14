## Purpose
Backups export every session to a CSV file the user picks and import
such files again, so practice history survives a new device, a wipe
or another app.

## ADDED Requirements

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
