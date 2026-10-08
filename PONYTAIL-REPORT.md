# Ponytail audit (2026-10-08)

General audit of the whole repo, run on beta at afc6697. Nothing was changed in the code.

Five read-only reviewers covered sync, the database and import, the GTK app, the core logic, and build, release and CI. Findings 1, 3, 5, 6, 7 and 13 were then rechecked directly in the code. Items already in ANDROID-BUGS.md or REFACTOR-AUDIT.md are left out.

**What this repo does:**
- Meditate is a meditation timer with two apps. The Linux app is built with GTK; the Android app is built with Slint.
- Both apps share one Rust core, which decides how sessions, bells and stats work.
- That core keeps your history in a local SQLite database. It syncs it between your devices through your own WebDAV server, such as Nextcloud.

**Assumed load:** one person, 1 to 3 devices, one server.

## Must fix

### 1. A half-downloaded sound file is uploaded over the good copy on the server
`meditate-core/src/sync/orchestrator.rs:744-758`, `891-905`, push at `781`
- **What this is:** Sync downloads custom bells and guided files that other devices added, and uploads your own.
- **Problem:**
  - The download writes straight into the final file. If Android kills the app partway through, a cut-off file is left behind.
  - The next sync sees "file exists" and skips the download.
  - The file isn't marked as being on the server yet, so the upload step sends the broken file and overwrites the good copy.
- **Fix:** download to `<file>.part`, flush to disk, then rename into place. About 5 lines in each of the two download loops.
- **If we skip it:** a broken audio file spreads to every device, and only the importing device keeps the original.

### 2. One bad event from another device stops sync for good
`meditate-core/src/db/events.rs:602`, `sessions.rs:1009-1032`, `device.rs:55,70`, `orchestrator.rs:321-323`
- **What this is:** An "event" is one recorded change, such as "session added". Devices send each other batches of events through the server.
- **Problem:** any of these stops the whole batch:
  - a session mode or bell category this version doesn't know (a future version adding a mode is enough; verified with a test);
  - an event that isn't valid JSON, or a batch file that can't be read;
  - an id longer than 255 characters;
  - a clock value near the largest possible number.

  The whole batch is rolled back and never marked as done, so every later sync fails at the same spot. With the clock value, every local save fails too, and "wipe local" does not help.
  Same pattern in `bell_sounds.rs:291`, `presets.rs:369`, `interval_bells.rs:267,274`, `vibration_patterns.rs:332`, `box_breath_phases.rs:140`.
- **Fix:** in the replay code, skip an event that can't be applied, the same way unknown event kinds are already skipped:
  - catch "CHECK constraint failed";
  - reject clock values outside a sane range;
  - cap the id length;
  - mark an unreadable batch as done.

  Add one test per case.
- **If we skip it:** one app update or one damaged file on the server can stop sync on every device, and the user can't fix it.

### 3. The server can crash the app with one HTTP header
`meditate-core/src/sync/backoff.rs:80`, `orchestrator.rs:967-970`
- **What this is:** When the server answers "too busy" (HTTP 429), the app waits for as long as the server's "Retry-After" header asks.
- **Problem:**
  - The wait is used without a limit. A huge value overflows the time calculation and panics.
  - Release builds abort on a panic (`Cargo.toml:20`), so both apps die.
  - A value like 3600 keeps sync spinning for hours.
- **Fix:** `retry_after_secs.map(|s| s.min(MAX_BACKOFF_SECS * 10))`. One line.
- **If we skip it:** a misconfigured proxy can crash the app on every sync.

## Should fix

### 4. Box Breath on the desktop stops when the screen is off, and can log 30 minutes for a 5-minute sit
`meditate-gtk/src/window/imp.rs:448`, `timer/imp.rs:2293`, `meditate-core/src/session/mod.rs:577`
- **What this is:** In the GTK app, Box Breath only advances when the window redraws.
- **Problem:**
  - When the screen blanks or the window is minimized, redraws stop, and the breathing cues and the end bell stop with them.
  - The session only ends when the window is drawn again. Core then saves the elapsed time instead of the planned length.
  - Example: a 304 s session seen again after 30 minutes is saved as 1800 s (reproduced in core; the GTK part is likely).
