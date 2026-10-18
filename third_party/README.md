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
only these two fixes.

The Java helper is compiled into the APK (the crate's `build.rs` runs
javac and d8), so a change here is a change to the shipped app: run the
repro probe after touching it.

Delete this directory and the `[patch.crates-io]` entry in the workspace
`Cargo.toml` once a Slint release carries both fixes.

### Refreshing after a Slint upgrade

    cp -r ~/.cargo/registry/src/*/i-slint-backend-android-activity-<version> third_party/i-slint-backend-android-activity
    rm -f third_party/i-slint-backend-android-activity/.cargo-checksum.json \
          third_party/i-slint-backend-android-activity/.cargo-ok \
          third_party/i-slint-backend-android-activity/Cargo.lock

then reapply whichever fix the release doesn't have yet, and regenerate
`build-aux/cargo-sources.json`.
