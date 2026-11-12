## ADDED Requirements

### Requirement: The bell counter counts the starting and end bell only when they rang
The running page's counter SHALL count the starting and end bell as heard only once they rang, and SHALL leave a bell the mode's signal setting mutes out of the total.

#### Scenario: Both bells muted by the mode's signal setting
- **WHEN** a 30 minute Timer with a fixed bell at 10 minutes has a sound-only starting and end bell, and the Timer's signal setting is vibration only
- **THEN** the counter reads "0 of 1" at the start, "1 of 1" at 12 minutes and "1" after the end
- **TEST** `meditate-core/src/session/mod.rs::a_muted_starting_or_end_bell_is_not_counted`

#### Scenario: A suspend through preparation skips the starting bell
- **WHEN** a 1 minute Timer with 30 s preparation, a starting and end bell and a fixed bell at 30 s is suspended 5 s in and woken 3 minutes later
- **THEN** only the end bell rings, and the counter reads "1": the starting bell never rang
- **TEST** `meditate-core/src/session/mod.rs::a_starting_bell_skipped_by_a_suspend_is_not_counted`
