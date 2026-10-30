# Tasks: session buttons

Base: 016e633; suites on it: core 1438 green, android 283 green, clippy clean

## Loop
- [x] 0 Skills loaded: `/ponytail ultra`; `slint:slint`
- [x] 1 Understand
- [x] 2 Propose (approved: 2026-10-30)
- [x] 3 Build (core 1439, gtk 166, android 283 green)
- [x] 4 Review, then `/simplify` after the last review
- [x] 5 Specs, docs, list (README still accurate)
- [x] 6 Verify and hand over (Janek: "passt alles", 2026-10-30)
- [x] 7 Ship (specs merged, archived)

## Review Focus
- Box Breath and the stopwatch while running -> same row rule, "The buttons follow the session".
- Guided while running: Save keeps the time so far -> same stop path as Pause then Save.
- A note typed in an earlier pause, then resumed -> "Save while running keeps the time so far".
- Laptop: Esc and Space while running -> unchanged (Esc pauses, Space never saves); existing tests.
- Laptop: the row doesn't jump when the second button appears on pause -> it is there while running now; "Nothing moves when the session pauses" tests.
- Media control compact view shows 3 actions -> Pause, Save, Discard fit.
- Headphones while running -> still Pause (media_tap picks Pause/Resume from the row).

## 1. Save next to Pause
- [x] 1.1 Update `the_end_buttons_follow_the_session` (running -> Pause + Save, prep -> Pause). Files: `meditate-core/src/session/mod.rs`. Expected: FAIL then PASS
- [x] 1.2 Test `save_while_running_keeps_the_time_so_far` (7 min of a 20 min countdown -> EndSession 420 s). Expected: likely PASS at once (stop() from Running already exists): note it
- [x] 1.3 end_buttons: `(SessionPhase::Running, false) => buttons(Pause, Some(Save), false)`. Run core, GTK and android suites
- [x] 1.4 Laptop: refresh the end row when preparation ends (refresh_end_buttons on EndPrep in timer/imp.rs ~2844); a GTK test pins it. Files: `meditate-gtk/src/timer/imp.rs`. Expected: FAIL then PASS
- [x] 1.5 Update `the_notification_offers_the_session_buttons` (running -> Pause, Save, Discard) and any android/GTK test that pins the running row. Files: `meditate-android/src/app.rs`

## 2. Discard like the laptop (phone)
- [x] 2.1 Update `destructive_buttons_are_red`: EndButtonView's Discard is filled (no outline) and WrapButton takes error-container / on-error-container when destructive. Expected: FAIL then PASS
- [x] 2.2 WrapButton and EndButtonView. Files: `meditate-android/ui/main.slint`

## 3. Bead outlines in the accent
- [x] 3.1 BellRingView: border-color root.accent for the beads and the start point. Files: `meditate-android/ui/main.slint`. Proof: MANUAL
- [x] 3.2 GTK bead stroke in the accent instead of white 0.6. Files: `meditate-gtk/src/window/imp.rs`. Proof: MANUAL

## Notes
- Pre-build answers: the row shift at the end of preparation is accepted (Janek); laptop Save stays suggested-action while running (my call); Discard uses MaterialPalette.schemes.dark.errorContainer / onErrorContainer, pressed state mixes toward that text colour (my call); found: GTK didn't refresh the row on EndPrep (task 1.4).
- 1.2 passed at once (stop() from Running existed, as planned); mutation (+1 s in stop) makes it fail.
- Review 1 (fixed): "Save while running" now proven through AppState in app.rs (pause, resume, Save: 7 min, Finished, note field not conditional); the core-only stop() test removed as a duplicate; the core row test also covers stopwatch and Box Breath while running; Discard test forbids any outline; stale "outline one" comment.
- Review 1, for Janek at hand-over: at the end of preparation Pause slides aside; a tap meant for Pause right then can hit Save, which at 0 s drops the session without a message (and a few seconds later saves a stub). Janek accepted the slide; this consequence is his call.
- /simplify: applied: dropped the new test's row assert (the row tests cover it). Skipped: refresh GTK's row on any ui_state change in the tick (optional, more churn for one transition); one shared accent source for the GTK bead (2 lines); WrapButton's never-dark outline arm (the dark-role test pins the switch).

## Hand-over
- Verified: core 1438, gtk 166, android 284 green; clippy (gtk+core all-targets, core+android -D warnings) clean; check-specs green; debug APK 21:31 installed on the FP5; GTK debug build 21:31 started with the scratch data dir.
- Test steps: phone and laptop: start a Timer, the row shows Pause and Save; Save saves the time so far. With preparation: Pause alone, then Save joins. Phone: Discard is a flat pill, red text on a faint red tint; the media control while running shows pause, check, X. Both: beads not rung yet and the start point are outlined in the ring track's faint accent.
- Commit message:
  timer: Save next to Pause, Discard like the laptop's, beads in the accent

  Ending a session early took Pause, then Save. While a session runs,
  Save now sits next to Pause in both apps and in the phone's media
  control, keeping the time so far; during preparation Pause stays
  alone. The laptop refreshes the row when preparation ends, which it
  didn't before. On the phone, Discard on the session screen is the
  laptop's calm red pill, red text on a faint red tint, instead of a
  red outline. Ring
  beads that have not rung yet, and the start point, are outlined in
  the faint accent of the ring's track in both apps.
- Device test 1 (Janek, screenshots): the filled dark red didn't match the laptop, which is calmer: a flat pill, red text on a ~15 % red tint (measured #1D0000 / ~#C30000). Now WrapButton's destructive look is schemes.light.error text on that red at 15 % (25 % pressed); the dark-roles test exempts that one line with its reason.
- Device test 2 (Janek): unrung beads in the full accent were too bright; now the track's colour (accent at 25 %) in both apps.
- Added after device test 2 (Janek: option 1, then "jo"): one counter for all bells. Tests first (one_off_bells_are_large_beads, the_counter_counts_every_bell replacing the_counters_keep_their_order, the_stopwatch_counts_without_a_total), all three failed at their asserts, then bell_ring sums every bell; the sort by earliest ring went with it. Shells unchanged: they already draw a list of counters, now of at most one.
- Review of the fixes (fixed): the end bell counts as rung only in overtime or for a stop that reached the end (an early Save that failed left the clock running, and the count rose ~20 min later); test added to the_counter_counts_every_bell (failed 5 vs 4 first); GTK doc comment says one counter.
- /simplify after the fix review: applied: one `track` colour in BellRingView, plain `+=` in bell_ring, the one-off bell's `heard` bound once, a stale proposal line. Skipped: Option<BellCounter> instead of a Vec of at most one (touches both shells for no behaviour); counting a starting/end bell muted by the Cues toggle as not there (muted repeating bells still count, so the count would disagree).
