# Sync

## Purpose
Sync keeps sessions, labels, presets and the other synced items the
same on every device of a user, through their own WebDAV server; each
device writes its changes under its own device id.

## Requirements

### Requirement: A phone transfer leaves the device id behind
Moving to a new phone with Android's phone-to-phone transfer SHALL NOT copy the app's data, so the new phone gets its own device id.

#### Scenario: The old phone keeps syncing after a transfer
- **WHEN** the user transfers their apps to a new phone and keeps using the old one
- **THEN** the new phone starts without Meditate's data and the two phones sync under different device ids
- **TEST** `meditate-android/src/app.rs::a_phone_transfer_takes_no_app_data`

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

### Requirement: The sync settings say which address and password to enter
Both apps SHALL label the address field "WebDAV URL" and say where Nextcloud shows that address and where to create an app password.

#### Scenario: A Linux user opens the Nextcloud settings
- **WHEN** Preferences is opened on Linux in German
- **THEN** the Nextcloud Sync group names the WebDAV address under Files settings and the app password under Personal settings, Security, in German, and the first field reads "WebDAV-URL"
- **MANUAL** Janek on the laptop: the preferences dialog only renders in a running app

#### Scenario: An Android user opens the Nextcloud settings on a narrow phone
- **WHEN** Preferences is opened on the phone in German
- **THEN** the same sentence and label show in German, wrapped in full with nothing cut off and no box characters
- **MANUAL** Janek on the FP5: layout at phone width only shows on the device

#### Scenario: The new texts in every Linux language
- **WHEN** a Linux catalog lacks the label or the sentence
- **THEN** the check names the language and the text
- **TEST** `meditate-gtk/src/i18n.rs::every_language_translates_every_text`

#### Scenario: The new texts in every Android language
- **WHEN** an Android catalog lacks the label or the sentence, or leaves one empty
- **THEN** the check names the language and the text
- **TEST** `meditate-android/src/app.rs::nextcloud_help_is_translated`
