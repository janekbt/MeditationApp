# Tasks: preset-keeps-its-bells

Base: 79134ea; suites on it: core 1395 green (Android 272 and GTK preset tests green on the first attempt's tree; re-run before hand-over)

## Loop
- [x] 0 Skills loaded, again after every /compact: `/ponytail ultra`; `slint:slint` (loaded; Android change is Rust glue in ui.rs only)
- [x] 1 Understand (said back, Janek 2026-10-30 "passt")
- [x] 2 Propose (builder check 2 done, Notes; approved: 2026-10-30, Janek "passt so")
- [x] 3 Build (core 1402, android 272, gtk 164 green; host clippy both runs, Android-target clippy clean; GTK clippy proved live with a planted error)
- [x] 4 Review, then `/simplify` after the last review (review, three fix reviews, /simplify, fix review of its fixes; see below)
- [x] 5 Specs, docs, list (check-specs green; README presets line and the update note; no OPEN-ISSUES entry, the bug came from Janek directly; EVENTS.md unchanged: ordinary insert and update events)
- [x] 6 Verify and hand over (Janek 2026-10-30 "alle tests gruen", Linux and FP5)
- [x] 7 Ship (delta merged into openspec/specs/presets/spec.md, archived 2026-10-30)

## Review Focus
- Linux saves on every field change -> the first change copies, the edit page follows the new bell, later changes go in place; `the_new_bell_takes_further_edits_in_place` plus MANUAL.
- A preset deleted later -> its bell stays (off); nothing deletes bells, as today.
- Another device -> the copy is an insert event, the switch-off an update event; both apply like any edit. An older version editing in place: named in the proposal.
- A failure halfway -> copy and switch-off in one transaction; the shells show their existing save-failed message.
- A sync bringing an edit of the old row from an older version -> the preset rings it as edited (today's behaviour, accepted).

## 1. Core
- [x] 1.1 Tests `editing_a_preset_bell_makes_a_new_bell`, `a_save_without_changes_copies_nothing`, `only_bells_a_preset_rings_are_copied`, `the_new_bell_takes_further_edits_in_place`, `a_bell_no_preset_uses_is_edited_in_place`, `the_new_bell_sits_next_to_the_old_one`, `an_old_preset_keeps_its_bell_too` against a stub `preset_config::edit_interval_bell(db, &IntervalBell) -> Result<BellEdit>`, `BellEdit { InPlace, Copied(IntervalBellUuid) }`. FAIL then PASS.
- [x] 1.2 `Database::copy_interval_bell`: one transaction with its own SQL (the insert/update methods each open a transaction, which can't nest): insert the edited bell under a new uuid with the old created_iso and its real enabled/volume in row and event, switch the old row off; two events. Tested through 1.1.

## 2. Apps
- [x] 2.1 GTK `bells.rs` `write_back_bell`: call `edit_interval_bell`, on `Copied` set the snapshot's uuid to it and show the toast; Delete on the page deletes the snapshot's uuid, not the one captured at build. MANUAL.
- [x] 2.2 Android `ui.rs` interval editor Save (edit branch): call `edit_interval_bell`, on `Copied` show the snackbar (`show_notice`). MANUAL.
- [x] 2.3 The string "Saved as a new bell, your presets keep the old one": GTK `po/` via `build-aux/update-translations.sh` and `cargo test -p meditate i18n::`; Android nine `.po` files and its `Tr` global; `msgfmt --check` each. de: "Als neue Glocke gespeichert, deine Presets behalten die alte."

## Notes
- Builder check 2 (fresh agent), my calls in proposal Decisions: rung bells only, Box Breath out, no copy without a change, GTK Delete follows the copy, neighbour on either side; old presets compare with the settings before the edit, and an edit that makes the new bell equal an old preset's bell copies again on the next edit (accepted, rare). Keep `a_preset_finds_its_own_bell_after_the_bell_was_edited` (older versions edit in place). Janek: a hint on the first copy.
- The first attempt (repair at apply time, derived uuids) was built, reviewed twice and dropped for this design (Janek 2026-10-30); its code was reverted to the base.
- Build: `editing_a_preset_bell_makes_a_new_bell`, `the_new_bell_takes_further_edits_in_place`, `the_new_bell_sits_next_to_the_old_one`, `an_old_preset_keeps_its_bell_too` failed against the in-place stub; `a_save_without_changes_copies_nothing`, `only_bells_a_preset_rings_are_copied`, `a_bell_no_preset_uses_is_edited_in_place` pass with it (in place is today's behaviour), each guard proved by removing it: the unchanged check, the Timer filter, the master-switch filter, the bell-on check, and "always copy" each made one of them FAIL; restored.
- `copy_interval_bell` writes three events (insert, update with the real enabled/volume, the old bell's switch-off) in one transaction; the insert/update SQL moved into `insert_bell_in`/`update_bell_in` so it isn't duplicated.
- GTK: `show_write_failure`'s toast part is now `db::show_toast`, reused for the hint; the unused `update_interval_bell` wrapper became `edit_interval_bell`.
- String without a final period, like the other toasts (my call). de/nl/pl use the apps' own words for presets (Voreinstellungen, voorinstellingen, szablony).

## Review (fresh agent, on the copy-at-edit design)
- 1 CRITICAL, fixed: the GTK toast title is one line (libadwaita 1.7 source: single-line, ellipsize end), so the hint was cut at 360 px. Rendered both at 360 px with the German text: plain cut to "...deine V...", a wrapping custom-title label shows it on two lines. `db::show_toast` now uses a wrapping label (Janek: full sentence, wrapped).
- 2 IMPORTANT, Janek decided "Liegen lassen": a source bell no preset rings after an override stays, off.
- 3 MINOR, changed after Janek: two devices editing the same bell offline made two copies; now the copy's uuid is derived from the old bell's (FNV-1a 128 over "<old uuid>/<n>", the first n not in the list; own hash because std's may differ between builds, no new crate), so the copies merge and the later edit wins. `two_devices_editing_the_same_bell_keep_one_copy` FAIL (3 rows) then PASS; `a_second_copy_of_the_same_bell_is_a_bell_of_its_own` passed at once (random uuids before), proved by taking the first n without the free check: FAIL; restored.
- 4 MINOR, fixed: `another_device_gets_the_copy_as_it_is` replays the events into a second database and compares the rows; passed at once, proved by writing now() as the update's created_iso: FAIL; restored.
- 5 MINOR, fixed: stale ui.rs comment.
- The hint shows on every copy, i.e. once per edit page (the first change), not once ever: matches the MANUAL scenario (my reading of Janek's "einmal").

## Fix review (fresh agent)
- 1 IMPORTANT, fixed (test first, FAIL then PASS): the free check skipped only listed bells, so a deleted copy's uuid came back and a preset still ringing it rang the new edit. Now also skips any uuid with events (`Database::has_events_for`); synced devices hold the same events, so they still pick the same n. `a_deleted_copy_is_not_brought_back_by_the_next_edit`.
- 2 IMPORTANT, accepted as part of Janek's "the later edit wins": the phone copies O to X and saves preset B with X; the laptop, offline, edits O too and derives the same X; if the laptop's edit wins, B rings it. Same as two devices editing one bell before this change; told to Janek at hand-over.
- 3 MINOR, fixed: the THEN said "the later edit", but the sync rule decides (lamport, then device); reworded, and the test checks the copy is on with one of the two edits.

## Fix review 2 (fresh agent)
- 1 IMPORTANT, accepted (Janek, 2026-10-30 "Hinnehmen"): a device offline since before a copy was made and deleted elsewhere derives the deleted copy's uuid on its own edit; after sync the delete wins and that edit is gone. Unavoidable while offline copies must merge.
- 2 MINOR, fixed: `has_events_for` sat between `winning_mutate` and its doc comment; moved above it.

## /simplify (four agents)
- Applied: `insert_bell_in` takes an `IntervalBell` and writes its own enabled, volume, uuid and created_iso, so a copy is one insert plus the switch-off (two events, was three) and the 10-argument helper is gone; Android keeps `BellEdit` instead of a bool; one `sync` test helper; `rung_by_a_preset` follows `apply`'s rule (by row, else by settings when the preset's row is gone), which also covers a preset whose bell was deleted and added back by apply: `a_preset_whose_bell_was_re_made_keeps_it_too` (FAIL under the old rule, checked by restoring it; PASS).
- Skipped: `Uuid::new_v5` instead of the FNV hash (needs a uuid feature, so Cargo.lock changes: build environment, not without asking; the hash stays); dropping the list check before `has_events_for` (rows without events may exist); reading the list and presets once per edit page instead of per GTK save (a handful of rows, not noticeable; would move state into the shells); `apply`'s insert-then-update for a re-added bell's volume (outside this change, works).
- After it: core 1407, android 272, gtk 164 green; host clippy both runs, Android-target clippy clean.
- Fix review of the /simplify fixes (fresh agent): PASS; 1 MINOR accepted: with two identical bells and the preset's own row gone, editing the second one copies although `apply` would ring only the first; an extra copy, nothing lost, the hint says so.

## Hand-over (step 6)
Builds (debug): Linux `meditate-gtk/builddir/src/meditate` (09:41:59), APK `meditate-android/android/app/build/outputs/apk/debug/app-debug.apk` (09:43:15), `adb install -r`.

Test steps, Linux and FP5 alike:
1. Timer: an interval bell at 5 min with sound A, interval bells on; save it as preset "Test".
2. Edit that bell: another sound (Linux: as soon as you pick it; Android: Save). The hint "Als neue Glocke gespeichert, deine Voreinstellungen behalten die alte" shows once and in full (Linux: narrow the window, it wraps onto two lines). The list shows the 5 min bell off and the new bell on, right next to it.
3. Linux: on the same page change more fields (time, volume slider): no further hint, no further bell. Then Delete on that page: the new bell goes, the old one stays (off).
4. Edit the bell again, then apply "Test": the old bell with sound A is on, the edited one off; a short session rings sound A.
5. A bell no preset rings (switch "Test"'s bell off and add a new bell, or use a bell of no preset): edit it: no hint, no new bell.
6. Save without changing anything (Android Save, Linux re-picking the same value): no hint, no new bell.

Commit message:
```
presets: Editing a preset's bell makes a new bell

A preset finds its interval bells in the bell list, so editing a bell
in Setup changed it for every preset that rang it. Now an edit of a
bell a saved preset rings goes into a new bell beside it, the old one
is switched off and kept for the preset, and a short note says so.
Two devices editing the same bell offline make the same copy, so the
later edit wins as before.
```
