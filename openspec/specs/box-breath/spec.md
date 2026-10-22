# Box Breath

## Purpose
Box Breath guides breathing in four phases (in, hold, out, hold) with
an optional sound or vibration cue per phase, for a set number of
cycles or open-ended.

## Requirements

### Requirement: A Box Breath session with a set length saves that length
When a Box Breath session reaches its set length, the saved session SHALL have exactly that length, however late the ending tick arrives.

#### Scenario: The screen blanks near the end
- **WHEN** a 5-minute Box Breath session's last tick arrives 40 minutes after the start
- **THEN** the session is saved as 5 minutes
- **TEST** `meditate-core/src/session/mod.rs::a_box_breath_session_ending_late_saves_its_set_length`

### Requirement: Cues follow the breath from the first inhale to the end
With cues on, Box Breath SHALL cue the first inhale when the session starts, cue each later phase change, and give no cue for a cycle that would start at the end.

#### Scenario: The first inhale
- **WHEN** a Box Breath session with cues on starts
- **THEN** the "breathe in" cue plays at once
- **TEST** `meditate-core/src/session/mod.rs::box_breath_cues_the_first_inhale_at_the_start`

#### Scenario: The end on a cycle boundary
- **WHEN** a 4-4-4-4 session of 320 s is ticked every second through its end
- **THEN** the last tick gives the end and the end bell, and no "breathe in" cue
- **TEST** `meditate-core/src/session/mod.rs::box_breath_ends_without_cueing_another_cycle`

### Requirement: Box Breath on Linux runs while its window isn't drawn
On Linux, cues and the end of a Box Breath session SHALL happen on time while the window is minimised, on another workspace or behind a blank screen.

#### Scenario: Window minimised
- **WHEN** the window is minimised during a Box Breath session
- **THEN** the phase cues keep playing and the session ends on time
- **TEST** `meditate-gtk/src/timer/mod.rs::box_breath_ticks_on_a_timer_not_on_redraws`

### Requirement: The Duration row shows the length of the mode on screen
On Linux, the Setup page's Duration row SHALL show the session length of the mode that is selected, whenever the page is shown or reloaded.

#### Scenario: Back from Stats in Box Breath with a different Timer length
- **WHEN** Box Breath is set to 5 minutes and Timer to 10 minutes, Box Breath is selected, and the user opens Stats and comes back
- **THEN** the Duration row reads 0:05
- **TEST** `meditate-gtk/src/timer/mod.rs::the_duration_row_always_shows_the_selected_modes_length`

#### Scenario: Timer selected when the settings load
- **WHEN** Timer is selected with 10 minutes and Box Breath is set to 5 minutes, and the Setup page reloads its settings
- **THEN** the Duration row reads 0:10
- **TEST** `meditate-gtk/src/timer/mod.rs::the_duration_row_always_shows_the_selected_modes_length`

### Requirement: The counter shows the running session's length
On the phone, the Box Breath counter SHALL show the length of the running session, also after Pause, Resume or a sync that changes Setup.

#### Scenario: Setup changes during a 10-minute session
- **WHEN** a 10-minute session runs, Setup changes to 20 minutes through a sync, and the user pauses and resumes
- **THEN** the counter reads ".. / 10:00" and the session ends at 10:00
- **TEST** `meditate-android/src/app.rs::the_box_breath_counter_reads_the_running_session`

### Requirement: Android's breath dot follows the rounded square without jumps
On Android the Box Breath dot SHALL travel along the rounded square's outline, each phase ending where the next one starts.

#### Scenario: Phase boundaries and corners
- **WHEN** the dot reaches the end of In, Hold, Out and Hold
- **THEN** it stands where the next phase starts, on the outline, and halfway through a corner it lies on the corner's arc
- **TEST** `meditate-android/src/app.rs::breath_dot_path_has_no_jumps`
