# Tasks: one-session-end

Base: 5f10ea9; suites on it: core 1540 + 18 green (run 2026-11-11); Android 301 and GTK 177 green on the same tree (one-dialog-frame's final runs, committed unchanged)

## Loop
- [x] 0 Skills loaded, again after every /compact: `/ponytail ultra`; `slint:slint` (no .slint change planned; ui.rs glue touched)
- [x] 1 Understand
- [x] 2 Propose (approved: 2026-11-11, with the ended time showing the saved length)
- [x] 3 Build (core 1543, GTK 177, Android 302 green; both clippy runs and the Android-target clippy clean)
- [x] 4 Review (our reviewer and `/code-review`), then `/simplify` after the last review (five rounds, /simplify done)
- [x] 5 Specs, docs, list
- [x] 6 Verify and hand over (Janek tested on laptop and phone 2026-11-11: all eight steps pass; step 5 bell and "Session saved" come together, as expected)
- [x] 7 Ship (deltas merged, folder archived, check-specs green)

## Review Focus
- A session restored after a crash (in-progress snapshot) -> unchanged: the snapshot holds the length, written from core's ended length as before; no scenario.
- Pause during preparation, then stop -> "Stopped during preparation" (0 s, paused or not).
- Auto-Finish with the Discard dialog open -> unchanged path (GTK closes it, Android closes it); covered by the existing `an_auto_finished_session_is_saved_with_undo` tests.
- A tick after the end (the 200 ms Box Breath tick, a late timer) -> core returns nothing once ended; the apps stop the tick on "ended".
- Overtime entered by the guided track's end vs. by the tick -> both through the same reach-end path; "No bell rings after the end bell".
- A suspend across the end -> existing "A countdown skipped by a suspend rings only the end bell" keeps passing.
- Box Breath stopwatch (no end) -> never "complete"; in `only_an_end_reached_by_itself_is_complete`.

## 1. Core: what the effects carry (#271, #270, #430 end half)
- [x] 1.1 `only_an_end_reached_by_itself_is_complete`: one "reached its end" effect with the planned length at overtime, Box Breath end and Auto-Finish; none on later ticks or hand ends. Files: `meditate-core/src/session/{mod,effect}.rs`. Expected: FAIL then PASS.
- [x] 1.2 One "ended" effect with the length and save-now (Auto-Finish) replaces EndSession, EndBoxBreath and AutoFinish; one private `end()` sets the phase; `Stopped` carries the length (`final_duration_secs` reads it). Proof: existing end tests rewritten to the new effect (finish_overtime_returns_target_secs, add_overtime_and_finish_returns_running_plus_overtime, the Auto-Finish and Box Breath end tests).
- [x] 1.3 Delete UpdateDisplay, EndPrep, UpdateOvertimeLabel, EnterOvertime, `TickOutcome`, `tick_summary`, the free `ui_state`; add `Session::overtime(now)` for the Add label; one `secs_left(target, elapsed)` for the four inline "rounded up" sites. Proof: display/overtime tests rewritten to `display_secs` / `overtime`.
- [x] 1.5 `an_ended_session_shows_its_length` (core: display_secs once ended is the saved length; Android: `AppState::Finished` keeps the length for the hero). Expected: FAIL then PASS.
- [x] 1.4 `a_stop_during_preparation_records_no_time`. Expected: FAIL then PASS.

## 2. Core: pause in one place (#273, #277)
- [x] 2.1 `a_session_stopped_while_paused_is_ended`. Expected: FAIL then PASS.
- [x] 2.2 Delete `SessionPhase::Paused` and the `is_paused` field; `Stopwatch: Copy`, `is_paused()` from the clock. Proof: pause tests stay green.

## 3. Core: the end skips due bells (#278)
- [x] 3.1 `a_guided_track_ending_early_rings_no_bell_after_the_end_bell`; `skip_due` moves into `reach_end`. Expected: FAIL then PASS.

## 4. Core: smaller ones (#275)
- [x] 4.1 Shape matched by value, Guided count-up as a guard, prep rounding once, the rng-seed doc, `toggle_action(&self)`, stale docs. Proof: suite green.

## 5. Linux: one tick, one reaction (#313)
- [x] 5.1 One `session_tick()` and one `react(effects)`: dispatch, "complete" -> notify, "ended" -> stop tick, session ended, save at once for Auto-Finish; then redraw the time, the buttons and the Add label from core. The guided track's end calls the same `react`. Proof: `save_joins_pause_when_preparation_ends` and the source tests in `application.rs`, `sound.rs`, `timer/imp.rs` rewritten to the new names; GTK suite green.

## 6. Android: read the end from core (#430 end half)
- [x] 6.1 `ended_duration_secs` and `completion_notice_secs` give way to core's ended length and "reached its end"; the Add label from `overtime(now)` in `refresh`. Proof: Android tests that used them rewritten; suite green.

## 7. Docs and lists
- [x] 7.1 ARCHITECTURE.md effects and session engine; REFACTORING.md P5 row and items; OPEN-ISSUES.md #277, #278, HH line. Proof: `build-aux/check-specs.sh` green; grep the docs for the removed effect names.

## Notes
- Pre-build questions (fresh agent): the time after an end (Janek: both apps show the saved length); #278 skips bells due by the planned length, as REFACTORING #278 says (my call; spec reworded).
- TDD: the five new scenarios' tests failed first at the right assert (Completed missing, ui_state Paused, prep 12 s, the 900 s bell rang, display 0; Android hero "00:00"). 1.2, 1.3, 2.2 and 4.1 are refactors: the existing tests were rewritten to the new effects and pull API and stayed green.
- `stop_during_prep_returns_prep_elapsed` deleted: it pinned the old behaviour; `a_stop_during_preparation_records_no_time` replaces it. `ui_state_idle_when_session_is_none` went with the free `ui_state`.
- REFACTORING #271 called the free `ui_state` test-only; GTK's `ui_state()` used it. Replaced by `map_or(UiState::Idle, CoreSession::ui_state)`.
- GTK's guided track end reacts at once inside the track's callback (rounds 1 and 2 deferred it to an idle; round 3 went back, see there).
- Android keeps `completion_notice_secs(effects, in_foreground)`: the foreground gate is the phone's; the decision is core's `Completed`.

### Review round 1 (spec reviewer + /code-review high)
Fixed:
- Reviewer 1: the crash-recovery heartbeat recorded preparation as meditation (GTK `elapsed_secs_for_mode`, Android heartbeat). Core `Session::saved_secs(now)` (0 in Prep, the length once ended) used by both; scenario "The app is killed during preparation"; test asserts failed first.
- Reviewer 2: Add right after a guided track's early end saved 899 of 900 s. Add saves at least the planned length; failed first, scenario extended.
- CR 2: GTK's track end ran `enter_overtime` in an idle, so a tick in between could ring a bell #278 skips. Core is called in the track callback; only the reaction is deferred.
- CR 3: an ended session still read as paused and `resume` moved its clock. `is_paused` is false once ended (test extended, failed first).
- CR 1: GTK restyled the end buttons on every tick (Box Breath: 5 per second). `refresh_end_buttons` redraws only when the buttons or the overtime second change.
- CR 5: `box_breath_stopwatch_never_emits_end` matched only one kind of end; now any end or "complete".
- CR 7: GTK's notice takes core's `planned_secs`.
- CR 8: `on_pause` and `on_resume` end in `react`, no hand redraw.
- CR 9: `reach_end` takes the seconds to skip to; no ZERO sentinel; `tick_prep` reuses the tick's elapsed.
- CR 10: Android test renamed `hero_label_finished_shows_the_saved_length` (07:00); `UiState::Done` doc.
Rejected:
- CR 4: the ring and counter after a stop in preparation look complete. Unreachable: no Save during preparation in either app, and Discard leaves the page.
- CR 6: three readers of the end effect (GTK `session_tick`, `react`, Android `on_auto_finish`): left for /simplify, which looks at exactly that.

### Review round 2 (fix reviewer + /code-review high)
Fixed:
- Both: GTK's deferred track-end reaction ran even when Save or Discard came first (end bell after the save, a second `on_save(true)`). The idle now reacts only while the session is the same one (start time) in the state the end left it (Overtime, or Done for an end).
- CR 2: `saved_secs` in overtime could be under the planned length after an early track end (crash recovery 899 of 900 s). The max moved into `saved_secs`; Add is `stop` again. Assert failed first.
- CR 3: `pause` on an ended session froze its clock and cut signals again. Guarded; assert failed first.
- CR 4: `session_tick` stops (and forgets its source) when the session is not live, as the old `_ => Break` did.
- CR 6/8, reviewer 2: `ended_secs` in the existing `impl Effect`; docs of `elapsed_secs_for_mode` and `Session::start`; `refresh_end_buttons` reads the overtime once.
Rejected:
- CR 5: three readers of the end effect: for /simplify (as round 1).
- CR 7: `AppState::remaining` is test-only: REFACTORING #429 deletes it in P5-b.
- CR 9: `toggle_action`'s `Idle` arm: a Session never reports Idle, the arm only makes the match complete; the apps map no session to Start.

### Review round 3 (fix reviewer + /code-review high)
Fixed:
- Both: the deferred, guarded track-end reaction dropped the end bell and the notice when a pause or tap came between, and leaked one with Auto-Finish then Save. The reaction runs at once in the track's callback again; the deferral was caution only (the old code already dropped the track there through StopGuidedAudio). The guard is gone.
- Both: a track end while paused moved the session into overtime under the pause. Core's `enter_overtime` now does nothing while paused; the tick ends it after Resume. Assert failed first.
- CR 1: round 2's "never less than the planned length" credited minutes never meditated when a track ends well before its probed length (VBR). Reverted: overtime saves the time so far, as before this change; round 1's MINOR (899 of 900 s after a sub-second early end) is rejected for that reason.
- CR 8: after a hand end (a failed save keeps the page) the ring was full next to 07:00. The ring shows the saved length; full only when it reached the end. Assert failed first.
- CR 6/7: GTK `elapsed_secs_for_mode` renamed `saved_secs`; `drawn_buttons` cleared with the controls in `reset_mode`.
Rejected:
- CR 5: the readers of the end effect: for /simplify (as rounds 1 and 2).

### Review round 4 (fix reviewer + /code-review high)
Fixed:
- CR 1: a track end while paused was dropped (Android reads it once; Resume would replay the finished track until the probed length). Core keeps it (`track_ended`) and the first tick after Resume ends the countdown; Android's own Running guard went, so core decides. Core and Android asserts failed first.
- CR 3: after an early track end and Add (saved length under the planned one) the ring and the end bell's count said the end wasn't reached. Core records that it reached its end (`reached_end`) and draws from that. Assert failed first.
Rejected or moved:
- Both: Add saves less than Save after a guided track ends well before its probed length. As before this change; a decision for Janek: OPEN-ISSUES #518.
- CR 4: the phone's ended page has no ring (Finished keeps only the length): P5-b, noted under REFACTORING #429.
- CR 6: Android's `toggle` pauses in overtime: #437, P5-b.
- CR 5: readers of the end effect: /simplify.

### Review round 5 (fix reviewer + /code-review high): minor only, fixed, no further round
- Fix reviewer: no findings.
- CR 1: a track end arriving after the planned length (a suspend) skipped bells only to the planned length. `enter_overtime(now)` skips to the later of now and the planned length, like the tick. Test first written after the fix (slip); proven by a mutation back to the old skip, which failed it.
- CR 3: `Effect::completed_secs` next to `ended_secs`, used by GTK `react` and Android's notice.
- CR 4: "reached its end" lives in `SessionPhase::Stopped { reached_end }` (Overtime implies it); the field is gone.
- CR 6: one private `meditated(now)` for `saved_secs` and the ring.
- CR 7: `only_an_end_reached_by_itself_is_complete` asserts the play key does nothing in overtime (core, so CI runs it).
Rejected:
- CR 5: freeze the clock in `end()` instead of the two checks: `is_paused` reads the phase once; a Stopwatch has no stopped state and adding one is more code than the check.
- CR 2: the save-now scan: left for /simplify.

### /simplify (after round 5)
Applied:
- The track end goes through the tick: `enter_overtime` sets `track_ended` and calls `tick(now)`; one end path, `reach_end(elapsed)` computes its own skip (reuse, simplification, altitude).
- `Effect::saves_now` next to `ended_secs`; GTK `react` and Android `on_auto_finish` use it, no hand matching.
- GTK: `session_final_duration_secs` gone, `session_ended` writes `saved_secs()` and no longer redraws the buttons (`react` does); the extra `drawn_buttons` reset in `reset_mode` gone (`set_running_controls` resets it).
- `media_status` reads `overtime(now)`.
Skipped:
- Effects without numbers (`Ended { save_now }`, plain `Completed`), the apps reading the length from the session: Android drops the session at the end, so it needs P5-b's state (#429) first.
- `session_tick` deciding Continue/Break from state with `react`'s `cancel_tick` as the only stop: rests on glib-rs removing the dispatching source cleanly, unverified; the current handshake is known safe.
- Android reads the saved length from the effect and keeps it in `Finished(u64)`: P5-b reshapes that state.

## Hand-over (step 6)
Checks: core 1543 + 18, GTK 177, Android 303 green; both clippy runs from AGENTS.md and the aarch64-linux-android clippy clean; check-specs green.
Artifacts: `meditate-gtk/builddir/src/meditate` (2026-11-11 17:46), `meditate-android/android/app/build/outputs/apk/debug/app-debug.apk` (17:47, installed on the FP5). Laptop test app: XDG_DATA_HOME=~/.cache/meditate-p5a-test, log ~/.cache/meditate-p5a-test.log.
Lines: 59,489 before, 59,161 after: -328; since the refactors began -3,568.

Test steps, laptop and phone:
1. Timer 1 min, switch to another app: "Meditation complete" with 01:00 comes once; back in the app the time stays at 01:00 and Add counts "+ 00:05 und speichern" upward. Save stores 1 min.
2. Timer 1 min with a bell every 30 s: no bell rings after the end bell; in overtime the next bells still ring.
3. Timer with 10 s preparation: [Pause] becomes [Pause][Save] when preparation ends.
4. Pause after 20 s, Save: 20 s are stored.
5. Auto-Finish on, Timer 1 min: the end bell rings to its end, "Session saved" with Undo.
6. Box Breath countdown (shortest): at its end the page shows Save and the time shows the length (the phone used to show 00:00).
7. Guided file: the track's end rings the end bell once and goes to overtime; Space (laptop) / the media control's play key (phone) does nothing in overtime.
8. Guided file: pause shortly before the track's end, wait, Resume: overtime and the end bell within a second (phone: the track does not start again).

Proposed commit message:
    core, gtk, android: one session end

    Core told the apps about a session through effects that partly
    repeated its own state and said "ended" three ways; each app
    folded them back by hand and they drifted. Core now emits only
    what an app must do (bells, signals, the track, "complete", the
    end) and the apps redraw from it after every call. A pause lives
    only in the clock.

    On the way: a session stopped while paused counts as ended; a
    guided track ending early no longer lets a due bell ring after
    the end bell; preparation is never recorded as meditation, also
    after a crash; an ended session shows the length it saves.

    Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
    Claude-Session: https://claude.ai/code/session_012WGZUPYHL7TurMz6YSFVJs
