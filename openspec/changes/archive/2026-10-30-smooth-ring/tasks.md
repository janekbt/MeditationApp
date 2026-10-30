# Tasks: smooth-ring

Base: b61dedcc7e11775ae390af74f181b6eb960fc8cd; suites on it: core 1438, android 284, gtk 166, all green (release checks the same evening)

## Loop
- [x] 0 Skills loaded, again after every /compact: `/ponytail ultra`; `slint:slint`
- [x] 1 Understand
- [x] 2 Propose (approved: 2026-10-30, "linux mit reinnehmen")
- [x] 3 Build
- [x] 4 Review, then `/simplify` after the last review
- [x] 5 Specs, docs, list (no doc names the ring refresh; nothing in OPEN-ISSUES)
- [x] 6 Verify and hand over (Janek tested on the FP5 and the laptop 2026-10-30: "go")
- [x] 7 Ship

## Review Focus
- A new session after the last one ended full: the ring may glide back from full for 200 ms -> acceptable, the page is just shown; MANUAL
- Preparation's value is -1 when there is none: the glide must not draw a negative arc -> clamp at 0; TEST checks the clamp
- Pause: the value stops changing, so the glide stops too -> MANUAL

## 1. Android
- [x] 1.1 `the_ring_glides_between_ticks`: BellRingView draws both arcs from animated copies of `filled` and `prep` (`animate ... { duration: 200ms; easing: linear; }`), the preparation arc clamped at 0. Files: meditate-android/src/app.rs. Proof: FAIL then PASS
- [x] 1.2 Build it in meditate-android/ui/main.slint. Proof: 1.1

## 2. Linux
- [x] 2.1 `the_ring_follows_every_frame`: `bell_ring` adds a tick callback on the drawing area that reads `timer_view.bell_ring().ring` and queues a draw. Files: meditate-gtk/src/window/imp.rs. Proof: FAIL then PASS
- [x] 2.2 Build it. Proof: 2.1

## Notes
- Pre-build check, answered (my calls): the animated copies sit on BellRingView's root, not inside the `if` Paths (a property in an `if` is recreated without a glide); the `if`s keep reading the raw `filled` / `prep`, so the small ring vanishes at the end of preparation instead of winding back; the clamp goes on the input, `max(0, root.prep)`; a jump to full (Overtime, Stop) becomes a 200 ms sweep, accepted. A late tick can still show a small pause: MANUAL on the FP5.
- Review 1: IMPORTANT held: the GTK tick callback ran every frame also while paused or ended, recomputing the ring 60 times a second. Fix: the callback stops (Break) on the first frame the ring did not change; the quarter-second refresh, which already compares the ring, starts it again (`follow`) when it moves. One writer path less: the refresh's ring branch is now what re-arms the frame clock. Test extended (Break on a still ring, refresh calls `follow`), FAIL then PASS. MINOR held: the moved doc comment is back on `the_session_buttons_keep_their_place`. MINOR (source-text tests): the house pattern for UI glue; motion is MANUAL.
- Review 2 (fix review): PASS. MINOR held: the test now also requires `ticking.set(false)` before the Break, the line that lets `follow` start the frame clock again.
- /simplify: altitude and Android clean. Applied: the 250 ms refresh no longer stores and draws the ring itself, it only starts the frame clock, which does both (one writer). Skipped: Box Breath's always-on tick callback instead of `ticking`/`follow` (it would keep the frame clock running while paused, which Review 1 just fixed); per-frame fill-only accessor and cached beads (bounded cost for typical sessions; a `ponytail:` comment names the limit); the brittle text slicing in the GTK test (house pattern).

## Hand-over
- Verified: android 285, gtk 167 green (core untouched, 1438 on the base); clippy CI form and GTK all-targets clean; check-specs green. APK (release type, debug key) meditate-android/android/app/build/outputs/apk/release/app-release.apk (2026-10-30 23:16), installed on the FP5; GTK meditate-gtk/builddir/src/meditate (23:12), started with a scratch data dir.
- Test steps (phone and laptop):
  1. Timer, 2 minutes, 10 s preparation: the small ring fills smoothly, then the big ring, as smoothly as Box Breath's dot.
  2. Pause: the ring stands still; Resume: it moves on smoothly.
  3. Let it run out: the ring sweeps to full, nothing jumps back.
- Commit message (proposed):
  timer: Let the session ring glide

  On the phone the ring jumped five times a second while Box Breath's
  dot glided; it now glides between the ticks the same way. On Linux
  it follows the frame clock while it moves instead of a quarter-second
  refresh, and rests while paused.
