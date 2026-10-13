## ADDED Requirements

### Requirement: Esc pauses only a session that can pause
On Linux, Esc on the running page SHALL pause the session only during preparation or the running countdown or stopwatch, and SHALL do nothing in overtime, while paused or after the session ended.

#### Scenario: Esc in overtime
- **WHEN** a countdown has passed its end and the user presses Esc
- **THEN** nothing changes: the overtime keeps counting, Add and Finish still work, and Finish saves the overtime
- **TEST** `meditate-gtk/src/timer/mod.rs::escape_pauses_only_where_the_pause_button_would`

#### Scenario: Esc while paused
- **WHEN** a paused session's page is showing and the user presses Esc
- **THEN** the session stays paused and the button still reads "Resume"
- **TEST** `meditate-gtk/src/timer/mod.rs::escape_pauses_only_where_the_pause_button_would`

#### Scenario: Esc during a running countdown
- **WHEN** a countdown is running and the user presses Esc
- **THEN** the session pauses and the button reads "Resume", as before
- **MANUAL** press Esc during a running Timer session on the laptop; a keyboard event on a live window can't run in the test suite
