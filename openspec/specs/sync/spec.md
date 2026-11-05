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

### Requirement: A server that goes silent mid-request cannot hold a sync
When the server stops sending in the middle of an answer, the request SHALL end with a network error once one read has waited the read limit, on a fresh connection and on one reused from an earlier request of the same sync.

#### Scenario: The server sends the headers and then nothing
- **WHEN** a request's answer headers arrive and the body never does
- **THEN** the request ends with a network error after the read limit
- **TEST** `meditate-core/src/sync/webdav.rs::a_server_silent_after_the_headers_ends_the_request`

#### Scenario: The server goes silent on a reused connection
- **WHEN** a first request is answered in full and the second one on the same connection gets only its headers
- **THEN** the second request ends with a network error after the read limit
- **TEST** `meditate-core/src/sync/webdav.rs::a_server_silent_on_a_reused_connection_ends_the_request`

### Requirement: Server text stays out of the sync status
When the server answers with an error status, a redirect or a listing that is not WebDAV XML, the sync status SHALL name only the status, that it redirected or the parser's complaint, with at most a short cleaned excerpt of the page in the log.

#### Scenario: The server answers 500 with an error page
- **WHEN** a request gets a 500 whose page names the server's host
- **THEN** the sync error reads "server returned 500" without the page or the host
- **TEST** `meditate-core/src/sync/webdav.rs::a_server_error_shows_only_its_status`

#### Scenario: The server redirects elsewhere
- **WHEN** a request gets a redirect to an address with a user name and host
- **THEN** the sync error says the server redirected, without the address
- **TEST** `meditate-core/src/sync/webdav.rs::a_redirect_error_hides_its_target`

#### Scenario: The folder listing comes back as a web page
- **WHEN** the server answers the folder listing with success but a page instead of WebDAV XML (a proxy's login page naming the host)
- **THEN** the sync error names the parser's complaint only, without the page or the host
- **TEST** `meditate-core/src/sync/webdav.rs::a_listing_that_is_a_web_page_shows_no_page_text`

#### Scenario: The folder listing is a well-formed XHTML page
- **WHEN** the listing comes back as a page that is valid XML but not a WebDAV multistatus
- **THEN** the sync fails with a malformed response instead of reading an empty folder
- **TEST** `meditate-core/src/sync/webdav.rs::a_listing_that_is_an_xhtml_page_is_malformed`

#### Scenario: The error page is long or holds control characters
- **WHEN** the error page is longer than 512 characters or holds line breaks and escape characters
- **THEN** the logged excerpt is at most 512 characters with no control characters
- **TEST** `meditate-core/src/sync/webdav.rs::the_logged_error_excerpt_is_short_and_clean`

### Requirement: A leftover upload is not reported as unreadable
When the events folder on the server holds a `.tmp` file left by an interrupted upload, a pull SHALL skip it without reporting it, and still report any other file it cannot read.

#### Scenario: A leftover upload next to a stray file
- **WHEN** the events folder lists a batch, a stray file and a leftover `.json.tmp`
- **THEN** only the stray file is reported as unreadable
- **TEST** `meditate-core/src/sync/orchestrator.rs::a_leftover_upload_is_not_reported_as_unreadable`

### Requirement: Tapping the sync symbol shows the sync status
Clicking or tapping the sync symbol SHALL show the current sync state, the reason or the files concerned and the next step as buttons, on Linux and Android, except after a wiped server, where it opens the recovery dialog as before.

#### Scenario: Tapping after a good sync
- **WHEN** the last sync succeeded and the user taps the sync symbol
- **THEN** the status says when it last synced and offers Preferences and Sync Now
- **TEST** `meditate-core/src/sync/indicator.rs::a_good_sync_offers_preferences_and_sync_now`

#### Scenario: Tapping while a sync runs
- **WHEN** a sync is running and the user taps the sync symbol
- **THEN** the status says it is syncing and offers only Preferences
- **TEST** `meditate-core/src/sync/indicator.rs::a_running_sync_offers_no_second_sync`

#### Scenario: Tapping after files were not uploaded
- **WHEN** the server refused an audio file and the user taps the sync symbol
- **THEN** the status names each file with its reason and offers Preferences and Sync Now
- **TEST** `meditate-core/src/sync/indicator.rs::refused_uploads_show_by_name_until_retried`

#### Scenario: Tapping without a saved password
- **WHEN** no password is saved and the user taps the sync symbol
- **THEN** the status says so and offers only Enter Password, which opens the sync settings
- **TEST** `meditate-core/src/sync/indicator.rs::a_missing_password_offers_only_enter_password`

#### Scenario: Tapping after the server was wiped
- **WHEN** the last sync found the server wiped and the user taps the sync symbol
- **THEN** the recovery dialog opens instead of the status
- **TEST** `meditate-core/src/sync/indicator.rs::action_for_data_lost_error_opens_recovery`

#### Scenario: Tapping on a phone and on Linux
- **WHEN** the user taps the symbol in each state on the phone, and clicks it on Linux
- **THEN** a dialog (phone) or a popover (Linux) shows the whole text without cutting it off, in German and in Russian at phone width, and its buttons do what they say
- **MANUAL** the maintainer checks it on the Fairphone and the laptop; layout and taps can't run on the host

### Requirement: A failed sync names its reason in the user's language
After a failed sync the status SHALL name a short reason in the app's language (no connection, login refused, a server error with its status where it has one, a redirect, an unreadable reply, a local database error, an unreadable password) and SHALL show no reason line for an error stored before reasons existed.

#### Scenario: The connection fails
- **WHEN** a sync fails because the server can't be reached
- **THEN** the status shows the failure with the network-error reason, and the last success time stays
- **TEST** `meditate-core/src/sync/runner.rs::a_network_failure_records_its_reason`

#### Scenario: The server answers with an error status
- **WHEN** a sync fails with status 503, 404 or 429, a write conflict, a redirect, an unreadable reply, a 401, a local database error or an unreadable keyring
- **THEN** the status shows the matching reason, with the status number for a server error that has one, and a redirect as a redirect
- **TEST** `meditate-core/src/sync/runner.rs::each_failure_records_its_reason`

#### Scenario: An error stored by the previous release
- **WHEN** the database holds a failed sync recorded by 26.11.0, which stored no reason
- **THEN** the status shows the failure with no reason line, and the next failure stores its reason
- **TEST** `meditate-core/src/sync/indicator.rs::an_error_from_before_reasons_shows_no_reason`
