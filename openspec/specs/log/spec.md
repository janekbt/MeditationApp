# Log

## Purpose
The Log lists every session as a card, grouped by date, with its
length, label, note and, for guided sessions, the file it played.

## Requirements

### Requirement: A guided session's card names the file it played
The Log SHALL show the name of the file a guided session played, whether it came from the library, the local folder or Open File, on every device the session syncs to.

#### Scenario: A local folder session shows its file name on another device
- **WHEN** a session played from a local folder file "Body Scan" syncs to a second device
- **THEN** the second device's Log card names "Body Scan"
- **TEST** `meditate-core/src/db/sessions.rs::a_guided_session_carries_its_file_name_to_a_peer`

#### Scenario: A library file renamed later shows its new name
- **WHEN** a session played a library file that was renamed afterwards
- **THEN** the Log card shows the new name; a file not in the library shows the stored name
- **TEST** `meditate-core/src/db/guided_files.rs::the_log_name_prefers_the_library_then_the_stored_name`

#### Scenario: An older app edits the note of such a session
- **WHEN** a device on the last release changes the note and syncs
- **THEN** the card still names the file
- **TEST** `meditate-core/src/db/sessions.rs::an_edit_without_a_file_name_keeps_the_stored_one`

#### Scenario: A name that arrived before this device updated shows after the update
- **WHEN** a device still on the last release received a session with a file name, then updates
- **THEN** its Log names the file
- **TEST** `meditate-core/src/db/sessions.rs::a_name_received_before_the_update_shows_after_it`

#### Scenario: A session recovered after a crash keeps its file name
- **WHEN** the app was killed during a local file session and starts again
- **THEN** the recovered session names the file
- **TEST** `meditate-core/src/db/session_in_progress.rs::a_recovered_session_keeps_its_file_name`

#### Scenario: A restored backup keeps the file names
- **WHEN** a backup is exported and imported into an empty database
- **THEN** each guided session keeps its file name
- **TEST** `meditate-core/src/data_io.rs::a_restored_backup_keeps_guided_file_names`

#### Scenario: A database from the last release opens
- **WHEN** the app opens a database written by 26.10.1
- **THEN** it opens, its sessions are there and new sessions store a file name
- **TEST** `meditate-core/src/db/sessions.rs::a_database_from_the_last_release_gains_the_file_name_columns`

#### Scenario: The card shows the name on both apps
- **WHEN** the maintainer plays a local file on the phone and an Open File pick on the laptop, then syncs
- **THEN** both Logs on both devices name both files
- **MANUAL** card layout and sync between two real devices

### Requirement: The Log's day headers follow the calendar day
The Log SHALL head the new day's sessions "Today" and yesterday's "Yesterday" after midnight, without a second "Today" section.

#### Scenario: A session saved the morning after the app stayed open
- **WHEN** the Log was built yesterday, the day changes and a session is saved
- **THEN** the Log is rebuilt first, so there is one "Today" and the old one reads "Yesterday"
- **TEST** `meditate-gtk/src/window/imp.rs::the_window_poll_reloads_both_pages_on_a_new_day`

### Requirement: A filtered Log shows only matching sessions
The Log SHALL show only sessions that match its filters, also right after a session is saved.

#### Scenario: A session with another label is saved while the Log is filtered
- **WHEN** the Log is filtered to "Yoga" and a "Zen" session is saved
- **THEN** the Log reloads from the database instead of adding the card, and Load more continues at the next Yoga session
- **TEST** `meditate-gtk/src/log/mod.rs::a_save_under_a_filter_reloads_instead_of_adding`

### Requirement: The label filter shows the label it filters by
The label filter SHALL show the label the Log is filtered by, with its current name, and picking an entry SHALL filter by exactly the label it names.

#### Scenario: Coming back to the Log tab while filtered
- **WHEN** the Log is filtered to "Morning" and the user leaves and re-enters the tab
- **THEN** the filter shows "Morning", and picking "All labels" shows all sessions
- **TEST** `meditate-gtk/src/log/mod.rs::the_filter_shows_and_maps_the_labels_it_was_built_from`

