## ADDED Requirements

### Requirement: The phone's ended page shows the session as Linux does
After a session ends and until Save or Discard, the phone's running page SHALL show the saved length, the bell ring and the bell count at that length, as Linux does.

#### Scenario: A countdown with interval bells ends
- **WHEN** a 2 minute Timer with a bell every 30 seconds runs into overtime and Save ends it, and the page stays (a failed save keeps the ended session, `a_failed_save_keeps_the_session_and_says_so`)
- **THEN** the ended session shows 02:00, the ring full and the count of bells heard
- **TEST** `meditate-android/src/app.rs::an_ended_session_keeps_its_ring`

### Requirement: Pause on the phone follows core
Every pause or resume on the phone (the Pause and Resume buttons, the headphones' key, a call taking the audio) SHALL do what core's `toggle_action` says: pause while preparing or running, resume while paused, nothing in overtime or after the end.

#### Scenario: Overtime
- **WHEN** a Timer is in overtime and the headphones' key is pressed, or a call takes the audio
- **THEN** nothing changes: the overtime keeps counting, and Save stores the planned length
- **TEST** `meditate-android/src/app.rs::a_pause_in_overtime_does_nothing`

### Requirement: A session keeps what it started with
On the phone, a session SHALL save the mode, guided file, title and Local Folder play fixed at its Start, whatever the Setup page selects later. On both apps, only a Guided session carries a guided file.

#### Scenario: The Guided selection changes before Save
- **WHEN** a Guided session starts with file A, and the Guided selection becomes file B (a sync re-selects) before Save
- **THEN** the saved session names file A and its title, and counts A's Local Folder play
- **TEST** `meditate-android/src/app.rs::a_session_keeps_the_file_it_started_with`

#### Scenario: Only Guided carries a file
- **WHEN** a Timer or Box Breath session starts while a guided file is selected on the Guided tab
- **THEN** its record carries no guided file, on both apps
- **TEST** `meditate-core/src/db/sessions.rs::only_guided_carries_a_guided_file`

### Requirement: Both apps start the same session shape
Both apps SHALL take the session's shape from core: Timer and Box Breath count down from the set length or count up with Stopwatch Mode, a Box Breath countdown ends on a full cycle, Guided counts down the file's length or up with Stopwatch Mode, and a Timer or Box Breath countdown of 0 seconds or a Guided file of 0 seconds starts nothing.

#### Scenario: Each mode
- **WHEN** core builds the shape for Timer 10 min, Timer with Stopwatch Mode, Box Breath 60 s with a 16 s cycle, Box Breath with Stopwatch Mode, Guided of 900 s counting down and counting up, and Timer of 0 s counting down and Guided of 0 s counting down and up
- **THEN** it gives a 600 s countdown, a stopwatch, a 64 s Box Breath countdown, a Box Breath stopwatch, a 900 s guided countdown and a 900 s guided count-up, and nothing for the three 0 s cases
- **TEST** `meditate-core/src/session/settings.rs::each_mode_builds_its_shape`

### Requirement: The session's label is decided once
Both apps SHALL save a session with its mode's label only when that mode's label switch is on and the label still exists.

#### Scenario: Switch off, on, and the label gone
- **WHEN** the label for Timer is set and its switch is off; then on; then the label is deleted
- **THEN** the session's label is none; the label; none
- **TEST** `meditate-core/src/labels.rs::a_session_takes_its_label_only_when_the_switch_is_on`
