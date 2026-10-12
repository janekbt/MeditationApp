# Tasks: Guided local folder

Base: aac392c76d3382922867c432a1d0dc9ec7978aa6

## Loop
- [x] 1 Understand (GitHub #4; current guided flows mapped on both apps; UI agreed in the mock-up https://claude.ai/artifact/RHpAtbHNqxPYDeKD4ugThW)
- [x] 2 Propose (approved: 2026-10-12)
- [x] 3 Build
- [x] 4 Review (fresh agent 2026-10-12: FAIL / NEEDS FIXES; fixed below)
- [x] 5 Specs, docs, list (README, ARCHITECTURE, EVENTS; GitHub #4 is not in OPEN-ISSUES)
- [x] 6 Verify and hand over (maintainer tested Flatpak and FP5, 2026-10-12; label renamed to Manage Imports on his request)
- [x] 7 Ship

## Review Focus
- A second device -> nothing local syncs (`local_folder_changes_write_no_sync_event`); the session from a local file syncs with no file link, as Open File today.
- A replayed or pulled event -> none exist for local rows; nothing to replay.
- An old database -> `a_database_from_the_last_release_gains_the_local_folder_table`.
- Wipe Local -> `wipe_local_event_log_keeps_the_local_folder`.
- A deleted file -> `a_starred_file_gone_from_the_folder_stays_marked_missing`; a moved file counts as new (Left out in the proposal).
- An empty folder -> Folders view with no rows shows "No audio files in this folder" (tasks 4.2 and 5.3); no own scenario, the view simply returns no rows.
- A file selected as current, then deleted before Start -> the existing "Couldn't read audio file" path on start; no new scenario.
- A symbolic link loop on Linux -> walk doesn't follow folder links (task 4.1).
- Two folders with the same name at different depths -> rel paths differ; views key by path, not name.
- A file name with `#` or `%` -> GTK plays via `filename_to_uri` (already used); Android uses the content address.
- Another language -> every new string in nine languages, plurals for counts; long names wrap at 360 px (no ellipsis).
- A session recovered after a crash -> counts no play (Left out).

## 0. Before any code
- [x] 0.1 (2026-10-12, Flatpak test build: after an app restart the folder stays readable; choosing the same folder again gives the same portal path `/run/user/1000/doc/44c9852b/Meditate-Test`, one root, stars and plays kept. A full reboot was not tried; the document store is on disk.) Flatpak folder access after restart: `flatpak documents --app=io.github.janekbt.Meditate` lists earlier Open File picks across reboots; then, after task 4.2, choose a folder in the Flatpak, quit, restart, and check it still lists; choose the same folder again and check the path is the same. Proof: device check. If access is lost or the path changes, stop and ask (design.md, Risks). Done so far: earlier file picks are still in the document store.

## 1. Core: the local-only table
- [x] 1.1 Table in `schema.rs`, `db/local_guided_files.rs`. Proof: `a_database_from_the_last_release_gains_the_local_folder_table` (FAIL then PASS).
- [x] 1.2 Scan reconcile (insert, missing on/off, length reset on size/date change, failed scan changes nothing). Proof: `a_scan_keeps_audio_files_at_every_depth`, `a_rescan_keeps_the_length_of_an_unchanged_file`, `a_changed_file_needs_its_length_read_again`, `a_starred_file_gone_from_the_folder_stays_marked_missing`, `a_file_back_in_the_folder_keeps_its_star_and_plays`, `a_failed_scan_changes_no_file`, each FAIL then PASS.
- [x] 1.3 Star, set length, folder location in `sync_state`, rows keyed by root. Proof: `changing_the_folder_keeps_the_old_folders_stars_and_plays`, `local_folder_changes_write_no_sync_event`.
- [x] 1.4 Play counted with the session save, one transaction. Files: `db/session_in_progress.rs`. Proof: `saving_a_session_counts_one_play_of_its_local_file`.
- [x] 1.5 Wipe Local keeps the table. Files: `db/events.rs`. Proof: `wipe_local_event_log_keeps_the_local_folder` (passes at once if the wipe never names the table; then break-check per SKILL step 3.2 and note it).

## 2. Core: views
- [x] 2.1 `local_folder.rs`: Folders view rows and counts, any depth, List view and its four sorts, folder line with reading progress, core hands the apps numbers and rows, no text; each app words them. Proof: `folders_view_lists_subfolders_then_files_by_name`, `a_folder_row_counts_its_own_folders_and_files`, `folders_view_opens_any_depth`, `list_view_sorts_by_each_order_with_ties_by_name`, `an_unread_length_sorts_last`, `folder_line_counts_lengths_still_to_read`.
- [x] 2.2 Starred Files built in core from library and local rows, missing marked. Proof: `starred_files_list_library_and_local_files`; both apps switch to it.

## 3. Translations
- [x] 3.1 New strings in `meditate-gtk/po` via `build-aux/update-translations.sh` and in `meditate-android/lang/*/meditate-android.po`, nine languages each, plurals for counts. Proof: `cargo test -p meditate i18n::`, `msgfmt --check` on every touched file.

## 4. Linux app
- [x] 4.1 Folder walk on a worker thread, no folder-link following; length reading in the background with the existing probe. Files: `guided.rs`.
- [x] 4.2 "Local Folder" button; page with folder row (Change), Folders/List toggle group, Sort by row in List view only, Refresh, folder rows that open the next level with the path as subtitle, star buttons, empty state, "Folder not reachable" banner with Choose Again; view and sort remembered (`settings` keys from core). Files: `data/ui/timer_view.blp`, `guided.rs`.
- [x] 4.3 Picking a local file sets the current file ("Local folder · path · length"); a saved session counts the play; Starred Files from core with the not-found row highlighted and unstar there; tapping it shows the toast. Files: `timer/imp.rs`.

## 5. Android app
- [x] 5.1 Folder mode in `MeditateFilePickerActivity.kt` (ACTION_OPEN_DOCUMENT_TREE, persisted read access); a static Kotlin entry called from Rust over JNI runs the walk and the existing `probe` (from the content address) on a worker thread, results handed to Rust like the pick drop-file. No manifest change.
- [x] 5.2 `MeditateGuided.startAudio` plays a content address.
- [x] 5.3 Slint page and rows matching the Linux page (segmented Folders/List, sort menu, star buttons, empty state, not-reachable bar), declared after what they cover; glue in `ui.rs`/`guided.rs`; view and sort remembered; Starred Files from core with the highlighted not-found row.

## 6. Docs
- [x] 6.1 README features, ARCHITECTURE.md (local-only table, `sync_state` key), EVENTS.md (no events for the local folder), DECISIONS.md if a standing rule comes out of it.

## Notes
- Build 4-6: Linux and Android apps done; GTK walk/lengths on worker threads, Android walk/lengths in `MeditateLocalFolder.kt` (no manifest/Gradle change), playing by content address; picker activity gets a "folder" route. Android host tests: parse tests `a_scan_drop_becomes_the_folder_files`, `a_lengths_drop_gives_each_files_length`, `the_folder_name_comes_from_the_tree_address` (FAIL then PASS). Three source-text tests updated to the new code (play by `&source`, five picker drop-file writes, 13 page headers incl. `local-back`).
- Checks 2026-10-12: core 1336, GTK 115, Android 211 tests pass; both clippy runs clean (only the bundled Material library's old slint warnings); Android-target clippy clean; check-specs passes; debug APK built 18:38. The laptop froze once when tests, clippy and the APK ran back to back with little free memory; reran one step at a time.
- Build 1.x/2.1: every named test failed first against stubs, then passed. Passed at once: `a_session_that_fails_to_save_counts_no_play` and `wipe_local_event_log_keeps_the_local_folder` (behaviour came free); both break-checked (play bump moved outside the transaction; wipe made to delete the table): both then FAIL, restored.
- Extra tests beyond the spec: `the_walk_does_not_follow_folder_links`, `a_walk_of_a_missing_folder_fails`, `a_session_that_fails_to_save_counts_no_play`, `view_and_sort_are_remembered`.
- The Linux folder walk lives in core (`walk_folder`, std only) so it is tested; Android walks in Kotlin and hands core a `FolderScan`.
- Fixture `db/fixtures/schema_v26_10_1.sql` made from `git show v26.10.1:meditate-core/src/db/schema.rs` with the seed constants filled in.
- My call: folder location in `sync_state`, not a new table (design.md).
- My call: Folders view always by name; sorting only in List view (maintainer's request); the toggle sits on the top page only.
- My call: plays counted per device, only for saved sessions; a crash-recovered session counts none.
- My call: missing non-starred files simply leave the views; only starred ones stay visible, in Starred Files.
- My call: Android shows the folder's name only; the tree picker gives no path.
- Cold read (fresh agent) answered in design.md: Android keeps its accurate probe (header alone is wrong for VBR MP3); a pick before its length is read waits for it; Flatpak path stability added to 0.1; view and sort in synced `settings`; the play key is the row id; Android scan over JNI; play count inside the save transaction.

## Review (fresh agent, 2026-10-12)
Fixed:
- Silent failures: every Local Folder write goes through `save` (GTK) / `local_save` (Android) and shows "Save failed"; a failed scan write no longer shows "Folder not reachable"; the Android folder picker failing says "Couldn't open the folder picker" (new string, nine languages).
- Folder changed during a walk: the walk applies to the folder it started on and the new folder is walked next (both apps).
- Import was enabled for a Local Folder pick (Android passed a relative path): Import is off for local picks in both apps (proposal leaves import out).
- Android Start played the file under the folder chosen at Start: the pick keeps its own folder.
- Android lengths lost when a scan cleared the Kotlin buffer, and ids stuck in `reading` when the worker didn't start: buffer no longer cleared; ids released on a failed start.
- Android "Manage Files" + "Local Folder" side by side would cut Russian text at 360 px: stacked.
- GTK list jumped to the top on every length read: one scroller kept across rebuilds.
- GTK local writes started a sync each: they use `with_db` (no event, no sync).
- GTK waiting pick looked its length up only in the current folder: `find_local_file_from_db` (new, tested: `a_file_is_found_by_its_id_in_any_folder`).
- "reading 29 of 41 files" had no plural: now "reading 29/41" (spec and proposal updated).
- Links to files were skipped by the walk: followed (`the_walk_keeps_links_to_files`, FAIL then PASS); links to folders still not.
- Doc comment moved back to `push_guided_files_chooser`; dead `String` in `starred_guided_rows` removed.
- `starred_files_list_library_and_local_files` now also asserts the local entry carries its length (passed at once: the behaviour existed).
Kept (shown to the maintainer):
- After choosing another folder, the old folder's starred files show "Local file · not found". They are not in the chosen folder; they come back when it is chosen again.
- Android resolves a file's content address with one query per folder level (at Start, on the UI thread): a few ms for normal depths.
- File names that aren't valid UTF-8 are skipped on Linux.
- Stale review points: docs and the task ticks were done after the reviewer read them; task 0.1 is a device check at hand-over.

## Checks after the fixes (2026-10-12)
- core 1338, GTK 115, Android 211 tests pass; GTK/core clippy, CI clippy and Android-target clippy clean (only the bundled Material library's old slint warnings); check-specs passes.
- Debug APK: `meditate-android/android/app/build/outputs/apk/debug/app-debug.apk`, 19:07.

## Device test steps
Linux (Flatpak, task 0.1 too):
1. Guided: Local Folder: choose a folder with subfolders. See files with lengths filling in ("reading N/M"), folders first, counts per folder; open a folder three levels down; Back goes up one level.
2. List: sort by each of the four orders; Folders view has no sort row. Quit and reopen: view and sort kept.
3. Tap a file: it is the current file ("Local folder · path · length"), Import greyed. Start, let it end or stop, Save: its play count is 1.
4. Star a file in the folder view and in the list: it appears in Starred Files as "Local file". Delete or rename it outside the app, Refresh: Starred Files shows it highlighted "not found"; tapping says so; the star button there unstars it.
5. Quit, restart the Flatpak, open Local Folder: the folder is still readable. Choose the same folder again: the stars and plays are still there (task 0.1).
Android (FP5):
6. Same steps 1-4 on the phone; the folder picker shows the system folder dialog; Manage Files and Local Folder are stacked.
7. Play a long file from the folder and check Settings > Apps > Meditate > Storage: no growth by the file's size.

## Proposed commit message
guided: play guided files from a local folder (GitHub #4)

A Local Folder page in Guided: browse your own folder by subfolder or
as one list sorted by name, length or plays, star files, and play them
in place without copying. Lengths, stars and play counts stay on the
device; nothing syncs. A starred file that goes missing stays visible
as "not found".

Fixes #4
