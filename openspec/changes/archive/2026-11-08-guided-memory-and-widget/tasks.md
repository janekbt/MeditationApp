# Tasks: guided-memory-and-widget

Base: 6d19b58d7431a639305763eec0243d12dc77f474; suites on it: core 1482, Android 296, GTK 171 green

## Loop
- [x] 0 Skills loaded: `/ponytail ultra`, `slint:slint` (Android UI touched)
- [x] 1 Understand (confirmed 2026-11-07, plus the widget order wish; decisions: drag list, no headings, mode remembered too)
- [x] 2 Propose (approved 2026-11-07: "go")
- [x] 3 Build (core 1495, Android 298, GTK 171 green; clippy GTK+core, CI, Android target clean)
- [x] 4 Review (R1, fix reviews F1, F2), then `/simplify` (see below)
- [x] 5 Specs, docs, list (check-specs green; README features, ARCHITECTURE sync_state, OPEN-ISSUES dead `WidgetPreset::mode` item removed)
- [x] 6 Verify and hand over (Device test 1: steps 1-6, 8 passed on Android; order setting dropped; maintainer "go" 2026-11-08)
- [x] 7 Ship (deltas merged into openspec/specs guided, setup, presets, preferences)

## Review Focus
- Second device -> last mode and file live in `sync_state` (never synced): scenarios "Another device ..."
- Deleted file, missing local file, other folder -> "stays unchosen" scenarios; the restore checks at start only, the running follow rule is unchanged
- Undo of a delete after a restart -> nothing to undo (Undo dies with the app), out of scope
- Old database / first start after the update -> no keys: Timer, nothing chosen
- A session rescued after process death -> saved to the Log at start as today, mode untouched (pre-build Q1, my call)
- Selection changes not made by a tap (follow Clear/Reselect, failed length read, Create-from-Manage pick) -> the memory mirrors what is shown (pre-build Q3, Q4, my call)
- Local file with unread length at start -> not restored, no notice (pre-build Q2, my call)
- A widget tap at cold start -> wins over the remembered mode; MANUAL scenario
- Widget rows drawn by the previous version (bare preset uuid in `preset_uuid`) -> still start: `a_widget_tap_tells_a_file_from_a_preset`
- A tap value that is garbage (`guided:` with nothing after) -> "File not found", nothing starts; in the same test
- Empty widget (no starred presets, no starred files) -> empty text names both
- Another language -> two new Android strings x9, widget empty text x10 files; Box Breath/Timer/Guided names exist
- Long file names in the widget -> rows are one line with ellipsis today (`widget_item.xml`); unchanged, RemoteViews can't measure; not new
- GTK -> no widget; only the mode and file memory

## 1. Core
- [x] 1.1 `settings_keys.rs`: `last_mode_from_db` / `set_last_mode` in `sync_state`. Proof: `the_last_mode_comes_back_after_a_restart`, `the_last_mode_stays_on_its_device` (no `setting_changed` event), `the_app_opens_in_timer_with_nothing_remembered`. Expected FAIL then PASS.
- [ ] 1.2 DROPPED after Device test 1 (maintainer). Was: `settings_keys.rs`: `widget_mode_order_from_db` (Vec of three modes; broken value -> default) and `move_widget_mode(db, mode, to_index)` via `set_setting`. Proof: the three `widget` tests in the preferences delta.
- [x] 1.3 `guided.rs`: `remember_guided(db, Option<&StarredGuidedId>)` (None forgets) and `remembered_guided_from_db(db) -> Option<StarredGuidedId>` that returns only a library file that exists or a local file found in the current folder; value = `StarredGuidedId::key()` in `sync_state`. only with a known length. Proof: the six core tests in the guided delta.

## 2. GTK (`timer/imp.rs`)
- [x] 2.1 `on_mode_switched` saves the mode; `restore_last_choice` runs in `activate` before `present()` and before the recovery toast.
- [x] 2.2 The one place that shows the chosen row (`refresh_guided_selected_row`, called after every selection change) calls `remember_guided` with what it shows; a pick without uuid or local id forgets. At start, `remembered_guided_from_db` sets the pick as `follow_selected_guided_file` does. Proof: MANUAL (restart the app, file and mode back).

