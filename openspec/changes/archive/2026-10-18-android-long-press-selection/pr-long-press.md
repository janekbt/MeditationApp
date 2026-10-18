Title: android: Extend a long-press selection by words

Description (identical to the commit message, since a squash merge uses it):

A long press selects the word under the finger, but the finger never
rests completely still. Moving it while held, before or after the long
press fired, moved the end of the selection to the finger, so a part of
the word or a part of the next one ended up selected.

Add TextInput::select_word_on_long_press(), which selects the word where
the held press started and switches the press to word drag mode, the
mode a double click uses. The Android backend calls it when its
long-press timer fires. Dragging then extends the selection by whole
words, like in Android's own text fields.

== Implementation Notes ==

The touch-down puts the TextInput in character drag mode, so each move
of the held finger sets the cursor, and the long press didn't change
that mode. A move before the timer fired also left a selection, and the
backend then skipped selecting the word. The backend now checks the
character at the anchor, where the press started, and core moves the
cursor back there before selecting the word.

ChangeLog: Android: A long press selects the word under the finger even if the finger moves a little, and dragging extends the selection by whole words

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

---

First comment (review only, not part of the commit):

## How this shows up

Long-press a word in a text field and keep the finger down for a moment
before lifting it. Any small movement in between, which a real finger
always makes, shrinks the selection to the part of the word left of the
finger. In the material gallery on master, a long press on "ipsum" with a
few pixels of movement ends with "ips" selected.

## Testing

- `tests/cases/text/select_long_press.slint` (new) does what the Android
  long-press timer does and checks that a small move keeps the word, that
  dragging extends by words in both directions, that a drift into the next
  word before the timer still selects the word where the press started,
  that a held triple press keeps its paragraph, and that a long press
  without a held press only selects the word.
  Without the change it fails at the first small move (cursor 7, expected
  11).
- `SLINT_TEST_FILTER=text/` in test-driver-rust passes (38 cases).
- `material-gallery` on the API 35 emulator: master selects "ips" after
  the trembling long press; with this branch "ipsum" stays selected, and
  dragging on selects "ipsum dolor".
- The same change on 1.18.1 in an app on a Fairphone 5 (Android 15).
- `cargo clippy -p i-slint-core --features std` is clean, the Android
  backend for aarch64 has no new warnings, `cargo fmt` is clean.
