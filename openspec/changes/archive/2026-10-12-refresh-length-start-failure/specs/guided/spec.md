## MODIFIED Requirements

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
