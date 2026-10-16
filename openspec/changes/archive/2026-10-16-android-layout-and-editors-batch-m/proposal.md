# Android layout and editors (batch M)

## Why
On the phone, editing a vibration pattern can flatten it on every
device (#139) or save a typed value it never applied (#142). A volume
slider can show one value while the bell rings at another (#144).
Long texts push switches and buttons off screen (#143, #145), and two
editors can't scroll, so rows are out of reach in landscape (#146).
Neither app shows which guided file in Starred Files is selected (#189).

## What Changes
- Vibration editor (Android): opening a pattern loads its stored points
  and chart kind unchanged; only points the user drags still round on
  a phone without amplitude control.
- Vibration editor (Android): Save first applies a typed Duration and
  Points, so the saved curve is the one on screen.
- Volume slider (Android): when the page takes over the drag, the
  slider goes back to the value it had before the touch.
- Rows (Android): the texts beside a switch or button wrap instead of
  pushing it off screen.
- Sound and pattern choosers (Android): icon buttons as on the laptop
  (check, play/stop, edit, delete), so the name gets the room.
- Interval-bell editor and vibration editor (Android): their bodies
  scroll (the vibration editor only above the chart).
- Starred Files (both apps): the selected file's row shows a check.
- Left out: #148 and #160 (Slint library code, protected build
  environment) moved to batch Y; #147 (touch targets) moved to batch Z.

## Decisions
- #189: mark the selected file in both apps with a check, as presets,
  sounds and patterns already do (Janek).
- #148, #160 to their own last batch Y; #147 to its own batch Z (Janek).
- #143: wrap, not elide (no "..." rule) (my call).
- #145: order and icons follow GTK: check, play (pause icon while
  playing), edit, delete; the play and pause SVGs Manage Files uses and
  the Material edit icon (my call).
- #189: only the Starred Files list on the Guided page; picking a file
  on the Local Folder page closes that page, so there is nothing to mark
  there (my call).
- #144: a cancelled drag puts the slider back and sends that value once
  more, so anything that follows the slider live follows it back (my call).

## Capabilities
### Modified Capabilities
- `vibration`: editing keeps stored points; Save applies typed values; the editor scrolls; chooser row layout
- `bells`: slider cancel; interval-bell editor scrolls; sound chooser row layout
- `guided`: the selected starred file is marked
- `timer`: setup row texts wrap

## Impact
- Core: none
- GTK: `meditate-gtk/src/timer/imp.rs` (Starred Files check)
- Android: `meditate-android/ui/main.slint`, `meditate-android/src/ui.rs`, tests in `meditate-android/src/app.rs`
- Translations: none (the icon buttons reuse the existing "Edit", "Play", "Stop" strings as accessible labels)
- Data: none
- Android build environment: untouched
- Docs: OPEN-ISSUES.md (M ticked and removed; Y and Z added in step 1)
