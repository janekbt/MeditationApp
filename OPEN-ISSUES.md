# Open issues (2026-10-09)

One list for everything still open: the Android bugs (formerly ANDROID-BUGS.md) and the refactors left from the audit (formerly REFACTOR-AUDIT.md). Done items are removed; the git history has both old files (`git show 6e81927:ANDROID-BUGS.md`, `git show 6e81927:REFACTOR-AUDIT.md`). Bug numbers stay as they were, because commit messages refer to them.

**Ground rules:**
- Decisions go in core.
- No DB schema or sync wire-format change without a migration, and nothing touches the Android build environment (Cargo.toml, Cargo.lock, build.rs, main.rs, gradle, the manifest, rust-build.sh) without asking.
- msgids stay unless the item says otherwise; new ones get all nine translations.
- TDD, then a phone check before the commit.
- Before the next release: run `build-aux/repro-probe.sh`.

## Batches, in suggested order

From two fresh-eyes audits on 2026-10-09 (code reading plus two small GTK probes; nothing tried on a device). Numbers continue after the old list (#1 to #38), so commit messages stay unambiguous. Mark a batch `[x]` when it is committed.

| Done | Batch | Items | Why this place |
|:---:|---|---|---|
| [ ] | A. Sync data safety | #40, #66, #69, #43, #51 | data loss across devices |
| [ ] | B. Recovery, import and deletes | #41, #67, #49, #54, #62 | a backup that won't restore, deletes that come back |
| [ ] | C. Sync robustness | #44, #70, #68, #83, #65 | sync stalls silently, files go missing |
| [ ] | D. Labels and presets | #74, #75, #71, #48, #82, #86 | saves fail, presets break |
| [ ] | E. Timer engine | #39, #45, #46, #47, #72 | wrong minutes, bells at wrong times |
| [ ] | F. GTK saves and playback | #42, #52, #53, #73, #76, #77 | failures that look like success |
| [ ] | G. Android | #50, #80, #78, #57, #79, #84, #63 | another app can start sessions, stale screens |
| [ ] | H. Stats numbers | #55, #59, #60 | screens disagree |
| [ ] | I. GTK translations | #56, #81, #64 | mixed languages |
| [ ] | J. Cleanup | #58, #61, #85, #87 | dead and duplicated code |

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

### #51 Android: phone-to-phone transfer copies the device id
- Where: `meditate-android/android/app/src/main/AndroidManifest.xml:41-47`.
- Problem: since API 31, `allowBackup="false"` only stops cloud backup, not device-to-device transfer. A new phone gets the old `device_id`; if the old phone keeps syncing, both write events under one id.
- Fix: `android:dataExtractionRules` with a `<device-transfer>` section excluding everything. Manifest change: ask first and run the repro probe.

## B. Recovery, import and deletes

### #41 Recovery saves 0-second sessions, and one such row makes the CSV backup unimportable
- Where: `meditate-core/src/db/session_in_progress.rs:182-207`; `data_io.rs:197, 301`; both shells write a 0 s snapshot at start (gtk `timer/imp.rs:2181`, android `ui.rs:4153`).
- Problem: killed within the first minute, the next launch saves a 0 s session ("Recovered 0 min"), which syncs and is exported. CSV and Insight Timer import both fail the whole file on a 0-length row. Normal Save and `hold_ended_session` already drop 0 s sessions.
- Fix: in `finalize_session_in_progress`, delete a 0 s snapshot and return `None`. In both importers, skip 0-length rows instead of failing (update `parse_insighttimer_csv_rejects_zero_duration_with_line_number`).

### #67 GTK: a Log delete still waiting for Undo is lost on close or quit
- Where: `meditate-gtk/src/log/imp.rs:701-706, 727-738`, `get_app` at 1215.
- Problem: the row is only deleted when the 5 s Undo toast is dismissed. Probe (libadwaita 1.7): on window close `dismissed` fires but `root()` is already None, so `commit_all_pending` drops the pending deletes; on Ctrl+Q it never fires. The session is back on the next launch.
- Fix: commit pending deletes in the window's `close_request`; make the `app.quit` action close the windows instead of calling `quit()`.

### #49 Millisecond timestamps in a CSV land in 1970 and rows are dropped
- Where: `meditate-core/src/data_io.rs:188-192`, `time.rs:73-79`.
- Problem: an out-of-range `start_time_unix` silently becomes 1970-01-01; the duplicate check then keeps one row per duration (200 × 600 s becomes 1) and the import reports success.
- Fix: parse error "line N: bad start_time_unix" when the year is outside 0..=9999. One test.

### #54 A failed file import leaves a partial file
- Where: `meditate-core/src/sound.rs:215-238` (`safe_copy_no_follow`), used by gtk `guided.rs:400` and `sounds.rs:709`.
- Problem: if `write_all` fails (disk full), the half-written `<uuid>.ogg` stays with no row pointing to it.
- Fix: remove `dest` when any step after open fails.

### #62 Label merge is one transaction per session
- Where: `meditate-core/src/db/labels.rs:238-247` (`merge_labels`).
- Problem: 1,500 sessions take 1,500 write locks; a BUSY from the sync worker stops the merge halfway (sessions partly retagged, label still there). Nothing is lost, the merge can be repeated.
- Fix: one transaction with a tx-less update, like `insert_session_tx_less`.

## C. Sync robustness

### #44 Retry-After is not capped, so a sync can hang for hours
- Where: `meditate-core/src/sync/backoff.rs:72-74`, used by `put_with_rate_limit_retry` in the orchestrator.
- Problem: only the fallback wait is capped at 30 s. `Retry-After: 3600` sleeps the worker 1 h per retry, up to 8 times; `in_flight` stays true and every other sync is skipped as AlreadyRunning.
- Fix: return `RateLimited` right away when Retry-After exceeds the cap.

### #70 Changing the sync URL with an empty password field silently stops sync
- Where: `meditate-core/src/sync/credentials.rs:176-178`; keychains key on (url, username) (gtk `keychain.rs:95-99`, android `keychain.rs:42-45`).
- Problem: empty field means `PasswordAction::Keep`. After fixing a URL typo, the toast says saved, every sync fails with `PasswordMissing`, and neither shell records it in the sync status (GTK: #42; Android `ui.rs:230-234` only logs).
- Fix: `prepare_save` takes the previous account and returns `NoPassword` when URL or username changed and the field is empty; the shells' `unreachable!` arms become a toast.

### #68 A sound or guided file that comes back after a delete never gets its audio again
- Where: `meditate-core/src/sync/orchestrator.rs:715-720, 880-885`.
- Problem: pull only downloads uuids not in `known_remote_sounds` / `known_remote_guided_files`. Deletes remove the local audio (gtk `guided.rs:1124`, `sounds.rs:988`; android `ui.rs:6287`; `remove_files_of_pulled_deletes`) but leave the uuid "known". If a peer brings the row back (higher-Lamport star, or Undo after a peer pulled the delete), it never downloads again.
- Fix: delete the `!known.contains(..)` filter in both pulls; the `local.exists()` check already prevents double downloads.

### #83 Two small sync gaps: a batch compacted mid-pull, and leftover `.tmp` files
- Where: `meditate-core/src/sync/orchestrator.rs:313` and `1027-1046, 432`.
- Problem: a peer compacts batch X between listing and GET, the GET returns `NotFound`, and the whole sync fails (push skipped). A PUT that completes server-side but times out client-side leaves a full `<lamport>-<uuid>.json.tmp` forever (fresh batch uuid each push, so it is never overwritten).
- Fix: `Err(WebDavError::NotFound) => continue` without marking the file known; `let _ = webdav.delete(&tmp_path);` when the PUT fails.

### #65 A label deleted on one device and renamed on another comes back without its sessions
- Where: `meditate-core/src/db/labels.rs:106-118, 280-330`, `ON DELETE SET NULL` on `sessions.label_id`.
- Problem: laptop deletes L, phone renames L concurrently, the rename wins; on the laptop L returns with a new rowid but its sessions stay unlabelled. Editing one of them there then sends `label_uuid: null` to every peer.
- Fix: when `recompute_label` inserts a new row, recompute the sessions whose events name that label uuid. Or accept the gap: it needs a concurrent delete and rename.

## D. Labels and presets

### #74 GTK: deleting the selected label on the Done page makes Save fail
- Where: `meditate-gtk/src/timer/imp.rs:2395, 2412-2419`; core `db/sessions.rs:449`.
- Problem: `done_selected_label_id` keeps a deleted id (deleted in the chooser or by sync). `label_uuid_by_id` errors, Save shows "storage error", the session disappears and comes back from the recovery snapshot up to 60 s short, without note or label.
- Fix: load the label list (already loaded at 2428) before building `data` and drop an id that is no longer in it.

### #75 Android: label Merge skips the cleanup Delete does
- Where: `meditate-android/src/ui.rs:7784-7810` (compare Delete at 7743-7770).
- Problem: only `refresh_label_state` runs. With the merged label selected on Edit Session or Done, Save fails with "storage error" (foreign key); a Log filter on it shows "No matching sessions".
- Fix: `refresh_after_label_change(&ui, mode); refresh_filter_label_items(&ui);` before `reset_log_feed`.

### #71 Deleting a custom sound or pattern makes some presets impossible to apply
- Where: `meditate-core/src/preset_config.rs:484-535`; message at gtk `timer/imp.rs:3544`, android `ui.rs:1281`.
- Problem: `apply` returns `SyncPending` ("Wait for sync") for any missing reference and cannot tell "not synced yet" from "deleted". It also checks references that never fire: interval bell sounds with bells off (`:490`), the starting-bell pattern with the bell off (`:507`), Box Breath phase patterns on a Timer preset (`:518`; `snapshot` always stores them). Delete pattern "Soft" used on a phase, and Timer preset "Morning" says "Wait for sync" forever on every device.
- Fix: skip a missing uuid that has a `bell_sound_delete` / `vibration_pattern_delete` event; check phases only for BoxBreath. One test.

### #48 Applying a preset doubles interval bells across devices
- Where: `meditate-core/src/preset_config.rs:617-656` (step 4 of `apply`).
- Problem: every apply deletes all interval bells and inserts new uuids, one transaction per write. Phone and laptop both apply before syncing, and after sync both sets exist, so each bell rings twice. A failure midway leaves the library half deleted; an unchanged preset still writes events.
- Fix: update existing rows in order with `update_interval_bell`, delete extras, insert only the missing ones.

### #82 The preset row counts switched-off bells
- Where: `meditate-core/src/format.rs:567-572`; capture at `preset_config.rs:399-414`; Setup count `bells.rs:540`.
- Problem: library of 3 bells, 1 enabled: the preset says "3 bells", Setup says "1 enabled", 1 rings. A stopwatch preset also counts a "before end" bell that can never ring.
- Fix: count `b.enabled && !is_bell_inert_in_stopwatch(kind, display)`; add a disabled bell to `preset_subtitle_parts_bells_uses_one_or_many`.

### #86 A label merge leaves the per-mode default label on the deleted label
- Where: `meditate-core/src/db/labels.rs:238-252` (`merge_labels`).
- Problem: `default_label_uuid_*` settings are not rewritten; if a mode's default was the merged-away "(conflict)" label, Setup shows "(none, pick one)" and new sessions save unlabelled.
- Fix: in `merge_labels`, repoint any per-mode default from the deleted uuid to the kept one. One test.

## E. Timer engine

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

## F. GTK saves and playback

### #42 GTK: a sync that never starts still shows as healthy
- Where: `meditate-gtk/src/sync_runner.rs:108-124, 172`.
- Problem: early returns (Unconfigured, PasswordMissing, keychain error, busy timeout, `get_sync_state`) skip `record_outcome`; the error only goes to the diagnostics log. Saving settings with no password leaves "waiting for first run" forever; a later wiped keyring keeps "Synced N ago".
- Fix: record those errors with `record_sync_error` in `run_sync_attempt`. About 5 lines.

### #52 GTK: database write errors are thrown away
- Where: `presets.rs:292, 317, 368, 583, 607`; `labels.rs:402`; `log/imp.rs:733`; `vibrations.rs:481, 520`.
- Problem: `let _ = db…` or a dropped `with_db_mut` result. "Override preset" during a sync lock fails with BUSY after 1 s, yet the toast says it was overridden. Undo after a preset delete can fail silently on a name clash.
- Fix: `if let Err(e)`, log, toast. One line per site.

### #53 GTK: a volume change is lost if the bell page closes within 400 ms
- Where: `meditate-gtk/src/volume_row.rs:107-130`.
- Problem: the settle timeout uses `#[weak] stop`; once the page is popped the whole body, including `save(volume)`, is skipped.
- Fix: save first, upgrade `stop` only for the preview part.

### #73 GTK: the Box Breath Duration row shows the Timer duration
- Where: `meditate-gtk/src/timer/imp.rs:3185-3187` (`refresh_streak`), called on return from Stats (`window/imp.rs:143`) and from `apply_config` (3510).
- Problem: it always writes `countdown_target_secs`. Box Breath 5 min, Timer 10 min: the row says 0:10, Start runs 5 min.
- Fix: call `refresh_duration_value_label()` there; drop the redundant write in `load_breathing_settings` (4246-4247).

### #76 GTK: files with `#` or `%` in the name cannot be opened
- Where: `meditate-gtk/src/guided.rs:1261, 1372` (`format!("file://{}", abs)`).
- Problem: probe: "Talk #3.ogg" and "100%25 calm.ogg" fail in playbin; Open File says "Couldn't read audio file". The bell import pre-check uses the same probe.
- Fix: `glib::filename_to_uri(&abs, None)` at both places.

### #77 GTK: a replaced vibration keeps sending its later chunks
- Where: `meditate-gtk/src/vibration.rs:206-208` (`disarm`); callers `timer/imp.rs:3910`, `vibrations.rs:319`, `vibration_editor.rs:582`.
- Problem: `disarm` only skips the drop-cancel and never sets `cancel`, so chunk 1+ timeouts of a long pattern still fire and replace the new pattern. Not tried on the Librem 5.
- Fix: `self.cancel.store(true, Ordering::Relaxed)` in `disarm`.

## G. Android

### #50 Android: any installed app can start a meditation
- Where: `meditate-android/kotlin/MeditateWidgetProvider.kt:55-78`; receiver `exported="true"` in the manifest.
- Problem: `WIDGET_LAUNCH` arrives at the exported widget receiver, which writes `widget_launch` without a check. The default preset uuids are public constants. A session starts (now or at next launch) and its end bell rings at alarm volume.
- Fix: a small non-exported receiver for the launch action; point the widget's tap PendingIntent at it. Manifest change: ask first.

### #80 Android: a failed preset delete or rename looks successful
- Where: `meditate-android/src/ui.rs:5818-5850` (delete), 5770-5776 (rename); Override at 5655-5664 is correct.
- Problem: a BUSY delete still shows "'X' deleted" with Undo while X stays listed; Undo then fails on the same uuid. Rename closes its dialog with no message on failure. Android side of #52.
- Fix: in the `Err` branch, `show_notice(delete_failed)` and return, as Override does.

### #78 Android: the Timer streak line does not update after Save
- Where: `meditate-android/src/ui.rs:1550` (`set_streak_text` only in `refresh_stats`), `on_save_tap` 4770-4859, `on_edit_save_tap` 8199-8330.
- Problem: after the first session of the day it still says "No streak" until Stats opens, a sync pulls, or a restart.
- Fix: `refresh_stats(&ui);` after `reset_log_feed` in both handlers.

### #57 Android: guided audio quirks
- Where: `kotlin/MeditateGuided.kt:38`; `src/ui.rs:5332-5334`.
- Problem: `startAudio` ignores a refused audio focus, so a track plays during a call and is never paused. In Manage Files, Play on a not-yet-downloaded file does nothing visible.
- Fix: return false when focus is refused (Rust already shows "Couldn't start playback"); show `invoke_playback_failed` in the preview path.

### #79 Android: the Simplified Chinese translation is never picked
- Where: `meditate-android/kotlin/MeditateAbout.kt:268-270`; `src/ui.rs:3831-3841`.
- Problem: the system tag is usually `zh-Hans-CN`, which becomes `zh_Hans_CN` then `zh`; the bundle is `zh_CN`, so the app stays English. Not checked on a device.
- Fix: return `"${l.language}-${l.country}"` (or just `language` when country is empty).

### #84 Android: outdated names after renames
- Where: `meditate-android/src/ui.rs:4329-4334, 781` (widget); 6228-6297 (sound rename/delete).
- Problem: renaming label "Yoga" to "Zen" leaves the widget subtitle at "Yoga". Renaming or deleting a sound leaves the Interval Bells list showing the old name.
- Fix: `refresh_widget(&ui);` next to `refresh_preset_chips` at 4333 (then the separate calls after preset actions can go); `populate_interval_bells(&ui);` in both sound handlers.

### #63 Android: a paused session keeps the phone awake
- Where: `kotlin/MeditateSessionService.kt:178-186`, `src/ui.rs:277-313`.
- Problem: pause keeps the partial wake lock, renewed every 30 min, so the 200 ms tick runs all night if a paused session is forgotten.
- Fix: release on pause, re-acquire on resume. Or skip if this never happens in practice.

## H. Stats numbers

### #55 Goal ring and heatmap round down, the Log rounds to nearest
- Where: `meditate-core/src/goal.rs:103`, `contrib.rs:91` vs `format.rs:389-397`.
- Problem: a 19:30 session with a 20 min goal: Log says "20 min", ring says "1 min to go". A 40 s session shows "1 min" in the Log but leaves its heatmap cell empty.
- Fix: use `(secs + 30) / 60` in `goal::compute` and `build_grid`.

### #59 Week-over-week compares part of today with a full day
- Where: `meditate-core/src/date_math.rs:105-117`.
- Problem: Monday 07:00, nothing yet today, 30 min last Monday: the card shows -100% every morning.
- Fix: sum through yesterday on both sides; skip the card when that is 0 days.

### #60 Year chart has two partial months, and both shells build the series
- Where: `meditate-gtk/src/stats/imp.rs:416-444`, `meditate-android/src/ui.rs:1708-1735`.
- Problem: on 2026-10-09 the year view starts 2025-10-10: 13 bars, first and last October both partial and both labelled "O". Both shells zero-fill the daily series in their own code.
- Fix: one core `chart_series(totals, today, period)` starting the year window on the 1st of a month; both shells call it.

## I. GTK translations

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

## J. Cleanup

### #58 Dead and duplicated code (core and Android)
- Core: `SessionPhase::Paused` (`session/mod.rs:121`) is never created; delete it and its 3 match arms. `daily_totals()` / `get_daily_totals_from_db` (`db/sessions.rs:172, 671-697`) reads the whole table for the 91-day heatmaps; call the `_since` version and delete it with its tests.
- Android: about 60 `let _ = weak.clone();` and 5 `let _ = current_mode.get();` (e.g. `ui.rs:4751`); `AppState::primary_label`, `remaining`, `is_idle` (`app.rs:392, 594, 616`) used only by their own tests; Kotlin `MeditateKeychain.clearPassword` has no caller; `service.rs::call` (87-104) repeats `jni_call::load_class`.

### #61 GTK dead code and the duplicated transcode pipeline
- Dead: `populating` Cell never set (`bells.rs:604`, guards at 630, 647, 661, 740); unused `toast_slot` (`presets.rs:336/360`); unused `chart_h` (`stats/imp.rs:465/496`); unused `area` (`stats/imp.rs:697/707`); `let _ = BellSoundCategory::General` (`db/mod.rs:372`); half-deleted "Throwaway" comment (`timer/imp.rs:1170-1175`).
- Duplicate: `sounds.rs:735-849` and `guided.rs:433-534` are the same gst pipeline except `audioloudnorm` and the cancel-aware bus loop; one function with `loudnorm: bool` and `cancel: Option<&AtomicBool>`, about -80 lines.

### #85 Dead code, duplicates and a test that cannot fail
- Core: `Effect::EndPrep` is never consumed (effect.rs:21-25, mod.rs:460, asserts 1065/1077/1636, ARCHITECTURE.md); `tick_box_breath` calls `fire_due_bells` though Box Breath never has interval bells (mod.rs:597-604, test 1475); `db_open_failure_key` (format.rs:295-315) is unused and repeated in gtk `application.rs:168-177`; `Stopwatch` serde derives and tests (timer.rs:1-12, 86-113) are unused and assert a false restart claim; `box_breath_phases.rs:17-47` repeats `get_box_breath_phase` and hard-codes the seed uuids at 141-144; `list_bell_sounds_for_category` (bell_sounds.rs:236-264) copies `list_bell_sounds`; `count_labels_from_db` (labels.rs:68) is test-only.
- GTK: `preload_end_bell` builds a media file that is never played, `play_starting_sound` has no caller (sound.rs:250-301, calls at timer/imp.rs:688, 735, 1209-1212, window/imp.rs:111-118).
- Android: label rename/delete handlers repeat `refresh_after_label_change` (ui.rs:7700-7706, 7752-7764); `ui.rs:537-545` repeats core `bells::signal_mode_override_from_db`.
- Test: `bell_rng_state_advances_deterministically_from_seed` (mod.rs:1488-1520) uses `jitter_pct: 0`, so the RNG is never used; set 20 and assert a different seed differs, or delete it.

### #87 Trimming the diagnostics log drops its 0600 permission
- Where: `meditate-core/src/diag.rs:165-185` vs `:77-87`.
- Problem: `trim_to_tail` writes the copy with `File::create` (0644) and renames it over the 0600 log. No secrets are logged today, and Flatpak/Android keep it private anyway.
- Fix: create the temp copy with `OpenOptions` and `.mode(0o600)`, as `init` does.

---

## Deferred refactors

Only together with other work in the same area; none has a bug behind it.

- **R12, sync runner into core**: core `run_attempt` with a password closure, `SyncError::is_transient_network()`, a typed error, one blob routine for sounds and guided files (remote paths byte-identical). About -150 in the apps, +100 in core, -90 in the orchestrator. Includes the narrow sync-account error enums that remove four `unreachable!` arms. Do it when sync needs work.

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
- A `guided_file_uuid` CSV column (#7): after a restore the uuid points at no file, and synced devices get their sessions by sync anyway.
- Typed `BellSlot` through Slint: large, with no host safety net.
- Moving the whole sync account Save/Test into core: the keychains are per app.
- Moving the Log edit build into core: low value; reconsider when fixing #5/#6.
- Splitting `ui.rs` per screen or the big core files: cosmetic.
- A `Divider` component, a page stack model, caching JNI classes, moving the CAN_DUCK policy to Rust, a typed `SettingKey` enum, a core pending-Undo queue.
- Left out of the bug audit on purpose: configChanges / activity recreate and the import-labels transaction (both in TODO.md); Back on the Done screen discarding the session (decided behaviour).
