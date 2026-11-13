## ADDED Requirements

### Requirement: The ring moves in steps too small to see
The ring's fill, its preparation ring and a bead riding the fill (once the fill reached the bead's time) SHALL move in steps, one two-thousandth of the circle for the ring and one two-hundredth for the small preparation ring, rounded down, and both apps SHALL build and draw the ring and its counters again only when a step, a bell, a pause or the phase changed them.

#### Scenario: A 1 hour ring holds still between steps
- **WHEN** a 1 hour Timer countdown with a bell every 5 seconds has run 15:00, then 15:01, then 15:02
- **THEN** at 15:00 and 15:01 the ring is the same, filled 500 of 2000 steps; at 15:02 it is filled 501 of 2000
- **TEST** `meditate-core/src/session/mod.rs::a_long_ring_holds_still_between_steps`

#### Scenario: A random bell's bead rides the stepped fill
- **WHEN** a 1 hour Timer has a bell every 10 minutes give or take 1 minute, and 9:10 has passed without its first ring; and a 50 minute Timer has a bell whose earliest first ring is at 16:41, seen at 16:40.5, 16:41.1 and 16:43
- **THEN** in the first its bead sits exactly at the front of the fill; in the second it stays at 16:41 while the fill's step is still below that time (16:40.5 and 16:41.1), never earlier, and at 16:43 it sits at the front of the fill
- **AND** a 2 hour Timer stopped a second before its end shows a ring one step short of full, not a complete one
- **TEST** `meditate-core/src/session/mod.rs::a_long_ring_holds_still_between_steps`

#### Scenario: The quick check changes exactly when the ring or its counters do
- **WHEN** a Timer countdown with a 10 second preparation and a bell every 5 seconds is checked within one step, across a step, when its preparation ends, when a bell rings, when it is paused, when it reaches its end and when it is stopped
- **THEN** the quick check stays the same within the step and changes at each of the others, and it says the ring moves only while running or preparing and not paused
- **TEST** `meditate-core/src/session/mod.rs::the_ring_check_changes_with_the_ring`

#### Scenario: A new session on the phone draws its own beads
- **WHEN** a session with bells is discarded within its first second and a session with other bells starts
- **THEN** the phone shows the new session's beads at once
- **TEST** `meditate-android/src/app.rs::a_new_session_brings_its_own_ring`

#### Scenario: A long session costs little on the Librem 5
- **WHEN** a 1 hour Timer countdown runs on the Librem 5, once with a bell every 5 seconds and once without bells, and Meditate is measured with `top` for 30 seconds
- **THEN** both stay far below the 60 % and 98 % of one core measured before, and a 1 minute ring still glides
- **MANUAL** load and smoothness on the device; Claude reads `top` over SSH, Janek watches the ring

### Requirement: The Linux app ticks on the session's whole seconds
The Linux app SHALL tick when the session's clock reaches each whole second, also after the preparation ends and after Resume, so a bell rings at its second and its bead does not ride the fill first.

#### Scenario: Bells ring on time after a preparation and after a pause
- **WHEN** a Timer countdown with a 10 second preparation and a bell every 5 seconds sees the tick that ends its preparation 0.4 s late, and is later paused 6.3 s in and resumed
- **THEN** the next tick is due 0.6 s after that late tick, the bell rings on the tick 5 s into the meditation, and after Resume the next tick is due 0.7 s later
- **TEST** `meditate-core/src/session/mod.rs::the_next_tick_lands_on_the_sessions_whole_seconds`

#### Scenario: The Linux tick waits for the session's next second
- **WHEN** the Linux app starts its once-a-second tick for a Timer or Guided session
- **THEN** it waits until the session's next whole second and sets the next tick from there, not a fixed second from the start
- **TEST** `meditate-gtk/src/timer/mod.rs::the_tick_follows_the_sessions_seconds`

#### Scenario: A bead fills in place after a preparation on the Librem 5
- **WHEN** a 1 minute Timer with a 10 second preparation and a bell every 5 seconds runs on the Librem 5
- **THEN** each bead fills at its place when its bell rings, without riding the fill first
- **MANUAL** Janek watches the ring

## MODIFIED Requirements

### Requirement: A random bell waits at its earliest time and stays where it rang
A repeating bell with random spread SHALL show each bead hollow at the earliest time its ring can come, ride a bead along the front of the fill once the fill's step reached its time, until it rings, and keep it where it rang.

#### Scenario: The bell rings late
- **WHEN** a 30 minute Timer has a bell every 5 minutes give or take 1 minute, which rings first at 5:40
- **THEN** at 3:00 its beads are hollow at 4, 9, 14, 19, 24 and 29 minutes; at 5:20 the first is hollow on the fill's step at 5:20; after the ring it is filled at 5:40 and the next is still at 9:00
- **TEST** `meditate-core/src/session/mod.rs::a_random_bell_rides_the_fill_until_it_rings`

#### Scenario: The bell rings early
- **WHEN** the same bell rings first at 4:30
- **THEN** at 4:15 its first bead is hollow on the fill's step at 4:15; after the ring it is filled at 4:30 and the next is still at 9:00
- **TEST** `meditate-core/src/session/mod.rs::a_random_bell_rides_the_fill_until_it_rings`