## 3. Android
- [x] 3.1 `ui.rs`: `mode_changed` saves the mode; `build_ui` restores it before the cold-start widget deep link runs (the tap then sets its own mode).
- [x] 3.2 `ui.rs`: `show_guided_sel` (called after every `guided_sel` change) remembers what it shows (library uuid, `local:<id>`) or forgets (no selection, Open File or Create pick); `build_ui` restores through the existing starred-tap and `select_local_file` paths. Proof: MANUAL "Android ends the app".
- [x] 3.3 `widget.rs`: `WidgetPreset` -> `WidgetRow { launch, name, subtitle }` (drop the unread `mode` field); pure `order_rows(order, rows_per_mode)`; `parse_launch(value) -> Launch::{Preset(uuid), Guided(StarredGuidedId)}`, a bare value is a preset. Proof: `the_widget_lists_files_between_timer_and_box_breath`, `a_widget_tap_tells_a_file_from_a_preset`.
- [x] 3.4 `ui.rs`: `widget_presets_snapshot` builds Guided rows from `starred_guided_from_db` with the Starred Files subtitles (shared with `refresh_guided_files`); `refresh_guided_files` also calls `refresh_widget`. `MeditateWidgetService.kt` puts the row's `launch` value in the `preset_uuid` extra.
- [x] 3.5 `try_widget_deep_link`: a Guided launch -> Timer tab, close overlays, Guided mode, choose the file through the starred-tap path; found and length known -> `invoke_action_tap`; gone -> "File not found" notice. Proof: MANUAL widget scenarios.
- [ ] 3.6 DROPPED after Device test 1 (maintainer). Was: `main.slint`: Widget section after Appearance, title "Widget", row header "Order in the Widget", three rows with `DragHandle`, DragArea/DropArea as the Setup preset rows; callback `widget-mode-move(int, int)` -> `move_widget_mode` -> `refresh_widget`. Check 360 px in all languages. Proof: MANUAL "Dragging on the phone".
- [x] 3.7 Translations (the two Preferences strings dropped after Device test 1):  `widget_empty` "No starred presets or files" in `res/values*/strings.xml` (ten). `msgfmt --check`.

## 4. Docs
- [x] 3.8 `main.slint`: Danger Zone last in Preferences (asked during Device test 1). Proof: MANUAL preferences delta.
- [x] 4.1 README features (widget line), ARCHITECTURE.md `sync_state` keys if listed; grep docs for `WidgetPreset`.

## Notes
- Tests that passed at once, checked by breaking the code: `the_last_mode_stays_on_its_device`, `the_app_opens_in_timer_with_nothing_remembered`, and four guided tests (gone, unread length, one-off, not found): each failed with its check removed.
- Build notes: `MeditateWidgetService.kt` needed no change: the JSON key stays `uuid` and carries the tap value. `WidgetPreset::mode` and its test `mode_tag_is_carried` are gone (OPEN-ISSUES listed them as dead). New `impl From<SessionMode> for TimerMode` (`every_mode_maps_back_from_core`) for the two new mappings.
- GTK: `choice_restored` holds the memory writes back until `restore_last_choice` ran at window start, so a selection shown while the window builds can't erase it. Android reads the memory at the top of `build_ui`, before any selection shows, and restores before the recovery snackbar (a mode switch closes an open notice).
- Maintainer asked to shorten the hold before a drag by a third. The wait is Slint's Flickable `DURATION_THRESHOLD` (500 ms, `third_party/i-slint-core/items/flickable.rs`), shared by every scroll list; asked about third_party, answer: leave it. Not changed.
- Pre-build questions (fresh agent, 4): all my call, see Decisions and Review Focus. Keep the receiver's `MeditateDropFile.write(File(dir, "widget_launch"), uuid)` text as is: `only_the_widget_can_start_a_preset` reads it.

## Review R1 (fresh agent)
- 1 CRITICAL, fixed: the mode save ran before `dismiss_toasts()` (GTK) / `finish_notice()` (Android), so its "Save failed" closed at once. Both writes now follow the dismiss.
- 2 IMPORTANT, fixed: GTK restored in the map idle, after the recovery toast, and the mode switch dismissed it. `restore_last_choice` now runs in `application.rs` `activate` between `MeditateWindow::new` and `present()`.
- 3 MINOR, fixed by 2: the window no longer paints Timer first.
- 4 MINOR, fixed: Android `show_guided_sel` writes the memory only when the shown key changed (`GUIDED_REMEMBERED`), so a mode switch with the database closed shows nothing new.
- 5 MINOR, not fixed: no host test that `widget_rows` reads the stored order; `ui.rs` builds only for Android (no host tests there). MANUAL "Dragging on the phone" covers it.
- Docs (4.1): step 5, not done yet at review time.

## Fix review F1 (fresh agent, on R1 fixes 1-4)
- F1.1 IMPORTANT, fixed: GTK `guided.remember` went through `setting_saved`, whose failure path queues `reload_setup`; that refreshes the selected row, which wrote and failed again: an endless "Save failed" loop on a database that refuses writes. Both GTK memory writes now use `crate::db::write_ok` (message, no reload), and the file is written only when the shown id differs from the last one written (`guided_remembered`), as on Android.
- F1.2 MINOR, fixed: Android with the database not open showed "Save failed" on every mode switch. Both memory writes are skipped when there is no database (nothing to remember into); R1 item 4's claim now holds.

## Fix review F2 (fresh agent, on F1)
- F2.1 IMPORTANT, fixed: `a_failed_setting_says_so_and_springs_back` (GTK) counted 2 direct `write_ok` calls; now 4, and it asserts the two memory writes use `write_ok` without the reload and set `guided_remembered` only on success.
- F2.2 MINOR, fixed: the Android write-only-on-change logic got `a_refused_memory_write_is_tried_again` (source text; `ui.rs` builds only for Android).
- Suites: GTK 171, Android 299 green.

