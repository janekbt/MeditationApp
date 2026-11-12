# Tasks: number-field-tap

Base: 2938b977b2846fca6f499b4b268e89a6c1c6ece1; suites on it: GTK 178 green (run 2026-11-12); core 1551 and Android 295 green on the same commit at P5-c's ship (core untouched here)

## Loop
- [x] 0 Skills loaded, again after every /compact: `/ponytail ultra`; `slint:slint` (main.slint changes)
- [x] 1 Understand (said back and confirmed 2026-11-12; Janek then chose select on Linux, empty plus pale old number on Android)
- [x] 2 Propose (approved 2026-11-12)
- [x] 3 Build (GTK 179, Android 295 green; GTK clippy all-targets and CI clippy clean)
- [x] 4 Review (five rounds; the round-5 fixes below had no sixth round, see there; /simplify done)
- [x] 5 Specs, docs, list (check-specs green; no doc names number-field behaviour; REFACTORING #519, #520; OPEN-ISSUES untouched, no item)
- [x] 6 Verify and hand over (laptop and FP5 passed 2026-11-12; Librem 5 not checked)
- [ ] 7 Ship

## Review Focus
- Mouse click: GTK places the cursor in the press handler after the focus moves; the selection must land after that (idle), and a release without motion must not undo it (laptop check).
- Touch on the Librem 5: GTK may place the cursor on release or show its text bubble on a tap; checked on the device.
- A spin row (libadwaita) wraps a GTK spin button: the handler matches the text inside any spin button, so both kinds (test uses both).
- Dialogs (Custom Time, Preferences, Log edit, Bell and Vibration editors) show inside the main window, so the main window's focus covers them; the second window (database error screen) has no number field.
- Arrow-key focus on a spin row itself (not its text): no selection, nothing changes.
- The window loses and regains focus: the focus widget does not change, so nothing is selected again.
- Android: the pale number shows only while the field has the focus and is empty; a screen reader still reads the field's name.

## 1. GTK
- [x] 1.1 `a_focused_number_field_selects_its_number` (spin button and spin row, real shown window; focus by `grab_focus_without_selecting`, which is how a click arrives). Expected: FAIL then PASS. Tab already selects (GTK's select-on-focus), so it is no scenario of ours. Ran: first passed for the spin button by accident (its text kept the selection from the window's first focus); the test now places the cursor after focusing, as a click does, and failed for both kinds; with the handler it passes. `is_focus` instead of `has_focus` in the idle: the test window is not active, and the field is the focus either way.
- [x] 1.2 Wire the handler on the main window in `constructed`. Proof: MANUAL scenarios on the laptop and the Librem 5.

## 2. Android
- [x] 2.1 VerticalSpinBox and StepperRow: the old number pale while focused and empty. Proof: MANUAL on the FP5 (slint-viewer is not installed; no render check). The existing source test `the_time_dialogs_are_number_fields` had its needle re-indented for the wrapper.

## 3. Docs
- [x] 3.1 check-specs.

## Pre-build questions (fresh agent, 2026-11-12, on the first plan), my calls
- The agent's flaw (GTK reads an empty field as 0 on focus loss, before the window reports the new focus) applied to emptying; with selecting, the field is never emptied by us, so it no longer applies.
- Preferences opens on the active window, which is the main window whenever the menu opens it.
- Tests use a shown window, so GTK sends real focus changes.

## Pre-build questions (fresh agent, 2026-11-12, revised plan), my calls
- Tab and `grab_focus` already select (GTK's `gtk-entry-select-on-focus`); only a click comes without a selection. The test focuses with `grab_focus_without_selecting` to model the click.
- Focus put back on a field when a dialog closes: GTK's grab already selects it today, so the handler adds nothing there. The Sync rows that deliberately focus without selecting (`preferences.rs:546`) are entry rows, not number fields; the handler only matches text inside a spin button.
- Android: the change is in the two shared components, so every number field gets it; the scenario lists them all.
- Slint: TextInput has no placeholder; the pale number is a Text declared after the input (draws on top), `accessible-role: none` so TalkBack reads only the field's name; VerticalSpinBox's input gets a wrapper with the same height.

## Review round 1 (reviewer agent + /code-review high, 2026-11-12)
Fixed:
- R1 (code-review): a click into a field that already has the focus with its number selected (Tab, a dialog's first field) cleared the selection; the focus handler never ran. GTK's source (gtktext.c 4.18.6, click press) confirms: a press inside a selection starts a drag and the release places the cursor; on a touchscreen it pops the copy bubble. Now a capture-phase click handler on the window empties the selection before GTK's press (so GTK just places the cursor, no drag, no bubble) and selects the number in an idle after. Test renamed `a_clicked_number_field_selects_its_number`, two cases (from elsewhere, already focused); broke the handler to skip a focused field: the second case failed.
- R2 (reviewer, code-review): the zero-padding was written twice in VerticalSpinBox; one `padded()` now. The source test "one place formats the number" follows.
- R3 (code-review): the pale number had no test; `interval_editor_numbers_let_go_of_focus` now checks both components carry it (broke StepperRow's condition: failed).
- R4 (code-review): the test's tree walk now uses the house `successors` idiom.
Rejected:
- Touch release undoing the selection (code-review): gtktext.c places the cursor on press, not release; the release only updates the touch handles.
- Click-and-drag in an unfocused field selects from the start of the number (both): 1 to 3 digit numbers; not worth a pointer-state check.
- A digit typed between the press and the idle gets selected too (code-review): needs a key event queued inside one main-loop turn after a click; not seen in practice.
- Each click claims the PRIMARY selection (code-review): Tab does the same today, GTK's own behaviour.
- TalkBack does not hear the old number (code-review): the vendored Android backend has no mapping for a placeholder (reviewer); the field keeps its name. 
- The cursor sits in the middle of the pale digits (both): device check on the FP5.
- Spec Purpose says Android only (reviewer): widened when the delta is merged at ship.

## Review round 2 (fix reviewer + /code-review high, 2026-11-12)
Fixed:
- F1 (reviewer, IMPORTANT): selecting right after the press stopped a second tap from bringing back a dismissed on-screen keyboard: GTK's click release (gtktext.c 3076-3090) calls `activate_osk` only while nothing is selected. Now a capture-phase legacy controller on the window empties the selection on the press and selects in an idle after the release, once GTK's release is done. The press and release logic are two functions (`number_pressed`, `select_after_release`) the test drives; it now also asserts the press leaves no selection (broke the clear: failed).
- F2 (code-review): a press inside a popover (its own surface) was mapped into window coordinates; presses from another surface are skipped.
- F3 (reviewer): the pale-number check also asserts its colour and its text (the old number).
- F4 (code-review): "one place formats the number" also asserts both writes go through `padded()`.
- F5 (code-review): the test's main-loop draining uses the house `while iteration(false)` idiom; the layout wait is bounded (500 short sleeps).
Rejected:
- The controller glue (event types, coordinates, primary button) has no host test (code-review): GDK events can't be made on the host; the device test covers it (laptop click, Librem 5 tap).
- Multi-touch shares one press slot (code-review): two fingers on a number field; not worth keying by sequence.
- A keyboard that composes digits draws them over the pale number (code-review): number keyboards commit digits directly; FP5 check.
- A failed assert leaves the test window shown (code-review): as in the other GTK window tests.
- The two pale-number blocks are copies (code-review): seven lines each, different text; a component adds more than it saves.

## Review round 3 (fix reviewer + /code-review high, 2026-11-12)
Fixed:
- G1 (code-review): a drag that selects part of a number was followed by select-all. First fixed with a distance check; replaced in round 4 (H1).
- G2 (code-review): a press whose release never comes (grab broken) left a stale field for the next release; GrabBroken clears it, and every press starts empty.
- G3 (reviewer): the test asserts nothing is selected before GTK's release is done (guards the idle).
- G4 (reviewer, code-review): the test presses on a plain text entry and asserts it is left alone (the y = -5 case proved nothing).
- G5 (code-review): the press arm is flat let-else; a Cell instead of a RefCell.
Noted, from gtkspinbutton.c 4.18.6 (lines 894-900, 1135-1147): on a touchscreen the spin button's own capture swipe gesture claims every touch on its text and grabs the focus, which selects the whole number (select-on-focus); GtkText's click never runs there. So on the Librem 5 a tap already selected the number before this change, and the on-screen-keyboard reasoning of F1 applies to the mouse path only; our press clear and release select change nothing on a touch tap. (Since H1, a swipe that spins the value ends with the new number selected: accepted in round 5, typing replaces it.) Device check stays.
Rejected:
- Move the test helpers (`text_of`, draining) into a shared test module (code-review): two lines each, one test; /simplify may take it.
- A spin button in a popover would not get the selection (code-review): none exists; the surface check says why in a comment.
- Generic `impl IsA<gtk::Widget>` for one caller (code-review): the caller hands the window subclass; the generic saves an upcast.

## Review round 4 (fix reviewer + /code-review high, 2026-11-12)
Fixed:
- H1 (both): G1's distance check missed short drag-selections (GtkText selects on every motion with no threshold, gtktext.c drag update) and judged diagonal clicks differently from GTK (per-axis, click.c:171). Replaced by the outcome: the idle selects only while GTK left no selection; the press already emptied it, so any selection then is the user's drag. The coordinates, the Settings read and the GrabBroken/TouchCancel arms are gone (a stale field can only select a focused field with nothing selected). Test case "drag keeps what it selected" added; dropping the check failed it.
- H2 (reviewer): the plain-entry case asserts the press really lands in the entry's text.
Rejected:
- Pressing inside a selection to drag the number out (drag and drop) no longer starts, as the press clears it (code-review): dragging a one or two digit number elsewhere is not a use.
- A second touch point resets the pressed field (code-review): multi-touch on a number field, as in round 2.

## Review round 5, the last (fix reviewer + /code-review high, 2026-11-12)
Fixed (no sixth round: five is the limit; asked at hand-over):
- J1 (code-review): every plain click into a focused field selected everything, so a click could not put the cursor inside a number mid-edit (0.75 lead-in, 1.5 vibration). Now a click selects only when the field gets the focus or its whole number is already selected; mid-edit GTK places the cursor. Scenario and test case added (failed before the change).
- J2 (code-review): Shift+click anchored on 0 after our clear; a Shift press is left to GTK.
- J3 (code-review): TouchCancel and GrabBroken clear the pressed field again, so a lost release can't select a field being edited.
- J4 (code-review): the press arm is one helper (`number_under`) with `?`.
Accepted or rejected:
- A touch swipe that spins the value ends with the new number selected (reviewer, code-review): typing replaces it; accepted.
- A partial selection made earlier is dropped by a touch on the field (code-review): needs the whole number selected for us to act since J1, so only a whole selection is touched.
- The test hand-writes GTK's press and drag; the controller glue has no host test (code-review): GDK events can't be made on the host; device test.
- A per-field controller instead of the window's (code-review): twelve call sites and every future field, against one handler.
- `number_pressed` both finds and clears (code-review): its doc says so; the press is the only caller besides the test.

## /simplify (2026-11-12)
Applied: the pale number is one `PaleNumber` component (both reviewers); the press reads the text length only for a focused field; the test binds the press point once and names the upcast window apart.
Deferred to REFACTORING: #519 one Android number input; #520 source tests without indentation.
Skipped: a click gesture instead of the window's legacy controller (it is cancelled when the field claims the press, so its release never comes; the per-event match is a few instructions); the pale number as a permanent Text with `visible` (an `if` on a focus change is negligible); the test's helpers and layout wait (settled in round 3); `delegate()` instead of the tree walk (unclear for a spin row).
Suites after: GTK 179, Android 295; GTK+core clippy all-targets and CI clippy clean.

## Hand-over (2026-11-12)
Artifacts: GTK `meditate-gtk/builddir/src/meditate` (20:44, running on the laptop with `XDG_DATA_HOME=~/.cache/meditate-nft-test`); APK `meditate-android/android/app/build/outputs/apk/debug/app-debug.apk` (20:45), not installed yet (needs "verbunden").
LOC: 58,761 -> 58,849 (+88: the click handler with its docs, the pale number in Slint).

Test steps:
- Laptop 1: click into Preparation Time (Timer setup) from elsewhere: the number is highlighted; type 12: it shows 12.
- Laptop 2: click into a field, then click elsewhere without typing: the number stays.
- Laptop 3: Custom Time dialog: click into hours, then minutes: each highlighted; Tab into a field and click it: stays highlighted.
- Laptop 4: Preferences lead-in length: type 0.8, then click between 0 and the point: cursor there, nothing highlighted.
- Laptop 5: drag over one digit of a number: only that digit is highlighted.
- Laptop 6: Log, add or edit a session: hours and minutes fields highlight on click.
- FP5 7: tap Preparation Time, Custom Time, Daily Goal, an interval bell's Time and a vibration editor field: each empties and shows the old number pale; the first digit makes it go; leaving without typing shows the number normally. The cursor inside the pale digits looks fine.
- Librem 5 (optional, needs a cross-build): tap a number field: highlighted, no copy bubble; close the keyboard and tap again: the keyboard comes back.

Proposed commit message:
gtk, android: a tapped number field readies for a new number

On Linux a click put the cursor into the number, which then had to be
deleted. A click or tap now selects the number, as Tab already did, so
typing replaces it; mid-edit a click still moves the cursor. On Android
the tapped field empties as before and now shows the old number pale
until the first digit.

## Device test 1 (laptop, Janek, 2026-11-12)
- Found: the Custom Time dialog did not select. Logging at the window's controller showed none of the dialog's presses reached it. A first guess (the dialog is its own window; handler on every toplevel) was wrong: the second log showed no new toplevel, and the dialog's native is the main window. Cause, from adw-dialog.c 1.7.0 line 1314: every AdwDialog sets `gtk_widget_set_limit_events`, so GTK's propagation (gtkmain.c `gtk_propagate_event_internal`) stops at the dialog and the window's capture controllers never see its events. The plan's claim "dialogs inside the main window are covered" was wrong.
- Fix: the handler also goes on each dialog the window shows (`visible-dialog` notify), once per dialog (named controller); the press picks in the whole window, as the dialog's controller sits inside it. Scenario and test `every_dialog_selects_numbers_on_click` (failed before the fix). The toplevel attempt is gone. GTK 180 green, clippy clean. No review round: five were done; asked at hand-over.
- Retest after the fix (Janek, 2026-11-12): Laptop 1 to 6 all passed. Open: FP5 step 7.

## Device test 2 (FP5, Janek, 2026-11-12)
- Found: the time dialogs (Custom Time, Preparation Time, Daily Goal) showed no number at all. Cause: the new wrapper Rectangle around the VerticalSpinBox input takes no height from its children (they fill it), so the layout gave it 0 px. Fix: `min-height: input.preferred-height` on the wrapper. No host test: the Slint layout does not run on the host and slint-viewer is not installed; proof is the FP5. Android 295 green; APK 23:22 installed.
- Found: the pale number looked almost like the real one (on-surface-variant is close to on-surface in the dark theme). Now on-surface at 38%, Material's disabled level; source test updated first and failed. APK 23:30 installed.
- FP5 retest (Janek, 2026-11-12): numbers visible again, pale number clearly paler: passed. Librem 5 not checked (needs a cross-build); its touch path is GTK's, covered by the host test and the laptop.
- LOC (non-test): 58,761 -> 58,868 (+107).
