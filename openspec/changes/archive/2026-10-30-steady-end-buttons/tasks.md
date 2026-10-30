# Tasks: steady-end-buttons

Base: cbc616651b777a601e2fbc0edf9298f209c00c1c; suites on it: core 1425, android 274, gtk 164, all green

## Loop
- [x] 0 Skills loaded, again after every /compact: `/ponytail ultra`; `slint:slint`
- [x] 1 Understand
- [x] 2 Propose (approved: 2026-10-30)
- [x] 3 Build
- [x] 4 Review, then `/simplify` after the last review
- [x] 5 Specs, docs, list (no other doc names these buttons; nothing in OPEN-ISSUES)
- [x] 6 Verify and hand over (Janek tested on the FP5 and the laptop 2026-10-30: "Works")
- [x] 7 Ship

## Review Focus
- Paused during preparation and ended unsaved: one button in the row -> "The buttons follow the session"
- A short screen (phone landscape, small laptop window) with the note and the keyboard -> the page still scrolls; MANUAL "How it looks"
- Box Breath's running page (square above the buttons) -> same controls on both pages; MANUAL "How it looks"
- Discard's confirm-on-note and Esc/Space handling -> unchanged code paths; existing scenarios
- A long translation of the Add label -> it still wraps; "+" shortens it in every language
- An open snapshot from before the update (recovery) -> no data change; buttons come from the session state

## 1. Core button set
- [x] 1.1 Rewrite `the_end_buttons_follow_the_session` for a first button, an optional second and the note; no Discard in the row. Files: meditate-core/src/session/mod.rs. Proof: FAIL then PASS
- [x] 1.2 `EndButtons { first, second: Option<EndButton>, note }`, `ENDED` = [Save] + note; `end_buttons` per the scenario. Files: same. Proof: 1.1

## 2. GTK
- [x] 2.1 `the_session_buttons_keep_their_place`: end controls always show Discard (no `discard.set_visible`), the second row button hides when there is none, the note is the end child of a vertical CenterBox whose centre holds the time and the buttons, on both running pages. Files: meditate-gtk/src/window/imp.rs, src/timer/imp.rs. Proof: FAIL then PASS
- [x] 2.2 Build it: refresh hides `second` on None; clicks map the row; Discard only from its own button. Proof: 2.1, `the_running_page_is_the_only_end_screen` (timer/mod.rs) still green
- [x] 2.3 `the_add_button_says_that_it_saves` expects "+ 03:00 and Save"; msgid `+ {time} and Save` via `build-aux/update-translations.sh`, nine translations. Proof: FAIL then PASS, `cargo test -p meditate i18n::`, msgfmt --check

## 3. Android
- [x] 3.1 `the_session_buttons_keep_their_place`: the Discard EndButtonView is not behind an `if`, the right button is shown only when there is one, the time and buttons sit in one block centred by padding and the note follows it outside the block. `the_running_page_is_the_only_end_screen` follows the new set. Files: meditate-android/src/app.rs. Proof: FAIL then PASS
- [x] 3.2 Build it: main.slint running stack and row; ui.rs `refresh` sets left, right (or none) and note; `end-discard-below` goes. Proof: 3.1
- [x] 3.3 `the_add_button_says_that_it_saves` expects `@tr("+ {} and Save", t)`; nine .po files. Proof: FAIL then PASS, msgfmt --check

## Notes
- Pre-build check, answered (my calls):
  - `EndButton::Discard` leaves core: `end_buttons` never returns it and GTK's arms for it would be dead. Android keeps code 4 for its own Discard view and handles it in the tap handler; `end_button_from_code` returns `Option`, an unknown code does nothing (no destructive fallback).
  - Android "no second button" is code -1, the default of `end-right`.
  - GTK on a window too short for the note: superseded by Janek's answer after Review 3, it scrolls like Android.
  - Android centres the block in the area between the system bars, as today; the keyboard only scrolls.
  - Box Breath's eyebrow, counter and square are in the centred block; only the note is outside.
  - `RunningControls.discard` goes once nothing reads it; paused in preparation shows [Resume] alone, centred.
