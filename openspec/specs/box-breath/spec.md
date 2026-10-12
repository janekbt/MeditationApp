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
