# Android text fields: keyboard cursor and copy bar (batch Y, part 2)

## Why
#160. On Android, sliding over Gboard's space bar doesn't move the cursor
in any text field, and the cut/copy/paste bar covers the selected text
instead of sitting above it. Both are bugs in Slint's Android backend
(`SlintAndroidJavaHelper.java`), still on Slint's master (2026-10-18). A
logging spike on the FP5 proved both causes:
- Gboard moves the cursor with `InputConnection.setSelection` outside a
  batch edit. Slint's editable only reports text replacements, so Rust
  never learns of the move.
- Slint's input view sits at y = 110 px in the window (below the status
  bar); `onGetContentRect` returns the selection in window coordinates
  where Android expects view coordinates, so the bar lands 110 px low.

## What Changes
- A selection change from the keyboard moves the cursor or selection in
  the focused text field, without editing the text (no `edited`, no undo step).
- The cut/copy/paste bar sits above the selection (below it when there's
  no room above).
- Meditate builds a vendored `i-slint-backend-android-activity` 1.18.1
  carrying exactly these two fixes until a Slint release has them.
- Two upstream pull requests against Slint's master are prepared (one per
  bug), ready to open after the maintainer's go.
- Left out: Slint's "FIXME: I don't know why" `top < 0` workaround in the
  same function stays untouched (maintainer); the selection-change path
  while a word is being composed keeps today's full-text update.

## Decisions
- Fix upstream-quality, two separate PRs, limited to the two bugs (maintainer).
- The FIXME stays (maintainer).
- Test on the FP5 plus emulators API 26, 30, 35 (maintainer).
- Rust side sets cursor and anchor on the focused `TextInput` directly,
  like `callback_move_cursor_handle`, not a full-text commit, which would
  fire `edited` and push two undo items per step (my call; the empty
  `CommitComposition` route is the alternative, mentioned in the PR).
- While a composing span exists, or the last state Rust knows had one
  (Gboard's `finishComposingText` removes the span without telling Rust),
  a selection change falls back to the existing full update, which ends
  the preedit cleanly (my call, from the pre-build questions).
- Java tells the keyboard about the new selection itself
  (`InputMethodManager.updateSelection`), as Android's own text fields do;
  Rust's echo then matches and stays silent (my call).
- Selection moves are allowed in read-only fields, like the handle drag and
  Slint 1.17's selectable read-only inputs; only focus is checked (my call).

## Capabilities
### New Capabilities
- `text-input`: how Android text fields follow the keyboard and place the copy bar.

## Impact
- Core: none
- GTK: none (GTK text fields are native)
- Android: no app code; `third_party/i-slint-backend-android-activity/` (new)
- Translations: none
- Data: none
- Android build environment: `third_party/` (new vendored crate),
  `Cargo.toml` (`[patch.crates-io]`), `Cargo.lock` (source line). Approved
  for this batch; repro probe required.
- Docs: `third_party/README.md` (what is patched, when to drop it);
  `build-aux/cargo-sources.json` regenerated; OPEN-ISSUES #160 removed,
  batch Y ticked.
