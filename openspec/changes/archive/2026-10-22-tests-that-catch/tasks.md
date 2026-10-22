# Tasks: Tests that catch the bug

Base: 1b60232 (beta); suites on it: core 1406 green, android 254 green, gtk 162 green at 1b60232 (not re-run, GTK untouched)

## Loop
- [x] 0 Skills loaded, again after every /compact: `/ponytail ultra`, `slint:slint` (ui.rs glue touched, no .slint)
- [x] 1 Understand
- [x] 2 Propose (approved: 2026-10-22)
- [x] 3 Build
- [x] 4 Review (done, fixes below), then `/simplify` after the last review
- [x] 5 Specs, docs, list (check-specs green; OPEN-ISSUES batch R ticked; fake.rs header fixed)
- [ ] 6 Verify and hand over
- [x] 7 Ship

## Review Focus
- A test that is green with or without the code it guards -> every task names the mutation that must turn it red; run it, restore.
- Strict server: MKCOL of the base folder on a fresh server, a second device's first sync on folders it didn't create -> covered by the two-round test plus the peer's pull.
- Local time zone of the test machine for Log grouping -> start times are naive local text, read back in the same zone, so the tests hold in any zone.
- Empty Log, a deleted label (label id without a name) -> deleted label in `log_card_shows_label_colour_and_guided_name`; empty input returns no days (assert inside `log_groups_sessions_by_start_day`).
- A JNI call whose method name is a variable (`invoke(app, "start")`) -> the table lists the name; the test finds the name literal in the Rust file.
- A Kotlin parameter that is nullable (`String?`) -> same JNI type as `String`.

## 1. #91 Sync tests (core)
- [x] 1.1 Fix `prepare_push_local_recovery_flags_all_events_unsynced`: mark every event synced first, then assert all pending after recovery. Files: `meditate-core/src/sync/settings.rs`. Proof: mutation (drop `flag_all_events_unsynced` from `prepare_push_local_recovery`) FAIL, restore PASS.
- [x] 1.2 Replace the delete loop in `sync_does_not_trigger_remote_data_lost_after_an_explicit_wipe_known_remote_files` with `prepare_push_local_recovery`, as new test `push_local_recovery_reuploads_every_old_session`: sync, wipe remote events, recovery, sync, fresh peer pulls and has the session. It replaces the old test and also asserts the second sync raises no "data lost". Files: `orchestrator.rs`. Proof: same mutation FAIL, restore PASS.
- [x] 1.3 `pull_drops_a_sound_just_over_the_cap` replaces `pull_drops_files_over_10mb_cap`: push a small sound, overwrite its remote file with 10 MB + 1 byte via `fs.put`, peer pulls. Proof: mutation (drop the `MAX_CUSTOM_BELL_BYTES` check in pull) FAIL, restore PASS.
- [x] 1.4 `pull_skips_a_sound_too_large_to_download`: same with 12 MB. Proof: mutation (`ResponseTooLarge => continue` to `return Err`) FAIL, restore PASS.
- [x] 1.5 Test-only `StrictWebDav` wrapper in the orchestrator tests (folders tracked; MKCOL on existing -> `Conflict`; MKCOL or PUT with missing parent -> `Conflict`; list of a missing folder -> `NotFound`), and `two_syncs_with_audio_files_work_against_a_strict_server`. Proof: mutation (drop `| Err(WebDavError::Conflict)` in `ensure_sounds_dir_exists`, then in `ensure_guided_dir_exists`, then in `ensure_events_dir_exists`) FAIL each, restore PASS.

