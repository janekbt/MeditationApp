## ADDED Requirements

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
