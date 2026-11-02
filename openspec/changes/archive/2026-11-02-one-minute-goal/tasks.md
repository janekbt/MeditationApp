# Tasks: One-minute goal

Base: ae3854b3076454f63c64d22d663f304052341d34; suites on it: core 1438
passed, android --lib 286 passed, gtk 167 passed (the tree the #18
commit was tested on, unchanged since)

## Loop
- [x] 0 Skills loaded, again after every /compact: `/ponytail ultra`,
      `slint:slint`
- [x] 1 Understand
- [x] 2 Propose (approved: 2026-11-02)
- [x] 3 Build
- [x] 4 Review (one fresh agent, 3 minor; /simplify below)
- [x] 5 Specs, docs, list (check-specs passes; no doc names the limit; #19 is a GitHub issue, not in OPEN-ISSUES)
- [x] 6 Verify and hand over (Janek: laptop and FP5 passed, hold speed fine, 2026-11-02)
- [x] 7 Ship

## Review Focus
- Another device on 26.11.0 gets a 1 minute goal through sync -> its
  goal ring and heatmap use 1 (the reader keeps any positive number);
  its old Linux spin row shows 5 until touched and its old Android
  dialog refuses 1. Accepted: goes away with the update, no data lost.
- Goal 1 and the heatmap -> 1 min is level 3, 2 min and more level 4;
  the levels are shares of the goal, nothing special at 1.
- Typing 0 or a fraction -> Linux clamps to 1 and rounds (digits 0),
  Android's Set stays off below 1.

## Pre-build questions (fresh agent), answered (my calls)
- The hold speed (climb rate) also read the step -> stays 5 as a literal
  in GTK, so holding + still climbs fast toward long retreat goals.
- Page step 5 -> a literal in GTK next to it: page and climb are how
  the GTK spin row feels, Android has no stepper.

## Review findings (all MINOR)
- 1 GTK hold speed is capped at the page step (GTK raises the hold
  step only while it is below the page increment): holding + now
  reaches about 6 per tick, before 25. The comment claimed it stays
  fast -> comment reworded; the hold is in Janek's laptop test, his
  call if it is too slow (page 20 would bring it back, Page Up/Down
  then jumps 20).
- 2 Minutes round to the nearest minute, so 30 s reach a 1 minute goal
  -> not changed: the living Stats rule (round like the Log, the 19:30
  scenario) says so; asked Janek at hand-over.
- 3 The read-back in the test passes with any minimum, only the range
  assert catches the change -> kept: the scenario's THEN names all
  three; the apps' use of the bound rests on the MANUAL scenarios.

## /simplify (4 agents)
- Applied: `DAILY_GOAL_STEP` deleted, only GTK used it; GTK writes 1.0
  like its other spin rows (log, timer). Suites again: core 1439,
  android 286, gtk 167; clippy clean both.
- Skipped: debounce the GTK goal write (efficiency) -> new code on a
  save path, the per-change write predates this change.
- Skipped: a core `DAILY_GOAL_PAGE_STEP` (altitude) -> answered before
  the build, GTK-only feel; house spin rows write literals.
- Skipped: one local for the two 5.0 (simplification) -> page step and
  hold speed are different knobs that happen to match.
- Skipped: drop the test's read-back (simplification) -> same as
  review 3.
- Reuse: none.

## Tasks
- [x] 1 Core test `a_one_minute_goal_is_allowed_and_kept`: 1 lies in the
      goal range, written and read back as 1, `compute(60, 1)` is
      Reached; watch it fail on the range (failed: range assert)
- [x] 2 Core: `DAILY_GOAL_MIN = 1`, drop the
      wrong comment; GTK page step and climb rate 5.0; Slint preview default 1; test
      passes, whole suites, clippy (core 1439, android 286, gtk 167; clippy clean both)
- [x] 3 Builds for the two MANUAL scenarios (GTK debug, debug APK)

## Test steps (step 6)
Artifacts: `meditate-gtk/builddir/src/meditate`,
`meditate-android/android/app/build/outputs/apk/debug/app-debug.apk`.
1. Laptop: Preferences, Daily goal. Minus counts down by 1 and stops
   at 1; Stats ring reads against 1 min. Hold + for a few seconds: is
   the climb fast enough?
2. FP5: Preferences, Daily Goal. Hint "Minutes (1–1440)", 1 is
   accepted, 0 is refused; Stats ring reads against 1 min.

## Commit message
stats: Allow a daily goal of one minute

A 1 minute goal is a real goal for a beginner; the minimum was 5.
Fixes #19.
