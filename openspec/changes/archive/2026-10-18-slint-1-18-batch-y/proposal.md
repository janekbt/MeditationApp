# Slint 1.18 and the year list (batch Y, part 1)

## Why
Our Slint fork (`third_party/i-slint-compiler`) only existed to sort the
bundled languages for reproducible builds. Slint 1.18.0 ships that fix
(slint-ui/slint#12932), so the fork is dead weight. 1.18 also fixes part
of #160: the cursor handle of an empty field no longer sits at the
screen's left edge. #148: the Android date picker's year list is fixed at
2024 to 2043 without 2030, so a session from 2022 can't be moved by year.

## What Changes
- Slint 1.16.1 to 1.18.1 (all `slint`/`i-slint-*` crates in Cargo.lock).
- `third_party/i-slint-compiler` and the `[patch.crates-io]` entry are deleted.
- Android date picker: the year list runs from this year back to 2000, newest first; picking a year keeps the day and month (Material jumped to 1 January).
- The 1.18 deprecated property names in our .slint files are renamed (warnings, no behaviour change).
- Left out: the space-bar slide of #160 (still missing upstream). It gets
  its own change: a patch to `i-slint-backend-android-activity`, vendored,
  tested, then proposed upstream. Left out too: refreshing `material-1.0`
  from the 1.18 Material library (upstream's year list is still hard-coded;
  nothing else is needed).

## Decisions
- Do this part first and ship it, then the slide patch on its own (maintainer).
- Years: this year back to 2000, no future years (maintainer).
- Picking a year keeps day and month, the day clamped to the month's length, like GTK's calendar (maintainer, after review finding 1).
- The list is built in Rust through a new `DatePickerAdapter` callback, so
  it has a host test; Slint has no range-to-array (my call).
- Newest year first (my call, offered in the question; the user picks a
  recent year far more often).

## Capabilities
### Modified Capabilities
- `log`: the Android edit dialog's year list.

## Impact
- Core: none
- GTK: none (its `gtk::Calendar` has no year list; it doesn't use Slint)
- Android: `src/app.rs` (year list), `src/ui.rs` (wire the callback),
  `material-1.0/ui/components/date_picker.slint` (callback + use)
- Translations: none
- Data: none
- Android build environment: `Cargo.toml` (patch entry and comment),
  `Cargo.lock` (Slint bump), `third_party/` (deleted),
  `material-1.0/` (year list). Approved for this batch; repro probe required.
- Docs: `third_party/README.md` goes with the folder; AGENTS.md and
  BUILDING.md keep `third_party/` in the build-environment list (the
  slide patch vendors there next);
  `build-aux/cargo-sources.json` regenerated; OPEN-ISSUES #148 removed,
  #160 rewritten to the part that is left.
