# Android long press: the selection extends by word (#195, batch Y2)

## Why
#195. A long press in a text field selects the word, but the slightest
finger movement before lifting pulls the selection end to the finger,
so half the word is deselected. Cause (Slint, not our code): on touch-down
`TextInput` enters character-drag mode (`pressed = 1`); the Android
backend's long-press timer then calls `select_word`, but the mode stays,
so every move sets the cursor to the finger by character. A double click
on the desktop uses word mode (`pressed = 2`), where moves extend by whole
words. `pressed` is private to `i-slint-core`.

## What Changes
- After a long press selects a word, moving the still-held finger extends
  the selection by whole words, and jitter inside the word changes nothing,
  as in Android's own text fields.
- Slint core gets `TextInput::select_word_on_long_press()` (selects the word
  and switches a held press to word mode); the Android backend's
  long-press timer calls it instead of `select_word`. One upstream PR.
- Meditate vendors `i-slint-core` 1.18.1 with that function, next to the
  vendored backend, until a Slint release has it.
- Left out: moving long-press detection into core for other touch
  platforms; the long press on an empty field or a line end keeps today's
  behaviour (no word to select); a long press on a field that isn't
  focused yet still selects nothing (Android focuses on release); a
  vertical move of 8 px or more within the first 500 ms still lets a
  scroll area take over the finger, which ends the drag (the word stays
  selected). Each is a separate behaviour, not this bug.

## Decisions
- Extend by word like Android, fix in core + backend, Meditate vendors core (maintainer).
- The test calls the function inline from the `.slint` test's Rust block (test-driver-rust links i-slint-core), no new testing helper (my call).
- The PR carries a `ChangeLog:` trailer like the other two (my call).
- The function sets word mode only while a press is held, so a call without
  a press (or after release) only selects the word (my call).

## Capabilities
### Modified Capabilities
- `text-input`: long-press selection on Android.

## Impact
- Core, GTK, Android app code, translations, data: none
- Android build environment: `third_party/i-slint-core/` (new),
  `third_party/i-slint-backend-android-activity/` (one call),
  `Cargo.toml` patch entry, `Cargo.lock` source line,
  `build-aux/cargo-sources.json`. Approved for this batch; repro probe.
- Docs: `third_party/README.md`; OPEN-ISSUES #195 removed, Y2 ticked.
