# Tasks: light-system-bars

Base: 0a5a72a2dbf11f51fa97b92c95962023a1cec4b2; suites on it: green at ship time of 0a5a72a (core 1425, gtk 164, android 272), HEAD unchanged since

## Loop
- [x] 0 Skills loaded, again after every /compact: `/ponytail ultra`; `slint:slint`
- [x] 1 Understand
- [x] 2 Propose (approved: 2026-10-30)
- [x] 3 Build
- [x] 4 Review, then `/simplify` after the last review
- [x] 5 Specs, docs, list (no doc names the bars; OPEN-ISSUES has no entry)
- [x] 6 Verify and hand over (Janek tested on the FP5 2026-10-30: "Passt alles")
- [ ] 7 Ship

## Review Focus
- Android 8.0 to 10 (API 26 to 29), no WindowInsetsController -> the old light-bar view flags, both exist from API 26
- Theme switch while open (uiMode is in configChanges, no restart) -> scenario "The theme changes while Meditate is open"
- Start before the window has a decor view -> the call runs on the UI thread after the Slint window exists; a JNI failure is logged and the bars keep today's look
- Gesture navigation -> Android adapts the handle itself; unchanged
- Accent colour or system colours -> do not change light or dark; unchanged

## 1. Bar symbols follow the theme
- [x] 1.1 `MeditateScreen.matchSystemBars(activity, light)`: on the UI thread, applies what Slint decided; API 30+ via insetsController, below via the light-bar view flags. Files: meditate-android/kotlin/MeditateScreen.kt. Proof: MANUAL scenarios on the FP5
- [x] 1.2 `screen::match_system_bars(app)`, same pattern as `set_keep_awake`. Files: meditate-android/src/screen.rs. Proof: android suite and clippy for the Android target green
- [x] 1.4 Review fix: symbols light on the black session screen. Slint `out property <bool> light-bars` (light theme and not the session screen) with `changed` calling `system-bars-changed(bool)`; Kotlin `matchSystemBars(activity, light)` only applies it. Files: main.slint, ui.rs, screen.rs, MeditateScreen.kt, app.rs JNI row. Proof: MANUAL scenario "A session runs in the light theme"
- [x] 1.3 main.slint: `light-bars` with a `changed` handler that calls `system-bars-changed(bool)`; ui.rs calls `match_system_bars` at start and in that callback (since 1.4). Files: meditate-android/ui/main.slint, meditate-android/src/ui.rs. Proof: MANUAL scenario "The theme changes while Meditate is open"

## Notes
- No automated test: Android draws the system bars and the call runs only on a device; a test reading the source would only restate it.
- Base suites not re-run: HEAD is the commit whose full suites were green at ship time today.
- Build found the JNI table guard (`app::jni_tests::the_jni_table_lists_exactly_the_rust_calls`) failing for the new call, as it should; the row is added.
- Kotlin uses `@Suppress("DEPRECATION")` on the API 26-29 branch, as MeditateInsets.kt and MeditateHaptics.kt do; the old flags are the only way there.
- Review 1: CRITICAL held (session screen is #000 in both themes, dark symbols would vanish there) -> fixed as 1.4. MINOR held (requirement worded by theme) -> reworded by what lies behind the bars, scenario added.
- Fix review: three MINOR held. The symbols flipped at the start of the 300 ms slide, so `light-bars` now keys on the animated offset and flips halfway. Tasks 1.1 and 1.3 reworded to the current design. The screen.rs header and the call layout are updated.
- Fix review 2: rejected the MINOR rotation glimpse. Turning portrait to landscape during a light-theme session may show dark symbols for one or two frames, inside Android's own rotation animation. A fix needs an extra non-animated flag, which is not worth it; check on the FP5.
- /simplify: applied the shared `call_bool` in screen.rs (reuse + simplification both flagged the copied JNI call) and one `match_bars` closure in ui.rs. Skipped: the theme XML light-bar entries (altitude, optional). They would only cover the moment before the first frame, and values-v27 would be a new file for that. Efficiency: clean.

## Hand-over
- Verified: android suite 272/272, clippy CI form and Android target clean, check-specs green. APK meditate-android/android/app/build/outputs/apk/debug/app-debug.apk (2026-10-30 12:54, matchSystemBars in classes3.dex), installed on the FP5 with `adb install -r`.
- Test steps (FP5, three-button navigation):
  1. Light mode, open Meditate: back/home/recent and clock/battery dark.
  2. Start a session: on the black screen the symbols turn light halfway through the slide; stop and save: dark again on the timer page.
  3. Dark mode, open Meditate: symbols light as before.
  4. With Meditate open, switch dark/light in the quick settings: symbols follow each time.
  5. Optional: rotate during a light-mode session; at most a flicker inside the rotation.
- Commit message (proposed):
  android: Dark system bar symbols on light pages

  The status and navigation bar symbols stayed white in the light
  theme and vanished on the light page. Slint now says whether a
  light page sits behind the bars (not the black session screen),
  and Android sets the symbols to match, also when the theme
  changes while the app is open.

  Part of #15 (the Discard button waits for a screenshot)
