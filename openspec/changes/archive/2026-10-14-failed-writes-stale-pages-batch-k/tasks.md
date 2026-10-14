# Tasks: Failed writes and stale pages (batch K)

Base: 9d21c02df652183a339bea439af589303a34a4b1; suites on it: green (`cargo test --workspace`, 1713 passed)

## Loop
- [x] 1 Understand (question pass by a fresh agent: six questions, five decided as my call below, one to Janek (Undo reselects); all nine issues re-checked in the code, all still present; two #52 sites already fixed: GTK guided rename, sound delete; Android pattern delete no longer dead-taps; current lines below)
- [x] 2 Propose (approved: 2026-10-13)
- [x] 3 Build (1731 passed; clippy clean on host, CI form and Android target)
- [x] 4 Review (fresh review: R1-R8, all held and fixed; second review on the fixes: PASS with three minor, R9-R11 fixed, R11 decided by Janek; third review on R9-R11: R12 fixed)
- [x] 5 Specs, docs, list (check-specs green; OPEN-ISSUES: batch K removed and ticked, #191 added and pointed at write_ok; no other doc names the changed code)
- [x] 6 Verify and hand over (laptop steps 4-6 and phone steps 7-11 passed, Janek 2026-10-14; steps 1-3 skipped by Janek; 1735 passed, clippy clean on host, CI form and Android target, check-specs and msgfmt green)
- [x] 7 Ship (deltas merged, presets and backup-import new areas; archived 2026-10-14)

## Review Focus
- A rename that fails with a duplicate name -> keeps its "«X» is already taken" text, not "Save failed" (helper checks duplicates first).
- A failure on a write the user didn't start (a star toggle, a bell switch, a slider step in the bell editor) -> "Save failed" too; a slider can raise it several times in a row, so a new failure toast dismisses the previous one (adw would queue them). (my call)
- Android Log delete failing inside the Undo-commit path -> the message is shown on the next main-loop turn, after the old snackbar is hidden; otherwise it is overwritten at once.
- Several Log deletes at once, one fails -> only that row stays; the others go.
- GTK guided delete failing -> "Delete failed" instead of the Undo toast, and the audio file is kept (the delete runs at confirm, the file removal on the toast's end).
- Android Log delete failing while a newer notice is on screen (the commit runs because a new notice replaces the Undo one) -> "Delete failed" waits until the snackbar is free, so the newer notice keeps its Undo. (my call)
- Android Log commit with the database unavailable -> counts as a failed delete: rows come back, "Delete failed". (my call)
- GTK Override failing -> stays on the preset page, so Override can be pressed again. (my call)
- No database open at all (`with_db_mut` gives nothing) -> counts as a failure in GTK too, as Android's `write_ok` does. (my call)
- Flatpak export: the save dialog hands back a document-portal path; the temporary file next to it must be allowed there. Checked on the laptop Flatpak in the device test (export over an existing backup).
- Pattern preview that can't play (pattern deleted by a sync while its chooser is open) -> same "Playback failed".
- #124 during a running session or on Done (Android) -> the selection is not touched, Save still reads it.
- #124 GTK: the selected file deleted or renamed by a sync pull -> the same re-resolve runs after a pull, outside a session and Done, as on Android. (my call: same rule in both apps)
- Chooser refilled after a pull while the user has a row's preview playing -> refill keeps the current tick; the preview state lives in its own toggle.
- Box Breath without a length (stopwatch) -> no target, same as now.
- Export: the temporary file sits next to the backup (same folder, so the rename can't cross disks); Android exports to its cache first, unchanged.
- Old database, second device, another language -> no schema or sync change; one new GTK string with all nine translations.

## 1. #170 Export (core)
- [x] 1.1 `a_failed_export_keeps_the_old_backup` in `meditate-core/src/data_io.rs`: write "old" to the path, drop the sessions table, export -> Err, file still "old", no `<path>.tmp`. Expected: FAIL (file emptied), then PASS.
- [x] 1.2 `an_export_replaces_the_backup_and_leaves_no_temp_file`: one session, export over "old" -> file starts with the header, no `.tmp`. Passed at once (the write already replaced the file); proven by removing the rename: it then failed.

## 2. #52 GTK failed writes
- [x] 2.1 `no_write_result_is_dropped` in `meditate-gtk/src/db/mod.rs`: in presets.rs, labels.rs, log/imp.rs, vibrations.rs, bells.rs, sounds.rs, guided.rs, preferences.rs no `let _ =` on a database call and no `with_db_mut(...)` statement whose value is thrown away. Expected: FAIL (about 20 sites), then PASS through one helper `write_ok(anchor, tag, result, Failed::Save|Delete) -> bool` next to `surface_duplicate_toast` (duplicate errors keep their own text; it toasts on the nearest ToastOverlay, so the bell edit page got one). Also found and fixed: guided rename Undo (guided.rs ~1028) and sound delete (silent), not in the list.
- [x] 2.2 Add msgid "Delete failed" to GTK, nine translations copied from the Android .po; `cargo test -p meditate i18n::`, `msgfmt --check`.
- [x] 2.3 `a_failed_delete_keeps_the_audio_file` in `guided.rs`: the audio removal is reached only on a successful delete (as `sounds.rs` does). Expected: FAIL, then PASS.
- [x] 2.4 `a_failed_delete_brings_the_card_back` in `log/mod.rs`: the commit calls `commit_delete_in_place` only on success, else toasts and invalidates the Log. Expected: FAIL, then PASS.

## 3. #80 Android failed writes
- [x] 3.1 `failed_dialog_writes_say_so_and_close` in `app.rs`: Override, preset rename, preset delete, guided rename, guided delete, sound rename, sound delete and pattern delete show `save_failed` or `delete_failed` on Err, close their dialog on every exit, and preset delete shows `deleted` only on Ok. Expected: FAIL, then PASS.
- [x] 3.2 `a_failed_log_delete_keeps_its_row`: `commit_pending_deletes` removes only the ids it deleted and queues `delete_failed` through a zero-delay timer. Expected: FAIL, then PASS.

## 4. #78 Streak (Android)
- [x] 4.1 `saving_or_undoing_a_session_refreshes_the_streak`: `on_save_tap`, `on_edit_save_tap` and Recovery Undo call `refresh_stats` after `reset_log_feed`. Expected: FAIL, then PASS.

## 5. #84 Names
- [x] 5.1 `label_and_sound_renames_reach_widget_and_bell_list` (Android): `refresh_after_label_change` calls `refresh_preset_chips` and `refresh_widget`; sound rename and delete call `populate_interval_bells`. Expected: FAIL, then PASS.
- [x] 5.2 `bell_pages_reread_names_when_shown` (GTK `bells.rs`): the Interval Bells page and the bell edit page re-read sound and pattern names on `shown`. Expected: FAIL, then PASS.
- [x] 5.3 `setup_rereads_bell_rows_when_shown_again` (GTK `timer/imp.rs`): Setup's page re-reads its four bell subtitles when shown again. Expected: FAIL, then PASS.

## 6. #124 Selected guided file and open choosers
- [x] 6.1 `manage_files_changes_follow_the_selected_file` (GTK): Manage Files' change callback and the after-pull refresh re-resolve the selected uuid (clear or rename `guided_pick`; not during a session or on Done), then refresh the row and the hero. Expected: FAIL, then PASS.
- [x] 6.2 `a_pull_follows_the_guided_file_and_refills_choosers` (Android): after a pull, the guided selection is re-resolved outside a session and Done, the pattern chooser refills through `refresh_after_pattern_change`, an open bell chooser through `repopulate_bell_chooser`; `refresh_after_pull` still calls `refresh_bell_rows`. Expected: FAIL, then PASS.

- [x] 6.3 `undo_of_a_deleted_guided_file_reselects_it` (GTK `timer/imp.rs`, Android `app.rs`): a selection cleared by a delete remembers the uuid; Undo (GTK: the next re-resolve that finds it again with nothing else picked; Android: the GuidedDelete undo branch) selects it again. Expected: FAIL, then PASS.

## 7. #136, #140, #141 (Android)
- [x] 7.1 `the_box_breath_counter_reads_the_running_session`: the tick takes the target from `completion_duration_secs`; no `bb_target_secs` left (update the `end_session` test at app.rs ~1517). Expected: FAIL, then PASS.
- [x] 7.2 `back_from_the_interval_editor_cancels_it`: the Back branch for the interval editor calls `invoke_interval_editor_cancel`. Expected: FAIL, then PASS.
- [x] 7.3 `a_preview_that_cannot_play_says_so`: bell and pattern previews set the preview uuid only when the duration is above 0, else stop the toggle and show `playback_failed`. Expected: FAIL, then PASS.

## Current sites (from step 1, base 9d21c02)
- GTK #52: presets.rs 292, 317, 368, 583, 607; labels.rs 408; log/imp.rs 769; vibrations.rs 475, 514, 649; bells.rs 247-249, 327, 419, 771; sounds.rs 954; guided.rs 770-772, 1097 (+ removal ~1150), 1139; preferences.rs 86-87.
- GTK #84: bells.rs 216, 534, 552 (pages pushed at 77, 791); Setup subtitles timer/imp.rs 4125-4232. #124: timer/imp.rs 617-627, `rebuild_starred_guided_list` 3401, `refresh_hero_for_idle` 2018, `refresh_guided_selected_row` 3540.
- Android ui.rs: Override 6241-6287, preset rename 6354-6388, preset delete 6408-6447, guided rename 5971-6020, guided delete 5858-5866, sound rename 6824-6846, sound delete 6864-6891, pattern delete 7288-7310, `commit_pending_deletes` 2940-2985; #78 5340, 8905, 8618; #84 `refresh_after_label_change` 2397, `populate_interval_bells` 4056; #124 `refresh_after_pull` 2870 (callers 4855, 9270); #136 Cell 4354, set 4555, tick 5172; #140 9875; #141 7731-7787, 7663-7726.

## Review fixes
- [x] R1 More writes failed silently in the batch's own files. GTK: preset, label, guided and pattern rename, preset and label create, Add bell, pattern editor save (all now `write_ok`; `surface_duplicate_toast` removed, as `write_ok` keeps the clash text). Android: guided star, interval bell switch and delete, preset create (snapshot no longer taken, so a retry works), pattern editor save. The GTK scan also flags read-as-nothing writes (`.and_then(..ok)`) and duplicate-only handling; the Android test lists the new handlers. Labels, sounds and the pattern editor pages got their own ToastOverlay, or a failure there showed on the hidden root page.
- [x] R2 The export test now goes through `export_csv`: with `<path>.tmp` blocked it fails and the old backup is whole (fails against the old direct write).
- [x] R3 Setup's Box Breath phase cue rows re-read their names on return too.
- [x] R4 The guided selection rule moved to core (`meditate_core::guided::follow_selection`, unit-tested); both apps apply its result. An unreadable library never clears the selection (GTK used to).
- [x] R5 Android preset delete closes its dialog before the lookup, so a preset deleted by a sync doesn't leave it stuck.
- [x] R6 GTK: a new failure toast dismisses the previous one instead of queueing.
- [x] R7 GTK: a name clash from core's error type (Undo restores) keeps "«X» is already taken" (`a_core_name_clash_keeps_its_text`).
- [x] R8 GTK: an Undo whose restore fails no longer leaks the audio (superseded by R11).
- [x] R9 GTK sound and guided imports: no database at all is an import failure (the copied audio is removed, the error shown), not a silent success. The scan now flags every dropped `with_db_mut` statement.
- [x] R10 Android test: preset create and the pattern editor are asserted to stay open on failure (a retry under the snackbar), instead of passing on the Ok arm's close.
- [x] R11 A guided delete's Undo that fails is offered again ("Save failed" with Undo), both apps; the audio stays until an offer ends untaken (Janek, 2026-10-13). GTK: `offer_guided_undo`.
- [x] R12 Third review: R9's import error was untranslated English; it now reads "Save failed" (translated). A name clash on a guided Undo isn't offered again: GTK shows "«X» is already taken", Android "Save failed" (it has no clash text; wording may differ), and the delete stands. GTK test asserts the re-offer says "Save failed"; the scan also counts method-chain statements (`a.b().with_db_mut(..);`).
- Kept: the /spec skill change (the question pass for every change) is Janek's request from this session, not batch K's code; it ships in the same commit.

## Hand-over (step 6)
Artifacts (2026-10-13): laptop Flatpak installed 22:03 (binary holds the new code); APK `meditate-android/android/app/build/outputs/apk/debug/app-debug.apk` 22:04, installed on the FP5 2026-10-14 11:06; Librem binary `target/aarch64-unknown-linux-gnu/release/meditate` 22:06 (sha256 749b8e2685497f83), not deployed.

Making a write fail on the laptop: hold the database for 30 s from a terminal (each app write then waits 8 s and fails; nothing is written by the lock):
`( echo "BEGIN EXCLUSIVE;"; sleep 30; echo "COMMIT;" ) | ~/Android/Sdk/platform-tools/sqlite3 ~/.var/app/io.github.janekbt.Meditate/data/meditate/meditate.db`

Laptop steps 4 to 6 passed (Janek, 2026-10-14); Start with a "Missing" bell still works, which is #180 in OPEN-ISSUES (Later), left there. Steps 1 to 3 skipped by Janek (2026-10-14): a held database is rare and too much effort to stage; covered by tests only.

Laptop:
1. Failed writes: start the lock, delete a preset -> after about 8 s "Delete failed", the preset stays, no Undo. Again with an interval bell's switch -> "Save failed".
2. Log: delete a session, start the lock, let the Undo run out -> "Delete failed", the session is back.
3. Guided Undo: delete a guided file, start the lock, tap Undo -> "Save failed" with Undo again; after the lock ends tap Undo -> the file is back and plays.
4. Names: rename a sound used by an interval bell and by the end bell, go back -> Interval Bells list, bell edit page and Setup show the new name. Delete a pattern used by the end bell (or a Box Breath phase) -> Setup shows "Missing".
5. Guided selection: select a file, delete it in Manage Files -> Setup shows no file; Undo -> selected again. Rename it -> Setup shows the new name.
6. Export over an existing backup -> "Exported N sessions", the file has the new rows, no `.tmp` next to it (Flatpak file portal).

Phone:
7. Streak: save a session that starts a new streak day -> the Timer page's streak line changes without opening Stats.
8. Rename a label used by a starred preset -> its chip and the widget show the new name. Rename a sound used by an interval bell -> the Interval Bells list shows it.
9. Interval bell editor: drag Volume, press the phone's Back -> the sound stops.
10. Box Breath: start 10 min, change Box Breath to 20 min on the laptop and sync, pause and resume on the phone -> the counter still ends at 10:00.
11. Guided after a sync: select a file on the phone, rename it on the laptop, sync -> the phone shows the new name; delete it on the laptop, sync -> the phone's selection clears (not during a session).
Not on a device: a failed write on the phone (no way to hold its database), a preview that cannot play, a name clash on Undo. Covered by tests.

Proposed commit message:
```
gtk, android: Failed writes say so, pages follow changes (batch K)

Both apps check every write in presets, labels, the Log, bells,
sounds, patterns and guided files: a failure says "Save failed" or
"Delete failed" instead of a success, keeps the item, and leaves the
dialog usable. A guided file's audio goes only with its row, and a
failed Undo of its delete is offered again. Sound, pattern and label
names, the selected guided file, open choosers, the streak line and
the Box Breath counter follow changes made elsewhere, and a CSV
export replaces the old backup only once the new one is complete.
/spec now asks a fresh agent for open questions on every change.
```
