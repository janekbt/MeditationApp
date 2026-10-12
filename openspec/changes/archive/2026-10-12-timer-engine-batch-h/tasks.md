# Tasks: Timer engine (batch H)

Base: dc7004ecfeedea1c346bb212957ffa1a51c316ee

## Loop
- [x] 1 Understand (all seven issues re-checked in the code: still present)
- [x] 2 Propose (approved: 2026-10-12)
- [x] 3 Build (#185 and the late bell added 2026-10-12 with the maintainer's go)
- [x] 4 Review (again, for #185 and the late bell)
- [x] 5 Specs, docs, list
- [x] 6 Verify and hand over (pause and refusal added 2026-10-12 after the maintainer's answers; back through 3-5)
- [x] 7 Ship

## Review Focus
- Pause during Box Breath, then a late tick -> paused time never counts (existing pause tests stay green; no new scenario needed).
- The user stops Box Breath early -> still saves the elapsed time (`stop()` unchanged; existing test).
- Box Breath without a set length -> never ends, cues unchanged (existing test).
- A time jump of days with a 1-minute bell -> the catch-up loop is bounded by jump / interval (at least 1 s per step); fine.
- Jittered interval bells -> each skipped step draws the RNG, so later times differ from before; only the order is deterministic, which the tests use.
- Suspend that crosses preparation AND the whole countdown -> lands in overtime on the next tick, end bell once (covered by the prep scenario).
- Android -> already ticks every 200 ms and dispatches start effects, so the first-inhale cue reaches it with no shell change.
- GTK pause/resume of Box Breath -> the 200 ms timer must stop on pause and restart on resume like the 1 Hz one.
- GTK frame callback -> must stop when the session ends, without finishing it a second time.

## 1. #39 Box Breath saves its set length
- [x] 1.1 Test `a_box_breath_session_ending_late_saves_its_set_length` (FAIL: saves 2400), then save `target_secs` in `tick_box_breath`.

## 2. #45 Countdown after preparation keeps its start
- [x] 2.1 Tests `a_late_tick_at_the_end_of_prep_keeps_the_countdown_on_time` and `a_slightly_late_prep_tick_loses_no_countdown_time` (FAIL), then start the Running clock at `now - (elapsed - prep)`.

## 3. #46 Interval bells ring once after a jump
- [x] 3.1 Test `an_interval_bell_rings_once_after_a_time_jump` in bells.rs (FAIL: next due 10:00), then advance `next_ring_secs` past `elapsed` in a loop.

## 4. #47 End bell first
- [x] 4.1 Test `an_interval_bell_due_at_the_end_gives_way_to_the_end_bell` (FAIL: bell at 10:01), then run `fire_due_bells` in the overtime transition and drop its effects.

## 5. #72, #113 Box Breath cues
- [x] 5.1 Test `box_breath_ends_without_cueing_another_cycle` (FAIL), then check the end before phase changes.
- [x] 5.2 Test `box_breath_cues_the_first_inhale_at_the_start` (FAIL), then add the In cue to the start effects for Box Breath; replace `first_box_breath_tick_seeds_phase_silently` and fix the comments that say the starting bell covers it.

## 6. #166 Linux Box Breath on a timer
- [x] 6.1 Source test `box_breath_ticks_on_a_timer_not_on_redraws` in `meditate-gtk/src/timer/mod.rs` (FAIL), then: Box Breath gets a 200 ms timeout that ticks the session, dispatches effects and finishes the session on `EndBoxBreath`; pause/resume stop and restart it; the frame callback in `window/imp.rs` only draws and stops when the session is no longer running.
- Device check: minimise the window during Box Breath; cues continue, and it ends on time.

## 7. #185 and the late starting bell
- [x] 7.1 Test `a_suspend_past_the_end_during_prep_rings_only_the_end_bell` (FAIL: starting bell on waking), then skip the starting bell in `tick_prep` when the running time already reached the target.
- [x] 7.2 Source test `a_running_session_blocks_suspend` (FAIL), then inhibit SUSPEND for every session in `acquire_screen_awake_lock`, plus IDLE when "Keep screen awake" is on.
- Device check: with "Keep screen awake" off, GNOME's menu shows Meditate preventing suspend during a session, and not after it ends.
- [x] 7.3 Test `a_suspend_past_the_end_during_prep_rings_no_interval_bell` (passed at once: pins the maintainer's device test).
- [x] 7.4 Source test `a_paused_session_lets_the_computer_suspend` (FAIL), then release the block in `on_pause`, take it back in `on_resume`.
- [x] 7.5 Source test `a_refused_suspend_block_tells_the_user` (FAIL), then a toast when the cookie is 0; template and nine translations.
- [x] 7.6 Source test `a_paused_session_lets_the_phone_sleep` (FAIL), then PAUSE/RESUME actions in the Kotlin service, `service::pause`/`resume`, and `wake_follows_pause` on the Pause button and on audio-focus loss.

## Notes
- No design.md: no schema, sync or dependency change.
- No cold read by a fresh agent: a bug batch with the fixes already spelled out per issue; my call.
- Two old tests pinned the old behaviour on purpose and were changed: `interval_rerolls_only_once_per_tick_even_when_far_past` (bells.rs) is replaced by `an_interval_bell_rings_once_after_a_time_jump`; `tick_past_prep_boundary_still_emits_end_prep_once` now expects 594 (prep ended at 130 s, not at the late 135 s tick).
- GTK: `tick_breathing` forgets the timer's source id before returning Break, so a later `cancel_tick` doesn't remove an ended source.
- After build: workspace tests (core 1312, GTK 112, Android 207) and both clippy runs clean; check-specs passes.
- Review (fresh agent): spec compliance PASS, quality PASS. Fixed: the #47 test now also asserts the 10:00 tick has the end bell and no interval bell; removed the dead `TimerView::finish_breath_session` wrapper; corrected four comments. Task 5.2 kept `first_box_breath_tick_seeds_phase_silently` with a new comment instead of replacing it (still true, still useful).
- Resolved (7.1, maintainer chose it): a suspend past the end during preparation rings only the end bell.
- Device test 1 (laptop, dev build): blocks suspend while running and not after; lid close still suspends (logind's default ignores the block); a wake past the end rang the end bell and, 3 s later, the next overtime interval bell. Maintainer: keep that; release the block while paused; show a message on refusal.
- Second review (#185, late bell): PASS. Fixed: the suspend test now pins SUSPEND as the unconditional base flag. Open for the maintainer: a paused session (and a forgotten overtime) keeps blocking suspend, like Android's wake lock; no message if GNOME refuses the inhibit (the gap is older).

## Hand-over (step 6)
- Verified: workspace tests (core 1313, GTK 113, Android 207), both clippy runs clean, check-specs passes.
- Artifacts: GTK dev build `meditate-gtk/builddir/src/meditate` (14:42, running with test data in the scratchpad `gtkH`); debug APK `meditate-android/android/app/build/outputs/apk/debug/app-debug.apk` (14:43, not installed: phone not connected).
- Test steps: in the chat message of 2026-10-12 (laptop 1-4, phone 5-6).
- Proposed commit message: `timer: right minutes and bells after suspend or a late tick (batch H)`, body listing #39 #45 #46 #47 #72 #113 #166 #185.
- Third review (pause, refusal): PASS. Fixed: the Polish message (it read as "couldn't keep it in standby"); ARCHITECTURE.md now says a pause releases the lock. Kept: a refused block toasts again on each resume, because the block really is still missing.
- Device test 2 (Flatpak, own data, no network): blocks suspend while running, not while paused, again after resume. Phone (debug APK): first inhale cued, no cue at the end; only the end bell at 2:00; wake lock and screen-on hold released on pause, back on resume. Maintainer approved 2026-10-12.
