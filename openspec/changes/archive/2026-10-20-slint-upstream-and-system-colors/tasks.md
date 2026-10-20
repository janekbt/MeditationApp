# Tasks: Slint fixes as merged upstream, and the system colour scheme

Base: 72b3d7c (beta); suites on it: green (`cargo test -p meditate-core -p meditate-android --lib`: 236 + 1386 passed).

## Loop
- [x] 0 Skills loaded, again after every /compact: `/ponytail ultra`, `slint:slint` (Android UI and third_party/ touched)
- [x] 1 Understand
- [x] 2 Propose (approved: 2026-10-20)
- [x] 3 Build
- [x] 4 Review, then `/simplify` after the last review
- [x] 5 Specs, docs, list
- [x] 6 Verify and hand over (maintainer: all FP5 tests passed, 2026-10-20)
- [x] 7 Ship

## Review Focus
- The previous release receives `system` by sync: its `parse().unwrap_or(0)` shows Blue. That is the old code and can't get a test here. Covered by a reason: it's the documented fallback.
- A stored value out of range ("9") or garbage: shows Blue as today. Covered by steps in `an_old_pick_is_system_where_the_system_has_a_scheme` and `an_old_pick_below_android_14_keeps_its_colour`.
- Nothing is written at startup, whatever is stored: only a pick writes. Covered by `only_a_pick_writes_the_accent`.
- The system scheme is read once at startup. Light/dark switching uses the stored light and dark schemes, the same path as `schemes`. Covered by MANUAL "Picking System".
- System picked, then the phone is updated from 13 to 14: the circle appears and is ringed. This follows from the same function and needs no extra scenario.
- Label palette under System: derived from the system primary in light and dark. Covered by MANUAL "Picking System" (Log).
- F-Droid: all sources stay in the repo and nothing is fetched. The repro probe runs on the final tree.
- Old `N` versus new `+N`: on 14+ only the old form (and a missing key) means System. A read failure returns the default and writes nothing, as today. Garbage or an out-of-range number shows Blue below 14 and System on 14+. Covered by a step in `an_old_pick_is_system_where_the_system_has_a_scheme`.
- Compatibility with the previous release: `"+2".parse::<usize>()` is `Ok(2)`, which is exactly the previous release's parse. Asserted in `a_pick_made_now_is_kept`.
- A pick made on a device still on the previous release shows System on 14+. That is the known corner; covered by `a_synced_old_pick_reads_like_an_old_pick`.
- JNI on a phone whose resources lack a name: `getIdentifier` returns 0, which counts as a missing role. Covered by `a_missing_role_means_no_system_scheme`.