- **Fix:** also run the existing 1-second timer for Box Breath. In core, save `target_secs` instead of `elapsed`.
- **If we skip it:** Box Breath with the screen off is broken on the Librem 5, and the stats get inflated.

### 5. Interval bells ring once per second after a suspend
`meditate-core/src/bells.rs:215-219`
- **What this is:** Interval bells ring every N minutes.
- **Problem:** the next ring time moves forward by only one interval at a time. After a 20-minute gap with a 1-minute bell, the next 20 ticks each ring: 20 bells, one second apart (reproduced).
- **Fix:** ring once, then move the next ring time forward until it is past now. Add a test.
- **If we skip it:** a burst of bells right after waking the phone.

### 6. An interval bell that falls on the end rings 1 second after the end bell
`meditate-core/src/session/mod.rs:497-501`
- **What this is:** Fixed bells at the end are already dropped so they don't ring on top of the end bell. Interval bells are not.
- **Problem:** a 10-minute timer with a 5-minute interval rings the end bell at 600 s and the interval bell at 601 s (reproduced).
- **Fix:** when the session enters overtime, use up any interval bells already due, without ringing them.
- **If we skip it:** a double bell at the end of every sit where the interval divides the length evenly.

### 7. On the desktop, Esc during overtime can end the session and lose the overtime
`meditate-gtk/src/window/imp.rs:549-556`, `timer/imp.rs:2321-2324, 2340`
- **What this is:** Esc pauses the running page.
- **Problem:**
  - In overtime, Esc pauses, and Resume then relabels the Finish button "Pause".
  - Clicking that "Pause" really runs Finish. The session saves at the planned length, without the overtime.
- **Fix:** make Esc do nothing in overtime, and have Resume restore the "Finish" label in overtime.
- **If we skip it:** the session ends unexpectedly and the overtime is lost.

### 8. The desktop app starts a full sync every minute during a session
`meditate-gtk/src/timer/imp.rs:2708, 2725`
- **What this is:** The app saves a crash-recovery snapshot every 60 seconds.
- **Problem:**
  - The snapshot uses `with_db_mut`, which also starts a sync, even though the snapshot table never syncs.
  - Each minute means a server round trip, a keyring read, and a rebuild of the Log and Stats.
  - The rebuild also collapses "Load more" back to 15 rows.
- **Fix:** use `with_db` instead of `with_db_mut` in both functions.
- **If we skip it:** network and battery use all through every meditation on the Librem 5.

### 9. A stuck file download stops the device from uploading its sessions
`meditate-core/src/sync/orchestrator.rs:357-358, 503-504`
- **What this is:** One sync downloads first, then uploads.
- **Problem:** if one audio download keeps failing (disk full, flaky mobile link), the download step returns an error and the upload never runs. New sessions stay only on that device.
- **Fix:** make a failed file download non-fatal: log it, retry on the next sync, and continue. It already works that way for "not found".
- **If we skip it:** a phone that is low on space silently stops backing up its sessions.

### 10. Switching to a new server leaves your history behind
`meditate-core/src/sync/settings.rs:61-74`
- **What this is:** Changing the server address or user name resets the list of files known to be on the server.
- **Problem:** it resets the audio-file lists but not the "already uploaded" flag on events. The new server only gets changes made after the switch, so a new device on it sees none of the old sessions.
- **Fix:** call `db.flag_all_events_unsynced()` in the same branch.
- **If we skip it:** moving servers quietly drops your history from sync.

### 11. On the desktop, guided files with `#` or `%` in the name can't be used
`meditate-gtk/src/guided.rs:1261, 1372`
- **What this is:** Guided playback builds a `file://` address for GStreamer.
- **Problem:** the address is glued together without escaping. "Body Scan #2.mp3" or "100% Calm.ogg" fails with "Couldn't read audio file" (tested).
- **Fix:** `glib::filename_to_uri(&path, None)` in both places.
- **If we skip it:** some files can never be imported.

### 12. A CSV with millisecond timestamps imports as one session in 1970 and reports success
`meditate-core/src/data_io.rs:158-162`, `time.rs:73-79`, dedupe at `data_io.rs:341-355`
- **What this is:** Restoring sessions from a CSV backup.
- **Problem:**
  - The start time isn't range-checked. Values that are too large are clamped to 1970-01-01.
  - All rows then look the same, the duplicate check merges them, and one wrong row is saved (reproduced).
