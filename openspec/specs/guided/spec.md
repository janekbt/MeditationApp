# Guided

## Purpose
Guided mode plays the user's own guided-meditation audio: files imported
into the synced library, a one-off Open File pick, or files in a local
folder the user manages outside the app.

## Requirements

### Requirement: A local folder is played in place and stays on the device
The app SHALL play audio files from the chosen local folder where they are, without copying them, and SHALL never send anything about them to the sync server.

#### Scenario: Audio files at every depth are listed, other files are not
- **WHEN** the chosen folder holds `a.mp3`, `notes.txt` and `Sleep/Short/b.ogg`
- **THEN** the folder's files are `a.mp3` and `Sleep/Short/b.ogg`
- **TEST** `meditate-core/src/db/local_guided_files.rs::a_scan_keeps_audio_files_at_every_depth`

#### Scenario: Starring, playing and scanning write nothing to sync
- **WHEN** a folder is scanned, a file starred and a session from it saved
- **THEN** the only new sync event is the session itself
- **TEST** `meditate-core/src/db/local_guided_files.rs::local_folder_changes_write_no_sync_event`

#### Scenario: A local file plays on the phone without a copy
- **WHEN** the user picks a 40-minute file from the Local Folder on Android and starts a session
- **THEN** it plays, and the app's storage does not grow by the file's size
- **MANUAL** play on the FP5 and compare the app's storage in Android settings before and after; content-address playback can't run on the host

### Requirement: Folders view shows one level at a time, at any depth
The Folders view SHALL show the subfolders of the open folder, then its files, each group by name, with each subfolder's own folder and file counts.

#### Scenario: Subfolders come first, then files
- **WHEN** the open folder holds files `b.ogg`, `a.ogg` and folders `Sleep`, `Morning`
- **THEN** the rows are Morning, Sleep, a, b
- **TEST** `meditate-core/src/local_folder.rs::folders_view_lists_subfolders_then_files_by_name`

#### Scenario: A folder row counts only what is directly inside it
- **WHEN** `Sleep` holds two folders and five files, and those folders hold more files
- **THEN** its row says 2 folders, 5 files
- **TEST** `meditate-core/src/local_folder.rs::a_folder_row_counts_its_own_folders_and_files`

#### Scenario: A folder three levels down opens like the top one
- **WHEN** the user opens `Retreats/Autumn 2025/Day 3`
- **THEN** its rows are its own subfolders and files, and the page names "Day 3" under "Retreats / Autumn 2025"
- **TEST** `meditate-core/src/local_folder.rs::folders_view_opens_any_depth`

### Requirement: List view shows every file with its folder, in the chosen order
The List view SHALL show every present file of the folder tree once, with its folder path, sorted by name, length, most played or least played, ties by name.

#### Scenario: Least played puts never-played files first
- **WHEN** files have 0, 3 and 0 plays and the sort is Least played
- **THEN** the two never-played files come first, by name, then the one with 3
- **TEST** `meditate-core/src/local_folder.rs::list_view_sorts_by_each_order_with_ties_by_name`

#### Scenario: Files whose length is not read yet sort last by length
- **WHEN** the sort is Length and one file's length is not read yet
- **THEN** that file is last and says its length is not read yet
- **TEST** `meditate-core/src/local_folder.rs::an_unread_length_sorts_last`

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

### Requirement: A saved session counts one play of its local file
Saving a Guided session played from a local file SHALL add one play to that file on this device, together with the session or not at all.

#### Scenario: Saving the session counts the play
- **WHEN** a session from `Sleep/b.ogg` is saved
- **THEN** that file's play count goes from 2 to 3 and the session is in the Log
- **TEST** `meditate-core/src/db/local_guided_files.rs::saving_a_session_counts_one_play_of_its_local_file`

### Requirement: Local files can be starred and show in Starred Files
A local file SHALL be starrable in both views, and starred local files SHALL appear in Starred Files with the library's starred files, marked as a local file.

