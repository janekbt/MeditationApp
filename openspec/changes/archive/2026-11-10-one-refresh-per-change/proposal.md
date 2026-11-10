# One refresh per change

## Why
P2 step 3, the last one. After a write, the Android app re-reads its lists by hand at every call site: the same three or four refresh calls are written out five to nine times each (bell sounds, presets, guided files), each reading all labels and presets again, and every list is rebuilt as a new model; a Local star rescans the whole folder to learn its own state. The Linux Interval Bells page rebuilds on every spin step. The Log filter shares the app-wide "data changed" flag. Replay reads a row's name back to learn what it just wrote. And on the phone, an interval bell whose sound is gone shows an empty line instead of "Missing" (#408). REFACTORING #418, #468, #353, the rest of #397, #505, #506; bug #408.

## What Changes
- Android: one helper per kind of change, called instead of the written-out lists: bell sounds (open chooser, Setup bell rows, interval bells, editor's sound name), presets (chooser if open, Setup list, widget; each list reads the label names once, through one helper), guided files (Manage list and Setup list; rename, delete and Undo still follow the selection themselves). The vibration editor's Save calls the existing pattern helper. The label delete drops its hand patch of the Edit label, which `refresh_after_label_change` already does.
- Android: a pull refills the open bell chooser inside `refresh_after_pull` (category and tick passed in), so one function says what a pull re-reads.
- Android: Setup's bell rows read under one lock; the interval-editor ranges are set once at startup; the Interval Bells list reads the sound library once and names a gone sound "Missing" (#408).
- Android: the refreshed lists (preset chooser and list, guided Manage and Starred, Local rows) update in place through `put_rows`; the Local star passes the row's state from the layout instead of rescanning the folder to learn it.
- Linux: the Interval Bells list rebuilds when its page starts to show (opening it, coming back from the edit page), not on every edit-page change, and builds once when opened; `on_changed` runs once per change; rows name their sound from one library read.
- Linux: a Log filter change reloads the Log itself instead of marking the app-wide "data changed" flag; the app's two stale flags become private behind `mark_stale`/`take_*`.
- Core: the replay reclaim learns from the recompute whether it renamed a row, instead of reading the name back.
- Left out: Interval Bells on Android keep a new model per refresh (the Switch would echo a pulled or dimmed state back as a write; stays in REFACTORING with that reason).
- Left out: Q (#92, #100) goes with P8, EE with P3 (decided). #505's "Setup mark in `refresh_shown`" moves to #312, where `load_setup()` becomes one (folded, not dropped).

## Decisions
- Ride-along bugs: only #408 (maintainer, 2026-11-10: "Nur #408 mit").
- Helpers live in `ui.rs` beside the ones they call; no new module (my call).
- The open bell chooser's category and tick go to `refresh_after_pull` as arguments, not new Slint properties (my call: no layout change for it).
- A preset change refreshes the chooser only while it is open, as a pull does today (my call: same visible result, one read less).

## Capabilities
### Modified Capabilities
- `bells`: the phone's Interval Bells list says "Missing" for a gone sound, as the requirement already states.

## Impact
- Core: `db/events.rs` (reclaim), the recompute functions that claim names (labels, presets, guided files, vibration patterns).
- GTK: `bells.rs`, `log/mod.rs`, `log/imp.rs`, `application.rs`.
- Android: `src/ui.rs`, `ui/main.slint` (the Local star callback gets the row's state), source tests in `src/app.rs`.
- Translations: none ("Missing" exists in both apps).
- Data: none.
- Android build environment: untouched.
- Docs: REFACTORING.md (#418, #468, #353, #397, #505, #506 out, #505's Setup mark into #312, P2 ticked), OPEN-ISSUES.md (#408 out of HH).
