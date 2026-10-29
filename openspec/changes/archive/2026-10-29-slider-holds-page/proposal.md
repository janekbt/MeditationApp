# A touched volume slider holds the page still (#199)

## Why
On Android a finger that lands on a bell's volume slider and moves a
little up or down makes the page scroll, and the slider snaps back to
its old value. During the #10 tests on the FP5 (2026-10-23) this
happened again and again; Janek found it "super annoying". (#199 was
noted in OPEN-ISSUES only in the discarded #10 work, so it is not in
the list on beta.)

## What Changes
- Android: while a finger rests on a volume slider, the page under it
  does not scroll; the slider follows the finger sideways as today and
  jumps to where it landed as today. Lifting the finger lets the page
  scroll again.
- A fast jerk up or down in the first 0.1 s still scrolls the page (the
  slider has not had the touch yet); the slider then never moves. The
  existing snap-back code stays as a safety net (my call).
- Left out: Linux (the problem is Android-only); splitting the movement
  into slider and scroll (Janek chose the simpler rule); coasting.

## Decisions
- Touching the slider turns off scrolling until the finger lifts
  (Janek, 2026-10-29, after first asking for x to the slider and y to
  the page).
- Landing on the slider still sets the value there (my call: unchanged
  behaviour, nothing asked for).
- Both pages with volume sliders get it: the setup page (Timer,
  Guided, Box Breath, Box Breath phase cues) and the interval bell
  editor (my call: the only two).

## Capabilities
### Modified Capabilities
- `bells`: a touched volume slider keeps the page from scrolling.

## Impact
- Core: none
- GTK: none
- Android: `ui/main.slint` (a global "slider held" flag set by
  `VolumeSlider` on down, cleared on up and cancel; the two Flickables
  bind `interactive` to it), `src/app.rs` test.
- Translations: none
- Data: none
- Android build environment: untouched (no change to vendored Slint)
- Docs: none (OPEN-ISSUES has no #199 entry on beta)
