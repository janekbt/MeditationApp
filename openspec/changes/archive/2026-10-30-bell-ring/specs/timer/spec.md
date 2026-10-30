## ADDED Requirements

### Requirement: A ring shows how far the session is
The running page of a Timer countdown or a Guided session SHALL show a ring around the time that fills clockwise from the top in proportion to the elapsed part of the planned length, and stands still while paused.

#### Scenario: A quarter of a 20 minute timer has passed
- **WHEN** a 20 minute Timer countdown has run for 5 minutes
- **THEN** a quarter of the ring is filled
- **TEST** `meditate-core/src/session/mod.rs::the_ring_fills_with_the_session`

#### Scenario: Paused, the ring stands still
- **WHEN** the session is paused at 5 minutes and 3 minutes pass
- **THEN** the ring still shows 5 minutes
- **TEST** `meditate-core/src/session/mod.rs::the_ring_fills_with_the_session`

#### Scenario: Stopwatch and Box Breath have no ring
- **WHEN** a Timer stopwatch or a Box Breath session runs
- **THEN** there is no ring
- **TEST** `meditate-core/src/session/mod.rs::only_a_known_length_gets_a_ring`

#### Scenario: Guided gets a ring without bells
- **WHEN** a 10 minute guided track plays
- **THEN** the ring fills over the 10 minutes and holds only the start point
- **TEST** `meditate-core/src/session/mod.rs::only_a_known_length_gets_a_ring`

#### Scenario: The ring on the phone and the laptop
- **WHEN** a 30 minute Timer with bells runs on the phone and on the laptop, in portrait and in a short window
- **THEN** the ring sits around the time, the time is never cut, and the buttons stay where they were
- **MANUAL** layout on a real screen; a person checks the FP5 and the GTK window at 400 and 700 px height

### Requirement: Each bell is a bead on the ring
The ring SHALL show each bell of the session as a bead at its time, small for a repeating bell and larger for a one-off bell, filled once it rang and hollow before.

#### Scenario: A one-off bell before and after it rings
- **WHEN** a 30 minute Timer has a bell 3 minutes from the start and 2 minutes before the end, and 10 minutes have passed
- **THEN** there is a large filled bead at 3 minutes and a large hollow bead at 28 minutes
- **TEST** `meditate-core/src/session/mod.rs::one_off_bells_are_large_beads`

#### Scenario: A one-off bell slept through stays hollow
- **WHEN** a 30 minute Timer has a bell 2 minutes before the end and the device sleeps from 27:00 to 31:00
- **THEN** in overtime its bead is still hollow, because it never rang
- **TEST** `meditate-core/src/session/mod.rs::a_one_off_bell_slept_through_stays_hollow`

#### Scenario: A repeating bell lists every ring that fits
- **WHEN** a 30 minute Timer has a bell every 5 minutes without spread, and 12 minutes have passed
- **THEN** small beads sit at 5, 10, 15, 20 and 25 minutes, the first two filled
- **AND** the counter reads "2 of 5" and, once the last has rung, "5 of 5": no bead and no count for 30:00, where it would not ring
- **TEST** `meditate-core/src/session/mod.rs::repeating_bells_are_small_beads`

#### Scenario: A bell with growing gaps
- **WHEN** a 30 minute Timer has a bell every 5 minutes growing by 1 minute
- **THEN** beads sit at 5, 11, 18 and 26 minutes
- **TEST** `meditate-core/src/session/mod.rs::repeating_bells_are_small_beads`

#### Scenario: A bell skipped by a suspend
- **WHEN** a 30 minute Timer has a bell every 5 minutes without spread and the device sleeps from 4:00 to 12:00
- **THEN** the bell rings once at 12:00: its 10 minute bead rides to 12:00 and is filled, its 5 minute bead stays hollow because it never rang, and the counter reads "1 of 5"
- **TEST** `meditate-core/src/session/mod.rs::repeating_bells_are_small_beads`

### Requirement: A random bell waits at its earliest time and stays where it rang
A repeating bell with random spread SHALL show each bead hollow at the earliest time its ring can come, ride a bead whose time has come along the front of the fill until it rings, and keep it where it rang.

#### Scenario: The bell rings late
- **WHEN** a 30 minute Timer has a bell every 5 minutes give or take 1 minute, which rings first at 5:40
- **THEN** at 3:00 its beads are hollow at 4, 9, 14, 19, 24 and 29 minutes; at 5:20 the first is hollow at 5:20; after the ring it is filled at 5:40 and the next is still at 9:00
- **TEST** `meditate-core/src/session/mod.rs::a_random_bell_rides_the_fill_until_it_rings`

#### Scenario: The bell rings early
- **WHEN** the same bell rings first at 4:30
- **THEN** at 4:15 its first bead is hollow at 4:15; after the ring it is filled at 4:30 and the next is still at 9:00
- **TEST** `meditate-core/src/session/mod.rs::a_random_bell_rides_the_fill_until_it_rings`

### Requirement: Repeating bells are counted under the time
The running page SHALL show one counter per repeating bell under the time, in an order fixed at the start by each bell's earliest first ring, as "rung of total" in a countdown and as the plain number in the Timer stopwatch.

#### Scenario: Two repeating bells in a countdown
- **WHEN** a 30 minute Timer has a bell every 7 minutes and one every 5 minutes, and 12 minutes have passed
- **THEN** the counters read "2 of 5" and then "1 of 4"
- **TEST** `meditate-core/src/session/mod.rs::the_counters_keep_their_order`

#### Scenario: A late first ring does not change the order
- **WHEN** a bell every 5 minutes give or take 2 and a bell every 4 minutes run, and the first one's first ring comes at 7:00
- **THEN** the 5 minute bell's counter stays first the whole session, its earliest first ring being 3:00
- **AND** two bells with the same earliest first ring keep the order of the bell list
- **TEST** `meditate-core/src/session/mod.rs::the_counters_keep_their_order`

#### Scenario: Stopwatch counts without a total
- **WHEN** a Timer stopwatch with a bell every 5 minutes has run 12 minutes
- **THEN** the counter reads "2" and there is no ring
- **TEST** `meditate-core/src/session/mod.rs::the_stopwatch_counts_without_a_total`

#### Scenario: The counter text in every language
- **WHEN** the app runs in any of its nine languages
- **THEN** the counter reads "2 of 5" in that language, in both apps
- **TEST** `meditate-android/src/app.rs::ui_text_is_translated_whole`

### Requirement: Preparation fills a small ring at the start point
During the preparation before the starting bell, the page SHALL show the ring empty, its beads hollow, with a small ring around the start point that fills over the preparation time.

#### Scenario: Halfway through a 10 second preparation
- **WHEN** a Timer with 10 seconds of preparation is 5 seconds in
- **THEN** the small ring is half filled, the big ring is empty, and the start point is hollow
- **AND** after the starting bell the small ring is gone and the start point is filled
- **TEST** `meditate-core/src/session/mod.rs::preparation_fills_the_small_ring`

### Requirement: Overtime shows a full ring
Once a countdown passes its end, the ring SHALL be full with every rung bead filled, and each counter SHALL show only its number, still counting the bells that ring in overtime.

#### Scenario: A bell rings on in overtime
- **WHEN** a 30 minute Timer with a bell every 5 minutes give or take 1 has rung six times and passes the end
- **THEN** the ring is full and the counter reads "6"
- **AND** when it rings again in overtime the counter reads "7"
- **TEST** `meditate-core/src/session/mod.rs::overtime_shows_a_full_ring`
