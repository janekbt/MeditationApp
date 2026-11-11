# Open issues

Every open bug, in batches. Refactors and cleanup live in REFACTORING.md; a number named here without its own heading (#214 and up) is a REFACTORING item. Done items are removed; git history has them, and bug numbers stay as they were, because commit messages refer to them.

**Ground rules:**
- Decisions go in core.
- No DB schema or sync wire-format change without a migration, and nothing touches the Android build environment (Cargo.toml, Cargo.lock, build.rs, main.rs, gradle, the manifest, rust-build.sh) without asking.
- msgids stay unless the item says otherwise; new ones get all nine translations.
- TDD, then a phone check before the commit.
- Before the next release: run `build-aux/repro-probe.sh`.

## Batches, in suggested order

From the audits of 2026-10-09 and the whole-repo sweep of 2026-11-10 (REFACTORING.md; its bugs moved here, numbers kept). Ordered by harm: data first, then failures the user never hears of, then the rest. Each batch is one /spec change with one device test; the REFACTORING items named in a batch belong in the same change. Done batches are removed (git history has them).

| Done | Batch | Items | Why this place |
|:---:|---|---|---|
| [ ] | DD. Passwords and edits lost or doubled | #488, #451, #387, #407, #422, #423, #497 | what the user typed or saved goes missing |
| [x] | K3. Presets and multi-step writes all or nothing | #193, #306, #291, #292, #325, #324 | a preset or pattern that lands halfway |
| [ ] | EE. Failures nobody sees: core and Linux | #347, #366, #489, #251, #247, #344, #365 | the user thinks it worked |
| [ ] | FF. Failures nobody sees: Android | #409, #410, #411, #425, #426, #427, #428, #448 | the user thinks it worked |
| [ ] | GG. Android Edit Session | #472, #473, #475, #478, #479 | wrong times, lost words, popping bars |
| [ ] | Q. Sync efficiency | #92, #100 | wasted network, disk and rebuilds |
| [ ] | W. Sync that stops on one bad item | #164, #165, #172, #173, #188, #192, #263-#268 | one bad file or event blocks sync |
| [ ] | HH. Timer engine and bells | #277, #278, #279, #280, #437, #293, #309 | bells and lengths that are off |
| [ ] | II. Linux audio and guided playback | #367, #383, #384, #385, #386, #364, #388, #389, #390, #493 | freezes and silent audio errors |
| [ ] | JJ. Guided files and Local Folder | #446, #447, #413, #443, #250 | freezes and wrong lengths |
| [ ] | KK. Android screen recreate and widget (manifest: ask first) | #406, #424, #435, #445, #449, #450 | rotation, recreate and widget taps |
| [ ] | T. Text that doesn't fit | #159, #434, #452, #462, #463, #474, #436 | information the user can't see |
| [ ] | LL. Keyboard, touch and screen reader | #343, #345, #363, #476, #477, #511, #147 | controls some users can't reach |
| [ ] | MM. Linux small ones | #348, #391, #492, #350, #393 | stale screens, leaks, warnings |
| [ ] | V. Randomised sync test | #162 | sync bugs that only two interleaved devices show |
| [ ] | X. Release pipeline (ask first) | #168, #169, #201, #209 | builds that aren't what F-Droid or users get |
| [ ] | Y2. Slint text fields (upstream) | #196, #198, #210 | a long press that selects nothing, a key that does nothing |
| [ ] | AA. Library major versions (Android ones: ask first) | #204-#208 | crates stuck on an old major get no more fixes |

---

## Decisions only the maintainer can make

Answer these once; each batch waits only for its own.
- #387 (DD): when the Linux keyring can't be opened, keep the file fallback (and how to protect it), or refuse and ask the user?
- #434, #452 (T): may a Log card note preview and a widget row be cut after two lines (as GTK does), or must they show in full?
- #436 (T): should pt-PT get the pt_BR translation, and zh-Hant/zh-TW the zh_CN one, or stay English?
- #473 (GG): fix the 12-hour AM/PM at Meditate's call site now, or wait for the Slint AM/PM fix upstream (parked)?
- #490 (P11): a failed seed at startup: open anyway and tell the user, or refuse the database as Linux does?
- #493 (II): normalise imported bells to -16 LUFS on both apps, or on neither?
- #164 (W): should a newer peer's unknown value keep blocking the pull on purpose, or be skipped and kept for a later build?
- #445 (KK): fix the rotation during a file pick in the manifest (protected) or in Kotlin only?
- REFACTORING #470: Prep Time and Daily Goal as inline steppers like GTK, instead of dialogs?
- REFACTORING #85: should a ringing end bell keep ringing when its settings change? #197: plain show/hide without the slide? #171: `synchronous=FULL` around Save and Edit?
- Protected build files (ask first): REFACTORING #167 and #175, #440 (bells as Gradle assets), #441 (Kotlin tests in Gradle), batches X and AA.
- REFACTORING "Considered and rejected": now that every refactor gets done eventually, should any of those come back?

---

## DD. Passwords and edits lost or doubled

Data the user typed or saved goes missing, moves, or exists twice.

### #488 Editing a Log entry on Android can move its start to now
- The edit page seeds date and time with `Local.timestamp_opt(..).single().unwrap_or_else(Local::now)` (`android ui.rs:8718-8721`); for a start in the hour that repeats when clocks go back, `single()` is None, the fields show now, and Save rebuilds the start from the fields (8818-8830), so changing only the note moves the session to today and syncs it. GTK seeds with `glib::DateTime::from_unix_local`. Fix: keep the stored start unless date or time was changed; seed with `earliest()`.

### #451 A crash while saving the sync password loses it
- `MeditateKeychain.storePassword` truncates and rewrites `sync_secret.json` in place (55-58); `MeditateDropFile.write` already writes a temp file and renames. A full disk or a kill mid-write leaves an unreadable file, sync stops and the user must enter the password again.

### #387 One failed keyring open moves the sync password into a file whose key sits next to it
- Where: `keychain.rs:214-240`: when a store can't open the portal or D-Bus Secret Service, it switches to the self-keyed file backend (`nextcloud-sync.keyring` plus `nextcloud-sync.master` in the same folder); once the master file exists, every later launch uses that backend (220-224) and never tries the keyring again. Only logged.
- Scenario: the Secret Service starts slowly after login and the user saves sync settings in that moment: the app password is stored in a file that anyone with the user's files can decrypt, from then on, with no notice. Against the "never a password in a plain file" rule.
- Fix (maintainer decision): drop the fallback and tell the user the keyring isn't reachable (retry later), or at least ask before using it and move back to the keyring once it opens.

### #407 Save can store one guided file's uuid with another's name
- Where: Save takes the uuid from the live `guided_sel` (5285-5294), the name from `SESSION_GUIDED_NAME` fixed at Start; an import that finishes during a session overwrites `guided_sel` (5006-5021), which `follow_guided_sel` guards against and this path doesn't.
- Scenario (plausible): an import is transcoding, a widget tap starts a Guided session with file A, the import finishes and selects file B; Save writes uuid B with name A, and per-file stats count the wrong file.
- Fix: read the uuid from the session record (#395); don't change the selection under a running session.

### #422 Saving a new interval bell with a changed volume can create it twice
- Where: `insert_interval_bell`, `find_interval_bell_by_id`, `update_interval_bell` (8117-8137) are three writes without a transaction.
- Scenario: the volume update fails (the sync holds the database): "Save failed", the editor stays open, but a bell at the default volume is stored; Save again adds a second one, and the bell rings twice. Against "multi-step writes are all-or-nothing".
- Fix: #354's `insert_interval_bell(.., volume)`.

### #423 Re-saving after a sync can store a duplicate pattern name
- The vibration editor only re-checks the name when it is edited (`ve_recompute_save` 7245), Save doesn't check again (7597), and a DB error counts as "not taken". A same-named pattern synced in while the editor is open is saved as a duplicate. Fix with #415.

### #497 Opening a pattern on Android cuts its points to the length's limit
- Where: android `ui.rs` `on_pattern_edit` (about 7378-7386) sets `ve-points-max` from `max_points_for_duration_s` (one point per 100 ms, `vibration.rs:249`) before `ve-points`; the points control clamps a stored count above it and `on_ve_points_changed` resamples. Linux keeps the stored points (`vibration_editor.rs:188`).
- Scenario (seen 2026-11-10, device test of one-path-per-table): "Tiny-Dots", 19 points at 0.5 s (3.0 s with 19 points since May; on 2026-10-08 the phone saved 0.5 s, the Duration field's minimum in tenths, keeping the 19 points, likely a typed value below 5 during a device test; a typed length then didn't cut the points, it does now), opened on the phone and saved unchanged: stored with 5 points, synced to every device.
- Fix (maintainer, 2026-11-10: like Linux, own change): opening keeps the stored points; the limit applies only when the user changes the length or the points.

- Plausible, narrow window (sweep, slice 8): after Save commits (which deletes the snapshot), the 60 s heartbeat (gtk `timer/imp.rs:2912`) or closing the window (`save_snapshot_now`) can write the snapshot again before the continuation runs `reset_mode` and cancels the heartbeat; the next start then recovers the same session a second time. Fix: cancel the heartbeat and skip snapshot writes while `saving` is set.

## K3. Presets and multi-step writes all or nothing

Done 2026-11-10 (change writes-all-or-nothing). Moved on: the session-start half of #291, the load-path defaults the review found, and an unreadable setup in the active-preset check go with #308 (P9 and P15); a deleted default label (#498) and the general "read before locking" (#499) wait for P15 below.

## EE. Failures nobody sees: core and Linux

AGENTS.md: every failure tells the user. One reporting path per app is the class fix; Linux has it since P3 step 1 (`db::show_toast`, `show_in_view`, `write_ok`).

### #347 Closing the window right after a delete hides a failed delete
- Where: `close_request` (`window/imp.rs:140-144`) commits pending deletes; a failure toast goes to the window being destroyed.
- Scenario: delete a session and press Ctrl+W within the 5 s undo window; the delete fails, the user never sees it, and the session is back at the next start.
- Fix: on failure keep the window open (or tell the user before closing).

### #366 A bell's switch stays flipped when saving it fails
- Where: `bells.rs:262-275`: a toast, but the switch isn't reverted.
- Scenario: the toast fades, the switch shows On while the database says Off, and the next session silently doesn't ring that bell. Fix: revert under a guard, or rebuild the row.

### #489 On Linux, a preset that can't be read does nothing when tapped
- `timer/imp.rs:3955-3963` returns on a failed read or a `PresetConfig::from_json` error (a preset from a newer version); Android's `apply_preset_json` says it failed. Same class: #382.

### #251 Plausible: a corrupt vibration pattern plays nothing, silently
- (Same class as #291.) `vibration_patterns.rs:60` turns unparseable `intensities_json` into an empty envelope (`unwrap_or_default`), while renaming the same row fails with `Decode`. Only a damaged row or a newer peer's format gets there; the user hears no cue and sees no message.

### #247 Plausible: a bundled-pattern seed can fail on a user's own pattern name
- `seeds.rs:92-101` doesn't ignore a duplicate name, unlike labels and presets (74-78, 127-131). If the seed runs on a database that already has a user pattern called "Pulse" (an upgrade from before the seed existed), the seed fails at startup. Not reproduced.

### #344 The database-open error points to a place the user can't reach
- Same on Android: #425.
- Where: `application.rs:258-264` says "Check the diagnostics log in About → Troubleshooting", but the error window has no menu and no way to About.
- Fix: a button that opens the diagnostics (or the About dialog) on that page.

### #365 A failed read shows an empty bell, preset or sound list
- Where: `bells.rs:135-137`, `presets.rs:202`, `sounds.rs:173` turn an error into an empty list.
- Scenario: a locked database shows "No interval bells configured" or an empty chooser; the user may recreate what exists. Same class as #291 and #308. Also a failed sound-library read names every interval bell's sound "Missing" (both apps' bell lists and Setup rows, through `sound_name`/`resolve_sound_name`), so the user may re-pick working sounds.
- Fix: show the error on the page. The "Missing" half (a failed sound-library read) goes with REFACTORING #284 (P6), which passes the read error on.

## Bugs that wait for a refactor

Each is fixed in the REFACTORING package that rewrites its code, not before (2026-11-11). Under each package, the refactor item that covers it.

### P6. Bells, presets and breath

- #365, the "Missing" half: with #284 (the list half is in EE above).

### P7. Guided files, Local Folder and imports

With #300:

#### #305 One unreadable line stops the whole import
- Where: `let rec = record?;` in all three importers (`data_io.rs:284, 367, 413`).
- Scenario: a file with 500 good rows and one row the csv reader rejects (a stray quote, invalid UTF-8). The import fails with "CSV error"; the question "import the readable part?" that `ParsedImport` exists for is never asked.
- Fix: push the record as unreadable and go on; do with #300.

#### #310 A corrupt database row during export is reported as a "CSV error"
- Where: `data_io.rs:74` maps `DbError::Decode` to `DataIoError::Csv`. Fix: #300.

### P9. Stats and Log

With #297:

#### #308 A failed read shows on the Stats page as "no data"
- Where: `insights.rs:55-80` turns every read error into 0 or empty (`unwrap_or_default`, `unwrap_or(0)`), not even logged; `goal::today_from_db` the same.
- Scenario: the database is locked at refresh; Stats shows an empty history or a 0-minute goal ring as if the user had never meditated.
- Fix: return the error and show it; do with #297. Same class as #291 and #251.
- From K3 (2026-11-10, maintainer: with #308), the same class at other reads:
  - Session start (#291's other half): `bells.rs` `session_bells_from_db` reads the interval bells with `unwrap_or_default`, so a failed read starts a session with no interval bells, silently; `SessionSettings::from_db` reads every other setting the same way.
  - Setup's loaded values: a failed read at a page load puts defaults into the shell's state, and the next write starts from them. Linux: `load_breathing_settings` (4-4-4-4 and the default length into the Cells), then a phase step (`adjust_phase`), a length commit (`set_breathing_duration_secs`) or "Save current setup" (`snapshot_current_setup` takes its Box Breath timing from the Cells) stores and syncs the defaults without a word. Both apps: a failed read of the Timer length leaves 10 minutes in the length cell, which "Save current setup" and a preset tap's Undo snapshot then store. Android's Box Breath step and length commit already start from `try_breathing_from_db` (`breathing_to_change`). A core `edit_breathing(db, |pattern, secs|)` would serve both apps (Related: REFACTORING #500).
  - The active-preset check: `preset_config::active_presets` logs an unreadable setup (`preset.active`) and shows no preset active, with no message.
- Package split: the Stats page with #297 (P9); the K3 sub-bullets (session start, Setup's loaded values, the active-preset check) with #502, readers that return `Result` (P15), the Box Breath part with #500 (P6).

### P11. Startup

With #228, #100:

#### #246 A failed integrity check is only logged
- (Related: #100, #228, #258.) `mod.rs:308-313`: `quick_check` failures go to the diagnostics log and the app carries on, so the user keeps writing into a damaged file without knowing. AGENTS.md: a failure that only reaches the log is a bug. Not refusing to open is deliberate; telling the user is missing. Do together with #100 and #228 (where quick_check runs).

With #483:

#### #490 A failed seed: Linux refuses the database, Android only logs it
- GTK's open runs the seeds with `?` (`db/mod.rs:417-418`), so a failed seed shows the database as unavailable; Android logs `seed_all_non_audio FAILED` and carries on (`ui.rs:10512-10517`), so missing defaults go unsaid. Decide one rule in #483.

#### #351 A crashed session that can't be recovered at startup is only logged
- `application.rs:161-165`: `finalize FAILED at startup` goes to the log; the session from the crash is silently not saved. Tell the user, and keep the snapshot for a retry.

### P14. GTK structure

With #302:

#### #307 One damaged line in the diagnostics log stops its trimming for good, or drops the newest lines
- Where: `diag.rs:169` reads with `map_while(Result::ok)`, which stops at the first line that isn't UTF-8.
- Scenario: a kill mid-write leaves a cut multi-byte character. With fewer than 2000 lines before it, the trim returns early at every start and the log grows without limit; with more, the rewrite keeps only lines from before the bad byte and drops everything newer, the part needed for triage.
- Fix: #302 (read bytes, split on newlines).

With #379:

#### #382 Recovery and label-conflict failures only reach the log
- Android side: #426, #427. Same class: #489.
- Where: `recovery_dialog.rs:66-83` (Push My Data) and `117-140` (Wipe Local) log a failed prepare or a missing DB; `labels.rs:483` logs a failed Keep Both, then marks the conflict decided.
- Scenario: remote data lost, the user taps Push My Data while the write fails: no toast, no sync, the warning stays and the user thinks recovery ran. Keep Both failing: the same conflict dialog comes back with no word why.
- Fix: `write_ok` (log and toast) in one `run_recovery` (#379); don't mark the conflict decided on failure.

With #312:

#### #323 A failed crash-recovery snapshot only reaches the log
- Where: `timer/imp.rs:2863-2870` (`write_in_progress_snapshot`) and `2880-2887` (`clear_in_progress_snapshot`).
- Scenario: the disk is full or the database locked; the session runs with no crash protection and nothing says so, so a crash loses it. A failed clear leaves a phantom session for the next start (the comment admits it). Android the same (`ui.rs:3101`).
- Fix: tell the user once per session that recovery is off, and show a failed clear like any failed save.

With #338 (one core `retry_now()` for "Sync Now" in both apps):

#### #340 "Sync Now" drops a failed write without a word (Android)
- Same class on Android: #427. GTK fixed in sync-after-real-writes (says "Save failed").
- Where: Android `ui.rs:9734` (`let _ =`).
- Scenario: the write fails (locked or full database); the sync runs and the popover keeps saying "Some files weren't uploaded" with nothing saying why. Fix: show "Save failed" as GTK does, or a core `retry_now()` (#338).

### P15. Core speed and smaller ones

With #504:

#### #498 A deleted default label leaves a "default" preset unable to pick a label
- Where: `preset_config::apply_writes`, the `(true, None)` label arm, writes the mode default only while the default label exists.
- Scenario: the mode's default label was deleted (not merged; a merge now carries such presets along), the user picks "Walking" and applies a preset saved with the label on and none picked: "Walking" stays, and the preset never shows as active. As before K3.
- Fix: let a label delete do what `merge_labels` does for these presets (pin them, or switch their label off), or store presets with a real label uuid (Related: REFACTORING #504).

With #501:

#### #499 Unchanged writes still wait for the write lock
- Where: `set_setting`, `edit_bell`, `edit_preset`, `edit_guided_file`, `update_label`, `update_session` open `tx()` (BEGIN IMMEDIATE) before seeing that nothing changes.
- Scenario: a Setup refresh writes stored values back while a sync holds the lock: the refresh waits out busy_timeout and can say "Save failed" for a write that changes nothing. K3 fixed it for the Box Breath phase edit, the seeds and crash recovery.
- Fix: one "read first, lock only for a change" helper in core for all of them (Related: REFACTORING #501).
- Also check: `Database::open` stamps `PRAGMA user_version` at every open; if that takes the write lock, an open during a sync write still waits (from K3's review; on 2026-11-11 the code shows `init` stamps it at every open with no check, `db/mod.rs:381`, and a header write needs the write lock).

With #299: the daily goal is never capped at 1440 on write or read (`goal.rs:38-48`; `DAILY_GOAL_MAX` is unused there). From the sweep, not checked to the end.

## FF. Failures nobody sees: Android

The Android half of EE.

### #409 A widget tap that can't start says nothing
- Where: a deleted or not-yet-synced preset only logs "deep-link preset gone" (979), a mode mismatch logs "deep-link ignored"; a starred Local Folder file whose length isn't read yet selects the file and never starts (1058-1066: `ready` is false, and `poll_local_folder` 1571-1586 doesn't start when the length arrives).
- Scenario: tap a widget row for a preset deleted on the other device, or a fresh local file: the app opens on Setup, nothing runs, no message. The starred-file "not found" path (1050) does tell.
- Fix: a snackbar for each; start once the length arrives when the tap asked for it.

### #410 Creating a label can report "Save failed" for a label that was saved
- `create_label_in_db` (3405-3426) returns `None` when reading the uuid back fails after a successful insert; the caller shows "Save failed", and a retry is refused as a duplicate. The comment says failures are "just logged". Fix: `insert_label` returns the row (#401).

### #411 Tapping a Local Folder file can say "Save failed" for a read
- `select_local_file` (1401) runs the lookup through `local_save`, so a read error shows "Save failed" and marks a running folder refresh as failed. Fix: a read path with its own message.

### #425 After a failed database open, Preferences, About and Diagnostics can't be opened
- Where: `on_preferences_tap` returns at `lock_db()` = `None` (9036, 9057); the open failure itself only reaches the log (10570-10580).
- Scenario: a corrupt file or failed migration: the user taps Preferences to read or share the log and nothing happens, with no word why. Same class as #344 (GTK).
- Fix: say on screen that the database didn't open, and open Preferences without it.

### #426 "Keep both" on a label conflict that fails to save brings the same dialog back, with no message
- `dismiss_label_conflict` failing only logs (8396-8411), then `check_label_conflicts` shows the same conflict again; with no database the slot is already taken and nothing is said. The Android side of #382.

### #427 Recovery, the daily goal and Sync Now fail silently
- Where: the Push My Data and Wipe Local prepare steps (9213-9232, 9253-9286), the daily goal write (9347-9356) and `let _ = clear_refused_uploads` (9734) only log.
- Scenario: the user confirms Wipe Local twice, the prepare fails: the dialog closes, no sync starts, nothing says the data wasn't reset. A failed goal write leaves Preferences showing the new goal while Stats uses the old one. Same class as #382 and #340.
- Fix: `write_ok`/`setting_saved` for each.

### #428 Creating or renaming a label that fails closes the dialog and loses the typed name
- `create_label` and `rename_label` close the dialog and the labels page even after "Save failed" (8220-8231, 8298), unlike every other name dialog here, which stays open for a retry.

### #448 A file pick whose provider fails the name query does nothing
- `queryDisplayName` (`MeditateFilePickerActivity.kt:153`) runs outside the try that writes `err:`; the outer catch (145) only logs. An offline cloud file or a revoked grant: the user picks a file and nothing happens.

## GG. Android Edit Session

One screen, one device test. REFACTORING #469 and #465 touch the same code.

### #472 The app and navigation bars pop over the sliding Edit Session page
- The overlay (5146) is declared before the AppBar (5356) and NavigationBar (5499), so both bars are hidden with `visible: !root.running-page && !root.edit-session-page` (5357, 5500), which AGENTS.md forbids. The bool flips at once while the page slides for 300 ms: on open the bars vanish before the page arrives, on Save or Cancel they paint over it while it slides out. Fix: declare the overlay after the NavigationBar (still before the Duration dialog and the label chooser), drop both gates; check Edit then Duration and Edit then Label on the phone.

### #473 With the 12-hour clock, an edited session time is always saved as AM
- `time-popup` passes `use_24_hour_format: root.clock-24h` (5336) and reads `get_current_time()`, whose PM conversion runs only in keyboard mode (`material-1.0/ui/components/time_picker.slint:592-603`); a dial pick of 2 with PM saves 02:30, and 12 gives noon. The Material bug of the parked Slint AM/PM work. `material-1.0` is protected: convert at the call site, or fix it upstream and update.

### #475 Set in the Duration dialog closes silently at 0:00
- The guard at 6609 skips the write but the dialog closes anyway; a new Log session keeps Save disabled with no word why. The Goal dialog disables Set instead. Fix: `enabled: root.dialog-hours > 0 || root.dialog-minutes > 0`.

### #478 Save in Edit Session can drop the word still being typed (plausible)
- The Save action calls `edit-save-tap` (5195) without `edit-note.release()`; Rust reads `get_edit_note_text()` at once (`ui.rs` 8791), and the release runs only later in `changed edit-session-page`. The running page's end buttons (6488) and Preferences Save (8586) release first for this reason. Not reproduced on the phone.

### #479 A cancelled date or time pick may come back (plausible)
- The Material pickers seed `current_date`/`current_time` from `date`/`time` by binding and re-seed only in `changed date`/`changed time` (`date_picker.slint:332, 506`; `time_picker.slint:408, 616`); after Cancel, reopening on the same session date shows the cancelled day, and OK writes it. Depends on whether the popup keeps its state between `show()`s, not checked. Fix: re-seed before `show()`.

## Q. Sync efficiency

### #92 Every pulled event is uploaded again
- Where: `meditate-core/src/db/events.rs:270-282` (the INSERT leaves `synced` at its default 0, `schema.rs:245`); push `orchestrator.rs:420`.
- Problem: a device pushes back everything it pulled. A new phone joining a remote with 4,000 events uploads them all again; after Wipe Local a device pushes the whole remote log back. The batch count doubles, so compaction runs twice as often. No case needs this forwarding: compaction serialises all events, and Push local and #89 flag the rows.
- Fix: set `synced = 1` on the replay path only (`append_event_returning_newness` is shared with `emit_event`). Reword the comment in `tests/sync_roundtrip.rs:857`.

### #100 Every sync attempt runs the full database open with an integrity check
- Where: `meditate-core/src/db/mod.rs:253-317` (quick_check at 296-305); gtk `sync_runner.rs:109`; android `sync_runner.rs:102`.
- Problem: each attempt runs the schema setup, a `user_version` write (which takes the write lock before the sync busy timeout is set) and `PRAGMA quick_check`, which reads every page. A failing quick_check counts as "ok" and is never logged.
- Fix: run quick_check once from the startup opens (gtk `application.rs:136`, android `ui.rs:9673`) and log its error. Update the `open()` docs (mod.rs:160, 186).

## W. Sync that stops on one bad item

Decided 2026-11-03: each skipped item stays visible in the sync status that the sync symbol opens (popover on Linux, dialog on Android, since the change sync-status-on-tap), never in the log only. #173 waits for the next metainfo change; #192 waits until it happens again on ureq 3.

### #164 A batch this build can't read or apply stops every later pull
- Where: `meditate-core/src/sync/orchestrator.rs:311-312` (an unparseable batch is fatal and never recorded as known); `db/sessions.rs:882, 889-900` writes an unknown `mode` straight into the CHECK column (`schema.rs:151`; the same for bell `category`, interval-bell `kind`, `signal_mode`, `chart_kind`); `db/events.rs:717-749` replays the whole pull in one transaction.
- Problem: since #43 the device still uploads, but it never pulls again. A damaged batch file on the server never heals, and a newer peer's new mode or category blocks every later event from every device until this device updates; the status stays red.
- Raised again in the 2026-11-10 sweep, to cover with it: an unknown preset `mode` fails the CHECK (`presets.rs:459`); the other CHECK columns (interval bell `kind` and `signal_mode`, bell `category`, `chart_kind`, box breath `signal_mode`); a file in `events/` that isn't a batch (a stray `backup__notes.json`, a name with `%` or `#`) stops every later pull, since the name isn't checked against `<lamport:014>__<uuid>.json`.
- Fix: decide first whether version skew should keep blocking on purpose. Then: an unparseable batch is logged and recorded as known; a value outside the CHECK set skips that event's recompute (the event stays recorded, as unknown kinds are) so a later build's cache walk applies it. One test per case.

### #165 One audio file that fails to download stops the upload
- Where: `meditate-core/src/sync/orchestrator.rs:757, 912` (`Err(e) => return Err(e.into())`), `500` (WebDAV errors skip push); `db/error.rs:103-115` (no length cap on ids).
- Problem: a flaky link or a server error on one bell or guided file fails the pull, so push never runs and new sessions stay on that device. A uuid longer than 255 bytes makes `<uuid>.<ext>` too long for the file system, so every sync fails at the same file.
- Fix: for a per-file GET or write error, log it, leave the uuid unknown and continue, as `NotFound` already does (keep `Unauthorized` fatal). Cap `target_id` length in `target_id_is_well_formed_for` (e.g. 128). One test each.

### #172 A long Line vibration pattern overflows u32
- Where: `meditate-core/src/vibration.rs:145`; replay `recompute_vibration_pattern` (clamps to `u32` through `peer_u32` since 2026-11-10, not to the editor's range); editor cap `meditate-gtk/src/vibration_editor.rs:34` (10 s).
- Problem: above about 463 s, `(2*i+1)*duration_ms` overflows: a panic in debug, a wrong envelope in release. Only a corrupt or hostile synced pattern gets there, because the editors cap the length.
- Fix: compute in u64 and clamp `duration_ms` to the editor's range on replay. One test. Core has no duration bound at all (sweep, slice 6).

### #173 The release-notes parser is fragile
- Where: `meditate-core/src/release_notes.rs:24, 63-64, 74-77, 91-93`.
- Problem: the About release-notes parser escapes `<em>`/`<code>`, misses `<p class=...>` and treats `type='development'` as stable; latent while the metainfo stays plain.
- Fix: a test per markup case when the metainfo next changes.

### #188 A session event that isn't JSON can stop the database from opening
- Where: `meditate-core/src/db/schema.rs` (`ADDED_COLUMNS` fill for `sessions.guided_file_name`), `db/sessions.rs` (`recompute_session`, the named-event lookup), `db/labels.rs:366`; `append_event` stores payloads unchecked.
- Problem: `json_extract` fails on a payload that isn't JSON. In the one-time fill that fails `Database::init` on every open (the fill rolls back and retries). No known writer produces such a payload; found by the second review of f4c6b81.
- Fix: `CASE WHEN json_valid(payload) THEN json_extract(...) END` at all three places; one test with a broken payload in the log.

### #192 GTK: a sync can wait minutes after its push, past the 60 s read limit
- Seen: laptop, 2026-10-14 11:15:55, during batch K's device test (after deleting a guided file). The log ends at `[sync.push] ok events=1 sounds=0 guided=0` with no `sync.done`; the spinner kept turning. The worker thread sat in `recvfrom` on the Nextcloud TLS connection, which had neither sent nor received anything since the push (`ss`: lastsnd/lastrcv equal, over 3 min), although `webdav.rs` sets `timeout_read(60 s)`. The next step after the push is `maybe_compact_events`. At 11:19:50 it ended on its own without an error: `sync.done ... in 235.95s`, so a read waited about 4 min although each read is capped at 60 s.
- Problem: while one sync waits, the in-flight flag stays set and no later sync runs; the user sees a spinner for minutes. If the server never answers, it may never end.
- Ruled out (2026-11-03, with the ureq 3 port): a plain-HTTP read on a fresh or reused connection. `webdav.rs` tests `a_server_silent_after_the_headers_ends_the_request` and `a_server_silent_on_a_reused_connection_ends_the_request` end within the read limit on ureq 2 and on ureq 3; the TLS path is unchecked.
- Fix: find where it blocks, most likely a read inside TLS (test with a self-signed certificate that only the test client trusts); then bound it and test that.

### #263 A push-local recovery cut short leaves the server empty, and the status says OK
- Where: `settings.rs:252-260` (`prepare_push_local_recovery`). Four separate writes and no rerun marker.
- Scenario: after "remote data lost" the user picks "push local". The app dies or gets SQLITE_BUSY after the known batches are wiped but before the events are flagged unsynced. On the next sync, the data-lost check is skipped (nothing known) and nothing is pending, so nothing is pushed and the error is cleared. The server stays empty and the status shows success.
- Fix: #255.

### #264 A temporary server error parks an audio file until the user taps Retry
- Where: `orchestrator.rs:806` (`push_file`). It records every `WebDavError::Server` status as a refusal, including 500, 502, 503 and 504, and a failed MOVE takes the same branch.
- Scenario: an upload during Nextcloud maintenance (503) or a proxy timeout (504) is skipped by every later automatic sync. The file stays on this device until the user notices "not uploaded".
- Fix: only lasting statuses (413, 415, 403 and the like) count as refusals; 5xx fails the sync and is retried.

### #265 A remote audio file over the cap is downloaded again on every sync and dropped silently
- Where: `orchestrator.rs:748, 904`. `ResponseTooLarge` and the second size check just `continue`; nothing is recorded or shown.
- Scenario: a 100 to 101 MB guided file on the server (put there over WebDAV) costs about 101 MB of mobile data on every save. Larger ones stream 101 MB before stopping.
- Fix: record it as known-and-skipped and show it in the sync status, as refused uploads are. Do with #253.

### #266 A full disk is reported as an unreadable server reply
- Where: local write and create-dir failures map to `UnreadableReply` (`orchestrator.rs:730, 892, 1077`, `runner.rs:144`).
- Scenario: the phone's storage fills during a pull; the status points at the server.
- Fix: #257.

### #267 A rate limit on anything but PUT fails the sync, and on MOVE it throws the upload away
- Where: `orchestrator.rs:1025`. After a successful PUT, a 429 on the MOVE deletes the `.tmp`, so the next sync uploads the whole file (up to 100 MB) again, possibly repeatedly under throttling. A 429 on the first PROPFIND fails the sync.
- Fix: #252.

### #268 Re-saving the same server with a trailing slash re-uploads everything
- Where: `settings.rs:73` (`adopt_account`) compares the URL as text, and `prepare_test` (`credentials.rs:185`) only trims spaces.
- Scenario: `https://cloud.example.org/.../u` re-saved as `.../u/` counts as a new account. All trackers are wiped, the whole history is flagged unsynced, and the next sync re-reads every batch and re-uploads every event and audio file to the same server.
- Fix: normalise the URL (trailing slash, host case) before saving and comparing.

- Plausible, smaller, not yet checked (sweep, slice 4): if `record_outcome` fails, it hides a successful sync (`runner.rs:69`); in `sync_with_progress` a push error replaces the pull error before it (`orchestrator.rs:492`); two devices compacting at once can overwrite each other's `compacted.json`.

## HH. Timer engine and bells

Mostly core, host-testable.

### #277 A session stopped while paused reports "Paused", not "Done"
- Where: `mod.rs:849-852`. `ui_state()` checks `is_paused` before the phase, and `stop()` (446) leaves `is_paused` set; `end_buttons()` (834) checks `Stopped` first, so the two disagree.
- Scenario: pause, then Save. If the ended page stays (a failed save), GTK's MPRIS shows a resumable session and its play key calls resume on an ended session, and the Box Breath frame callback (`window/imp.rs:499`, runs while Running or Paused) keeps drawing.
- Fix: #273 with #270; at least match `Stopped` first.

### #278 When a guided track ends early, a bell due before the planned end rings just after the end bell
- Where: `enter_overtime` (`mod.rs:517-523`) goes to `reach_end` without the `skip_due` loop that only `tick_running` runs (`627-633`).
- Scenario: a guided file probed at 900 s with an interval ring due at 899 s; the audio ends at 899.4 s, the end bell rings, and the next overtime tick rings the interval bell a second later.
- Fix: move the `skip_due` loop into `reach_end` (with `max(elapsed, target)`); do with #270.

### #279 After a suspend, every missed fixed bell rings at once
- Where: `fire_due_bells` (`mod.rs:1151-1174`) with `ActiveBell::advance` (`bells.rs:267`): an interval bell rings once for its latest due time after a jump, but each fixed bell flips and fires on its own.
- Scenario: a 60 min Timer with fixed bells at 10, 20 and 30 min; the phone sleeps from minute 5 to 35. The first tick fires three bells (plus any interval bell) on the stacking channel.
- Fix: after a jump, ring only the latest due fixed bell and skip the earlier ones, as interval bells do. Related: slice 6 (bells).

### #280 The bell counter counts the starting bell as heard when it never rang
- Where: `bell_ring` (`mod.rs:938`): `rung += !preparing`.
- Scenario: the per-mode signal override mutes it (`starting_bell_effect` returns nothing), or a suspend through prep past the end skips it (`tick_prep` 586-592); the counter still shows it as heard, "1 of N" before any bell. The end bell has the same blind spot for the override.
- Fix: record in the session whether the starting and end bells actually fired, and count that.

### #437 A pause in Overtime would freeze it and save the wrong length
- `AppState::toggle` pauses any Active state (626-640); core's `toggle_action` makes Overtime a no-op; a paused Overtime then saves through `stop()` with the overtime included. Latent: the Overtime buttons offer no Pause today. Fix with #429.

### #293 The Bells subtitle counts bells that won't ring
- Where: `bells.rs:771-778` (`interval_bells_count`) filters only the stopwatch case; `build_active_bells` also drops a fixed bell at or past the target, a "before end" offset at or past it, and a repeating bell longer than the session.
- Scenario: a 10 min countdown with one bell "at 30 min": the row says "1 interval bell", nothing rings.
- Fix: count what `build_active_bells` would build for the current target.

### #309 The lead-in length rounds twice
- Where: `settings_keys.rs:287`: rounds to whole ms, then half up to 10 ms.
- Scenario: a typed 0.1049 s becomes 105 ms, then 110 ms; rounding once gives 100 ms. Do with the lead-in bullet of #303.

- Plausible, not yet checked to the end (sweep, slice 5): `stop()` during Prep records the prep seconds as the session length (`session/mod.rs:446-457`), against `end_buttons`' rule "Preparation seconds are never saved as meditation"; the apps show no Save during prep, but GTK's `session_ended` writes the value into the in-progress snapshot. Fix in core: `stop` from Prep ends with 0.

## II. Linux audio and guided playback

GStreamer work on the main thread, previews and errors that are lost.

### #367 Picking a sound file freezes the window until GStreamer has probed it
- Fix together with #383 (the guided file probe).
- Where: `sounds.rs:420` runs `probe_duration_secs` (a playbin preroll, up to 5 s, longer without a duration header) on the main thread; the transcode right after is already off-thread and reports `NoAudioTrack` itself. If `metadata` fails (412), the size counts as 0 and the 10 MB cap is skipped.
- Fix: probe off-thread or leave the check to the transcode; treat a failed `metadata` as an error.

### #383 Opening a guided file freezes the window while GStreamer probes it
- Where: `guided.rs:147` runs `probe_duration_secs` on the main thread: up to 5 s preroll, then `duration_by_parsing` up to 30 s for a file without a duration header. `local_folder.rs:163` already does it in `spawn_blocking`.
- Scenario: a long MP3 without a Xing header on the Librem 5: the window hangs for seconds, GNOME may offer to force-quit.
- Fix: `spawn_blocking`, as local_folder does. Same as #367 for bell sounds: fix both together.

### #384 A slow probe rejects a good file as "no audio track", and a failed probe leaks the pipeline
- Where: `guided.rs:1450-1480`: `pipeline.state(5 s)` returns `Ok(Async)` on timeout, so the code reads `n-audio` (still 0) and returns `NoAudioTrack`; the `?` on that state change returns before `set_state(Null)`, leaving the playbin Paused.
- Scenario: a file on a sleeping SD card or a network share takes more than 5 s to preroll: "This file has no audio track". On error, decoder threads and the open file stay until the element is finalized, with a GStreamer warning.
- Fix: treat `Async` as a timeout error with its own text; set the pipeline to Null on every exit (a guard).

### #385 Guided audio that fails mid-session plays nothing and says nothing
- Android side: #447.
- Where: the `GuidedPlayback` bus watch only logs `MessageView::Error` (`guided.rs:1355-1375`).
- Scenario: a codec missing in the Flatpak, or the output device disappears: the timer runs a silent guided session; the user learns nothing.
- Fix: a toast (and stop or pause the session, the user's call).

### #386 A vibration preview keeps playing after leaving the chooser or the editor
- Where: `vibrations.rs:65-69` relies on dropping the chooser to cancel the preview, but the Rebuilder cycle (#368) never drops it; neither `vibrations.rs` nor `vibration_editor.rs:531` stops the preview on hide, as `guided.rs:639` and `sounds.rs:113` do.
- Scenario: preview a 10 s pattern, tap Back (or Save in the editor): the phone keeps vibrating on the next screen.
- Fix: stop the preview in `connect_hidden` on both pages; #368 then frees the slot.

### #364 Closing Preferences doesn't stop the Bluetooth test sound
- Where: the closed handler calls `sound::stop_current()` (`preferences.rs:527`), but the Test plays through `play_bell_preview` (`PREVIEW_MEDIA`, 159).
- Scenario: Length 5 s, press Play, close Preferences: the noise and the woodblock play on. Fix: `stop_preview()`.

### #388 The "Couldn't play bell sound" notice can be used up unseen
- Where: `sound.rs:260-268` sets the once-per-run latch before `surface_audio_error_toast` (270-290), which returns silently without an active MeditateWindow.
- Scenario: the first bell fails while another app has focus: no toast, latch set; every later bell failure that run only reaches the log.
- Fix: set the latch only when the toast was shown. Since P3 step 1 the latch is once per session or preview (Start and each preview re-arm it), and the toast goes through `show_in_view`, which still skips without a main window.

### #389 With the Bluetooth lead-in on, every bell queries the sound devices on the main thread
- Where: `sound.rs:395-399` calls `default_output_is_bluetooth` (487-527) for each bell: starts and stops the pulse device provider, or falls back to a full `DeviceMonitor`, which the code itself says can take 2 s for a sleeping card; the first use of a length also writes the noise WAV on the main thread (415-420).
- Scenario (plausible, not measured): in the Flatpak without the pulse provider, each interval bell rings late and the timer display stutters.
- Fix: cache the answer and update it from device-changed messages, or query off the main thread before the bell.

### #390 A guided preview skips the Bluetooth lead-in and leaves the previous row's button on Stop
- Where: `play_preview_for_guided_file` (`sound.rs:165-181`) calls `set_playing(true)` instead of `start()` and doesn't `notify("playing")` on the media it replaces; `play_preview` (135-150) does both.
- Scenario: on Bluetooth headphones the first second of a guided preview is lost; a bell preview replaced by a guided one keeps its Stop icon.
- Fix: one `play_in_preview_slot` (#375).

### #493 A bell imported on Linux is louder than the same file imported on Android
- GTK normalises to -16 LUFS (`audioloudnorm`, `sounds.rs:742-846`); Android's `MeditateGuidedImport.transcodeToOpusOgg` doesn't. The synced file depends on the importing device. Decide the rule in core (#441's import passthrough, #372).

## JJ. Guided files and Local Folder

Mostly Android; REFACTORING #481 (the Local Folder job in core) and #439 are the general fixes.

### #446 Starting a Local Folder file can freeze the app on a cloud folder
- `MeditateGuided.kt:56-73` resolves the address (one query per path segment) and runs `setDataSource(content://..)` and `prepare()` synchronously on the UI thread inside the lock. On a Nextcloud or Drive provider that downloads in `openFile`, Start blocks for the download; after 5 s Android offers to close the app. Fix: prepare asynchronously.

### #447 A guided track that fails mid-session jumps to Overtime, and a late end can hit the next track
- Where: the completion and error listeners (`MeditateGuided.kt:64-73`) both write EOS, then release whatever `player` holds now, not the one that finished.
- Scenario: (a) a stream error (SD card pulled, cloud stream dropped) rings the end bell as if the track ended, the error only in logcat; (b) a track ends just as a new one starts: the old listener releases the new player and its late EOS forces the new session into Overtime.
- Fix: release only `mp`; report an error as an error (#385 is the GTK side).

### #413 A leftover end-of-track or focus-loss file can act on a new session
- General fix: #439.
- Where: the drop files are not cleared when a session starts (5050, 5092); a Manage Files preview that played to its end leaves one.
- Scenario (plausible, a window of one tick): the preview ends and the user starts a session within 200 ms: the next tick enters Overtime (end bell at once) or pauses the session.
- Fix: clear both at Start, or tag them with the playback they belong to.

### #443 Old Local Folder length results come back
- Where: `MeditateLocalFolder.kt:22, 100-120`: the `lengths` buffer holds every result of the run and is rewritten whole after each file; Rust applies all of it again (`ui.rs` ~1554).
- Scenario: file X fails to read (`X\t0`), the user taps X again and a new read starts, while a folder refresh rewrites the file with the old `X\t0`: the selection is refused as unreadable. After a folder switch, old lines can set lengths on reused row ids. A refresh of N files runs about N²/2 updates on the UI thread.
- Fix: write only new lines and clear on take (#439).

### #250 Plausible: a stale length kept for a replaced local file
- `local_guided_files.rs:141` (`set_local_file_duration`) writes a length without checking the file is still the one it was read from; a rescan in between resets it to NULL and the late write stores the old length for good. Fix: `AND size = ?3 AND mtime = ?4`. Not reproduced.

- Small (sweep, slice 7): folders whose names differ only in case swap places between starts (core `local_folder.rs:41-42`: collected from a `HashMap`, sorted by lowercase name with no tie-break; files have one).

## KK. Android screen recreate and widget

What breaks when Android rebuilds the screen, and the home-screen widget. #445 needs a manifest change or a Kotlin fix (ask first for the manifest).

### #406 Every recreate of the activity leaks a running tick loop
- Where: `build_ui` `Box::leak`s the tick `Timer` (4708) and the snapshot timer (4413), and `android_main` runs again on every activity recreate (the comment at 15-32 says so). The leaked closure holds the old `AppState`, the guided selection and the import slots.
- Scenario (plausible, not traced): after a USB attach or a low-memory recreate, the old timer keeps its state alive; if the new event loop runs on the same thread, the old tick still fires and runs `state.tick`, effects and `on_state_changed` on stale state, next to the new one.
- Fix: let the timers live as long as the window (return them from `build_ui` and hold them next to `ui`), never leaked.

### #424 Every activity recreate re-runs startup cleanup under a running sync or import
- Related: #435.
- Where: `open_database` (10479-10584) runs on every `android_main`; its cleanup deletes every `*.part` file and every file no row owns (`remove_orphan_files`, `remove_stale_import_files`, 10561-10566) on a second connection.
- Scenario (plausible): a sync downloads `guided/<uuid>.part`, the wallpaper colours change and Android recreates the activity: the half-written file is deleted, the download fails or leaves a row without its file. The second connection is then dropped.
- Fix: open and clean up once per process (#419).

### #435 A screen recreated during a guided import undoes its cancel and orphans the result
- `remove_stale_import_files` (955-964) runs on every `android_main` (#419, #424) and deletes the running import's cancel and progress files: a cancel is undone, the transcode finishes, and its result and audio are never adopted until the next start. Plausible, not tried on a device. Fix: once per process (#419).

### #445 Rotating the phone during a file pick leaves an invisible screen on top
- `MeditateFilePickerActivity` returns early in a recreated instance (`savedInstanceState != null`, 38) and never finishes; the copy thread's `finish()` (148) finishes the old, destroyed instance. The translucent activity then takes every touch and the app looks frozen until Back. Kotlin-only fix possible; adding `configChanges` to the manifest **needs maintainer approval**.

### #449 A pause or resume can start a ghost media session
- `ACTION_PAUSE_SESSION`/`RESUME` (`MeditateSessionService.kt:487-494`) lack the `!started -> stopSelf()` guard the other actions have, and `LIVE` in `service.rs:40` isn't reset by `stop_leftover`. A pause after the service died creates an instance with an active MediaSession that nothing stops.

### #450 The widget can show one preset's name and start another
- `hasStableIds()` is true while `getItemId` returns the position (`MeditateWidgetService.kt:104-106`); after unstarring or reordering, a cached row may keep its old name while the tap sends the new row's uuid. Fix: `false`, or the uuid's hash.

## T. Text that doesn't fit

### #159 Text cut off or shortened with "…", found one screenshot at a time
- Rule (Janek, 2026-10-10): every text shows in full at phone width (360 px) in all nine languages. Cut off is not acceptable, and neither is "…": both hide information.
- Seen so far: Android's guided-import dialog title ("Geführte Meditation importi", `main.slint` guided-import card, a 22 px Text with no wrap); GTK's import toast (fixed in batch E with a shorter text); Android's import-question buttons (stacked in batch D); GTK's Type rows squeezing their choices (pinned in batch C).
- Android: Slint clips a Text that neither wraps nor elides, silently. 142 Text elements, 31 wrap, 10 elide. Check 1, a test: every Text showing translated words wraps (`wrap: word-wrap`) and has room to grow; `overflow: elide` is not allowed for them either. An explicit short list is exempt (numbers, the big timer, single symbols).
- Both apps, check 2, a test: slots that can't wrap (buttons, toasts, toggle and tab labels, dialog buttons, header titles, GTK row titles that ellipsize) have every translation measured with the real font at its real size against the slot's width at 360 px, and the test lists each one that doesn't fit. The work is the list of fixed slots and their widths; measuring needs nothing new (Pango in the GTK tests, the same font files for Android).
- Fix what the first run lists: wrap, give the slot room, or shorten the translation; never "…". Changing how buttons are arranged (stacking them, moving them) is the last resort and needs Janek's yes each time (2026-10-18).

### #434 Long Log notes end in "…", a glyph the phone's font lacks
- `truncate_note_for_card` (1113-1122) cuts notes over 120 characters and appends "…"; the test (1280-1284) pins it. On the FP5 the card ends in a box. Against both Android text rules. GTK cuts the same notes with `lines: 2; ellipsize: end`. Decide whether a card preview may be cut (then elide in Slint, #433) or must show in full. Same class: #452 (widget), #462 (Setup and Stats). The comment at main.slint 4402-4411 describes a clip wrapper that does not exist (#461).

### #452 Widget rows cut long names with "…"
- `res/layout/widget_item.xml:22-23, 31-32` (`maxLines="1"`, `ellipsize="end"`). A long preset name or a German subtitle is cut on the home screen. Same class as #434, #462.

### #462 Setup and Stats cut text with "…"
- Where: `overflow: elide` at 4023 (the Guided current file name), 4177 (the empty-starred hint), 4666 (the daily-goal line, 20px bold), 5023 (chart x-axis labels, 9px) and 5059 (the mini-stat values, 22px bold in a third of the width).
- Scenario: Guided with no starred file on a 360 px phone shows "Tap Open File then Import File, or st..." in every language, and never says where to star; German Total "123 Std 45 Min" and the goal "1 Std 48 Min / 2 Std 30 Min" are cut; a long file name ends in a box on the FP5. Widths are estimates from font size, not measured.
- Also without wrap and clipped by BoxedList's `clip: true`: the BY LABEL row name (5115) and the Log section date (4386), so a long label name is cut silently (plausible).
- Fix: wrap or stack each (the mini-stats could stack value over caption); the current-file row's fixed `height: 64px` (4008) becomes a minimum so a wrapping subtitle fits. Same class: #434, #452, #474.

### #463 The bell volume reveal can clip its own content (plausible)
- RevealVolume opens to a hand-summed `full-height: 105px` (2090) instead of measuring; a translation that wraps "Volume", or a bigger system font, is cut at the bottom. Fixed by #453's measured `Reveal`.

### #474 Six dialog titles are cut instead of wrapped
- Filter Sessions (5589), Custom Time (6556), Preparation Time (7985), the guided import title (9032), Daily Goal (9239) and the label conflict title (9491) are 22px Texts without `wrap` in a 280-340px card; ConfirmDialog's title wraps (1631). A long translation is cut. Same class: #462. Fixed by #465.

### #436 A Chinese or Portuguese phone outside zh-CN and pt-BR gets English
- `translation_candidates` (1344-1357) tries `lang_REGION`, then `lang`; only `zh_CN` and `pt_BR` exist, so zh-Hans, zh-Hans-SG, pt-PT and pt fall to English. Fix: map the script (`Hans`) and the bare language to the bundled one (whether pt-PT should get pt_BR is the maintainer's call).

## LL. Keyboard, touch and screen reader

Z (#147, small touch targets) is part of this package.

### #343 Space on a focused Log card starts a meditation
- Where: the window's Space shortcut runs in the Capture phase (`window/imp.rs:1079-1099`) and only skips text fields and dialogs, so the card's own Space (open the edit dialog, `log/imp.rs:597-599`) never fires. The same happens on the "Has notes" switch in the filter popover.
- Fix: only take Space when the timer page is visible, or skip any focused widget that handles Space itself.

### #345 "Load more" can't be reached with the keyboard
- Where: `log_view.blp:66-71`, `focusable: false` (a workaround for focus restore after a dialog closes).
- Scenario: a keyboard or screen-reader user can't load anything past the first 15 sessions.
- Fix: keep it focusable and fix the focus restore where the dialog closes.

### #363 Holding + on a Preferences number writes hundreds of synced events
- Where: the lead-in Length (`preferences.rs:132-144`, 0.01 s steps) and Daily goal (77-88) write `set_setting` and trigger a sync on every value change; the bell edit page's time rows the same (`bells.rs:795-806`, plus a list rebuild, #353).
- Scenario: holding + from 0.10 to 5.00 s writes up to about 490 `setting_changed` events, kept in the event log and uploaded to every device.
- Fix: save when the value settles, as `VolumeRow` does (or on close).

### #476 TalkBack reads most icon buttons as just "button"
- `IconButton`'s accessible label is its tooltip (`material-1.0/ui/components/icon_button.slint:25`), and 14 of the 19 in this range set none: AppBar Add, Filter, Menu (5440-5454), label rename and delete (6794, 6810), interval-bell delete (6220), sound edit and delete (7118, 7124), pattern delete (7286), preset rename and delete (7813, 7820), Manage Imports play, rename, delete (8179-8197). The preset and guided stars (7771, 8136) and the accent swatches (8792) are bare TouchAreas with no role. Fixed in one place by #467's LibraryRow.

### #477 A cancelled touch leaves the vibration drag open (plausible)
- The plot's TouchArea handles `PointerEventKind.down` and `up` only (7570-7580); a back swipe or the notification shade sends `cancel`, so `ve-drag-end` never comes and the Rust drag and preview state stays on until the next touch. Fix: treat `cancel` like `up`.

### #511 Linux: a two-finger swipe doesn't close the interval bell's sound chooser
- Found by the maintainer, 2026-11-11 (laptop test of one-database-type-linux; there before it): a two-finger swipe right on the touchpad closes the sound chooser everywhere except when it was opened from an interval bell's edit page (`bells.rs` `push_edit_page`, chooser via `sounds.rs` `push_sounds_chooser`).
- Fix: find what takes the swipe on that path (reproduce, then compare the edit page's navigation setup with the Setup page's), fix it there; a test where the code allows.

### #147 Android: small touch targets
- Where: `meditate-android/ui/main.slint:641, 701, 1277, 6353, 1782, 1799`.
- Problem: the mode switch, the Stats period toggle and the Sound/Vibration/Both segments are 32 dp tall and the Box Breath steppers 36 dp; Material asks for 48.
- Fix: taller controls or taller TouchAreas (the PhaseTile row is already full width at 360 dp, so it can only grow taller).

## MM. Linux small ones



### #348 A paused Box Breath session redraws at full frame rate
- Where: `window/imp.rs:496-501`: the tick callback continues for `Paused`, so the canvas and three labels are redrawn every frame though nothing moves; battery on the Librem 5. The bell ring already stops when nothing changes.
- Fix: return `Break` while paused and restart on resume.

### #391 Every Local Folder page ever opened stays alive and is rebuilt on every change
- Where: the `choose` closure captures `page` strongly and is connected to the banner inside that page (`local_folder.rs:385-389`): a cycle, so the page is never destroyed, `alive` stays true, and its listener stays in `LISTENERS`.
- Scenario: open Local Folder and a few subfolders, go back: each scan, length read and star tap rebuilds every one of those pages (four DB reads and all rows each, #371). Memory and CPU grow with each visit. Inferred from the code, not measured.
- Fix: capture `page` weakly. Related: #352, #368.

### #492 Linux never withdraws "Meditation Complete"
- It is sent with the id `timer-done` (`timer/imp.rs:2559`) and never withdrawn; Android clears it when the app comes back (`service::clear_complete`). After saving, the notification stays in GNOME's list (plausible: not checked on the desktop).

### #350 Two `#[allow(deprecated)]`, against the no-suppression rule
- `window/imp.rs:1065` (`set_help_overlay`/`ShortcutsWindow`) and `application.rs:215` (`style_context_add_provider_for_display`, which is the non-deprecated function, so that one is stale). Fix the cause or drop it. Same class: #393.

### #393 `#[allow(dead_code)]` on `PKGDATADIR`
- `config.rs:6`, against the no-suppression rule; the constant has no user. Delete it (#379). Same class as #350.

- Plausible, to measure (sweep, slice 10): the reference cycles in REFACTORING #352 keep every visited bell, sound and preset page and every name dialog in memory (same class as #391).

## V. Randomised sync test

### #162 Sync tests only cover the interleavings someone thought of
- Where: `meditate-core/src/sync/` tests (`FakeWebDav`, `Sync::new`), `db/events.rs` replay.
- Problem: most sync bugs so far (#40, #101 and others in batch A) needed two devices editing in an order no hand-written test tried. `label_name_collision_is_idempotent_on_replay` replayed in the same order on both sides and missed #101.
- Fix: one test in core: two or three in-memory devices share one `FakeWebDav`; a seeded xorshift64 (the one bell jitter already uses, no new crate, so Cargo.lock stays untouched) picks a few hundred random steps: create, rename, delete or merge a label, preset, guided file or vibration pattern, add, edit or delete a session, change a setting, sync one device, compact. At the end every device syncs twice and all must hold identical data (compare sorted rows of each synced table). Run a fixed list of seeds in `cargo test`, and print the seed and the steps on failure so it reproduces. Optional: an `#[ignore]`d variant that runs many seeds for a manual soak.
- Then: a short "sync rules that always hold" section in EVENTS.md (devices converge; a delete at the same or later time wins; same names settle on the smaller uuid), each pointing to its test.
- Every failure it finds is a real bug: fix it in the same batch with its own small regression test.

## X. Release pipeline (build environment: ask first)

### #168 A release APK can build without its reproducibility pins
- Where: `meditate-android/android/rust-build.sh:41-50, 76`.
- Problem: a missing platform 34 prints nothing and a missing build-tools 31.0.0 only warns; both exit 0 with a signed APK that F-Droid's check rejects. RUSTFLAGS is a space-joined string, so a repo path with a space breaks the remap flags.
- Fix: for `PROFILE=release`, fail when either pin is missing (debug keeps the warning); use `CARGO_ENCODED_RUSTFLAGS`. Build environment: ask first, then run the repro probe.

### #169 Release CI: unpinned actions and images, and a light run can be picked
- Where: `.github/workflows/flatpak.yml:27-181` (`@v6`/`@v7`, image by tag, `tonistiigi/binfmt` untagged and privileged at 146); `build-aux/attach-flatpaks.sh:24-26`.
- Problem: whoever controls those tags can change what Linux users install. attach-flatpaks takes any successful run for the commit, including a `scope=light` run with no bundles (`flatpak.yml:107, 131`), and then fails at the download without saying why.
- Fix: pin each `uses:` to a commit SHA and each image to `@sha256:`; in attach-flatpaks, pick a run that has both artifacts.

### #201 Android: targets API 34, and the native library isn't built for 16 KB pages
- Where: `meditate-android/android/app/build.gradle` (`compileSdk 34`, `targetSdk 34`, `ndkVersion '27.2.12479018'`); the platform 34 pin in `rust-build.sh`.
- Problem: Google Play takes new apps only at the current target level (35, probably 36 by now) and with native code aligned for 16 KB memory pages, which NDK 27 doesn't do by default. F-Droid doesn't require either yet, but a stale target makes Android treat the app as legacy and makes a later Play listing a build-environment change first.
- Fix: check Google's current required level, raise compileSdk/targetSdk to it, and build for 16 KB pages (NDK 28, or `-z max-page-size=16384` for the Rust library). Re-test on the phone what the new target changes (notifications, the foreground service, exact timing), then run the repro probe and update the pins. Build environment: ask first. Raised while weighing a Play Store listing, 2026-11-02.
- Same change could end the JDK 17 pin: it exists because the d8 from build-tools 31.0.0 (F-Droid's buildserver) rejects classes from a newer javac, and AGP 7.3 stops at JDK 17. A newer AGP brings its own pinned d8, so both sides match again; needs a fdroiddata MR to the recipe (newer JDK) landing with that release.

### #209 The pinned toolchain is behind, and two crates wait for it
- Where: `rust-toolchain.toml`, `build-aux/setup-android.sh` (`PINNED_NDK`, `PINNED_GRADLE`, `PINNED_KOTLIN`), `meditate-android/android/build.gradle` (Kotlin plugin), the Gradle wrapper, the CI APK's own JDK 21 and NDK (`.github/workflows/flatpak.yml:73-81`, and its JDK rule in `.github/renovate-config.json5`); F-Droid's recipe pins Rust and the NDK as well.
- Problem: the update list (#21) shows newer Rust, NDK patches (r27d), Gradle and Kotlin; AGP and NDK 28 are #201. Two crates are held in `Cargo.lock` for them: kstring 2.0.2 (2.0.5 needs Rust 1.96) and cc 1.2.60 (it compiles the APK's C code; only F-Droid's own rebuild can show that a newer one keeps the bytes).
- Fix: one toolchain step at a time, each with the repro probe, and a fdroiddata MR that changes the recipe in the same release; then lift the two holds (`cargo update -p kstring -p cc`). Until then a bare `cargo update` moves them too: re-hold both with `--precise` afterwards. For kstring, `rust-version = "1.95"` on the workspace crates (meditate-gtk still says 1.75) with Cargo's `incompatible-rust-versions = "fallback"` would hold it and any later crate by itself. Build environment: ask first. Found with the first update list, 2026-11-03.

## Y2. Slint text fields (upstream)

### #196 Android: a long press on a field that has no focus yet selects nothing
- Where: Slint's Android backend (`long_press_timeout` only acts on the focused item) and core `TextInput` (on Android it takes focus on release, `items/text.rs` Released branch). Not our code.
- Problem: long-pressing a word in a field you haven't tapped yet selects nothing; you have to tap first. Android's own fields select the word. Found in the review of #195 (2026-10-18).
- Fix: upstream; the long press would need to focus the pressed field first. Report it there.

### #198 Android: Gboard's "line start" key doesn't move the cursor
- Where: Slint's Android backend, not our code. It maps `KEYCODE_MOVE_HOME` to Home (`androidwindowadapter.rs` `map_key_code`), so the cause is unknown.
- Problem: in Gboard's text editing panel, the |< key left the cursor where it was in a Slint text field; the arrow keys and Select worked. Seen once in the material gallery on the API 35 emulator (2026-10-19), while testing Slint PR #14205; not reproduced, not tried in Meditate or on the FP5.
- Fix: first reproduce it (with and without Select), log which key event or `setSelection` arrives, then fix or report upstream.

### #210 Android: the word under the cursor flickers in the accent colour and changes by itself (GitHub #23)
- Where: Slint's Android backend, not our code. `callback_update_text` (`third_party/i-slint-backend-android-activity/javahelper.rs`) turns the keyboard's composing region into a TextInput preedit: it shows as a selection and moves the cursor to the word's end, the keyboard re-composes, and the loop repeats; Gboard may then insert a suggestion.
- Problem: tapping into a field (sync server URL, session note) highlights a word, flickers and can change it without any typing. Reported in GitHub #23 with a video.
- Fix: upstream in Slint PR #14216 (merged 2026-11-07, after 1.18.1). Decided 2026-11-07 to wait for the release that carries it, then update Slint (see `third_party/README.md`), test on the FP5 and close GitHub #23. A local minimal port (always commit the whole text, drop the preedit branch) was weighed and not taken.

## AA. Library major versions (Android ones: ask first)

Found with the first update list (#21), 2026-11-03. Each is its own change: read the crate's changelog, adapt the code, run the suites, and test on a device. The ones in the Android build change the APK, so they need a go and the repro probe.

### #204 jni 0.21 -> 0.22 (Android)
- Where: every JNI call in `meditate-android/src/`.
- Problem: one major behind on the bridge to the Kotlin services.
- Fix: test every Android feature that crosses JNI on the phone (sessions in the background, bells, vibration, the keychain, the widget, file picking).

### #205 oo7 0.4 -> 0.6 (Linux)
- Where: `meditate-gtk/src/keychain.rs`, `meditate-gtk/Cargo.toml`.
- Problem: two minors behind (0.x, so breaking) on the keyring client that stores the Nextcloud app password.
- Fix: test that an existing stored password is still found after the update, in the Flatpak and the native build.

### #206 gettext-rs 0.7 -> 0.8 (Linux)
- Where: `meditate-gtk/src/main.rs`, `meditate-gtk/src/i18n.rs`, `meditate-gtk/Cargo.toml`.
- Problem: one breaking release behind on the Linux translations.
- Fix: run `cargo test -p meditate i18n::` and start the app in two languages.

### #207 roxmltree 0.20 -> 0.21 (Android too)
- Where: `meditate-core/src/sync/webdav.rs` (WebDAV PROPFIND replies), `meditate-core/Cargo.toml`.
- Problem: one breaking release behind on the XML parser for server replies.
- Fix: the sync tests cover the parsing.

### #208 material-colors 0.4 -> 0.5 (Android)
- Where: `meditate-android/src/theme.rs`, `meditate-android/Cargo.toml`.
- Problem: one breaking release behind on the library that turns the system colour into the Material palette.
- Fix: compare screenshots of light and dark mode before and after.

---

## Later

Ideas and larger improvements, not bugs; the maintainer decides if and when.

### #178 GTK: the Done note field should grow instead of scrolling in 180 px
- Where: `meditate-gtk/data/ui/timer_view.blp:836-841`.
- Fix: drop the inner ScrolledWindow and `height-request`, keep `note_view` (the page already scrolls), and check that `log-note-editor` still draws its background. Android already grows.

### #179 Bundled Box Breath voice cues
- Where: `meditate-gtk/data/sounds/` + gresource, `meditate-android/assets/sounds/` + `src/sounds.rs`, `meditate-core/src/db/seeds.rs`.
- Idea: four short CC0/CC-BY cues ("Inhale", "Hold", "Exhale", "Hold") as box-breath seeds, so the phase chooser isn't empty. The seed is one-shot (`seeds.rs:27-29`), so existing installs need a seed-version step.

### #509 Renaming or deleting a bell sound says nothing and can't be undone
- Where: GTK `sounds.rs` (`present_rename_dialog`, `present_delete_dialog`), Android `ui.rs` (`on_bell_delete_confirm`, the rename). Core has `restore_vibration_pattern` and `restore_guided_file`, but no `restore_bell_sound`.
- Want (maintainer, 2026-11-11, after P3 step 1's laptop test): as for patterns and guided files, "'{name}' deleted" and "'{name}' updated" with Undo, in both apps. The delete keeps the sound file until the offer ends untaken, as guided files do.
- Its own change: a core restore, both apps, nine languages on Android.

### #180 Warn about a missing bell sound before Start
- Where: rows already say "Missing" (`meditate-core/src/bells.rs:304`, gtk `bells.rs:384`, android `ui.rs:3224`).
- Idea: a core check over the active bells; a warning style on the row, a Setup banner, and block or confirm Start, so a bell isn't silent mid-session. Both apps; vibration patterns the same.
- Maintainer, 2026-11-10 (phone test of one-refresh-per-change): deleted an interval bell's sound, the row said "Missing", and a session still started. That should not be possible: Start stays blocked while an active bell's sound is missing (block, not confirm).

### #181 Daily goal history
- Idea: a `goal_history(effective_from, minutes)` table instead of the `daily_goal_mins` setting; on change, ask "from today" or "retroactively"; the heatmap and goal ring look up the value per day. Synced through events; needs a migration seeded from the current setting. Both apps.

### #182 Mindfulness bell during the day
- Idea (tentative): a tab that rings at intervals during set hours. Prototype background scheduling first (Android exact alarms; Linux notifications or feedbackd).

### #187 Android: a Local Folder the system hides shows as empty, without a word
- Where: `meditate-android/kotlin/MeditateLocalFolder.kt` (`walk`, an empty children cursor counts as "ok"); `meditate-core/src/db/local_guided_files.rs` (`apply_local_scan` marks every file missing).
- Problem: seen on the FP5 (Android 15) 2026-10-12: the system's ExternalStorageProvider failed its restricted-path check ("NoSuchFileException: /storage/emulated", "Queried directory ... is hidden") and returned no children. Meditate showed "0 files" and marked all files not found; nothing said that Android, not the folder, was the problem. A phone restart fixed it. Stars and plays were kept.
- Idea: when a scan of a folder that had files comes back with none, keep the files as they were and show the "Folder not reachable" banner with Choose Again, instead of marking all missing. A really emptied folder then needs one confirmation.

### #183 Nextcloud with a self-signed or private-CA certificate can't connect
- Where: `meditate-core/src/sync/webdav.rs` (`HttpWebDav::with_read_timeout`, the Agent config; ureq 3 with its built-in webpki roots only).
- Idea: ureq 3's `TlsConfig` takes other roots: the system store (`RootCerts::PlatformVerifier`, its `platform-verifier` feature) or one pinned certificate. A new feature or crate changes Cargo.lock: ask first.

### #190 The day ends at midnight, also for night owls
- Where: every "today" in core (`time::today_local`, goal, streak, heatmap, Log day groups) counts by calendar day.
- Idea: a "day starts at" time (for example 3:00), so a 1 a.m. session counts for the day the user lived. It changes counting in both apps (goal, streak, heatmap, Log groups, synced sessions on another device), probably as a setting. Raised during batch J, 2026-10-13.

### #200 Android: one test that every UI text is translated
- Where: `meditate-android/src/app.rs` checks single texts per issue (`nextcloud_help_is_translated`, `ui_text_is_translated_whole`); GTK already checks every msgid (`meditate-gtk/src/i18n.rs::every_language_translates_every_text`).
- Idea: one test that reads every `@tr("...")` in `ui/*.slint` and fails when a catalog lacks it or leaves it empty, so a new text can't ship untranslated without its own test. Existing gaps may show up first. Raised during GitHub #18, 2026-11-02.

### #194 Back up everything, not only the Log
- Where: Preferences backup in both apps; the CSV export (`meditate-core/src/data_io.rs`) carries sessions and labels only.
- Problem: without sync, presets, interval bells, Box Breath patterns, vibration patterns, imported sounds and guided files, and every setting live only on the device. A lost or reset phone or laptop loses them; the CSV restores the Log alone.
- Idea: a full backup file (the database, e.g. `VACUUM INTO`, which needs no new dependency, plus the sounds and guided audio) and a restore for it, in both apps. Open questions: what a restore does to the device id, the event log and sync state on a device that syncs, and whether it replaces or merges. Raised by the maintainer, 2026-10-18.

## Checked and not kept

Findings of the 2026-11-10 sweep that turned out wrong, or that the maintainer chose not to fix, so they aren't raised again:
- #249, a session saved twice after a time zone change between the last recovery snapshot and Save: the snapshot is matched by its local start text. Too unlikely for a new column (maintainer, 2026-11-10).
- Cases that need two devices editing at the same time before a sync (maintainer, 2026-11-10: one person uses one device at a time; fix such cases only when almost free, never with a new sync field):
  - A label merge rewrites the presets pinned to the merged-away label as full rows, so a preset edit made on another device before the merge syncs can be lost, or that preset can point at the deleted label again (#243's fix, change writes-keep-peer-edits).
  - A new preset, guided file or pattern named like the base of an existing "(conflict-...)" row passes the name check and may be suffixed later without an event.
  - A Log entry whose start is stored in another text form (a space instead of "T") counts as changed on an unchanged save.
- Two connections minting two device ids (`device.rs:14-31`): the startup seeds mint the row before any sync connection exists.
- Presets' name check without `COLLATE NOCASE` (`presets.rs:129`): the column itself is `COLLATE NOCASE` (`schema.rs:192`).
- "Box Breath's first cue is lost when the first phase is short": `In` is at least 1 s and Box Breath ticks every 200 ms in both apps.
- "`phase_at` crashes on a zero cycle in release": only test code builds that pattern; the wrong doc is in REFACTORING #286.
- "GTK's `d[5..7]` can panic on a short date": core always writes ten characters (goes away with REFACTORING #294).
- "Every Guided refresh writes the remembered file": `remember_guided` skips unchanged values; only the read remains (REFACTORING #321).
- "Syncing with Nextcloud…", "Testing…", "Converting… {}%": a progress ellipsis is fine when nothing is cut (maintainer, 2026-11-10).
- Cancel in the vibration or interval editor commits a half-typed number: it only touches working values the next open resets.
