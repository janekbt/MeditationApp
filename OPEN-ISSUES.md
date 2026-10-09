# Open issues (2026-10-09)

One list for everything still open: the Android bugs (formerly ANDROID-BUGS.md) and the refactors left from the audit (formerly REFACTOR-AUDIT.md). Done items are removed; the git history has both old files (`git show 6e81927:ANDROID-BUGS.md`, `git show 6e81927:REFACTOR-AUDIT.md`). Bug numbers stay as they were, because commit messages refer to them.

**Ground rules:**
- Decisions go in core.
- No DB schema or sync wire-format change without a migration, and nothing touches the Android build environment (Cargo.toml, Cargo.lock, build.rs, main.rs, gradle, the manifest, rust-build.sh) without asking.
- msgids stay unless the item says otherwise; new ones get all nine translations.
- TDD, then a phone check before the commit.
- Before the next release: run `build-aux/repro-probe.sh`.

## Batches, in suggested order

From the fresh-eyes audit of 2026-10-09 (code reading only; no tests run, nothing tried on a device). Numbers continue after the old list (#1 to #38), so commit messages stay unambiguous. Mark a batch `[x]` when it is committed.

| Done | Batch | Items | Why this place |
|:---:|---|---|---|
| [ ] | A. Sync safety | #40, #43, #44, #51, #48, #65 | data loss across devices |
| [ ] | B. Recovery and import | #41, #49, #54, #62 | a backup that won't restore |
| [ ] | C. Timer engine | #39, #45, #46, #47 | wrong minutes, bells at wrong times |
| [ ] | D. GTK saves and sync status | #42, #52, #53, #56, #64 | failures that look like success |
| [ ] | E. Android | #50, #57, #63 | another app can start sessions |
| [ ] | F. Stats numbers | #55, #59, #60 | screens disagree |
| [ ] | G. Cleanup | #58, #61 | dead and duplicated code |

---

## A. Sync safety

### #40 After "wipe local", new edits lose to the device's own old ones
- Where: `meditate-core/src/db/events.rs:572` (`apply_event_record_only`), `wipe_local_event_log` sets `lamport_clock = 0`.
- Problem: pulling its own old events back does not raise the device's Lamport clock. Laptop was at 500, ends near 301 after the wipe; an edit to a session last changed at 480 loses on every device, and a delete loses to the old insert, so the session comes back. Restoring an old GTK database backup does the same and can create duplicate `(lamport, device_id)` pairs.
- Fix: drop `event.device_id != self.device_id()?`; `was_new` already guards retries. Flip `apply_event_does_not_advance_local_lamport_for_our_own_device_events`.

### #43 A failed pull also blocks the upload
- Where: `meditate-core/src/sync/orchestrator.rs:496` (`self.pull()?` before the push).
- Problem: during version skew (phone updated first, sends a value the old laptop's CHECK rejects), every laptop pull fails, so its own sessions never upload until it updates.
- Fix: on `Db`/`InvalidEvent` pull errors, still push, then return the pull error. Keep `RemoteDataLost` and transport errors fatal. One test with a CHECK failure on pull.

### #44 Retry-After is not capped, so a sync can hang for hours
- Where: `meditate-core/src/sync/backoff.rs:72-74`, used by `put_with_rate_limit_retry` in the orchestrator.
- Problem: only the fallback wait is capped at 30 s. `Retry-After: 3600` sleeps the worker 1 h per retry, up to 8 times; `in_flight` stays true and every other sync is skipped as AlreadyRunning.
- Fix: return `RateLimited` right away when Retry-After exceeds the cap.

### #51 Android: phone-to-phone transfer copies the device id
- Where: `meditate-android/android/app/src/main/AndroidManifest.xml:41-47`.
- Problem: since API 31, `allowBackup="false"` only stops cloud backup, not device-to-device transfer. A new phone gets the old `device_id`; if the old phone keeps syncing, both write events under one id.
- Fix: `android:dataExtractionRules` with a `<device-transfer>` section excluding everything. Manifest change: ask first and run the repro probe.

### #48 Applying a preset doubles interval bells across devices
- Where: `meditate-core/src/preset_config.rs:617-656` (step 4 of `apply`).
- Problem: every apply deletes all interval bells and inserts new uuids, one transaction per write. Phone and laptop both apply before syncing, and after sync both sets exist, so each bell rings twice. A failure midway leaves the library half deleted; an unchanged preset still writes events.
- Fix: update existing rows in order with `update_interval_bell`, delete extras, insert only the missing ones.

### #65 A label deleted on one device and renamed on another comes back without its sessions
- Where: `meditate-core/src/db/labels.rs:106-118, 280-330`, `ON DELETE SET NULL` on `sessions.label_id`.
- Problem: laptop deletes L, phone renames L concurrently, the rename wins; on the laptop L returns with a new rowid but its sessions stay unlabelled. Editing one of them there then sends `label_uuid: null` to every peer.
- Fix: when `recompute_label` inserts a new row, recompute the sessions whose events name that label uuid. Or accept the gap: it needs a concurrent delete and rename.

## B. Recovery and import

### #41 Recovery saves 0-second sessions, and one such row makes the CSV backup unimportable
- Where: `meditate-core/src/db/session_in_progress.rs:182-207`; `data_io.rs:197, 301`; both shells write a 0 s snapshot at start (gtk `timer/imp.rs:2181`, android `ui.rs:4153`).
- Problem: killed within the first minute, the next launch saves a 0 s session ("Recovered 0 min"), which syncs and is exported. CSV and Insight Timer import both fail the whole file on a 0-length row. Normal Save and `hold_ended_session` already drop 0 s sessions.
- Fix: in `finalize_session_in_progress`, delete a 0 s snapshot and return `None`. In both importers, skip 0-length rows instead of failing (update `parse_insighttimer_csv_rejects_zero_duration_with_line_number`).

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

## C. Timer engine

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

## D. GTK saves and sync status

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

### #56 GTK: the completion notification is English and copied twice
- Where: `meditate-gtk/src/timer/imp.rs:2797-2809` and `2972-2980`; `preferences.rs:636` (`"CSV files"` filter).
- Problem: "Meditation Complete" and "Session: …" have no gettext; the same block is in `transition_running_to_overtime` and `finish_breath_session`. The import filter name is unwrapped while export wraps it.
- Fix: one `send_done_notification` helper with gettext; wrap the filter name; all nine translations.

### #64 GTK: 14 user-visible strings use the long dash
- Where: e.g. `timer/imp.rs` and `log/imp.rs:1189` ("Couldn't save session — storage error"), `window/imp.rs:677`, `stats/imp.rs:181-304`, `guided.rs:733/735`, `preferences.rs:68`, `bells.rs:484`, sync error texts at `sync_runner.rs:66-69` ("—" and "→").
- Fix: rephrase with comma or colon, update msgids and the .po files.

## E. Android

### #50 Android: any installed app can start a meditation
- Where: `meditate-android/kotlin/MeditateWidgetProvider.kt:55-78`; receiver `exported="true"` in the manifest.
- Problem: `WIDGET_LAUNCH` arrives at the exported widget receiver, which writes `widget_launch` without a check. The default preset uuids are public constants. A session starts (now or at next launch) and its end bell rings at alarm volume.
- Fix: a small non-exported receiver for the launch action; point the widget's tap PendingIntent at it. Manifest change: ask first.

### #57 Android: guided audio quirks
- Where: `kotlin/MeditateGuided.kt:38`; `src/ui.rs:5332-5334`.
- Problem: `startAudio` ignores a refused audio focus, so a track plays during a call and is never paused. In Manage Files, Play on a not-yet-downloaded file does nothing visible.
- Fix: return false when focus is refused (Rust already shows "Couldn't start playback"); show `invoke_playback_failed` in the preview path.

### #63 Android: a paused session keeps the phone awake
- Where: `kotlin/MeditateSessionService.kt:178-186`, `src/ui.rs:277-313`.
- Problem: pause keeps the partial wake lock, renewed every 30 min, so the 200 ms tick runs all night if a paused session is forgotten.
- Fix: release on pause, re-acquire on resume. Or skip if this never happens in practice.

## F. Stats numbers

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

## G. Cleanup

### #58 Dead and duplicated code (core and Android)
- Core: `SessionPhase::Paused` (`session/mod.rs:121`) is never created; delete it and its 3 match arms. `daily_totals()` / `get_daily_totals_from_db` (`db/sessions.rs:172, 671-697`) reads the whole table for the 91-day heatmaps; call the `_since` version and delete it with its tests.
- Android: about 60 `let _ = weak.clone();` and 5 `let _ = current_mode.get();` (e.g. `ui.rs:4751`); `AppState::primary_label`, `remaining`, `is_idle` (`app.rs:392, 594, 616`) used only by their own tests; Kotlin `MeditateKeychain.clearPassword` has no caller; `service.rs::call` (87-104) repeats `jni_call::load_class`.

### #61 GTK dead code and the duplicated transcode pipeline
- Dead: `populating` Cell never set (`bells.rs:604`, guards at 630, 647, 661, 740); unused `toast_slot` (`presets.rs:336/360`); unused `chart_h` (`stats/imp.rs:465/496`); unused `area` (`stats/imp.rs:697/707`); `let _ = BellSoundCategory::General` (`db/mod.rs:372`); half-deleted "Throwaway" comment (`timer/imp.rs:1170-1175`).
- Duplicate: `sounds.rs:735-849` and `guided.rs:433-534` are the same gst pipeline except `audioloudnorm` and the cancel-aware bus loop; one function with `loudnorm: bool` and `cancel: Option<&AtomicBool>`, about -80 lines.

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
