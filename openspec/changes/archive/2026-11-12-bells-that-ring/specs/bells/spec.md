## ADDED Requirements

### Requirement: After a suspend only the latest missed fixed bell rings
When a device wakes from a suspend past several fixed bells, the session SHALL ring only the latest of them and leave the earlier ones unheard.

#### Scenario: Three fixed bells slept through
- **WHEN** a 60 minute Timer with fixed bells at 10, 20 and 30 minutes is suspended at 5 minutes and woken at 35
- **THEN** one bell rings; the 30 minute bead is filled, the 10 and 20 minute beads stay hollow, and the counter reads "1 of 3"
- **TEST** `meditate-core/src/session/mod.rs::after_a_suspend_only_the_latest_missed_fixed_bell_rings`

### Requirement: The Bells count leaves out bells that can't ring at the set length
The Timer's Bells subtitle and the main Bells row SHALL count only the enabled bells that can ring before the end at the Timer's set length and that the Timer's signal setting does not mute.

#### Scenario: A 10 minute Timer, then 40 minutes
- **WHEN** the enabled bells are fixed at 5 minutes, fixed at 30 minutes, every 15 minutes and 20 minutes before the end, and the Timer is set to 10 minutes; then to 40 minutes
- **THEN** the count is 1; then 4
- **TEST** `meditate-core/src/bells.rs::the_bell_count_leaves_out_bells_that_cannot_ring`

#### Scenario: A bell the Timer's signal setting mutes
- **WHEN** a sound-only fixed bell at 5 minutes and a vibration-only one at 6 minutes are enabled, and the Timer's signal setting is vibration only
- **THEN** the Bells count is 1, and a session shows one bead and "0 of 1"
- **TEST** `meditate-core/src/session/settings.rs::a_muted_bell_is_left_out`

#### Scenario: The length or the signal setting changes on the Setup page
- **WHEN** on either app the Timer length goes from 40 to 10 minutes with the bells above (on the main screen or the Session page), or the Timer's signal setting changes so a bell is muted
- **THEN** the main Bells row and the Bells page's count show the new number at once, and an open Manage Bells (its start bar sets the length too) greys or frees its bells at once
- **MANUAL** the refresh is UI wiring; checked on the laptop and the phone

### Requirement: Manage Bells greys out a bell that can't ring
Manage Bells SHALL show a switched-on bell that can't ring at the Timer's set length, or that the Timer's signal setting mutes, as off and greyed, without changing the saved switch, as it does for a bell timed from the end in Stopwatch.

#### Scenario: A 10 minute Timer
- **WHEN** a switched-on fixed bell at 30 minutes, a switched-on fixed bell at 5 minutes and a switched-off fixed bell at 5 minutes are listed with the Timer at 10 minutes
- **THEN** the 30 minute bell is off and greyed, the switched-on 5 minute bell is on, and the switched-off one is off and can be switched on
- **TEST** `meditate-core/src/bells.rs::a_bell_that_cannot_ring_is_greyed_off`

### Requirement: The Bells row names only bells that ring
The main Bells row SHALL leave out a starting bell, an end bell or Box Breath phase cues that the mode's signal setting mutes.

#### Scenario: Sound-only bells under a vibration-only setting
- **WHEN** the Timer has a sound-only starting and end bell and its signal setting is vibration only
- **THEN** the main Bells row names neither
- **TEST** `meditate-core/src/preset_config.rs::bells_summary_leaves_out_muted_bells`

### Requirement: A preset counts only the bells that ring
A preset's subtitle SHALL count only the switched-on bells that can ring at the preset's length and under its signal setting.

#### Scenario: A 10 minute preset
- **WHEN** a 10 minute Timer preset carries switched-on fixed bells at 5, 15 and 20 minutes, and one at 6 minutes that is sound-only while the preset's signal setting is vibration only
- **THEN** its subtitle counts 1 bell
- **TEST** `meditate-core/src/format.rs::a_preset_counts_only_bells_that_can_ring`
