# Tasks: one-android-session

Base: b5afdd7; suites on it: core 1543, Android 303, GTK 177 green (run 2026-11-11)

## Loop
- [x] 0 Skills loaded, again after every /compact: `/ponytail ultra`; `slint:slint` (no .slint change planned; ui.rs glue touched)
- [x] 1 Understand (said back and confirmed 2026-11-11; no question for Janek: Android's Start is already disabled at 0)
- [x] 2 Propose (approved 2026-11-11; deleted-file rule ok, no time on two-device edge cases)
- [x] 3 Build (core 1546 + 18, Android 295, GTK 177 green; both clippy runs and the Android-target clippy clean)
- [x] 4 Review (three rounds, the third minor-only; /simplify done)
- [x] 5 Specs, docs, list (check-specs green; ARCHITECTURE, REFACTORING P5 row and the five sections, #459 and #312 notes from /simplify; OPEN-ISSUES #437 and #407 removed, #407 fixed by Started)
- [x] 6 Verify and hand over (installed 2026-11-12; Janek tested all nine steps; a call on a silenced phone does not pause, as Android takes no focus without a ringtone)
- [x] 7 Ship

## Review Focus
- A session ended, then the app killed before Save -> the held snapshot (written at the end from the session's facts) recovers it, as today; no new scenario.
- A save that fails -> the page stays with Save and Discard; a second Save must still find the ended session (today `pending_done` is put back; now the state is untouched until a save succeeds). Covered by a test in 2.3.
- Auto-Finish -> `on_auto_finish` still saves once with Undo; the ended session stays in the state until that save dismisses it.
- Stop during preparation -> 0 s, nothing saved (unchanged, P5-a test).
- A guided track end while paused -> core keeps the end for Resume (P5-a test `a_track_ending_during_a_pause_ends_the_session_after_resume` keeps passing).
- A focus loss while preparing -> can't happen: only Timer has preparation and only a guided track takes audio focus; the focus loss keeps its "only while running" check through `toggle_action` == Pause.
- Discard of an ended session -> back to Setup, nothing saved (unchanged).

## 1. Core
- [x] 1.1 `each_mode_builds_its_shape`: `SessionShape::new(mode, stopwatch_on, target_secs, pattern)`. Files: `meditate-core/src/session/settings.rs`. Expected: FAIL then PASS. Then GTK `on_start` and Android Start use it.
- [x] 1.2 `a_session_takes_its_label_only_when_the_switch_is_on`: `labels::session_label(db, mode)`; `preset_config::session_summary`, GTK `setup_selected_label_id` and Android's four sites use it. Expected: FAIL then PASS.
- [x] 1.3 `only_guided_carries_a_guided_file`: `SessionMode::guided_file(uuid)`; GTK's two gates and Android's `snapshot_guided_file` go. Expected: FAIL then PASS.

## 2. Android state (#429, #437, #395)
- [x] 2.1 `an_ended_session_keeps_its_ring`: `AppState` keeps the ended session; `Finished(u64)` goes. Expected: FAIL then PASS.
- [x] 2.2 `a_pause_in_overtime_does_nothing`: `AppState::pause_or_resume(now)` through `toggle_action`; `toggle(shape, now)` goes; the headphones' key maps `toggle_action`. Expected: FAIL then PASS.
- [x] 2.3 `a_session_keeps_the_file_it_started_with`: Start fixes start time, mode, guided file, title, local file in the state; Save, heartbeat, hold and media control read them; the statics, `session_start_unix`, `pending_done` go. Expected: FAIL then PASS.
- [x] 2.4 Transitions `&mut self -> Vec<Effect>`; `Transition`, `prepend`, `remaining`, the forwarders go; tests start through `start_session`. Proof: the existing AppState tests, rewritten.

## 3. Android shell (#394, #430)
- [x] 3.1 One `apply` in `ui.rs` replaces the seven blocks (action tap, end taps, guided end, focus loss, tick, both dismisses); the pause edge calls `guided_follows_pause` and `wake_follows_pause` once. Proof: suite and clippy green; device test.
- [x] 3.2 Setup time through core `idle_hero_label`. Proof: existing hero tests.

## 4. Docs and list
- [x] 4.1 ARCHITECTURE.md, REFACTORING.md (P5 row, the five items), OPEN-ISSUES.md (#437); check-specs.

## Builder questions (fresh agent, 2026-11-11), answered
- Linux and "keeps what it started with" -> scoped to the phone; Linux's half is #312 (P14). "Only Guided carries a file" stays for both (my call).
- The ended page is only up after a failed save (Save ends and saves in one tap) -> the scenario says so; host test only, no device step (my call).
- `SessionShape::new(Guided, count-up, 0)` -> `None`, like a countdown (my call).
- File deleted between Start and Save -> keeps the Start uuid and title, as the snapshot does; nothing joins on it (my call).
- Label "deleted gives none" -> `resolve_label_for_mode` finds by uuid, no fallback after a delete (checked).

## Build notes
- 1.1 to 1.3: each test failed first (stub), then passed.
- TDD slip (2.1 to 2.3): the new `AppState` (task 2.4) was written before these three tests, so they passed at once. Each was proven by a break in the production code (play key pausing on NoOp; no ring once ended; `started()` empty once ended): all three failed, then the code was restored.
- 2.4: the AppState tests chain through a test-only `Run` (state + last effects) instead of the deleted `Transition`; removed with their subject: `remaining` (4), `is_running_page` (3), `toggle_from_finished_starts_a_fresh_countdown` (a Start comes only from Setup now), `snapshot_guided_file` (3, replaced by core `only_guided_carries_a_guided_file`).
- 3.1: `apply` also posts the "complete" notice on every transition (it only comes from a tick or a track end) and redraws after the guided end in all cases. The source tests of the seven blocks now check `apply` instead. GTK `on_start` builds the shape before starting the track (a 0 s file now returns before the audio, as it did).
- Save reads the session's mode from the state, not the mode chip.

## Review round 1 (reviewer + /code-review high)
Fixed:
- R1.1 (both, IMPORTANT) A Guided file whose length is 0 started nothing and said nothing (the shape check came before the guided gate). Now the no-shape branch shows "Couldn't start playback" for Guided; the dead `duration_secs == 0` check went. Test: `a_guided_file_that_will_not_play_starts_no_session` (source; failed first).
- R1.2 (both, IMPORTANT) `a_session_keeps_the_file_it_started_with` didn't check the saved row. `Started::record(secs, note, mode, label)` builds it in app.rs; `finalize_session` calls it; the test builds A's row (uuid, title, start, length), an Open-File pick's row (title, no uuid) and checks Save never reads the selection. Failed first (stub).
- R1.3 Stale comments: app.rs header, `configured_duration`, `on_state_changed`, `clear_session_in_progress_snapshot`, `end_buttons`, the guided-follow note.
- R1.4 GTK `on_start` clones the guided pick only for Guided, one match.
- R1.5 `SessionShape::new` saturates the Box Breath target instead of `as u32`.
- R1.6 `Started`'s Default/PartialEq/Eq only under `cfg(test)`.
Rejected:
- AUTO_FINISHED into AppState: same lifetime and reset as before the change (read once by Save, which Auto-Finish always calls); no path leaks it today.
Left for /simplify:
- One `ended()` accessor instead of the three-way match in Save.

## Review round 2 (fix reviewer + /code-review high)
Fixed:
- R2.1 (code-review) Android Start took a Guided session's length from the UI property, not the file that plays; now from the pick (`g.duration_secs`), as before the change.
- R2.2 (code-review) `stop_all_previews` ran after `guided::play`; a Manage Files preview still flagged would stop the session's own track (shared player). Moved before the track starts (the order was the same at base).
- R2.3 (code-review) The heartbeat writes only for a live session again (the base had that guard); an ended one holds its own snapshot.
- R2.4 (reviewer, IMPORTANT) `a_session_keeps_the_file_it_started_with` also checks that `finalize_session` counts `started.local_file`.
- R2.5 Stale test and code comments (dismiss, the empty `remaining` header, guided follow, the EOS passthrough note).
For Janek (behaviour beyond the proposal):
- Linux: a Guided file of 0 s still starts nothing silently (as at base); Android now says "Couldn't start playback". Same toast on Linux?
- Android: a widget-launched preset of 0 min used to start a 0 s countdown (instant end bell); now nothing happens and nothing is said. A 0-min preset can only be saved from a 0-min Setup.
Left for /simplify: one ended accessor / `ended_record` instead of Save's match and `record`'s mode parameter; `AppState::idle()` beside `Default`.

## Review round 3 (fix reviewer + /code-review high): minor only, so no further round
Fixed:
- R3.1 `a_session_keeps_the_file_it_started_with` builds A's row from `state.started()`, not the local copy.
- R3.2 The ring scenario says what its test proves (Save ends it, the page stays; the failed save that keeps it is `a_failed_save_keeps_the_session_and_says_so`).
- R3.3 Source checks for R2.1 (Guided length from the pick) and R2.2 (previews stopped before the track) in `a_guided_file_that_will_not_play_starts_no_session`.
- R3.4 The two new core tests sit after their module's imports.
Rejected:
- Guided's count-up flag from the UI switch instead of the saved setting: Timer and Box Breath already read it so; it differs only for one turn after a failed setting save.
- "The heartbeat guard is dead": the base had it, round 2 asked for it back; it costs one line and keeps an ended session's held snapshot safe if an end ever skips `end_session`.
- The two "For Janek" items (round 2) stay with Janek.

## /simplify (four angles), after round 3
Applied:
- `AppState::ended()` gives Save the ended session and its Start facts; `Started::record(session, note, label)` takes mode and length from the session (no mode parameter, no three-way match, no clone); `finalize_session(row, local_file)`.
- `Started.guided_file` is a `GuidedFileUuid`, converted once at Start.
- `AppState::dismiss` returns effects like every transition: `apply(&ui, &state, AppState::dismiss)`.
- `AppState::idle()` gone (Default).
- The snapshot is built under the writer's one DB lock (the builder takes `&Database`; the hold drops it before clearing); `end_session` gets `now` from `apply`. Test `snapshot_writers_take_the_db_lock_once` replaces `..._build_the_snapshot_before_locking_the_db`.
- `toggle_from_idle_starts_running` deleted (no toggle any more; the start tests cover it).
Skipped:
- Skip the ring redraw on the ended page when nothing changed: the page is rarely up, `put_rows` repaints nothing.
- Duplicate headphone assert between two tests: harmless.
- The test-only `Run` chain: kept on purpose (about 60 call sites stay short).
- AppState's one-line forwarders (`tick`, `stop`, ...): thin and named like the buttons; `end_tap` takes them as `fn` values.
- Core `Session::pause_or_resume`: it would only remove one `match` per app; GTK's `on_pause`/`on_resume` do GTK-only work.
- Loosen whitespace-exact source tests: the file's existing style.
Deferred (REFACTORING "Deferred refactors"): Start's enabled state from core's `SessionShape::new`; a core `Started` for both apps (with #312); a core helper for "which length counts for this mode".
- Suites after: core 1546 + 18, Android 294, GTK 177; both clippy runs and the Android-target clippy clean.

## Hand-over (step 6)
Checks (just run): core 1546 + 18, Android 294, GTK 177 green; clippy for GTK + core (all targets) and for core + Android lib (-D warnings) clean; Android-target clippy clean; check-specs green.
Artifacts: GTK `meditate-gtk/builddir/src/meditate` (2026-11-11 21:58), running with `XDG_DATA_HOME=~/.cache/meditate-p5b-test`, log `~/.cache/meditate-p5b-test.log`; debug APK `meditate-android/android/app/build/outputs/apk/debug/app-debug.apk` (22:00, log shows `Compiling meditate-android`), not installed yet (phone not connected). Gradle daemon stopped.
Lines (loc.py, non-test .rs/.slint/.kt): 59,161 -> 58,807 (-354); since the refactors began 62,729 -> 58,807 (-3,922).

Test steps (phone, unless named):
1. Timer 1 min: Pause and Resume by the button and by the headphones' key. In overtime the headphones' key and the play key in the media control do nothing. Save stores 01:00.
2. Guided library file: a call or another media app taking the audio pauses timer and track together; Resume; the track's end rings the end bell once and starts overtime. Save: the Log shows the file's name.
3. Guided Local Folder file to its end, Save: its play count goes up by one; Undo of an Auto-Finish save takes it back.
4. Auto-Finish, Timer 1 min: "Session saved" with Undo.
5. Box Breath, shortest length: Save at the end; another one: Discard mid-session saves nothing.
6. Running Timer, swipe the app away after more than a minute, open it again: the session is recovered in the Log.
7. Widget: a starred preset starts its session.
8. The media control shows the guided file's name as its title during a Guided session.
9. Laptop: Timer, Box Breath and Guided start as before; with the label switch on, a saved session carries the label.

Proposed commit message:
```
android, core, gtk: one Android session

Android kept its own state machine beside core's session: a
consuming API that ui.rs swapped in and out seven times, its own
pause rule, three statics for the running file, and its own copy
of the session shape and the label and guided rules. The state
now keeps the session until Save or Discard, as Linux does, with
what Start fixed; every transition goes through one apply. Core
builds the shape, the label rule and the guided-file rule for
both apps.

On the way: the phone's ended page shows the ring again; pause
follows core, so overtime can't be paused; Save records the file
the session started with, not the one selected later.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_012WGZUPYHL7TurMz6YSFVJs
```
