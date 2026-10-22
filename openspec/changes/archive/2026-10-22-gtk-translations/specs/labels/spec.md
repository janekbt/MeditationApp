## MODIFIED Requirements

### Requirement: A deleted label never blocks saving a session
Saving a session whose selected label was deleted meanwhile SHALL save it without a label.

#### Scenario: The label is deleted while a session runs or is edited
- **WHEN** a session's label is deleted, here or by a sync, while that session runs with it as the Setup label or the Log editor has it selected, and the user saves
- **THEN** the session is saved without a label, with no error
- **TEST** `meditate-core/src/db/sessions.rs::a_session_on_a_deleted_label_saves_without_it`

#### Scenario: The desktop shows the deleted label as gone
- **WHEN** the user comes back from the chooser after deleting the selected label
- **THEN** the label row shows "(none, pick one)" instead of the old name
- **TEST** `meditate-gtk/src/labels.rs::label_rows_reread_their_label_when_shown_again`

