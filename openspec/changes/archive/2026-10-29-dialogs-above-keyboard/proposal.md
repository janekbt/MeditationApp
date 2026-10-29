# Dialogs above the keyboard (GitHub #11)

## Why
GitHub #11: in "Create Preset" the keyboard hid half the dialog and its
buttons, and Enter on the keyboard did nothing; the name had to be
confirmed by closing the keyboard with Back first. (The issue's first
part, the name selected while typing, no longer happens with the Slint
1.18.1 upgrade and text-field fixes on beta: Janek on the FP5,
2026-10-29.)

## What Changes
- Android: every modal card (the name dialogs for presets, labels,
  sounds and guided tracks; Track name on import; Daily Goal; Duration;
  the countdown; the Log filter; Recovery; Label conflict) centers in
  the part of the window above the keyboard while one is shown
  (Slint's `Window.virtual-keyboard-position/-size`), else in the window
  as now.
- Enter on the keyboard in a name dialog's field, Track name or Daily
  Goal confirms it, as its button would; while the button is disabled
  (an empty or taken name, a goal out of range) Enter does nothing.
- Those eight dialogs open with their field focused and the keyboard
  up, as on Linux (Janek, after the device test, 2026-10-29).
- Enter in every other number field (Duration, the countdown, the − / +
  rows: bell time, jitter, step, the lead-in length) takes the number
  and closes the keyboard; the dialog or page stays open (Janek,
  2026-10-29).
- The keyboard closes when a dialog closes (Enter, a button, a tap
  outside, Back), so it never stays open over the page.
- Left out: the Material date and time pickers in Edit Session (in
  `material-1.0/`, the build environment).
- Linux: nothing (GTK dialogs already sit above the keyboard; Enter
  activates the default button).

## Decisions
1. Fix the keyboard and Enter parts of #11 (Janek, 2026-10-29).
2. Enter in the other number fields: take the number, close the
   keyboard (Janek, 2026-10-29).
3. My calls: all modal cards, not only those with a field (one rule);
   centered in the room above the keyboard, never closer than 16 px
   below the status bar, except that a card too tall for that keeps its
   buttons above the keyboard; the keyboard's top taken as the window height
   minus the keyboard's height (its position is in screen coordinates
   in split screen); Enter on an invalid entry does nothing (the field keeps focus);
   the Duration and countdown dialogs don't confirm on Enter (several
   fields; Enter takes the number and closes the keyboard, decision 2); Enter on Track name starts the import as its
   button does, the progress stays visible above the keyboard; if
   Gboard's Enter turns out not to reach the field on the FP5, I stop
   and ask (the fix would be in the vendored Slint Java, the build
   environment).

## Capabilities
### Modified Capabilities
- `text-input`: dialogs stay above the keyboard; Enter confirms.

## Impact
Android `ui/main.slint` (a global the window feeds with the
keyboard's top and the status bar, the cards' `y`, `accepted` on the
fields, one `Keyboard.hide()` callback used by the number fields'
Enter and by every dialog closing), `src/ui.rs` (it hides the keyboard
through Android), a source check in `src/app.rs`. No strings, no core, no GTK, no data, no build
environment.