- **Fix:** reject a row whose time is outside the years 1971 to 9999, with the message "line N: start time out of range".
- **If we skip it:** silent data loss when restoring an edited or foreign backup.

### 13. "Practising less" shows for most of every month
`meditate-core/src/insights.rs:162-171`
- **What this is:** The Stats card compares this month with last month.
- **Problem:** it compares part of this month with all of last month. Someone who sits 20 minutes every day sees "−73%" on 8 October.
- **Fix:** compare against the same days of last month, the way the week card already does.
- **If we skip it:** a wrong, discouraging message for steady practisers.

### 14. A guided WAV of 10 minutes or longer never syncs, with no message
`meditate-core/src/sync/orchestrator.rs:940-943`, GTK `guided.rs:394`, `kotlin/MeditateGuidedImport.kt:111`
- **What this is:** Uploads skip guided files over 100 MB, but imports copy WAV files unchanged and have no size limit.
- **Problem:** a 10-minute CD-quality WAV is about 106 MB. Other devices get the entry but never the audio.
- **Fix:** check the 100 MB limit at import and show a message; log the skip.
- **If we skip it:** guided files show up on other devices but won't play.

### 15. The screenshot links in the Linux app description are broken
`meditate-gtk/data/io.github.janekbt.Meditate.metainfo.xml.in:27, 31, 35`
- **What this is:** The screenshots that GNOME Software shows.
- **Problem:** the links point to `data/screenshots/`, which returns 404. The files are now under `meditate-gtk/data/screenshots/` (checked). CI validates offline, so it doesn't notice.
- **Fix:** add `meditate-gtk/` to the three links.
- **If we skip it:** software centres show no screenshots.

### 16. build.sh lets through a Rust version that can't build the app
`build.sh:72`, `meditate-gtk/Cargo.toml:6`, `clippy.toml:5`
- **What this is:** The script's early check for a new enough Rust compiler (rustc).
- **Problem:** it accepts 1.85, but the pinned gtk4 and glib need 1.92. On Debian 13 the build passes every check, then fails halfway. `rust-version` and `msrv` still say 1.75.
- **Fix:** raise the check, `rust-version` and `msrv` to 1.92.
- **If we skip it:** exactly the support question the check exists to prevent.

### 17. A release APK can be built without the pinned dex tool, and nothing fails
`meditate-android/android/rust-build.sh:42-51`
- **What this is:** The pins that make the APK match F-Droid's rebuild byte for byte. The dex tool turns Java code into Android's format.
- **Problem:** if build-tools 31.0.0 is missing, a release build only prints a warning; if platform 34 is missing, it prints nothing. Both exit with success, so you get a signed APK that F-Droid's reproducibility check will reject.
- **Fix:** fail a release build when a pin is missing. **This touches the Android build setup, so it needs Janek's approval (F-Droid reproducibility).**
- **If we skip it:** a tagged release that F-Droid never publishes.

### 18. CI uses actions and images that can change under us
`.github/workflows/flatpak.yml:27-165`
- **What this is:** The Flatpak bundles on GitHub releases are the CI output, uploaded unchanged.
- **Problem:**
  - Actions are pinned by tags like `@v5`, and images by name only.
  - `tonistiigi/binfmt` runs privileged with no tag at all.
  - Whoever controls those tags can change what Linux users install.
- **Fix:** pin each `uses:` to a commit SHA and each image to a `@sha256:` digest.
- **If we skip it:** the usual supply-chain exposure on the only Linux distribution path.

## Nice to have

### 19. On the desktop, the running page and the completion notification are English-only
`meditate-gtk/src/window/imp.rs:192-215, 253`, `timer/imp.rs:2882, 2892, 3057, 3063`
- **What this is:** Labels like "Pause", "Stop", "Meditation Complete".
- **Problem:** they are plain text, not passed through translation. In German the button reads "Stop", then turns into "Stopp" after Pause and Resume.
- **Fix:** wrap them in `gettext` and regenerate the .pot.
- **If we skip it:** mixed-language screens in 7 languages.

