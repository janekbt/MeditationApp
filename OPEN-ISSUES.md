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
| [x] | G. Labels, names and Undo | #74, #75, #86, #101, #103, #104, #122, #158 | names that differ per device, saves that fail |
| [x] | U1. /spec groundwork (before H) | #161 | every later batch leaves specs and gets a review |
| [x] | H. Timer engine | #39, #45, #46, #47, #72, #113, #166, #185 | wrong minutes, bells at wrong times |
| [x] | I. GTK timer and bells | #73, #126, #127, #77, #130, #53, #132 | GTK controls and bells misbehave |
| [x] | J. GTK Log | #117, #118, #119, #120, #121, #123 | the Log shows wrong rows and totals |
| [x] | K. Failed writes and stale pages | #52, #80, #78, #84, #124, #136, #140, #141, #170 | failures that look like success, stale screens |
| [x] | K2. GTK settings that don't stick | #191 | a setting reverts on the next start, unnoticed |
| [ ] | K3. Presets all or nothing | #193 | a preset that fails halfway |
| [ ] | L. Android platform (manifest: ask first) | #50, #51, #57, #79, #63, #149, #176 | other apps, phone transfer, audio focus, battery saver |
| [x] | M. Android layout and editors | #139, #142, #143, #144, #145, #146, #189 | hidden controls, lost edits |
| [ ] | N. Stats numbers | #55, #59, #60, #114, #115 | screens disagree |
| [ ] | O. Dates in other languages | #151, #152, #153, #154, #116 | wrong date order and labels |
| [ ] | P. GTK translations | #56, #81, #64, #155, #156 | mixed languages |
| [ ] | Q. Sync efficiency (after G and J) | #92, #98, #99, #100, #137 | wasted network, disk and rebuilds |
| [ ] | R. Tests | #91, #138, #150 | tests that cannot catch the bug |
| [ ] | S. Cleanup | #58, #61, #85, #87, #157, #167, #171, #175 | dead code, wrong comments |
| [ ] | T. Text that doesn't fit | #159 | information the user can't see |
| [x] | U2. Docs for contributors | #163 | stale docs an AI trusts |
| [ ] | V. Randomised sync test | #162 | sync bugs that only two interleaved devices show |
| [ ] | W. Sync that stops on one bad item | #164, #165, #172, #173, #184, #188, #192 | one bad file or event blocks sync |
| [ ] | X. Release pipeline (ask first) | #168, #169, #174 | builds that aren't what F-Droid or users get |
| [ ] | Y. Slint library fixes (build environment: ask first) | #148, #160 | vendored Slint code; needs the repro probe |
| [ ] | Z. Android touch targets | #147 | controls under 48 dp; needs some UI rework |

---

## K3. Presets all or nothing

### #193 Core: applying a preset is not one transaction
- Where: `meditate-core/src/preset_config.rs` `apply` (comment above it: core::db has no nestable transactions; `set_interval_bell_enabled` opens its own); the length is written after it by each app (`set_timer_session_secs` / `set_breathing`).
- Problem: a write that fails partway (sync lock, full disk) leaves part of the preset. Since K2 both apps say "Save failed" and re-apply the setup from before, but that can fail under the same lock, and then the page shows the half-applied setup.
- Fix: nestable transactions (savepoints) in core::db, then `apply` with the length inside one transaction; the apps drop the re-apply. Check what the sync event log records for a rolled-back apply.

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

### #176 Android: a font-size or language change mid-session ends the session but leaves its service running
- Where: `meditate-android/android/app/src/main/AndroidManifest.xml:56` (`configChanges` lacks `fontScale|locale|smallestScreenSize|keyboard|navigation|layoutDirection`); `service::stop` / `guided::stop` / `audio::stop` run only on an Active-to-inactive transition.
- Problem: a font size or language change, split screen or a hardware keyboard recreates the NativeActivity. Startup finalizes the live snapshot as "recovered" (up to 60 s lost) and comes back Idle, while the foreground service, its notification, the wake lock and a guided track keep going. Not yet confirmed on the FP5.
- Fix: add the missing `configChanges`; at startup stop any leftover service and players when no session is active. Manifest: ask first, run the repro probe.

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

