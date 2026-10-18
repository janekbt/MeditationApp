# Tasks: Android platform (batch L)

Base: f654f3d449aa07cd3e80559f41955a21047cd08d; suites on it: core 1404
passed, android --lib 226 passed; GTK not run (batch touches no GTK code)

## Loop
- [x] 0 Skills loaded, again after every /compact: `/ponytail ultra`;
      `slint:slint` (ui.rs is touched)
- [x] 1 Understand
- [x] 2 Propose (approved: 2026-10-18)
- [x] 3 Build
- [x] 4 Review
- [x] 5 Specs, docs, list
- [x] 6 Verify and hand over (Janek: tested what he could, passes)
- [ ] 7 Ship

## Review Focus
- A widget tap on a cold start (app not running) -> "Tapping a starred
  preset in the widget" (MANUAL covers cold and warm)
- An old widget instance whose PendingIntent still targets the provider
  -> `onUpdate` re-renders every instance with the new target on the
  first update after install; until then a tap is ignored. Said in the
  hand-over, no scenario.
- A refused focus while a bell plays (bells use their own alarm stream)
  -> not affected: only the guided track asks for focus
- Earbuds disconnect while the session is paused -> the player and the
  receiver stay, but the focus-loss path ignores a paused session
  (existing path, no scenario). During a Manage Files preview -> left
  out (proposal)
- "zh-Hant-TW" (Traditional Chinese) -> no zh_TW or zh bundle, English
  as today; "sr-Latn" with no region -> plain sr
- A config change on an old phone (Android 8) -> the attributes all
  exist since API 13-17
- Startup stop while the widget is launching a preset -> the stop runs
  before the widget drop file is read, so the new session starts after

## 1. #50 Only the widget starts a preset
- [x] 1.1 New non-exported `MeditateWidgetLaunchReceiver` takes the
      launch branch of `MeditateWidgetProvider.onReceive` (drop file +
      foreground); the widget template PendingIntent targets it; the
      provider no longer handles the launch action. Files:
      `kotlin/MeditateWidgetLaunchReceiver.kt`,
      `kotlin/MeditateWidgetProvider.kt`, `AndroidManifest.xml`. Proof:
      `only_the_widget_can_start_a_preset` (manifest has the receiver
      with exported="false" and no intent-filter; the provider has no
      launch handling; the template targets the new class), Expected:
      FAIL then PASS

## 2. #51 Phone transfer
- [x] 2.1 `res/xml/data_extraction_rules.xml` excluding every domain
      under `<device-transfer>` and `<cloud-backup>`, spelled out: root,
      file, database, sharedpref, external, device_root, device_file,
      device_database, device_sharedpref (the db is under `file`);
      `android:dataExtractionRules` on `<application>`; correct the
      manifest's "No cloud/D2D backup" comment. Proof:
      `a_phone_transfer_takes_no_app_data` (each domain under both
      sections), Expected: FAIL then PASS

## 3. #57 Guided audio
- [x] 3.1 `startAudio` returns false when focus is refused, after
      releasing. Proof: `a_refused_audio_focus_starts_no_track`,
      Expected: FAIL then PASS
- [x] 3.2 A `ACTION_AUDIO_BECOMING_NOISY` receiver registered on the
      applicationContext in `startAudio`, calling `markFocusLoss`,
      unregistered in `releaseLocked` (field nulled, `runCatching`: it runs
      several times per track); the 2-argument `registerReceiver` (no
      androidx, the flags overload is API 33; a system broadcast needs no
      flag). Proof:
      `disconnected_headphones_pause_the_track`, Expected: FAIL then PASS
- [x] 3.3 Manage Files preview shows `invoke_playback_failed` when
      `guided::play` returns false. File: `src/ui.rs`. Proof:
      `a_guided_preview_that_fails_says_so`, Expected: FAIL then PASS

