Title: android: Report selection changes from the input method

Description (identical to the commit message, since a squash merge uses it):

Keyboards move the cursor through InputConnection.setSelection(), for
example when the user slides over Gboard's space bar. SlintEditable only
reported text replacements, so the selection changed in the Java
editable while the TextInput's cursor stayed where it was.

Report a selection that the input method changed after setSelection()
and at the end of a batch edit, and tell the input method about it, like
Android's own text fields do. Rust sets the anchor and cursor of the
focused TextInput, the same way dragging a selection handle does, and
keeps the cursor visible while it moves, like the arrow keys do.

== Implementation Notes ==

Sending the selection through updateText() would move the cursor too, but
the TextInput applies it as a replacement of the whole text, so every step
would fire edited and add undo steps.

While a preedit exists, the selection still goes through updateText(),
which keeps the preedit in sync. That includes the state after
finishComposingText(), which drops the composing span without calling
replace(), so Rust still shows the preedit.

ChangeLog: Android: Text inputs follow cursor moves from the keyboard, such as sliding over Gboard's space bar

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

---

First comment (review only, not part of the commit):

## How this shows up

Slide over the space bar in any Slint text field on Android: the cursor
doesn't move, and the next letter lands at the old position.
Gboard meanwhile believes the cursor moved, so its suggestions refer to
the word under its own idea of the cursor.

Logging `setSelection()` on a Fairphone 5 (Android 15, Gboard) during a
slide shows a call per step, outside any batch edit, and nothing reaching
Rust:

```
setSelection 14,14 batch=0
setSelection 12,12 batch=0
setSelection 10,10 batch=0
```

With this change, each step reaches the `TextInput`, and the echo from
`set_imm_data` arrives within a few milliseconds with the same position,
so it doesn't move the cursor again:

```
setSelection 5,5 batch=0
updateSelection 5,5
setText from Rust sel=5,5
```

## Testing

- `material-gallery` from this branch on the API 35 emulator with Gboard:
  after a slide and a typed letter, the letter lands at the new position.
  On master it's appended at the old one.
- The same patch on 1.18.1 in an app, with logging:
  Fairphone 5 (Android 15, Gboard), and emulators with Gboard on API 26,
  30 and 35, including the composing case on API 26.
- `cargo clippy -p i-slint-backend-android-activity --target aarch64-linux-android --features aa-06,native-activity`
  reports no new warnings, and `cargo fmt` is clean.