- Review 1:
  - CRITICAL held: the hidden note dropped out of the column, so the spacer alone expanded and the block sat at the bottom while running. Fix: the note lives in an always-visible holder; spacer and holder share the size group. The guard now requires the holder and forbids the note itself in the group. (First tried a spacer in a size group instead of its CenterBox; Review 2 showed that drops the block on short windows.)
  - MINOR 2, 3, 4 held: the Slint property comment, the Box Breath comment and the leftover first line of the Add label's comment are fixed.
  - "tapped => {}" without the int: the android suite compiles main.slint, and it built.
- Review 2 (fix review): CRITICAL held: the spacer's minimum is the note's height too, so a short window dropped the block on Pause. A throwaway spike measured both layouts in real GTK windows (content 230 px, note 220 px): CenterBox with an expanding end child keeps the time at the same y running and paused in a 1200 px window with the note right under it, and lifts it (never drops it) at 650 and 400 px; the spacer dropped it by 52 and 177 px. Switched to the CenterBox, spike deleted. MINOR held: the guard now requires the end child's vexpand. Its suggested reserved-note Stack would leave an empty strip while running; not taken, the approved short-window lift stands.
- Review 3 (fix review of the CenterBox): IMPORTANT held: CenterBox still lifts the block unless the room under the centred block holds the whole note, so the default 700 px window lifted Timer about 30 px and Box Breath about 120 px. Asked Janek: "Wie am Handy: scrollen". Now a spacer's height is set from the visible page height (vadjustment page-size notify) and the time-and-buttons height, never from the note, with weak refs so the page is not kept alive. A throwaway spike measured it in real GTK windows (content 230 and 420 px, note 220 + 18 px; windows 1200, 700, 400 px): the time's y was the same running and paused in all six, and the page grew to scroll where the note did not fit. Spike deleted. MINOR (orientation check matched the wrong builder) is moot, the CenterBox is gone.
- Review 4 (fix review of the spacer): PASS. MINOR 2 (the 48 repeated the margins) and 4 (blank lines at the end) fixed. MINOR 1 accepted with a ponytail comment: the spacer follows the window height only, so a label that wraps later grows the block downward, off centre by half a line; nothing moves. MINOR 3 (source guard cannot check the formula) stands: the GTK UI tests here are source guards, the spike measured it, and MANUAL "How it looks" covers it.
- /simplify: efficiency and altitude clean. Applied: core `EndButtons::row()` so GTK's refresh and click share the row's shape; Android's running padding is one named `pad` instead of 24px and a hidden 48px; the `end_button_from_code` doc says "unknown code". Skipped: Discard as a bare WrapButton (the dark-roles guard forbids bare WrapButtons on the session screen, and EndButtonView's comment documents kind 4 as Discard's look); an `iter::once` form for the GTK row (row() covers it); a const for the GTK margins (the closure reads the column's own margins); test-helper reuse in window/imp.rs (that module slices by hand; the timer file's refresh is found before its tests); the brace-matching test simplification (the block's end is what proves the note sits outside it); a LayoutManager for GTK (more code; the drift is already ponytail-commented).

## Hand-over
- Verified: core 1425, android 275, gtk 165 green; clippy CI form, GTK all-targets and Android target clean; check-specs green. APK meditate-android/android/app/build/outputs/apk/debug/app-debug.apk (2026-10-30 16:04); GTK meditate-gtk/builddir/src/meditate (16:05), started with a scratch data dir.
- Test steps (phone and laptop, Timer and Box Breath, also in German):
  1. Start a session: the row shows only Pause, Discard sits under it.
  2. Pause: Resume and Save appear in the row, Discard stays where it was, the time and buttons don't move, the note comes in below.
  3. Resume, let the time run out: Save and "+ 00:05 und speichern" in the row, Discard unchanged.
  4. Laptop: make the window short and pause again: nothing moves, the page scrolls to the note.
  5. Pause during the preparation countdown: only Resume, Discard under it.
- Commit message (proposed):
  timer: Keep the session buttons in place

  Discard now has its own line under the row in every state instead of
  moving from the row to below it when the session pauses, and the time
  and buttons stay put when the note appears; the note hangs below and
  a short screen scrolls. The Add button reads "+ 03:00 and Save".
