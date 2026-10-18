# Tasks: Slint 1.18 and the year list (batch Y, part 1)

Base: 5d51a69652e332a86101c8be78b7a1be313f0ad7; suites on it: green
(core 1378 + 18, android --lib 223). GTK not run: it has no Slint
dependency (`cargo tree -p meditate` shows none) and no file of it changes.

## Loop
- [x] 1 Understand
- [x] 2 Propose (approved: 2026-10-18)
- [x] 3 Build
- [x] 4 Review (findings and re-review below)
- [x] 5 Specs, docs, list
- [x] 6 Verify and hand over (maintainer tested on the FP5 2026-10-18: all fine; the copy bar still covers the field, moved to #160)
- [ ] 7 Ship

## Review Focus
- A bump that changes generated code order -> repro probe (task 1.4), the whole point of the old fork.
- The Flatpak offline build -> cargo-sources.json regenerated (1.3); its CI run on the pull-request/manual trigger.
- Deprecations in 1.17/1.18 hitting our .slint or Rust -> clippy with -D warnings for the Android lib and the Android target (1.2).
- Behaviour changes in 1.18's Android backend (insets incl. cutout, Back via OnBackInvokedDispatcher on API 34+, IME selection sync, font scale) -> FP5 test steps in 6: Back closes pages and dialogs, edge-to-edge insets, text fields, #176 font-size path.
- A year count not divisible by 3 (every year but 2026, 2029, ...) -> the `ceil` in 2.2; checked on the FP5 by scrolling to 2000 (2026 has 27, so also checked by reading the code).
- A session older than 2000 -> not in the list; the dialog still shows and saves its date. No such sessions exist; no scenario.
- The year rolling over while the picker is open -> the list follows `date_now` on the next open. No scenario.

## 1. Slint 1.18.1, fork removed
- [x] 1.1 `cargo update -p slint --precise 1.18.1` (and slint-build), delete `third_party/` and the patch entry + its comment in `Cargo.toml`. Proof: `grep -c 'name = "i-slint' Cargo.lock` entries all 1.18.1, no `path+` source; `cargo tree -i i-slint-compiler` shows crates.io 1.18.1.
- [x] 1.2 Builds and lints. Proof: `cargo test -p meditate-android --lib`, `cargo clippy -p meditate-core -p meditate-android --lib -- -D warnings`, Android target clippy (BUILDING.md), a debug APK with a new timestamp.
- [x] 1.3 Regenerate `build-aux/cargo-sources.json` (RELEASE.md). Proof: no `1.16.1` slint entry left, every Cargo.lock crate listed.
- [x] 1.4 Repro probe on the uncommitted tree (temp commit via `git commit-tree`, `REF=<hash>`, JDK 17). Proof: all entries identical.

## 2. Year list (#148)
- [x] 2.1 Test `the_year_list_runs_from_this_year_back_to_2000` in `meditate-android/src/app.rs`: `picker_years(2026)` has len 27, first 2026, last 2000, each one less than the previous. Expected: FAIL (stub returns empty), then PASS.
- [x] 2.2 `picker_years(today_year: i32) -> Vec<i32>` in app.rs; `pure callback year_list(today_year: int) -> [int]` in `DatePickerAdapter`; `year_model: DatePickerAdapter.year_list(root.today_year)`; wire in `wire_date_picker_adapter` (ui.rs); `YearSelection.row_count` becomes `ceil(model.length / column_count)`, else from 2027 (28 years, 3 columns) the last row with 2000 can't be scrolled to. Proof: test passes, APK built, MANUAL scenario on the FP5.

## 3. Docs and list
- [x] 3.1 AGENTS.md, BUILDING.md keep `third_party/` in the build-environment lists (my call: the slide patch vendors the backend there next, and the rule holds for any vendored code). OPEN-ISSUES: #148 removed; #160 keeps only the space-bar slide, the copy bar (if still wrong on the FP5) and the close paths; batch Y stays open. Proof: `grep -n '#148' OPEN-ISSUES.md` empty; batch Y row lists #160 only.

## Notes
- 1.2 results: android --lib 224 passed (223 + the new test), core 1378+18 and GTK 151 passed after the libc bump; CI clippy, GTK/core clippy and Android target clippy zero warnings. APK (release type, debug key) 2026-10-18 12:49, its .so carries 1.18's `OnBackInvokedCallback`.
- Found while building: 1.18 deprecates `viewport-x/y/width/height` (now `content-*`) and `rotation-angle`/`rotation-origin-*` (now `transform-rotation`/`transform-origin`): 54 warnings in main.slint, material scroll_view.slint and date_picker.slint. Renamed at exactly the warned positions (the old names are aliases, no behaviour change); Material `ScrollView`'s own `viewport_*` properties stay, they aren't deprecated. Two source-text tests (`closing_a_page_releases_its_note`, `diagnostics_open_at_the_newest_entry`) looked for the old names and were updated.
- 2.1: the test failed on the stub for the right reason (len 0 != 27), then passed.
- Lockfile: the bump also moves libc 0.2.184 -> 0.2.191, which GTK and core use too (no other shared crate changes version). It can't stay: nix 0.31.3 under i-slint-backend-linuxkms 1.18.1 needs the newer one. So the GTK and core suites run after the bump as well.
- Pre-build agent: no questions. Found the row_count truncation; added to 2.2 (my call, inside the approved material-1.0 edit).

## Review (2026-10-18)
- R1 held, asked: picking a year jumped to 1 January. Maintainer: keep day and month. Fixed in `select_year` (day clamped by `month_day_count`). Test `picking_a_year_keeps_the_day_and_month_and_every_year_is_reachable` (FAIL, then PASS).
- R2 held: the `ceil` had no test. Covered by the same source-text test (Slint can't run on the host).
- R3 held: proposal said the docs drop `third_party/`; proposal corrected to what was done.
- R4 held: `hide_soft_keyboard` comment named 1.16.1 as the current stack; now says it was written for it.
- Probe 1 ran on 57ebe3e, before R1/R4; a second probe runs on the final tree.
- Re-review of R1-R4: PASS, one minor: the 28 February clamp rested on the untested `month_day_count` closure in ui.rs. Moved to `app::month_day_count`, test `a_month_has_its_calendar_length` (FAIL on stub, then PASS).
- After the fixes: android --lib 226 passed, 0 warnings; CI clippy and Android target clippy clean.
- Probe 1 (57ebe3e, before R1/R4): 22/22 identical. Probe 2 (b381bf3, the final tree): 22/22 identical.

## Hand-over
- APK: `meditate-android/android/app/build/outputs/apk/release/app-release.apk` (release type, debug key), 2026-10-18 13:09, installed on the FP5 13:10.
- Test steps (FP5):
  1. Log: edit an older session, tap the date, tap the year header. Years run from 2026 at the top to 2000 at the bottom, scrollable to 2000; the session's year is marked. Pick another year: day and month stay. OK, Save: the card moves to that date.
  2. Same with a session on 29 February 2024 (or add one), pick 2023: 28 February.
  3. Back (gesture or button) closes the edit dialog, the chooser pages, Preferences and the Diagnostics page; on the Timer page Back behaves as before. Nothing closes the app by surprise.
  4. Edge-to-edge: app bar and nav bar not under the status or gesture bar, also with the keyboard up.
  5. Text fields (rename label, name dialog, Log note, Preferences): empty field, tap: no handle at the screen's left edge. Copy bar position. Close the dialog: no handle or keyboard left behind.
  6. System font size larger while the app is open, then back: the app follows (#176 is a known separate bug).
  7. German and Russian: any text cut off at the screen width? 
- Proposed commit:

      android: Slint 1.18.1, own fork dropped, year picker from 2000

      Slint 1.18 sorts the bundled languages itself (slint-ui/slint#12932),
      so third_party/i-slint-compiler goes. It also hides the cursor handle
      of an empty field (part of #160). Deprecated viewport-* and rotation-*
      names renamed. The date picker's year list runs from this year back to
      2000 (#148, was 2024 to 2043 without 2030), and picking a year keeps
      the day and month. Repro probe: 22/22 identical.
