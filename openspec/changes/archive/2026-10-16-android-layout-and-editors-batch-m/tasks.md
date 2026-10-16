# Tasks: Android layout and editors (batch M)

Base: c46bfdb9fa53a8fb8a1a06a01cba2f556f0aac5c; suites on it: green (`cargo test --workspace`)

## Loop
- [x] 1 Understand (all of #139, #142-#146, #189 re-checked: still present, line numbers moved; GTK does not mark the guided file either; #148, #160 moved to batch Y and #147 to batch Z by Janek)
- [x] 2 Propose (approved: 2026-10-15)
- [x] 3 Build (1745 passed; clippy clean on host, CI form and Android target; no new Slint warnings)
- [x] 4 Review (first review: R1, R2 fixed, R3 rejected; second review on R1-R2: R4-R7 fixed; third review on R4-R7: PASS, nothing found)
- [x] 5 Specs, docs, list (check-specs green; OPEN-ISSUES: M ticked and its items removed, Y and Z added; no other doc names the changed code)
- [x] 6 Verify and hand over (Janek tested all steps on the FP5 and the laptop 2026-10-16: all passed; a drag on a volume slider does not let the page scroll)
- [x] 7 Ship (deltas merged into vibration, bells, guided, timer; archived 2026-10-16; check-specs green)

## Review Focus
- A bundled pattern opened on the FP5 -> it opens as a copy; the copy keeps the stored points too (same load path). No new scenario.
- A pattern created new on the FP5 -> its seed still rounds (nothing stored to keep). No change.
- Points or Duration changed with the steppers (not typed) on the FP5 -> the resample still rounds on that phone; that is a user edit. No change.
- Save tapped twice, or the deferred `changed` handler firing after Save's direct call -> the points handler only resamples when the count differs, the duration handler only clamps: running them twice is harmless. Checked in 2.1.
- Slider: a cancel without a prior down, or a cancel after a normal up -> restore only while a press is remembered; checked by the test in 3.1.
- The interval-bell editor scrolling while its volume slider is dragged sideways -> the slider must still take horizontal drags inside the Flickable (#146 needs #144). Device step.
- Selected guided file deleted, unstarred, renamed or synced away -> the check follows the selection the app already tracks (spec "The selected guided file follows renames and deletes"); no row shows it if the file is gone.
- Another language, 360 dp, old database, second device -> no text, data or sync change; layout in de/ru checked on the phone.

## 1. Pattern editing keeps the stored pattern (#139)
- [x] 1.1 `editing_a_pattern_loads_its_stored_points_and_kind` in `meditate-android/src/app.rs` (source text: ui.rs only builds for Android): the `on_pattern_edit` body sets the chart kind from the stored kind and the model from the stored intensities, with no `ve_maybe_binarize` and no `binary` branch. Then remove both in `src/ui.rs`. Expected: FAIL, then PASS.

## 2. Save applies typed values (#142)
- [x] 2.1 `vibration_save_applies_typed_duration_and_points_first` in `app.rs` (source text over `ui/main.slint`): the vibration editor's Save action calls `ve-duration-changed`, then `ve-points-changed`, after both `commit()` and before `vibration-editor-save()`. Then add the two calls. Expected: FAIL, then PASS.

## 3. Volume slider cancel (#144)
- [x] 3.1 `a_cancelled_volume_drag_puts_the_slider_back` in `app.rs` (source text over `VolumeSlider`): on `down` it remembers the value; on `cancel` it restores that value and sends `value-changed` with it; `up` releases as now. Then the code in `main.slint`. Expected: FAIL, then PASS.

## 4. Layout (#143, #145, #146, #189), proved on the device
- [x] 4.1 #143: `wrap: word-wrap` on the title/subtitle Texts of SwitchRow, ChooserRow, ExpanderRow, ExpanderGroup, the interval-bell list row and the preset chooser subtitle (`main.slint`); the fixed-height rows (SwitchRow 64, ChooserRow 56, expander headers 64) grow with `max(<old>, layout.preferred-height)` as the bell chooser rows do; the expander body maths stays.
- [x] 4.2 #145: pattern and sound chooser rows: the "Edit" text button becomes an `IconButton` with `Icons.edit`, the PreviewPill becomes an `IconButton` with `assets/play.svg` / `assets/pause.svg`, in GTK's order (check, play, edit, delete); accessible labels reuse the existing "Edit", "Play", "Stop" strings. Remove the stale "no play/stop SVG is bundled" comment. PreviewPill stays for the Volume card's Stop pill. While playing the button shows the pause icon, labelled "Stop", and stops.
- [x] 4.3 #146: a Flickable around the interval-bell editor body and around the vibration editor's rows above the chart.
- [x] 4.4 #189 Android: a selected-key property (`"<uuid>"` or `"local:<id>"`; not a `GuidedFileItem` field, Manage Files shares that struct) set wherever the selection is shown (`show_guided_sel`, `select_local_file`), compared in the Starred Files row; a check icon in primary colour at the row's right end, as in the pattern chooser. A missing row or a transient Open File pick shows none.
- [x] 4.5 #189 GTK: `starred_library_row` and `starred_local_row` in `meditate-gtk/src/timer/imp.rs` mark the selected file's row as the preset rows do: `object-select-symbolic` suffix with class `selected-check`, the `accent` class and `State::Selected(true)`. The list is rebuilt from `refresh_guided_selected_row` (Guided mode only), and focus goes back to the selected row so keyboard and screen-reader focus isn't lost.

## 5. Done
- [x] 5.1 OPEN-ISSUES: remove #139, #142-#146, #189 and the M section, tick M (Y and Z were added in step 1).

## Review (fresh agent, 2026-10-15)
- R1 held: ChooserRow now grows, but the ExpanderRow body (`reveal * 56px`) and RevealChooser (`reveal * 57px`) clip boxes stayed fixed, so a wrapped Sound, Pattern or Label subtitle lost its second line. Fixed: both boxes size from the row's height (`body-row.height`, `1px + rc-row.height`); `reveal` stays the only animated value. Proof: device step (layout).
- R2 held: two comments still said Edit forces Bar / binarizes on load. Fixed.
- R3 rejected: a core helper for the Starred Files selected key. Each app's key is internal to that app (Android matches `refresh_guided_files`, GTK its own rebuild), nothing crosses between them, and the helper would be one line per app; a shared type for it adds code without a case it prevents.
- Not code, for the device test: in landscape the vibration editor scrolls only the part above the chart, so Save and Back scroll away with the rows; the Starred Files check is an `if` inside a `for` (the pattern chooser once had such an element land behind its controls).
- Second review (fresh agent, on R1-R2): PASS on R1, one more fix and three comments.
  - R4 held: ExpanderRow's body row had no `y: 0`, so Slint centred it in the clip box and it shrank toward its middle instead of sliding up (also before this change; now visible on a taller wrapped row). Fixed: `y: 0`. Proof: device step (Label expander).
  - R5-R7 held (comments): ChooserRow "56 px fixed", the Log edit Label "0↔56 px", and the no-amplitude comments in `haptics.rs` and `main.slint`. Fixed.
- Third review (fresh agent, on R4-R7): PASS, no findings.

## Notes
- 4.2 broke two older source-text tests that pinned the old pill and "Edit" text button: `own_patterns_can_be_deleted_with_undo` now finds the new suffix layout; `preview_pills_fit_their_label` now counts one PreviewPill (the volume row) and no longer looks for the Edit text width. Same intent.
- 4.3 vibration editor (my call): a Flickable holds the header and the list, sized between 0 and its content height; a filler under the chart takes the spare space, so portrait looks as before. The interval-bell editor scrolls as a whole, header included, like the Log edit page.
- 4.5 (my call, changes the plan): GTK marks the row in place instead of rebuilding the list (`mark_selected_starred_row`, run after every rebuild and from `refresh_guided_selected_row`), so focus can't be lost and no focus restore is needed.
- 2.1 widened the window of `the_vibration_editor_commits_and_releases_its_numbers` from 200 to 300 characters: the two new calls pushed the commits out of it; same assertion.
- Question pass (fresh agent, 2026-10-15), all my call: rows grow with a wrapped text (#143); on the FP5 the Bar/Line toggle stays hidden and Save writes the stored kind back (#139); PreviewPill stays for the Volume card (#145); GTK marks like presets, with check, accent and selected state (#189); rebuild via `refresh_guided_selected_row` with focus restored (#189); a cancelled slider drag resends `value-changed` only, never `released`, so nothing saves and no preview starts (#144).
- Every TEST here reads source text: the code is Slint markup and Android-only `ui.rs`, which can't run on the test host. Layout scenarios are MANUAL.

## Hand-over
Artifacts: laptop Flatpak installed 2026-10-15 10:56 (compiled from this working tree); debug APK `meditate-android/android/app/build/outputs/apk/debug/app-debug.apk` built 2026-10-15 10:58 (after R4). Final run: 1745 passed, clippy clean on host and CI form (Android target clean before R4-R7, which only added `y: 0` and comments), check-specs green.

Test steps (Janek):
Laptop:
1. Guided, Starred Files: tap one starred file, then another. The check, accent title, moves to the tapped row; only one row has it. Open File (not imported): no row has a check. A starred local file picked from the Local Folder page: its row has the check.
Phone (FP5):
2. Vibration: open a custom Line pattern with soft points (or bundled "Wave", which opens as a copy), change only the name, Save. Open it again: same soft curve, still Line.
3. Vibration editor: type 12 into Points and tap Save right away: reopen, 12 points. Type 1.0 into Duration of a long pattern and tap Save right away: reopen, no more points than 1.0 s allows.
4. Setup: start a vertical swipe on a bell's volume slider (not at its current value) and scroll the page. The slider shows its old value again; no bell plays.
5. German or Russian, portrait: Keep Screen Awake and Cues during phases switches fully visible, texts in full. A preset with a long subtitle: delete button visible.
6. Sound chooser and pattern chooser: rows show check, play, edit (pattern: edit; sound: rename), delete as icons; a long name shows in full. Play turns into pause while it plays; tap stops it.
7. Landscape: open the interval-bell editor and the vibration editor; every row can be scrolled to. In the interval-bell editor the volume slider still drags sideways.
8. Label expander (Setup, Done, Log edit): with a long label name it shows in full; switching it off slides the row up (no squeeze toward the middle).
9. Guided, Starred Files: the selected file's row shows a check; tap another, the check moves; a transient Open File pick shows none.

Proposed commit message:
    android, gtk: Editors keep what you set, rows fit, the picked file is marked (batch M)

    The FP5's vibration editor rounded a stored pattern on open and
    synced it flat, and Save could miss a typed Duration or Points.
    A volume drag the page took over left the slider on a value that
    was never saved. Long row texts pushed switches and buttons off
    screen, the chooser rows' text buttons left no room for names, and
    two editors couldn't scroll in landscape. Starred Files now marks
    the selected file in both apps.

    #147, #148 and #160 moved to their own batches (Z, Y).

    Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
    Claude-Session: https://claude.ai/code/session_012WGZUPYHL7TurMz6YSFVJs
