# Tasks: one-dialog-frame

Base: c74d4c5; suites on it: core 1540 + 4 + 11 + 3, Android 300, green (GTK untouched by this change; 176 green on the same tree before c74d4c5 was committed).

## Loop
- [x] 0 Skills loaded, again after every /compact: `/ponytail ultra`; `slint:slint` (Android UI touched)
- [x] 1 Understand (say-back approved 2026-11-11 "go"; #470 dialogs stay, Goal number field, Prep steps 5)
- [x] 2 Propose (approved: 2026-11-11, "go", with − / + snapping to fives)
- [x] 3 Build (Android 301 green)
- [x] 4 Review: five rounds (reviewer + `/code-review high` each; round 5 minor only), then `/simplify`
- [x] 5 Specs, docs, list (check-specs green; Android 301, GTK 177, both clippy runs clean)
- [x] 6 Verify and hand over (Janek: all device and laptop tests passed, 2026-11-11)
- [x] 7 Ship (deltas merged, archived)

## Review Focus
- The filter sheet stays `visible`-gated (its DropDownMenu loses its text when rebuilt) -> `every_dialog_has_one_frame` checks it uses the frame with `visible:`, not `if`.
- The name dialog's scrim must not cancel while converting -> its frame cancel handler keeps the busy check; `every_dialog_is_one_modal` and `dialogs_center_above_the_keyboard` stay green.
- A number typed but not committed when Set is tapped -> Set still calls `commit()` on each field first (kept), and the live value makes Set's on/off follow the typing.
- An empty field after the tap that clears it -> the value keeps its last number, Set stays as it was; blur restores the text (unchanged).
- Edit Session's Custom Time keeps hours past 23 and writes the edit pair -> the Slint branch and `max-value` are kept as they are.
- Another language: no new strings; one unused msgid leaves nine `.po` files; `msgfmt --check` each.
- Back while a dialog is open: unchanged (`modal`), no scenario.

## 1. One frame (#465, #474)
- [x] 1.1 `DialogFrame` (scrim with `cancel()`, one card at `KeyboardArea.card-y`, width `min(340px, root.width - 32px)`, radius 28, swallow TouchArea, 24 px padding, wrapping 22 px title, `@children`); ConfirmDialog built on it; the eight hand-made dialogs moved onto it. Files: `meditate-android/ui/main.slint`, `src/app.rs` (update `simple_dialogs_share_one_card` into `every_dialog_has_one_frame`, adjust counts in `dialogs_center_above_the_keyboard`). Proof: `every_dialog_has_one_frame` (one `KeyboardArea.card-y` card, no `if root.modal == Modal.x : Rectangle {` left, every 22 px title wraps, the filter sheet `visible`-gated). Expected: FAIL then PASS.
  Result: FAIL (no DialogFrame) then PASS. ConfirmDialog inherits DialogFrame and passes `@children` on; the name dialog's `cancel()` is now `close()`. Also adjusted: `a_back_swipe_does_not_tap_a_backdrop` (one scrim), `dialogs_center_above_the_keyboard` (one card; Goal out of the Enter list), `bell_sounds_can_be_renamed_and_custom_ones_deleted`, `destructive_buttons_are_red` (indent).

## 2. Time dialogs (#475, Goal, Prep step)
- [x] 2.1 VerticalSpinBox: `step` (default 1) for − / +, snapping to the next multiple of it, and the value follows the typing (`edited`, clamped); Custom Time, Preparation Time (step 5) and Daily Goal (1..1440 from `goal-min/max-mins`) as ConfirmDialog with "Set"; Custom Time's `confirm-enabled` is `dialog-hours > 0 || dialog-minutes > 0`; `goal-valid`, `set-goal`, `goal-dialog-text` and the Goal TextField go; Goal's seeding becomes an int. Files: `main.slint`, `src/app.rs`, the nine `lang/*/LC_MESSAGES/meditate-android.po` (drop "Minutes ({}–{})"). Proof: `the_time_dialogs_are_number_fields`. Expected: FAIL then PASS.
  Result: FAIL (no `step`) then PASS; "Minutes ({}–{})" removed from the nine `.po` files, `msgfmt --check` clean (only the old header warnings).

## 3. Docs
- [x] 3.1 REFACTORING: #465 done, P4 row done, #470 to "Considered and rejected" (maintainer 2026-11-11), Duration routing added to #469. OPEN-ISSUES: #474, #475 out. check-specs.

## Notes
- Pre-build questions (fresh agent, 2026-11-11), my calls:
  - Set keeps its look: ConfirmButton draws like TextButton (primary text, no fill), only red when destructive.
  - The subtitles ("Hours : Minutes", "Seconds", "Minutes per day") stay above the field as the first child, `body` empty.
  - Daily Goal keeps the spin box's zero padding ("05") and 80 px; checked at 1440 on the phone, widened only if cut.
  - Label conflict's outside tap stays "decide later" (no Keep both).
  - − / + clamp at the limits. Maintainer (2026-11-11): − / + snap to the next multiple of the step (typed 13: + gives 15, − gives 10); a typed number itself stays.
  - The name dialog's `cancel()` function is renamed to avoid the frame's callback; Goal gets its own int dialog property, seeded on tap, so Cancel keeps the old goal.
  - If a component on a base with `@children` cannot pass children on, ConfirmDialog holds a DialogFrame as a child instead of inheriting.
- Pre-existing, not this change: `drop_files_take_one_path_and_report_failures` has an unused `body` closure (a test-build warning, already at c74d4c5).
- Review round 1 (reviewer + `/code-review high`), fixes:
  - F1 (reviewer) Linux closed Custom Time silently at 0:00 too: maintainer said fix it here. `custom_time_dialog` builds the dialog (moved out of `show_custom_time_dialog`), Set off while both spins are 0 (weak refs, no cycle). New requirement "Custom Time cannot be set to 0:00", test `custom_time_cannot_be_set_to_zero`, FAIL then PASS.
  - F2 (code-review) ConfirmButton's accessible action clicked a disabled button (TalkBack could set 0:00): guarded by `enabled`; assert in `the_time_dialogs_are_number_fields`, FAIL then PASS.
  - F3 (code-review) VerticalSpinBox comments stale; F4 the `commit()` calls redundant now that `edited` takes the typed number (IME commits fire `edited`, `i-slint-core/items/text.rs:1277`): `commit()`, its four call sites and the pre-step in − / + removed, comments rewritten.
  - F5 (reviewer) living stats scenario still named the hint and "0 is refused": MODIFIED delta, a typed 0 becomes 1.
- Rejected, shown to the maintainer:
  - Goal clamps a typed 0 or 2000 instead of keeping Set off (reviewer): the number field clamps, as Linux's spin row does for the same goal; Set then stores the clamped number. Spec says so.
  - Tests only read the Slint source (both): the crate has no Slint runtime on the host and adding one touches Cargo.toml (protected); the arithmetic is covered by the FP5 MANUAL scenario. Same as every Android layout test here.
  - 1440 in an 80 px field (code-review): about 70 px of digits at 32 px; checked on the FP5 (MANUAL scenario names 1440), widened only if cut.
  - A failed goal write only logs (code-review): older, already OPEN-ISSUES #427.
- Review round 2 (reviewer + `/code-review high` on the round-1 fixes), fixes:
  - G1 (both) Linux's gate followed only committed values; a typed 5 left Set grey until Tab: the gate reads each spin's text (`connect_changed`). Test extended (`set_text`), FAIL then PASS.
  - G2 (code-review) Android: a field typed into and emptied kept the half-typed number (Prep 30, "4" then delete: Set stored 5): an emptied field goes back to the number it showed. Slip: code written before its assert; the assert was then proved by breaking the code (FAIL), restored (PASS).
  - G3 (code-review) Custom Time's `confirm` re-checks 0:00 and stays open, a backstop for any path past the grey button.
  - G4 (reviewer) unused ids `hours-spin` etc. dropped; G5 (code-review) the new tests use the module's `slint_code()` / `component()`.
- Rejected in round 2, shown to the maintainer:
  - "0:00 invalid" belongs in core (code-review): one comparison of two numbers each dialog already holds; a core function adds a call in each app and saves nothing.
  - GTK's `if total_mins == 0 { return; }` (code-review): unreachable now that Set follows the typed text; kept as the backstop that no 0:00 is written, like Android's.
  - "Converting… {}%" has a "…" (code-review): only re-indented, and REFACTORING #467 records it as the accepted progress form.
- Review round 3 (reviewer + `/code-review high` on the round-2 fixes), fixes:
  - H1 (reviewer, IMPORTANT) Linux's gate counted an emptied spin as its old number, while GTK commits empty as 0: the gate counts empty as 0 (Android's emptied field goes back to its number; each follows its own toolkit's commit). Test: `set_value(10)` then `set_text("")` with hours at "0", FAIL then PASS.
  - H2 (code-review) Linux's Set reads `value()` without taking typed text: `update()` on both spins first, so the backstop is unreachable. One line each, no test (the handler needs a running TimerView).
  - H3 (code-review) the gate summed both fields ("-1" and "1" gave 0): each field on its own (`> 0 ||`).
  - H4 (code-review) Android's backstop repeated the rule: `if (!self.confirm-enabled)`; asserted in `the_time_dialogs_are_number_fields` (reviewer: the backstop was unasserted).
  - H5 (code-review) `commit-text()` had one caller left: inlined.
- Rejected in round 3, shown to the maintainer:
  - A typed 3 in Prep or 30 in hours stores the clamped 5 or 23 while the field reads 3 or 30 (code-review): the same clamp as Linux's spin rows, decided in round 1.
  - The tests match exact indentation (code-review): the convention of every Android layout test here.
- Review round 4 (reviewer + `/code-review high` on the round-3 fixes), fixes:
  - I1 (reviewer) H2 and H3 had no test: Set's arithmetic moved into `custom_time_minutes` (takes typed text, `None` at 0:00) with asserts in `custom_time_cannot_be_set_to_zero` (typed 5 gives 5, FAIL then PASS); the "-1 and 1" assert proved by putting the sum back (FAIL), restored.
  - I2 (code-review) Daily Goal lost its screen-reader name with the old field's label: VerticalSpinBox `name` as the number's `accessible-label`, Goal "Minutes per day", Prep "Seconds" (existing strings). FAIL then PASS.
  - I3 (code-review) the number's format was written twice: the text is set once by `sync-text()` (from `init`). Asserted in the same test, FAIL then PASS.
  - Found, not fixed here: Custom Time's two numbers had no screen-reader name before this change either; "Hours" and "Minutes" are not Android strings yet, so naming them is new UI text: OPEN-ISSUES.
- Rejected in round 4, shown to the maintainer:
  - An emptied Android field keeps its number while it shows blank (code-review): tapping a field empties it by design, so "empty = 0" would grey Set on every tap; blur shows the number again.
  - ConfirmDialog builds its buttons twice (code-review): REFACTORING #461, P12.
  - Repeats of rejected points (clamp shown vs stored, the GTK backstop, the core rule, the "…", 1440 width): reasons above.
- Review round 5 (the last: reviewer + `/code-review high` on the round-4 fixes), fixes:
  - J1 (reviewer) the `init` that writes the number on open was unasserted: asserted.
  - J2 (code-review) blur re-ran `take-typed()` though `edited` already took every keystroke: blur only writes the number back. J3 StepperRow's comment still described the old VerticalSpinBox: reworded.
- Rejected or deferred in round 5, shown to the maintainer:
  - A stored goal above 1440 (from another device) is written back unchanged by Set, where the old dialog greyed Set (code-review): no value changes; the missing cap is OPEN-ISSUES "daily goal cap" (P15).
  - StepperRow could follow the typing the same way and lose `commit()` and its five call sites (code-review): REFACTORING "Deferred refactors".
  - The gate and Set parse the spin text differently (code-review): the spins take digits only, so both read the same number; round-5 reviewer agrees.
- Round 5 left only minor findings: no sixth round.
- `/simplify` (four angles, after round 5): fixed: the grey-Set backstop moved from Custom Time into ConfirmDialog's two buttons (every gated dialog gets it; asserted in `the_time_dialogs_are_number_fields`); a `subtitle` on ConfirmDialog replaces the three hand-styled unit lines; the name dialog's focus assert without its one-use closure. Skipped: dropping `root.shown = root.value` on focus (keeps the field right if the value changes while unfocused). Reuse and efficiency: none. Deferred: REFACTORING #516 (StepperRow follows the typing).
- Docs: REFACTORING P4 done, #465 and #470 out (#470 to "Considered and rejected", and the "commit on every keystroke" entry now notes VerticalSpinBox), Custom Time's routing added to #469, #516 deferred; OPEN-ISSUES #474, #475, the #470 question out, #517 (Custom Time's numbers unnamed) added to LL.

## Hand-over
- Artifacts: GTK `meditate-gtk/builddir/src/meditate` (13:43), debug APK `meditate-android/android/app/build/outputs/apk/debug/app-debug.apk` (13:44). Laptop test app runs with `XDG_DATA_HOME=~/.cache/meditate-dialog-test` (log `~/.cache/meditate-dialog-test.log`).
- Laptop: Custom Time (Timer, Duration row, "Custom"): set 0:00 with −, Set grey; type 5 in minutes (no Tab), Set on; delete the 5, Set grey; 0:05 and Set sets 5 min.
- Phone (German):
  1. Filter (Log), Custom Time, Preparation Time, Daily Goal, Delete Preset, Sync status: same width, titles in full, buttons as before; a tap beside the card closes it.
  2. Custom Time at 0:00: Set grey; tap minutes, type 5: Set on; Set gives 0:05. Also from Edit Session.
  3. Preparation Time at 15: + 20, − 10; type 13, + gives 15; − stops at 5, + at 300.
  4. Daily Goal: − / + by 1; type 1440: fits in full; Set stores it; type 0: becomes 1; Cancel keeps the old goal.
  5. Name dialog (Create Preset), Recovery if reachable, Label conflict if reachable: look as before.
- Proposed commit: see the chat of 2026-11-11.
