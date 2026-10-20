# third_party

## i-slint-backend-android-activity

A copy of the crates.io release `i-slint-backend-android-activity 1.18.1`
with three fixes, as merged upstream into Slint's master:

- **The cursor follows the keyboard** (slint-ui/slint#14205, merge 5b528c1).
  Gboard moves the cursor with `InputConnection.setSelection()`, for example
  when sliding over the space bar; upstream only reported text replacements.
  `SlintEditable.updateSelection()` and the `setSelection()` override in
  `java/SlintAndroidJavaHelper.java`, `callback_update_selection` in
  `javahelper.rs` (through `TextInput::set_selection_offsets`). Master
  refactored the Java file since 1.18.1, so this is a hand port: 1.18.1
  keeps `mPreeditStart`/`mPreeditEnd`, and `hadPreedit` is read from them
  where master has a `mHasPreedit` field.
- **The cut/copy/paste bar sits next to the selection** (#14206, merge
  d5a600a). `onGetContentRect()` hands Android view coordinates instead of
  window coordinates; the old `top < 0` workaround is gone.
- **A long press selects by words** (#14208, merge 7cb5f71). The long-press
  timer in `androidwindowadapter.rs` calls
  `TextInput::select_word_on_long_press` (see i-slint-core).

The Java helper is compiled into the APK (the crate's `build.rs` runs
javac and d8), so a change here is a change to the shipped app: run the
repro probe after touching it.

## i-slint-core

A copy of the crates.io release `i-slint-core 1.18.1` with, in
`items/text.rs`, the core parts of the same merges:

- `TextInput::select_word_on_long_press()` (#14208): a held long press
  switches to word drag mode as a double click does; a held double or
  triple press keeps its selection; nothing is selected at a line break or
  behind the end of the text.
- `set_selection_offsets()` keeps the cursor visible while it moves (#14205).

`diff -r` against the registry copies shows only these. Both crates are
compiled into the APK: run the repro probe after touching either.

Delete these directories and the `[patch.crates-io]` entries in the
workspace `Cargo.toml` once a Slint release carries all three fixes; both
crates must stay on the same version (the backend pins `=x.y.z` on core).

### Refreshing after a Slint upgrade

    for c in i-slint-backend-android-activity i-slint-core; do
        cp -r ~/.cargo/registry/src/*/$c-<version> third_party/$c
        rm -f third_party/$c/.cargo-checksum.json third_party/$c/.cargo-ok third_party/$c/Cargo.lock
    done

then reapply whichever fix the release doesn't have yet, and regenerate
`build-aux/cargo-sources.json`.
