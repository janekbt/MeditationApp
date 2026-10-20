# Slint fixes as merged upstream, and the system colour scheme

## Why
Meditate's own copy of Slint (`third_party/`) carries the three text-field
fixes as they were first submitted. Upstream merged #14205 and #14208 with
review changes, which our copy lacks; #14206 matches. Until a Slint release
ships them, the copy should match what was merged. Android 14+ derives a full
Material 3 scheme from the wallpaper. The Linux app already follows the system
accent through libadwaita, so on Android a "System" choice in the accent picker
is the matching behaviour. Most people would never notice it as an extra
option, so it becomes the default.

## What Changes
- Text fields behave as upstream merged them:
  - When the cursor moves while Gboard is still composing a word, the keyboard
    is told the composing range.
  - A long press on a line break or behind the end of the text selects nothing.
  - A double or triple press that is held keeps its selection.
- On Android 14+, the accent picker in Preferences gets a first circle,
  "System": the system primary colour with a small wand symbol. Picking it
  colours the whole app from the wallpaper's Material 3 scheme in light and
  dark mode, label colours included.
- On Android 14+, System is the accent for everyone, whatever colour an
  earlier version stored. Nothing is rewritten for that. A pick made with
  this change is kept. Below Android 14 nothing changes.
- All seven circles shrink from 40 to 32 px, so the row fits at 360 px.
- Left out:
  - The draft #14244 itself, with its compiler and common changes, so there
    are no new Slint copies. Meditate reads the same 47 roles through its own
    JNI. It switches to `Platform.system-color-schemes` once a Slint release
    has it.
  - Android 12 and 13 tonal palettes.

## Decisions
- Scope: the three merged PRs, and System colours through Meditate's own query rather than the draft's Slint changes (maintainer).
- System is the first circle and the default (maintainer).
- On Android 14+, every pick from before this change becomes System once (maintainer).
- Below Android 14 the System circle is hidden and the default is Blue (maintainer).
- Circles are 32 px; the System circle is the system primary plus a wand symbol (maintainer).
- The pick is stored as `system`, never as a shifted index. The six numbers keep their meaning, so picks synced with older versions stay correct. An older version that receives `system` shows Blue. (my call, forced by sync)
- How old picks become System: a pick made from now on is stored as `+N` (for example `+2` for Teal), and System as `system`. The old plain `N` is read as System on 14+ and as colour N below 14. Nothing is written at startup, so there is no marker and no race between devices that haven't synced yet. The previous release reads `+N` as N, because Rust's integer parse accepts a leading `+`, and it reads `system` as Blue through its fallback. The one corner: a pick made on a device still on the previous release shows as System on 14+ until it is picked again with this version. (my call: a written switch plus a marker can override a deliberate pick through sync, as the pre-build review found)
- A phone below Android 14 that receives `system` by sync shows Blue with the Blue circle ringed, and leaves the stored value alone. (my call)
- The role table is the draft's, by resource name: `surfaceTint` is the system primary, and the three inverse roles come from the tonal palettes; `shadow` and `scrim` stay as shipped. (my call)
- A JNI failure while reading the system colours only goes to the log: the app shows the fixed colours without the System circle, as below Android 14, and there is nothing the user could do about a message. A recorded exception to "every failure tells the user". (maintainer, 2026-10-20)

## Capabilities
### New Capabilities
- `preferences`: the accent picker's System choice and its default.
### Modified Capabilities
- `text-input`: the composing word and a long press on a line break or behind the end of the text, as merged upstream.

## Impact
- Core: none. The accent is Android-only; the settings are written through the existing `Database::set_setting`.
- GTK: none. It already follows the system accent.
- Android:
  - `src/theme.rs`: the accent choice, its parsing, the circle order, the role table and building the scheme from the roles.
  - `src/ui.rs`: the JNI read (`with_env`, `SDK_INT >= 34`, `Resources.getIdentifier` and `getColor`), `apply_accent`, the picker, and `reload_setup`.
  - `ui/main.slint`: the circle size, `accent-has-system`, and the wand on row 0.
  - A new wand SVG in `ui/`.
- Translations: none. The circles have no text.
- Data: no schema change. `android_accent` gains the values `system` and `+N`, which are ordinary `setting_changed` events.
- Android build environment, approved for part 1 only (the repro probe runs at the end):
  - `third_party/i-slint-core` and `third_party/i-slint-backend-android-activity` get the merged fixes.
  - No new crates, and no change to `Cargo.toml`, `Cargo.lock` or `material-1.0/`.
- Docs:
  - `third_party/README.md`: the three fixes now as merged.
  - The text-input spec and the new preferences spec.
  - README features: "follows the system colours on Android 14+".
