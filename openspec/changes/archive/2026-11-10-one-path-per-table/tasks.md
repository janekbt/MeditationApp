# Tasks: one-path-per-table

Base: ab256a9ffdc9bc5b746d92feb5bd725bc1c538ac; suites on it: core 1520 + 18 green (re-run 2026-11-10), GTK 171 and Android 298 green at this commit (device-test run before its commit)

## Loop
- [x] 0 Skills loaded: `/ponytail ultra`, `slint:slint` (loaded; no Android UI touched)
- [x] 1 Understand (confirmed 2026-11-10 "Go"; decision: K3 separately, after this one)
- [x] 2 Propose (approved: 2026-11-10 "Go")
- [x] 3 Build (core 1521+18, GTK 171, Android 298 green; clippy GTK+core all-targets and CI lib clean; Android-target clippy at step 6)
- [x] 4 Review (R1 reviewer agent + /code-review high; fixes need no second round, see Notes), then `/simplify` (see Notes)
- [x] 5 Specs, docs, list (check-specs green; no spec deltas; REFACTORING: six items out, P1 ticked, #495 #496 added, #239 #232 trimmed, cross-refs updated; OPEN-ISSUES: K3 #193 and #306 notes)
- [x] 6 Verify and hand over (device test FP5 + laptop, maintainer: "alles andere laeuft", 2026-11-10; one pre-existing Android bug found, OPEN-ISSUES #497)
- [x] 7 Ship (no deltas, skip_specs; archived 2026-11-10; committed after the go)

## Review Focus
- Behaviour drift hidden in a merged copy (an `expect` message, a default, an ORDER BY, `unwrap_or_default` vs error on a bad intensities JSON) -> each reader keeps the stricter of today's copies only where they already agree; where they differ, the list path's behaviour wins and the note says which.
- A deleted row -> every edit and delete on an unknown uuid still writes nothing and sends no event (existing `*_unknown_uuid_is_silent_noop` tests, plus pins below).
- A replayed event / old database -> no rebuild code changes except the shared `delete_row`; the round trip `every_local_write_is_stored_as_a_peer_stores_it` stays green.
- An import with a label deleted meanwhile -> after #215 the bulk path saves it as none; pinned by 3.1.
- An empty list -> `fresh_keys(0)` returns nothing; seed_once on a seeded database does nothing (existing seed idempotence tests).
- A second device, another language -> no event, payload or text changes.
- Speed -> keyed lookups replace whole-list scans; `insert_imported` keeps one prepared statement for many rows.

## 1. Pins first (tests for touched functions that have none)
- [x] 1.1 `delete_session_by_uuid`: removes the row and sends one tombstone; an unknown uuid sends nothing. Files: `db/sessions.rs`. Proof: `deleting_a_session_by_its_uuid_sends_one_tombstone`, `deleting_an_unknown_session_uuid_sends_nothing`. Expected: PASS at once (pin), noted.
- [x] 1.2 Known-remote guided files: record, list, wipe. Files: `db/known_remote.rs`. Proof: `known_remote_guided_files_are_recorded_and_wiped` (PASS at once, pin; folded by /simplify into `each_tracker_records_once_lists_and_wipes_only_itself`, which loops all three trackers).
- [x] 1.3 Before each later task: grep the tests of every function it touches; a function with none gets a pin here first (listed in Notes).

## 2. One row reader per table (#235, #216)
- [x] 2.1 Sessions: `SESSION_COLUMNS` + `session_from_row` for `session_by_id`, `get_longest_session_from_db`, `query_sessions_from_db`, `list_sessions_filtered`; `list_sessions_filtered` becomes `query_sessions_from_db` with a label filter where its ORDER BY allows (else stays a thin caller). Proof: existing session tests green.
- [x] 2.2 Events: `event_from_row` for `all_events`, `pending_events` and the third copy. Proof: existing event tests green.
- [x] 2.3 Bell sounds, vibration patterns, guided files, Box Breath phases, interval bells: `COLUMNS` + `from_row`; list and keyed lookups share it; `find_interval_bell_by_id` and a bell-sound lookup become keyed queries. Proof: existing tests per table green.

## 3. One insert path for sessions (#215)
- [x] 3.1 Pin: a session whose label is gone saves without one, through the single path that will now run on the shared insert. Proof: existing `a_session_on_a_deleted_label_saves_without_it`. (Changed while building: no production path reaches the bulk path's raw label id, since `insert_imported` resolves labels first and `bulk_insert_sessions` goes; a FAIL-first test would need the function being deleted.)
- [x] 3.2 `insert_session_tx_less` calls `insert_sessions_in` with one row; delete `bulk_insert_sessions` and the GTK wrapper; move its tests (and the one in `local_changes.rs`) onto `insert_imported`, drop duplicates. Proof: suites green, `grep bulk_insert_sessions` empty.

## 4. One edit shape and one insert shape per table (#235 note)
- [x] 4.1 `edit_*(uuid, FnOnce(&mut Row))` for presets (exists), guided files (exists), vibration patterns, interval bells; `update_vibration_pattern`, `rename_vibration_pattern`, `update_bell_in`, `set_interval_bell_enabled` go through it. Bell sounds (`rename_bell_sound`) and Box Breath (`set_box_breath_phase`) have one edit each and keep build row, compare, write (see Notes). Proof: every `unchanged ... sends nothing` test and the duplicate-name tests stay green.
- [x] 4.2 Inserts: one `insert_*_at(row)` shape (existing uuid returns its rowid, name check, write, rowid) shared by the tables that have one. Proof: `*_existing_uuid_is_silent_noop` and duplicate-name tests green.

## 5. Delete paths (#237, #217)
- [x] 5.1 `delete_synced_row(table, uuid, kind)` and `delete_row(table, uuid)`; every table's delete and every rebuild's no-winner branch use them. Proof: delete and tombstone tests per table green.
- [x] 5.2 Sessions: `delete_session` -> uuid lookup -> `delete_session_by_uuid`; `delete_all_sessions` with the same tombstone; `existing_rowid_by_uuid` and the shared label lookup in `recompute_session`. Proof: 1.1 and existing delete tests green.

## 6. Repeated helpers (#231)
- [x] 6.1 `order_key::fresh_keys(n)` for presets and starred order. Proof: `fresh_keys_are_ascending_and_one_per_item`; existing order tests green.
- [x] 6.2 known_remote: one record/list/wipe per table parameter. Proof: 1.2 and existing tests green.
- [x] 6.3 `seed_once(key, body)` for the four seeds. Proof: existing seed idempotence tests green.
- [x] 6.4 One label uuid-by-id / id-by-uuid helper (labels, sessions; session_in_progress has these lookups only in tests); `write_kv` in settings; `existing_rowid_by_uuid` in presets; `mint_uuid()` in labels and device; `.optional()` in `mod.rs`; Box Breath phase names in `error.rs` from `BoxBreathPhaseId`. Proof: suites green.
- [x] 6.5 Tests: the inline event swaps in `settings.rs`, `local_changes.rs`, `preset_config.rs` use `sent`/`push`/`exchange`. Proof: those tests green.

## 7. Wrap up
- [x] 7.1 (suites and clippy at steps 3 and 4; Android-target clippy clean at step 6) Clippy (GTK + core all-targets, CI lib `-D warnings`, Android target); GTK and Android suites. LOC count against the base (`loc.py ab256a9`) into Notes.

## Notes
- 1.1, 1.2 passed at once (pins); each failed with the production code broken (tombstone not sent; guided uuid recorded in the sounds table), then restored.
- 2.x: a shared `Database::query_rows(sql, params, from_row)` (prepare_cached) in `mod.rs` holds the list boilerplate; presets' four copies of the reader were folded in too (#225 had left them; #235 says do all readers together). Box Breath's list reads the four phases by key (four cached lookups instead of one scan plus a map). `read_kv` uses `.optional()` (#231 item done here). Preset list doc now says box_breath sorts first (#232 doc item).
- 3.2: three bulk tests dropped as duplicates of `an_import_lands_whole_or_not_at_all` (rollback, atomic) or merged (empty slice: count and events in one); the rest run through `insert_imported`. Core lib tests 1523 -> 1520.
- 4.1: Box Breath keeps `set_box_breath_phase` (build row, compare, write) with no closure: no caller edits one field, and K3 #325 is where one would. Bell sounds: rename builds the row with `bundled_name`, compares the whole row. Interval bells: `edit_bell_in(tx, uuid, FnOnce)`, `update_bell_in` and `set_interval_bell_enabled` use it; keyed `find_interval_bell`.
- 4.2: `Database::insert_new_in(table, uuid, write)` (existing uuid returns its rowid, else write, then rowid) for presets, guided files, vibration patterns, bell sounds, interval bells; the name check stays inside each `write` closure, before the event.
- 5: `delete_synced_row(table, uuid, kind)` (DELETE, nothing more when 0 rows), `delete_row(table, uuid)` for the six rebuild no-winner branches and labels' delete, `emit_tombstone`; `delete_session` -> `session_by_id` -> `delete_session_by_uuid`; `delete_all_sessions` uses `DELETE ... RETURNING uuid`.
- 6: `fresh_keys` FAILED with a stub first, then PASS. `seed_once(key, tag, seed)`; `Known::{Files, Sounds, GuidedFiles}` with `known_remote` / `record_known_remote` / `wipe_known_remote` (nine functions became three; callers only in core); `label_uuid_by_id` shared by labels and sessions, `existing_rowid_by_uuid("labels", ..)` in `recompute_session`; `recompute_setting` writes through `write_kv`; `mint_uuid()` in labels and device; `error.rs` uses `EventKind::BoxBreathPhaseUpdate` and `BoxBreathPhaseId::from_db_str`. The presets `existing_rowid_by_uuid` item was already done in the previous change.
- Not done, for K3: a transaction helper (`in_tx(|tx| ..)`) would shorten about 40 hand-written transactions; K3 #193 adds the savepoint helper, which is the place for it.
- LOC (loc.py ab256a9): after step 3 7963 -> 7501 (-462); after /simplify 7963 -> 7461, net -502.
Review R1 (reviewer agent): spec PASS, five MINOR, all fixed: `stored_time`'s doc moved back off `COLUMNS`; `mint_uuid()` also in vibration patterns, interval bells, bell sounds; pin tests renamed `..._by_its_uuid_...`; `delete_row` doc; tasks 2.3, 4.1, 6.4 reworded to what was built.
Review R1 (/code-review high): nine cleanup findings, no bug; all fixed: `stored_time` doc and `mint_uuid` (same as the reviewer's); `flag_all_events_unsynced` doc names `wipe_known_remote`; import test comments name `an_import_lands_whole_or_not_at_all`; `delete_synced_row_in(tx, ..)` so labels' delete shares it; `delete_session` looks up only the uuid again (decoding the whole row would panic on an unknown mode the delete used to remove); Box Breath back to one scan through one `COLUMNS`/`from_row`, sorted into cycle order; statement cache 32 -> 64 for the new cached readers; `exchange` in `preset_config.rs`. Suites green after (core 1521+18).
- No second review round: the R1 fixes change no behaviour, data or sync (they restore the base's behaviour or touch docs and tests).
/simplify (four agents): fixed: `query_rows` collects into any `FromIterator`, used for the known-remote, event-uuid, start/duration and import-id sets and both label readers; `query_one` (cached) for every keyed reader and `existing_rowid_by_uuid`; `uuid_by_id(table, id)` replaces `label_uuid_by_id` and `delete_session`'s query; a single insert mints its uuid instead of a blank sentinel; `delete_all_sessions` prepares its DELETE uncached; `Known::ALL` loops the two triple wipes; known-remote tests: twelve per-tracker copies -> one loop test plus persistence and other-tables tests (core lib 1521 -> 1512); delete docs point to `delete_synced_row`; cache comment says 64. Deferred: a `SyncedRow` trait for one find/edit/delete per table (REFACTORING #495, with K3); `query_rows` for the stats readers (#496, P9); transaction helper and `edit_bell_in` from `apply` (OPEN-ISSUES #193 note); `seed_once` in one transaction and `set_box_breath_phase_in` (#306 note). Skipped: a per-import label cache (N cached lookups, imports are rare); `synth_*` test fixtures (already REFACTORING #240); `format!` per call in readers (negligible, cached by text). Suites after: core 1512+18, GTK 171, Android 298; clippy GTK+core all-targets and CI lib clean.
Pre-build questions (fresh agent), all decided as my call:
- A single insert keeps minting a new uuid: `insert_session_tx_less` blanks `session.uuid` before calling `insert_sessions_in`, which returns `(rowid, uuid)` pairs instead of a count (callers that need the count use `.len()`).
- One hand-written `edit_*_in` per table, same shape as `edit_preset_in`, no trait: each keeps its own "unchanged" comparison (patterns: `patterns_equivalent` compared before `updated_iso` is stamped; presets: parsed config; Box Breath keyed by phase id).
- Bell sounds get no duplicate-name check; `bundled_name` stays (a check would be new behaviour).
- `seed_once` wraps only the one-time gate; the seed bodies, and their different handling of a taken name (a bug in REFACTORING #231), stay as they are.
- `list_sessions_filtered` keeps its `ORDER BY id` and only shares `session_from_row`.
- The shared session reader keeps the list path's `expect` text.
- `delete_synced_row`: `DELETE` then `if n == 0` return without a tombstone (one statement fewer, same result).

## Hand-over (step 6)
Artifacts: GTK `target/debug/meditate` (14:29), APK `meditate-android/android/app/build/outputs/apk/debug/app-debug.apk` (14:31), both 2026-11-10.

Test steps (nothing should look or behave differently; these touch every rewritten path):
1. Log: the sessions list, a label filter, the longest session in Stats. Edit a session's note, then delete it; Undo where offered.
2. Labels: create, rename, delete a label; a session with that label keeps showing without it.
3. Bells: add an interval bell, switch it off and on, edit its time, delete it. Rename a custom bell sound (if you have one).
4. Vibration patterns: rename one, edit points and save, save without changes (no Undo toast), delete one and undo.
5. Guided files: star or unstar, rename, delete and undo.
6. Presets: save, rename, star, reorder, delete.
7. Box Breath: change a phase's sound or volume.
8. Sync Now on both devices; the other device shows each change.
9. Import a CSV or backup if handy: sessions land, a second import adds nothing.

Proposed commit message:
core: one reader, insert, edit and delete path per table

Each table read, inserted, edited and deleted its rows through hand-
written copies that had already drifted. One row reader per table,
one insert and delete helper for all synced tables, one insert path
for sessions (the unused bulk insert and its GTK wrapper go), and the
repeated small helpers merged. Nothing a user or another device sees
changes. Finishes REFACTORING package P1 (#215, #216, #217, #231,
#235, #237).

Device test (2026-11-10): "Tiny-Dots" went from 19 to 5 points after an unchanged save on the phone. Events: the laptop's two saves at 12:40 UTC kept 19; the phone's at 13:44:55 and 13:45:01 UTC wrote 5. Cause: Android's editor clamps the stored point count to `max_points_for_duration_s` on open (`ui.rs` `on_pattern_edit`, not touched by this change); Linux keeps it. Pre-existing; maintainer: own change, like Linux. Filed as OPEN-ISSUES #497 (batch DD). The maintainer restores the pattern himself.
