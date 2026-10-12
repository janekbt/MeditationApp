# Bells

## Purpose
Bells mark a session: a starting bell, interval bells during it and
an end bell. Each can play a sound, a vibration or both, at the time
the user set.

## Requirements

### Requirement: An interval bell rings at most once per tick
After a time jump, an interval bell SHALL ring once and continue from its next due time after now, not ring for every interval it missed.

#### Scenario: Suspend across several intervals
- **WHEN** a session with a bell every 5 minutes is suspended at 4:59 and woken at 25:00
- **THEN** the bell rings once, and next at 30:00
- **TEST** `meditate-core/src/bells.rs::an_interval_bell_rings_once_after_a_time_jump`

### Requirement: The end bell is not followed by an interval bell
An interval bell due at or before the moment the countdown ends SHALL NOT ring after the end bell.

#### Scenario: Interval bell due exactly at the end
- **WHEN** a 10-minute countdown has a bell every 5 minutes
- **THEN** at 10:00 only the end bell rings, and no interval bell rings at 10:01
- **TEST** `meditate-core/src/session/mod.rs::an_interval_bell_due_at_the_end_gives_way_to_the_end_bell`