#### Scenario: A label was renamed after the Log loaded
- **WHEN** "Zen" was renamed "Afternoon" and the user opens the filter and picks "Morning"
- **THEN** the list shows "Afternoon" and the Log filters by Morning
- **TEST** `meditate-gtk/src/log/mod.rs::the_filter_shows_and_maps_the_labels_it_was_built_from`

#### Scenario: The filtered label was deleted
- **WHEN** the Log is filtered by a label that is deleted (here or on another device)
- **THEN** the next time the filter is built the filter is cleared and all sessions show, as on Android
- **TEST** `meditate-gtk/src/log/mod.rs::a_deleted_filter_label_clears_the_filter`

### Requirement: Editing a session keeps its start seconds
Saving the edit dialog SHALL keep the start's seconds unless the date, hour or minute is changed; a session added by hand starts at second 0.

#### Scenario: Only the note is changed
- **WHEN** a session that started at 07:12:45 gets a new note and is saved
- **THEN** it still starts at 07:12:45, and no start change syncs
- **TEST** `meditate-gtk/src/log/mod.rs::an_edit_keeps_the_start_seconds`

### Requirement: The day caption counts what the cards show
The caption above a day SHALL equal the sum of the minutes its cards show, also after a delete.

#### Scenario: Deleting a session with odd seconds
- **WHEN** a day holds a 10:20 and a 5:00 session (caption "15m") and the 10:20 one is deleted
- **THEN** the caption reads "5m"
- **TEST** `meditate-gtk/src/log/mod.rs::a_delete_subtracts_what_the_card_added`

### Requirement: A delete waiting for Undo survives a Log rebuild
A session deleted within the Undo time SHALL stay hidden when the Log rebuilds, come back on Undo, and be counted once when the delete goes through.

#### Scenario: A sync or another save rebuilds the Log during Undo
- **WHEN** session A is deleted and the Log rebuilds before the toast times out
- **THEN** A stays hidden, Undo brings it back, and without Undo the caption and Load more drop it once
- **TEST** `meditate-gtk/src/log/mod.rs::a_pending_delete_stays_hidden_and_counts_once`

#### Scenario: Seen on the laptop
- **WHEN** the maintainer deletes a session and saves another edit within 5 s
- **THEN** the deleted card stays away, the toast says "1 session deleted", and the caption is right afterwards
- **MANUAL** timing between a toast and a rebuild on a real screen

### Requirement: A Log delete that fails brings the session back
When deleting a session fails after the Undo time, the Log SHALL show "Delete failed" and keep the session, in the Log and in Stats.

#### Scenario: The delete fails on the laptop
- **WHEN** the user deletes a session, the Undo time runs out and the write fails
- **THEN** "Delete failed" shows and the Log reloads with the session in it
- **TEST** `meditate-gtk/src/log/mod.rs::a_failed_delete_brings_the_card_back`

#### Scenario: The delete fails on the phone
- **WHEN** the user deletes a session, the Undo message goes away and the write fails
- **THEN** the row stays in the Log and "Delete failed" shows after the Undo message is gone
- **TEST** `meditate-android/src/app.rs::a_failed_log_delete_keeps_its_row`

### Requirement: Label colours follow the accent
The Log SHALL colour each label from the current accent: sixteen colours, the first being the accent, the next seven stepping round the colour wheel from it, and the last eight the same hues turned half a step and set apart in brightness; a session without a label stays grey.

#### Scenario: A green accent on Android
- **WHEN** the accent is Green and the phone is in light mode
- **THEN** the first label colour is the green the app uses for its buttons, colours 1 to 8 have hues about 140, 185, 230, 275, 320, 5, 50 and 95 degrees, and colours 9 to 16 the same hues plus 22.5 degrees, darker than colours 1 to 8
- **AND** in dark mode colours 1 to 8 are lighter than in light mode, and colours 9 to 16 lighter than colours 1 to 8
- **TEST** `meditate-android/src/theme.rs::label_colours_start_at_the_accent_and_step_round_the_wheel`

