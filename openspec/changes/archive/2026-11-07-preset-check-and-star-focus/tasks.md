# Tasks: preset-check-and-star-focus

Base: 4e4d041a7512d3241f4a37fa8601ee18229603cc; suites on it: core 1481 + Android lib 296 green, GTK 171 green

## Loop
- [x] 0 Skills loaded: `/ponytail ultra`, `slint:slint` (loaded by the gate; no Android UI touched)
- [x] 1 Understand (confirmed 2026-11-07: "go")
- [x] 2 Propose (approved 2026-11-07: "go"; pre-build questions: 2, both my call, see Review Focus)
- [x] 3 Build (core 1482, Android 296, GTK 171 green; clippy GTK+core clean)
- [x] 4 Review (R1 one agent, fix review PASS), then `/simplify` (see below)
- [x] 5 Specs, docs, list (check-specs green; no doc names the changed code; BB ticked, #211-#213 out)
- [x] 6 Verify and hand over (maintainer 2026-11-07: "Rest passt"; #213 not testable with the scratch app)
- [x] 7 Ship (deltas appended to openspec/specs/presets and guided)

## Review Focus
- A preset from an older version with label {enabled: false, uuid: "x"} -> same rule, ignored while off; covered by the scenario's rule
- A label switched on in both preset and setup -> uuid still compared (existing `the_active_preset_is_the_one_matching_setup` stays green)
- The label deleted behind a switched-off switch -> off, check stays; same rule
- Box Breath mode -> same `shown` function for both modes; the test runs Timer, the code has no mode branch
- #212: a length read or scan rebuilding the page while a star has focus -> same refocus path
- #212: the focused file vanishes in the rebuild (folder rescanned) -> no row of that name, focus stays where GTK puts it
- #212: focus on the Folders/List toggle or Sort row -> they are rebuilt too; out of scope (not reported), name it in the hand-over
- #212: a star clicked, the list scrolled away by wheel, then a length read rebuilds -> the scroll value is restored, not the star scrolled into view (pre-review Q1, my call)
- #212: the row itself focused (keyboard) -> the row gets the focus back, not its star, so Enter still plays (pre-review Q2, my call)
- Another language -> no new strings

## 1. Core
- [x] 1.1 `active_presets`: in `shown`, a switched-off label drops its uuid. Proof: `a_preset_saved_with_the_label_off_keeps_its_check` (Expected: FAIL, then PASS).

## 2. GTK
- [x] 2.1 `local_folder.rs`: name each file row by its id and each folder row by its path; `fill_page` remembers which row or star had the focus, turns off the auto viewport's scroll-to-focus while it swaps the page and hands the focus back to the same widget of the new row. Proof: MANUAL scenario (device test).

## 3. List
- [x] 3.1 OPEN-ISSUES.md: #211, #212, #213 out, batch BB ticked.

## Review R1 (one agent: spec PASS, quality NEEDS FIXES)
- [x] R1.1 IMPORTANT: other rows (folder rows, the Folder row, empty row) all carry GTK's default name, so a focused folder row sent the focus to the first row on the page. Fixed: only rows named `file-` are refocused.
- [x] R1.2 MINOR: the scroll was restored only when the refocus worked. Superseded by R1.3.
- [x] R1.3 MINOR: an idle restore of the scroll value may run before the layout. Fixed differently: the auto viewport's scroll-to-focus is off while the page is swapped and the focus is handed back, so no focus move scrolls the list; no saved value, no idle.

## Fix review (one agent: PASS)
- [x] F1 MINOR: a focused folder row still lost its focus and the list jumped on a length read. Fixed: folder rows are named `folder-<path>` and refocused too.
- [x] F2 MINOR: task 2.1 still described the dropped scroll restore. Reworded.

## /simplify
- [x] Applied: the focused widget's own name replaces the row/star flag (one lookup, no branch); `if let` instead of `Option::iter` for the viewport; test helper `active_names` shared by the two active-preset tests.
- Skipped: move `focused_row_name`/`refocus_row` from timer/imp.rs into a shared helper (different shape: flat row lists there, a row-or-star tree here; touches code outside the change).
- Skipped: remember the matching row while building instead of the tree walk (runs only when the focus was in the list; more plumbing than it saves).
- Skipped: let the star flip its own icon and skip the rebuild (length reads and scans rebuild the page too, so the focus keeping is needed anyway; other pages not reported).
- Suites after: core 1482, Android 296, GTK 171 green; clippy GTK+core clean.

## Hand-over
- Artifact: `meditate-gtk/builddir/src/meditate` (built 2026-11-07 22:33), run with `XDG_DATA_HOME=~/.cache/meditate-starorder/data LOCALEDIR=meditate-gtk/builddir/po`; test folder `~/.cache/meditate-starorder/folder` with 30 files in `Viele/`.
- Test steps:
  1. #211: Timer, label off, save preset A; pick a label, switch it on, save preset B; apply B, then A. A shows its check and highlight.
  2. #212: Guided, Local Folder, `Viele`: scroll to the bottom, click a star. The list stays put. Then Tab to another star and press Space: same. In the List view: same. (Dropped from the steps: a focused file row during a background rebuild by a length read; it can't be triggered on purpose by hand. Maintainer: the step as first written made no sense, since a star click moves the focus to that star.)
  3. #213: needs the beta app with a connected account (the test app has none); not testable with the scratch data.
- Commit message (proposed):
  presets, guided: preset check with the label off, star keeps the place

  A preset saved with the label off lost its check once a label had been
  chosen, because the label kept behind the switch was compared (#211).
  On Linux, starring a file on the Local Folder page rebuilt it and the
  focus move scrolled the list to the top; the focus now goes back to
  the same row or star without scrolling (#212). #213 was already fixed
  on beta (66b188d).

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_012WGZUPYHL7TurMz6YSFVJs