## 2. #138 Android Log and dot helpers
- [x] 2.1 Tests in `app.rs` against `app::group_log_sessions(rows, labels, places, guided, hidden, &ClockFormat)` (stub first so they compile): `log_groups_sessions_by_start_day`, `log_card_time_follows_the_clock_format`, `log_hides_a_session_waiting_for_undo`, `log_card_shows_label_colour_and_guided_name`, plus `long_notes_are_shortened_for_the_card` (119 chars + "…", short note unchanged, no scenario, see Notes). Expected: FAIL on the stub.
- [x] 2.2 Move `group_log_sessions`, `LogDaySectionData`, `LogCardItemData`, `truncate_note_for_card` to `app.rs` (pub, unchanged bodies); `format_time_of_day` takes the clock format; `ui.rs` callers pass `CLOCK_FORMAT`'s value. Delete the `group_log_sessions` source-text assertions in `app.rs` (the guided-name part of the guided Log test, `log_days_cards_and_headers_agree`, and the day-total line in `stats_come_from_core_and_the_streak_line_is_set`, now proven by the 10:20 card in `log_groups_sessions_by_start_day`). Proof: 2.1 PASS; mutation `last.count += 2` FAIL, restore.
- [x] 2.3 `breath_dot_path_has_no_jumps`, then move `rounded_perimeter_point` to `app.rs`. Values by hand for pad 10, side 100, radius 20: In t=0 -> (10, 90); In t=1 -> (30, 10); HoldIn t=1 -> (110, 30); Out t=1 -> (90, 110); HoldOut t=1 -> (10, 90); end of each phase equals start of the next; In at the arc's midpoint on the circle of radius 20 around (30, 30). Proof: FAIL on stub, PASS after move; mutation (swap two corner centres) FAIL.
- [x] 2.4 Android-target clippy clean (BUILDING.md), since `ui.rs` builds only for Android.

## 3. #150 JNI table
- [x] 3.1 `every_jni_call_matches_its_kotlin_method` in `app.rs`: table rows (Rust file, class constant, Kotlin file, method, signature). Per row: the Rust file holds the signature literal and the method literal; the Kotlin file holds `package io.github.janekbt.Meditate`, `object <Class>` or `class <Class>` with a `companion object`, matching the constant, and `@JvmStatic` before `fun <method>(` whose parameter and return types translate to the signature (Context, Activity, String, String?, Int, Long, Boolean, FloatArray, LongArray, IntArray, Array<String>, no return -> V). Completeness: every signature literal passed to `call_static_method` in `src/*.rs` (outside tests) appears in a row for that file. Proof: each mutation FAILs, then restore: in `MeditateHaptics.kt` change `vibrateWaveform`'s `IntArray` to `LongArray`; rename one Kotlin method; add a signature literal to a Rust `call_static_method` that no row lists.