## 1. The three merged PRs (third_party/, own commit)
- [x] 1.1 Rebuild `third_party/i-slint-core` and `third_party/i-slint-backend-android-activity` as registry 1.18.1 plus the merged commits 5b528c1 (#14205), d5a600a (#14206) and 7cb5f71 (#14208), adapted to 1.18.1 where the context differs. Proof: the same added-line check as in step 1 reports 0 missing lines for all three; `diff -r` against the registry shows only those three. `cargo test -p meditate-android --lib`, CI clippy and Android target clippy are clean.
- [x] 1.2 Debug APK; text-input MANUAL scenarios on the FP5: the new three plus the earlier ones as a regression check. The maintainer confirms.

## 2. System colours through Meditate's own JNI (app code, own commit)
- [x] 2.1 Test first, against stubs, in theme.rs: the eleven tests named in the preferences delta, with expected values written by hand. Run them and see them fail. Then the least code to pass:
  - `Accent`: System or one of the six.
  - The stored value parsed together with "system available": the picked accent and the ringed circle.
  - The circle order, and the value to store for each circle.
  - The role table: the draft's 47 resource names copied exactly, light and dark. Fixed and tonal roles have no suffix; `surfaceTint` is the system primary.
  - The scheme built from a lookup `Fn(&str) -> Option<Color>` plus the shipped fallback: `None` when any role is missing; shadow and scrim from the fallback.
  Proof: the eleven tests are red, then green, and `cargo test -p meditate-android --lib` passes.
- [x] 2.2 ui.rs and main.slint:
  - The JNI read: `Build.VERSION.SDK_INT >= 34`, `getResources().getIdentifier(name, "color", "android")`, `getColor(id, null)`, wrapped in `with_env`. A JNI failure counts as "no scheme" and goes to the log.
  - The read runs in the per-run accent block (not a OnceLock), before the accent is applied. `getIdentifier` returning 0 means no scheme, before any `getColor` call. Delete the local refs per name. `SDK_INT` is the static field `android/os/Build$VERSION`.SDK_INT with signature I.
  - `apply_accent`: with System, `set_schemes(system)`, and the label palette from the system's light and dark primaries.
  - Picker: the swatches, row to accent through theme.rs, the stored value, and the log line naming "System". Keep the literal `write_global_setting(&ui, "android_accent"`, which the source test at app.rs:4694 checks.
  - `reload_setup` reads through the same parse.
  - main.slint: 32 px circles, and `in property <bool> accent-has-system` set by Rust, with the wand on row 0 when it is true. `prefs-accent` and `accent-picked` stay row indices, and all mapping goes through theme.rs. The old clamp `.min(ACCENTS.len()-1)` goes away.
  Proof: android --lib and CI clippy pass, Android target clippy is clean, a screenshot on the API 35 emulator at 360 px, and an API 30 emulator without the circle.
- [x] 2.3 Debug APK: the preferences MANUAL scenarios on the FP5 and the API 35 emulator (wallpaper with no session running). The maintainer confirms.

## 3. Close
- [x] 3.1 third_party/README.md (the three fixes as merged), README features, cargo-sources.json only if Cargo.lock moved (it should not), check-specs.
- [ ] 3.2 Repro probe on the final tree: it runs on this commit before the push (it needs a commit), and the push only happens if it matches.

## Notes
- /simplify (2026-10-20), four agents. Fixed:
  - `SystemScheme` duplicated `MaterialScheme`; `system_schemes` now writes onto a clone of the baseline. The table has three columns, and the struct, the alias and `copy_roles` are gone.
  - JNI errors go back through `Result` and `?` instead of a side variable.
  - The startup reads the accent once and rings from `row`.
  - The swatch arm that could never run is gone.
  - SDK_INT comes from `AndroidApp::sdk_version()` instead of JNI.
  - The tests were rewritten to the new shape. A mutation that swaps a row still fails `the_system_scheme_takes_every_role_from_the_system`.
- /simplify, skipped:
  - The ~98 `getIdentifier`/`getColor` calls at startup, cutting down to the primary unless System is picked. They run once per launch, the gain is unmeasured and small, and it would add a second read path.
  - Merging the picked_accent tests into a table: each test is named by a spec scenario.
  - The double bounds check in `accent_schemes`: it is a pub fn.
  - The source-text test is fragile against refactors: that is house style (`handler`/`ui_code`), and the alternative needs a host-run UI.
- Review (2026-10-20), two fresh agents: data and sync, and completeness. Both say spec PASS and quality NEEDS FIXES. Fixed:
  - (sync) "Nothing written at startup" was untested. Added the source test `only_a_pick_writes_the_accent`. It passed at once, which is the behaviour existing; a mutation that writes at startup makes it fail.
  - (both) The ring logic was untested in ui.rs. Moved it to `theme::accent_ring`, with the test `the_ring_marks_the_accent_that_shows`. A mutation that reads the raw number fails it.
  - (sync) The SystemScheme to MaterialScheme mapping and the shadow/scrim fallback were untested. Moved `accent_schemes` into theme.rs, with the test `system_puts_every_role_on_the_material_scheme`. A mutation that swaps one row fails it. `system_roles!` no longer leaves theme.rs.
  - (both) The log line said the row index; it now says the stored value.
  - (both) Two stale Review Focus lines fixed.
- Review, open for the maintainer: a JNI failure while reading the system colours only goes to the log (hard rule: every failure tells the user). Effect: no System circle and the fixed colours, as below Android 14. Showing it needs a new text in nine languages.
- 1.1 (2026-10-20):
  - Added-line check: #14206 and #14208 have 0 missing lines. #14205 has 8 missing lines, all of them the hand-port (`hadPreedit` from `mPreeditStart/End`, `public static` order).
  - `diff -r` against the registry: only the three fixes.
  - android --lib 247 + core 1386 passed, CI clippy clean, Android target clippy clean, and the Java compiled.
- 2.1:
  - Red on the stubs: 8 of 11 failed.
  - Three passed on the stub because the stub returns the fallback, Blue or None. Mutations: forcing System onto an empty or `system` value fails `a_missing_pick_is_blue_*` and `a_system_pick_shows_blue_*`; `unwrap_or_default` for a missing role fails `a_missing_role_*`. Restored afterwards.
  - Green with the code; theme:: 15 passed.
- 2.2:
  - API 35 emulator at 360 dp, MCP x86_64 build 13:33. The arm64 build crashes on the x86_64 emulator's translator, which is not our bug.
  - With no pick, the app starts in the emulator's red system scheme.
  - Seven circles fit; System comes first with the wand and the ring.
  - Picking Green turns the app green, and it stays green after a restart. Picking System brings the system colours back.
  - API 30: six circles, Blue ringed, no crash.
- Group 1 (2026-10-20), maintainer: take #14208 exactly as merged. The merge drops the cursor reset, so a drift into the next word before the long press fires selects both words. The scenario was changed to that.
- Group 1: the line-break and end-of-text check already existed in our backend. Upstream moved it into core, so its scenario is behaviour that already existed.
- Group 1: master refactored the Java file since 1.18.1, so the merged Java was ported by hand. 1.18.1 tracks `mPreeditStart/End`, so `hadPreedit` comes from those fields instead of master's `mHasPreedit` field. The logic is the same.
- Pre-build agent (2026-10-20) on the first plan, which vendored the draft. That plan was dropped by the maintainer's choice. Points that still apply, decided as my call:
  - Ring and reload use the same parse as startup.
  - Row to accent goes through the function that builds the row, and Rust alone decides whether the circle is there.
  - The label palette needs both primaries, light and dark.
  - The circle is filled with the light system primary.
  - The wallpaper test runs with no session running (Android restarts the activity, as today).
  - The source-text test at app.rs:4694 keeps its literal.
- Second pre-build agent (2026-10-20) on this plan. It found:
  - A written one-time switch plus a marker can override a deliberate Blue: on a new install, from a Blue on Android 13, and in a race before the first sync.
  - Replaced by encoding new picks as `+N`, with no write at all (proposal, my call). The maintainer then asked for every old pick, not only Blue, to become System once, and `+N` covers that.
  - Fixed the `surfaceTint` wording to match the draft table.
  - Added `accent-has-system`.
  - Taken over its JNI details.
  - Known before this change: a synced accent applies only after a restart, because `refresh_after_pull` doesn't touch it. Not in scope.
  - Resource names through `getIdentifier` need no API 34 android.jar constants, so the jar pin stays untouched.
- Two commits are planned, one per group 1 and 2, with the change folder archived in the second. (my call)

## Hand-over (2026-10-20)
- APK: ~/.cache/slint-pr/meditate-syscolors.apk (debug arm64, 13:53), `adb install -r`.
- Emulator checks done (API 35, x86_64 MCP build 13:49):
  - With no pick, the red system scheme shows.
  - With a new system palette set while running (settings put), the app turned green by itself.
  - API 30 (build 13:33): six circles.
- FP5 test steps:
  1. Install. The app should have the wallpaper colours, since every old pick counts as System. In Preferences, the first circle has the wand and the ring.
  2. Pick Green: the app turns green. Restart: it stays green. Pick System again: the wallpaper colours come back.
  3. Open the Log with labels, in light and dark mode: the label colours follow the System primary.
  4. With no session running, change the wallpaper: on return, Meditate has the new colours.
  5. Text fields (for example a label's name):
     - Slide over the space bar while a word is still underlined: the word stays once, and the suggestions fit.
     - Long-press an empty line, or behind the end of the text: nothing is selected.
     - Double-tap a word and hold: the selection doesn't grow.
     - The earlier scenarios (sliding, the copy bar, a long press with drag) still work.
- Repro probe: it builds a commit (REF), so it runs after approval, on the local commits before the push.
- Proposed commits (two):
  1. `android: Take Slint's text-field fixes as merged upstream`
     Body: The copies in third_party/ carried #14205 and #14208 as first submitted. Upstream merged them with review changes: the keyboard is told the composing range on a cursor move, the long-press rules sit in TextInput, and the cursor reset before a long press is gone. The Java is a hand port, because master refactored the file since 1.18.1.
  2. `android: Follow the system colours on Android 14+`
     Body: Preferences gets a first accent circle, System, which takes all 47 Material 3 roles from the phone. Picks are now stored as `+N` and `system`, so every pick from before reads as System on 14+ without anything being written at startup, and the previous release still reads `+N` as its colour. Includes the archived change folder.
