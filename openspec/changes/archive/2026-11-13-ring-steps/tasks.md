# Tasks: ring-steps

Base: b32d428 (gtk, android: a tapped number field readies for a new number); suites on it 2026-11-12: core 1551 green, GTK 180 green, Android 295 green

## Loop
- [x] 0 Skills loaded, again after every /compact: `/ponytail ultra`; `slint:slint` (loaded for the Android ring check; no .slint change planned)
- [x] 1 Understand (Janek chose "measure first"; Librem 5 measured 2026-11-12: 1 h countdown 60 % of a core without bells, 98 % with a bell every 5 s; 1 minute ring 60 % and 70 %; fix said back, Janek: "Go")
- [x] 2 Propose (approved 2026-11-13, Janek: "Go")
- [x] 3 Build (core 1553, GTK 180, Android 296 green; clippy GTK all-targets and CI clean; debug APK built, ui.rs compiles)
- [x] 4 Review (five rounds; round 5 found no bug, its notes below; /simplify after)
- [x] 5 Specs, docs, list (check-specs green; REFACTORING: P5 ticked, #274 gone, #289 and #338 lines updated; ARCHITECTURE does not describe the ring; OPEN-ISSUES no item)
- [x] 6 Verify and hand over (Librem 5: 3.5 % and 17 % instead of 60 % and 98 %; the late tick found and fixed; Janek: "Funktioniert jetzt richtig")
- [x] 7 Ship (deltas merged, archived, check-specs green; Janek: "Go", FP5 looks as before)

## Review Focus
- Paused, ended, overtime: the fill is 1.0 or still; stepping must not move a still ring (exact 1.0 stays 1.0, 0 stays 0).
- Preparation: the small ring is stepped the same way; a 5 s preparation still glides.
- A random bell's riding bead sits at its exact earliest time until the stepped fill reaches it, then on the fill's step (F13); never early, never backwards.
- Guided: same ring, same step; a track's length can be any number of seconds.
- Existing tests with exact fractions (`an_ended_session_shows_its_length` 420/2700) now see the stepped value; the expected values are rewritten by hand, not computed by the stepping code.
- GTK: the frame clock rests while the ring is unchanged and the quarter-second refresh restarts it (existing `the_ring_follows_every_frame`); Android: unchanged values cause no redraw, the 200 ms glide animation runs only after a step.

- The check must change whenever `bell_ring`'s output changes: every input `bell_ring` reads that moves during a session (phase, pause, track end, `fired`, `gaps`, `rang`, the stepped elapsed and prep) is in the key or follows from it. A missed one shows stale beads or counters.
- Android: the stored key belongs to one session; a new session, Discard, or the undo of a discard must never keep the old ring.
- A ring-less stopwatch with bells: only counters; they must still update (GTK refresh, Android refresh).

## 1. Core
- [x] 1.1 `a_long_ring_holds_still_between_steps` (1 h countdown, bell every 5 s ticked up to 15:00: ring equal at 900 s and 901 s with fill 500/2000, 501/2000 at 902 s (rounded down since F3); bell every 10 min ±1 min first ringing at 10:20 (`every(600, 60, 0, Some(3600), 620)`), at 550 s its first bead at 305/2000 = the fill). Expected: FAIL, then PASS. Ran: failed (ring changed within the step), passes with the steps.
- [x] 1.2 Step fill, prep and the riding bead in `bell_ring`; rewrite by hand the expectations of existing tests that hit a non-step fraction (`an_ended_session_shows_its_length` 311/2000, riding beads near lines 2681 (356/2000) and 2686 (283/2000)).
- [x] 1.3 `the_ring_check_changes_with_the_ring` (prep 10 s, bell every 5 s, 1 h: same within a step; changes across a step, at prep end, at a ring, at pause, at end, at stop; `moving()` true while running and preparing, false paused, in overtime, stopped). Compile stub first, FAIL, then `ring_key` (phase, paused, fill step, prep step, the whole phase (`Stopped` with its fields), a bell count: one-off `fired.is_some()` plus each interval's `gaps`, which grows on every heard and skipped ring), sharing the step helper with `bell_ring`. Ran: stub failed on `moving()`; passes; with the bell count left out it fails at the 15 s ring.
- [x] 1.4 `rings_before` as an iterator; the existing bead tests stay green (pure refactor, no new test). Core 1553 green.

## 1b. GTK
- [x] 1.5 Update the source test `the_ring_follows_every_frame` first (FAIL), then the code. Final form (after F4): one `sync` closure calls `bell_ring_if_changed`, draws a changed ring and updates the counters; the frame callback continues while it moves and breaks after the last change is drawn; the quarter-second refresh calls `sync` and restarts the frame clock. Proof of the glide: MANUAL on the laptop and the Librem 5.

## 1c. Android
- [x] 1.6 `a_new_session_brings_its_own_ring` (start a session with one bell, take its ring, dismiss/discard, start one with other bells at the same moment: `bell_ring_if_changed` returns the new beads; asked again unchanged it returns nothing). FAIL, then `AppState`'s box keeps the last key (`Cell`, so `refresh(&AppState)` can write it) and `ui.rs` `refresh` sets ring and counters only on `Some`.

## 2. Device
- [x] 2.1 Cross-build for the Librem 5 (GNOME 50 sysroot, copy of `dev-xbuild.sh` in the scratchpad: the repo script now wants GNOME 51, switch later per Janek), re-measure the two 1 h runs and a 1 minute ring. Measured 2026-11-13 (md5 c99637fe…): 1 h with a 5 s bell about 17 %, 1 h without bells about 3.5 % of one core. The frame loop costs at most the 3.5 %, the rest is drawing 720 beads when the check changes; no frame-loop fix needed.
- [ ] 2.2 FP5 debug APK: a 1 minute ring and a 1 h ring with a 5 s bell look as before.

## 2b. Device finding (Librem 5, 2026-11-13): the late Linux tick
Janek: with a preparation, a 5 s bell's bead rides the fill for about a second, then fills at its place; without a preparation it fills in place. Cause (older than this change, base rode too): GTK's 1 s tick counted from the session's start, and the meditation clock starts at the preparation's end (or continues at Resume) between two ticks, so every bell rang up to a second late. Janek chose to fix it here, with one review round of its own (the sixth).
- [x] 2b.1 Core `Session::until_next_second(now)`: test `the_next_tick_lands_on_the_sessions_whole_seconds` FAIL (stub 1 s), then PASS; core 1554 green.
- [x] 2b.2 GTK `start_tick`: Timer and Guided wait for the session's next whole second with a one-shot timeout and set the next tick after it; Box Breath keeps 200 ms. Source test `the_tick_follows_the_sessions_seconds` FAIL, then PASS; GTK 181 green; clippy clean.
- [x] 2b.3 Review round 6 (fix reviewer + /code-review high) on this fix only, then /simplify. Fixed: the one-shot fired just before the second (GLib cuts the wait to whole ms from its cached loop time), so most seconds ticked twice: 5 ms margin, source test pins it; core test also checks a whole second gives 1 s, never 0. /simplify: both modes now use the one self re-arming one-shot (Box Breath waits 200 ms after each tick, drifting by the tick's own work, its cues were already polled), `session_tick` returns a bool and lost its two `tick_source` takes. Core 1554, GTK 181, Android 296 green; clippy (GTK all-targets, CI) clean; check-specs green.
  - Rejected (code-review): a pause or hand-off inside `react` re-arming a tick: every such path comes with an end effect, so the tick does not continue.
  - Rejected (code-review): the GTK test reads source text: GTK's timer cannot run on the host; the timing itself is the core test and the device check.
  - Not done, out of this fix (code-review, /simplify altitude): Box Breath on GTK and the whole Android app still poll every 200 ms from their own start, so a bell or cue there is up to 0.2 s late after Resume. A core "how long until anything is due" would align both; Android's loop also polls widgets and sync, so it stays. Janek chose the whole-second fix over a 200 ms tick on Linux.
- [x] 2b.4 Librem 5: a 1 minute Timer with a 10 s preparation and a 5 s bell: beads fill in place (Janek, 2026-11-13: "Funktioniert jetzt richtig").

## Pre-build questions (fresh agent, 2026-11-12), my calls
- GTK stopped its frame callback at the first unchanged frame: with steps every ring over ~30 s would jump four times a second. Taken as task 1.5 (the check says whether the ring moves; GTK skips the draw, not the callback). Computing each frame is then only the cheap check (Janek asked for it, 2026-11-13).
- Rounding: to the nearest step, as the scenarios' numbers need. (Overruled by F3: rounded down.)
- Riding bead: exactly the fill once its earliest time has passed, exactly its earliest time before.
- Exact 1.0 (past end) and 0 stay as they are; only the elapsed part is stepped.
- Two riding-bead tests and `an_ended_session_shows_its_length` change their expected values (listed in 1.2).
- Android: the fill glides over 200 ms, the beads do not, so a riding bead can sit up to 200 ms ahead of the fill; that is today's behaviour, unchanged.

## Pre-build questions round 2 (fresh agent, 2026-11-13), my calls
- Riding bead: `max(part(earliest), stepped fill)`, so it is a function of the key and never jumps while the key stays (the agent's one coverage gap). (Replaced by F13: exact earliest time until the fill reaches it, then on the fill.)
- Key: whole `SessionPhase`, `is_paused()`, fill and prep steps, bell count `Σ fired.is_some()` + `Σ gaps` (`rang` always grows with `gaps`); `track_ended`, `next_ring_secs`, `planned_secs` are not read by `bell_ring`. Fill step clamped: 2000 past the end, so overtime makes no redraw; overtime bells change the count, which the refresh compares.
- GTK: one shared last-key cell for the frame callback and the refresh; the callback compares, stores, rebuilds and draws first, then breaks if `!moving()`, so the last frame of a pause or end is drawn. The refresh redraws ring and counters itself on a change and starts `follow` only when moving; `follow` also starts at build when moving.
- Android: the key lives in the box (now the named `Live { session, started, ring_seen }`, F8); no path rebuilds a session but `start_session` (no undo restores one, recovery only writes a row). With no session `bell_ring_if_changed` returns `Some(default)` so the hidden page never keeps old values.
- Rounding fills a 1 h ring up to 0.9 s before its end: under a pixel, accepted. (Gone with F3: rounded down never fills early.)
- GTK's reused "running" page during a pop animation: same risk as today, left alone.

## Review round 1 (reviewer agent + /code-review high, 2026-11-13)
Fixed:
- F1 (reviewer): the one-off bell's part of the check had no test; `the_ring_check_changes_with_the_ring` now ticks a 30 s one-off bell alone. Broke the Fixed arm to 0: failed.
- F2 (reviewer): the riding bead could sit a third of a step ahead of the fill (`part(earliest).max(filled)` with an off-step earliest). Now it rides once the fill reaches its earliest time's step and sits on the fill; before that at its exact time. Test: 50 min ring, earliest 1001 s, at 1001.1 s on step 667 (failed before).
- F3 (code-review): rounding to the nearest step made a ring stopped a second early look complete (a 2 h track stopped 1 s early: 1999.72 rounds to 2000). Now rounded down. Expected values rewritten by hand (15:00/15:01 same, 501 at 15:02; 305 at 9:10; riding 355); new assert: 1 h stopped at 59:59 fills 1999/2000. Broke `floor` back to `round`: failed. Spec delta and proposal follow.
- F4 (code-review): GTK kept two last-check cells (`drawn`, `counted`), so each change built the whole ring twice, and a change at rest started a frame callback to draw once. Now one `sync` closure owns the one cell, builds once, draws and updates the counters directly; frame callback and refresh both call it. `the_ring_follows_every_frame` rewritten for it.
- F5 (code-review): key and ring read the clock separately, so the key could be a step older than the ring. Core `Session::bell_ring_if_changed(now, seen)` takes both from one `now`; GTK's TimerView and Android's AppState call it (old `bell_ring` wrappers removed, no other callers).
- F6 (code-review): `RingKey.moving` duplicated phase and pause; now derived in `moving()`.
- F7 (code-review): `rings_before` used a flag `take_while`; now `Option::into_iter().flat_map(successors…)` (it works out one planned ring past the end, one addition, as round 5 noted).
- F8 (code-review, and clippy's type-complexity): Android's box is a named `Live { session, started, ring_seen }`.
Rejected:
- (code-review) Android with no session returns the empty ring each 200 ms tick: setting the same `ring-on` and an empty model is a no-op for Slint, and it guarantees the hidden page never keeps an old ring. Shown to Janek.
Suites after: core 1553, GTK 180, Android 296 green; clippy GTK all-targets and CI clean. Pre-existing (base too): an unused `body` closure in `drop_files_take_one_path_and_report_failures` warns in the Android test build; not this change's.

## Review round 2 (fix reviewer + /code-review high, 2026-11-13)
Fixed:
- F9 (reviewer): the F3 assert (1 h stopped at 59:59, 1999.44) passed with `round` too; now 2 h stopped at 7199 s (1999.72 rounds to 2000), and the delta names it.
- F10 (reviewer): `draw_bell_ring`'s doc comment had slid onto `show_counters`; moved back.
- F11 (reviewer): the living random-bell scenarios said "hollow at 5:20 / 4:15" while the test asserts the fill's step; MODIFIED block in the delta.
- F12 (reviewer): the riding scenario now names both setups it tests.
- F13 (code-review): the riding bead jumped onto the fill as soon as the fill reached its time's step, up to a step early (5.4 s on a 3 h ring) and backwards. Now it rides once the floored fill reached its exact time: never early, never backwards, on the fill while riding. Test: 50 min, earliest 1001 s: at 1000.5 s and 1001.1 s still at 1001 s, at 1003 s on the fill (668); failed before the fix.
- F14 (code-review): a bell with no time (old data) bumped `gaps` every second, so the check changed each second and the whole ring was rebuilt; such bells leave the count, as `bell_ring` skips them. Test in `the_ring_check_changes_with_the_ring`; failed before.
- F15 (code-review): "under a pixel" is not true on the FP5 (about 1.3 device pixels); wording fixed, the FP5 check in 2.2 looks at it.
- F16 (code-review): tasks 1.1's numbers updated to the rounded-down ones; the source test slices `sync` at its own closing line.
Rejected:
- (code-review) "no session" decided differently in the apps (GTK returns the empty ring once, Android each tick): GTK's page and Android's check both belong to the session by design (pre-build round 2); without a session Android repeats a no-op. Shown to Janek.
- (code-review) `bell_ring_if_changed` computes fill and prep twice on a change: once per step, from the same function and `now`; not worth a second entry point.
Suites after: core 1553, GTK 180, Android 296 green; clippy GTK all-targets and CI clean.

## Review round 3 (fix reviewer + /code-review high, 2026-11-13)
Fixed:
- F17 (reviewer): the MODIFIED requirement said a bead rides "whose time has come"; now "once the fill's step reached its time", as the code does.
- F18 (code-review): `floor(secs / end * 2000)` in floating point put some whole seconds right on a step one step low (1809 s of an hour gave 1004, not 1005). `step(secs, of)` multiplies first, so the division is exact. Test: an hour stopped at 1809 s fills 1005/2000; failed before.
- F19 (code-review): `ring_key` was public though only core uses it; now private (`RingKey` stays public for the apps' cells).
- F20 (code-review): task 1.5 and the pre-build round 2 note described the replaced design; updated.
Rejected:
- (code-review) On Stop the fill is recomputed from the saved whole seconds, so it can drop a step and a riding bead can go back to its own time: the ring shows the saved length, as the time does, as before this change; under a pixel. Shown to Janek.
- (code-review) "no session" in the apps, and fill computed twice on a change: rejected in round 2, same reasons.
Suites after: core 1553, GTK 180, Android 296 green; clippy GTK all-targets and CI clean.

## Review round 4 (fix reviewer + /code-review high, 2026-11-13)
Fixed:
- F21 (reviewer): the Loop line still said round 2 was open; updated.
- F22 (code-review): the preparation's small ring had 2000 steps of its own length (5 ms for 10 s), so during preparation the check changed every frame and the whole ring, 720 beads included, was rebuilt 60 times a second. Its ring now has 200 steps (half a pixel on its small circle; 50 ms for 10 s). `the_ring_check_changes_with_the_ring` asserts the same check at 2.000 and 2.040 s (failed with 2000 steps) and a change at 2.060 s.
- F23 (code-review): the "bell with no time" rule lived in `bell_ring` and `ring_key` separately; one `BellSchedule::untimed()` serves both.
- F24 (code-review): GTK's `sync` compared the whole ring (all beads) on each change though a changed check already means a change; it now stores and draws.
- F25 (code-review): `Session::bell_ring` is `pub(crate)`; the apps go through `bell_ring_if_changed` only.
- F26 (code-review): TimerView's `bell_ring_if_changed` got its doc, with why it returns the empty ring once without a session.
Suites after: core 1553, GTK 180, Android 296 green; clippy GTK all-targets and CI clean.

## Review round 5 (fix reviewer + /code-review high, 2026-11-13), the last
No bug found. Fixed (notes only): two stale pre-build notes, the Review Focus line on the riding bead, the F7 note.
Not fixed, shown to Janek:
- (code-review) GTK's frame callback runs every frame while a ring moves, though on a long ring the check changes only every few seconds; each frame costs a clock read and the cheap check. The re-measurement (2.1) decides: if the Librem 5 load is still high, following the frame clock only for rings whose step is under the refresh's 250 ms (rings under about 8 minutes) is the fix, and it would need a sixth review round (ask first). Measured: 3.5 % without bells, so not done.
- (reviewer) no test pins F24 (a dropped compare): a source test of an absent line would only restate the code.
- (code-review) a rung or one-off bead sits at its exact time, up to a step ahead of the floored fill: a pixel at most, and moving it would put a bell off its time.
- (code-review) `RingKey` compares the stepped fractions as floats: both come from the one `ring_steps`, so they are equal exactly when the steps are.

## /simplify (2026-11-13, four angles)
Applied: `RingKey::moving()` also needs a ring, so GTK drops its `has_ring` guard in the refresh (altitude; test: a running stopwatch does not move, failed first); GTK's `seen` is a plain `Cell` (efficiency); Android's two ring tests share `two_minutes_ringing_every` (reuse). Clean otherwise. Suites after: core 1553, GTK 180, Android 296; clippy both clean.

## Hand-over (step 6)
Artifacts (2026-11-13):
- Librem 5: `target/aarch64-unknown-linux-gnu/release/meditate` (00:54, GNOME 50 sysroot via a scratchpad copy of the repo's cross-build script, which now wants GNOME 51); copied 2026-11-13, md5 checked; rebuilt 06:14 with the tick fix (md5 2e51dac1…), copied and checked. Its earlier test binary (base b32d428) is there as `meditate`, the October one as `meditate.okt13`.
- FP5: debug APK `meditate-android/android/app/build/outputs/apk/debug/app-debug.apk` (00:55), installed.

Test steps:
1. Librem 5, measured by Claude over SSH with `top` (30 s each): a 1 h Timer countdown with a bell every 5 s, then one without bells. Before: about 98 % and 60 % of one core. Expected: far lower.
2. Librem 5 and laptop: a 1 minute countdown with a 5 s bell: the ring glides as before, beads fill when their bell rings, the counter counts.
3. Librem 5: a countdown with a 10 s preparation: the small ring at the start point fills smoothly.
4. Pause, resume, stop: the ring stops, moves on, and shows the saved length.
5. FP5: the same 1 minute run, and a 1 h run with a 5 s bell for a minute: looks as before (on a long ring the fill may move in single-pixel steps; say if it looks jerky).

Proposed commit message:

    core, gtk, android: the ring is built only when it changes

    The ring round the running time was rebuilt and drawn on every frame:
    on the Librem 5 a 1 hour countdown kept a core at 60 %, and at 98 %
    with a bell every 5 seconds. Its fill, its preparation ring and a
    riding bead now move in steps of about a pixel, rounded down, and a
    cheap check in core tells both apps when the ring or its counters
    changed; only then are they built and drawn: now about 3.5 % and 17 %.
    A repeating bell's rings are walked without collecting a list first.

    The Linux tick now waits for the session clock's next whole second:
    it counted from the start, so after a preparation or a Resume every
    bell rang up to a second late and its bead rode the fill first.

    Closes REFACTORING #274 and package P5.
