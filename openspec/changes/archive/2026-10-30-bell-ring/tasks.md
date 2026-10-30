# Tasks: bell-ring

Base: 90bec0ef244ef7a598ef607654305a81499b1127; suites on it: green (core 1425, android 275, gtk 165)

## Loop
- [x] 0 Skills loaded, again after every /compact: `/ponytail ultra`; `slint:slint`
- [x] 1 Understand (mockups: https://claude.ai/artifact/XeFXyw9tSgEq1dZRy5Hab5)
- [x] 2 Propose (approved: 2026-10-30)
- [ ] 3 Build
- [x] 4 Review, then `/simplify` after the last review
- [x] 5 Specs, docs, list
- [x] 6 Verify and hand over (waiting for the device test)
- [x] 7 Ship (device test passed 2026-10-30: "sieht toll aus und funktioniert ueberall")

## Review Focus
- Suspend or a long pause-free time jump: a bell rings once for several
  due times (`ActiveBell::tick`'s catch-up loop) -> the bead list records
  one ring, the counter counts real rings only; covered by
  `repeating_bells_are_small_beads` with a jump case.
- Fixed-from-end bell in a stopwatch: inert today -> no bead, no counter
  (`the_stopwatch_counts_without_a_total`).
- A bell whose first time lies past the end (every 40 min in 30): no bead,
  counter "0 of 0" would be noise -> no counter for a repeating bell with
  no bead in a countdown; covered in `repeating_bells_are_small_beads`.
- Guided track ending early: overtime path as today -> full ring
  (`overtime_shows_a_full_ring` covers the phase, not the track).
- Display mode count-up (Timer countdown showing elapsed): the ring is
  the same; nothing to test separately.
- Another language: counter text in nine languages, both apps; the
  existing whole-UI translation tests in both apps.
- Phone landscape (< 560 px high) and a short GTK window: the ring
  shrinks, the time is never cut -> MANUAL scenario.
- Box Breath: untouched; `only_a_known_length_gets_a_ring`.

## 1. Core
- [x] 1.1 Grid schedule. `BellSchedule::Interval` keeps the planned time
  of its next ring (`planned_secs`) instead of chaining from the last
  draw: on a fire, `planned_secs += interval_gap_secs(base, step, gaps)`,
  `gaps += 1`, then draw `next_ring_secs` around it. The draw is uniform
  in `[planned - jitter, planned + jitter]` (at least 1 s), and in a
  countdown, when `planned - jitter < target`, the top is
  `min(planned + jitter, target - 1)`. `build_active_bells` passes the
  target in. The catch-up loop after a jump stays (rings once, advances
  past `elapsed`). Files: `meditate-core/src/bells.rs`. Proof:
  `a_jitter_in_seconds_bounds_each_gap` (rewritten for the grid),
  `each_ring_keeps_to_its_own_planned_time`,
  `a_jitter_is_capped_at_half_the_time` (rewritten),
  `a_ring_that_can_come_before_the_end_does` (Expected: FAIL then PASS
  each); `a_step_grows_each_gap`, `a_step_survives_a_time_jump` stay
  green. A scheduling test that assumed chaining is rewritten to the grid,
  noted here by name.
- [x] 1.2 Ring history. Each interval bell records, per planned ring
  index, the elapsed seconds it really rang at (only fires that were
  dispatched; the silent pass at the end in `tick_running` records
  nothing). Files: `meditate-core/src/bells.rs`,
  `meditate-core/src/session/mod.rs`. Proof:
  `repeating_bells_are_small_beads` (suspend case).
- [x] 1.3 `Session::bell_ring(now) -> BellRing { ring: Option<Ring>, counters: Vec<Counter> }`,
  `Ring { filled: f64 /*0..=1*/, prep: Option<f64>, start_filled: bool, beads: Vec<Bead> }`,
  `Bead { at: f64 /*0..=1*/, large: bool, rung: bool }`,
  `Counter { rung: u32, total: Option<u32> }`. Ring for `TimerCountdown`
  and every `Guided`; none for Box Breath or `TimerStopwatch`.
  Interval beads: one per planned ring index whose earliest time
  (`planned - jitter`) is before the target, fixed at the start. A rung
  bead sits at its ring time, filled; the next unrung bead sits at
  `max(earliest, elapsed)`; any other unrung bead at its earliest time,
  hollow (a bead skipped by a jump stays hollow behind the fill).
  Fixed bells: large beads at their target. Total = beads of that bell,
  fixed at the start. Counter order fixed at the start: earliest first
  ring, ties by the bell list order; a repeating bell with no bead gets
  no counter in a countdown. Overtime: `filled = 1`, `total: None`, rung
  keeps counting. Files: `meditate-core/src/session/mod.rs`. Proof: the
  timer delta's core tests (Expected: FAIL then PASS each). Random rings
  in tests: set `next_ring_secs` directly after each fire, expected
  values by hand.

## 2. GTK
- [ ] 2.1 Running page of Timer/Guided: a `gtk::Overlay` with a
  `DrawingArea` (300 px, `set_content_width/height`, shrinks to
  `min(300, page width - 24)`) under the time label; cairo draws the track, the
  arc, the beads (radius 5 small, 7 large, 9 start; filled = accent,
  hollow = `window_fg` at 0.45 alpha) and the prep ring (radius 14
  around the start). Colours from the style context like the stats goal
  ring (`src/stats/imp.rs` goal_ring). A row of counter labels under
  the time (`dim-label`, tabular numbers; a small filled circle icon
  drawn the same way, 10 px). Redraw on the existing tick
  (`src/timer/imp.rs`). Files: `meditate-gtk/src/window/imp.rs`,
  `meditate-gtk/src/timer/imp.rs`. Proof: MANUAL scenario; plus
  `the_session_buttons_keep_their_place` still passes.
- [x] 2.2 `gettext("{rung} of {total}")`, nine `.po` via
  `build-aux/update-translations.sh`. Proof:
  `meditate-gtk/src/i18n.rs::every_language_translates_every_text`.

## 3. Android
- [ ] 3.1 `running-block`: hero inside a 300 px square (min of 300 and
  the column width; 200 px when `root.height < 560px`), Material
  `CircularProgressIndicator` for track + fill (as the stats goal ring),
  beads as a `[{at, large, rung}]` model from `src/app.rs`, placed by
  sin/cos in Slint on the indicator's stroke centre, prep ring
  as a second small indicator at the top. Counter row under the time.
  Box Breath branch untouched. Files: `meditate-android/ui/main.slint`,
  `meditate-android/src/app.rs`, `meditate-android/src/ui.rs`. Proof: the MANUAL scenario.
- [x] 3.2 `@tr("{} of {}", rung, total)`, nine `.po`. Proof:
  `ui_text_is_translated_whole`.

## 4. Docs
- [x] 4.1 README features list: the ring and the bell counter.

## Notes
- Grid (Janek, 2026-10-30): random bells ring at planned time ± jitter,
  independent of when the last one rang; a ring whose earliest time is
  before the end rings before the end. Today's app chains the gaps; the
  bells spec changes with this change. Known effect, accepted: with a
  jitter of half the time two rings can come seconds apart.
- Legend order fixed at the start (Janek).
- Pre-build agent questions, my call:
  - A real ring is a dispatched fire; the silent pass at the end records
    nothing. A bell muted by its signal mode still counts.
  - A bead skipped by a suspend stays hollow, its total unchanged: it did
    not ring (my call, replaces the earlier "1 of 4").
  - Preparation: beads hollow on the empty ring.
  - The start point is always drawn, filled from Running on.
  - Android geometry in Slint (`at` from Rust, x/y by sin/cos in Slint);
    proof is the MANUAL scenario.
  - Bead centres on the stroke centre (Material indicator: radius minus
    2 px); GTK draws the same 4 px stroke.
  - Bells ringing in overtime: no bead, the counter goes up.
- Built (my call): GTK ring is a fixed 300 px (the page scrolls in a
  short window, the narrowest window still fits it); Android
  min(320, column) or 220 px on short screens. Both: track inset 22 px,
  4 px stroke, beads 10/14 px, start 18 px, prep ring 40 px across.
  Hollow beads: GTK black with white 60 %, Android dark `background`
  with dark `outline` (the Material-roles guard allows no fixed colour
  outside the running section).
- Review 1 (fresh agent), each checked against the code:
  1. GTK stopwatch counter froze (area dropped, timeout stopped): fixed,
     the area is weak and optional, the timeout keys on the counters.
  2. A bell with no time hung the tick: fixed (`gap.max(1)`), test
     `a_bell_with_no_time_still_moves_on`, bells delta scenario.
  3. GTK ring doesn't shrink: rejected, a short window scrolls as Janek
     chose for the running page in steady-end-buttons; proposal wording
     corrected. Shown to Janek.
  4. Android counter translation unproven: `"{} of {}"` added to
     `ui_text_is_translated_whole`; it fails with the de entry removed.
  5. MODIFIED step requirement dropped two scenarios: copied in.
  6. Time with hours ran over the ring: both apps shrink it (GTK
     `in-ring-long` 3.5rem, Android 56/40 px).
  7. A one-off bell slept through showed filled: fixed with a `heard`
     flag, test `a_one_off_bell_slept_through_stays_hollow`.
  8. Separate 1 s timer lagged the bell: now every 250 ms (a deviation
     from "no extra timer": the GTK session tick has no hook the page can
     use without new plumbing; my call). The dot glyph is presentation
     only for screen readers.
  9. New Slint models each refresh: skipped, 1 Hz, a handful of rows.
- Review 2 (fixes only): PASS, four MINOR, all fixed: Android models
  updated in place (`put_rows`; refresh is 200 ms, not 1 Hz as item 9
  said); proposal timer line corrected (shown to Janek); CSS rule moved
  above the `.running-view-dark` comment; a bell with no time gets no
  beads or counter (`repeating_bells_are_small_beads`).
- /simplify (4 agents), applied: planned-time rules shared in bells.rs
  (`earliest_ring_secs`, `next_planned_secs`, `BellSchedule::rings_before`),
  one `bell_rng` helper, `Fixed { fired: Option<bool> }` instead of two
  bools, rung lookup walks `rang` once, `end` from `target_secs`, GTK
  computes the ring once per refresh and redraws only on a change, no
  refresh timer without ring or counter, `accent_rgb` reused, misplaced
  doc comments moved back. Skipped: dropping a zero-time bell at build
  (would change when it rings); the hours check in core (font size is
  the shells' layout); riding the GTK session tick (no hook without new
  plumbing); Material `CircularProgressIndicator` (light-scheme colour
  on the black page); a Slint arc component, a ring struct property and
  merging the test helpers (small gain).
- Mockup H showed "+03:00" as the overtime time; the app keeps the time
  frozen at the planned length, unchanged here.

## Hand-over
- Artifacts: `meditate-android/android/app/build/outputs/apk/debug/app-debug.apk`
  (18:31, installed on the FP5), GTK `meditate-gtk/builddir/src/meditate`
  (18:29, running with `XDG_DATA_HOME=~/.cache/meditate-datetest`).
- Suites: core 1438, android 275, gtk 165 green; clippy host, CI and
  Android target clean; check-specs green.
- Test steps (both apps):
  1. Timer 30 min, bells: every 5 min ±1 min, one 3 min from start, one
     2 min before the end. Start: ring round the time, hollow beads at
     4, 9, 14, 19, 24, 29 (small), 3 and 28 (large), "0 of 6" under it.
  2. Watch the fill move; at the first ring the bead rides the fill and
     stays filled where it rang, the counter goes to "1 of 6".
  3. Pause: ring stands still. Resume.
  4. Preparation 10 s: a small ring round the start point fills, then
     the start point fills.
  5. Session of 1 h or more: the time is smaller and stays inside the
     ring.
  6. Stopwatch with a repeating bell: no ring, counter "0", then "1".
  7. Guided track: ring without beads. Box Breath: unchanged.
  8. Phone in landscape, laptop window low: nothing cut, the laptop
     page scrolls.
- Proposed commit (show before committing):
  timer: Show the session and its bells as a ring
  (body: ring, beads, counters; random bells on a grid; Fixes #14)
- Device test 1 (FP5): the fill arc lay inside the track. Cause: a Slint
  Path fits its viewbox into its size less the stroke width
  (`third_party/i-slint-core/items/path.rs:176`). Fixed by sizing the
  viewbox radius against `width - stroke` for the fill and the prep ring.
  Layout only, no second review needed.
- Device test 2 (FP5): the counter dot sat at the top of its line; now centred on the text (inner Rectangle at half height). Layout only.
