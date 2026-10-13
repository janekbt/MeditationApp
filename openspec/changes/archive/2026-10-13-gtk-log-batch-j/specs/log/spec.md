## ADDED Requirements

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
