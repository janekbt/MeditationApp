# Labels

## Purpose
Labels name the kind of practice a session was. The user creates,
renames, merges and deletes them in a chooser; they sync between
devices and show on sessions in the Log, in Log filters and in Stats.

## Requirements

### Requirement: Same names settle the same way on every device
When two labels have the same name, every device SHALL keep the name on the label with the smaller id and give the other a "(conflict-<id>)" suffix.

#### Scenario: Two devices create "Walking" before their first sync
- **WHEN** a laptop and a phone each create "Walking" and then both sync
- **THEN** both show the same label as "Walking" and the same other one as "Walking (conflict-...)"
- **TEST** `meditate-core/src/db/labels.rs::every_device_suffixes_the_same_row_of_a_same_name_pair`

#### Scenario: A name freed by a rename in the same pull
- **WHEN** one pull renames "Walking" to "Hiking" and brings a new label "Walking"
- **THEN** the new label is "Walking" without a suffix, whatever order the events arrive in
- **TEST** `meditate-core/src/db/labels.rs::a_name_freed_in_the_same_pull_goes_to_the_new_row`

#### Scenario: A pair suffixed by an older version heals
- **WHEN** a device suffixed the label with the smaller id, as earlier versions did, and then pulls anything
- **THEN** the suffix moves to the label with the larger id
- **TEST** `meditate-core/src/db/labels.rs::a_pair_suffixed_the_old_way_heals_on_the_next_pull`

### Requirement: Both apps offer to merge a duplicate label
After a sync that leaves a "(conflict-...)" label, each app SHALL ask once whether to merge it into the label of the same name or keep both.

#### Scenario: The desktop app asks after a sync
- **WHEN** a sync on the desktop leaves a "(conflict-...)" label and no other dialog is open
- **THEN** it asks "Duplicate Label" with Merge, Keep Both, and closing to decide later
- **TEST** `meditate-gtk/src/labels.rs::a_sync_asks_about_a_duplicate_label_like_the_phone`

### Requirement: A merge leaves nothing on the merged-away label
Merging a label into another SHALL move its sessions, every mode's default label and any open selection to the kept label.

#### Scenario: A mode's default label is merged away
- **WHEN** the Timer's default label is merged into another
- **THEN** the Timer's default becomes the kept label, and new sessions save with it
- **TEST** `meditate-core/src/db/labels.rs::a_merge_moves_a_modes_default_label_to_the_kept_one`

#### Scenario: The phone merges while the Done page has it selected
- **WHEN** the phone merges a label that the Done page, the session editor or a Log filter is using
- **THEN** they switch to the kept label, and Save works
- **TEST** `meditate-android/src/app.rs::a_merge_cleans_up_like_a_delete`

### Requirement: A deleted label never blocks saving a session
Saving a session whose selected label was deleted meanwhile SHALL save it without a label.

#### Scenario: The label is deleted in the chooser while editing
- **WHEN** a session's label is deleted, here or by a sync, while the Done page or the Log editor has it selected, and the user saves
- **THEN** the session is saved without a label, with no error
- **TEST** `meditate-core/src/db/sessions.rs::a_session_on_a_deleted_label_saves_without_it`

#### Scenario: The desktop shows the deleted label as gone
- **WHEN** the user comes back from the chooser after deleting the selected label
- **THEN** the label row shows "(none — pick one)" instead of the old name
- **TEST** `meditate-gtk/src/labels.rs::label_rows_reread_their_label_when_shown_again`

### Requirement: A label renamed elsewhere while deleted here keeps its sessions
When a delete on one device and a later rename on another meet, the label SHALL come back with all its sessions.

#### Scenario: Laptop deletes, phone renames later
- **WHEN** the laptop deletes "Yoga" and the phone, after it, renames "Yoga", and both sync
- **THEN** both devices have the renamed label, and its sessions still carry it
- **TEST** `meditate-core/src/db/labels.rs::a_label_renamed_elsewhere_while_deleted_here_keeps_its_sessions`

### Requirement: Renaming to the same name changes nothing
Confirming a rename without changing the name SHALL write nothing and send nothing to other devices.

#### Scenario: Enter on the unchanged name
- **WHEN** the phone renamed "Yoga" to "Zen" and the laptop, before syncing, confirms its rename dialog on "Yoga" unchanged
- **THEN** after both sync, both show "Zen"
- **TEST** `meditate-core/src/db/labels.rs::renaming_a_label_to_its_own_name_writes_nothing`

### Requirement: Label changes show everywhere at once
Creating, renaming or deleting a label SHALL update the Log, its filters and Stats right away, without waiting for a sync.

#### Scenario: Rename on the desktop without sync set up
- **WHEN** the user renames "Yoga" to "Zen" on the desktop with no sync configured
- **THEN** the Log chips, the Log filter and Stats show "Zen" the next time they are shown
- **TEST** `meditate-gtk/src/labels.rs::name_changes_refresh_log_and_stats`
