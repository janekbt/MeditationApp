# third_party

## i-slint-backend-android-activity

A copy of the crates.io release `i-slint-backend-android-activity 1.18.1`
with two fixes, both proposed upstream against Slint's master:

- **The cursor follows the keyboard.** Gboard moves the cursor with
  `InputConnection.setSelection()`, for example when sliding over the space
  bar. Upstream only reports text replacements to Rust, so the cursor
  stayed put. `SlintEditable.updateSelection()` and the `setSelection()`
  override in `java/SlintAndroidJavaHelper.java`, and
  `callback_update_selection` in `javahelper.rs`.
- **The cut/copy/paste bar sits next to the selection.**
  `onGetContentRect()` handed Android window coordinates where it expects
  view coordinates, so the bar sat one status bar lower, on the text.

Everything else is unchanged; `diff -r` against the registry copy shows
only these two fixes, plus the call to `select_word_on_long_press` in the
long-press timer (`androidwindowadapter.rs`), which needs the core below.
Upstream: slint-ui/slint#14205 (keyboard), #14206 (selection bar), and
the long-press call is part of #14208 (see i-slint-core).

The Java helper is compiled into the APK (the crate's `build.rs` runs
javac and d8), so a change here is a change to the shipped app: run the
repro probe after touching it.

## i-slint-core

A copy of the crates.io release `i-slint-core 1.18.1` with one addition in
`items/text.rs`: `TextInput::select_word_on_long_press()`. A long press
selected the word, but the held finger kept the character drag mode, so
the slightest move pulled the selection end to the finger. The function
switches a held press to word drag mode, as a double click does. `diff -r`
against the registry copy shows only this function. Upstream:
slint-ui/slint#14208, together with the backend call.

Both crates are compiled into the APK: run the repro probe after touching
either.

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
