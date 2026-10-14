## ADDED Requirements

### Requirement: A daily goal that fails to save says so
A failed save of the daily goal on the laptop SHALL show "Save failed".

#### Scenario: Goal changed during a sync
- **WHEN** the user changes the daily goal and the write fails
- **THEN** "Save failed" shows
- **TEST** `meditate-gtk/src/db/mod.rs::no_write_result_is_dropped`