#### Scenario: A starred local file joins Starred Files
- **WHEN** one library file and one local file are starred
- **THEN** Starred Files lists both, and the local one is marked Local file with its length
- **TEST** `meditate-core/src/local_folder.rs::starred_files_list_library_and_local_files`

### Requirement: A missing local file is kept and shown as missing
A local file that a scan no longer finds SHALL keep its star and plays, SHALL stay in Starred Files marked as not found when starred, and SHALL get them back when it returns.

#### Scenario: A starred file deleted outside the app is marked not found
- **WHEN** a starred file is gone at the next scan
- **THEN** it leaves both views and stays in Starred Files marked not found
- **TEST** `meditate-core/src/db/local_guided_files.rs::a_starred_file_gone_from_the_folder_stays_marked_missing`

#### Scenario: A file that comes back keeps its star and plays
- **WHEN** a missing file with 4 plays and a star is found again
- **THEN** it shows again with 4 plays and its star
- **TEST** `meditate-core/src/db/local_guided_files.rs::a_file_back_in_the_folder_keeps_its_star_and_plays`

#### Scenario: Tapping a missing file selects nothing and says why
- **WHEN** the user taps a starred file marked not found
- **THEN** the current file stays as it was and a message says the file was not found in the local folder
- **MANUAL** tap the row on both apps; widget behaviour

#### Scenario: An unreachable folder changes nothing
- **WHEN** the folder can't be read (moved, access lost, card removed)
- **THEN** no file is marked missing and the Local Folder page shows Folder not reachable with Choose Again
- **TEST** `meditate-core/src/db/local_guided_files.rs::a_failed_scan_changes_no_file`

### Requirement: Local folder data survives a folder change and Wipe Local
Choosing another folder or wiping local data SHALL keep the stars and plays of local files.

#### Scenario: Going back to an earlier folder brings its stars back
- **WHEN** the user switches from folder A to B and back to A
- **THEN** A's files have their stars and plays
- **TEST** `meditate-core/src/db/local_guided_files.rs::changing_the_folder_keeps_the_old_folders_stars_and_plays`

#### Scenario: Wipe Local keeps the local folder
- **WHEN** the user wipes local data after "remote data lost"
- **THEN** the folder, its stars and its plays are still there
- **TEST** `meditate-core/src/db/events.rs::wipe_local_event_log_keeps_the_local_folder`

#### Scenario: A database from the last release opens
- **WHEN** a database written by 26.10.1 is opened
- **THEN** the local folder is empty and every session and library file is unchanged
- **TEST** `meditate-core/src/db/local_guided_files.rs::a_database_from_the_last_release_gains_the_local_folder_table`

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

#### Scenario: A tapped refresh whose result could not be saved or read shows no success
- **WHEN** the user taps refresh and saving the scan or a length fails, or the length reading can't start
- **THEN** "Save failed" or "Couldn't read audio file" shows and no "Folder refreshed" message
- **TEST** `meditate-core/src/local_folder.rs::a_failed_write_reports_no_refresh`

#### Scenario: A failure while no refresh waits doesn't silence a later one
- **WHEN** a picked file's length can't be read, and later the user taps refresh and it succeeds
- **THEN** "Folder refreshed" shows
- **TEST** `meditate-core/src/local_folder.rs::a_failure_with_no_tapped_refresh_is_forgotten`

#### Scenario: The percentage counts only this refresh's files
- **WHEN** a folder of 400 read files gains 4 new ones and 1 of them has been read
- **THEN** the button shows 25%, and no number while the folder is still being walked
- **TEST** `meditate-core/src/local_folder.rs::the_refresh_percentage_counts_this_runs_files`

#### Scenario: Spin and message on both apps
- **WHEN** the maintainer adds files to the folder and taps refresh on the laptop and on the phone
- **THEN** the button spins with a rising percentage until the new files show their lengths, then the message shows
- **MANUAL** animation and timing need a real device
