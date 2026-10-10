# Open issues (2026-10-09)

One list for everything still open: the Android bugs (formerly ANDROID-BUGS.md) and the refactors left from the audit (formerly REFACTOR-AUDIT.md). Done items are removed; the git history has both old files (`git show 6e81927:ANDROID-BUGS.md`, `git show 6e81927:REFACTOR-AUDIT.md`). Bug numbers stay as they were, because commit messages refer to them.

**Ground rules:**
- Decisions go in core.
- No DB schema or sync wire-format change without a migration, and nothing touches the Android build environment (Cargo.toml, Cargo.lock, build.rs, main.rs, gradle, the manifest, rust-build.sh) without asking.
- msgids stay unless the item says otherwise; new ones get all nine translations.
- TDD, then a phone check before the commit.
- Before the next release: run `build-aux/repro-probe.sh`.

## Batches, in suggested order

From three fresh-eyes audits on 2026-10-09 (code reading plus small GTK, GStreamer and SQLite probes; every finding of the third audit was re-checked by a second agent; nothing tried on a device). Numbers continue after the old list (#1 to #38), so commit messages stay unambiguous. Mark a batch `[x]` when it is committed.

| Done | Batch | Items | Why this place |
|:---:|---|---|---|
| [x] | A. Sync data safety | #40, #66, #69, #43, #88, #68, #90, #94 | data loss across devices |
| [x] | B. GTK session safety | #135, #129, #128, #67, #131, #134 | GTK loses or ends sessions |
| [x] | C. Presets and bell settings | #111, #125, #112, #71, #102, #48, #82, #133 | settings that change silently on other modes or devices |
| [x] | D. Backups and import | #41, #105, #49, #109, #110, #62 | backups that double or will not restore |
| [x] | E. Audio files | #54, #106, #93, #108, #107, #76 | broken, oversized or orphaned audio files |
| [x] | F. Sync account and status | #42, #70, #89, #95, #96, #97, #44, #83 | sync that stops silently or leaks the password |
| [ ] | G. Labels, names and Undo | #74, #75, #86, #101, #103, #104, #65, #122, #158 | names that differ per device, saves that fail |
| [ ] | H. Timer engine | #39, #45, #46, #47, #72, #113 | wrong minutes, bells at wrong times |
| [ ] | I. GTK timer and bells | #73, #126, #127, #77, #130, #53, #132 | GTK controls and bells misbehave |
| [ ] | J. GTK Log | #117, #118, #119, #120, #121, #123 | the Log shows wrong rows and totals |
| [ ] | K. Failed writes and stale pages | #52, #80, #78, #84, #124, #136, #140, #141 | failures that look like success, stale screens |
| [ ] | L. Android platform (manifest: ask first) | #50, #51, #57, #79, #63, #149 | other apps, phone transfer, audio focus, battery saver |
| [ ] | M. Android layout and editors | #139, #142, #143, #144, #145, #146, #147, #148, #160 | hidden controls, lost edits |
| [ ] | N. Stats numbers | #55, #59, #60, #114, #115 | screens disagree |
| [ ] | O. Dates in other languages | #151, #152, #153, #154, #116 | wrong date order and labels |
| [ ] | P. GTK translations | #56, #81, #64, #155, #156 | mixed languages |
| [ ] | Q. Sync efficiency (after G and J) | #92, #98, #99, #100, #137 | wasted network, disk and rebuilds |
| [ ] | R. Tests | #91, #138, #150 | tests that cannot catch the bug |
| [ ] | S. Cleanup | #58, #61, #85, #87, #157 | dead code, wrong comments |
| [ ] | T. Text that doesn't fit | #159 | information the user can't see |

---

## A. Sync data safety

### #40 After "wipe local", new edits lose to the device's own old ones
- Where: `meditate-core/src/db/events.rs:572` (`apply_event_record_only`), `wipe_local_event_log` sets `lamport_clock = 0`.
- Problem: pulling its own old events back does not raise the device's Lamport clock. Laptop was at 500, ends near 301 after the wipe; an edit to a session last changed at 480 loses on every device, and a delete loses to the old insert, so the session comes back. Restoring an old GTK database backup does the same and can create duplicate `(lamport, device_id)` pairs.
- Fix: drop `event.device_id != self.device_id()?`; `was_new` already guards retries. Flip `apply_event_does_not_advance_local_lamport_for_our_own_device_events`.

### #66 A failed manifest read can bring up the "remote data wiped" dialog
- Where: `meditate-core/src/sync/orchestrator.rs:286-288, 630-646` (`manifest_vouches_for_any`).
- Problem: any GET error on `compacted.json` (timeout, 503) counts as "not vouched", so a peer's compaction plus one network blip gives `RemoteDataLost`. Picking "Wipe Local" then deletes the device's unsynced sessions. No test covers a failed manifest GET.
- Fix: return `SyncResult<bool>`; only `NotFound` and a corrupt body mean `false`, any other error is a normal sync error. One test with a `Network` error.

### #69 "Wipe Local" deletes the bundled bells and patterns for good
- Where: `meditate-core/src/db/events.rs:403-407`; `db/seeds.rs:27, 93`.
- Problem: `wipe_local_event_log` deletes bundled rows but keeps the `*_seeded = "1"` settings, so seeds never run again. After Wipe Local the bell chooser is empty, patterns are gone, and interval bells and box-breath phases point at a missing `BUNDLED_BOWL_UUID`.
- Fix: `DELETE … WHERE is_bundled = 0` for `bell_sounds` and `vibration_patterns`; update `wipe_local_event_log_clears_every_event_sourced_table`.

### #43 A failed pull also blocks the upload
- Where: `meditate-core/src/sync/orchestrator.rs:496` (`self.pull()?` before the push).
- Problem: during version skew (phone updated first, sends a value the old laptop's CHECK rejects), every laptop pull fails, so its own sessions never upload until it updates.
- Fix: on `Db`/`InvalidEvent` pull errors, still push, then return the pull error. Keep `RemoteDataLost` and transport errors fatal. One test with a CHECK failure on pull.

### #88 An interrupted audio download is later uploaded over the good copy
- Where: `meditate-core/src/sync/orchestrator.rs:734-743, 769-783` (sound pull), `897-901, 916-930` (guided pull); push `799-841, 941-976`.
- Problem: pull writes straight to the final `<uuid>.<ext>`. A full disk, a kill or a power cut leaves a partial or 0-byte file, and the uuid stays unknown. The next pull skips it (`local.exists()`), and push PUTs and MOVEs it over the author's complete server copy (MOVE sends no Overwrite header), then marks it known. Every device that downloads later gets the cut-off file; the author never re-uploads.
- Fix: one shared helper for both pulls: write `<local>.part`, `sync_all`, rename; remove the `.part` on any error. Test: a leftover `.part` still leads to a full download. #68 relies on `local.exists()`, so do this first.

### #68 A sound or guided file that comes back after a delete never gets its audio again
- Where: `meditate-core/src/sync/orchestrator.rs:715-720, 880-885`.
- Problem: pull only downloads uuids not in `known_remote_sounds` / `known_remote_guided_files`. Deletes remove the local audio (gtk `guided.rs:1124`, `sounds.rs:988`; android `ui.rs:6287`; `remove_files_of_pulled_deletes`) but leave the uuid "known". If a peer brings the row back (higher-Lamport star, or Undo after a peer pulled the delete), it never downloads again.
- Fix: delete the `!known.contains(..)` filter in both pulls; the `local.exists()` check already prevents double downloads.

### #90 A pulled guided-file delete skips the path check
- Where: `meditate-core/src/sync/orchestrator.rs:696-713` (called at 337); the check exists only in `db/events.rs:582-591`.
- Problem: `remove_files_of_pulled_deletes` runs over every pulled event, including ones whose `target_id` was rejected. A batch with `guided_file_delete` and `target_id = "../../../../Music/talk"` deletes `~/Music/talk.ogg`. Anyone with write access to the sync folder can do this. Only `*.ogg` files and only native builds (the scp deploy on the Librem 5); Flatpak and Android stay in their sandboxes.
- Fix: at the top of the loop: `if !crate::db::target_id_is_well_formed_for(&e.kind, &e.target_id) { continue; }`.

### #94 A remote event with a huge Lamport time can stop all saving
- Where: `meditate-core/src/db/events.rs:569-571`; `device.rs:49-59, 66-75`.
- Problem: `observe_remote_lamport` accepts any value. One event with `lamport_ts` 9223372036854775800 (a corrupt file or a broken peer) moves the clock there; seven writes later the clock overflows into a REAL, `emit_event` fails, and every save shows "storage error" on every device that pulled it (SQLite probe). Exactly `i64::MAX` makes the pull fail forever instead. Wipe Local doesn't help: the next pull brings the event back.
- Fix: before `append_event_returning_newness`, log and skip (don't record) events with `lamport_ts` outside `0..=i64::MAX / 2`.

## B. GTK session safety

### #135 GTK: a failed Save is lost once the next session starts
- Where: `meditate-gtk/src/timer/imp.rs:2451-2508` (reset at 2508), 2181; `meditate-core/src/db/session_in_progress.rs:62-86`.
- Problem: `on_save` resets to Idle before the write result is known. If the write fails (#74, a lock held past 1 s, a full disk), the snapshot row is the only copy, and recovery only runs at startup. The next Start in the same run overwrites it, and the session is gone. Android keeps Done open on failure.
- Fix: reset only on success; on error keep Done up with the toast. Make Save and Discard insensitive while the write runs (a double tap would insert twice).

### #129 GTK: the running page can be swiped away, and Start then discards the session
- Where: `meditate-gtk/src/window/imp.rs:530-533`; `timer/imp.rs:2049-2209`.
- Problem: the running page keeps the default `can-pop`, so Alt+Left, the mouse Back button or a swipe goes back to Setup (probe). Start there overwrites the session, its start time and the recovery snapshot: 12 minutes gone. Switching the mode instead saves the session under the new mode.
- Fix: `.can_pop(false)` on the running page (the code's own pop still works), plus an `if self.ui_state() != UiState::Idle { return; }` guard in `on_start`.

### #128 GTK: the Space shortcut eats typed spaces and can start a session
- Where: `meditate-gtk/src/application.rs:424`; `window/imp.rs:852-861`.
- Problem: `win.timer-toggle` is a plain "space" app accelerator, and GTK 4 runs those before the focused widget (probe). A note "felt calm" is saved as "feltcalm"; typing "Morning sit" as a preset name starts and pauses a Timer session behind the dialog; Space on a focused button toggles the timer instead. The Librem 5 on-screen keyboard is not affected.
- Fix: replace the accelerator with a window `ShortcutController` in the capture phase whose callback returns false when the focus is a `gtk::Text` or `gtk::TextView`. (A bubble-phase controller would let Space click focused buttons instead.)

### #67 GTK: a Log delete still waiting for Undo is lost on close or quit
- Where: `meditate-gtk/src/log/imp.rs:701-706, 727-738`, `get_app` at 1215.
- Problem: the row is only deleted when the 5 s Undo toast is dismissed. Probe (libadwaita 1.7): on window close `dismissed` fires but `root()` is already None, so `commit_all_pending` drops the pending deletes; on Ctrl+Q it never fires. The session is back on the next launch.
- Fix: commit pending deletes in the window's `close_request`; make the `app.quit` action close the windows instead of calling `quit()`.
- Share the `close_request` handler with #131.

### #131 GTK: closing the window mid-session loses up to 59 s
- Where: `meditate-gtk/src/window/imp.rs:127`; `timer/imp.rs:2661-2678`.
- Problem: there is no `close_request` handler, and the snapshot is written every 60 s. Closing at 10:59 recovers 10:00, and a note typed on Done is lost.
- Fix: write the snapshot with the current elapsed time in `close_request` (needs a pub(crate) hook on TimerView). Share the handler with #67.

### #134 GTK: a full sync every minute during a session
- Where: `meditate-gtk/src/timer/imp.rs:2624, 2641`; `application.rs:551-558, 581-645`.
- Problem: the recovery snapshot is written through `with_db_mut`, which triggers a sync although it emits no event. A 45 min session runs about 46 passes; each opens the database (#100), reads the keyring, makes 4 HTTPS requests and then rebuilds Timer, Stats and Log on the UI thread (#98). Offline, it shows the warning icon mid-session. Android doesn't do this.
- Fix: `app.with_db(...)` at both places.

## C. Presets and bell settings

### #111 Applying a Box Breath preset wipes the Timer's bells
- Where: `meditate-core/src/preset_config.rs:553-580, 617-656`; `seeds.rs:135-143, 163-188`; callers gtk `timer/imp.rs:3471-3476`, android `ui.rs:1288-1301` and the widget deep link (`ui.rs:920`).
- Problem: `apply` writes the starting bell, prep and `interval_bells_active` and replays the whole interval-bell library for every preset. Those keys are global and only Timer reads them. Tapping the bundled "Box Breath 4-4-4-4" turns off the Timer's starting bell and prep and empties its interval bells, on every device. The widget applies with no Undo, and a user-saved Box Breath preset rolls the Timer bells back to how they were when it was saved.
- Fix: write those keys and run step 4 only for Timer presets (`setup_visibility`, as step 3 already gates the phases). Test: applying a Box Breath preset leaves the Timer bells alone. Related: #71.

### #125 Opening the laptop app resets the phone's Cues setting to "Sound"
- Where: `meditate-gtk/src/timer/imp.rs:1729, 1733-1740, 1110-1117`; reached at launch via `window/imp.rs:96`, `refresh_streak`, 3164.
- Problem: the Cues toggle is built on "both". Without haptics the launch clamp moves it to "sound", and its handler (which ignores `bells_loading`) writes `timer_signal_mode = sound` as a synced setting, on every launch, whenever the stored value is not "sound" (probe). On the phone, Vibration bells then go silent and Both bells stop vibrating.
- Fix: in `setup_cues_signal_mode_toggle`, make the `get_app` closure return None while `bells_loading` is set; add a source test like the existing ones. Values that already synced stay at "sound".

### #112 Laptop sessions fire Vibration bells that cannot vibrate
- Where: `meditate-core/src/session/settings.rs:122-143`; `bells.rs:359-366`; gtk `timer/imp.rs:1550-1635, 3962-3998`.
- Problem: without haptics, Setup shows every Vibration or Both bell as "Sound", but the session uses the stored value. The phone sets the End Bell type to Vibration, it syncs, the laptop shows "Sound", and the session ends in silence. With the Cues override on Vibration, a Sound bell is never fired at all.
- Fix: a core `SessionSettings::sound_only(self)` that sets the override, starting bell, end bell, each interval bell and each phase cue to Sound; GTK calls it in `on_start` when `!has_haptic()`. One test.

### #71 Deleting a custom sound or pattern makes some presets impossible to apply
- Where: `meditate-core/src/preset_config.rs:484-535`; message at gtk `timer/imp.rs:3544`, android `ui.rs:1281`.
- Problem: `apply` returns `SyncPending` ("Wait for sync") for any missing reference and cannot tell "not synced yet" from "deleted". It also checks references that never fire: interval bell sounds with bells off (`:490`), the starting-bell pattern with the bell off (`:507`), Box Breath phase patterns on a Timer preset (`:518`; `snapshot` always stores them). Delete pattern "Soft" used on a phase, and Timer preset "Morning" says "Wait for sync" forever on every device.
- Fix: skip a missing uuid that has a `bell_sound_delete` / `vibration_pattern_delete` event; check phases only for BoxBreath. One test.
- Related: #111 (the write side of `apply`).

### #102 Interval bells are listed in local rowid order, which differs per device
- Where: `meditate-core/src/db/interval_bells.rs:216-223`; `preset_config.rs:350-353`.
- Problem: rowids on a peer follow the replay order, so two bells pulled together can be stored in reverse. The list order differs between devices, and `active_presets` compares the list in order, so a starred preset doesn't highlight on the second device about half the time. Custom sounds and patterns (`ORDER BY is_bundled, id`) have the same display issue.
- Fix: `ORDER BY created_iso, uuid` (created_iso travels in the payload), and `ORDER BY is_bundled, created_iso, uuid` for sounds and patterns. Do this before #48.

### #48 Applying a preset doubles interval bells across devices
- Where: `meditate-core/src/preset_config.rs:617-656` (step 4 of `apply`).
- Problem: every apply deletes all interval bells and inserts new uuids, one transaction per write. Phone and laptop both apply before syncing, and after sync both sets exist, so each bell rings twice. A failure midway leaves the library half deleted; an unchanged preset still writes events.
- Fix: update existing rows in order with `update_interval_bell`, delete extras, insert only the missing ones.
- Do #102 first: updating rows in order needs the same order on every device.

### #82 The preset row counts switched-off bells
- Where: `meditate-core/src/format.rs:567-572`; capture at `preset_config.rs:399-414`; Setup count `bells.rs:540`.
- Problem: library of 3 bells, 1 enabled: the preset says "3 bells", Setup says "1 enabled", 1 rings. A stopwatch preset also counts a "before end" bell that can never ring.
- Fix: count `b.enabled && !is_bell_inert_in_stopwatch(kind, display)`; add a disabled bell to `preset_subtitle_parts_bells_uses_one_or_many`.

### #133 Undo of a preset apply after a mode switch
- Where: gtk `timer/imp.rs:3474-3477, 3575-3577, 1806-1860`; android `ui.rs:8020-8029, 4904-4937`.
- Problem: GTK: tap "Morning" in Timer, switch to Guided, tap Undo within 5 s. The Timer snapshot's label, end bell, stopwatch, Cues and keep-awake are written into Guided, while Timer keeps Morning's values. Android: the Undo does nothing.
- Fix: drop the apply toast on a mode switch in both apps (GTK: dismiss `current_apply_toast` in `on_mode_switched`, as `on_start` does; Android: `finish_notice` in `on_mode_changed`).

## D. Backups and import

### #41 Recovery saves 0-second sessions, and one such row makes the CSV backup unimportable
- Where: `meditate-core/src/db/session_in_progress.rs:182-207`; `data_io.rs:197, 301`; both shells write a 0 s snapshot at start (gtk `timer/imp.rs:2181`, android `ui.rs:4153`).
- Problem: killed within the first minute, the next launch saves a 0 s session ("Recovered 0 min"), which syncs and is exported. CSV and Insight Timer import both fail the whole file on a 0-length row. Normal Save and `hold_ended_session` already drop 0 s sessions.
- Fix: in `finalize_session_in_progress`, delete a 0 s snapshot and return `None`. In both importers, skip 0-length rows instead of failing (update `parse_insighttimer_csv_rejects_zero_duration_with_line_number`).

### #105 A CSV backup carries no session identity and no wall-clock time
- Where: `meditate-core/src/data_io.rs:125-160, 345-368`; `db/sessions.rs:498` (always mints a uuid), 807-848.
- Problem: import gives every row a new uuid and only checks against local rows. Restoring a backup on a new phone and then turning on sync doubles the history on every device (1,500 becomes 3,000, plus "(conflict)" copies of every label); importing during the first sync does the same. The file also stores unix seconds computed in the exporting device's time zone, so restoring in another zone shifts every start (07:00 in Berlin becomes 01:00 in New York) and the duplicate check misses.
- Fix: append two columns at the end: the session uuid and `start_local` (the stored `start_iso`). Import keeps a valid uuid and skips rows whose uuid exists (also a uuid repeated within the file), and uses `start_local` when present. Older apps read fields 0-4 by position and ignore extra columns; old files without the columns import as today. This is not the rejected guided-file column (#7).

### #49 Millisecond timestamps in a CSV land in 1970 and rows are dropped
- Where: `meditate-core/src/data_io.rs:188-192`, `time.rs:73-79`.
- Problem: an out-of-range `start_time_unix` silently becomes 1970-01-01; the duplicate check then keeps one row per duration (200 × 600 s becomes 1) and the import reports success.
- Fix: parse error "line N: bad start_time_unix" when the year is outside 0..=9999. One test.

### #109 Re-importing a backup brings back deleted labels
- Where: `meditate-core/src/data_io.rs:348-350`.
- Problem: labels are created for every name in the file before the duplicate filter runs. Delete "Work", re-import an older backup whose rows all exist: "Imported 0 sessions", yet an empty "Work" label comes back and syncs.
- Fix: filter the rows first, then create only the labels the kept rows use, inside the session transaction (this also settles the import-labels transaction item in TODO.md).

### #110 CSV import errors name the wrong line after a multi-line note
- Where: `meditate-core/src/data_io.rs:188, 284`.
- Problem: the reported line is the record index + 2, which ignores newlines inside quoted notes.
- Fix: `let line = rec.position().map_or(i + 2, |p| p.line() as usize);`

### #62 Label merge is one transaction per session
- Where: `meditate-core/src/db/labels.rs:238-247` (`merge_labels`).
- Problem: 1,500 sessions take 1,500 write locks; a BUSY from the sync worker stops the merge halfway (sessions partly retagged, label still there). Nothing is lost, the merge can be repeated.
- Fix: one transaction with a tx-less update, like `insert_session_tx_less`.

## E. Audio files

### #54 A failed file import leaves a partial file
- Where: `meditate-core/src/sound.rs:215-238` (`safe_copy_no_follow`), used by gtk `guided.rs:400` and `sounds.rs:709`.
- Problem: if `write_all` fails (disk full), the half-written `<uuid>.ogg` stays with no row pointing to it.
- Fix: remove `dest` when any step after open fails.

### #106 Guided imports have no size limit, but sync drops files over 100 MB
- Where: gtk `guided.rs:378-416`; `kotlin/MeditateGuidedImport.kt:107-118`; `meditate-core/src/sync/orchestrator.rs:76, 941-969`.
- Problem: only bell imports check the size. A WAV guide longer than about 10 minutes imports fine, its row syncs, its audio never does, and nothing says why. Every sync on the importing device reads the whole file, finds it too big and drops it.
- Fix: move the 100 MB cap into `meditate_core::sound` next to `MAX_CUSTOM_BELL_BYTES`; refuse bigger files in both import paths with a new message (nine translations); check `metadata().len()` before reading in `push_custom_guided_files`.

### #93 One audio file the server refuses breaks every sync
- Where: `meditate-core/src/sync/orchestrator.rs:424-425, 466-467, 497, 502`; the per-file `?` at 836 and 971.
- Problem: a 413 (proxy body limit) or 507 (quota) on one file makes every sync return an error. Events still upload, but later files never do (a failing sound also skips every guided upload), compaction never runs, the status stays red, and every sync sends the large body again.
- Fix: on a per-file `WebDavError::Server{..}`, log and continue, leaving the file unknown. Keep Network and Unauthorized fatal (a proxy that closes the connection mid-body still shows as Network).

### #108 Interrupted imports leave orphaned audio files
- Where: gtk `guided.rs:310-317`; `kotlin/MeditateGuidedImport.kt:62-100`; android `ui.rs:5135-5142`.
- Problem: imports write `<uuid>.ogg` before the row exists. A kill during a long Android transcode, quitting GTK mid-import, or Cancel in the short window after the worker finished leaves the file (and on Android a `guided_import_cancel.<uuid>` flag) forever.
- Fix: one core startup sweep that deletes `<uuid>.*` files in guided/ and sounds/ that have no row (skip names that aren't uuids, such as `transient.<ext>`). This also covers #54.

### #107 GTK refuses headerless MP3 and raw AAC files
- Where: `meditate-gtk/src/guided.rs:1362-1421` (called at 134, used by both pick paths).
- Problem: the duration comes from a paused playbin, which has none yet for CBR or VBR MP3 without a Xing header, or for ADTS .aac (probe in the GNOME 50 runtime). Open File says "Couldn't read audio file: duration unknown", so the file can't be played or imported. Android accepts the same files.
- Fix: when `query_duration` returns None, run `filesrc ! parsebin ! fakesink sync=false` to EOS and use `query_position` (0.13 s for a 30 min file). Add a headerless MP3 and an .aac fixture. Fix together with #76 (same function).

### #76 GTK: files with `#` or `%` in the name cannot be opened
- Where: `meditate-gtk/src/guided.rs:1261, 1372` (`format!("file://{}", abs)`).
- Problem: probe: "Talk #3.ogg" and "100%25 calm.ogg" fail in playbin; Open File says "Couldn't read audio file". The bell import pre-check uses the same probe.
- Fix: `glib::filename_to_uri(&abs, None)` at both places.
- Fix together with #107 (same function).

## F. Sync account and status

### #42 GTK: a sync that never starts still shows as healthy
- Where: `meditate-gtk/src/sync_runner.rs:108-124, 172`.
- Problem: early returns (Unconfigured, PasswordMissing, keychain error, busy timeout, `get_sync_state`) skip `record_outcome`; the error only goes to the diagnostics log. Saving settings with no password leaves "waiting for first run" forever; a later wiped keyring keeps "Synced N ago".
- Fix: record those errors with `record_sync_error` in `run_sync_attempt`. About 5 lines.

### #70 Changing the sync URL with an empty password field silently stops sync
- Where: `meditate-core/src/sync/credentials.rs:176-178`; keychains key on (url, username) (gtk `keychain.rs:95-99`, android `keychain.rs:42-45`).
- Problem: empty field means `PasswordAction::Keep`. After fixing a URL typo, the toast says saved, every sync fails with `PasswordMissing`, and neither shell records it in the sync status (GTK: #42; Android `ui.rs:230-234` only logs).
- Fix: `prepare_save` takes the previous account and returns `NoPassword` when URL or username changed and the field is empty; the shells' `unreachable!` arms become a toast.

### #89 Changing the Nextcloud account: old history never uploads, and a running pass mixes the two servers
- Where: `meditate-core/src/sync/settings.rs:61-75` (gtk `preferences.rs:419-440`, android `ui.rs:8573-8600`); tracker writes at `orchestrator.rs:343, 457, 784, 837, 931, 972`.
- Problem: a URL or username change wipes the known-file trackers but leaves every event marked synced, so the new server only gets events written after the move; a new phone there sees almost no history until a compaction (after about 50 batches). Save also runs on the UI connection while a pass for the old server keeps writing trackers and `mark_events_synced` on its own connection: a guided file can be marked known and never reach the new server, and if Save lands during the old pass's pull or batch PUT, the next pass reports "remote data lost", whose Wipe Local deletes local data.
- Fix: keep in `sync_state` which account the trackers belong to. At the start of each pass, if it differs, wipe the trackers and `flag_all_events_unsynced()`. An empty marker adopts the current account without a reset (otherwise every install re-uploads once). `set_nextcloud_account` then only writes URL and username. Fixing only the URL spelling re-uploads the log once; peers drop the duplicates.

### #95 Uploads fail for a server URL with a non-ASCII character
- Where: `meditate-core/src/sync/webdav.rs:197-200` (`url()` is a plain `format!`), 338 (`Destination` header).
- Problem: ureq 2.12.1 rejects header bytes outside printable ASCII. With `…/files/janek/Übungen/` or an umlaut domain, Test, PROPFIND and GET work (the request line is encoded), but every MOVE fails as a "network error", so events stay pending forever.
- Fix: build the header from the parsed URL (`request_url()?.as_url().as_str()`, map the error to Network), or normalise `base_url` once in `HttpWebDav::new`. One mockito test with "Ü" in the path.

### #96 Test Connection sends the app password over plain http
- Where: `meditate-core/src/sync/credentials.rs:206-234`; gtk `preferences.rs:278-283`; android `ui.rs:9043-9052`.
- Problem: Save refuses `http://` but Test allows it on purpose. Typing `http://…` and the password, then Test, sends the password in clear text (even if the server then redirects to https); only Save complains.
- Fix: the same https check in `prepare_test`; the shells' `unreachable!` arms become the existing "URL must start with https://" toast. Update the `SyncSettingsError` docs.

### #97 GTK: one failed keyring open hides the stored password
- Where: `meditate-gtk/src/keychain.rs:208-232` (Err arm 218-231, master-file shortcut 212-216), 340-345, 357-373.
- Problem: if `oo7::Keyring::new()` fails once (oo7 0.4.3 falls back to D-Bus only on PortalNotFound, so a cancelled unlock prompt is enough), the code falls back to its own file store, and even a plain read creates its master file. From then on every launch reads that empty store: PasswordMissing until the password is typed again, and with #42 nothing says so.
- Fix: only `store_password` may fall back and create the master file; reads and deletes return the error.

### #44 Retry-After is not capped, so a sync can hang for hours
- Where: `meditate-core/src/sync/backoff.rs:72-74`, used by `put_with_rate_limit_retry` in the orchestrator.
- Problem: only the fallback wait is capped at 30 s. `Retry-After: 3600` sleeps the worker 1 h per retry, up to 8 times; `in_flight` stays true and every other sync is skipped as AlreadyRunning.
- Fix: return `RateLimited` right away when Retry-After exceeds the cap.

### #83 Two small sync gaps: a batch compacted mid-pull, and leftover `.tmp` files
- Where: `meditate-core/src/sync/orchestrator.rs:313` and `1027-1046, 432`.
- Problem: a peer compacts batch X between listing and GET, the GET returns `NotFound`, and the whole sync fails (push skipped). A PUT that completes server-side but times out client-side leaves a full `<lamport>-<uuid>.json.tmp` forever (fresh batch uuid each push, so it is never overwritten).
- Fix: `Err(WebDavError::NotFound) => continue` without marking the file known; `let _ = webdav.delete(&tmp_path);` when the PUT fails.

## G. Labels, names and Undo

### #74 GTK: deleting the selected label on the Done page makes Save fail
- Where: `meditate-gtk/src/timer/imp.rs:2395, 2412-2419`; core `db/sessions.rs:449`.
- Problem: `done_selected_label_id` keeps a deleted id (deleted in the chooser or by sync). `label_uuid_by_id` errors, Save shows "storage error", the session disappears and comes back from the recovery snapshot up to 60 s short, without note or label.
- Fix: load the label list (already loaded at 2428) before building `data` and drop an id that is no longer in it.
- Same bug in the Log edit dialog: `log/imp.rs:1099-1120, 1158-1162` keeps `selected_label_id` after the label is deleted in its own chooser, and every Save fails until the label is switched off. Same fix at Save.

### #75 Android: label Merge skips the cleanup Delete does
- Where: `meditate-android/src/ui.rs:7784-7810` (compare Delete at 7743-7770).
- Problem: only `refresh_label_state` runs. With the merged label selected on Edit Session or Done, Save fails with "storage error" (foreign key); a Log filter on it shows "No matching sessions".
- Fix: `refresh_after_label_change(&ui, mode); refresh_filter_label_items(&ui);` before `reset_log_feed`.

### #86 A label merge leaves the per-mode default label on the deleted label
- Where: `meditate-core/src/db/labels.rs:238-252` (`merge_labels`).
- Problem: `default_label_uuid_*` settings are not rewritten; if a mode's default was the merged-away "(conflict)" label, Setup shows "(none, pick one)" and new sessions save unlabelled.
- Fix: in `merge_labels`, repoint any per-mode default from the deleted uuid to the kept one. One test.

### #101 Same-name items get the "(conflict)" suffix on different rows on each device
- Where: `meditate-core/src/db/events.rs:711-735` (HashSet replay order); `labels.rs:297-326`, `presets.rs:379-398`, `guided_files.rs:329-348`, `vibration_patterns.rs:348-367`; Android merge `ui.rs:7794`.
- Problem: on a name clash, recompute suffixes the arriving row, so each device suffixes the other device's row. Laptop and phone both create "Walking" before syncing, and each shows the other's as "(conflict-…)". Android's Merge then deletes the laptop's label, and the laptop shows those sessions as "Walking (conflict-…)" for good; if both devices merge opposite ways, both labels are deleted and every such session loses its label. Inside one pull the result also depends on HashSet order, so a rename plus a new row with the old name can end up suffixed on one device only. `label_name_collision_is_idempotent_on_replay` replays in the same order on both sides and can't catch this.
- Fix: one shared helper for the four kinds: on a clash the row with the larger uuid takes the suffix, renaming the current holder with a plain SQL UPDATE inside the recompute (no event). Rerun only the rows that got a suffix at the end of the replay. Fix both uuids in `make_conflict`, or the conflict tests flake. Rows already suffixed heal only with a cache-version bump. Test: each device inserts its own "Walking" before syncing.

### #103 Pressing Rename without changing the name overwrites another device's rename
- Where: `meditate-core/src/db/labels.rs:128-149`, `presets.rs:239-268`, `bell_sounds.rs:145-178`, `guided_files.rs:198-235`.
- Problem: these always write and emit an event. The rename dialogs prefill the name, so pressing Enter sends the old name with a fresh Lamport time. The phone renamed "Yoga" to "Zen" (not synced yet), the laptop presses Enter on "Yoga", and after sync both show "Yoga". `set_setting`, `rename_vibration_pattern` and GTK's guided rename already skip an unchanged value; Android's guided rename does not.
- Fix: return `Ok(())` in all four when the stored name equals the new one exactly (case-only renames still save). One test each.

### #104 Undo after a delete puts the item at the end of its list
- Where: `presets.rs:197`, `guided_files.rs:160`, `vibration_patterns.rs:140` (a new `created_iso`); lists ordered by `created_iso` or `id`. Undo: gtk `presets.rs:601-612`, `guided.rs:1101-1117`, `vibrations.rs:519-528`; android `ui.rs:8030-8061`.
- Problem: chips "Morning, Evening, Walk": delete Morning, tap Undo, and they read "Evening, Walk, Morning".
- Fix: an insert variant that takes the deleted row's `created_iso`, used only by Undo (Android's PresetDelete and GuidedDelete must carry it). Sort patterns by `created_iso` too (see #102).

### #65 A label deleted on one device and renamed on another comes back without its sessions
- Where: `meditate-core/src/db/labels.rs:106-118, 280-330`, `ON DELETE SET NULL` on `sessions.label_id`.
- Problem: laptop deletes L, phone renames L concurrently, the rename wins; on the laptop L returns with a new rowid but its sessions stay unlabelled. Editing one of them there then sends `label_uuid: null` to every peer.
- Fix: when `recompute_label` inserts a new row, recompute the sessions whose events name that label uuid. Or accept the gap: it needs a concurrent delete and rename.
- Third audit: probably already handled. `labels.rs:328-354` relinks the sessions when the rename re-creates the label (a SQLite probe of this case relinked both). Only a session edited on the laptop while the label was gone stays unlabelled, on every device, which matches what the user did. Add a delete-then-rename test; if it passes, remove #65.

### #122 GTK: renaming or deleting a label leaves Log and Stats stale
- Where: `meditate-gtk/src/labels.rs:277, 344, 402`; `guided.rs:976-985, 1001-1007`.
- Problem: only a sync marks Log and Stats dirty. Without Nextcloud, deleting "Yoga" leaves its chips, its filter entry and its Stats row, and editing such a card fails with "storage error". Renaming a label or a guided file leaves the old name.
- Fix: `app.invalidate(InvalidateScope::ALL)` in those label handlers and in the guided rename and its Undo (create only needs LOG).

### #158 GTK: no way to merge a "(conflict)" label
- Where: `meditate-gtk/src` has no caller of core's `list_label_conflicts` / `merge_labels`; Android has the dialog (`ui.rs` `check_label_conflicts`, `on_conflict_merge_tap`).
- Problem: two devices that create the same label name before syncing end with "Work" and "Work (conflict-…)". The phone asks to merge them; the laptop shows both labels for good, and its sessions stay split between them. (Found while planning batch D; a restored backup no longer causes this.)
- Fix: the GTK twin of Android's conflict dialog, on the same core calls.

## H. Timer engine

### #39 Box Breath saves the elapsed time, not the target
- Where: `meditate-core/src/session/mod.rs:576-583`; GTK ticks Box Breath only from a frame callback (`window/imp.rs:448`).
- Problem: a hidden window, blank screen or suspend gets no frames. 5 min session, screen blanks at 4 min, back at 40 min: a 40 min session is saved.
- Fix: `let duration_secs = u64::from(target_secs);` plus a late-tick test.

### #45 With prep, the Running clock starts at the late tick
- Where: `meditate-core/src/session/mod.rs:459` (`Stopwatch::started_at(now)`).
- Problem: 30 s prep, suspend at 5 s, wake 20 min later: a 10 min countdown starts again from 0. Without prep the same suspend correctly ends in Overtime. Normal ticks also lose up to 1 s.
- Fix: `Stopwatch::started_at(now.saturating_sub(elapsed - target))`.

### #46 Interval bells ring in a burst after a time jump
- Where: `meditate-core/src/bells.rs:211-214`.
- Problem: the next due time steps from the previous one and only one bell rings per tick. 5 min bell, suspend 4:59 to 25:00: rings 5 times in 5 s.
- Fix: advance in a loop until the next due time is past `elapsed_secs`, ring once.

### #47 An interval bell due exactly at the end rings 1 s after the end bell
- Where: `meditate-core/src/session/mod.rs:500-504`.
- Problem: the tick that crosses into Overtime returns before checking due bells; the bell fires on the next tick. 10 min timer with "every 5 min": end bell at 10:00, interval bell at 10:01.
- Fix: run `fire_due_bells` in the transition branch and drop its effects.

### #72 Box Breath ends with a "Breathe in" cue on top of the end bell
- Where: `meditate-core/src/session/mod.rs:557-589`.
- Problem: the target lands on a cycle boundary, so the ending tick also enters the next `In` phase: 4-4-4-4 at 320 s gives `FireBoxBreathCue{In}`, `EndBoxBreath`, `FireEndBell`. The test `box_breath_with_target_emits_end_at_target_boundary` (1259) jumps 0.5 s to 16 s and misses it.
- Fix: check the end before phase-change detection; add a test that ticks every second through the end.

### #113 Box Breath gives no cue for the first inhale
- Where: `meditate-core/src/session/mod.rs:242-250, 554-561`; test 1187-1199.
- Problem: the first tick records the phase silently because "the starting bell already fired", but Box Breath has no starting bell. With cues on, the first signal is Hold at 4 s.
- Fix: in `Session::start`, add `box_breath_cue_effect(&settings, In)` to the start effects; update the test and the comments.

## I. GTK timer and bells

### #73 GTK: the Box Breath Duration row shows the Timer duration
- Where: `meditate-gtk/src/timer/imp.rs:3185-3187` (`refresh_streak`), called on return from Stats (`window/imp.rs:143`) and from `apply_config` (3510).
- Problem: it always writes `countdown_target_secs`. Box Breath 5 min, Timer 10 min: the row says 0:10, Start runs 5 min.
- Fix: call `refresh_duration_value_label()` there; drop the redundant write in `load_breathing_settings` (4246-4247).

### #126 GTK: Esc in Overtime pauses, then "Pause" ends the session
- Where: `meditate-gtk/src/window/imp.rs:547-556`; `timer/imp.rs:2252-2285, 2814, 2880-2905`.
- Problem: Esc calls `on_pause` in any state. In Overtime it pauses: Add does nothing and Stop is hidden. After Resume the button reads "Pause" but finishes the session, saving the target and dropping the overtime.
- Fix: `if !matches!(self.ui_state(), UiState::Running | UiState::Preparing) { return; }` at the top of `on_pause` (a check in core alone would still relabel the button).

### #127 GTK: a sound-only bell cuts a running vibration
- Where: `meditate-gtk/src/timer/imp.rs:3982-3998, 3903-3914`; `vibration.rs:214-242`.
- Problem: `install_vibration_handle(None)` drops the old handle, and its Drop cancels the vibration. Two bells at 10:00, one Vibration and one Sound: on the Librem 5 the vibration is cut or never starts. Android only cancels when a new vibration starts.
- Fix: `if handle.is_some() { self.install_vibration_handle(handle); }`.

### #77 GTK: a replaced vibration keeps sending its later chunks
- Where: `meditate-gtk/src/vibration.rs:206-208` (`disarm`); callers `timer/imp.rs:3910`, `vibrations.rs:319`, `vibration_editor.rs:582`.
- Problem: `disarm` only skips the drop-cancel and never sets `cancel`, so chunk 1+ timeouts of a long pattern still fire and replace the new pattern. Not tried on the Librem 5.
- Fix: `self.cancel.store(true, Ordering::Relaxed)` in `disarm`.

### #130 GTK: the laptop can suspend during a session
- Where: `meditate-gtk/src/timer/imp.rs:1139-1156`.
- Problem: the only inhibit is IDLE, and only with Keep Screen Awake on (off by default). With GNOME's default 15 min suspend, a 20 min session suspends at 15 min, and the bells wait until wake (then #46's burst).
- Fix: always inhibit SUSPEND while a session runs (perhaps not while paused, see #63), and add IDLE only with the setting.

### #53 GTK: a volume change is lost if the bell page closes within 400 ms
- Where: `meditate-gtk/src/volume_row.rs:107-130`.
- Problem: the settle timeout uses `#[weak] stop`; once the page is popped the whole body, including `save(volume)`, is skipped.
- Fix: save first, upgrade `stop` only for the preview part.
- Fix together with #132 (same 400 ms timer).

### #132 GTK: an End Bell volume change can land in the wrong mode
- Where: `meditate-gtk/src/timer/imp.rs:1197-1233`; `volume_row.rs:90-153`.
- Problem: the 400 ms settled save reads the mode when it fires. Drag Timer's End Bell volume to 20% and switch to Guided within 0.4 s: Guided gets 20% and Timer keeps its old level.
- Fix: capture the mode's key when the slider moves. Fix together with #53 (same timer).

## J. GTK Log

### #117 GTK: Stats and Log never notice a new day
- Where: `meditate-gtk/src/stats/mod.rs:16-25`, `log/mod.rs:16-38`; "today" is read at reload (`stats/imp.rs:153-270`) and the Log headers are fixed when built (`log/imp.rs:614-635`).
- Problem: the app stays open overnight (Librem 5, suspended laptop). The next morning Stats still says "Goal reached · 25m today", the heatmap marks yesterday as today, the Log heads yesterday "Today", and the next save adds a second "Today" section. For sync users #98's rebuild hides it.
- Fix: store the day each view was built for and count a different day as dirty. Not a GLib midnight timeout (the monotonic clock stops in suspend).

### #118 GTK: saving a session while a Log filter is on adds a non-matching card
- Where: `meditate-gtk/src/log/mod.rs:31-38`, `log/imp.rs:259-319`.
- Problem: `prepend_session` ignores the label and notes-only filters and adds 1 to `loaded_count`. Filtered to "Yoga" with 15 of 40 loaded, a saved "Zen" session appears in the list, and Load more skips Yoga #16.
- Fix: when a filter is set, `invalidate(LOG)` and return.

### #119 GTK: the Log label filter shows "All labels" while it still filters
- Where: `meditate-gtk/src/log/imp.rs:322-330`; `window/imp.rs:762-777, 804-825`.
- Problem: entering the Log tab rebuilds the dropdown at "All labels" but keeps the filter, and picking "All labels" again changes nothing. Edit Save, `prepend_session` and the post-sync refresh reload the labels without rebuilding the dropdown, so after renaming "Zen" to "Afternoon", picking "Morning" filters by Afternoon.
- Fix: `refresh_filter_labels` stores the ids it shows and selects the active filter (clearing it if the label is gone, as Android does); the handler maps through those ids.

### #120 GTK: editing a session in the Log resets its start seconds
- Where: `meditate-gtk/src/log/imp.rs:1150-1157`.
- Problem: Save builds the start with 0 seconds, so a note-only edit moves 07:12:45 to 07:12:00 and syncs the change; re-importing an older backup then duplicates the session. Android keeps the seconds.
- Fix: build the start like Android (`meditate_core::time::local_naive_to_unix` with the original second). One test.

### #121 GTK: deleting a session miscounts the day caption
- Where: `meditate-gtk/src/log/imp.rs:754`.
- Problem: cards add rounded minutes but the delete subtracts raw seconds. 10:20 + 5:00 reads "15m"; deleting the 10:20 session leaves "4m" above a "5 min" card.
- Fix: subtract `log_card_total_secs(session.duration_secs)`.

### #123 GTK: a Log rebuild during a pending delete brings the card back
- Where: `meditate-gtk/src/log/imp.rs:127-159, 660-666, 726-785`.
- Problem: delete A, then save another edit (or a sync finishes) within the 5 s Undo: A reappears while the toast still offers Undo. Deleting A again counts it twice ("2 sessions deleted", caption and `loaded_count` off). Nothing is lost.
- Fix: return early in `commit_delete_in_place` unless `cards_by_id.remove(&id)` finds the card, and hide pending ids when building cards.

## K. Failed writes and stale pages

### #52 GTK: database write errors are thrown away
- Where: `presets.rs:292, 317, 368, 583, 607`; `labels.rs:402`; `log/imp.rs:733`; `vibrations.rs:481, 520`.
- Problem: `let _ = db…` or a dropped `with_db_mut` result. "Override preset" during a sync lock fails with BUSY after 1 s, yet the toast says it was overridden. Undo after a preset delete can fail silently on a name clash.
- Fix: `if let Err(e)`, log, toast. One line per site.
- More sites (third audit): `bells.rs:247-249, 327, 419, 777`; `sounds.rs:953`; `vibrations.rs:664` (shows "Deleted X · Undo" for a pattern that was not deleted); `guided.rs:745, 1001, 1110`; `preferences.rs:86`. Worst: `guided.rs:1069` ignores the delete result, yet the toast timeout still removes `<uuid>.ogg`, so a BUSY failure loses the audio while the row stays. Schedule the file removal only on success, as `sounds.rs:987` does.

### #80 Android: failed writes look successful
- Where: `meditate-android/src/ui.rs:5818-5850` (delete), 5770-5776 (rename); Override at 5655-5664 is correct.
- Problem: a BUSY delete still shows "'X' deleted" with Undo while X stays listed; Undo then fails on the same uuid. Rename closes its dialog with no message on failure. Android side of #52.
- Fix: in the `Err` branch, `show_notice(delete_failed)` and return, as Override does.
- More sites (third audit): Override, guided rename, sound rename and sound delete `take()` their uuid (Override also its settings snapshot) before the write and return silently on error, so the dialog does nothing on the next tap (`ui.rs:5643-5664, 5383-5407, 6230-6241, 6270-6283`). Clear them only on success and show `save_failed`. `commit_pending_deletes` (2566-2585) removes every drained Log row even when its delete failed: the row vanishes, comes back on the next reload, and Stats still count it. Remove only the ids that were deleted.

### #78 Android: the Timer streak line does not update after Save
- Where: `meditate-android/src/ui.rs:1550` (`set_streak_text` only in `refresh_stats`), `on_save_tap` 4770-4859, `on_edit_save_tap` 8199-8330.
- Problem: after the first session of the day it still says "No streak" until Stats opens, a sync pulls, or a restart.
- Fix: `refresh_stats(&ui);` after `reset_log_feed` in both handlers.
- Third site: Recovery Undo (`ui.rs:8014-8019`) reloads only the Log; the streak line and Stats keep the removed session. Add `refresh_stats(&ui);` there too.

### #84 Outdated names after renames (both apps)
- Where: `meditate-android/src/ui.rs:4329-4334, 781` (widget); 6228-6297 (sound rename/delete).
- Problem: renaming label "Yoga" to "Zen" leaves the widget subtitle at "Yoga". Renaming or deleting a sound leaves the Interval Bells list showing the old name.
- Fix: `refresh_widget(&ui);` next to `refresh_preset_chips` at 4333 (then the separate calls after preset actions can go); `populate_interval_bells(&ui);` in both sound handlers.
- GTK twin: renaming or deleting a sound leaves the bell edit page and the Interval Bells list with the old name, and a deleted sound never shows "Missing" (`bells.rs:216, 539-541, 696`; `sounds.rs:946-957, 985-993`). Refresh both on the NavigationPage `shown` signal.
- Same for patterns, and for Setup's own rows: deleting a vibration pattern in the chooser and going back leaves Setup's bell pattern row with the deleted name (seen while testing batch C). Setup re-reads its bell rows only on a pick, a mode switch or a preset apply (`timer/imp.rs` `refresh_streak`). Re-read them when a chooser closes.

### #124 A deleted or renamed guided file stays selected; open choosers keep deleted items
- Where: gtk `timer/imp.rs:610-622, 3289-3398`; android `ui.rs:2475-2493, 5273-5285, 6150-6185, 6374-6412`.
- Problem: GTK: delete the selected "Body Scan" in Manage Files, and Start fails with the raw "canonicalize … (os error 2)" toast; a rename keeps the old name. Android: the same after a sync pulls another device's delete or rename (local deletes are handled). An Android bell or pattern chooser left open across a resume still lists a sound deleted elsewhere; picking it saves a dead uuid, and the bell is silent.
- Fix: GTK: in Manage Files' `on_changed`, re-resolve the selected uuid (clear or rename), then `refresh_hero_for_idle()`. Android: re-resolve `guided_sel` in `refresh_after_pull` (not during a session or on Done, because Save reads it), call `refresh_after_pattern_change` and refill an open bell chooser. The test at `app.rs:1573` checks the literal `refresh_bell_rows(`.

### #136 Android: the Box Breath counter shows the Setup target, not the session's
- Where: `meditate-android/src/ui.rs:4089-4096, 4693-4695, 3889, 2747`; test `app.rs:1280`.
- Problem: the counter target is rebuilt from Setup on every Pause and Resume, and a sync pull mid-session can change Setup. A 10 min session then reads "03:10 / 20:00" while it still ends at 10:00.
- Fix: use `session.completion_duration_secs()` (filtered to > 0) in the tick and delete the cached Cell and its plumbing (about -20 lines).

### #140 Android: Back from the interval-bell editor leaves its preview playing
- Where: `meditate-android/src/ui.rs:9274-9277` (Cancel at 7421-7431).
- Problem: drag Volume (the bell plays at alarm level), press Back: the bell rings to its end with no Stop button on screen.
- Fix: `ui.invoke_interval_editor_cancel(); return;` in that branch.

### #141 Android: the bell chooser shows "Stop" when nothing plays
- Where: `meditate-android/src/ui.rs:7140-7155`; pattern preview 7076-7094.
- Problem: a sound whose file hasn't arrived fails to play (`play` returns 0), but the row switches to "Stop" in silence.
- Fix: if the returned duration is 0 or less, stop the preview state and leave the uuid unset.

## L. Android platform (manifest: ask first)

### #50 Android: any installed app can start a meditation
- Where: `meditate-android/kotlin/MeditateWidgetProvider.kt:55-78`; receiver `exported="true"` in the manifest.
- Problem: `WIDGET_LAUNCH` arrives at the exported widget receiver, which writes `widget_launch` without a check. The default preset uuids are public constants. A session starts (now or at next launch) and its end bell rings at alarm volume.
- Fix: a small non-exported receiver for the launch action; point the widget's tap PendingIntent at it. Manifest change: ask first.

### #51 Android: phone-to-phone transfer copies the device id
- Where: `meditate-android/android/app/src/main/AndroidManifest.xml:41-47`.
- Problem: since API 31, `allowBackup="false"` only stops cloud backup, not device-to-device transfer. A new phone gets the old `device_id`; if the old phone keeps syncing, both write events under one id.
- Fix: `android:dataExtractionRules` with a `<device-transfer>` section excluding everything. Manifest change: ask first and run the repro probe.

### #57 Android: guided audio quirks
- Where: `kotlin/MeditateGuided.kt:38`; `src/ui.rs:5332-5334`.
- Problem: `startAudio` ignores a refused audio focus, so a track plays during a call and is never paused. In Manage Files, Play on a not-yet-downloaded file does nothing visible.
- Fix: return false when focus is refused (Rust already shows "Couldn't start playback"); show `invoke_playback_failed` in the preview path.
- Also: nothing listens for `ACTION_AUDIO_BECOMING_NOISY`, so when Bluetooth earbuds disconnect during a guided session the track moves to the loudspeaker (`MeditateGuided.kt:36-69, 146-153`). Register a receiver on applicationContext in `startAudio` that calls `markFocusLoss`, unregister it in `releaseLocked`; the existing focus-loss path pauses the timer and the track.

### #79 Android: the Simplified Chinese translation is never picked
- Where: `meditate-android/kotlin/MeditateAbout.kt:268-270`; `src/ui.rs:3831-3841`.
- Problem: the system tag is usually `zh-Hans-CN`, which becomes `zh_Hans_CN` then `zh`; the bundle is `zh_CN`, so the app stays English. Not checked on a device.
- Fix: return `"${l.language}-${l.country}"` (or just `language` when country is empty).

### #63 Android: a paused session keeps the phone awake
- Where: `kotlin/MeditateSessionService.kt:178-186`, `src/ui.rs:277-313`.
- Problem: pause keeps the partial wake lock, renewed every 30 min, so the 200 ms tick runs all night if a paused session is forgotten.
- Fix: release on pause, re-acquire on resume. Or skip if this never happens in practice.

### #149 Android 8-12: vibration bells are dropped under Battery Saver
- Where: `meditate-android/kotlin/MeditateHaptics.kt:67-84`.
- Problem: below API 33 the code calls `vibrate(effect)` with no usage. In battery saver, Android 8-12 only lets alarm, ringtone and communication vibrations through, so a Vibration end bell is silent. The FP5 (Android 15) is not affected.
- Fix: `vibrator.vibrate(effect, AudioAttributes` with `USAGE_ALARM)`, which exists since API 26; correct the comment that says it needs API 33.

## M. Android layout and editors

### #139 Android: editing a vibration pattern on the FP5 flattens it everywhere
- Where: `meditate-android/src/ui.rs:6721-6748` (`on_pattern_edit`), 6954-6992 (Save).
- Problem: on a phone without amplitude control, Edit loads the points rounded to 0 or 100% and forces Bar; Save writes that and syncs it. Renaming "Wave" (Line 0.3, 0.6, 0.9), or saving with no change, stores Bar [0, 1, 1] on every device.
- Fix: load the stored intensities and chart kind unchanged; only points the user drags still round.

### #142 Android: the vibration editor saves before a typed value reaches the curve
- Where: `meditate-android/ui/main.slint:6433-6439, 2568-2573`; `src/ui.rs:6954-6963`.
- Problem: Slint runs `changed` handlers after the click, so Save reads the old curve. Type 12 Points and Save: it is saved with 7. Type 1.0 s Duration on a 20-point pattern: it is saved as 20 points 50 ms apart, below the 100 ms floor, and synced.
- Fix: after the two `commit()` calls, call `root.ve-duration-changed(...)` and `root.ve-points-changed(...)` directly.

### #143 Android: long row texts push switches off screen
- Where: `meditate-android/ui/main.slint:595-605, 839-848, 910-919, 1019-1029, 5875-5886, 6844-6848`.
- Problem: a Slint Text with neither wrap nor elide can't shrink, so the switch is laid out past the clipped list. At 360 dp the Keep Screen Awake switch is hidden in 7 of 9 languages, the German and Russian "Cues during phases" switch too, and long preset subtitles hide the delete button.
- Fix: `wrap: word-wrap;` (two lines fit) or `overflow: elide;` on those Texts.

### #144 Android: a volume slider change is lost when the page scrolls
- Where: `meditate-android/ui/main.slint:1505-1574`; saved in `on_bell_volume_released` (`ui.rs:8719-8737`).
- Problem: the slider sets the value on press but saves on release. When the page takes over a vertical drag, the slider gets a cancel it ignores, and then shows 30% while the bell rings at the stored 80%.
- Fix: remember the value on press and restore it on cancel (about 3 lines).

### #145 Android: chooser rows leave no room for the name
- Where: `meditate-android/ui/main.slint:6153-6211, 6315-6391`; PreviewPill 1266-1300.
- Problem: at 360 dp the "Edit" text button and the Play pill leave a German custom pattern name 29 px and a Russian one nothing.
- Fix: icon buttons (the edit icon, and the play and pause icons Manage Files already uses), as GTK does; the name then gets about 108 px. Remove the stale comment at 6226-6228.

### #146 Android: the interval-bell and vibration editors can't scroll
- Where: `meditate-android/ui/main.slint:5935-6055, 6407-6696`.
- Problem: they need about 600-650 dp, so in landscape or split screen some rows are cut off and can't be reached.
- Fix: a Flickable around the interval-bell editor body (needs #144); in the vibration editor only above the chart.

### #147 Android: small touch targets
- Where: `meditate-android/ui/main.slint:641, 701, 1277, 6353, 1782, 1799`.
- Problem: the mode switch, the Stats period toggle and the Sound/Vibration/Both segments are 32 dp tall and the Box Breath steppers 36 dp; Material asks for 48.
- Fix: taller controls or taller TouchAreas (the PhaseTile row is already full width at 360 dp, so it can only grow taller).

### #148 Android: the date picker's year grid skips 2030 and starts at 2024
- Where: `meditate-android/material-1.0/ui/components/date_picker.slint:356`.
- Problem: the vendored year list is hard-coded, so an imported 2022 session can't be moved by year.
- Fix: build the list from the current year backwards.

### #160 Android: Slint's text fields misplace the cursor handle and ignore the space-bar slide
- Where: Slint's Android layer, `i-slint-backend-android-activity` 1.16.1 (`java/SlintAndroidJavaHelper.java`, `javahelper.rs`), not our code. Seen on the FP5 in Preferences (batch F test); batch F made Preferences let go of its fields, which removed the stuck keyboard, handle and copy bar there.
- Problem:
  - A cursor at the very start of a field (an empty field) gets its handle at the screen's left edge: `set_imm_data` sends x = -1 when the cursor fails its clip check, and the one-handle path shows the handle there instead of hiding it.
  - The cut/copy/paste bar covers the field's text instead of sitting above it (cause not found yet; the content rect comes from the same cursor coordinates).
  - Sliding over Gboard's space bar doesn't move the cursor: the keyboard moves the selection through the input connection, and Slint's editable only reports text replacements, never a selection-only change.
  - Other pages with a Material `TextField` (rename and name dialogs, Log edit) may keep the handle after closing too; check each close path like Preferences.
- Fix: patch the Java helper (hide the handle at x = -1, report `setSelection` back through `updateText`, then the content rect), which means vendoring the backend like `third_party/i-slint-compiler`: build environment, so ask first, then run the repro probe. Report it upstream either way.

## N. Stats numbers

### #55 Goal ring and heatmap round down, the Log rounds to nearest
- Where: `meditate-core/src/goal.rs:103`, `contrib.rs:91` vs `format.rs:389-397`.
- Problem: a 19:30 session with a 20 min goal: Log says "20 min", ring says "1 min to go". A 40 s session shows "1 min" in the Log but leaves its heatmap cell empty.
- Fix: use `(secs + 30) / 60` in `goal::compute` and `build_grid`.
- Same class: `format.rs:154-169` (`hm_secs_key`, used by the by-label rows, insights and chart axis in both apps) and `hm_compact_key` (126-137) round down too. A label with one 40 s session reads "0m · 1 session", a 59 s maximum puts "0m" at the top of the axis, and the Total tile shows "–". Update the test at 1222-1229. Sessions under 30 s still read "0m" after the change.

### #59 Week-over-week compares part of today with a full day
- Where: `meditate-core/src/date_math.rs:105-117`.
- Problem: Monday 07:00, nothing yet today, 30 min last Monday: the card shows -100% every morning.
- Fix: sum through yesterday on both sides; skip the card when that is 0 days.

### #60 Year chart has two partial months, and both shells build the series
- Where: `meditate-gtk/src/stats/imp.rs:416-444`, `meditate-android/src/ui.rs:1708-1735`.
- Problem: on 2026-10-09 the year view starts 2025-10-10: 13 bars, first and last October both partial and both labelled "O". Both shells zero-fill the daily series in their own code.
- Fix: one core `chart_series(totals, today, period)` starting the year window on the 1st of a month; both shells call it.

### #114 The best streak counts future days
- Where: `meditate-core/src/db/sessions.rs:644-664` (`streak()` filters at 718-721); callers gtk `stats/imp.rs:530` via `db/mod.rs:862`, android `ui.rs:1482`, `insights.rs:65`.
- Problem: a 5-day streak plus a session dated tomorrow shows "6d" and "5 days · best was 6".
- Fix: `best_streak(today)` with `days.retain(|d| *d <= today)`; extend the #18 test (`sessions.rs:3014`).

### #115 The Stats "Streak" tile shows the best streak, the Timer the current one
- Where: gtk `stats/imp.rs:526-539`, `stats_view.blp:225-236`; android `ui.rs:1474-1492`, `main.slint:4873`.
- Problem: an old 10-day run, a gap, a session today: the Timer says "1 day streak", Stats says "Streak 10d". The insight on the same page already says "best was N".
- Fix (decide first): show the current streak (about 3 lines, no strings), or caption the tile "Best streak" (new msgid, nine translations, both apps).

## O. Dates in other languages

### #151 GTK: dates in English word order in every language
- Where: `meditate-gtk/src/log/imp.rs:628-631, 1232-1237`; `stats/imp.rs:241, 358, 745-754`.
- Problem: the patterns are fixed ("%b %-d", "%b %-d, %Y", "%b %d, %Y", "%A, %B %e"). German Log headers read "Okt 9, 2025", Chinese "10月 9, 2025", and the Russian heatmap's screen-reader name "Четверг, октября  9". Android uses locale skeletons.
- Fix: make the patterns translatable msgids with a translator comment (nine translations each) and build the chart label from them.

### #152 Android: Log dates use US month/day for English and untranslated languages
- Where: `meditate-android/ui/main.slint:156-157`; `src/ui.rs:2405-2422`; test `app.rs:2212`.
- Problem: day headers use `@tr("{1}/{0}")`, so en-GB (and Swedish, which falls back to English) shows 9 October as "10/9".
- Fix: `locale_date(d, "MMMd")` / `"yMMMd"`; delete the two Tr entries and their po entries.

### #153 Chinese charts label October to December "1"
- Where: gtk `stats/imp.rs:759`; android `ui.rs:1697`.
- Problem: the month letter is the first character of the month name, and Chinese month names start with digits ("10月", "11月", "12月").
- Fix: GTK: take the leading digits when the name starts with one. Android: drop `take(1)` (narrow month names are already short).

### #154 GTK: Portuguese weekday labels repeat
- Where: `meditate-gtk/src/stats/imp.rs:764-775`.
- Problem: cutting `%a` to 2 characters gives "se te qu qu se sá do".
- Fix: drop `.chars().take(2)`, as Android does.

### #116 The longest-session date has no year
- Where: gtk `stats/imp.rs:356-367` ("%b %-d"); android `ui.rs:1925-1929` ("MMMd").
- Problem: the insight is the lifetime longest session, so "1h 30m on Mar 3" can be from 2024.
- Fix: use the Log header rule: with the year when `date_group_kind` is `EarlierYearOther` ("%b %-d, %Y" / "yMMMd"). Do it together with #151.

## P. GTK translations

### #56 GTK: the completion notification is English and copied twice
- Where: `meditate-gtk/src/timer/imp.rs:2797-2809` and `2972-2980`; `preferences.rs:636` (`"CSV files"` filter).
- Problem: "Meditation Complete" and "Session: …" have no gettext; the same block is in `transition_running_to_overtime` and `finish_breath_session`. The import filter name is unwrapped while export wraps it.
- Fix: one `send_done_notification` helper with gettext; wrap the filter name; all nine translations.

### #81 GTK: the running page and the vibration editor show English
- Where: `meditate-gtk/src/window/imp.rs:192, 194, 197, 199, 215, 253, 502`; `vibration_editor.rs:199, 423`.
- Problem: "Pause", "Stop", their tooltips, the overtime tooltip, "Meditating" and "Box Breathing" have no gettext; in German one page says "Stopp", the other "Stop". The editor shows "Mind. 100 ms zwischen Punkten (up to 24 for this duration)".
- Fix: wrap them (Pause, Stop, Pause Timer, Stop and Save Session, Box Breathing already exist). New msgids for "Meditating", the overtime tooltip and one full sentence "Min 100 ms between points (up to {max} for this duration)", with all nine translations.

### #64 GTK: 14 user-visible strings use the long dash
- Where: e.g. `timer/imp.rs` and `log/imp.rs:1189` ("Couldn't save session — storage error"), `window/imp.rs:677`, `stats/imp.rs:181-304`, `guided.rs:733/735`, `preferences.rs:68`, `bells.rs:484`, sync error texts at `sync_runner.rs:66-69` ("—" and "→").
- Fix: rephrase with comma or colon, update msgids and the .po files.
- Also in the translations alone: `ru.po:1080, 1085, 1090, 1108` ("{pct} % сессий — утром", "{duration} — {date}"), the Russian metainfo description and 4 Russian release notes, and 1 Polish release note. Rephrase those msgstrs with a comma or colon.

### #155 Unit suffixes skip translation
- Where: gtk `timer/imp.rs:4229` ("{val}s"), `vibration_editor.rs:767-769` ("{secs:.1}s"), `stats/imp.rs:538` and android `ui.rs:1490` ("{streak}d").
- Problem: Russian GTK shows "4s" where Android shows "4 с", and every language shows "5d".
- Fix: GTK reuses Android's "{}s" msgid (its translations exist); add a plural pair for "{n}d" in both apps. Decimal commas are a separate matter.

### #156 GTK: the By-label session count builds its plural by hand
- Where: `meditate-gtk/src/stats/imp.rs:566-575`.
- Problem: it picks between "1 session" and "{count} sessions"; Russian and Polish need three forms (the translators worked around it).
- Fix: reuse `ngettext("1 session", "{n} sessions", n)` as `section_caption_text` does; delete the two msgids and fix the comment that names `pluralize_sessions`.

## Q. Sync efficiency (after G and J)

### #92 Every pulled event is uploaded again
- Where: `meditate-core/src/db/events.rs:270-282` (the INSERT leaves `synced` at its default 0, `schema.rs:245`); push `orchestrator.rs:420`.
- Problem: a device pushes back everything it pulled. A new phone joining a remote with 4,000 events uploads them all again; after Wipe Local a device pushes the whole remote log back. The batch count doubles, so compaction runs twice as often. No case needs this forwarding: compaction serialises all events, and Push local and #89 flag the rows.
- Fix: set `synced = 1` on the replay path only (`append_event_returning_newness` is shared with `emit_event`). Reword the comment in `tests/sync_roundtrip.rs:857`.

### #98 GTK rebuilds every view after every sync
- Where: `meditate-gtk/src/application.rs:617-648`.
- Problem: every write starts a sync, and every sync re-runs all Stats queries and resets the Log to page 1, even when nothing came in. Deleting a session deep in the Log jumps the list back a second after the delete commits.
- Fix: in `drain`, `changed |= r.as_ref().map_or(true, |s| s.brought_changes())`, and refresh only when true (as Android does, `ui.rs:203`). Do this after #118, #121 and #122: today this rebuild hides those bugs for sync users.

### #99 Going back to an older build leaves a cache marker that blocks a later replay
- Where: `meditate-core/src/db/events.rs:198-205`.
- Problem: `maybe_walk_events_for_cache_upgrade` returns early when the stored version is higher, so an older build leaves the newer marker. Events of a new kind that the older build recorded are never applied when the newer build comes back. Linux only (Android refuses a versionCode downgrade).
- Fix: when `stored > CACHE_SCHEMA_VERSION`, write `CACHE_SCHEMA_VERSION` and return. One test next to `re_open_at_current_cache_version_does_not_re_walk`. Must ship before the next `CACHE_SCHEMA_VERSION` bump.

### #100 Every sync attempt runs the full database open with an integrity check
- Where: `meditate-core/src/db/mod.rs:253-317` (quick_check at 296-305); gtk `sync_runner.rs:109`; android `sync_runner.rs:102`.
- Problem: each attempt runs the schema setup, a `user_version` write (which takes the write lock before the sync busy timeout is set) and `PRAGMA quick_check`, which reads every page. A failing quick_check counts as "ok" and is never logged.
- Fix: run quick_check once from the startup opens (gtk `application.rs:136`, android `ui.rs:9673`) and log its error. Update the `open()` docs (mod.rs:160, 186).

### #137 Android: a sync that fails before reaching the server reloads every screen
- Where: `meditate-android/src/ui.rs:197-203, 2475-2493`; `sync_runner.rs:107-116`; test `app.rs:1562`.
- Problem: every error counts as "maybe changed". With the password missing (#70), each save triggers a sync that fails at once, and the next tick reloads the Log to its first 15 rows and re-runs Stats.
- Fix: `Err(e) => !matches!(e, Unconfigured | PasswordMissing | OpenDb(_))`.

## R. Tests

### #91 Sync tests that pass for the wrong reason
- Push local: `prepare_push_local_recovery_flags_all_events_unsynced` (`sync/settings.rs:290-301`) never marks events synced first, and the orchestrator test's "mark events un-synced" loop (`orchestrator.rs:1687`) deletes from an empty listing. Dropping `flag_all_events_unsynced` keeps both green. Mark events synced before the recovery; in the orchestrator test call `prepare_push_local_recovery` and assert that a fresh peer gets the old session.
- Pull size cap: `pull_drops_files_over_10mb_cap` (`orchestrator.rs:2233-2259`) never reaches the pull cap, because push refuses the file first. Put the 10 MB + 1 byte body straight on the fake server. The `ResponseTooLarge` branch needs a body over 11 MB.
- Folders: `FakeWebDav` (`fake.rs:47-84`) creates parents implicitly, accepts MKCOL on an existing folder and lists a missing folder as empty. Removing the `Conflict` tolerance in `ensure_*_dir_exists` (`orchestrator.rs:649-678, 863-868`) stays green, while Nextcloud would answer 405 or 409. Add a strict wrapper in the tests and a two-round sync test with a custom sound and a guided file.

### #138 Android: ui.rs logic has no behaviour tests
- Where: `lib.rs` (`mod ui` builds only for Android); `ui.rs:2092, 2231, 2337`; source-text tests in `app.rs` (e.g. 1562, 2516-2527).
- Problem: `group_log_sessions`, `truncate_note_for_card` and `rounded_perimeter_point` never run in a test. Changing `last.count += 1` to `+= 2` passes every test, while renaming a local variable fails one.
- Fix: move these pure helpers (and `LogDaySectionData`) into app.rs and test them with real inputs, passing the clock format in. Drop the matching source-text assertions. This is not the rejected ui.rs split.

### #150 No test ties most JNI signatures to their Kotlin methods
- Where: the bridges in `meditate-android/src/` (haptics, screen, insets, keychain, widget, about, service); existing pins at `app.rs:1752, 1880`.
- Problem: renaming or retyping one of those Kotlin methods turns the call into a logged NoSuchMethodError: bells, vibration, the keychain or the service stop silently, and `cargo test` stays green.
- Fix: one table test that checks each Rust signature string against the matching full `fun` line in the Kotlin file.

## S. Cleanup

### #58 Dead and duplicated code (core and Android)
- Core: `SessionPhase::Paused` (`session/mod.rs:121`) is never created; delete it and its 3 match arms. `daily_totals()` / `get_daily_totals_from_db` (`db/sessions.rs:172, 671-697`) reads the whole table for the 91-day heatmaps; call the `_since` version and delete it with its tests.
- Android: about 60 `let _ = weak.clone();` and 5 `let _ = current_mode.get();` (e.g. `ui.rs:4751`); `AppState::primary_label`, `remaining`, `is_idle` (`app.rs:392, 594, 616`) used only by their own tests; Kotlin `MeditateKeychain.clearPassword` has no caller; `service.rs::call` (87-104) repeats `jni_call::load_class`.
- More (third audit): gtk `keychain::delete_password` (141-156) with `Backend::delete` (306-319) and core `settings::clear_nextcloud_account` (`sync/settings.rs:77-85`) have no callers (also fix the comment at android `keychain.rs:63-67`). Android no-op lines `ui.rs:5994, 6067, 6077, 6089, 7201, 7245, 8744` and the stale `chooser_bell_volume` doc (9621-9623). `WidgetPreset::mode` (`widget.rs:45-50, 63-67, 189-199`) is written but never read: delete it and `mode_tag_is_carried`. `invoke_no_arg`, `invoke_player_noarg`, `invoke_refresh` and `service.rs::call` are one static `(Landroid/content/Context;)V` call four times: one `jni_call::call_context_void`.

### #61 GTK dead code and the duplicated transcode pipeline
- Dead: `populating` Cell never set (`bells.rs:604`, guards at 630, 647, 661, 740); unused `toast_slot` (`presets.rs:336/360`); unused `chart_h` (`stats/imp.rs:465/496`); unused `area` (`stats/imp.rs:697/707`); `let _ = BellSoundCategory::General` (`db/mod.rs:372`); half-deleted "Throwaway" comment (`timer/imp.rs:1170-1175`).
- Duplicate: `sounds.rs:735-849` and `guided.rs:433-534` are the same gst pipeline except `audioloudnorm` and the cancel-aware bus loop; one function with `loudnorm: bool` and `cancel: Option<&AtomicBool>`, about -80 lines.
- Update (third audit): `audioloudnorm` never runs. It is not in the GNOME 50 or 51 runtime and the manifest adds no GStreamer module (it comes from gst-plugins-rs, not plugins-bad). Delete that branch (`sounds.rs:756-776, 788-794`) and fix the comment at `guided.rs:426`; the merged pipeline then needs no `loudnorm` flag.
- More dead code in `timer/imp.rs`: the `starting_bell_sound` read with its `"bowl"` fallback, bound to `_starting_bell_sound_legacy` (3081-3083, 3101, 3110, 3137); `apply_preferred_label_for_mode(_mode)` (4267) ignores its argument; `build_breathing_setup` (4076) and `breath_elapsed` (3011) only pass through; 2397-2401 and 2605-2609 hand-copy `From<TimerMode>` (use `mode.into()`); `PKGDATADIR` (`config.rs:6-7`) has no reader and hides behind `#[allow(dead_code)]`. Remove `tick_mode` (194) only after #129.

### #85 Dead code, duplicates and a test that cannot fail
- Core: `Effect::EndPrep` is never consumed (effect.rs:21-25, mod.rs:460, asserts 1065/1077/1636, ARCHITECTURE.md); `tick_box_breath` calls `fire_due_bells` though Box Breath never has interval bells (mod.rs:597-604, test 1475); `db_open_failure_key` (format.rs:295-315) is unused and repeated in gtk `application.rs:168-177`; `Stopwatch` serde derives and tests (timer.rs:1-12, 86-113) are unused and assert a false restart claim; `box_breath_phases.rs:17-47` repeats `get_box_breath_phase` and hard-codes the seed uuids at 141-144; `list_bell_sounds_for_category` (bell_sounds.rs:236-264) copies `list_bell_sounds`; `count_labels_from_db` (labels.rs:68) is test-only.
- GTK: `preload_end_bell` builds a media file that is never played, `play_starting_sound` has no caller (sound.rs:250-301, calls at timer/imp.rs:688, 735, 1209-1212, window/imp.rs:111-118).
- Android: label rename/delete handlers repeat `refresh_after_label_change` (ui.rs:7700-7706, 7752-7764); `ui.rs:537-545` repeats core `bells::signal_mode_override_from_db`.
- Test: `bell_rng_state_advances_deterministically_from_seed` (mod.rs:1488-1520) uses `jitter_pct: 0`, so the RNG is never used; set 20 and assert a different seed differs, or delete it.
- Android (third audit): `validate_label_name` (2186) is `validate_rename_label_name(n, 0)` (2817); `bell_name_taken` (1175) is `bell_sound_name_taken(n, "")` (3715); the body of `read_global_setting` (3189-3193) is core `read_str` and its four `== "true"` callers are `read_bool`; the label-name map is built three times (609, 716, 763); 4007-4011 repeats `push_session_length_to_ui`; 2453-2458 repeats `refresh_setup_label_name`; `snapshot_setup_json` (1347-1371) rebuilds the timing that core `active_presets` builds (needs a small core function). About -30 lines.

### #87 Trimming the diagnostics log drops its 0600 permission
- Where: `meditate-core/src/diag.rs:165-185` vs `:77-87`.
- Problem: `trim_to_tail` writes the copy with `File::create` (0644) and renames it over the 0600 log. No secrets are logged today, and Flatpak/Android keep it private anyway.
- Fix: create the temp copy with `OpenOptions` and `.mode(0o600)`, as `init` does.

### #157 Wrong or misplaced docs and comments
- EVENTS.md:78, 89, 235: the batch file name is `<min_lamport:014>__<batch_uuid>.json`, the bell categories are general and box_breath, and there are 7 entity families plus 2 singletons. The same stale file name is at `orchestrator.rs:374`.
- gtk `db/mod.rs:113-117` says new bundled sounds reach existing installs, but the seed is one-shot (`seeds.rs:27-29`); the block also sits on the wrong item.
- android `ui.rs`: doc comments on the wrong item at 488-492, 803-822, 1128-1132, 2348-2350 (shows a start time with a UTC offset, which never exists), 2904-2909 and 9477-9481; false claims at 1406, 1444, 3874, 4028, 5878 and 5909.
- `sync/fake.rs:1-4` claims the fake behaves like Nextcloud (see #91).

## T. Text that doesn't fit

### #159 Text cut off or shortened with "…", found one screenshot at a time
- Rule (Janek, 2026-10-10): every text shows in full at phone width (360 px) in all nine languages. Cut off is not acceptable, and neither is "…": both hide information.
- Seen so far: Android's guided-import dialog title ("Geführte Meditation importi", `main.slint` guided-import card, a 22 px Text with no wrap); GTK's import toast (fixed in batch E with a shorter text); Android's import-question buttons (stacked in batch D); GTK's Type rows squeezing their choices (pinned in batch C).
- Android: Slint clips a Text that neither wraps nor elides, silently. 142 Text elements, 31 wrap, 10 elide. Check 1, a test: every Text showing translated words wraps (`wrap: word-wrap`) and has room to grow; `overflow: elide` is not allowed for them either. An explicit short list is exempt (numbers, the big timer, single symbols).
- Both apps, check 2, a test: slots that can't wrap (buttons, toasts, toggle and tab labels, dialog buttons, header titles, GTK row titles that ellipsize) have every translation measured with the real font at its real size against the slot's width at 360 px, and the test lists each one that doesn't fit. The work is the list of fixed slots and their widths; measuring needs nothing new (Pango in the GTK tests, the same font files for Android).
- Fix what the first run lists: wrap, stack buttons, give the slot room, or shorten the translation; never "…".

---

## Deferred refactors

Only together with other work in the same area; none has a bug behind it.

- **R12, one blob routine for sounds and guided files** (the runner half moved to core in batch F): about -90 in the orchestrator; the remote paths must stay byte-identical.

## Considered and rejected

So these aren't proposed again:
- Typed serde structs for every sync event payload: the silent defaults are deliberate compatibility tolerance, and there is wire risk.
- A change-counter-driven view refresh: `LocalChanges` is per connection, so sync pulls never bump it.
- A run token on every drop file: only imports needed one (done).
- Number fields that commit on every keystroke: intermediate values go stale or clamp mid-typing.
- Removing the picker's `transient.<ext>` copies: one per extension, overwritten by the next pick, and the guided play-now selection plays the copy in place, so cleanup needs three separate paths for a few MB.
- R13, small duplicates (`hm_mins_key` into `hm_secs_key`, one `session_payload()`): too little gain for touching the sync payload.
- R7(b), a session record instead of loose session state: about -30 lines for about 54 rewritten test chains.
- R15(c), one `SlidePage` frame for 13 pages: every page reaches into its own Flickable, so each needs rework and a retest, for a repeated frame and no bug.
- R15(d), merging `VerticalSpinBox` and `StepperRow`: about -40 lines in the focus and keyboard code the cursor bugs came from.
- R2b, all database writes off the UI thread: about 175 call sites, and no lock wait noticed in practice.
- Closing the file picker before its copy ends (#37): the read grant on the picked file can end with that screen, so every pick path would need the file opened first; a local copy takes about a second.
- Typed `BellSlot` through Slint: large, with no host safety net.
- Moving the whole sync account Save/Test into core: the keychains are per app.
- Moving the Log edit build into core: low value; reconsider when fixing #5/#6.
- Splitting `ui.rs` per screen or the big core files: cosmetic.
- A `Divider` component, a page stack model, caching JNI classes, moving the CAN_DUCK policy to Rust, a typed `SettingKey` enum, a core pending-Undo queue.
- Left out of the bug audit on purpose: configChanges / activity recreate and the import-labels transaction (both in TODO.md); Back on the Done screen discarding the session (decided behaviour).
