# Sync (delta)

## ADDED Requirements

### Requirement: A sync refreshes only the screens its changes touch
After a sync, each app SHALL re-read only the screens showing what that sync brought from other devices, and nothing when it brought nothing, also when it failed.

#### Scenario: A delete on Log page 3 while a sync only uploads
- **WHEN** the Log is scrolled to its third page, a session is deleted, and the sync that follows only uploads
- **THEN** the Log keeps its pages and position, and no other screen reloads
- **TEST** `meditate-core/src/sync/orchestrator.rs::a_sync_that_only_uploads_refreshes_nothing`

#### Scenario: Offline, every save starts a sync that cannot reach the server
- **WHEN** the device is offline and a session is deleted on Log page 3
- **THEN** the failed syncs reload no screen
- **TEST** `meditate-core/src/sync/runner.rs::an_attempt_that_fails_before_the_server_refreshes_nothing`

#### Scenario: A session from the other device arrives
- **WHEN** a sync brings a session saved on the other device
- **THEN** the Log and Stats with the streak are re-read, Setup and the guided list are not
- **TEST** `meditate-core/src/sync/orchestrator.rs::a_pulled_session_refreshes_the_log_and_stats`

#### Scenario: A setting from the other device arrives
- **WHEN** a sync brings the changed daily goal, and later another changed setting such as the starting bell
- **THEN** the goal re-reads Setup and Stats, the other setting only Setup; the Log is not re-read
- **TEST** `meditate-core/src/sync/orchestrator.rs::a_pulled_setting_refreshes_setup_and_stats`

#### Scenario: An audio file downloads
- **WHEN** a sync downloads a bell sound's audio, and later a guided file's
- **THEN** the bell sound re-reads Setup, the guided file Setup and the guided list
- **TEST** `meditate-core/src/sync/orchestrator.rs::a_downloaded_audio_file_refreshes_the_guided_list_and_setup`

#### Scenario: The upload fails after the download worked
- **WHEN** a sync brings a session and then fails to upload
- **THEN** the Log and Stats still show the new session
- **TEST** `meditate-core/src/sync/orchestrator.rs::a_failed_upload_still_reports_what_the_download_applied`

#### Scenario: Linux, the page that is not shown
- **WHEN** a sync brings a session while the Linux app shows the Timer page, and the user then opens the Log
- **THEN** the Timer streak updates at once and the Log shows the session when opened
- **MANUAL** on the laptop: save a session on the phone, sync, watch the streak, open the Log (GTK views can't run in host tests)
