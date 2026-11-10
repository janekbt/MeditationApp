# Tasks: writes-keep-peer-edits

Base: 16b1e43c22ce3cc73e963cded3f430ce84db0131; suites on it: core 1510, Android 298, GTK 171 green

## Loop
- [x] 0 Skills loaded: `/ponytail ultra`, `slint:slint` (loaded; Android only gets return-type callers)
- [x] 1 Understand (confirmed 2026-11-10 "go"; decisions: Kansho everywhere, #218 pulled in, two changes in a row, #249 not kept)
- [x] 2 Propose (approved: 2026-11-10 "go")
- [x] 3 Build (core 1510+18, GTK 171, Android 298 green; clippy GTK+core all-targets and CI lib clean; Android-target clippy at step 6)
- [x] 4 Review (R1 two agents + /code-review; fix reviews R2, R3, R4 agent + /code-review each), then `/simplify` (see Notes)
- [x] 5 Specs, docs, list (check-specs green; EVENTS, ENTITIES, ARCHITECTURE; OPEN-ISSUES: CC gone, #249 and the two-device limits to "Checked and not kept", #99 closed with test `an_event_an_older_build_skipped_is_applied_when_this_build_returns`; REFACTORING: done items removed, #218 out of P15, notes on #216 #219 #231 #235 #238, new #494)
- [x] 6 Verify and hand over (device test FP5 + laptop, maintainer: "alle tests bestanden", 2026-11-10; laptop first start rebuilt 6338 targets with no noticeable wait)
- [x] 7 Ship (deltas merged into openspec/specs sync, labels, log, bells, vibration; archived; committed after the go, 2026-11-10)

## Review Focus
- A second device -> each "unchanged ... keeps a peer's edit" scenario syncs two databases.
- A replayed event / same event twice -> unchanged: recompute is a pure function of the events; the round-trip test replays twice.
- An old database (26.11.1) -> no marker in `sync_state`: the first start rebuilds every item once (#218's helper); `a_name_comes_back_after_a_round_trip_through_the_last_release` and the existing `a_database_from_the_last_release_gains_the_file_name_columns`, `a_name_received_before_the_update_shows_after_it` and the interval-bell "Bells from older versions keep their times" tests stay green.
- Events table with ids below the marker (database file replaced by a backup) -> marker > max id: full rebuild; test `a_marker_past_the_last_event_rebuilds_everything` in task 6.
- A deleted row -> mutators on an unknown uuid still write nothing and return "unchanged".
- An insert whose name is taken -> still `Duplicate*` before any event (name check first, since the rebuild would suffix instead).
- An empty list -> merge with no presets, rebuild with no events: covered in the tests' setup.
- A preset whose config isn't valid JSON during a merge -> left alone, merge goes on (task 4 test).
- Another language -> no strings change.
- Startup cost -> after the first start only events newer than the marker are rebuilt; the full rebuild runs once per database.

## 1. One write path per table with the unchanged guard (#234, #225, #248, #244, #184, #370)
Per table: a `*_payload(&row)` built from the row struct; a writer that emits it and calls the table's existing `recompute_*` in the same transaction; mutators load the stored row, build the new one, return `false` without writing when they are equal (compared before `updated_iso` is stamped), else stamp and write. Every write that sets a name (insert, rename, an update carrying a name) checks `is_*_name_taken_from_db(name, except_uuid)` first and returns `Duplicate*`, before any event (the rebuild would suffix instead, or rename the holder without an event). The writer sends the name without this row's own `(conflict-<uuid>)` suffix (`strip_conflict_suffix`, task 3.1 comes first). Pattern points compare with the 1e-6 tolerance of `patterns_equivalent` (moved into the guard); preset `config_json` compares as parsed JSON, as text when it doesn't parse. Tables: presets, interval bells, Box Breath phases, vibration patterns, guided files (with `starred_order::write_library_key`), bell sounds. Mutators that returned `()` return `Result<bool>` (changed).
- [x] 1.1 Presets, with an `_in(tx, ..)` writer for task 4. Proof: `an_unchanged_override_keeps_a_peers_rename` (stored config written with another key order), `starring_a_starred_preset_sends_nothing`, `a_suffixed_preset_starred_here_still_heals`; the existing duplicate-name tests of every table stay green. Expected FAIL then PASS.
- [x] 1.2 Box Breath phases. Proof: `an_unchanged_phase_write_keeps_a_peers_edit`.
- [x] 1.3 Interval bells (update, enabled, copy). Proof: `an_unchanged_bell_write_sends_nothing`.
- [x] 1.4 Vibration patterns; `update_vibration_pattern` returns `bool`, `rename_vibration_pattern` reuses it. Proof: `an_unchanged_pattern_save_keeps_a_peers_edit`, `saving_an_unchanged_pattern_reports_no_change`.
- [x] 1.5 Guided files and their star key. Proof: `starring_a_starred_guided_file_sends_nothing`.
- [x] 1.6 Bell sounds (insert, rename through the shared writer; rename's guard exists).
- [x] 1.7 Sessions: one `session_payload` for the three copies (#214 part of #234); `update_session_in` skips an unchanged row. Session writes keep their own SQL (no rebuild per row: imports). Proof: `an_unchanged_session_edit_sends_nothing`.
- [x] 1.8 Round trip for every kind. Proof: `every_local_write_is_stored_as_a_peer_stores_it` (events.rs). Must pass after 1.1-1.7; written first and FAIL on at least one table (default drift) or noted.

## 2. Undo only after a change (#370)
- [x] 2.1 GTK: the editor's saved callback carries `changed`; `vibrations.rs` drops the read-back. `patterns_equivalent` stays in core only as the guard's comparison. Proof: MANUAL scenario.
- 2.2 dropped: Android has no Undo after a pattern edit (pre-build question 4).

## 3. Name conflicts settled once (#241, #230)
- [x] 3.1 `strip_conflict_suffix(name, uuid)` next to `conflict_suffixed_name`; `labels.rs` and `reclaim_suffixed_names` use it, no `LIKE` pattern of the format.
- [x] 3.2 `reclaim_suffixed_names` skips a row whose base name is held by a row with a smaller uuid. Proof: `a_permanent_name_conflict_is_not_settled_again`; `a_name_freed_in_the_same_pull_goes_to_the_new_row` and `a_pair_suffixed_the_old_way_heals_on_the_next_pull` stay green.

## 4. Merge moves presets (#243)
- [x] 4.1 `merge_labels` rewrites `label.uuid` in each preset config that names the merged-away label, through the preset writer, in the merge transaction; an unreadable config is left as is. Proof: `a_merge_moves_presets_to_the_kept_label`.

## 5. Clamp a peer's length (#242)
- [x] 5.1 `recompute_session` reads the length as `i64` or `f64` and clamps as `Session::from_unix`. Proof: `a_peers_damaged_length_is_clamped`.

## 6. Rebuild what an older release applied (#226, #218, #245)
- [x] 6.1 #218: the dedup-and-recompute half of `replay_events` becomes a helper (non-session first, sessions second); the cache upgrade collects distinct `(kind, target_id)` from `events` and calls it, without re-appending. The collection skips what `apply_event_record_only` skips: unknown kinds and target ids that fail `target_id_is_well_formed_for`. Proof: test `a_rebuild_skips_an_event_with_a_bad_target_id`. Proof: existing cache-upgrade tests stay green.
- [x] 6.2 #226: `sync_state` key `last_applied_event_id`, moved forward by this build whenever it appends an event (in the append, same transaction), so only events an older build appended stay above it. On open, rebuild the targets of events with a larger id (through 6.1's filter; all of them when the key is missing or above the largest id), then store the largest id. Delete `recompute_interval_bells_from_events` and the `guided_file_name` fill in `schema.rs`. Proof: `a_name_comes_back_after_a_round_trip_through_the_last_release`, `a_marker_past_the_last_event_rebuilds_everything`.

## 7. One name for the Kansho bell (#444)
- [x] 7.1 GTK's seed row says "Kansho"; `recompute_bell_sound` maps exactly "Kanshō" on uuid ...0007 to "Kansho" (changed after review R1 from a sent rename); old databases get it from the first-start rebuild. Proof: `the_bundled_kansho_bell_drops_its_macron_once`, `a_user_name_on_the_kansho_bell_stays`.

## 8. Docs and lists
- [x] 8.1 EVENTS.md, ARCHITECTURE.md; OPEN-ISSUES: CC items removed, #249 to "Checked and not kept" (maintainer: too unlikely for a new column), decision #444 removed; REFACTORING: #218 and #230 into P1, done items removed.

## Notes
- 1.8 was written after 1.1-1.7 (not first, as planned) and passed at once, as the shared writers build local rows by the rebuild. Proved it can fail: dropping `guided_file_name` from the session payload makes it FAIL; restored.
- 1.x: mutators kept their `Result<()>` except `update_vibration_pattern` (now `Result<bool>`, the only caller that needs it); "return bool" in task 1 narrowed to that (my call, smallest change).
- Pre-build questions (fresh agent, 2026-11-10), all answered from the code or as my call: 1 name checks on every naming write; 2 rebuild uses the record-time filter; 3 marker moves on every append; 4 Android pattern Undo doesn't exist, 2.2 dropped; 5 tolerance kept; 6 parsed JSON compare; 7 patch the raw JSON in the merge via an `_in` writer; 8 writer strips its own suffix; 9 rename in `seed_all_non_audio`; 10 sessions without rebuild-on-write.
- Review R1 (data/sync agent, completeness agent, /code-review high), 2026-11-10:
  - Fixed F1: Kansho rename sent an event that could undo a peer's rename (all three) -> mapped in `recompute_bell_sound`, no event; also ends the "user picks Kanshō, renamed on every launch" loop (/code-review 2).
  - Fixed F2: cache upgrade duplicated the rebuild (/code-review 4) -> it now resets the marker and lets `rebuild_events_from_older_builds` run.
  - Fixed F3: `edit_preset` committed only on change, `edit_guided_file` always (/code-review 9) -> both always commit.
  - Fixed F4: name-check tests for guided files and patterns (data agent 5): `renaming_a_guided_file_to_a_taken_name_sends_nothing`, `renaming_a_pattern_to_a_taken_name_sends_nothing`.
  - Fixed: stale `patterns_equivalent` doc, double blank line in GTK vibrations.rs.
  - Spec text aligned with the tests: the round-trip scenario is the new-session case through 26.10.1 (the real #245 path; an older build's edit leaves the column it doesn't know alone); the conflict scenario asserts "not written again" (the diag log can't be read in tests).
  - Kept, maintainer 2026-11-10: merge rewrites presets with full rows; concurrent edits on two devices may lose a preset edit (both agents). One user, one device at a time; recorded in OPEN-ISSUES "Checked and not kept".
  - Not fixed: marker upsert per `emit_event` (/code-review 3): one row, inside the caller's transaction, one fsync at commit; the data agent set it aside too.
  - Not fixed: `reclaim_suffixed_names` reads the four library tables per pull (/code-review 7): tens of rows; the LIKE prefilter would spell the suffix format again (#230).
  - Not fixed: `target_id_is_well_formed_for` runs twice for a rejected event (/code-review 8): a string check; `dispatchable` stays the one decision, the second call only logs.
  - Deferred to part 2: `session_by_id` is a fourth session row mapper (/code-review 5) -> REFACTORING #216; interval bell and bell sound edits list the table to find one row (/code-review 6) -> #235.
  - Not fixed: a failing recompute in the start-up rebuild stops the database from opening (data agent 3): the cache-upgrade walk had the same property; no path reaches it today.
  - Not fixed: merge test checks the preset's label, not a saved session (completeness agent 3): applying a preset is covered by preset_config tests; the label field is the whole change.
  - Not fixed: GTK Undo after a save that a sync made "changed" restores the same values (completeness agent 7): harmless.
- Fix review R2 (fresh agent on the R1 fixes), 2026-11-10:
  - Fixed F5: a "Kanshō" row without events (after Wipe Local) was never rebuilt -> `rename_unrebuilt_kansho` at open, no event. Test `a_kansho_bell_without_events_drops_its_macron_too` (written with the code; FAILS with the call removed).
  - Fixed F6: renaming the bundled bell to "Kanshō" sent an event the rebuild then undid -> `bundled_name` maps it before the guard. Test `typing_kansho_with_its_macron_sends_nothing` (FAILS with the mapping removed).
  - Fixed: the duplicate-name scenario split per kind, each with a test that asserts its THEN.
- Fix review R2 (/code-review high on the whole change), 2026-11-10:
  - Same as F5/F6 (findings 1, 2): fixed above.
  - Fixed F7: peers' lengths in guided files, patterns and interval bells were still cut, not clamped (finding 3) -> one `peer_u32` for every recompute. Test `a_peers_damaged_file_length_is_clamped` (FAILED before).
  - Fixed F8: a session edit without a file name cleared it here while peers kept it (finding 4) -> the edit keeps the stored name; `update_session_in` reads the row once. Test `an_edit_without_a_file_name_keeps_it_here_as_on_a_peer` (FAILED before).
  - Fixed F9: interval-bell insert lost its existing-uuid check (finding 5) -> early return as in the other tables. Test `copying_a_bell_twice_under_one_uuid_writes_it_once` (FAILED before).
  - Fixed F10: the cache-upgrade check folded into `rebuild_events_from_older_builds`; `maybe_walk_events_for_cache_upgrade` deleted (finding 7).
  - Fixed: presets tests use the shared `exchange` (finding 8); both shells name the Kansho uuid by core's constant (finding 9).
  - Not fixed: marker per emit (finding 6), as in R1.
  - Not fixed: `reclaim_suffixed_names` snapshot (finding 10): result is right; a three-way same-name clash costs one extra write on that pull.
- Fix review R3 (fresh agent and /code-review high on the R2 fixes), 2026-11-10:
  - Fixed F11: a bell's seconds fields still dropped a fractional or too-large value (fell back to whole minutes) -> through `peer_u32`. Test `a_peers_damaged_bell_times_are_clamped` (FAILED before); `a_peers_damaged_pattern_length_is_clamped` added (passed at once: F7 already covered it; `as_u64` on 1200.0 gives 0, so the old code fails it). Requirement widened to every kind.
  - Fixed: `typing_kansho_with_its_macron_sends_nothing` asserts the name too; Kansho names are constants shared by the mapping and the start-up UPDATE; `edit_preset_in` returns `()` like its guided twin; new tests use `exchange`; stale proposal decision text.
  - Measured (/code-review 3, the first-start full rebuild): 10k guided sessions, 9998 events, release build on the laptop: first open 0.62 s, next open 0.013 s. Once per database; the phones are checked in the device test (first start after the update).
  - Not fixed: start_iso compared as text in the session guard (/code-review 1): only a start stored in another text form, and only harmful with a concurrent edit on another device (maintainer: very unlikely).
  - Deferred to part 2 (#235): table scans in interval-bell and bell-sound edits (/code-review 2), as in R1.
  - Not fixed: marker per emit (4), MAX(id) read twice per rebuild and the double target check (9): trivial.
  - Docs naming deleted functions (agent 3): step 5.
- Fix review R4 (fresh agent: no findings; /code-review high), 2026-11-10:
  - Fixed: the start-up rebuild logs only when it rebuilt something (finding 7).
  - Not fixed: a new name equal to the base of an existing "(conflict-...)" row passes the name check and may be suffixed later without an event (finding 1). Needs a conflict that only two devices creating the same name before a sync make; maintainer 2026-11-10: such cases only when almost free, and the check would have to spell the suffix format again. Recorded in OPEN-ISSUES "Checked and not kept".
  - Not fixed: the Kansho UPDATE at every open (finding 5): one indexed statement that writes nothing once fixed; a gate would need its own flag and still miss Wipe Local.
  - Not fixed, as before: start_iso text compare (2), marker per emit (3), library scan per pull (4), table scans (6, deferred to #235).
- /simplify (4 agents), 2026-11-10:
  - Applied: one `emit_and_rebuild_in` (emit, strip own suffix, `recompute_for`) under every table's writer; the start-up rebuild inlined, its `MAX(id)` read once; `dispatchable` logs the rejected target (one check); `edit_preset_in` compares by copying the stored text when the JSON matches; `retarget_label` moved to `preset_config.rs`; `stored_time` takes `u32`; session edit uses the row's uuid; Kansho consts above their use, one `impl` block; test helpers `sent`/`push`/`exchange` shared (two `pending` and two `swap_events` copies deleted), seed constants in tests, stale test name and doc text.
  - Deferred to part 2: one insert/edit-and-guard shape per table on keyed readers (#235), `session_by_id` into the shared mapper (#216), re-reads of rows the caller holds (#235), `reclaim_suffixed_names` relying on no-op upserts instead of its own rule (#238), the remaining inline event-swap copies in tests (#231). Added to REFACTORING as a new item: a composite index `events(target_id, lamport_ts, device_id)` for `winning_mutate`.
  - Skipped: Android ignores the new "changed" bool (no Undo there, decided).
  - Suites after it: core 1520+18, GTK 171, Android 298; clippy GTK+core all-targets and CI lib clean.

## Hand-over (step 6)
- Verified 2026-11-10: core 1520 + 18 integration, GTK 171, Android 298 green; clippy GTK+core all-targets, CI lib (-D warnings) and Android target clean (only slint build-script notes).
- Artifacts (13:33, new code checked by string in the binary): `meditate-android/android/app/build/outputs/apk/debug/app-debug.apk` (debug), `meditate-gtk/builddir/src/meditate`.
- Lines of production code (tests excluded): 9314 before, 9061 after, net -253.
- Device test steps:
  1. Phone: `adb install -r` the debug APK over the installed app (no running session). First start after the update rebuilds the cache once: note whether it opens noticeably slower; the next start must be normal. Log, Stats, presets, bells, patterns, guided files look as before.
  2. Laptop: start the new build. Bells chooser: the bundled bell reads "Kansho" (no macron). Sync: the phone shows "Kansho" too.
  3. Laptop: Vibrations, open a pattern, Save without a change: no Undo toast. Open it again, move one point, Save: Undo toast; Undo brings the old curve back.
  4. Phone: rename a preset, sync. Laptop: sync, then Override that preset in Save mode without changing anything, sync. Phone after its sync: still the new name.
  5. Optional, if a duplicate label exists: a preset pinned to the "(conflict-...)" label, Merge: apply the preset, the session gets the kept label.
- Proposed commit message (after the go):

  core: send nothing for unchanged saves, build local rows as peers do

  Saving an item with the values it already had sent a full copy with
  a newer clock and undid an edit another device made meanwhile. Every
  synced table now writes through one path that emits the event and
  rebuilds the row from the log, and skips writes that change nothing.

  Also: a settled name conflict is no longer redone on every sync;
  peers' lengths are clamped; a label merge moves its presets; rows an
  older build applied are rebuilt at start (replaces the interval-bell
  pass and the guided-name fill); the bundled bell is "Kansho" in both
  apps; Linux offers Undo after a pattern Save only when it changed.

  OPEN-ISSUES CC (#248 #244 #241 #242 #243 #245 #444 #184) and #99;
  REFACTORING #234 #225 #370 #226 #218 #230.
