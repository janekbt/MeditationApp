## MODIFIED Requirements

### Requirement: A merge leaves nothing on the merged-away label
Merging a label into another SHALL move its sessions, every mode's default label and any open selection to the kept label.

#### Scenario: A mode's default label is merged away
- **WHEN** the Timer's default label is merged into another
- **THEN** the Timer's default becomes the kept label, and new sessions save with it
- **TEST** `meditate-core/src/db/labels.rs::a_merge_moves_a_modes_default_label_to_the_kept_one`

#### Scenario: The phone merges while the session editor has it selected
- **WHEN** the phone merges a label that Setup, the session editor or a Log filter is using
- **THEN** they switch to the kept label, and Save works
- **TEST** `meditate-android/src/app.rs::a_merge_cleans_up_like_a_delete`

### Requirement: A deleted label never blocks saving a session
Saving a session whose selected label was deleted meanwhile SHALL save it without a label.

#### Scenario: The label is deleted while a session runs or is edited
- **WHEN** a session's label is deleted, here or by a sync, while that session runs with it as the Setup label or the Log editor has it selected, and the user saves
- **THEN** the session is saved without a label, with no error
- **TEST** `meditate-core/src/db/sessions.rs::a_session_on_a_deleted_label_saves_without_it`

#### Scenario: The desktop shows the deleted label as gone
- **WHEN** the user comes back from the chooser after deleting the selected label
- **THEN** the label row shows "(none — pick one)" instead of the old name
- **TEST** `meditate-gtk/src/labels.rs::label_rows_reread_their_label_when_shown_again`