### #167 Linux packaging: broken screenshot links and a Rust floor that's too low
- Where: `meditate-gtk/data/io.github.janekbt.Meditate.metainfo.xml.in:27, 31, 35`; `build.sh:72`; `meditate-gtk/Cargo.toml:6`; `clippy.toml` (`msrv`).
- Problem: the screenshot URLs point at `main/data/screenshots/`, which doesn't exist (the files are under `meditate-gtk/data/screenshots/`), so software centres show none. build.sh accepts rustc 1.85, but the locked gtk4 0.11.2 and glib 0.22.5 need 1.92, so the build fails halfway; `rust-version` and `msrv` still say 1.75.
- Fix: add `meditate-gtk/` to the three URLs; raise the check, `rust-version` and `msrv` to 1.92 (Cargo.toml: ask first).

### #171 The `synchronous=NORMAL` comment promises safety it doesn't have
- Where: `meditate-core/src/db/mod.rs:229-241`.
- Problem: "events are append-only and idempotent on re-sync" is only true for events already uploaded; a power cut can roll back a just-saved session that exists nowhere else.
- Fix: correct the comment; or, if wanted, `PRAGMA synchronous=FULL` around Save and Edit only.

### #175 Leftover config
- Where: `meditate-android/manifest.yaml` (xbuild leftover) and the comment at `meditate-android/Cargo.toml:76-80`; `meditate-gtk/src/meson.build:3-9` (`cargo_profile_args` set, never used); `build-aux/io.github.janekbt.Meditate.json:25-26` (two Notifications talk-names, likely surplus under the portal).
- Fix: delete them and check that notifications still show in the Flatpak. Cargo.toml and manifest.yaml are protected build environment: ask first.

## T. Text that doesn't fit

### #159 Text cut off or shortened with "…", found one screenshot at a time
- Rule (Janek, 2026-10-10): every text shows in full at phone width (360 px) in all nine languages. Cut off is not acceptable, and neither is "…": both hide information.
- Seen so far: Android's guided-import dialog title ("Geführte Meditation importi", `main.slint` guided-import card, a 22 px Text with no wrap); GTK's import toast (fixed in batch E with a shorter text); Android's import-question buttons (stacked in batch D); GTK's Type rows squeezing their choices (pinned in batch C).
- Android: Slint clips a Text that neither wraps nor elides, silently. 142 Text elements, 31 wrap, 10 elide. Check 1, a test: every Text showing translated words wraps (`wrap: word-wrap`) and has room to grow; `overflow: elide` is not allowed for them either. An explicit short list is exempt (numbers, the big timer, single symbols).
- Both apps, check 2, a test: slots that can't wrap (buttons, toasts, toggle and tab labels, dialog buttons, header titles, GTK row titles that ellipsize) have every translation measured with the real font at its real size against the slot's width at 360 px, and the test lists each one that doesn't fit. The work is the list of fixed slots and their widths; measuring needs nothing new (Pango in the GTK tests, the same font files for Android).
- Fix what the first run lists: wrap, stack buttons, give the slot room, or shorten the translation; never "…".

---

## V. Randomised sync test