## /simplify (4 agents, after the last review)
- Applied: core `remember_guided` writes only when the value changed, so both shell caches (`GUIDED_REMEMBERED`, `guided_remembered`) and their source-text checks (F2) are gone; a refused write now says "Save failed" once per refresh, without the reload, so no loop. `widget::publish` skips the write and the redraw when the rows are unchanged (every switch into Guided and every sync called it). GTK mode toggle names in one table `MODE_NAMES` for `current_mode` and the restore. GTK restore of a local file goes through `select_local_pick`. Android: `show_setup_for_launch` shared by the preset and file deep links; the file's ready check reads the shown key and length, so `try_widget_deep_link` lost its `guided_sel` parameter. The misplaced `refresh_after_pull` doc comment is back on its function.
- Skipped: the startup restore switching to Guided and back when a file is remembered but another mode was open (it goes through the same tap the user makes; a direct set would be a second path); one extra file lookup at GTK start (one query); inlining `order_rows` (it is the tested piece); building the widget's tap value with `format!` instead of `launch_value` (one place for the prefix); the GTK `mark_selected_starred_row` key builder (outside this diff).
- Suites after: core 1495, Android 298, GTK 171; clippy GTK+core, CI, Android target clean; check-specs green.

## Hand-over
Artifacts: `meditate-android/android/app/build/outputs/apk/debug/app-debug.apk` (2026-11-08 00:36, installed on the FP5 00:37), `meditate-gtk/builddir/src/meditate` (00:36). GTK start: `XDG_DATA_HOME=/home/janek/.cache/meditate-guidedmem LOCALEDIR=meditate-gtk/builddir/po ./meditate-gtk/builddir/src/meditate`.

Test steps:
1. Phone: Guided, choose a starred library file. Send the app to the background, run `adb shell am kill io.github.janekbt.Meditate`, open it: Guided is open, the file chosen.
2. Same with a local folder file whose length shows. Then with an Open File pick: after the restart nothing is chosen.
3. Box Breath open, close the app fully, open: Box Breath. Linux: same with the test build (close the window, start again).
4. Delete the chosen library file in Manage Files, restart: nothing chosen, no message.
5. Widget: starred files show after the Timer presets, before Box Breath, with the same second line as Starred Files. Tap one: Guided opens and the file plays.
6. During a Timer session tap a file in the widget: the session shows, nothing new starts.
7. Preferences: scroll to the end: the Danger Zone is the last group.
8. Widget with nothing starred: the empty text names presets and files.

Proposed commit message:
```
guided, widget: remember the last file and mode, start files from the widget

GitHub #22 asked to keep the guided file when Android ends the app
and to start guided meditations from the widget. Each device now
remembers the mode open last and the chosen Guided file (device-local,
never synced) and opens with them, on the phone and on Linux. The
widget lists the starred files between the Timer and the Box Breath
presets and starts them. The phone's Preferences show the Danger Zone
last, as on Linux.

Fixes #22

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_012WGZUPYHL7TurMz6YSFVJs
```

## Device test 1 (maintainer, 2026-11-07/08)
- Steps 1-6 and 8 passed on Android. Step 7 failed: the Widget order rows in Preferences never started a drag (all other drag lists work). Maintainer: "Das reicht mir erstmal, wir lassen das erstmal weg." Removed: the Preferences Widget section, `widget-mode-move`, `show_widget_modes`, core `widget_mode_order_from_db` / `move_widget_mode` and their three tests, the two strings in the nine `.po` files, the preferences delta. The widget keeps the fixed order Timer, Guided, Box Breath (guided delta scenario "Files between the Timer and Box Breath presets").
- Investigation for a later attempt: in-app logging showed rows built with data "0".."2" but `dragging` never true. Reproduced on the API 35 emulator with `input draganddrop` (the same gesture starts a drag on the Setup preset row) and with the MCP `drag_element`. Not the cause: scrolling (section moved to the top of the page, still no drag), the pinned header (page rebuilt like Manage Presets with the header inside the Flickable, still no drag), long press (only acts on a focused TextInput), sizes (handles 32x40, rows full width). Not finished: the same list copied into Setup (built, not run).
- Asked during the test: Danger Zone to the end of the phone's Preferences. Done (`main.slint`), scenario in the preferences delta. GTK already has it last.

## /simplify after Device test 1 (4 agents, post-test edits only)
- Applied: `set_last_mode` writes only when the mode changed (as `remember_guided`); the `refresh_widget` doc names the file rows and the skip-unchanged publish.
- Skipped: passing the Starred Files items into `widget_rows` (it is also called from preset paths without them; the unchanged-rows check already avoids the redraw); a core `SessionMode::ALL` for the Timer, Guided, Box Breath order (touches GTK and Android mode code outside this change); a `guided_launch` helper instead of `launch_value` (the preset arm keeps the round-trip test meaningful); keeping the last widget JSON in memory instead of reading the file (one small read).
