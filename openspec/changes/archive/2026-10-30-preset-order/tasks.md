# Tasks: preset-order (GitHub #12)

Base: f36a3d6; suites on it: core 1407, android 272, gtk 164 green
(run on this tree right before the f36a3d6 commit, not again).

## Loop
- [x] 0 Skills loaded, again after every /compact: `/ponytail ultra`; `slint:slint`
- [x] 1 Understand (said back, Janek "passt"; decisions in proposal.md)
- [x] 2 Propose (fresh-agent check done, answered in Notes; approved: 2026-10-30, Janek "go")
- [x] 3 Build (core 1413, android 272 green; GTK clippy (proved with a planted error) and Android-target clippy clean; every core test failed first for the right reason)
- [x] 4 Review, then `/simplify` after the last review (review, seven fix reviews, a fuzzing review of the whole mechanism, /simplify; see below), then `/simplify` after the last review
- [x] 5 Specs, docs, list (check-specs green; README presets line; EVENTS.md `created_iso` note; no OPEN-ISSUES entry for GitHub #12)
- [x] 6 Verify and hand over (FP5 and Linux: Janek 2026-10-30 "alles tests gruen"; the Linux handle stood right of the star, moved in front of it, Janek "passt")
- [x] 7 Ship (delta merged into openspec/specs/presets/spec.md, archived 2026-10-30)

## Review Focus
- A second device moves presets too before syncing -> "Two devices move presets before they sync".
- Another device edits a moved preset before syncing -> accepted limit in proposal.md; presets the move passes are untouched ("An edit or delete elsewhere to a preset the move passes").
- An older version on another device -> it sorts by the same creation time ("Another device, also one with an older version").
- Equal creation times (imported or synced data) -> "Two presets created at the same moment".
- A preset deleted on another device while the list is open -> the move takes the uuid and the index in the list as stored now; an unknown uuid writes nothing (task 1.1).
- An index past the end -> clamped to the last place (task 1.1).
- A one-preset or empty list -> no handle needed, nothing to drop on; a move writes nothing.
- Another language -> the one new Linux string, nine languages.
- The write fails (sync holds the database) -> existing "Save failed" path, list reloaded from the database.

## 1. Core: move a preset
- [x] 1.1 `Database::move_preset(uuid, to_index)` in `meditate-core/src/db/presets.rs`: reads the mode's list, moves the preset to `to_index` (clamped), and gives the presets from the old to the new place the sorted creation times of that span in their new order, one `preset_update` each (full payload as `update_preset_config` writes it), in one transaction; an unknown uuid or the same place writes nothing; equal creation times in the mode: fresh, evenly spaced times for the whole mode. Proof: `a_moved_preset_takes_its_new_place`, `a_preset_moved_down_lands_below_the_others`, `a_preset_dropped_on_its_place_writes_nothing`, `presets_with_the_same_time_can_still_be_moved`, FAIL then PASS each.
- [x] 1.2 Sync. Proof: `another_device_gets_the_new_order`, `an_edit_elsewhere_to_a_preset_not_moved_keeps_the_order` (two devices, `FakeWebDav`, as the bell copy tests do), FAIL then PASS.

## 2. Linux
- [x] 2.1 `meditate-gtk/src/presets.rs`, Manage mode only: a handle image (bundled `list-drag-handle-symbolic.svg`, fill currentColor, in the gresource) as the first prefix with tooltip "Drag to reorder"; a `gtk::DragSource` on the handle carrying the preset uuid, a `gtk::DropTarget` on each row; a drop calls `move_preset(uuid, row index)` through `with_db_mut` + `write_ok(.., Failed::Save)`, then `on_changed()` and the rebuilder; Alt+Up / Alt+Down (`EventControllerKey` on the row) moves by one and keeps the focus on the moved row. Proof: MANUAL scenarios.

## 3. Android
- [x] 3.1 `meditate-android/ui/main.slint`, Manage rows only: a handle (`assets/drag_handle.svg`) left of the star; DragArea on the handle (`data` from a Rust pure callback, `slint::DataTransfer` plain text = uuid; row dims while `dragging`), a DropArea per row (uuid read back by a Rust callback, a line at the row while `has-drag`); callback `preset-move(string, int)`. `src/ui.rs`: `on_preset_move` calls `move_preset`, failure as the star tap's, then reload the chooser, the home list and `refresh_widget`. Proof: MANUAL scenarios.

## 4. Text and docs
- [x] 4.1 "Drag to reorder" in nine languages, Linux (`update-translations.sh`), `msgfmt --check`, `cargo test -p meditate i18n::`.
- [x] 4.2 README presets line; `EVENTS.md` `preset_*`: `created_iso` is also the preset's place in its mode's list.

## Notes
- `a_preset_dropped_on_its_place_writes_nothing` passed against the stub (a stub writes nothing); proved by removing both no-write guards: FAIL, restored.
- `long_names_wrap_and_rows_grow` (Android source-text test) looked 4500 characters past a list's start for the name; the handle and drop area pushed it past that, window widened to 6500 (the test's point is unchanged).
- Android drop feedback: the dragged row dims (`preset-drag-index` set from the DragArea's `dragging`), a 2 px primary line marks the landing edge of the row under the finger.
- Builder check (fresh agent): (1) the vendored Android backend has no accessibility bridge (no TalkBack for any Slint element; adding one touches third_party/): the Android second way can't work, Janek: drop it on Android. (2) Flickable takes a quick vertical swipe: press then drag; no auto-scroll; feedback via dim + line (proposal). (3) DragArea `data` is a `data-transfer`, built and read by Rust callbacks (`slint::DataTransfer`, plain text). (4) ties happen after normal multi-device use, not only from outside: whole-mode respacing kept, the order stays the same everywhere (my call, proposal). (5) respaced times from the earliest, 1 ms apart, empty = earliest (my call). (6) v26.10.2 already takes `created_iso` from update events: confirmed, older devices follow.

## Review (fresh agent)
- 1 CRITICAL, fixed after asking Janek ("Nur das gezogene"): every preset the move passed got a full-row update, so a sync could drop an edit made elsewhere to one of them, or bring back one deleted there (a later update beats a delete). Now only the moved preset gets a time between its new neighbours (`time_between`, checked against the neighbours as text, since the lists sort by text); no room: the mode is respaced. Test first: `an_edit_elsewhere_to_a_preset_the_move_passes_is_kept` failed (A came back, B's rename lost), then passed; `presets_with_the_same_time_can_still_be_moved` now moves into a gap with no room (proved: disabling the respace fails it).
- 2 IMPORTANT, fixed: Android `dropped` swaps the model before the source's `changed dragging` runs, so a row could stay dimmed; `dropped` now clears `preset-drag-index` first. Device check.
- 3 IMPORTANT, fixed: GTK `add_prefix` order unknown; the handle is now moved to the front of the prefix box explicitly.
- 4 MINOR, fixed: the GTK drop target accepted text from other apps as a MOVE (the source app would delete it); now only drops from inside the app. Also from "cannot verify": the rebuild ran inside the drop handler and destroyed the dragged row mid-drag; the move now runs on idle after the drop.
- 5 MINOR, fixed: `a_move_past_the_end_lands_last` (passed at once: the clamp existed since 1.1; proved by loosening the clamp: FAIL, restored) and `two_devices_moving_before_they_sync_end_in_one_order` (failed against the first design, passes now).

## Fix review (fresh agent)
- 1 CRITICAL, fixed: the respace fallback rewrote the whole mode, and moves made it reachable: two devices' moves into one gap got equal times, and halving a gap ran out after about 15 moves. Now `key_between` extends the lower neighbour's text with digits (`digits_below` when the upper one starts with it), so there is always room, plus five digits from the moved uuid; to the top: the upper neighbour's time (`time_at_start`, reading past a digit tail) minus 1 s; to the end: now, or the lower text with digits if that is later. Only equal neighbours (outside data) respace. Tests first: `many_moves_into_one_gap_write_only_the_moved_preset` (event count off after the gap ran out) and `moves_into_one_gap_on_two_devices_leave_room_between_them` (A came back) failed, then passed; the top part proved by limiting `time_at_start` to the whole text: FAIL, restored.
- 2 MINOR, fixed: a move to the end took the last time plus 1 s, so a preset saved within that second sorted above it; now `now`. `a_preset_moved_to_the_end_stays_above_one_saved_later` failed, then passed.
- 3 MINOR, fixed: the respace starts from the earliest readable time (`min`), as EVENTS.md says.

## Second fix review (fresh agent)
- 1 CRITICAL, fixed: the uuid tail mapped 16 hex digits onto 9 (1 in 32768 pairs of ids collide), so two devices' moves into one gap could get equal text and the next move respaced the mode. Now the first 8 hex digits become 16 digits one to one. `moves_of_two_presets_into_one_gap_never_share_a_place` (ids 0000... and 9999..., equal under the old mapping) failed (respace: 10 events), then passed.
- 2 MINOR, fixed: `lo + "5"` grew the text by 6 bytes per move when swapping the top two rows. `just_above` takes the shortest start of the lower text with one digit raised. `swapping_the_top_two_again_and_again_keeps_the_place_short` failed (1235 bytes after 200 swaps), then passed. Guarded against non-ASCII foreign text (no slicing inside a character, no byte overflow).
- 3 MINOR, fixed in the docs: a first preset with an unreadable time or another time zone also respaces on a move to the top; only foreign data has that, proposal.md and EVENTS.md now say so.

## Third fix review (fresh agent)
- 1 CRITICAL, fixed: a key from a move into a middle gap is cut inside the date, so it can't be read as a time; a move to the top above such a key respaced the mode (247 of 1000 random moves in the reviewer's simulation). The top key is now built from the text alone: the first preset's text with its first digit above 0 lowered; nothing reads a preset's `created_iso` as a time any more (`time_at_start` deleted; a respace starts from now). `a_move_to_the_top_after_moves_in_the_middle_writes_only_the_moved_preset` failed (respace: 8 events), then passed.
- Added `random_moves_write_only_the_moved_preset` (seeded xorshift, 600 moves on two devices, sync every 50: order, one event per move, one order after sync, short keys). Passed with the fix; proved by forcing the top branch to fail on long text: FAIL at step 10, restored.

## Fourth fix review (fresh agent)
- 1 IMPORTANT, fixed: the tail came from the first 8 hex digits, which the bundled presets share, so the two Box Breath seeds moved into one gap on two devices got equal text. Now the last 8. `the_bundled_box_breath_presets_moved_into_one_gap_on_two_devices_leave_room` failed (respace: 10 events), then passed.
- 2 MINOR, fixed: repeated moves to the top grew the text by about one character per move or two (150 after 200); the lowered digit is now followed by a 9. `swapping_the_top_two_again_and_again_keeps_the_place_short` tightened to under 120 after 200 moves to the top: failed at 150, then passed.
- 3 MINOR, fixed in EVENTS.md: a move to the end writes the lower text with digits while the clock is behind it.

## Fifth fix review (fresh agent)
- 1 IMPORTANT, fixed: a move to the end with the clock behind the last text appended "5" plus a tail every time (3425 characters after 200 swaps of the last two). Now it raises the last digit below 9 before the last text's own tail and cuts there, so the text never grows. `swapping_the_last_two_while_the_clock_is_behind_keeps_the_place_short` failed (3425), then passed.
- Set aside (reviewer's own): two random v4 ids sharing their last 8 hex digits (about 2^-32 per pair, moved into one gap at the same time) still meet; the respace then repairs it.

## Sixth fix review (fresh agent)
- 1 IMPORTANT, fixed: cutting the last text at its last digit below 9 moved the place forward each time (seconds, hours, years, then "9..."), so after about 30 swaps of the last two with the clock 1 s behind, presets saved later sorted above it for years. Now `count_up`: the text without its tail (only when the last 16 characters are digits), counted up by one in its last digits, same length, same time. The test now also asserts the place stays within the other device's second: it failed ("9915..."), then passed.
- 2 MINOR, fixed: EVENTS.md's end bullet now matches the code.

## Seventh fix review (fresh agent)
- 1 IMPORTANT, fixed: `count_up` stripped the last text's tail and counted in what was left; a middle move's key is cut short at a coarse digit (an hour, a month), so each swap of the last two moved it an hour or a month ahead. Now the count runs in the tail itself (only a second tail on top is dropped), never in the time. The test now also starts from a cut-short key (`2999-01-01T01` + 16 digits): it failed (`2999-01-03...`), then passed.

## Whole-mechanism review (fresh agent, fuzzing a Python port)
- About 220k moves on 2 to 6 devices (clocks right, ahead, behind, 1970), with creates, deletes, undo-restores, edits and syncs: no respace from own data, no key out of place, no misplaced drop, no disagreement after sync, no panic case. Longest text in random runs 57 to 74.
- 1 MINOR, accepted with a `ponytail:` comment: moves dropped at the same edge of one gap a thousand times grow the text (about 160 characters after 1000); `digits_below` no longer recurses (it counted one level per leading 0).
- 2 MINOR, fixed: EVENTS.md said `created_iso` is always RFC 3339, and named only equal neighbours as respace triggers.

## /simplify (four agents)
- Applied: GTK adds the handle before the star in `build_preset_row` (no reaching into the row's prefix box); Alt+Down no longer repeats core's end rule (`last` gone; core clamps, the focus goes to the last row); `count_up` cut written plainly and its impossible UTF-8 fallback is an `expect`; tests share `sync_both` and `longest_place`, `starred_order` inlined. Suites again: core 1425, gtk 164, android 272; clippy (both, CI form) clean.
- Skipped: one helper for the five `preset_update` payloads (four of them predate this change); a shared test sync helper in `db/test_helpers.rs` (other files' copies are outside the change); cutting the Android source-text test at the row's end instead of a window (the window only widened); calling `refresh_widget` only for starred presets (keeps the star/rename/delete pattern); one query instead of find + list in `move_preset` (matches the file); cheaper event counting in the tests (under 2 s).

## Hand-over (step 6)
Builds (debug): Linux `meditate-gtk/builddir/src/meditate` (11:42:20), APK `meditate-android/android/app/build/outputs/apk/debug/app-debug.apk` (11:43:34), `adb install -r`.

Linux (scratch data dir, three or four Timer presets saved first):
1. Manage Presets: each row shows a dotted handle at its left, in front of the star; tooltip "Ziehen zum Umsortieren".
2. Drag a row by its handle onto another row: it stands at that row's place; the home page's starred list follows. No GTK warnings in the terminal.
3. Tab to a row, Alt+Down / Alt+Up: it moves one place and keeps the focus; at the top or bottom nothing moves.
4. Text dragged in from another app onto a row: nothing happens, the text stays in the other app.
5. Box Breath presets keep their own order.

FP5:
1. Manage Presets: a handle left of the star on each row, the names still in full at phone width.
2. Short press on a handle, then drag onto another row: the dragged row dims, a line shows where it lands, on release it stands there and no row stays dimmed.
3. A quick swipe on a handle in a list long enough to scroll: the list scrolls.
4. The home page's starred list and the home-screen widget follow the new order.
5. With sync: the other device shows the same order after a sync.

Commit message:
```
presets: Drag presets into your own order

Presets always stood in the order they were made. On the Manage page
each row now has a drag handle (on Linux also Alt+Up / Alt+Down), and
the order holds in every list of that mode, the widget and on every
synced device, older versions included. A move rewrites only the moved
preset's sort key, so a sync never touches the presets it passed.

Fixes #12
```
