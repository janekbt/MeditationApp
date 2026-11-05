## ADDED Requirements

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
