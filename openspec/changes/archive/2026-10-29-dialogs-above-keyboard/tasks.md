# Tasks: dialogs-above-keyboard

Base: 2dc28d8; suites on it: core 1395, android 271, GTK 164 green (run
on this tree before the 2dc28d8 commit).

## Loop
- [x] 0 Skills loaded, again after every /compact: `/ponytail ultra`; `slint:slint`
- [x] 1 Understand (#11 read; part 1 confirmed fixed on the FP5 by Janek; say-back confirmed: "kannst mit 2 weitermachen")
- [x] 2 Propose (fresh-agent check done, answered in proposal decision 3; Enter in number fields added by Janek; approved: "go" 2026-10-29)
- [x] 3 Build (android 272 green; the new test failed first on the missing KeyboardArea; Android-target and CI clippy clean; two older checks follow the dialog ids)
- [x] 4 Review, then `/simplify` after the last review (review, fix review, /simplify; see Notes)
- [x] 5 Specs, docs, list (check-specs green; no doc names these dialogs; no OPEN-ISSUES entry for #11)
- [x] 6 Verify and hand over (FP5: Janek 2026-10-29 "klappt alles" on the last build; android 272 green; CI and Android-target clippy clean; check-specs green; APK 23:03:19 installed; )
- [x] 7 Ship (delta appended to openspec/specs/text-input, archived 2026-10-29, check-specs green)

## Review Focus
- A card taller than the room above the keyboard (Duration in landscape, a long German title): it starts at 16 px and its bottom may still sit under the keyboard; a scroll area is out of scope.
- The keyboard closing while a card is open: the card moves back to the middle (the binding follows).
- A floating or split keyboard: Slint reports size 0 for a floating one, so the card stays centered.
- Safe-area insets: the cards are full-window overlays at (0, 0), so the keyboard's top is in their coordinates.

## Review (fresh agent)
PASS, five MINOR:
1. Fixed: a card taller than the room kept its top at the status bar and pushed its buttons under the keyboard; now the buttons stay above the keyboard first (`max(0px, min(top, bottom - h), (bottom - h) / 2)`).
2. Kept, to check on the FP5: `KeyboardArea.hide()` on Enter and on every dialog closing may be redundant (Slint 1.18.1 disables the IME on focus out and on deinit); ui.rs's note says it wasn't reliable before. If the device shows the keyboard closing without it, a later cleanup can drop it.
3. Not changed: a dialog opened while a page field (Edit Session notes) has focus leaves that keyboard up; already so before this change.
4. Fixed: the proposal and tasks named `Keyboard`/"next field" (now `KeyboardArea`, and Enter in Duration takes the number).
5. Noted: source checks can't prove positions; the FP5 scenario does.

Fix review (fresh agent, fix 1): one MINOR, handled in the spec: a card taller than the whole room above the keyboard (a name dialog in landscape) starts at y = 0 with its buttons under the keyboard. Kept: dropping the 0 floor would push the title and field off the top instead; a scrollable card is out of scope. The spec scenario and the comment say so.

/simplify (four agents), applied: one `KeyboardArea.card-y(h)` instead of eight copies of the formula; the import field's Enter gate is the button's (`guided-import-valid`); the `on_hide` registration moved out from under the Bluetooth comment. Skipped: one `ModalCard` component for all eight cards (a refactor of every dialog, its own change); turning the goal card into a `ConfirmDialog` (width and body order would change); dropping the number fields' `KeyboardArea.hide()` (the FP5 test decides).

## Tasks
- [x] 1.1 `main.slint`: `global KeyboardArea { top; bottom; callback hide(); }` fed by MainWindow from `virtual-keyboard-position/-size`; every modal card's `y` from it (not the slider's three centred bars); `Keyboard.hide()` (Rust: `hide_soft_keyboard`) on the number fields' Enter and whenever `modal` becomes none; `ConfirmDialog.accept()` and `accepted` on the eight single fields. Proof: `dialogs_center_above_the_keyboard`, FAIL then PASS.
- [x] 6.1 FP5 (spec scenario "On the FP5").

## Device test 1 (2026-10-29)
Janek on the FP5: everything works, except that Create Preset opens without its field focused, so the keyboard needs a tap first. Added as a scenario ("The field is ready to type"), test first (the count of `init => { self.focus(); }` failed, then passed): the six name dialogs, Track name and Daily Goal focus their field when they open.

Janek re-tested: "funktioniert". Fix review (fresh agent): IMPORTANT, fixed: the six pre-filled fields (Rename Label, Rename Preset, Rename Sound, Rename Track, Track name, Daily Goal) opened with the cursor before the text, so typing "30" into a goal of "20" gave "3020"; they now select their text on open, as GTK does (test first, each dialog checked on its own, which also covers the review's MINOR on a file-wide count). To check on the FP5: whether the keyboard comes up when the import dialog opens right after the file picker.

Janek re-tested the select-all build: "klappt alles" (renames, goal, the import after the file picker). Fix review (fresh agent): PASS; MINOR, kept: a selection made in code shows Android's cut/copy/paste bar until the first keystroke (Janek tested it and was fine with it); MINOR, fixed: the test anchored the import field on a label that the guided rename dialog shares, now on `guided-import-text`.

/simplify on the focus fixes (four agents): reuse, efficiency, altitude clean; applied: the test checks that each field's own `init` starts with the line (no 60-character window). Then, at Janek's word ("mach das ruhig"): all eight fields use one line, `init => { self.select-all(); self.focus(); }` (empty fields select nothing; selecting first hands the keyboard the selection with the focus in one message); the test checks one list of eight anchors; needs another FP5 look.

## Hand-over (step 6)
APK: `meditate-android/android/app/build/outputs/apk/debug/app-debug.apk` (23:03:19, one focus line for all eight), debug, `adb install -r`.

FP5 steps, with Gboard:
1. Presets, Create Preset, type a name: title, field, Cancel and Create stay visible above the keyboard.
1b. The field is focused and the keyboard up as the dialog opens.
2. Enter on the keyboard: the preset is created, the dialog and the keyboard are gone. With an empty or taken name Enter does nothing.
3. The same for Rename Preset, a label (Create/Rename), Rename Sound, the Daily Goal (Enter sets it, out of range nothing).
4. Duration (Timer) and the countdown: the dialog sits above the number keypad; Enter takes the number and closes the keyboard, the dialog stays.
5. Interval editor, bell time and jitter, and Preferences, lead-in length: Enter takes the number and closes the keyboard.
6. Any dialog closed by Cancel, a tap outside or Back while the keyboard is up: the keyboard goes too.
7. Without the keyboard every dialog sits in the middle as before.

Commit message:
```
android: Keep dialogs above the keyboard, Enter confirms

The keyboard hid half of a name dialog with its buttons, and Enter did
nothing, so a preset name needed Back and then Create. Every dialog now
centers above the keyboard, Enter confirms a single-field dialog while
its button is enabled, Enter in a number field takes the number and
closes the keyboard, and the keyboard closes with every dialog. A
name or number dialog opens with its field focused and its text
selected, as on Linux. The name no longer staying selected while
typing came with the Slint 1.18.1 text-field fixes already on beta.

Fixes #11
```