### #162 Sync tests only cover the interleavings someone thought of
- Where: `meditate-core/src/sync/` tests (`FakeWebDav`, `Sync::new`), `db/events.rs` replay.
- Problem: most sync bugs so far (#40, #101 and others in batch A) needed two devices editing in an order no hand-written test tried. `label_name_collision_is_idempotent_on_replay` replayed in the same order on both sides and missed #101.
- Fix: one test in core: two or three in-memory devices share one `FakeWebDav`; a seeded xorshift64 (the one bell jitter already uses, no new crate, so Cargo.lock stays untouched) picks a few hundred random steps: create, rename, delete or merge a label, preset, guided file or vibration pattern, add, edit or delete a session, change a setting, sync one device, compact. At the end every device syncs twice and all must hold identical data (compare sorted rows of each synced table). Run a fixed list of seeds in `cargo test`, and print the seed and the steps on failure so it reproduces. Optional: an `#[ignore]`d variant that runs many seeds for a manual soak.
- Then: a short "sync rules that always hold" section in EVENTS.md (devices converge; a delete at the same or later time wins; same names settle on the smaller uuid), each pointing to its test.
- Every failure it finds is a real bug: fix it in the same batch with its own small regression test.

## W. Sync that stops on one bad item

### #164 A batch this build can't read or apply stops every later pull
- Where: `meditate-core/src/sync/orchestrator.rs:311-312` (an unparseable batch is fatal and never recorded as known); `db/sessions.rs:882, 889-900` writes an unknown `mode` straight into the CHECK column (`schema.rs:151`; the same for bell `category`, interval-bell `kind`, `signal_mode`, `chart_kind`); `db/events.rs:717-749` replays the whole pull in one transaction.
- Problem: since #43 the device still uploads, but it never pulls again. A damaged batch file on the server never heals, and a newer peer's new mode or category blocks every later event from every device until this device updates; the status stays red.
- Fix: decide first whether version skew should keep blocking on purpose. Then: an unparseable batch is logged and recorded as known; a value outside the CHECK set skips that event's recompute (the event stays recorded, as unknown kinds are) so a later build's cache walk applies it. One test per case.

### #165 One audio file that fails to download stops the upload
- Where: `meditate-core/src/sync/orchestrator.rs:757, 912` (`Err(e) => return Err(e.into())`), `500` (WebDAV errors skip push); `db/error.rs:103-115` (no length cap on ids).
- Problem: a flaky link or a server error on one bell or guided file fails the pull, so push never runs and new sessions stay on that device. A uuid longer than 255 bytes makes `<uuid>.<ext>` too long for the file system, so every sync fails at the same file.
- Fix: for a per-file GET or write error, log it, leave the uuid unknown and continue, as `NotFound` already does (keep `Unauthorized` fatal). Cap `target_id` length in `target_id_is_well_formed_for` (e.g. 128). One test each.

### #172 A long Line vibration pattern overflows u32
- Where: `meditate-core/src/vibration.rs:145`; replay `db/vibration_patterns.rs:358` (`as u32`, no clamp); editor cap `meditate-gtk/src/vibration_editor.rs:34` (10 s).
- Problem: above about 463 s, `(2*i+1)*duration_ms` overflows: a panic in debug, a wrong envelope in release. Only a corrupt or hostile synced pattern gets there, because the editors cap the length.
- Fix: compute in u64 and clamp `duration_ms` to the editor's range on replay. One test.

### #173 Server text ends up in the sync status and log; the release-notes parser is fragile
- Where: `meditate-core/src/sync/webdav.rs:97` (whole error body), `:107` (full redirect Location), `:282` etc. (`into_string()`); stored by `sync/settings.rs:134-138`. `release_notes.rs:24, 63-64, 74-77, 91-93`.
- Problem: an error page (up to 10 MB) or a Location with user name and host goes into `last_sync_error`, the tooltip and the shared diagnostics log. The About release-notes parser escapes `<em>`/`<code>`, misses `<p class=...>` and treats `type='development'` as stable; latent while the metainfo stays plain.
- Fix: show the status code only and log a truncated, control-character-stripped excerpt (as `MalformedResponse` does with 512 chars). Parser: a test per markup case when the metainfo next changes.

### #184 Saving an unchanged vibration pattern overwrites another device's edit
- Where: `meditate-core/src/db/vibration_patterns.rs:266-312` (`update_vibration_pattern` always writes and emits).
- Problem: the same as #103 for the other kinds: the phone edits a pattern, the laptop opens the editor on the old version and saves without changing anything; after sync both show the laptop's old version.
- Fix: return early when name, duration, intensities and chart kind equal the stored row. One test.

### #188 A session event that isn't JSON can stop the database from opening
- Where: `meditate-core/src/db/schema.rs` (`ADDED_COLUMNS` fill for `sessions.guided_file_name`), `db/sessions.rs` (`recompute_session`, the named-event lookup), `db/labels.rs:366`; `append_event` stores payloads unchecked.
- Problem: `json_extract` fails on a payload that isn't JSON. In the one-time fill that fails `Database::init` on every open (the fill rolls back and retries). No known writer produces such a payload; found by the second review of f4c6b81.
- Fix: `CASE WHEN json_valid(payload) THEN json_extract(...) END` at all three places; one test with a broken payload in the log.

### #192 GTK: a sync can wait minutes after its push, past the 60 s read limit
- Seen: laptop, 2026-10-14 11:15:55, during batch K's device test (after deleting a guided file). The log ends at `[sync.push] ok events=1 sounds=0 guided=0` with no `sync.done`; the spinner kept turning. The worker thread sat in `recvfrom` on the Nextcloud TLS connection, which had neither sent nor received anything since the push (`ss`: lastsnd/lastrcv equal, over 3 min), although `webdav.rs` sets `timeout_read(60 s)`. The next step after the push is `maybe_compact_events`. At 11:19:50 it ended on its own without an error: `sync.done ... in 235.95s`, so a read waited about 4 min although each read is capped at 60 s.
- Problem: while one sync waits, the in-flight flag stays set and no later sync runs; the user sees a spinner for minutes. If the server never answers, it may never end.
- Fix: first find where it blocks (a body read on a pooled connection, ureq 2's timeouts on reuse); then make every request bounded (`AgentBuilder::timeout` as an overall cap), and test with a fake server that answers the headers and then goes silent.
- Also seen: every pull skips the server file `00000000021265__7b91feb1-….json.tmp` as unparseable (a leftover from an interrupted upload by some device); nothing removes it.

## X. Release pipeline (build environment: ask first)

### #168 A release APK can build without its reproducibility pins
- Where: `meditate-android/android/rust-build.sh:41-50, 76`.
- Problem: a missing platform 34 prints nothing and a missing build-tools 31.0.0 only warns; both exit 0 with a signed APK that F-Droid's check rejects. RUSTFLAGS is a space-joined string, so a repo path with a space breaks the remap flags.
- Fix: for `PROFILE=release`, fail when either pin is missing (debug keeps the warning); use `CARGO_ENCODED_RUSTFLAGS`. Build environment: ask first, then run the repro probe.

### #169 Release CI: unpinned actions and images, and a light run can be picked
- Where: `.github/workflows/flatpak.yml:27-165` (`@v5`/`@v6`, image by tag, `tonistiigi/binfmt` untagged and privileged at 137); `build-aux/attach-flatpaks.sh:24-26`.
- Problem: whoever controls those tags can change what Linux users install. attach-flatpaks takes any successful run for the commit, including a `scope=light` run with no bundles (`flatpak.yml:107, 131`), and then fails at the download without saying why.
- Fix: pin each `uses:` to a commit SHA and each image to `@sha256:`; in attach-flatpaks, pick a run that has both artifacts.

### #174 build.sh fails for Android on a machine without rustup
- Where: `build.sh:167` runs `build-aux/setup-android.sh` before `ensure_rust` (`:171`); `setup-android.sh:300-303` exits when rustup is missing.
- Problem: README promises that build.sh installs missing dependencies, but a fresh machine stops with a rustup error on `./build.sh android`.
- Fix: call `ensure_rust` before the Android bootstrap. README/BUILDING say "install rustup first" until then.

## Y. Slint library fixes (build environment: ask first)

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

## Z. Android touch targets

### #147 Android: small touch targets
- Where: `meditate-android/ui/main.slint:641, 701, 1277, 6353, 1782, 1799`.
- Problem: the mode switch, the Stats period toggle and the Sound/Vibration/Both segments are 32 dp tall and the Box Breath steppers 36 dp; Material asks for 48.
- Fix: taller controls or taller TouchAreas (the PhaseTile row is already full width at 360 dp, so it can only grow taller).

## Later

Ideas and larger improvements, not bugs; the maintainer decides if and when.

### #177 Applying a preset is not atomic
- Where: `meditate-core/src/preset_config.rs:485-` (each write in its own transaction, `db/settings.rs:25`); callers gtk `timer/imp.rs:3498`, android `ui.rs:1288`.
- Problem: a write failing partway (busy database, full disk) leaves settings, interval bells and box-breath phases out of step with the preset.
- Fix: a savepoint helper (`SAVEPOINT` / `RELEASE` / `ROLLBACK TO`, keeps `&self`) and `db.run_in_transaction(|db| apply(...))`.

### #178 GTK: the Done note field should grow instead of scrolling in 180 px
- Where: `meditate-gtk/data/ui/timer_view.blp:836-841`.
- Fix: drop the inner ScrolledWindow and `height-request`, keep `note_view` (the page already scrolls), and check that `log-note-editor` still draws its background. Android already grows.

### #179 Bundled Box Breath voice cues
- Where: `meditate-gtk/data/sounds/` + gresource, `meditate-android/assets/sounds/` + `src/sounds.rs`, `meditate-core/src/db/seeds.rs`.
- Idea: four short CC0/CC-BY cues ("Inhale", "Hold", "Exhale", "Hold") as box-breath seeds, so the phase chooser isn't empty. The seed is one-shot (`seeds.rs:27-29`), so existing installs need a seed-version step.

### #180 Warn about a missing bell sound before Start
- Where: rows already say "Missing" (`meditate-core/src/bells.rs:304`, gtk `bells.rs:384`, android `ui.rs:3224`).
- Idea: a core check over the active bells; a warning style on the row, a Setup banner, and block or confirm Start, so a bell isn't silent mid-session. Both apps; vibration patterns the same.

### #181 Daily goal history
- Idea: a `goal_history(effective_from, minutes)` table instead of the `daily_goal_mins` setting; on change, ask "from today" or "retroactively"; the heatmap and goal ring look up the value per day. Synced through events; needs a migration seeded from the current setting. Both apps.

### #182 Mindfulness bell during the day
- Idea (tentative): a tab that rings at intervals during set hours. Prototype background scheduling first (Android exact alarms; Linux notifications or feedbackd).

### #186 Log label colours ignore the accent colour
- Where: gtk `log/imp.rs:387-400` (`label_color_class`) and the fixed `@log_c0`... colours in the stylesheet (`.log-stripe.log-cN`, line 234-); android `ui/main.slint:4438` (label stripe).
- Idea: the stripe colours follow the system accent (GTK) and the accent the user picked (Android, `apply_accent`), for example tones of the accent instead of a fixed palette. Labels must still be told apart.

### #187 Android: a Local Folder the system hides shows as empty, without a word
- Where: `meditate-android/kotlin/MeditateLocalFolder.kt` (`walk`, an empty children cursor counts as "ok"); `meditate-core/src/db/local_guided_files.rs` (`apply_local_scan` marks every file missing).
- Problem: seen on the FP5 (Android 15) 2026-10-12: the system's ExternalStorageProvider failed its restricted-path check ("NoSuchFileException: /storage/emulated", "Queried directory ... is hidden") and returned no children. Meditate showed "0 files" and marked all files not found; nothing said that Android, not the folder, was the problem. A phone restart fixed it. Stars and plays were kept.
- Idea: when a scan of a folder that had files comes back with none, keep the files as they were and show the "Folder not reachable" banner with Choose Again, instead of marking all missing. A really emptied folder then needs one confirmation.

### #183 Nextcloud with a self-signed or private-CA certificate can't connect
- Where: `meditate-core/Cargo.toml:20` (`ureq = "2"`, built-in roots only), `sync/webdav.rs:176`.
- Idea: also trust the system store or a user-pinned certificate. A new dependency changes Cargo.lock: ask first.

### #190 The day ends at midnight, also for night owls
- Where: every "today" in core (`time::today_local`, goal, streak, heatmap, Log day groups) counts by calendar day.
- Idea: a "day starts at" time (for example 3:00), so a 1 a.m. session counts for the day the user lived. It changes counting in both apps (goal, streak, heatmap, Log groups, synced sessions on another device), probably as a setting. Raised during batch J, 2026-10-13.

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
- Back on the Done screen discarding the session (decided behaviour).
