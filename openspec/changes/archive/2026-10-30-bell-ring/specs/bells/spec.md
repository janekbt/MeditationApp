## MODIFIED Requirements

### Requirement: Jitter is a time
A repeating bell's jitter SHALL be a time from 0 to half the bell's time, and each ring SHALL land anywhere from its planned time minus the jitter to its planned time plus the jitter, whenever the ring before it came.

#### Scenario: Every 7 minutes give or take one minute
- **WHEN** a bell rings every 7 min ±1 min
- **THEN** its first ring is between 6:00 and 8:00, its second between 13:00 and 15:00, and the lowest and highest draws give those bounds
- **TEST** `meditate-core/src/bells.rs::a_jitter_in_seconds_bounds_each_gap`

#### Scenario: A late ring does not delay the next
- **WHEN** a bell every 5 min ±1 min rings first at 5:59
- **THEN** its second ring is still between 9:00 and 11:00
- **TEST** `meditate-core/src/bells.rs::each_ring_keeps_to_its_own_planned_time`

#### Scenario: A jitter longer than half the time
- **WHEN** a bell every 60 s has a jitter of 45 s
- **THEN** its first ring is between 30 s and 90 s, its second between 90 s and 150 s, and the stored jitter stays 45 s
- **TEST** `meditate-core/src/bells.rs::a_jitter_is_capped_at_half_the_time`

### Requirement: A repeating bell's gap can grow by a step
A repeating bell SHALL have a step, 0 to 60 min; each gap between planned times SHALL be the previous gap plus the step, and a jitter SHALL apply around each planned time.

#### Scenario: Every 60 seconds, growing by 5
- **WHEN** a bell rings every 60 s with a step of 5 s and no jitter
- **THEN** it rings at 1:00, 2:05, 3:15 and 4:30
- **TEST** `meditate-core/src/bells.rs::a_step_grows_each_gap`

#### Scenario: A time jump with a step
- **WHEN** that bell's session is suspended at 0:59 and woken at 3:20
- **THEN** the bell rings once, and next at 4:30
- **TEST** `meditate-core/src/bells.rs::a_step_survives_a_time_jump`

#### Scenario: A bell read with no time
- **WHEN** a repeating bell arrives with no time, from an old row or an older device's event
- **THEN** it rings once a second, and the session goes on
- **TEST** `meditate-core/src/bells.rs::a_bell_with_no_time_still_moves_on`

#### Scenario: A fixed bell has no jitter or step
- **WHEN** a bell every 60 s ±30 s, +5 s is switched to "At time from start", in either app
- **THEN** it is saved with no jitter and no step
- **TEST** `meditate-core/src/bells.rs::a_fixed_bell_keeps_no_jitter_or_step`

#### Scenario: The row title names all three
- **WHEN** a bell is every 120 s ±1 min with a step of 5 s, and another is the same with ±3 min
- **THEN** both rows read "Every 120 s ±1 min, +5 s" in both apps: a jitter shows as it rings
- **TEST** `meditate-core/src/bells.rs::the_title_key_carries_each_time_with_its_unit`

#### Scenario: The same titles on the laptop and the phone
- **WHEN** those two bells are listed in each app
- **THEN** the laptop reads "Every 120 s ±1 min, +5 s" and the phone "Every 120 s +/-1 min, +5 s" (its font has no plus-minus sign), for both rows
- **MANUAL** the GTK title is unit-tested (`bell_title_names_time_jitter_and_step_with_their_units`); the phone's title goes through Slint's catalogue, which only runs on Android

## ADDED Requirements

### Requirement: A random bell that can ring before the end does
In a countdown, a repeating bell's ring whose earliest time lies before the end SHALL ring before the end, anywhere from its earliest time to the end.

#### Scenario: The last window reaches past the end
- **WHEN** a 30 minute Timer has a bell every 5 min ±1 min
- **THEN** its sixth ring, planned at 30:00, rings between 29:00 and the end, for every draw
- **TEST** `meditate-core/src/bells.rs::a_ring_that_can_come_before_the_end_does`

#### Scenario: Without jitter nothing changes at the end
- **WHEN** a 30 minute Timer has a bell every 5 min without jitter
- **THEN** it rings at 5, 10, 15, 20 and 25 minutes, and not at 30:00
- **TEST** `meditate-core/src/bells.rs::a_ring_that_can_come_before_the_end_does`