## 4. #79 Language with a script
- [x] 4.1 Lift the candidate list from `ui.rs` into
      `app::translation_candidates(tag)`: language plus the first
      region subtag (two letters or three digits), then the language.
      Proof: `a_language_with_a_script_picks_its_region` (zh-Hans-CN ->
      [zh_CN, zh]), Expected: FAIL then PASS;
      `a_language_without_a_script_picks_as_before` (pt-BR -> [pt_BR,
      pt], de -> [de]), passes at once by design (guards the old cases)

## 5. #149 Battery Saver
- [x] 5.1 Below API 33: `vibrate(effect, AudioAttributes` with
      `USAGE_ALARM`); fix the comment. File: `kotlin/MeditateHaptics.kt`.
      Proof: `vibrations_are_alarms_on_every_android`, reading the
      below-33 `else` branch only (USAGE_ALARM is already in the 33+
      branch), Expected: FAIL then PASS

## 6. #176 Configuration changes
- [x] 6.1 `configChanges` gains
      `fontScale|smallestScreenSize|keyboard|navigation|fontWeightAdjustment`
      (not locale/layoutDirection: Janek after review 1).
      Proof: `configuration_changes_keep_the_session`, one check per
      added value, Expected: FAIL then PASS
- [x] 6.2 In `android_main`, beside `audio::recover_alarm_volume` (before
      `build_ui`, so before the widget drop file is read):
      `service::stop_leftover` (new: `stopService`, which never creates
      the service, so a cold start flashes no media control),
      `guided::stop`, `audio::stop`. Files: `src/ui.rs`, `src/service.rs`,
      `kotlin/MeditateSessionService.kt`.
      Proof: `startup_stops_what_an_old_screen_left_running`, checking
      the calls sit after `set_android_app` and before
      `init_with_event_listener`, Expected:
      FAIL then PASS

## 7. List and build environment
- [x] 7.1 OPEN-ISSUES: remove #50 #51 #57 #79 #63 #149 #176, tick L
- [ ] 7.2 skipped (Janek: "Repro-Test weglassen"). Repro probe (`build-aux/repro-probe.sh`) on the finished
      change: two clean builds match. It builds the release APK twice
      (compared, never installed): tell Janek before it runs

## Notes
- Kotlin and the manifest can't run on the host, so their proofs read
  the source, as the existing `app.rs` guards do.
- All nine new tests failed first for the right reason (missing file,
  missing code, the empty stub); 236 passed after.
- `drop_files_the_tick_consumes_are_written_whole` now reads the drop
  file write in the new receiver.
- Review 1 (findings checked against the code):
  - 1 a language change no longer recreates the screen, so the app
    keeps the old language until the process dies: true (the bundle is
    picked once in build_ui). Janek: "Sprache wie bisher": `locale` and `layoutDirection` left out
    of configChanges, test checks they stay out (failed first).
  - 2 `fontWeightAdjustment` (Bold text) missing: true, added with its
    test check. `grammaticalGender` left out: a language-like change,
    follows the answer to 1.
  - 3 a failed preview left the previous row on Stop: true, fixed,
    scenario extended, test failed first.
  - 4 rejected: pausing inside the noisy receiver instead of on the
    next tick saves at most one 200 ms tick; the preview case is left
    out in the proposal.
- Review 2 (fixes only): 1 rejected: "a language change mid-session
  ends the session silently". It doesn't: open_database finalizes the
  snapshot at every android_main and build_ui shows "Recovered N min"
  with Undo (ui.rs RECOVERED_SESSION); only the stale notification
  goes, which is the point of #176.
- #63: already fixed in aac392c, proven by
  `a_paused_session_lets_the_phone_sleep`; leaves the list only.

## Hand-over
- Debug APK `meditate-android/android/app/build/outputs/apk/debug/app-debug.apk`
  (2026-10-18 22:30), installed on the FP5 22:33 with `adb install -r`.
- Repro probe: asked Janek whether it is needed; not run.
- Test steps: widget tap cold and warm; font size / Bold text during a
  session (keeps running, text changes at once); app language during a
  session (new language, "Recovered" notice, no notification left);
  earbuds disconnect in a guided session (pauses); Manage Files: preview
  playing, then Play on a missing file (notice, no row on Stop); guided
  start during a call (notice, no session).
