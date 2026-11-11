# One dialog frame (P4-b)

## Why
On Android, eight dialogs draw the scrim and card by hand. They have
drifted apart: three widths, six titles that are cut instead of wrapped
(#474), and a Custom Time dialog whose Set closes silently at 0:00
(#475). REFACTORING #465.

## What Changes
- One frame for every Android dialog: scrim (a tap outside cancels), one
  card width, a 22 px title that wraps. Filter, Custom Time, Preparation
  Time, the name dialog, Daily Goal, Recovery, Sync status and Label
  conflict move onto it; ConfirmDialog is built on it.
- Linux: Custom Time's Set is off at 0:00 too (it closed silently;
  maintainer, 2026-11-11, after review).
- Custom Time, Preparation Time and Daily Goal become ConfirmDialogs
  with "Set". Custom Time's Set is off at 0:00 and comes on as soon as a
  typed number makes it non-zero (#475).
- Daily Goal becomes the same − / + number field as Preparation Time
  (1 to 1440, steps of 1); the free text field and its validation in
  the layout go.
- Preparation Time's − / + steps by 5 and snaps to fives; a typed number
  stays free (5 to 300).
- REFACTORING #470 (Prep and Goal as rows in the page): rejected by the
  maintainer, the time is set in a dialog.
- Left out: routing Custom Time's Set through Rust (the dialog still
  knows Edit Session vs. Setup); it moves to #469, which rewrites the
  Edit Session decisions. #461's double button row stays in P12.

## Decisions
- #470: dialogs stay, no inline rows (maintainer, 2026-11-11).
- Daily Goal: − / + number field like Preparation Time (maintainer).
- Preparation Time steps by 5 like Linux; − / + snap to the next multiple of 5, a typed number stays (maintainer).
- One card width, `min(340px, window - 32px)` for every dialog (my
  call): at 360 px every card is 328 px wide anyway; 340 is what the
  long button rows needed on wider phones.
- The number field takes a typed number as it is typed (clamped on
  Enter or blur as before), so Set sees it at once (my call; needed
  for #475).
- Daily Goal no longer opens with the keyboard up: like the other
  number dialogs, the field is typed into on tap (my call).

## Capabilities
### Modified Capabilities
- `appearance`: one dialog frame on Android, titles wrap.
- `stats`: a typed 0 in Daily Goal on the phone becomes 1 (the hint is gone).
- `text-input`: Custom Time never 0:00 (both apps); the time dialogs' number fields; Daily Goal leaves the
  Enter-confirms list.

## Impact
- Core: none
- GTK: `src/timer/imp.rs` (Custom Time's Set off at 0:00, the same bug #475; the dialog's build moved into a testable function)
- Android: `ui/main.slint`, `src/app.rs` (tests), `src/ui.rs` (goal
  seeding if needed)
- Translations: none new; the Goal field's label "Minutes ({}–{})" is
  no longer used and leaves the nine `.po` files
- Data: none
- Android build environment: untouched
- Docs: REFACTORING (#465 done, #470 rejected, P4 done, Duration routing
  noted under #469), OPEN-ISSUES (#474, #475 out)
