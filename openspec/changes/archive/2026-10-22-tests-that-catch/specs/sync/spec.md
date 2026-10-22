## ADDED Requirements

### Requirement: "Push My Data" after a wiped server restores every session
After the user answers the wiped-server question with "Push My Data", the next sync SHALL upload every session, custom sound and guided file again, also those uploaded before.

#### Scenario: Sessions synced before the server was wiped
- **WHEN** a device synced a session, a custom sound and a guided file, the server folder is emptied and the user picks "Push My Data"
- **THEN** the next sync succeeds and a new device gets the session, the sound and the guided file
- **TEST** `meditate-core/src/sync/orchestrator.rs::push_local_recovery_reuploads_every_old_session`

#### Scenario: Every event is waiting for upload again
- **WHEN** all events were uploaded and the recovery runs
- **THEN** every event is waiting for upload
- **TEST** `meditate-core/src/sync/settings.rs::prepare_push_local_recovery_flags_all_events_unsynced`

### Requirement: An audio file over the size limit on the server is not saved
A pulled custom bell sound bigger than 10 MB SHALL NOT be saved on the device, and SHALL be tried again on a later sync.

#### Scenario: A sound 1 byte over the limit
- **WHEN** the server holds a custom sound of 10 MB + 1 byte, put there outside the app
- **THEN** the peer gets the sound's row but no file, and once the server holds a smaller file, the next sync saves it
- **TEST** `meditate-core/src/sync/orchestrator.rs::pull_drops_a_sound_just_over_the_cap`

#### Scenario: A sound too large to download at all
- **WHEN** the server holds a custom sound of 12 MB
- **THEN** the download is cut off at the limit, no file is saved and the sync still succeeds
- **TEST** `meditate-core/src/sync/orchestrator.rs::pull_skips_a_sound_too_large_to_download`

### Requirement: Sync works against a server that refuses to create a folder twice
Sync SHALL succeed against a server that refuses to create an existing folder, refuses a file in a missing folder and reports a missing folder as not found, as Nextcloud does.

#### Scenario: Two syncs, each with a new custom sound and guided file
- **WHEN** a device syncs twice against such a server, adding a custom sound and a guided file before each sync
- **THEN** both syncs succeed and a new device pulls all four files
- **TEST** `meditate-core/src/sync/orchestrator.rs::two_syncs_with_audio_files_work_against_a_strict_server`