### 20. A new session shows in a filtered Log, and "Load more" then skips one
`meditate-gtk/src/log/imp.rs:259-319`, called from `timer/imp.rs:2581`
- **What this is:** After Save, the new session is added at the top of the Log.
- **Problem:** this ignores an active filter and moves the page offset by one, so the next "Load more" skips a session that does match.
- **Fix:** when a filter is active, reload the Log instead of adding the card.
- **If we skip it:** a wrong filtered view until the filter changes.

## Smaller findings (not ranked)

- **Crash recovery saves a 0-second session.**
  - Where: `db/session_in_progress.rs` (finalize).
  - The snapshot is written with 0 s at Start. A kill in the first minute gives a 0-minute card on every device.
  - Fix: clear the snapshot and return None when 0.
- **CSV export truncates the old backup before writing.**
  - Where: `data_io.rs:118`.
  - Fix: write to a `.tmp` file, then rename.
- **The `synchronous=NORMAL` comment is wrong for writes not yet synced.**
  - Where: `db/mod.rs:227-241`.
  - A power cut can lose a just-saved session that exists nowhere else.
  - Fix: correct the comment, or use FULL around Save and Edit.
- **CSV import creates labels outside the session transaction.**
  - Where: `data_io.rs:337-340`.
  - Empty labels are left behind and synced when the insert then fails.
- **A late tick at the end of prep drops the overshoot.**
  - Where: `session/mod.rs:456`.
  - Fix: `started_at(now - (elapsed - target))`.
- **The release-notes parser is limited.**
  - Where: `release_notes.rs:22, 47, 66-77`.
  - It escapes `<em>` and `<code>`, misses `<p>` with attributes or a line break after `<release`, and treats single-quoted `type='development'` as stable.
  - Latent today: the metainfo file is clean.
- **Preset apply doesn't check Box Breath phase sound uuids.**
  - Where: `preset_config.rs:438-447`.
  - Unlike pattern uuids, there is no `SyncPending`, so it plays a missing file.
- **A Line vibration pattern over about 463 s overflows u32.**
  - Where: `vibration.rs:145`.
  - The editor caps it, sync doesn't.
  - Fix: compute in u64, and clamp on replay.
- **A replaced vibration pattern keeps firing its old chunks.**
  - Where: `vibration.rs:206-208`.
  - `disarm()` doesn't set `cancel`.
- **Any app can trigger the exported widget receiver.**
  - Where: `AndroidManifest.xml:91-101`, `MeditateWidgetProvider.kt:55-78`.
  - Another app can send the `WIDGET_LAUNCH` broadcast to switch tabs, close dialogs, or start a starred preset.
  - Fix: move it to a separate receiver that isn't exported.
- **attach-flatpaks.sh can pick a "light" CI run without bundles.**
  - Where: `build-aux/attach-flatpaks.sh:24-26`.
- **RUSTFLAGS breaks on a repo path with a space.**
  - Where: `rust-build.sh:76`.
  - Fix: use `CARGO_ENCODED_RUSTFLAGS`. Needs approval (reproducibility).
- **Server text ends up in the error string and the shared log.**
  - Where: `sync/webdav.rs:97` (error page body), `:107` (redirect hostname and user).
  - Fix: show only the status code, and truncate and sanitize the rest.
- **Leftover config:**
  - `meditate-android/manifest.yaml`, the xbuild leftover (and the comment at `Cargo.toml:76-80`);
  - `meditate-gtk/src/meson.build:3-9` (`cargo_profile_args`, unused);
  - the two Notifications talk-names in the Flatpak manifest (likely surplus under the portal).
- **Limitation, not a bug:** TLS trusts only the built-in root certificates, so a Nextcloud with a self-signed or private-CA certificate can't connect.

## Summary

**Verdict:** mostly healthy, but sync is the weak point. Fix 1 to 3 first, and before any release that adds a new session mode or bell category, because older versions would stop syncing.

**Lean:** about -40 lines from leftover config. The larger savings are in REFACTOR-AUDIT.md.

**Not checked:**
- Nothing was run on a device or against a real Nextcloud.
- XML entity limits on hostile PROPFIND replies (the server's file listings).
- The Android keychain key scheme.
- How long the GTK keyring calls and the audio-length probe freeze the UI thread (not measured).
- Android's CSV export path.
- The GTK stats, presets and labels files, which were only skimmed.