#### Scenario: The Linux stylesheet's accent colours load
- **WHEN** the Linux app loads its stylesheet under libadwaita
- **THEN** the sixteen label colours, defined relative to the system accent, load without an error
- **TEST** `meditate-gtk/src/log/mod.rs::the_label_colours_load_relative_to_the_accent`

#### Scenario: The accent is changed while the Log is open
- **WHEN** the Log shows labelled sessions and the accent is changed (Linux: GNOME Settings, Appearance; Android: Preferences)
- **THEN** the stripes (and on Linux the chips and dots) take the new colours at once, each label keeps its place, and unlabelled sessions stay grey
- **MANUAL** on the laptop and the FP5: colour on screen can't be checked on the host

### Requirement: Labels take colours in the order they were created
Each label SHALL take the next of the sixteen colours in the order labels were created, the same on every device once synced, so the first sixteen labels all differ.

#### Scenario: Three labels, then a fourth
- **WHEN** "Morning", "Walking" and "Evening" are created in that order, then "Body Scan"
- **THEN** they take colours 1, 2, 3 and 4, and creating "Body Scan" changes none of the first three
- **TEST** `meditate-core/src/db/labels.rs::labels_take_colours_in_the_order_they_were_created`

#### Scenario: A label is renamed or deleted
- **WHEN** "Walking" (colour 2 of three) is renamed to "Walk", and later "Morning" (colour 1) is deleted
- **THEN** the rename keeps colour 2; after the delete "Walk" has colour 1 and "Evening" colour 2
- **TEST** `meditate-core/src/db/labels.rs::a_rename_keeps_a_label_colour_and_a_delete_moves_later_ones_up`

#### Scenario: Two devices created labels before they synced
- **WHEN** device A created "Morning" and device B created "Walking", each offline, and both have synced
- **THEN** both devices give each label the same colour
- **TEST** `meditate-core/src/db/labels.rs::synced_devices_agree_on_label_colours`

#### Scenario: A device made a label before its first sync
- **WHEN** a laptop has "Morning" and "Evening" after some sessions, and a new phone that never synced creates "Walking" (or restores a backup), then both sync
- **THEN** both devices agree on every label's colour, and "Walking" may take an early place, moving the laptop's labels on by one, once
- **TEST** `meditate-core/src/db/labels.rs::a_label_made_before_a_device_first_synced_moves_the_others_once`

#### Scenario: The seventeenth label
- **WHEN** seventeen labels exist
- **THEN** the seventeenth has colour 1 again
- **TEST** `meditate-core/src/db/labels.rs::labels_take_colours_in_the_order_they_were_created`

### Requirement: The Android date picker offers every year back to 2000
The Android edit dialog's year list SHALL offer every year from the current year back to 2000, newest first, and no future year, and picking one SHALL keep the day and month.

#### Scenario: The list in 2026
- **WHEN** the year list is built in 2026
- **THEN** it holds 27 years, 2026 first and 2000 last, with no gap
- **TEST** `meditate-android/src/app.rs::the_year_list_runs_from_this_year_back_to_2000`

#### Scenario: A session from 2022 is moved to another year
- **WHEN** the user edits a 2022 session on the phone and taps the year in the date picker
- **THEN** 2022 is marked as selected, this year as today, and picking 2019 saves the session in 2019
- **MANUAL** on the FP5: Slint layout and touch can't run on the host.

#### Scenario: Picking another year keeps the day and month
- **WHEN** the user picks 2019 in the year list for a session on 15 June 2022, or 2023 for one on 29 February 2024
- **THEN** the picker shows 15 June 2019, or 28 February 2023
- **TEST** `meditate-android/src/app.rs::picking_a_year_keeps_the_day_and_month_and_every_year_is_reachable`
- **TEST** `meditate-android/src/app.rs::a_month_has_its_calendar_length`
