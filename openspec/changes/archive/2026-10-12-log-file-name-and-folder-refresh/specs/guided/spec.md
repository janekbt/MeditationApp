## ADDED Requirements

### Requirement: The local folder refresh shows that it runs and when it is done
The Local Folder refresh button SHALL spin while the folder or new file lengths are being read, and after a tapped refresh SHALL say once that the folder was refreshed.

#### Scenario: A tapped refresh says so once when it ends
- **WHEN** the user taps refresh and the folder and all new lengths have been read
- **THEN** one "Folder refreshed" message shows; none while it runs and none for a refresh the page started itself
- **TEST** `meditate-core/src/local_folder.rs::a_tapped_refresh_reports_once_when_done`

#### Scenario: A tapped refresh of an unreachable folder shows no success
- **WHEN** the user taps refresh and the folder can't be read
- **THEN** the "can't be reached" banner shows and no "Folder refreshed" message
- **TEST** `meditate-core/src/local_folder.rs::an_unreachable_folder_reports_no_refresh`

#### Scenario: A tapped refresh whose result could not be saved shows no success
- **WHEN** the user taps refresh and saving the scan or a length fails
- **THEN** "Save failed" shows and no "Folder refreshed" message
- **TEST** `meditate-core/src/local_folder.rs::a_failed_write_reports_no_refresh`

#### Scenario: The percentage counts only this refresh's files
- **WHEN** a folder of 400 read files gains 4 new ones and 1 of them has been read
- **THEN** the button shows 25%, and no number while the folder is still being walked
- **TEST** `meditate-core/src/local_folder.rs::the_refresh_percentage_counts_this_runs_files`

#### Scenario: Spin and message on both apps
- **WHEN** the maintainer adds files to the folder and taps refresh on the laptop and on the phone
- **THEN** the button spins with a rising percentage until the new files show their lengths, then the message shows
- **MANUAL** animation and timing need a real device

## MODIFIED Requirements

### Requirement: A file's length is read once and again only when it changed
The app SHALL keep each local file's length and read it again only when the file's size or modification date changed.

#### Scenario: An unchanged file keeps its length on the next scan
- **WHEN** a file's length was read and a later scan sees the same size and date
- **THEN** the file keeps its length and is not read again
- **TEST** `meditate-core/src/db/local_guided_files.rs::a_rescan_keeps_the_length_of_an_unchanged_file`

#### Scenario: A replaced file is read again
- **WHEN** a later scan sees a different size or date for the same file
- **THEN** its length is unknown until it is read again
- **TEST** `meditate-core/src/db/local_guided_files.rs::a_changed_file_needs_its_length_read_again`

#### Scenario: The first scan shows how far it got
- **WHEN** 41 files were found, one is gone, and 29 lengths are read
- **THEN** the folder line says 41 files and the refresh button shows the share read (see the refresh requirement)
- **TEST** `meditate-core/src/local_folder.rs::folder_line_counts_files_still_there`

#### Scenario: A file picked before its length is read
- **WHEN** the user picks a file that still says its length is not read yet
- **THEN** that file is read next, the current file says it is reading the length, and Start Session is unavailable until the length is in
- **MANUAL** pick a file during the first scan of a large folder on both apps; widget state and the reading queue run on the device