## Notes
- The note shortening keeps its "…" (batch T, #159); tested, not spec'd, so the spec doesn't fix it in place (my call).
- #150 has no spec delta, it guards a mechanism (my call).
- Blind-builder questions, all decided (my call):
  1. `format_time_of_day` moves to `app.rs` too, taking `&ClockFormat`; struct fields `pub`.
  2. `log_groups_by_start_instant_not_stored_text` also checks the card time against the start instant (stored text with a -13:00 offset), so the dropped source-text check keeps its coverage.
  3. JNI completeness also covers method names: every method literal passed to `call_static_method` or to a file's `invoke*`/`call` helper has a row.
  4. A Kotlin class is `object X` or `class X` with a `companion object` (the service).
  5. The Kotlin reader skips comment and blank lines after `@JvmStatic`, reads parameters across lines with trailing commas, and treats `T?` like `T` in parameters and returns (`: String = try`, `): Boolean = synchronized` count as returns).
  6. Older JNI pin assertions in `app.rs` stay; mutation proofs run the new test with `--exact`.
  7. `StrictWebDav` normalizes paths itself, treats the root as existing, and refuses a MOVE into a missing folder too.
  8. `sync_does_not_trigger_remote_data_lost_after_an_explicit_wipe_known_remote_files` is replaced by `push_local_recovery_reuploads_every_old_session`, which also asserts no "data lost" on the second sync.
- Built: `log_groups_by_start_instant_not_stored_text` dropped (my call, told Janek at hand-over). Sessions are stored as naive local time and the start instant is read from that text in the current zone, so text and instant can't name different days; the scenario can't be built. Replaced by `log_card_time_follows_the_clock_format` (24 h and 12 h card times). Blind-builder answer 2 is void with it.
- 3.1 built as two tests: `every_jni_call_matches_its_kotlin_method` (the rows) and `every_kotlin_call_is_in_the_jni_table` (completeness); both passed at once (the calls are right today); mutations each red: Kotlin param retyped, method renamed, `@JvmStatic` dropped, return retyped, unlisted Rust signature, unlisted method through a helper.
- Every test in 1.x and 2.x: red on its mutation, then restored (files compared with the backup).
- Review 1 findings, all fixed (test-only, so no second review):
  1. JNI table checked literals per file, not per call: `the_jni_table_lists_exactly_the_rust_calls` (replaces `every_kotlin_call_is_in_the_jni_table`) extracts each (class constant, method, signature) from the code, helpers resolved through their callers and the argument count, and compares with the table. Red on: `notify_complete` through the plain helper, `start_local_scan` loading the player class, an unlisted signature, an unlisted helper method.
  2. The 12 MB test didn't prove the download was cut off: a counting wrapper (`CountsTooLarge`) asserts one refused GET, and the peer runs `sync()`. Red on `MAX_SOUND_GET_BYTES = u64::MAX`.
  3. The recovery test wiped only events: now the whole folder, with a custom sound and a guided file the peer must get. Red on dropping `wipe_known_remote_sounds` or `wipe_known_remote_guided_files`.
  4. "Tried again later" wasn't proven: the just-over test replaces the file with a small one and syncs again. Red on marking an oversized sound known.
  5. Blank lines left from the move: removed.
- /simplify applied: `StrictWebDav` uses `fake::norm` (now `pub(crate)`) instead of a copy; the `FakeWebDav` header no longer claims Nextcloud's folder behaviour; `ACTIVITY` const beside `CTX`; the `group` test helper takes the clock; `truncate_note_for_card` private again; `format_time_of_day` inlined (one caller).
- /simplify skipped:
  - derive the JNI table from the Rust parser: the hand table is the expected value; derived, a parser miss would check nothing;
  - per-call consts in each bridge, one `ensure_dir` for the three MKCOL copies, grouping in core for both apps: production refactors beyond this test batch (offered to Janek);
  - strict `FakeWebDav` for every sync test: changes ~30 tests' setup, beyond scope (offered);
  - shared forwarding wrapper for the 11 WebDav test wrappers, a shared `source()` helper across app.rs/alarm_volume.rs: older duplication outside the diff;
  - caching file reads in the JNI test: milliseconds.

## Hand-over
- Suites: core 1408 (1390 + 4 + 11 + 3) green, android 261 green; clippy core+android `--all-targets -D warnings` clean; Android-target clippy clean; check-specs green. GTK untouched, not rebuilt.
- APK: `meditate-android/android/app/build/outputs/apk/debug/app-debug.apk` (debug, 2026-10-22 14:22).
- Device test (FP5), Android only; nothing user-visible should change:
  1. Log: days show the right count and minutes; a card with a label shows its colour, a guided session its file name; times as before.
  2. Delete a session: card gone, day caption drops it; Undo brings it back.
  3. Box Breath: the dot runs around the rounded square without jumps at the corners or phase changes.
  4. A Timer session with a bell and vibration: bell plays, phone vibrates, notification appears; pause/resume; the end notification shows.
  5. Guided session plays its file; Settings, sync account still shows connected (keychain); About shows the version.
- Commit message (proposed):
  ```
  tests: make sync, Log and JNI tests catch their bug (batch R)

  Sync tests ran against a fake server kinder than Nextcloud and stayed
  green with the guarded code removed: the push-my-data recovery, the
  pull size cap and the folder handling now fail when their code breaks.
  Android's Log grouping, note shortening and Box Breath dot path move
  from ui.rs to app.rs and get real tests. One table pins every Rust
  call into Kotlin to its @JvmStatic method, class and signature.
  ```
