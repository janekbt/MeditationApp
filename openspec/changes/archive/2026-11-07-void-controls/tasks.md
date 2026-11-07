# Tasks: void-controls

Base: 5769a321303076c0974efcaf6a359aab3b175bff; suites on it: green (core 1464, Android lib 290; GTK 171 on the same commit 2026-11-06)

## Loop
- [x] 0 Skills loaded: `/ponytail ultra`; `slint:slint` (Android UI)
- [x] 1 Understand
- [x] 2 Propose (approved: 2026-11-07)
- [x] 3 Build (3.2 detection: code written, proof is the device test)
- [x] 4 Review (R1, R2 on the R1 fixes, R3 on the R2 fixes: clean), then `/simplify`
- [x] 5 Specs, docs, list (check-specs green; README features line; OPEN-ISSUES has no #17 entry)
- [x] 6 Verify and hand over (maintainer 2026-11-07: pulse on shake after the fix, all passed)
- [x] 7 Ship

## Review Focus
- Session started with the phone already face down -> "Turning, shaking and the vibration on the phone" (the first stable side is the baseline, no event)
- A shake ends with the phone turned over -> same MANUAL scenario; after a shake the detector ignores turns for 2 s, and the saved session offers no Pause anyway
- A gesture file written while no session is live, or a stale one -> goes through the offered-buttons check like a media tap: nothing
- Switch toggled mid-session -> "The sensor listens only with the switch on and a session live", and the pause wake lock follows it
- Paused in overtime -> the page offers Resume and Save, so shake = Save (as the page's Save button); no extra rule
- Box Breath ended (Save only) -> shake = Save, as the page
- Guided session -> same buttons, a paused guided track resumes with Resume (the media-tap path does that already)
- Second device -> the key syncs; a Linux device ignores it
- Another language -> the explanation's length at 360 px, MANUAL in German

## 1. Setting
- [x] 1.1 `VOID_CONTROLS_KEY` + `void_controls_from_db`, off until set. Files: meditate-core/src/settings_keys.rs. Proof: `void_controls_are_off_until_set`, Expected: FAIL then PASS
- [x] 1.2 Experimental group before About (heading, explanation, SwitchRow "Void-Controls"), saved with `write_global_setting`, loaded on open and in `reload_setup`; nine Android translations of "Experimental" and the explanation. Files: ui/main.slint, src/ui.rs, lang/*/LC_MESSAGES/meditate-android.po. Proof: `the_experimental_group_holds_the_void_controls`, Expected: FAIL then PASS; msgfmt --check per file

## 2. Gestures to buttons
- [x] 2.1 Codes `VOID_FACE_DOWN = 6`, `VOID_FACE_UP = 7`, `VOID_SHAKE = 8` and `void_tap(state, note_empty, now, code)`: the offered code from `media_buttons` (Pause, Resume, Save, or Add when the overtime is over 60 s, else Save). Files: src/app.rs. Proof: `each_void_gesture_stands_for_one_session_button`, Expected: FAIL then PASS
- [x] 2.2 The tick reads gesture codes from the same `session_tap` file, routes 6..=8 through `void_tap`, acts like the media tap, and on an accepted gesture calls `haptics::vibrate_waveform` once (one 60 ms full-strength segment). Files: src/ui.rs. Proof: `a_void_gesture_goes_through_the_session_buttons`, Expected: FAIL then PASS

## 3. Service
- [x] 3.1 `MeditateSessionService.voidControls(context, on)` (intent ACTION_VOID_CONTROLS, extra on), sent by `service::set_void_controls` after `service::start` and on every toggle while a session is live; the service registers an accelerometer listener (SENSOR_DELAY_GAME) only while started and on, unregisters on off, stop, task removal and destroy. Files: kotlin/MeditateSessionService.kt, src/service.rs, src/ui.rs. Proof: `void_controls_listen_only_during_a_session`, Expected: FAIL then PASS
- [x] 3.2 Detection in the service: a side counts once z stays beyond 8 m/s² (face up) or below -8 m/s² (face down) for 500 ms; the first side after registering is the baseline, a change writes 6 or 7 via `writeTap`; a shake is 3 readings with |a| over 25 m/s² within 1 s, writes 8, and turns are ignored for 2 s after it. Thresholds tuned on the FP5. Files: kotlin/MeditateSessionService.kt. Proof: MANUAL "Turning, shaking and the vibration on the phone"
- [x] 3.3 ACTION_PAUSE_SESSION keeps the session wake lock while Void-Controls is on; turning them off during a pause releases it, on during a pause takes it. Files: kotlin/MeditateSessionService.kt. Proof: `a_void_controls_pause_keeps_the_processor_awake`, Expected: FAIL then PASS; `a_paused_session_lets_the_phone_sleep` updated for the switch-off case

## 4. Docs
- [x] 4.1 README features list: Void-Controls (Android, experimental). Proof: grep

## Notes
- Pre-build questions (fresh agent), answered (my call):
  a change of the switch reaches a live session's service on a local toggle and after a failed save (`reload_setup`); a change synced in from another device takes effect at the next session (corrected after review R1, see below);
  the switch is only sent while a session is live, and ACTION_VOID_CONTROLS gets the `!started -> stopSelf()` guard, so no stray service;
  the service keeps its own `paused` flag (set by PAUSE, cleared by RESUME/START/STOP) for 3.3;
  the confirmation pulse is `vibrate_waveform(app, &[(1.0, 60)])` without an explicit cancel;
  "over one minute" is overtime > 60 s, from `media_status(now)`'s overtime line;
  gestures act with the app in front too, no exception.
- GTK unchanged on purpose: no orientation or shake sensor on a laptop (proposal).
- Source-text tests for Kotlin and Slint follow the existing ones (`the_headphone_button_toggles_the_pause`, `the_bluetooth_group_holds_the_lead_in`): that code cannot run on the host.
- Build: every new test failed first (stub or missing code) and passes now. 2.2 uses an explicit `if taken.is_some()` instead of `Option::inspect` (MSRV 1.75). The old tick test's source window grew from 900 to 1400 characters for the longer branch.
- 3.2 is code only so far: `VoidDetector` in the service. Thresholds are first guesses until the FP5 test.
- Suites after the build: core 1465, Android lib 295, CI clippy and GTK clippy clean, Android-target clippy clean, check-specs green, msgfmt clean (the header warnings were already there).
- Review R1 (fresh agent), findings checked against the code:
  1 IMPORTANT, fixed: every sample over the shake threshold counted as a peak, so one knock (3 samples at 50 Hz) saved the session. A peak now counts only on the rising edge; the MANUAL scenario gained "a single knock or a hard set-down does not save".
  2 MINOR, accepted: a new vibrate() replaces a running one, so the pulse cuts a bell's vibration that is still playing (shake during the end bell's pattern, face down during an interval bell). Rare and harmless; the false "keeps playing" assertion and note were removed. Shown to the maintainer.
  3 MINOR, wording fixed: sync never runs `reload_setup`, so a switch changed on another device reaches a live session only at its next start. The proposal promised "at once" for a local toggle only. Notes and test messages corrected.
  4 MINOR, fixed: `bells_as_alarm`'s doc comment had ended up above the new function.
- Review R2 (fresh agent, R1 fixes only), all MINOR, fixed:
  1 hysteresis: `above` is released only under 15 m/s² (SHAKE_RELEASE_MS2), so a knock wobbling around 25 m/s² is one peak;
  2 after a recognised shake `above` stays true, so the same hard stretch is no new peak;
  3 the stale "after a synced change" test comment now says "after a failed save";
  4 source-text assertions in `void_controls_listen_only_during_a_session` pin the rising edge, the release and the post-shake state (failed first, pass now).
- Review R3 (fresh agent, R2 fixes only): clean.
- /simplify (4 agents), applied:
  the tap dispatch (gesture or media code, pulse or not) moved from the tick into `app::session_tap`, now tested by behaviour in `a_void_gesture_goes_through_the_session_buttons`;
  the wake-lock rule is one `updateWakeLock()` (`started && (!paused || voidOn)`) used by PAUSE, RESUME and `setVoidControls`;
  the start calls `void_controls_follow()` instead of repeating its body; `voidOn = on` (the intent guard and the stop paths already cover `started`).
  Skipped: dropping the Rust `LIVE` flag (without it a toggle in Preferences with no session would create the service and its MediaSession just to stop it again);
  the wake-up accelerometer instead of a wake lock in a pause (a design change, not every phone has one; revisit if the battery cost shows on the FP5);
  reading the setting once in `reload_setup` and using `v` in the toggle (negligible; the read-back shows the stored value).
- Suites after /simplify: core 1465, Android lib 295, GTK 171, CI clippy, GTK clippy and Android-target clippy clean, check-specs green.

## Hand-over
APK (debug, built 2026-11-07 14:01): meditate-android/android/app/build/outputs/apk/debug/app-debug.apk, install with `adb install -r`.

Test steps on the FP5:
1. Preferences: "Experimental" sits right before About, the explanation shows in full in German, the switch "Void-Controls" is off. Turn it on, leave Preferences, open it again: still on.
2. Timer session, phone face up on the floor, screen off. Turn it face down: one short pulse, the session pauses. Wait a minute, turn it face up: one pulse, it runs on.
3. Lay the phone face down before pressing Start, then start: no pause.
4. Knock the phone once on the floor, and set it down hard: nothing happens.
5. Shake it: one pulse, the session is saved (Log shows it). Turning during the shake does not also pause.
6. Countdown into overtime: shake within the first minute saves the planned length; after more than a minute of overtime it saves with the overtime.
7. During preparation: shaking does nothing and does not vibrate; turning face down pauses.
8. Switch off: turning and shaking do nothing; a pause lets the phone sleep again.

Proposed commit message:

    android: Void-Controls, turn over to pause, shake to save

    An opt-in Experimental switch in Preferences, named after voidplayer,
    who asked for it. With it on, turning the phone face down pauses a
    session, face up resumes it and shaking saves it, each with a short
    pulse; a paused session keeps the processor awake so the turn back is
    heard. Android only: a laptop has no such sensors.

    Fixes #17
- Device test 1 (maintainer, 2026-11-07): shaking gave no pulse. Cause: Save emits StopActiveSignals, whose `haptics::cancel` cut the pulse sent just before the button. Fixed: the pulse now follows the button; `a_void_gesture_goes_through_the_session_buttons` asserts the order (failed first, passes now). APK rebuilt 14:31 and installed.
- Fix review R4 (fresh agent, device-test fix only): PASS. One MINOR, accepted and shown to the maintainer: if the save a shake triggers fails (database write error), the pulse still comes; the page keeps Save and Discard, a notice shows and the snapshot recovers the session, so nothing is lost. /simplify runs again after the last device-test fix.
- /simplify after R4 (4 agents): clean except the test's whitespace-bound needle, loosened. Android lib 295 green.
