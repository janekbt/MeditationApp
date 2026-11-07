# Tasks: setup-lists-order

Base: 006f524c70ea5f92f805995755d3f52867bbac6e; suites on it: core 1469 + 4 + 11 + 3 green, Android lib 296 green, GTK 171 green

## Loop
- [x] 0 Skills loaded: `/ponytail ultra`, `slint:slint` (Android UI touched)
- [x] 1 Understand
- [x] 2 Propose (approved: 2026-11-07, Janek added handles on Manage Imports with the go)
- [x] 3 Build (core 1479+4+11+3, GTK 171, Android lib 296 green; clippy GTK+core and Android target clean)
- [x] 4 Review (R1: two agents, see below; fix review: PASS), then `/simplify` (see below); suites core 1481+4+11+3, GTK 171, Android 296 green; clippy GTK+core and Android target clean
- [x] 5 Specs, docs, list (check-specs green; EVENTS, ARCHITECTURE, README updated; #22 is a GitHub issue, not in OPEN-ISSUES)
- [x] 6 Verify and hand over (maintainer 2026-11-07: "alle tests bestanden", sync phone -> laptop v26.11.0 included)
- [x] 7 Ship (deltas appended to openspec/specs/guided and presets)

## Review Focus
- Two devices moving library files before they sync -> the reused key scheme's `two_devices_moving_before_they_sync_end_in_one_order` covers the keys; no new scenario (the helpers move unchanged)
- An older device (v26.11.0) receiving a moved library file -> it applies `created_iso` from updates (checked in v26.11.0's `guided_files.rs:353`); its list follows
- A starred local file from another folder, or marked not found -> it keeps its key and can be moved like any row
- A starred library file unstarred, then starred again -> back at its old place (key kept); covered by the decision, no scenario
- A local file unstarred -> key cleared; starred again -> last (scenario "A newly starred local file comes last")
- A list of one row, an empty list -> no move possible; a drop writes nothing
- A move while a sync refresh rebuilt the list -> moves by identity, not by index; an unknown identity writes nothing
- Equal neighbour keys (two imports in the same millisecond) -> library re-timed as presets do; local run re-keyed
- Another language -> no new strings
- Widget -> follows the preset order; Starred Files are not in the widget

## 1. Core
- [x] 1.1 Move `key_between`, `just_above`, `count_up`, `digits_below` to `meditate-core/src/db/order_key.rs` (`pub(crate)`), unchanged; `presets.rs` uses them. Proof: the 18 existing preset move tests stay green (refactor, no new test).
- [x] 1.2 `fixtures/schema_v26_11_0.sql` from v26.11.0, `release_v26_11_0_database()` in `test_helpers.rs`; `star_key` in `ADDED_COLUMNS`; starred list sorts by key. Files: `db/schema.rs`, `db/local_guided_files.rs`, `local_folder.rs` (unkeyed rows keep `by_name`). Proof: `starred_files_from_an_older_database_keep_their_order` (Expected: FAIL, then PASS) and `an_upgraded_database_has_the_same_columns_as_a_fresh_one`.
- [x] 1.3 Starring a local file gives it the key after the highest; unstarring clears it. Proof: `a_newly_starred_local_file_comes_last`.
- [x] 1.4 `move_starred_guided(id: StarredGuidedId, to)` as in design.md. Proofs: `a_local_file_moves_between_library_files_without_sync`, `a_library_file_moved_past_only_local_files_writes_no_sync`, `a_library_reorder_reaches_the_other_device`, `a_reorder_from_another_device_keeps_local_files_in_place`, `a_starred_file_dropped_on_its_place_writes_nothing`.
- [x] 1.4b `move_guided_file(uuid, to)` for Manage Imports. Proof: `a_move_on_manage_imports_shows_in_starred_files`.
- [x] 1.5 `move_starred_preset(uuid, to)` in `db/presets.rs`. Proofs: `a_starred_preset_moved_on_setup_takes_its_place`, `a_starred_preset_moved_down_on_setup_lands_last`.

## 2. GTK
- [x] 2.1 Make `add_reordering` (`presets.rs:337`) take the drag text and a move closure instead of `&Preset`; the closure gets the place in the list it is shown in (Manage: whole mode list; Setup: starred list); Manage Presets keeps its behaviour. Proof: MANUAL (Manage page drag and Alt+Up/Down unchanged).
- [x] 2.2 Handle, drop and Alt+Up/Down on the Setup Presets rows (`timer/imp.rs`, calls `move_starred_preset`) the Starred Files rows (`rebuild_starred_guided_list`, calls `move_starred_guided`) and the Manage Imports rows (`guided.rs` `rebuild_chooser_rows`, calls `move_guided_file`; the create row is not a place); failures through `write_ok(.., Failed::Save)` as presets do; the move runs on idle after the drop; focus stays on the moved row. Proof: MANUAL scenarios in both deltas.

## 3. Android
- [x] 3.1 Rename `refresh_preset_chips` -> `refresh_preset_list`, `preset-chip-tap` -> `preset-row-tap` (+ `on_preset_row_tap`) and the "chip" comments that mean this list (`ui.rs`, `app.rs`, `widget.rs`, `main.slint`). Proof: suites green; grep shows no preset "chip" name left.
- [x] 3.2 `main.slint`: handle (`assets/drag_handle.svg`), DragArea, DropArea and landing line on the Setup Presets rows, the Starred Files rows and the Manage Imports rows, as on the Manage Presets rows; callbacks `preset-setup-move(string, int)`, `guided-starred-move(string, int)` and `guided-manage-move(string, int)`, each list with its own drag-index property (the Manage chooser sits over Setup, a shared one would light both). `ui.rs`: they call the core moves through `setting_saved`, then refresh the list (and the widget for presets). Check `long_names_wrap_and_rows_grow`. Proof: MANUAL scenarios.

## 4. Docs
- [x] 4.1 `ARCHITECTURE.md` (the column, the shared order key), `EVENTS.md` (a guided file's `created_iso` is a sort key), `README.md` if it lists preset order. Proof: grep for `created_iso` in the docs.

## Review R1 (2026-11-07, two agents: data and sync; completeness and rules)
- 1 IMPORTANT (rules): Android Manage Imports name elided in a fixed 64 px row, worse with the handle: fixed, the name wraps and the row grows; `long_names_wrap_and_rows_grow` now covers `root.guided-manage-items` (FAIL first, then PASS).
- 2 IMPORTANT/MINOR (both): GTK Alt+Down on the last Setup preset moved it past an unstarred one and wrote an event: fixed, `move_starred_preset` clamps `to`; asserted in `a_starred_preset_moved_on_setup_takes_its_place` (FAIL first) and a scenario.
- 3 MINOR (both): the Starred Files no-room fallback re-timed only starred library files, moving unstarred ones on Manage Imports: fixed, all re-timed with unstarred ones in place (`retime_library`, shared with Manage Imports); tests `a_move_with_no_room_keeps_the_manage_imports_order` (FAIL first after making the unstarred file the newest) and `a_manage_imports_move_with_no_room_keeps_local_places` (passed at once; FAIL with its repair removed); scenario added.
- 4 MINOR: unkeyed old local rows get keys at a local star too, not only at a move: decision text updated (keying at star time keeps "comes last" true).
- 5 MINOR: EVENTS.md "only the moved file" omitted the no-room case: fixed.
- 6 MINOR: leftover preset "chip" comments (ui.rs, app.rs): renamed.
- 7 MINOR: DragHandle split WrapButton from its comment: fixed.
- Rejected: whole-row last-writer-wins lets a move overwrite a concurrent rename or unstar on another device. That is the documented event model (EVENTS.md "no per-field-LWW merges"), the same for preset moves since v26.11.0; shown to Janek.
- Rejected: no TalkBack way to move rows on Android: the vendored backend has no accessibility bridge (preset-order notes, Janek dropped it there); unchanged here; shown to Janek.

## Fix review (2026-11-07): Quality PASS, three MINOR
- 1 The Manage Imports no-room fallback keeps local files at their rows, the normal path at their keys: documented in design.md (Risks), not changed; equal import times are near impossible.
- 2 EVENTS.md and proposal said "no event" without the no-room exception: fixed.
- 3 design.md said "re-keys every local row": now "repairs the local runs". The scenario with two halves was split so each test is linked.

## /simplify (2026-11-07)
- Fixed: the `local:<id>` row key now lives in core (`StarredGuidedId::key` / `from_key`), used by the new GTK and Android move code; `StarredGuided::key()` and `local_tail` reused in the starred sort and `set_local_file_starred` (were re-derived); `found` computed once, the sort compares `&str` instead of cloning; one `match` on the moved row instead of three checks; `write_star_key` takes `&str`; `move_guided_file` no longer keeps the starred rows mutable outside its fallback; the Android DropArea + landing line is one `RowDrop` component on the three new lists.
- Skipped: the older `local:` builders and parsers (ui.rs 1185, 4166, 5421, 5446; timer/imp.rs 3506, 3530), outside this diff; one payload builder for the three `guided_file_update` writers (two existed before; touches rename and star paths); the 1 ms re-time loop shared with `move_preset`; `swap_events` test helper copies; one `MoveTo` per Manage Presets row (built per row before too); `move_starred_preset` reading the list twice (one move per drag); the GTK `MoveTo` closures differ in their refresh, so no shared helper; RowDrop not used on Manage Presets (its drop also checks the save mode, and the code was there before).

## Notes
- Found in the laptop test (2026-11-07), not caused by this change: a preset saved with the label off and no label uuid never gets its check once another preset has set a label uuid (`active_presets` compares the hidden uuid of a switched-off label; probed in core). Janek: a separate small change right after this one.
- Builder check (fresh agent, 2026-11-07), all answered in design.md or tasks.md, my calls: (1) Manage Imports follows Starred moves, unstarred keep their keys; (2) "library order changed" counts starred library files only; (3) a new local star goes after the highest starred key; (4) core takes a typed `StarredGuidedId`; (5) Android Setup and Manage lists get separate drag-index properties. Plan fixes: the no-room fallback re-keys all local rows too; runs at the top and bottom are repaired; unkeyed rows keep `by_name`; the migration test lives in `local_folder.rs`; `add_reordering` gets the shown list's place.
- Core build: all 9 new tests failed on stubs first, except two that passed at once and were checked by breaking the code: `starred_files_from_an_older_database_keep_their_order` (first fixture had name order = path order, so it caught nothing; fixed with "Sub/c.ogg", then FAIL with by_name removed) and `a_starred_file_dropped_on_its_place_writes_nothing` (FAIL with the same-place check removed). That test first asserted "no key"; a newly starred local file gets its key at star time (design), so it now asserts the key is unchanged. `move_starred_preset` got a same-place check (A, u, B: moving A to its own place would have moved it past u and written an event); covered in `a_starred_preset_moved_on_setup_takes_its_place`. Moves live in `db/starred_order.rs` (needs `emit_event`). Suite 1479 + 4 + 11 + 3 green.
- Slint has no auto-scroll during a drag; a far move takes two drags (as for presets).

## Device test fixes (step 6)
- Linux, Alt+Up/Down on Setup Presets: the focus jumped back to the first row soon after. Cause: the 500 ms local-change watch rebuilds the Setup preset list after every write, which drops the focus. Fix: both Setup lists remember the focused row by its key (widget name) and give the focus back after any rebuild; the move closures no longer refocus by index. MANUAL scenario "Without dragging" covers it. Maintainer retest 2026-11-07: all tests passed (sync checked phone -> laptop v26.11.0).
- /simplify (focus fix): two one-line simplifications applied. Skipped: let the 500 ms watch only move the check instead of rebuilding the preset list (older code outside this change; the rebuild also refreshes subtitles; worth its own change); index-based refocus instead of keys (the watch rebuilds the list too, so keys are needed).
- Fix review (focus fix): PASS, two MINOR. 1 `has_focus()` is false while the window is inactive, so a rebuild then dropped the focus: fixed, `is_focus()`. 2 With the focus on a missing local file's Unstar button, a rebuild gives it to the row: accepted (rare state; the row is the closest stable target).

## Hand-over
Artifacts (2026-11-07):
- Linux: `meditate-gtk/builddir/src/meditate` (22:00, with the focus fix), started with
  `XDG_DATA_HOME=/home/janek/.cache/meditate-starorder/data LOCALEDIR=meditate-gtk/builddir/po`;
  test folder `/home/janek/.cache/meditate-starorder/folder` (Atem, Glocke, Abend/Gong).
- Android: `meditate-android/android/app/build/outputs/apk/debug/app-debug.apk` (21:45), debug, `adb install -r`.

Test steps (Linux and phone):
1. Timer Setup with at least three presets, one of them not starred: every row has a handle at the left. Drag the last one to the top: it stands there; Manage Presets shows it on top too (phone: the widget follows). A swipe over a row body scrolls; on the phone a quick swipe on the handle scrolls, a short press then drag moves.
2. Linux: focus a Setup preset row, Alt+Up / Alt+Down moves it, the focus stays; Alt+Down on the last row does nothing.
3. Guided: import two files (Open File, Import), choose the test folder as Local Folder and star two local files. Starred Files: imported first, then the local ones by name. Drag a local file between the two imported ones, then an imported one to the bottom: the order holds after leaving and reopening Guided and after a restart.
4. Manage Imports: every row has a handle; drag the second file to the top; Starred Files follows. Phone: a long file name wraps, nothing is cut off.
5. Linux: Alt+Up / Alt+Down on Starred Files and Manage Imports rows.
6. With sync on (phone): move one imported file in Starred Files; after sync the laptop shows the new order of the imported files; the local files stay on the phone only.

Proposed commit message:

    guided, presets: own order for Starred Files and the Setup lists

    GitHub #22 asked to rearrange the starred guided files. Starred
    Files, Manage Imports and the Setup preset list now get a drag
    handle (Alt+Up/Down on Linux), in both apps. Imported and local
    files mix freely; only the imported files' order syncs, as a
    guided file update like a preset move. Local files keep their
    place on the device in a new local-only column, added on open
    (v26.11.0 can still read the database). The preset list's old
    "chip" names are gone.

    Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
    Claude-Session: https://claude.ai/code/session_012WGZUPYHL7TurMz6YSFVJs
