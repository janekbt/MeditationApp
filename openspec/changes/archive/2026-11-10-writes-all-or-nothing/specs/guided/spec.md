## ADDED Requirements

### Requirement: Undo of an auto-finished save takes its play back
Undoing the save of an auto-finished session SHALL remove the play it added to its local file, together with the session.

#### Scenario: Undo after a local file auto-finished
- **WHEN** a session from a local file with 2 plays auto-finishes (3 plays) and the user taps Undo
- **THEN** the session is gone from the Log and the file shows 2 plays
- **TEST** `meditate-core/src/db/local_guided_files.rs::undoing_a_saved_session_takes_its_local_play_back`

#### Scenario: Undo never counts below zero
- **WHEN** the file's count was reset to 0 before the user taps Undo
- **THEN** the file shows 0 plays and the session is gone
- **TEST** `meditate-core/src/db/local_guided_files.rs::undoing_a_save_never_counts_below_zero`

#### Scenario: Undo after the session was already deleted
- **WHEN** the auto-finished session was deleted in the Log before the user taps Undo
- **THEN** the file keeps its count
- **TEST** `meditate-core/src/db/local_guided_files.rs::undo_after_the_session_is_gone_takes_no_play_back`
