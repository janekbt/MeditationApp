# Tasks: GTK timer and bells (batch I)

Base: f47fe9d9a007b68ce88908f4252c33d7488d4bf1; suites on it: green (`cargo test --workspace`, 1693 passed)

## Loop
- [x] 1 Understand (all seven issues re-checked in the code: #130 already fixed in batch H with `a_running_session_blocks_suspend`; the other six still present, line numbers moved)
- [x] 2 Propose (approved: 2026-10-13)
- [x] 3 Build
- [x] 4 Review (three fresh reviews: the change, the two review fixes, group 6)
- [x] 5 Specs, docs, list (check-specs green; OPEN-ISSUES: batch I removed and ticked; no other doc names the changed code)
- [x] 6 Verify and hand over (laptop and Librem 5 passed, 2026-10-13)
- [x] 7 Ship

## Review Focus
- Esc during preparation -> pauses, as before (toggle decision maps Preparing to Pause; covered by the Esc test).
- Esc on the Done page -> nothing (toggle decision is NoOp there; covered by the Esc test).
- Esc in stopwatch mode -> no overtime exists, so it always pauses (Running).
- The play/pause button itself -> already goes through the toggle decision; the new guard in `on_pause` changes nothing for it.
- A laptop without a vibration motor -> every bell gives no handle, so the slot is never touched; Pause still clears it (nothing to clear).
- Two vibration bells at once -> the second replaces the first, as before (replace scenario).
- A one-part pattern replaced -> setting its cancel flag is harmless; nothing more was scheduled.
- Pause, Stop, Discard during a vibration -> still drop the handle with its cancel (MANUAL scenario; the stop path sends a DBus call, which can't run in a unit test).
- Box Breath phase volume rows -> their bell doesn't depend on the mode, but they go through the same captured target; nothing changes for them.
- Drag, switch mode, drag again within 0.4 s -> the first pending save is written by the reload on the mode switch before the second drag starts its own (Starting Bell scenario).
- A sync reloading the rows while a save is pending -> the same flush runs first, so the user's level isn't overwritten by the older stored one; the preview for that drag step is skipped (my call).
- A page closed while its pop animation still runs at 0.4 s -> the stop button is alive but no longer mapped; no preview plays (task 5.1).
- Flushing an End Bell save after a mode switch preloads the old mode's end bell once; the next preload replaces it, and the end bell plays by its own sound id, so only a warm-up is wasted.
- App quits within 0.4 s of a drag -> the slider leaves the screen and saves at once (window scenario).
- Start within 0.4 s of a drag -> saved before the session reads the levels (Start scenario).
- A sub-page (sound or pattern chooser) pushed within 0.4 s of a drag -> the row leaves the screen and saves; the chooser opened during the push transition previews at the old level (only the preview; the saved level is right).
- Another language, an old database, a second device -> no text, schema or sync change; the volume write is the same setting or bell update as before.

## 1. #73 Duration row
- [x] 1.1 Source test `the_duration_row_always_shows_the_selected_modes_length` in `meditate-gtk/src/timer/mod.rs`: in `timer/imp.rs`, with whitespace removed, `duration_value_label` is used only inside `refresh_duration_value_label` and the template field (Expected: FAIL, four other writers today; three of all five are split over two lines, so a plain substring search would miss them). Then `refresh_streak`, `load_breathing_settings`, `set_countdown_target` and `set_breathing_duration_secs` call `refresh_duration_value_label()` instead of writing the label.

## 2. #126 Esc in overtime
- [x] 2.1 Source test `escape_pauses_only_where_the_pause_button_would` in `meditate-gtk/src/timer/mod.rs`: the guard `Session::toggle_action(self.ui_state())` != `ToggleAction::Pause` -> return sits in `on_pause` before `self.cancel_tick()` (checked by position: the test helper `body_of` runs past `pub fn` boundaries, so presence alone proves nothing) (Expected: FAIL). Then add that guard.
- Device check: Esc in overtime does nothing; Esc while running pauses.

## 3. #127 Sound-only bell and a running vibration
- [x] 3.1 Unit test `a_bell_without_vibration_keeps_the_running_one` in `meditate-gtk/src/vibration.rs`: a slot holding a playback, given no new playback, still holds it and its cancel flag is unset. Needs a stub `replace_playback(slot, new)` first (Expected: FAIL against the stub that copies today's `install_vibration_handle`). Test handles are built with `cancel_on_drop: false` and checked through a clone of their cancel flag, so no drop ever calls `stop()` (which needs the main context and DBus). Then the helper only replaces when a new playback is given; `install_vibration_handle` calls it; `stop_active_signals` drops the slot itself (`take()`), keeping its cancel.

## 4. #77 Replaced vibration
- [x] 4.1 Unit test `a_replaced_pattern_sends_no_more_parts` in `meditate-gtk/src/vibration.rs`: after `replace_playback` with a new playback, the old one's cancel flag is set, and the new one's is not (Expected: FAIL, `disarm` leaves the flag unset). Then `disarm` sets the flag. The preview slots in `vibrations.rs` and `vibration_editor.rs` already call `disarm`, so they get the fix too.
- Device check (Librem 5): a long pattern replaced by a short one plays only the short one.

## 5. #53, #132 Volume rows
- [x] 5.1 Source test `a_volume_is_saved_even_when_the_page_has_closed` in `meditate-gtk/src/volume_row.rs`: in the settle timeout the save runs before anything upgrades the stop button, and the closure no longer bails out on a dead `stop` (Expected: FAIL, `#[weak] stop` today). Then `install` takes a `target` closure called when the slider moves and a `save(target, volume)` called when it rests; the pending (source, target, volume) is kept so `flush()` can write it at once; the timeout saves first and only then plays the preview, and only if the stop button is still alive and mapped (the page's pop animation can leave it alive at 0.4 s). The test looks at the settle timeout closure only; the outer value-changed closure keeps its `#[weak] stop`. `bells.rs` passes a unit target.
- [x] 5.2 Source test `a_volume_change_keeps_the_mode_it_was_made_in` in `meditate-gtk/src/timer/mod.rs`: in `setup_volume_rows` the mode is read in the target closure, not in the save closure (Expected: FAIL). Then capture `(slot, mode)` in the target.
- [x] 5.3 Source test `a_pending_volume_is_saved_before_the_rows_reload` in `meditate-gtk/src/timer/mod.rs`: `refresh_volume_rows` flushes every row before its first `bell_volume::read` (checked by position within that function's own lines; `body_of` would run into `refresh_boxbreath_phase_state`) (Expected: FAIL). Then add the flush loop.
- Device check: drag Timer's End Bell to 20%, switch to Guided at once, back to Timer: Timer shows 20%, Guided its own level; on an interval bell page, drag and go back at once, reopen: the new level is kept.

## 6. Review follow-up: Start and closing (maintainer's go, 2026-10-13)
- [x] 6.1 Source test `start_saves_a_pending_volume_first` in `meditate-gtk/src/timer/mod.rs`: `on_start` calls `flush_volume_rows()` before `CoreSession::start` (FAIL with the line removed, PASS with it). Then the flush at Start.
- [x] 6.2 `a_volume_is_saved_even_when_the_page_has_closed` also asserts the slider flushes on unmap (FAIL, then PASS) and that a step reads its target before cancelling the waiting level. Then `connect_unmap` flushes; the bell page's own hidden-flush from the first review fix is replaced by it.

## Notes
- No design.md: no schema, sync or dependency change.
- Cold read by a fresh agent before approval (maintainer's wish): four test-precision points taken into tasks 1.1, 2.1, 3.1, 4.1, 5.1, 5.3; its open question (flush during a sync reload) settled as my call above.
- Source-reading tests for widget code: GTK needs a display, which CI doesn't have; same pattern as the existing tests in `timer/mod.rs`. The vibration slot and cancel flag run as real unit tests (no DBus touched: test handles never cancel on drop).
- #130 leaves OPEN-ISSUES without code: `timer/spec.md` already has its scenario and test.
- Build: task 1.1's first test also counted the `refresh_duration_value_label` calls; it now counts direct uses of the field (`.duration_value_label`), 5 on the base, 1 now. Task 5.2's lookup is `let target =` (the closure sits in a block that clones the view).
- Build (my calls): `install_vibration_handle` was a one-caller wrapper and is gone; the two pattern previews (`vibrations.rs`, `vibration_editor.rs`) now use `replace_playback` too, so `disarm` is private. `VolumeRow` got a hand-written `Debug` (it holds the flush closure).
- Build: the flush in `refresh_volume_rows` takes the database lock where the reload already takes it per row, so it adds no new nesting.
- After build: workspace tests 1700 passed (1693 on the base + 7 new), clippy on GTK and core clean.
- Review (fresh agent): spec compliance PASS, quality PASS, three MINOR findings.
  - Fixed: a late volume save after the interval bell page closed wrote the page's whole stale bell over a list change made meanwhile; the page now flushes the pending level when it hides (asserted in `a_volume_is_saved_even_when_the_page_has_closed`, FAIL then PASS).
  - Fixed: a new drag step cancelled the pending save before asking for its target, so a `None` target would drop it; the target is read first.
  - Asked the maintainer about Start and quitting within 0.4 s of a drag: "Ja, mit rein" -> group 6.
  - Second review (fresh agent, the two fixes): PASS. Taken: an order assertion for target-before-cancel. Set aside: the sound chooser opened within 0.4 s of a drag previews at the old level (preview only, data right).
  - Group 6 replaced the page-hide flush with an unmap flush and added the Start flush; a third fresh review covers group 6.
  - Third review (fresh agent, group 6): PASS. Fixed: `refresh_volume_rows`'s doc comment had slid onto `flush_volume_rows`. Taken: the window-close scenario is MANUAL, since the source test can't prove that closing the window unmaps the slider (the reviewer traced it: close_request -> destroy -> hide unmaps while the database is still open).
  - After group 6: workspace tests 1701 passed, clippy clean.

## Hand-over
- Devices: laptop (Flatpak, installed over the user's own) and Librem 5 (aarch64 binary copied over the installed Flatpak's). No database wipe: no schema change.
- Test steps:
  1. Laptop, Box Breath 5 min, Timer 10 min, Box Breath selected: open Stats, come back. Duration reads 0:05. Switch to Timer: 0:10.
  2. Laptop, Timer 1 min: let it pass into overtime, press Esc. Nothing changes; overtime keeps counting; Finish saves 1 min plus the overtime.
  3. Laptop, a running Timer session: Esc pauses, the button reads "Resume"; Esc again changes nothing.
  4. Laptop, Timer: drag the End Bell volume and switch to Guided at once. Back in Timer the new level shows; Guided keeps its own.
  5. Laptop: drag the Starting Bell volume and press Start at once. The starting bell rings at the new level.
  6. Laptop: open an interval bell, drag its volume and go back at once; reopen it: the new level. Drag again and close the window at once; start the app: the new level.
  7. Librem 5, Timer 2 min, one interval bell at 1 min set to Vibration with a long pattern, a second one at 1 min set to Sound: at 1:00 the sound plays and the vibration runs to its end.
  8. Librem 5, Vibrations list: play a long pattern's preview, then a short one's. Only the short one plays, nothing of the long one after it.
  9. Librem 5: pause during a long starting-bell vibration. The motor stops.
- Artifacts: laptop Flatpak installed 2026-10-13 10:03 (commit 8936dfc6, the app crate recompiled). Librem 5 binary `target/aarch64-unknown-linux-gnu/release/meditate` built 10:07 (the pinned 1.95.0 toolchain needed `rustup target add aarch64-unknown-linux-gnu` on this laptop first); not on the phone yet, the phone was unreachable.
- Proposed commit message:
  gtk: timer and bell fixes (batch I)

  The Duration row showed the Timer length in Box Breath after Stats.
  Esc in overtime paused a session that can't pause; "Pause" then
  ended it without the overtime. A sound-only bell cut a running
  vibration, and a replaced long pattern kept sending its old parts.
  A volume change was lost when its page closed within 0.4 s, or
  saved into the mode switched to; it is now saved for the bell and
  mode it was made in, also at Start and when the window closes.

## Device-test fixes
- 2026-10-13, laptop step 4: no preview played at all while dragging a volume. The settle timer asked the Stop button whether it was on screen, but that button is hidden until a preview plays, so it never was. The check is gone: it was never needed, since the row leaving the screen already cancels the waiting timer (flush on unmap). Proof: `meditate-gtk/src/volume_row.rs::a_volume_is_saved_even_when_the_page_has_closed` (asserts no on-screen check; FAIL with the old code, PASS after). Reviewed by a fresh agent: PASS. Its note, set aside: a Log/Stats refresh (sync pull, finished session) within 0.4 s of a drag saves the level but skips that preview; rare, nothing lost. Librem steps 7 to 9 passed before this fix; laptop steps 1 to 3 passed.
- 2026-10-13, laptop step 4 again: clicking several levels quickly crashed the app. gdb: SIGSEGV in GStreamer's player thread (g_object_unref in gst_play_main) as dropped preview players piled up; GTK 4.22.5, GStreamer 1.26.11. The preview code is unchanged since v26.10.1, so the release has it too; this batch only hid it while the preview was silent. Maintainer chose to fix it here. Each Volume row keeps its last preview player (by bell uuid) and, while that player is still in the preview slot and has no error, restarts it (seek to 0) instead of building a new one; only a new player gets a `playing` handler. A first version compared the player's "resource" property, which GtkMediaFile does not have (would abort on the second rest of a bundled bell); a fresh review caught it before the device test, along with: a failed player reused forever, two rows sharing one player's Stop button, one handler per rest. All four are fixed by the per-row design. Proof: `volume_row.rs::a_volume_is_saved_even_when_the_page_has_closed` (restart before new player, handler only for a new one) and `sound.rs::only_a_healthy_player_still_in_the_preview_slot_restarts`; both FAIL, then PASS. Source-reading tests: GtkMediaFile needs an initialised GTK. New bells requirement "A volume preview plays the bell at each resting level". Not covered: the sound chooser's per-row Play buttons still build a new player per click (slower, deliberate clicks); switching between different bells quickly still replaces players.
- Second fresh review of the per-row design: PASS on the scenarios; its test gaps (kept player, uuid check, Stop shown on restart, error check direction) now asserted. Set aside, written down: each Volume row keeps its last player alive while the row exists (at most six on the Timer page); alternating quickly between two rows on the same bell still builds a new player per rest, since each row only restarts its own.
- 2026-10-13, maintainer asked during the laptop test: on Android, tapping the volume bar stops the ringing bell at once instead of after the settle delay; GTK should do the same. Chosen: as on Android, letting go of the slider (mouse or finger) saves and plays at once; the keyboard keeps the 0.4 s settle. GtkRange claims its pointer gestures, so a capture-phase EventControllerLegacy watches ButtonRelease/TouchEnd and fires the waiting level. Save-and-play moved into one `preview` closure that both the release and the settle timer call. Proof: `volume_row.rs::letting_go_of_the_slider_plays_at_once` (FAIL, then PASS). New scenario "The user lets go of the slider"; the requirement now says "lets go of ... or it rests after a keyboard change".
- Fresh review of play-on-release: PASS on the scenarios; its test gap (the slice also covered the value-changed handler, and nothing checked the controller is attached) now closed and checked by removing the release block: FAIL, restored: PASS. Its other finding, holding the knob still for 0.4 s mid-drag playing once then and again on release, the maintainer asked to match Android: the legacy controller also tracks ButtonPress/TouchBegin, and while the slider is held the waiting level arms no timer (the timer is now optional in `pending`); release, TouchEnd and TouchCancel save and play it. New scenario "The slider is held still mid-drag"; proof in `letting_go_of_the_slider_plays_at_once` (FAIL, then PASS).
- Fresh review of the held-slider change: every way a hold ends still saves (release, TouchEnd, TouchCancel now; unmap, Start, mode refresh otherwise). Fixed: a release eaten above the scale (a swipe gesture) could leave the row "held" for good, so arrow keys and scroll would arm no timer and save only at the next page switch; a key press or scroll on the scale and the row leaving the screen now let go. Tests now pin each arm (press sets, release/TouchCancel clears, KeyPress/Scroll clears, unmap clears, `(!pressed.get())` gates the timer). Set aside: the hold is one flag, so a second button or finger lifted mid-drag counts as letting go (plays then; harmless). The maintainer confirmed the Android reference: a ringing bell only changes volume while the slider is held and restarts on release.
- Fresh review of that fix: on the phone a swipe-back gesture can take the touch so the scale never sees its end, and there are no keys to clear the hold, so a level could wait unsaved until the page closed. Replaced the "no timer while held" design: the timer is always armed, 0.4 s normally and 2 s (HELD_FALLBACK) while held; the release plays at once as before. A stuck hold now costs at most 2 s. The KeyPress/Scroll reset is gone (not needed; it also cleared a real hold mid-drag). Holding still for 2 s or more mid-drag plays once then (Android: only on release); set aside as rare. Tests now pin the arms per event kind and the unmap `flush();`.
- Fresh review of the fallback design: PASS; every level saved once for its target. Test gap closed: each step must cancel the last timer (`id.remove()` before re-arming). Spec scenario now names the 2 s fallback. Set aside, written down: a Log/Stats refresh (sync pull, finished session) during a hold saves the level unplayed, so that release plays nothing (rare; nothing lost).
