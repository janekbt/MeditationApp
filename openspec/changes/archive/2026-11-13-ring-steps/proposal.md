# Ring steps

## Why
REFACTORING #274 (P5): the ring around the time is rebuilt and redrawn on every frame. Measured on the Librem 5 (2026-11-12, release build, `top`, Meditate alone): a 1 hour Timer countdown keeps one of its four cores at about 60 % without bells and at about 98 % with a bell every 5 seconds (720 beads). Over an hour the fill grows by a visible pixel only every few seconds, so almost all of that work draws the same picture again.

## What Changes
- The ring's fill and a random bell's bead riding the fill move in steps of 1/2000 of the circle, the small preparation ring in 1/200 of its own (about a pixel: under one on the Librem 5, about 1.3 device pixels on the FP5, where the 200 ms glide smooths it), rounded down, so a ring stopped a second before its end never looks complete. Between two steps the ring is the same.
- Core gets a cheap check of everything the ring and its counters depend on that changes while a session runs: the fill and preparation steps, the phase, paused or not, and a count that grows each time a bell rings or is passed. It costs the same with 720 beads as with none, and says whether the ring moves (running or preparing, not paused).
- Both apps run the check on every frame (GTK) or tick (Android, 200 ms) through one core call that builds the whole ring, beads and counters only when it changed, from one look at the clock: a 1 hour ring with a bell every 5 s about every 1.8 s and at each ring, instead of 60 times a second.
- GTK: the frame callback runs while the ring moves and draws only when the check changed, and stops when the ring rests; the quarter-second refresh starts it again (after Resume) and keeps the counters of a ring-less stopwatch current. Today it stops at the first unchanged frame, which with steps would make every ring longer than about 30 s jump four times a second.
- Android: the last check is kept inside the session's own box, so it starts empty with every new session and goes with Save or Discard; a new session never keeps the last one's beads.
- A repeating bell's planned rings are walked one by one instead of collected into a list first (#274's iterator half, also listed in #289).
- A short session looks as now: a 1 minute ring takes a step every 30 ms, so it still glides at every frame.
- Found on the Librem 5 (2026-11-13): the Linux tick counted whole seconds from the session's start, so after a preparation or a Resume every bell rang up to a second late and its bead rode the fill first. Core now says how long until the session clock's next whole second, and the Linux tick waits for it (Box Breath keeps 200 ms).
- Left out: #338's quarter-second refresh that runs on after Stop. It is cheap and not what the measurement found; it stays in #338.

## Decisions
- Measure first, then fix (Janek, 2026-11-12); the measured fix plus the cheap check and the list-free rings (Janek: "Go" twice, the second after asking whether the computing is needed at all).
- Fix the late Linux tick in this change, on the session's whole seconds rather than a 200 ms tick (Janek, 2026-11-13).
- 1/2000 as the step (my call): 600 physical pixels across on the Librem 5 (scale 2) make a circumference of about 1900 pixels, so a step is under a pixel there and about 1.3 on the FP5 (checked on the device); a 1 hour ring redraws every 1.8 s.
- Bead positions that do not follow the clock (a one-off bell, a planned or rung repeating bell) are not stepped (my call): they do not change between frames, and stepping would move them off their exact time.
- The step and the check live in core, so both apps get them from one place, and the check uses the same steps as the ring (my call).
- A riding bead sits exactly at its earliest time until the stepped fill reaches that time, then on the fill (my call): it never rides early, never moves backwards, never sits off the fill while riding, and moves only when the check changes.

## Capabilities
### Modified Capabilities
- `timer`: the ring moves in steps too small to see.

## Impact
- Core: `meditate-core/src/session/mod.rs` (`bell_ring`, new `ring_key`, `bell_ring_if_changed` and `until_next_second`), `meditate-core/src/bells.rs` (`rings_before` as an iterator).
- GTK: `meditate-gtk/src/window/imp.rs` (`bell_ring`: one `sync` for frame callback and refresh, `show_counters`; source test `the_ring_follows_every_frame` follows), `meditate-gtk/src/timer/{mod,imp}.rs` (`bell_ring_if_changed` replaces `bell_ring`; `start_tick` waits for the next whole second, `session_tick` returns a bool).
- Android: `meditate-android/src/app.rs` (`AppState`'s `Live` box keeps the last check, `bell_ring_if_changed` replaces `bell_ring`), `meditate-android/src/ui.rs` (`refresh` sets ring and counters only on a change). No .slint change.
- Translations: none.
- Data: none.
- Android build environment: untouched.
- Docs: REFACTORING.md (#274 done with the measurement, P5 row, #289's iterator half), the timer spec.
