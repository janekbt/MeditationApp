# Refactoring

Everything that would make the code simpler, smaller or faster without changing what the app does, in one place. Bugs stay in OPEN-ISSUES.md; a bug found here goes to "Bugs found" below until the maintainer moves it there. Numbers continue the OPEN-ISSUES numbering, so commit messages stay unambiguous.

Each item becomes its own /spec change: tests that pin the behaviour first, then the refactor, then a phone check.

## Codebase sweep (started 2026-11-10)

The whole repo, slice by slice, through the real /simplify and /code-review skills: five agents per slice. /simplify starts its four agents (reuse, simplification, efficiency, altitude) and /code-review runs at level high in one more. Both skills run as written, with three adjustments: they get the slice's files instead of a diff and treat every line as new; their agents only read (no builds or tests, the laptop has little RAM); and /simplify reports its fixes instead of applying them. Every finding is checked against the code before it lands here. Items that overlap carry a "Related" line: when one is the general form of others, do that one and drop the partial ones. Not swept: `third_party/`, `meditate-android/material-1.0/`.

| Done | Slice | Files |
|:---:|---|---|
| [x] | 1 | core `db/sessions.rs`, `db/events.rs` |
| [x] | 2 | core db: presets, labels, schema, `db/mod.rs`, seeds, and the small db files (device, error, settings, uuids, order key, starred order, known remote, sync state, local changes, test helpers) |
| [x] | 3 | core db: bell sounds, interval bells, vibration patterns, guided and local guided files, box breath phases, session in progress |
| [x] | 4 | core `sync/` |
| [x] | 5 | core `session/` |
| [x] | 6 | core bells, preset config, vibration, breath, sound, bell volume |
| [x] | 7 | core format, time, date math, insights, data io, settings keys, local folder, diag, the rest of `src/` |
| [ ] | 8 | gtk `timer/`, `timer_view.blp` |
| [ ] | 9 | gtk window, log, stats, application, gtk `db/`, their `.blp` |
| [ ] | 10 | gtk preferences, presets, bells, sounds |
| [ ] | 11 | gtk vibrations, vibration editor, guided, labels, keychain, local folder, the rest |
| [ ] | 12 | android `ui.rs`, first half |
| [ ] | 13 | android `ui.rs`, second half |
| [ ] | 14 | android `app.rs` |
| [ ] | 15 | android Kotlin and the other `src/` files |
| [ ] | 16 | android `main.slint`, first half |
| [ ] | 17 | android `main.slint`, second half |
| [ ] | 18 | across apps: logic in both shells that belongs in core |

## Findings

### Slice 1: core `db/sessions.rs`, `db/events.rs` (swept 2026-11-10 at 3c845d9)

/simplify (four angles) and /code-review (high) on the whole files; every item below re-checked against the code. Ordered by gain.

#### #214 One session payload builder
- Where: `sessions.rs:458-467` (`insert_session_tx_less`), `576-585` (`insert_sessions_in`), `692-701` (`update_session_in`).
- Problem: the eight-field sync payload is written out three times. `guided_file_name` had to be added to all three; missing one would sync without the field and lose it on peers.
- Fix: `fn session_payload(uuid, &Session, label_uuid) -> String`, used by all three. Key order doesn't matter to readers; keep the field set byte-identical.
- Related: superseded by #234 (one write path per table, which builds this payload once); decide and do #234 instead of this. #225 is the same for presets.
- Note: rejected before as part of R13 ("too little gain for touching the sync payload"). All four /simplify angles and /code-review raised it again; decide again.

#### #215 One insert path, and the dead bulk insert
- Where: `sessions.rs:436-471` vs `548-589`; `bulk_insert_sessions` `495-500` and its gtk wrapper `meditate-gtk/src/db/mod.rs:814-820`.
- Problem: two copies of the INSERT. They already drifted: the single path saves a deleted label as none (`session_label`), the bulk path writes the raw `label_id` (564), so a stale label id fails the whole batch on the foreign key. Production import resolves labels first (`label_for_import`), so only the test-only path can hit it. `bulk_insert_sessions` has no production caller: the gtk wrapper is never called, Android doesn't use it.
- Fix: `insert_session_tx_less` calls `insert_sessions_in` with one row; the bulk path runs the label through `session_label` too. Delete `bulk_insert_sessions` and the gtk wrapper; move its 8 tests (and the one in `local_changes.rs`) onto `insert_imported` or drop the duplicates.

#### #216 One row mapper each for `Session` and `Event`
- Where: `sessions.rs:225-247, 350-393, 838-860` (same 9-column SELECT and `SessionMode::from_db_str(..).expect(..)`); `events.rs:214-223, 315-325, 338-349`.
- Problem: a new column means six edits; a SELECT whose `row.get` index drifts returns the wrong field. The `expect` messages already differ.
- Related: the same pattern as #225 (presets) and #235 (the other tables); do all row readers together.
- Fix: a `SESSION_COLUMNS` const with `fn session_from_row`, and `fn event_from_row`. `list_sessions_filtered` can become `query_sessions_from_db` with a label filter apart from its ORDER BY.

#### #217 Delete paths
- Where: `sessions.rs:594-632, 638-654`; label lookup in `recompute_session` `889-896`.
- Problem: `delete_session` and `delete_session_by_uuid` repeat select, delete, payload and emit; `delete_all_sessions` builds the delete payload a third time. The uuid-by-rowid and label-id-by-uuid lookups are hand-written although `Database::existing_rowid_by_uuid` (`db/mod.rs:374`) exists.
- Fix: `delete_session` looks up the uuid and calls `delete_session_by_uuid`; one `emit_session_delete(tx, uuid)`; `DELETE ... RETURNING` or `if n == 0` instead of the existence SELECT; `existing_rowid_by_uuid` for both lookups.

#### #218 Cache upgrade replays the slow way
- Where: `events.rs:199-250` (`maybe_walk_events_for_cache_upgrade`, run from `Database::init`).
- Problem: it re-reads the log with its own SELECT and mapper and calls `apply_event_inner` per event: a re-insert with INSERT OR IGNORE and a recompute after every event, about 30k recomputes at startup after a cache-schema bump. `replay_events` (`720-759`) already dedups recomputes.
- Fix: move the dedup-and-recompute half of `replay_events` into a helper; the upgrade collects the distinct `(kind, target_id)` pairs from `events` and recomputes each once, without re-appending.

#### #219 The `guided_file_name` fallback
- Where: `sessions.rs:900-913` and the column fill in `schema.rs:46-53`.
- Problem: the same "newest event that named the file" SQL twice. It also treats an explicit JSON `null` like a missing key, so an edit that clears the name would bring the old one back on peers. Latent: both Log editors keep the original name today (`log/imp.rs:1259`, android `ui.rs:8849`).
- Fix: fall back only when the key is missing (`v.get("guided_file_name").is_none()`); then delete the fill SQL and bump `CACHE_SCHEMA_VERSION`, so the upgrade walk (#218) fills old rows through the one recompute path. Do #218 first.

#### #220 Insights queries
- Where: `sessions.rs:707` and `782` (`best_streak`, `streak`, called back to back from `insights.rs:64-65`); `152` (median); `173`, `197` (running average, daily totals since).
- Problem: every Insights open scans and parses all session days twice; the median loads every duration to read one; `WHERE SUBSTR(start_iso, 1, 10) ...` can't use `sessions_start_idx`, so both are full scans (the doc at 184-188 says it's cheap).
- Related: #297 (one day-totals read for the whole Stats page: do together).
- Fix: one `distinct_session_days_ascending(today)` with `<= today` in the SQL, feeding both streaks; median with `LIMIT 1 OFFSET (SELECT (COUNT(*)-1)/2 ...)`; plain ranges `start_iso >= ?1 AND start_iso < ?2` as at 301. `daily_totals` (735-761) duplicates the `since` query: see #58, which deletes it.

#### #221 Statements per event on imports and wipes
- Where: `events.rs:490-500` (`emit_event`, `device_id()` twice, bump, re-read, insert), `442` (`mark_events_synced`, one UPDATE per id); `sessions.rs:574-575` (one label SELECT per imported row), `514-519` (two full scans before an import); plain `prepare` on paths that run often (`sessions.rs:641, 839`, `events.rs:209, 816`).
- Related: #258 (the sync side of the same per-row transactions) and #232's device note; do the batching for both together.
- Problem: clearing or importing 5,000 sessions runs about 25k statements; `reclaim_suffixed_names` re-parses four statements on every sync.
- Fix: read the device id once per batch and bump the clock with `UPDATE ... RETURNING`; one cached insert statement; memoise label uuids during import; one scan for uuids and start/duration pairs; `prepare_cached`; chunked `WHERE id IN (...)` for `mark_events_synced`.

#### #222 `EventKind::from_db_str` mirrors `as_db_str` by hand
- Where: `events.rs:81-108`, test `850-874`.
- Problem: 23 variants listed three times; every new kind is three edits, and only `as_db_str` is checked by the compiler.
- Fix: `const ALL: [EventKind; 23]`; `from_db_str` finds in `ALL`; the test iterates `ALL`.

#### #223 Smaller ones
- `uuid::Uuid::new_v4().to_string()` at `events.rs:493`, `sessions.rs:441, 557`: use `crate::db::mint_uuid()`.
- `wipe_local_event_log` (`events.rs:413-418`) copies the box-breath seed from `seeds.rs:45-58`, and its doc (389-391) says it calls the seed; share one helper that takes the connection.
- `query_sessions_from_db` (`sessions.rs:340-396`) builds SQL from clause and param vectors kept in step by hand; one fixed SQL with `?1 IS NULL OR ...` as at 839.
- Forwarding pairs: `best_streak`, `daily_totals`, `streak`, `list_sessions_filtered` (`sessions.rs:145-147, 179-181, 217-219, 397-402`) are free functions that only call private methods; move the bodies.
- `winning_mutate` (`events.rs:657-662`) takes three kinds every caller derives from one entity: take `EntityKind`. `row_should_exist` (693-705) is a match guard.
- `session_start_duration_pairs` (`sessions.rs:481`) can be private. Extra block in `recompute_session` (885-932); `let rows = ...; rows` (640-646); blank runs (822-835 and others).
- Docs on the wrong item or stale: `sessions.rs:473-478` (bulk insert's doc above `session_start_duration_pairs`), `events.rs:301-304` (`pending_events`' doc above `all_events`), `sessions.rs:310` (`total_minutes`, gone), `182` (`get_daily_totals`), `344-348` (history, not why).
- `note_or_none` already drops blank notes on every insert; the same check in `data_io.rs:316, 433` is redundant.
- To check, then maybe: the two-pass recompute in `replay_events` (`events.rs:740-756`) looks unnecessary, since `recompute_label` re-links sessions itself (`labels.rs:371-395`); only with a test that shows the order no longer matters.

#### #224 Tests
- `sessions.rs` tests: 117 `Session { ... }` literals with all fields, next to helpers `sit` (1023), `guided_named` (945), `session_on` (3294); use those with `..base`, so the next field isn't 117 edits.
- Duplicate pairs: `update_session_unknown_id_is_noop` (2935) and 1686; `delete_session_unknown_id_is_noop` (2993) and 1734; 3126 and 1800; 3242 and 1872; the three `SessionMode` string tests (1883, 1893, 1916). Merge each pair.
- `events.rs:901-908`: use `synth_event` from `test_helpers.rs:17`.

### Slice 2: core db presets, labels, schema, `db/mod.rs`, seeds and the small db files (swept 2026-11-10 at 3c845d9)

/simplify (four angles) and /code-review (high); every item re-checked against the code. Ordered by gain.

#### #225 One preset reader and one preset writer
- Where: reader `presets.rs:41-158` (same 8-column SELECT and `Preset` closure four times); payload `presets.rs:237-245, 282-290, 308-316, 335-343, 383-391`. Same for guided files: `guided_files.rs:204, 254, 287` and `starred_order.rs:210-218` (`write_library_key`).
- Problem: a new preset column is nine edits; a payload copy that misses a field drops it on peers, since `recompute_preset` takes the whole row from the winning event. The unchanged-write guard exists in some writers and not others (see Bugs).
- Fix: `PRESET_COLUMNS` with `preset_from_row` (as `local_guided_files.rs:34-37` does); one `write_preset(tx, &Preset, kind)` that compares with the stored row, updates and emits the payload from the struct. Each mutator becomes load, change one field, write. The same for guided files. Superseded by #234, its general form for every synced table: do #234 instead. The unchanged guard there also fixes #244 and #248.

#### #226 One mechanism for changes an older build applied
- Where: `mod.rs:324` (`recompute_interval_bells_from_events` on every open), `schema.rs:47-54` (one-shot `guided_file_name` fill).
- Problem: two special cases for "the user went back to an older release, which applied events without the newer field". The bell one runs on every open; the sessions one only once, so it misses that case (see Bugs). Every future field on a synced table needs its own hook.
- Fix: store the highest `events.id` this build has applied; on open, recompute the targets of newer events (those an older build applied). Replaces both, covers every table, costs only the new events. Do after #218.

#### #227 Re-linking sessions on every label event
- Where: `labels.rs:380-397` (`recompute_label`).
- Problem: every label insert and rename runs an UPDATE that `json_extract`s every session event with a correlated `NOT EXISTS`. A rename keeps the label's id, so nothing needs re-linking. During the cache-upgrade walk (#218) that's one full scan per label event.
- Related: worst inside the cache-upgrade walk; do after #218.
- Fix: re-link only when this recompute created the row (a deleted and re-created label counts). Needs a test that sessions still link when the label arrives later.

#### #228 Startup work on every open
- Where: `mod.rs:308-313` (`PRAGMA quick_check`), `schema.rs:59-64` (ten `pragma_table_info` queries, one per added column), `mod.rs:292-298` (three `execute_batch` calls).
- Problem: quick_check reads every page on every open, and the event log only grows. Related to #100, which covers the sync worker's open.
- Related: #100 (the sync worker's open) and #246 (a failure isn't shown); one change for all three.
- Related: also #258 (the sync worker's open).
- Fix: quick_check once per version change or off the UI thread; read each table's columns once; one batch for the pragmas.

#### #229 Preset and guided-file moves read too much
- Where: `presets.rs:358-359, 401-415` (`move_preset`, `move_starred_preset`: four queries carrying `config_json` where two do); `starred_order.rs:100` (`move_guided_file` loads the starred list, a setting and all local files on every move, used only in the no-room branch); `presets.rs:265-281` (`update_preset_name` reads the row twice); `presets.rs:423-432`, `labels.rs:131-137` (existence SELECT before DELETE).
- Fix: a private `move_preset_in(tx, list, from, to)` for both moves; load the starred list inside the fallback branch; read the full row once as `update_preset_config` does; `DELETE ... RETURNING` or the changed-row count.

#### #230 The conflict suffix format in three places
- Where: `error.rs:45-48` builds it, `labels.rs:255-256` rebuilds it by hand, `events.rs:816` matches it with `LIKE`.
- Problem: the format is meant to become translatable; changing it in one place silently stops the merge prompt from finding conflicts.
- Fix: `strip_conflict_suffix(name, uuid) -> Option<&str>` next to `conflict_suffixed_name`, used by both detection sites.

#### #231 Repeated helpers
- `order_key`: the "fresh times 1 ms apart" fallback twice (`presets.rs:369-373`, `starred_order.rs:186-189`); one `order_key::fresh_keys(n)`.
- `known_remote.rs:18-106`: three set/record/wipe trios that differ only in table and column; table-parameterised helpers as `read_kv`/`write_kv` do. The guided trio has no tests.
- `seeds.rs:27, 70, 93, 121`: the seeded-once gate four times; one `seed_once(key, tag, body)`. Labels and presets ignore a duplicate name, patterns and sounds don't (see Bugs).
- The "label uuid by id" query three times (`labels.rs:132, 301`, `sessions.rs:412`); one helper.
- `settings.rs:70-74` repeats `write_kv`'s upsert; `presets.rs:422-427` hand-writes `existing_rowid_by_uuid`; `labels.rs:181, 233`, `device.rs:25` mint uuids by hand instead of `mint_uuid()`; `mod.rs:340-346` matches `QueryReturnedNoRows` instead of `.optional()`.
- `error.rs:111-112` lists the four Box Breath phase names by hand; use `BoxBreathPhaseId::from_db_str` and `EventKind::BoxBreathPhaseUpdate.as_db_str()`.

#### #232 Smaller ones
- Extra `{ ... }` block after `let Some(v) = self.winning_mutate(..) else {..};` in six recompute functions (`labels.rs:360-398`, `presets.rs:457-478`, `guided_files.rs:338`, `bell_sounds.rs:287`, `vibration_patterns.rs:356`, `interval_bells.rs:358`).
- `schema.rs:31-41` vs `137-150`: post-release columns are written in `ADDED_COLUMNS` and again in `CREATE TABLE`, the volume default once as `50` and once as `{BELL_VOLUME_DEFAULT}`; leave them out of `CREATE TABLE` and let `add_missing_columns` add them on fresh databases too.
- `read_kv`, `write_kv`, `existing_rowid_by_uuid` (`mod.rs:339, 359, 380`) build SQL with `format!` and prepare it fresh; static SQL per table with `prepare_cached`.
- `device`: seed the row once in `init`, then `UPDATE ... RETURNING` (part of #221; do it there); `observe_remote_lamport` runs three statements per pulled event; a single-row CHECK as `session_in_progress` has.
- `uuids.rs`: the public `.0` field and cross-type `PartialEq<str>` are a test-compat shim (`starred_order.rs` uses `g.uuid.0`).
- Docs: `labels.rs:237-246` (`find_or_create_label`'s doc on `list_label_conflicts`), `mod.rs:135-144` (`Event`'s doc on `Database`), `schema.rs:128-131` ("kept literal", but it's substituted), `116-117` (`sound`, now `sound_uuid`), `23` (says three fields, has four), `presets.rs:36-38` (says timer first; `ORDER BY mode` puts box_breath first); `presets.rs:250-256` blank run. The busy-timeout comments (`mod.rs:24-26, 193`) are #157.

#### #233 Tests
- `labels.rs` tests 1130-1325: nine hand-built `Event` literals; `synth_event`/`synth_label_insert` exist (also `error.rs:206`). `presets.rs:595`: use `test_helpers::event_payload`.
- `test_helpers.rs`: `synth_session_insert`/`_update` (320-370) and `synth_label_insert`/`_rename` (391-419) differ only in the kind; `drain_events` (163-167, 449-453) marks one id per call and commits per event; `sample_event` (472) hard-codes `DEVICE_A`'s string; `looks_like_uuid_v4` (483-496) is `Uuid::parse_str` plus a version check; the module doc (293) says `pub(super)`, `release_v26_11_0_database` is `pub(crate)`.
- `pending_events().unwrap().into_iter().map(|(_, e)| e).collect()` 11 times in 8 files: one `events_of(db)` helper.
- `mod.rs:770-842`: three tests copy the same `session_in_progress` INSERT; one `insert_sip` helper.
- `presets.rs:1217, 1222, 1251, 1300`: fuzz loops call `pending_events().len()` twice per step on a log that never drains (quadratic); a `COUNT(*)` helper.

### Slice 3: core db bell sounds, interval bells, vibration patterns, guided and local guided files, box breath phases, session in progress (swept 2026-11-10 at 3c845d9)

/simplify (four angles) and /code-review (high); every item re-checked against the code. Ordered by gain.

#### #234 One write path per synced table
- Where: local writes `bell_sounds.rs:110-134, 166-179`, `guided_files.rs:183-209, 236-259, 279-292`, `vibration_patterns.rs:168-199, 287-311`, `interval_bells.rs:118-155, 181-217`, `box_breath_phases.rs:91-114`; their recomputes at `bell_sounds.rs:297-317`, `guided_files.rs:345-358`, `vibration_patterns.rs:364-379`, `interval_bells.rs:379-420`, `box_breath_phases.rs:147-166`.
- Problem: every local write runs its own INSERT or UPDATE and builds its payload by hand; peers rebuild the row from that payload with the recompute's own SQL. Each table spells out its columns three or four times and its payload two or three times (`interval_bells` 16 columns three times), and nothing ties the local row to what a peer builds. A missed field or a different default (`mime_type` "audio/wav", `enabled` true) only shows on the other device.
- Fix: per table, build the payload once from the row (`*_payload(&row)`), `emit_event`, then call the existing `recompute_*` in the same transaction; the write SQL in insert, rename, star and update goes away and local rows are built the way peers build them. Insert and rename keep a name check first (`is_*_name_taken_from_db`), since recompute suffixes a clash instead of returning `Duplicate*`. The general form of #214 and #225; the smaller step is one payload builder per table.

#### #235 One row reader per table
- Where: `bell_sounds.rs:212-233, 244-266`, `vibration_patterns.rs:45-70, 76-99` (and a third partial decode in `rename_vibration_pattern` 233-253), `guided_files.rs:36-55, 109-124`, `box_breath_phases.rs:19-47, 55-75`; `interval_bells.rs:226-232, 258-263` load the whole list to find one row, because there is no second decoder.
- Related: the same pattern as #216 (sessions, events) and #225 (presets); do all row readers together.
- Fix: `COLUMNS` plus `from_row` as `local_guided_files.rs:34-49` does; `list_bell_sounds` becomes `list_bell_sounds_for_category(None)`; keyed queries for the single-bell lookups. The bell-sound and box-breath pairs are in #85 too.

#### #236 Guided file names for the Log scan the whole event log
- Where: `guided_files.rs:64-91` (`guided_file_names_from_db`), called on every Log refresh: android `ui.rs:2959-2971` (six call sites), gtk `log/imp.rs:188`.
- Problem: `kind IN (...)` has no index, so every refresh walks the event log and parses the JSON of every guided-file event, then overwrites the name for every file still in the library anyway.
- Fix: in SQL, only the latest name per deleted file (`target_id NOT IN (SELECT uuid FROM guided_files)`, max lamport per target); or keep the map and rebuild it only when guided events change.

#### #237 One delete helper
- Where: `bell_sounds.rs:188-201`, `interval_bells.rs:238-251`, `vibration_patterns.rs:320-336`, `guided_files.rs:302-318`, `presets.rs:421`, and the no-winner branch of every `recompute_*`.
- Problem: five copies of transaction, existence check, DELETE, tombstone, commit; two of them hand-write the existence check instead of `existing_rowid_by_uuid`.
- Fix: `delete_synced_row(table, uuid, EventKind)` and `delete_row(table, uuid)` for the recompute branches.

#### #238 Replay and startup statements
- Where: the recompute upserts and deletes use `conn.execute` (`bell_sounds.rs:281, 297`, `interval_bells.rs:352, 379`, `vibration_patterns.rs:350, 376`, `guided_files.rs:332, 355`, `box_breath_phases.rs:125, 150`); `mod.rs:314-321` raised the statement cache for exactly these, but only `winning_mutate` uses it. `recompute_box_breath_phase` (125-144) rewrites `winning_mutate`'s query with the kind as a string literal. `recompute_interval_bells_from_events` (`interval_bells.rs:323-338`) upserts every bell on every open, even when the row is already right.
- Fix: `prepare_cached`; a `latest_mutate` helper that `winning_mutate` and the box-breath recompute share; the startup upsert only when values differ (`ON CONFLICT ... DO UPDATE ... WHERE ... IS NOT excluded...`). The startup upsert is superseded by #226: do #226 instead of that part.

#### #239 Smaller ones
- `rename_vibration_pattern` (`vibration_patterns.rs:232-258`) reads and decodes the row, then `update_vibration_pattern` reads it again and re-encodes the intensities; `rename_guided_file` (`guided_files.rs:245-249`) queries `is_starred` a second time.
- `is_*_name_taken_from_db` is the same query three times (`guided_files.rs:131`, `vibration_patterns.rs:106`, `presets.rs:129`); one table-parameterised helper.
- `interval_bells.rs`: insert and update build the same payload (144-154, 206-216) and the same params in another order; the `(time, jitter, step)` match twice (297-300, 370-373).
- `session_in_progress.rs`: `finalize_session_in_progress` writes the singleton DELETE twice (211, 227-230) instead of `clear_session_in_progress()`; the comment at 197 says "first minute", the check is 0 s; `insert_session_clearing_snapshot` (161-163) is only called by tests (both apps call `insert_session_counting_local_play`); the 60 s snapshot upsert (61-85) could use `prepare_cached`.
- `local_guided_files.rs:84-103` (`walk_folder`) stats and allocates every non-audio file before filtering; check `is_guided_audio` first.
- Docs: `vibration_patterns.rs:22` (`updated_iso` "so chooser sorts by recency", it sorts by `created_iso`), `261` and `guided_files.rs:215` (say `updated_iso` resolves concurrent edits; recompute uses lamport order), `bell_sounds.rs:203` ("creation order", bundled rows sort last). Blank runs at `vibration_patterns.rs:39, 223, 315, 337`, `guided_files.rs:213, 296-298`.

#### #240 Tests
- `bell_sounds.rs:328-355`, `interval_bells.rs:454-518`: five `synth_*` helpers hand-build `Event`; make them wrappers over `test_helpers::synth_event`.
- `sync/orchestrator.rs:2155-2162` (test seed) hand-writes the reverse of `BellSound::extension`'s MIME table.

### Slice 4: core `sync/` (swept 2026-11-10 at 3c845d9)

/simplify (four angles) and /code-review (high); every item re-checked against the code. Ordered by gain.

#### #252 Rate-limit retry in one place
- Where: `orchestrator.rs:949-1000` (`put_with_rate_limit_retry`), `1015-1035` (`put_atomic_with_rate_limit_retry`), all of `backoff.rs` (266 lines).
- Problem: only PUT retries a 429; PROPFIND, GET, MOVE, DELETE and MKCOL don't (see #267). `BackoffState` is built fresh per PUT and dropped, so its window, reset and counters reduce to one formula; three of its eleven tests cover paths that can't happen.
- Fix: retry inside `HttpWebDav::send` for every verb (sleep `retry_after`, else `min(1 << n, 30)` s, at most 8 tries); delete `put_with_rate_limit_retry` and most of `backoff.rs`. Fixes #267.

#### #253 One pull helper, one cap, for bells and guided files
- Where: pull loops `orchestrator.rs:716-766` and `885-918`; push `824-848` and `924-946` (push half-shared through `push_file` 782); paths and dirs `659-676`, `858-875`; caps `67` (`MAX_SOUND_GET_BYTES = 11 MB`, hard-coded) and `73`, and the second size check after the read (`758`, `910`).
- Problem: every pull rule is written twice (create dir, skip if present, NotFound and TooLarge mean skip, cap check, write, record known). Two caps for one limit; the bell one won't follow `sound::MAX_CUSTOM_BELL_BYTES` if that changes.
- Fix: `pull_file(local, remote, cap)` mirroring `push_file`, or a small per-kind descriptor (dirs, paths, cap, known-set) so each loop exists once; `get(&remote, MAX_CUSTOM_*_BYTES)` and drop the second check. Do together with #265.

#### #254 The sync failure state
- Where: `settings.rs:22-30, 114-189` (one key, `KEY_LAST_SYNC_ERROR_KIND`, holds `""`, two bare strings, or JSON; five writers of the same key pair; three readers of the same key), `indicator.rs:94-153` (`derive` builds a state that `state_from_db` then patches: reason, NeedsPassword, NotUploaded; `derive` is used only by tests), `indicator.rs:133-134` repeats `sync::should_attempt` (`mod.rs:38-41`), `runner.rs:120-131`.
- Problem: GTK polls `state_from_db` every 2 s (`window/imp.rs:761`): 5 to 8 SELECTs per tick, the kind key up to three times, and with a refused upload a full read of `bell_sounds` and `guided_files`.
- Fix: one stored reason enum with `RemoteDataLost` and `PasswordMissing` variants, written by one `set_error`, read once (still accepting the two old bare strings, so no migration); one `state_from_db` that reads the needed keys in one `WHERE key IN (...)`; call `should_attempt`.

#### #255 One transaction for resetting the remote trackers
- Where: `settings.rs:79-85` (`adopt_account`) and `252-260` (`prepare_push_local_recovery`).
- Problem: the same four or five writes, each its own transaction, already drifted (adopt also clears refusals). AGENTS.md: multi-step writes are all-or-nothing. Adopt reruns because its marker is written last; recovery has no marker (see #263).
- Fix: `Database::reset_remote_trackers()` in one transaction, used by both. Fixes #263.

#### #256 Status codes per verb, one MKCOL helper
- Where: `webdav.rs:258-267` (`map_status_error` turns 405, 409, 412 into `Conflict` for every verb), `orchestrator.rs:647-657, 671-676, 870-875` (three `ensure_*_dir_exists`), `runner.rs:147-148`.
- Problem: only MKCOL's 405 means "exists". A 409 or 405 on PUT or MOVE loses its status (shows as `Server(None)`); 412 can't happen (If-None-Match is never sent).
- Fix: `mkcol()` maps 405 to `Ok(())` itself, the shared map keeps `Server{status}`; the three helpers become one `mkcol()?` per path.

#### #257 A local file error isn't a bad event
- Where: `SyncError::InvalidEvent` used for local create-dir and write failures (`orchestrator.rs:730, 892, 1077`), serialise errors (432, 569) and a corrupt manifest (558, 592); `runner.rs:144` maps it to `UnreadableReply`; `sync_with_progress` (491) gates "push anyway" on it.
- Fix: a `SyncError::LocalFile` variant with its own failure key. Fixes #266.

#### #258 Requests and reads per sync
- Where and problem:
  - `runner.rs:56`: every sync opens a fresh connection, so it reruns the whole init including `quick_check`. That is #100; see also #228 and #246.
  - `orchestrator.rs:412`: two MKCOLs on every push, although pull has just listed `events/`.
  - `524-530`: compaction PROPFINDs `events/` again and re-reads the known batches.
  - `291`: `known_event_uuids()` loads every event uuid on every pull, even with nothing new.
  - `runner.rs:90`: loads every pending event with payload only to log the count; push loads them again.
  - `701`: reads the bell list once per pulled delete event.
  - `720/826`, `887/926`: pull and push each read the bell and guided lists.
  - `776`: rewrites `refused_uploads` on every push.
  - `336-338, 449-450`: one transaction per known batch; push marks synced and records known in two transactions.
- Fix:
  - MKCOL only when the listing was `NotFound`; pass pull's listing and the pushed uuid to compaction.
  - Load uuids lazily on the first unknown batch; `COUNT(*)` for the log line.
  - Build sets and lists once per sync; skip unchanged writes.
  - One transaction for the batch records and for mark-synced plus record-known.
  - The init part goes with #100.

#### #259 Audio goes through memory whole
- Where: push `orchestrator.rs:794-799` (`std::fs::read` up to 100 MB), pull `300, 748, 904` with `write_whole_file`.
- Problem: about 100 MB peak memory per guided file on a phone.
- Fix: stream: ureq takes a `File` as body; pull can `io::copy` the capped reader into the `.part` file. Needs reader-based `put`/`get` on the `WebDav` trait.

#### #260 The connection test has its own error mapping
- Where: `credentials.rs:27-90` (`TestConnectionResult`, its English `Display` and `detail()`), next to `runner::failure_key` (`runner.rs:135-156`).
- Problem: two mappings that already disagree (a redirect is "Server error" in the test, `Redirected` in the status; 404 is `NotWebDavRoot` vs `Server(Some(404))`). The English text is unused in production (both apps translate each variant) and goes against the typed-key rule. `SaveSyncPlan` (134) repeats `Credentials`' fields.
- Fix: `test_connection` returns `Result<(), SyncFailureKey>` with the mapping on `WebDavError` (maybe a NotWebDavRoot key); delete the `Display`.

#### #261 Smaller ones
- `write_whole_file` (`orchestrator.rs:1066-1081`) re-implements `data_io::replace_file` (`data_io.rs:126-141`); one helper with the suffix as parameter (keep `.part`, the orphan sweep needs it). Superseded by #298 (one helper for all three copies).
- `WebDavError` lacks `#[derive(Clone)]`, so `clone_err` is written twice (`fake.rs:113-136`, `meditate-gtk/src/sync_runner.rs:110-127`); GTK also copies core's `AlwaysErrs` and its `test_connection_with` tests (`sync_runner.rs:96-128`): delete the copy. Then `fake` can be `#[cfg(test)]` (the doc at `mod.rs:9-10` already says crate-private).
- `orchestrator.rs`:
  - The events listing (244-254, 525-529), the manifest read (550-564, 635-642) and the batch upload (422-447, 565-581) are each written twice: one helper each.
  - `if pushed > 0 || ...` (459) is always true.
  - `parse_batch_uuid_from_filename` allocates and runs up to three times per name.
- The progress callback (`push_with_progress`/`sync_with_progress`, `runner.rs:93-101`) fires once, as `(pushed, pushed)`, and its log line repeats `sync.done`. `push()` and `sync()` are only called by tests; keep one non-generic pair.
- `coordinator.rs:73, 81, 103`: `start_pass`, `should_run_again_after_pass` and `release` are `pub`, but only `drain` uses them and both apps forbid them by source-text tests; make them private. `release` can be two lines.
- `runner.rs:135-141`: arms of `failure_key` that `record_outcome` never reaches.
- `webdav.rs`: the last-segment logic is written twice (408-411, 432-435); `177` can use `format!`. Hand-written base64 and percent coding (440-492, 346): the crates are already in Cargo.lock through ureq but would become direct dependencies (Cargo.toml: ask first). Low priority.
- `clear_nextcloud_account` (`settings.rs:92`) is dead: already #58.
- Stale docs: `orchestrator.rs:376-379` (says the runner wraps push in `BackoffState::wait_for`); `webdav.rs:36, 134, 264` (If-None-Match, never sent).

#### #262 Tests
- `orchestrator.rs`: `Sync::new(&db, &x, "Meditate", PathBuf::new(), PathBuf::new())` 82 times, so add a `sync_of(db, w)` helper using `REMOTE_BASE_PATH`. Eleven `impl WebDav` wrappers each forward five methods to override one; one `Intercept` double with per-verb closures replaces them.
- `push_on_empty_pending_uploads_zero_files_but_creates_events_dir` (1160) never checks the directory (`FakeWebDav::mkcol` is a no-op): deleting `ensure_events_dir_exists()` keeps it green.
- `coordinator.rs`:
  - Three tests (196, 329, 387) hand-write the drain loop instead of calling `drain`.
  - `release_no_op_when_in_flight_was_already_taken_by_a_racing_request` (245-268) re-implements `release` inline and tests nothing.
- `runner.rs:173, 222`: `run` and `run_broken` are near-copies.
- `orchestrator.rs:2155-2162` is #240's MIME table.

### Slice 5: core `session/` (swept 2026-11-10 at 3c845d9)

/simplify (four angles) and /code-review (high); every item re-checked against the code. Ordered by gain.

#### #269 One bell effect that carries a `BellCue`
- Where: `effect.rs:31-76` (`FireBell`, `FireStartingBell`, `FireEndBell`, `FireBoxBreathCue`, each with sound uuid, pattern uuid, signal mode and volume), `effect.rs:155-203` (`fire_route` folds the four back into one `FireRoute`), and the four builders `mod.rs:1084-1134, 1165-1174` (`starting_bell_effect`, `end_bell_effect`, `box_breath_cue_effect`, `fire_due_bells`), which each repeat `effective_signal_mode`, clone `.0` of both uuids and copy the volume.
- Problem: a new cue field (as `volume` was) means about ten edits. The uuids lose their `BellSoundUuid`/`VibrationPatternUuid` types on the way. Both apps only read these through `fire_route()` (gtk `timer/imp.rs:4349`, android `ui.rs:437`); the variant names appear only in tests and two `matches!` in android `app.rs` (2481, 4511).
- Fix: one `Effect::Fire { channel: FireChannel, phase: Option<BoxBreathPhaseId>, cue: BellCue }`, the log tag derived from channel and phase, built by one `fn fire(channel, &BellCue, override) -> Option<Effect>`; `FireRoute` and `fire_route` go. About -80 lines; touches about 15 test asserts in core and two in android.

#### #270 One way to end a session
- Where:
  - Core: five paths set `phase = Stopped` and `final_duration_secs = Some(..)` by hand (`mod.rs:451, 483, 500, 535, 694`); `stop` and `add_overtime_and_finish` are the same body. The end is signalled as `EndSession`, `EndBoxBreath` or `AutoFinish`, all with the same `duration_secs`, and stored again in `final_duration_secs`. Box Breath's end (`691-705`) repeats `reach_end`'s Auto-Finish and end-bell rule (`528-544`). `finish_overtime` (`476-482`) re-derives `shape.target_secs()` by hand.
  - Android: `ended_duration_secs` and `completion_notice_secs` scan the effects (`app.rs:189-215`); the "was active, now finished: save and notify" block is in the tick handler and again in the guided-track-end handler (`ui.rs:5137-5165`, `5064-5080`); `finish_overtime`/`add_overtime` re-check `ui_state() == Overtime` (`app.rs:708-735`) although core already ignores them outside Overtime.
  - GTK: reads `final_duration_secs()` (`timer/imp.rs:2579`), `core_session_end` scans only `EndSession` (3137-3142), and it branches on `AutoFinish` / `EndBoxBreath` / `EnterOvertime` in three places (2262/2268, 3007/3011, 3044/3047). Both apps call `completion_duration_secs()` at the end for the notification (gtk 2557, 3089; android `app.rs:213`, `ui.rs:5183`), so "notify only when it ended by itself" is decided in each app.
- Fix: one private `fn end(&mut self, secs, how)` in core; one `Effect::Ended { duration_secs, how: Hand | Natural | AutoFinish }` (`EndBoxBreath` keeps its meaning as `Natural` in Box Breath: GTK keeps that page up with Save); `phase: Stopped { duration_secs }` instead of the second field; the notification decision as a core field of the end. Both apps then go to "ended" when the phase is `Stopped` after any call. Also fixes #277 and #278.
- Related: #273 (do together), #271.

#### #271 Effects that only repeat state
- Where: `effect.rs:20, 24, 84, 96` (`UpdateDisplay`, `EndPrep`, `UpdateOvertimeLabel`, `EnterOvertime`); `mod.rs:62-80, 798-810` (`TickOutcome`, `tick_summary`).
- Problem: all four can be read from `display_secs(now)`, `ui_state()` and `end_buttons()`, which core already offers. `tick_summary` exists to fold three of them back into state and has one caller (gtk `timer/imp.rs:3034`); gtk `tick_prep` (2986) and `tick_overtime` (3104-3114) fold them by hand, Android draws from `display_secs` (`app.rs:833`). So the display rule is kept twice in core: in `display_secs` and in the `UpdateDisplay` built by `tick_prep`, `tick_running` and `tick_box_breath` (which pushes `elapsed.as_secs()` itself, 729). "Seconds left, rounded up" is written inline four times (573-603, 872-879, 1059-1063, 970-984). `TickOutcome::overtime_delta` is never read; the free fn `ui_state(Option<&Session>)` (58-60) is test-only.
- Fix: effects carry only what an app must do (bells, stop signals, stop guided audio, the end, #270); after each call the apps redraw from the pull API. Delete `TickOutcome`, `tick_summary`, the four variants and the effect-scanning loops in both apps; one `secs_left(target, elapsed)` next to `ceil_secs`. Supersedes the `EndPrep` part of #85.

#### #272 The session shape is built in each app
- Where: gtk `timer/imp.rs:2191-2292`, android `ui.rs:4463-4510` (its comment: "Mirrors GTK's SessionShape::Guided build"); core `settings.rs:28-72` has `mode()`, `stopwatch_on()` and `target_secs()` but no constructor.
- Problem: both apps repeat the decision tree, including `pattern.cycle_aligned_target_secs(..)` for a Box Breath countdown and "Guided: the stopwatch flag means count-up display". They already differ: GTK returns early on a 0 target, Android reads Guided's stopwatch flag from the db and the other modes' from the UI. Against "decisions go in core".
- Fix: `SessionShape::new(mode, stopwatch_on, target_secs, pattern)` next to `from_db`, used by both apps. Related: slice 18.

#### #273 Pause in one place
- Where: `mod.rs:218-220` (`SessionPhase::Paused`, never set; arms at 563, 856, 981, 1019), `253` (`is_paused`), `timer.rs:18` (`Stopwatch::Paused`); `pause`/`resume` swap in a dummy clock (`mod.rs:400-403, 416-418`) because `paused_at`/`resumed_at` take `self`.
- Problem: three representations of one fact, two of which can disagree with the phase (#277).
- Fix: delete `SessionPhase::Paused`; derive `Copy` on `Stopwatch` (only `Duration`s) and write `self.phase_clock = self.phase_clock.paused_at(now)`; derive `is_paused` from the clock. Supersedes the `SessionPhase::Paused` part of #58. Related: #85 (`Stopwatch` serde), #270.

#### #274 The ring is rebuilt every frame
- Where: `mod.rs:883-960` (`bell_ring`), `bells.rs:210` (`rings_before` allocates a `Vec` of every planned ring from 0); called each frame by gtk `window/imp.rs:250` and at least 5 times a second on Android (`ui.rs:535`).
- Problem: a 5 s bell over an hour builds and drops two vectors of about 720 elements per frame, plus the counters the frame callback ignores. Already marked as a known `ponytail:` shortcut (gtk `window/imp.rs:246-248`).
- Fix, only if the Librem 5 janks (the iterator half is also in #289): `rings_before` as an iterator, and a cheap `ring_filled(now)` for the frame path; beads only when a bell rings or the phase changes.

#### #275 Smaller ones
- `mod.rs:613`: `self.settings.shape.clone()` on every tick just to branch; match by value (the fields are `Copy`).
- `display_secs_for_running` (`mod.rs:1059-1077`): two arms return `elapsed.as_secs()`; Guided's count-up can be a guard.
- `tick_prep` (573) and `prep_remaining_secs` (872-880) compute the same rounding; the `Prep` part of the `display_secs` arm at 1019 is unreachable (early return at 1015). Goes with #271.
- `settings.bell_rng_seed.max(1)` (358) repeats `bells::xorshift64`'s guard (`bells.rs:997`); the docs at `settings.rs:92-94`, `mod.rs:264-266` name the wrong owner.
- `phase_to_id` (`mod.rs:1033-1040`) belongs in `breath.rs` as `impl From<Phase> for BoxBreathPhaseId` (its comment at 23-29 already lists it). Superseded by #286 (one phase enum).
- `Session::toggle_action(ui_state)` takes a value both callers get from the session (gtk `timer/imp.rs:2383, 4245`, `mpris.rs:97`): make it `&self`.
- Stale docs: `effect.rs:5` ("future Android"); `effect.rs:148-151` leaves `AutoFinish` and `StopGuidedAudio` out of the non-bell list; `mod.rs:213-214` ("Stage 0a"); `mod.rs:426` (`stop` returns two effects, not one); `TickOutcome` "three values" vs `tick_summary` "two pieces"; `mod.rs:275-282`, eight lines on `u64` vs `u32`.

#### #276 Tests
- `settings.rs:177-197`: `by_hand` copies `from_db` line for line, so `from_db_builds_what_the_shells_built` compares the function with itself. Delete `by_hand`, `same` and that test; `from_db_gates_prep_starting_bell_and_cues_by_mode` (239-252) checks the real rules; add one assert on `stopwatch_on()` per shape.
- `mod.rs:1186-1265`: `timer_settings_with_prep`, `timer_countdown_settings`, `timer_stopwatch_settings` (and inline literals at 1976, 2328, 2747, 3148, 3184) spell out all nine fields; use `..Default::default()` as Android's tests do (`app.rs:619, 5010`). `timer_settings_with_prep(p)` is `timer_prep_settings(p, 600)` (16 call sites).
- Seven near-identical `BellCue` literals (`mod.rs:1235, 2445, 2558, 2587, 2914, 2961, 2981`): one `cue(sound)` helper.

### Slice 6: core bells, preset config, vibration, breath, sound, bell volume (swept 2026-11-10 at 3c845d9)

/simplify (four angles) and /code-review (high); every item re-checked against the code. Ordered by gain.

#### #281 One reader and writer per bell slot
- Where:
  - Starting and end bell settings are read in `bells.rs:702-752` (`read_volume`, `starting_bell_cue_from_db`, `end_bell_cue_from_db`, near copies of each other), `preset_config.rs:551-578` (`snapshot`) and `665-684` (`apply`), and `bell_volume.rs:201-246` (`read`, `write`, `sound_uuid`, each with its own three-way match on the slot).
  - The volume is parsed three times (`bells.rs:702`, `preset_config.rs:551`, `bell_volume.rs:202`) and written by hand in `apply` (674, 683) and android `ui.rs:3755`, although `bell_volume::read/write` exist (only GTK uses them).
  - Keys as bare strings: `"starting_bell_active"`, `"starting_bell_sound"`, `"starting_bell_pattern"`, `"starting_bell_signal_mode"`, `"interval_bells_active"`, `"boxbreath_cues_active"`, 6 to 20 times each across bells.rs, preset_config.rs, bell_volume.rs, format.rs, session/settings.rs, gtk `timer/imp.rs` and android `ui.rs`; `settings_keys` holds the rest of this family as constants.
  - `snapshot` re-reads values that have their own readers and has drifted: `preparation_time_secs` through `read_u32` without the 5-300 clamp of `format::parse_prep_secs` (`format.rs:643`), the label without `labels::label_uuid_from_db`, `keep_screen_awake` and `auto_finish` without their `settings_keys` readers.
- Problem: every new per-bell field (volume was the last) means about eight edits, and defaults drift: GTK still defaults the starting sound to the old `"bowl"` (#61).
- Fix: one `bell_cue(db, BellSlot) -> BellCue` and `set_bell_cue(db, BellSlot, &BellCue)` with key constants in `settings_keys`; `bell_volume::read`, `sound_uuid`, `snapshot`, `apply` and both apps use them; `snapshot` uses the existing readers. Related: #61, #269 (the effect carries the same `BellCue`).

#### #282 Preset enums as typed fields
- Where: `preset_config.rs:85-86, 107, 114, 234, 255-257` and `cues_signal_mode` (29) store signal mode, bell kind and phase as `String`; `apply` parses them back and silently `continue`s on a bad one (701-706, 737-743); GTK's fallback repeats `"both"` because `Default` gives `""` (`timer/imp.rs:3870-3880`).
- Problem: a malformed or newer entry is dropped from the preset without a word, against "every failure tells the user".
- Fix: serde-typed `SignalMode`, `IntervalBellKind`, `BoxBreathPhaseId` with `rename_all`; the JSON strings stay the same, so no migration. Do after the one-transaction apply (OPEN-ISSUES #193), so a bad entry fails the whole apply.

#### #283 The active-preset check reads everything on every change
- Where: `preset_config.rs:481-537` (`active_presets`): reads the current timing, a full `snapshot` (about 22 setting reads, the interval bells and four phase reads), lists the starred presets and parses each `config_json`. Both callers list the starred presets again right after (gtk `timer/imp.rs:3389/3415`, android `ui.rs:705/706`), and Android parses each JSON once more for the subtitle (733). It runs after every local synced write via the 500 ms change watch (gtk 402, android 4815), so on every slider or switch change. `shown(current.clone())` (530) clones the whole config for the phases.
- Also: the current `PresetTiming` is built here, in gtk (`timer/imp.rs:3837-3859`) and in android (`ui.rs:1876-1893`); `apply` returns `cfg.timing.clone()`, which both callers already hold (gtk 3907, android 1827).
- Fix: `active_presets(db, mode, &starred)` taking the list the caller has, or returning per-row flags with the parsed config; a core `current_timing(db, mode)` for all three; `apply` returns `Result<()>`. Related: #272 (the same kind of app-side building).

#### #284 Name lookups read the whole library
- Where: `bells.rs:541-561`. `resolve_sound_name` reads every bell sound (8 strings per row) to find one name; `resolve_pattern_name` fetches a full pattern and parses its `intensities_json` to read `.name`. Called once per row on the GTK Manage Bells list (`bells.rs:239, 660, 737, 836`), 4 times per Box Breath refresh on Android (`ui.rs:3897`) and twice on setup refresh (3730-3731).
- Problem: N full table reads per screen; a read error becomes `Missing`, so every subtitle asks the user to re-pick sounds that are fine.
- Fix: `SELECT name ... WHERE uuid=?1` for both (or load the library once per rebuild and use the pure `sound_name`); pass a read error on. `from_lookup` already handles the empty uuid (511), the early checks can go. Related: #235.

#### #285 Box Breath phases read one at a time
- Where: `bells.rs:880-901` (`box_breath_cues_from_db`), `preset_config.rs:592-599` (`snapshot`, which also re-lists the four phases instead of `BoxBreathPhaseId::all()`), android `ui.rs:3893`: four `get_box_breath_phase` calls where `list_box_breath_phases()` returns all four in one query.
- Fix: one `list_box_breath_phases()` and a match. Related: #85 (`list_box_breath_phases` repeats `get_box_breath_phase`: keep the list one).

#### #286 Breath: one rule, one enum
- Where:
  - Both apps step a phase with the same index-to-field match and clamp (gtk `timer/imp.rs:4645-4664`, android `ui.rs:5338-5357`, "mirrors GTK"); GTK adds a dead `CYCLE_MIN_SECS` check (4660); `breath.rs:139`'s doc speaks of a cycle of at least 2 while the value is 1.
  - The minimum rule is written twice: `clamp_from_raw` (209) and `phase_min_secs(index: u8)` (221, magic indices); `duration_for` (231, test-only) repeats `phases()` (244).
  - `Phase` and `BoxBreathPhaseId` (`breath.rs:16-36`) are the same enum twice; `phase_to_id` bridges them.
  - `phase_at` (263-306): the tail after the loop is unreachable, and its doc says a zero cycle panics only in debug, but `% cycle_nanos` panics in release too (only test code builds the 0-0-0-0 `Default`).
  - gtk `timer/imp.rs:384` writes 4-4-4-4 by hand instead of `BreathPattern::box_breath()`.
- Fix: `BreathPattern::adjusted(phase, delta) -> Option<Self>` in core over `clamp_from_raw`; one phase enum; `unreachable!()` and a correct doc. Supersedes the `phase_to_id` bullet of #275.

#### #287 Vibration editor math in core and again in GTK
- Where: `vibration.rs:339-415` (`EDITOR_HIT_RADIUS_PX`, `nearest_point`, `snap_intensity`) is used only by Android; gtk `vibration_editor.rs:38, 350-363, 391` keeps its own radius, hit test and snap. Two snaps to the 10 % grid: `quantise_amplitude` (78, with a `+1e-6` bias) and `snap_intensity` (390, without), which can differ at a half step; the lerp exists twice (`sample_line_at` 196, `resample_envelope` 271-279, which also repeats `point_x_fraction` 343). `chunk_start_offset_ms` (184) recomputes the stride `split_into_chunks` (159) uses; its only caller (gtk `vibration.rs:221`) calls both.
- Fix: GTK uses the core helpers; one snap; one `lerp_at`; `split_into_chunks` returns `(offset_ms, chunk)` pairs.

#### #288 sound.rs
- `is_passthrough_ext` (140) is called only by tests; `target_extension_and_mime` (174) repeats the wav/ogg rule with a redundant `"ogg"` arm. Kotlin re-decides it from the folder name (`MeditateGuidedImport.kt:111-118`); Rust could hand Kotlin the decision.
- `safe_copy_no_follow` (240-273) is `std::io::copy` by hand, and unlike it stops on `Interrupted` (then deletes the half copy).
- `name_collides` (185) is `name_collides_excluding(.., "")` (197) written out; android `ui.rs:1659` mirrors it again. Bell sounds compare with Unicode `to_lowercase`, the other four kinds with SQL `NOCASE` (ASCII only), so "Ä" and "ä" collide only for bells; one rule.
- `display_name_from_path` (215) is dead and returns English "Custom sound" (typed-key rule); `mime_matches` (51) is test-only; extension checks allocate lowercase strings where `eq_ignore_ascii_case` (as at 159) does.
- `sound.rs:15` and `vibration.rs:43` re-export `preview::{PreviewAction, PreviewToggle}` "for back-compat"; four gtk files use them, android imports `preview` directly.

#### #289 Smaller ones
- `bells.rs`:
  - The `display` parameter of `session_bells_from_db`/`build_active_bells` (788, 931) follows from `total_target_secs == None`; the stopwatch skip at 946 inlines `is_bell_inert_in_stopwatch` (917) and repeats the check at 963.
  - `bells_summary` (`preset_config.rs:449-473`) repeats the interval-bell gate of `session_bells_from_db` (795-797) and the four-field "any cue" test that `BoxBreathCueConfig::cue_for` folds in; it and `session_summary` (418) each read the stopwatch key, and both are always called together (gtk 4725, android 2813).
  - `Channels`/`channel_allowed` (599-635) are pub but only feed `effective_signal_mode`; `ringing_jitter` (426-436) builds the same literal twice; `let mut index = *gaps - 1;` (284) is a dead store; `rings_before` (210) can be an iterator (see #274).
- `preset_config.rs`:
  - `same_settings` (224-226) clones the whole bell per comparison; `edit_interval_bell` (784) clones `old` and `bell`.
  - `apply` inserts a re-added bell and then updates it for the volume (744-758): two writes, two sync events; give `insert_interval_bell` a volume.
  - `ModeSetupVisibility` (377-408) has eight fields from three booleans; `StarVisualState` (347-368) mirrors a bool (one user, gtk `presets.rs:414`); `default_starred_on_save()` (329) returns `true`.
- `bell_volume.rs`: the second clamp in `from_percent` (70-78) can't change anything; `gain_over` (118) has one caller with a constant.

#### #290 Tests
- `fn pct(p)` three times (`bells.rs:1641`, `preset_config.rs:1627`, `bell_volume.rs:270`): call `BellVolume::from_percent`.
- `bells.rs`: `interval_row` (1159) is `row(kind, m, j, true)` (1341); `fixed_bell`/`interval_bell` (1010/1021) repeat one literal.
- `preset_config.rs:1278` `sync()` repeats the pending/replay helper of the `local_folder.rs` tests (~532).

### Slice 7: the rest of core: format, time, date math, insights, data io, settings keys, local folder, diag and the small modules (swept 2026-11-10 at 3c845d9)

/simplify (four angles) and /code-review (high); every item re-checked against the code. Ordered by gain.

#### #294 Dates as `NaiveDate`, not text
- Where: `insights.rs:68-72` formats each day as `"%Y-%m-%d"`; `week_over_week` (`date_math.rs:95-124`) formats the days it walks again to look them up in a `HashMap<&str>`; `aggregate_for_chart_period` groups months by `k[..7]` (275-307); `chart_series` (248) and `ContribCell.date_iso` (`contrib.rs:31, 98`) hand strings to the apps.
- Problem: both apps parse them back: GTK rebuilds a month list with `d[5..7].parse()` on every bar (`stats/imp.rs:707-711`) and splits the heatmap date (`time.rs:22`); Android parses `get(5..7)` (`ui.rs:2277`), `chart_x_label` and `month_abbr(&date_iso)` (2210). About six format/parse round trips and two parsers in the apps.
- Fix: carry `NaiveDate` end to end: `week_over_week(&[(NaiveDate, i64)], today)`, `chart_series` returns `Vec<(NaiveDate, i64)>`, `ContribCell.date: NaiveDate`.

#### #295 One minute-rounding rule, one hours-and-minutes split
- Where: `(secs + 30) / 60` at `format.rs:128, 144, 162, 390`, `contrib.rs:91`, `goal.rs:100`, `bells.rs:457`, and as `>= 30` in `date_math.rs:110`; `log_card_total_secs` (`format.rs:396`) exists only to keep the day caption in step with the rounded cards. `hm_compact_key`, `hm_mins_key`, `hm_secs_key` (127-169) each repeat the split and the three-arm match; they already disagree under 30 s (`Empty` vs "0m"). Tests at 1274-1291 repeat the same way.
- Fix: one `round_mins(secs)` used everywhere; one private `hm(total_mins) -> HmKey` with each picker adding only its zero or 100 h rule.

#### #296 Count keys copy the plural rule
- Where: `format.rs:281` (`IntervalsCountKey`), 346 (`StreakKey`), 369 (`SessionCountKey`), 481 and 583-587 (`BellsCountKey`, a copy of the first without `None`).
- Problem: every `One` arm in both apps calls the same plural function as `Many` (gtk `timer/imp.rs:3340, 4507`, `log/imp.rs:684`, `preset_subtitle.rs:57`; android `invoke_streak_days(1)`, `invoke_n_enabled(1)`, `invoke_n_bells(1)`). The One/Many split is the English rule; ngettext already picks the form per language.
- Fix: keep only the real decisions (`StreakKey { Zero, Days(n) }`, `IntervalsCountKey { None, Enabled(n) }`, a plain count for sessions and bells).

#### #297 One read of the day totals for the Stats page
- Where: `insights.rs:51-80` (`input_from_db`, eight queries, plus a ninth for the running average that `totals` already holds), `goal.rs:89-96` (`today_from_db`, another day-totals read), the apps' mini stats re-read best streak and session count (gtk `stats/imp.rs:507-509`, android `ui.rs:2048-2050`), and the heatmap reads the whole history (gtk 196, android 2172).
- Fix: read the day totals once from the oldest window needed and derive streaks, the goal, the 30/60-day sums, the 7-day average, the heatmap and the chart from that list. Related: #220 (the query half: do together), #58 (the heatmap's `daily_totals`).

#### #298 One atomic-file-write helper
- Where: `data_io.rs:126-141` (`replace_file`), `sync/orchestrator.rs:1066-1081` (`write_whole_file`), `diag.rs:167-185` (`trim_to_tail`, which has drifted: no `sync_all`, leaves a `.log.tmp` behind on failure, and loses the 0600 mode, #87).
- Fix: one `write_atomically(path, suffix, write)` in core for all three. Supersedes the `write_whole_file` bullet of #261. Related: #87, #302.

#### #299 Settings readers that bypass `read_*`
- Where: `labels.rs:24-27` (`label_active_from_db` is `read_bool(db, key, mode == Guided)` by hand); `goal.rs:35-40` (also never clamps to `DAILY_GOAL_MAX`); `format.rs:633-646` (`prep_plan_from_db`/`parse_prep_secs`, repeated by gtk `timer/imp.rs:3249-3254` and android `ui.rs:3765-3769`); `settings_keys.rs:19-131`, ten per-mode key functions with the same three-arm match, and seven distinctness tests (391-570) where one test over all keys would also catch cross-family collisions; `set_last_mode` (302-307) and `guided::remember_guided` (47-54) share "write `sync_state` only if changed".
- Fix: the existing readers everywhere; a `prep_secs_from_db`; one key table per suffix; `Database::set_sync_state` skips equal values itself. Related: #281 (key constants for the bell family).

#### #300 The three CSV importers
- Where: `data_io.rs:280-442`: `parse_csv`, `parse_insighttimer_csv`, `parse_meditation_assistant_csv` each repeat the reader, `line_of`, the field closure, both `unreadable.push` branches and a full `Session { .. }` literal (312-321, 386-395, 429-438; again in tests at `insights.rs:308, 339`, `goal.rs:361`). `parse_insighttimer_csv(path, parse_dt)` takes a test seam as a public parameter (363; both apps pass `insighttimer_started_at`). `DataIoError::Parse` is never built, `From<rusqlite::Error>` has no user, and `From<DbError>` (70-88) restates every variant as English, which GTK mirrors to translate (`meditate-gtk/src/data_io.rs:17-63`). Export sorts with `local_iso_to_unix` inside the comparator and converts each row again (166-176); import lowercases every label name per row (245-255).
- Fix: one row loop with a per-row closure and a `Session::timer(start_iso, secs, notes)` constructor (fixes #305 on the way); a private `_with` seam; `DataIoError::Db(DbError)` and a typed key instead of English (fixes #310); `sort_by_cached_key`; a lowercase-name map.

#### #301 One decision, a different output per app
- `mini_stat_or_dash` (GTK) and `mini_stat_value` (Android) (`format.rs:430-444`): Android groups digits, GTK doesn't. `format_time_of_day` (GTK, 24 h only) and `time_of_day_key` (Android, follows the 12/24 h setting) (674-692).
- `NameValidity` (`naming.rs:132-170`) is only ever reduced with `is_savable()` (14 call sites, all trimming first); no collision hint is shown though the doc says so.
- `InsightKey::glyph` (`insights.rs:135-152`) says it keeps the icons portable, but Android has its own `insight_icon_id` (`ui.rs:2370`).
- Fix: GTK renders the typed keys and the string helpers go; `is_savable_name(s, taken) -> bool`; say in the glyph doc it is GTK-only.

#### #302 The diagnostics log
- Where: `diag.rs:168-187` reads the whole log into one `String` per line on every start, even when no trim is needed, and writes 2000 lines unbuffered when it trims; `log` (100-109) makes five or six write calls per line.
- Fix: `read_to_string`, find the start of the last 2000 lines, one `write_all` (fixes #307); `format!` the line and one `write_all`. Related: #298, #87.

#### #303 Smaller ones
- Chart period as a day count: the 90 and 365 thresholds twice (`date_math.rs:279/294` vs 324-332, "keep in lockstep"), `x_label_kind` matches 7 and 28 (149): take `ChartPeriod` and match on its unit. `aggregate_for_chart_period` and `ChartYAxisTicks` have no outside caller; the "smoke" loop at 364-372 asserts nothing.
- Lead-in: ms to seconds to ms (`settings_keys.rs:266-268, 285-289`), `100` hard-coded twice instead of `lead_in::MIN_MS`; Android rebuilds up to 480k noise samples and takes the db lock on every bell (`ui.rs:10376-10379`): cache by length as GTK caches its WAV.
- Time helpers: `Local.timestamp_opt(..).single()` six times (`format.rs:652-720`, `date_math.rs:102`, `time.rs:99-101`): one `time::local_dt`; the 0..=9999 year check and ISO parse three times (`time.rs:104, 139-151`, `data_io.rs:267`); the same `fixed_offset` closure twice (`time.rs:128, 181`); Log grouping converts each row twice (Android `app.rs:1055, 1070`); `"%Y-%m-%d"` has no shared constant.
- `local_folder.rs`: `by_name` (56-61) and the folder sort (41-42) allocate lowercase strings per comparison (`sort_by_cached_key`); `folder_title`/`folder_label` (65-86) repeat the path display; the `ListSort` string mapping (245-262) is written both ways.
- Android's vibration editor builds its own preview generation counter (`ui.rs:7679-7693, 10327-10336`) instead of `PreviewToggle`; the `preview.rs` doc (3-7) is stale.
- `insights.rs:191-208`: the percent change twice; `GoalStatus` (`goal.rs:255-281`) is derivable and its early return repeats the main literal.
- `seeds.rs`: the same `PresetEndBell` twice (153-159, 173-179), `"sound"`/`"both"` literals instead of `SignalMode`.
- `audio_files.rs:59-67` vs 86-98: two walkers over `<uuid>.*` with different id rules (`file_stem` vs `split('.')`).
- `db_open_failure_key` is dead and repeated in GTK: already #85.
- `format::ellipsize` (203) exists to cut text with "…", against the UI rule; its one user is #311.
- Docs on the wrong item: `format.rs:619-624` (`parse_prep_secs` on `prep_plan_from_db`), `661-663`, `416-419`; `announcement.rs` says both apps agree, only GTK uses it; `contrib.rs` still mentions the weekly goal.
- Could be private or merged: `labels::persist_active_for_mode` (an Android test already asserts nobody calls it), `contrib::CONTRIB_COLS/ROWS`, `lead_in::is_bluetooth_sink`, `format::prep_target_duration`; `idle_hero_label` into `idle_hero_label_from_modes`; the two `parse_hms_duration` arms; `format_time` and `format_duration_brief` share their split; the empty-uuid check in `labels::resolve_label_for_mode` (266-268) can't fire.

#### #304 Tests
- `time.rs:289-296` sleeps a real second to test `SystemTime`; delete it.
- `seeds.rs:221-273`: four copy-pasted uuid-distinctness tests, the bell one already misses `BUNDLED_WOODBLOCK_UUID`; 281-299 compare constants with themselves; so does `format.rs:1400-1410`.
- `timer.rs:283-293` and 310-337 test the same thing twice.

## Bugs found

(For OPEN-ISSUES.md once the maintainer agrees.)

### From slice 1
- **#241 Old name conflicts are redone on every sync.** `events.rs:808-829` (`reclaim_suffixed_names`) recomputes every row with a conflict suffix on every apply and replay, also when the conflict is permanent (larger uuid, holder still has the name); `claim_name` (798) then writes `sync.name_collision` to the log again each time. Two labels once named "Work" on two devices: from then on every sync re-resolves them and adds a log line. Fix: skip a row whose holder still has the name and whose uuid is the larger one.
- **#242 A peer's duration is cut or zeroed instead of clamped.** `sessions.rs:887`: `v["duration_secs"].as_u64().unwrap_or(0) as u32`; a corrupt payload with 4294967896 becomes 600, `600.0` becomes 0. Only a damaged or hostile peer gets there; same class as #172, fix together. Fix: clamp as `Session::from_unix` does.
- Already listed, raised again: an unknown `mode` from a newer peer rolls back the whole replay (#164); a payload that isn't JSON (#188).

### From slice 2
- **#243 Merging labels leaves presets pinned to the deleted one.** `labels.rs:287-318` (`merge_labels`) moves sessions and the per-mode default, but not a preset's `config_json` label uuid. Applying such a preset (`preset_config.rs:662-663`) writes the dead uuid into the mode's label setting; `resolve_label_for_mode` finds nothing, and every session from that preset is saved without a label, with no message. Fix: rewrite the presets' label uuid in the merge transaction (or map merged uuids to the survivor on lookup).
- **#244 Re-saving an unchanged preset can undo another device's edit.** `presets.rs:299` (`update_preset_config`, the GTK Override and Undo paths `meditate-gtk/src/presets.rs:300, 329`) and `325` (`update_preset_starred`) emit a full-row event even when nothing changed; `update_preset_name` and `set_setting` refuse to. The phone renames a preset, the laptop overrides it with the same config before syncing: the laptop's event carries the old name with a higher clock, and the rename is lost on both. Same class as #184. Same class as #248 (box breath, interval bells, guided star, vibration) and #184: fix all together through the guard in #234.
- **#245 Guided file names stay empty after going back to an older release and returning.** `schema.rs:47-54` fills `sessions.guided_file_name` only when the column is first added. New build, then the 26.10.1 release pulls a phone's Guided sessions (it doesn't know the column), then the new build again: the column exists, the fill is skipped, the cache version is unchanged, so those Log entries never get their title. Interval bells cover this case by recomputing on every open. Fix: #226, which replaces the one-shot fill (and with it #219's fill); don't patch the fill separately.
- **#246 A failed integrity check is only logged.** (Related: #100, #228, #258.) `mod.rs:308-313`: `quick_check` failures go to the diagnostics log and the app carries on, so the user keeps writing into a damaged file without knowing. AGENTS.md: a failure that only reaches the log is a bug. Not refusing to open is deliberate; telling the user is missing. Do together with #100 and #228 (where quick_check runs).
- **#247 Plausible: a bundled-pattern seed can fail on a user's own pattern name.** `seeds.rs:92-101` doesn't ignore a duplicate name, unlike labels and presets (74-78, 127-131). If the seed runs on a database that already has a user pattern called "Pulse" (an upgrade from before the seed existed), the seed fails at startup. Not reproduced.
- Already listed, raised again: an unknown preset `mode` from a newer peer fails the CHECK and rolls back the replay (`presets.rs:459`): add presets to #164.
- Checked and not kept: two connections minting two device ids (`device.rs:14-31`): the startup seeds mint the row before any sync connection exists.

### From slice 3
- **#248 Writes that change nothing still send a full-row event, which can undo another device's edit.** No unchanged guard in `set_box_breath_phase` (`box_breath_phases.rs:82`; its doc says so on purpose), `update_interval_bell` (`interval_bells.rs:165`), `set_interval_bell_enabled`, `set_guided_file_starred` (`guided_files.rs:266`), and `update_vibration_pattern` (that one is #184). `rename_bell_sound` and `rename_guided_file` skip unchanged writes for exactly this reason. Device B changes a Box Breath phase, device A writes the same old values before syncing: A's event wins and B's change is gone. Applying a Box Breath preset writes all four phases (`preset_config.rs:700-714`). Reported, not traced: the GTK timer page rewrites a phase when it is shown again. Same class as #244 (presets) and #184 (vibration patterns): fix all together through the guard in #234.
- **#249 Plausible: a session saved twice after a time zone change.** `session_in_progress.rs:178` deletes the recovery snapshot only if its `start_iso` matches the saved session's string exactly. Both are formatted from the same unix time in the local zone at different moments; change the zone between the last snapshot and Save (travel), and the snapshot stays, so the next launch saves the session again. Fix: compare on a session token or the unix start. Not reproduced.
- **#250 Plausible: a stale length kept for a replaced local file.** `local_guided_files.rs:141` (`set_local_file_duration`) writes a length without checking the file is still the one it was read from; a rescan in between resets it to NULL and the late write stores the old length for good. Fix: `AND size = ?3 AND mtime = ?4`. Not reproduced.
- **#251 Plausible: a corrupt vibration pattern plays nothing, silently.** (Same class as #291.) `vibration_patterns.rs:60` turns unparseable `intensities_json` into an empty envelope (`unwrap_or_default`), while renaming the same row fails with `Decode`. Only a damaged row or a newer peer's format gets there; the user hears no cue and sees no message.
- Already listed, raised again: unknown values from a newer peer in the CHECK columns (interval bell `kind` and `signal_mode`, bell `category`, `chart_kind`, box breath `signal_mode`) roll back the replay (#164).
- Checked and not kept: presets' name check without `COLLATE NOCASE` (`presets.rs:129`): the column itself is `COLLATE NOCASE` (`schema.rs:192`), so the comparison already ignores case.

### From slice 4
- **#263 A push-local recovery cut short leaves the server empty, and the status says OK.**
  - Where: `settings.rs:252-260` (`prepare_push_local_recovery`). Four separate writes and no rerun marker.
  - Scenario: after "remote data lost" the user picks "push local". The app dies or gets SQLITE_BUSY after the known batches are wiped but before the events are flagged unsynced. On the next sync, the data-lost check is skipped (nothing known) and nothing is pending, so nothing is pushed and the error is cleared. The server stays empty and the status shows success.
  - Fix: #255.
- **#264 A temporary server error parks an audio file until the user taps Retry.**
  - Where: `orchestrator.rs:806` (`push_file`). It records every `WebDavError::Server` status as a refusal, including 500, 502, 503 and 504, and a failed MOVE takes the same branch.
  - Scenario: an upload during Nextcloud maintenance (503) or a proxy timeout (504) is skipped by every later automatic sync. The file stays on this device until the user notices "not uploaded".
  - Fix: only lasting statuses (413, 415, 403 and the like) count as refusals; 5xx fails the sync and is retried.
- **#265 A remote audio file over the cap is downloaded again on every sync and dropped silently.**
  - Where: `orchestrator.rs:748, 904`. `ResponseTooLarge` and the second size check just `continue`; nothing is recorded or shown.
  - Scenario: a 100 to 101 MB guided file on the server (put there over WebDAV) costs about 101 MB of mobile data on every save. Larger ones stream 101 MB before stopping.
  - Fix: record it as known-and-skipped and show it in the sync status, as refused uploads are. Do with #253.
- **#266 A full disk is reported as an unreadable server reply.**
  - Where: local write and create-dir failures map to `UnreadableReply` (`orchestrator.rs:730, 892, 1077`, `runner.rs:144`).
  - Scenario: the phone's storage fills during a pull; the status points at the server.
  - Fix: #257.
- **#267 A rate limit on anything but PUT fails the sync, and on MOVE it throws the upload away.**
  - Where: `orchestrator.rs:1025`. After a successful PUT, a 429 on the MOVE deletes the `.tmp`, so the next sync uploads the whole file (up to 100 MB) again, possibly repeatedly under throttling. A 429 on the first PROPFIND fails the sync.
  - Fix: #252.
- **#268 Re-saving the same server with a trailing slash re-uploads everything.**
  - Where: `settings.rs:73` (`adopt_account`) compares the URL as text, and `prepare_test` (`credentials.rs:185`) only trims spaces.
  - Scenario: `https://cloud.example.org/.../u` re-saved as `.../u/` counts as a new account. All trackers are wiped, the whole history is flagged unsynced, and the next sync re-reads every batch and re-uploads every event and audio file to the same server.
  - Fix: normalise the URL (trailing slash, host case) before saving and comparing.
- Already listed, raised again: one file in `events/` that isn't a batch (a stray `backup__notes.json`, or a name with `%` or `#`) stops every later pull (#164). The file name isn't checked against the documented `<lamport:014>__<uuid>.json`; add that to #164.
- Plausible, smaller, not yet checked: if `record_outcome` fails, it hides a successful sync (`runner.rs:69`); in `sync_with_progress` a push error replaces the pull error before it (`orchestrator.rs:492`); two devices compacting at once can overwrite each other's `compacted.json`.

### From slice 5
- **#277 A session stopped while paused reports "Paused", not "Done".**
  - Where: `mod.rs:849-852`. `ui_state()` checks `is_paused` before the phase, and `stop()` (446) leaves `is_paused` set; `end_buttons()` (834) checks `Stopped` first, so the two disagree.
  - Scenario: pause, then Save. If the ended page stays (a failed save), GTK's MPRIS shows a resumable session and its play key calls resume on an ended session, and the Box Breath frame callback (`window/imp.rs:499`, runs while Running or Paused) keeps drawing.
  - Fix: #273 with #270; at least match `Stopped` first.
- **#278 When a guided track ends early, a bell due before the planned end rings just after the end bell.**
  - Where: `enter_overtime` (`mod.rs:517-523`) goes to `reach_end` without the `skip_due` loop that only `tick_running` runs (`627-633`).
  - Scenario: a guided file probed at 900 s with an interval ring due at 899 s; the audio ends at 899.4 s, the end bell rings, and the next overtime tick rings the interval bell a second later.
  - Fix: move the `skip_due` loop into `reach_end` (with `max(elapsed, target)`); do with #270.
- **#279 After a suspend, every missed fixed bell rings at once.**
  - Where: `fire_due_bells` (`mod.rs:1151-1174`) with `ActiveBell::advance` (`bells.rs:267`): an interval bell rings once for its latest due time after a jump, but each fixed bell flips and fires on its own.
  - Scenario: a 60 min Timer with fixed bells at 10, 20 and 30 min; the phone sleeps from minute 5 to 35. The first tick fires three bells (plus any interval bell) on the stacking channel.
  - Fix: after a jump, ring only the latest due fixed bell and skip the earlier ones, as interval bells do. Related: slice 6 (bells).
- **#280 The bell counter counts the starting bell as heard when it never rang.**
  - Where: `bell_ring` (`mod.rs:938`): `rung += !preparing`.
  - Scenario: the per-mode signal override mutes it (`starting_bell_effect` returns nothing), or a suspend through prep past the end skips it (`tick_prep` 586-592); the counter still shows it as heard, "1 of N" before any bell. The end bell has the same blind spot for the override.
  - Fix: record in the session whether the starting and end bells actually fired, and count that.
- Plausible, not yet checked to the end: `stop()` during Prep records the prep seconds as the session length (`mod.rs:446-457`), against `end_buttons`' rule "Preparation seconds are never saved as meditation"; the apps show no Save during prep, but GTK's `session_ended` writes the value into the in-progress snapshot. Fix in core: `stop` from Prep ends with 0.
- Checked and not kept: "Box Breath's first cue is lost when the first phase is short": `In` is at least 1 s (`breath.rs` `phase_min_secs(0) == 1`) and Box Breath ticks every 200 ms in both apps.

### From slice 6
- **#291 A read error while saving a preset stores it without its interval bells, silently.** (Same class as #308.)
  - Where: `preset_config.rs:581-586` (`snapshot`: `list_interval_bells().unwrap_or_default()`); the same at session start, `bells.rs:800`.
  - Scenario: the database is busy during "Save current setup"; the preset is stored with an empty bell list and the bells switched on. Applying it later switches every library bell off. At session start the same error starts a session with no interval bells. No message, not even a log line.
  - Fix: pass the error on (`snapshot` returns `Result`); the apps already show "Save failed". Same class as #251 (an unreadable row read as empty).
- **#292 Applying a preset saved without a label keeps whatever label is picked now.**
  - Where: `preset_config.rs:662-664`: the label is only written when the preset has one, though the field's doc (74-75) says `None` means "the mode's default".
  - Scenario: a preset saved with no label picked; the user picks "Walking" and applies the preset; the session is tagged "Walking", and the preset no longer shows as active.
  - Fix: write the mode default (or clear the setting) when `uuid` is `None`.
- **#293 The Bells subtitle counts bells that won't ring.**
  - Where: `bells.rs:771-778` (`interval_bells_count`) filters only the stopwatch case; `build_active_bells` also drops a fixed bell at or past the target, a "before end" offset at or past it, and a repeating bell longer than the session.
  - Scenario: a 10 min countdown with one bell "at 30 min": the row says "1 interval bell", nothing rings.
  - Fix: count what `build_active_bells` would build for the current target.
- Already listed: applying a preset is not one transaction: OPEN-ISSUES #193 (and #177, a duplicate of it: merge them). Both apps re-apply the old setup on failure (gtk `timer/imp.rs:3895-3902`, android `ui.rs:1809`), which can fail the same way and leaves inserted bells behind.
- Plausible, not checked to the end:
  - `bell_volume::write` returns `Ok` and saves nothing when a Box Breath phase row is missing (`bell_volume.rs:222`). Rows are seeded; only a damaged database gets there.
  - A vibration pattern longer than about 463 s overflows `(2 * i + 1) * duration_ms` in u32 (`vibration.rs:145`); core has no duration bound, and only a damaged or hostile peer could send one. Same class as #172 and #242.
- Checked and not kept: "`phase_at` crashes on a zero cycle in release": only test code builds that pattern; the wrong doc is in #286.

### From slice 7
- **#305 One unreadable line stops the whole import.**
  - Where: `let rec = record?;` in all three importers (`data_io.rs:284, 367, 413`).
  - Scenario: a file with 500 good rows and one row the csv reader rejects (a stray quote, invalid UTF-8). The import fails with "CSV error"; the question "import the readable part?" that `ParsedImport` exists for is never asked.
  - Fix: push the record as unreadable and go on; do with #300.
- **#306 A Box Breath pattern is saved in five steps.**
  - Where: `settings_keys.rs:243-253` (`set_breathing`): five `set_setting` calls, each its own transaction and synced event.
  - Scenario: a write fails after the first two (locked or full database); every device ends up with a mixed old and new pattern, and the message doesn't say what landed.
  - Fix: one transaction (a `set_settings(&[(k, v)])`). Same class as OPEN-ISSUES #193 (presets); the savepoint helper there covers both.
- **#307 One damaged line in the diagnostics log stops its trimming for good, or drops the newest lines.**
  - Where: `diag.rs:169` reads with `map_while(Result::ok)`, which stops at the first line that isn't UTF-8.
  - Scenario: a kill mid-write leaves a cut multi-byte character. With fewer than 2000 lines before it, the trim returns early at every start and the log grows without limit; with more, the rewrite keeps only lines from before the bad byte and drops everything newer, the part needed for triage.
  - Fix: #302 (read bytes, split on newlines).
- **#308 A failed read shows on the Stats page as "no data".**
  - Where: `insights.rs:55-80` turns every read error into 0 or empty (`unwrap_or_default`, `unwrap_or(0)`), not even logged; `goal::today_from_db` the same.
  - Scenario: the database is locked at refresh; Stats shows an empty history or a 0-minute goal ring as if the user had never meditated.
  - Fix: return the error and show it; do with #297. Same class as #291 and #251.
- **#309 The lead-in length rounds twice.**
  - Where: `settings_keys.rs:287`: rounds to whole ms, then half up to 10 ms.
  - Scenario: a typed 0.1049 s becomes 105 ms, then 110 ms; rounding once gives 100 ms. Do with the lead-in bullet of #303.
- **#310 A corrupt database row during export is reported as a "CSV error".**
  - Where: `data_io.rs:84` maps `DbError::Decode` to `DataIoError::Csv`. Fix: #300.
- **#311 The GTK "Deleted" toast cuts a guided file's name with "…".**
  - Where: gtk `guided.rs:1133` uses `format::ellipsize(.., 28)`, against "no cut-off text and no ellipsis". Fix: show the full name (the toast wraps) and delete `ellipsize`.
- Small: folders whose names differ only in case swap places between starts (`local_folder.rs:41-42`: collected from a `HashMap`, sorted by lowercase name with no tie-break; files have one).
- Plausible, not checked to the end: the daily goal is never capped at 1440 on write or read (`goal.rs:35-40`).
- Checked and not kept: "GTK's `d[5..7]` can panic on a short date": core always writes ten characters. Goes away with #294 anyway.

## S. Cleanup (moved from OPEN-ISSUES.md on 2026-11-10)

Re-checked against 9d3ecf6 (2026-10-22); line numbers below are current. Base suites green then (core 1390, android 261, gtk 162). Open decisions before a proposal: #197 plain show/hide without the slide; #85 whether a ringing end bell keeps ringing when its settings change; #171 comment only or `synchronous=FULL` around Save and Edit; #167/#175 protected files (ask). Proposed to drop: merging the five `(Context)V` JNI helpers (the batch-R JNI table test only sees `helper(app, "method")` calls and would need a rewrite), moving `snapshot_setup_json` timing to core (Android reads the UI and a Cell, core the DB), and `tick_mode` (not dead: read at `timer/imp.rs:2313, 2814`).

### #58 Dead and duplicated code (core and Android)
- Core: `SessionPhase::Paused` (`session/mod.rs:145`) is never created; delete it, its arms at 488, 781, 832, the comment at 486-487 and ARCHITECTURE.md:207. (Line numbers are old: now 218-220, arms 563, 856, 981, 1019. Superseded by #273, which does it with the rest of pause.) `daily_totals()` / `get_daily_totals_from_db` (related: #297) (`db/sessions.rs:179-181, 735-761`, tests 3472, 3480, export `db/mod.rs:91`) feed only the 91-cell heatmaps (gtk `stats/imp.rs:196`, android `ui.rs:2026`); call the `_since` version with the grid's first cell (`cur_week_start - 12 weeks`, locale week start) and delete it.
- Android: 59 `let _ = weak.clone();` in `ui.rs` (first at 5220); in 7 closures it is the only use, so their `let weak = ui.as_weak();` goes too (5209/5220, 5310/5317, 5603/5610, 7251/7255, 9064/9069, 9073/9078, 9187/9195). 4 `let _ = current_mode.get();` (5045, 7908, 7978, 8029). Other no-ops: `let _ = &weak;` 9018, `let _ = value;` 6319, 7539, 7585, `let _ = v;` 6394, 6406, `let _ = i;` 6420. `AppState::remaining` (`app.rs:640`) is test-only. Kotlin `MeditateKeychain.clearPassword` (`MeditateKeychain.kt:104-106`) has no caller.
- gtk `keychain::delete_password` (141-156) with `Backend::delete` (313-325) has no caller (edit the source-text test at `keychain.rs:422`); core `settings::clear_nextcloud_account` (`sync/settings.rs:91-95`) has none either; then fix the comment at android `keychain.rs:63-67`. Stale `chooser_bell_volume` doc (`ui.rs:10016-10018`, talks of integer targets 0..6).
- `service.rs::call` (105-152) repeats `jni_call::load_class` (112-125).

### #61 GTK dead code and the duplicated transcode pipeline
- Dead: in `bells.rs` the `populating` guards at 760 and 872 can only fire on re-entry (it is set at 770/773 for the guard at 788), and the comment at 926-930 with `let _ = populating;` (931) is stale. Unused `toast_slot` (`presets.rs:332, 357-359`, one call site near 239); `chart_h` (`stats/imp.rs:443, 474`); `area` (`stats/imp.rs:669, 679`, caller 101); `let _ = BellSoundCategory::General` (`db/mod.rs:431`; check the import). Half-deleted "Throwaway" comments at `timer/imp.rs:1175-1180` and 153 (`setup_vibration_proto` no longer exists).
- Duplicate: `sounds.rs:742-846` (`transcode_to_ogg`) and `guided.rs:461-556` (`transcode_to_ogg_preserve_channels`) differ only in the `audioloudnorm` branch and guided's cancel-aware 250 ms `timed_pop` loop. `audioloudnorm` never runs (not in the runtime, the manifest adds no GStreamer module): delete `sounds.rs:763-783, 795-801`; then one `transcode_to_ogg(source, dest, cancel: Option<&AtomicBool>)` with the polling loop. Fix guided's doc (452-455, claims the sounds version forces mono). Tests: `sounds.rs:1019-1045`, `guided.rs:1601-1641`.
- `timer/imp.rs` (related: #281, one reader per bell slot): the `starting_bell_sound` read with its `"bowl"` fallback bound to `_starting_bell_sound_legacy` (3110-3112, 3133, 3141, 3167); `apply_preferred_label_for_mode(_mode)` (4451, callers 1891, 3230); `build_breathing_setup` (4268) and `breath_elapsed` (3040; keep the public wrapper `timer/mod.rs:126`) only pass through; 2484-2486 and 2717-2719 hand-copy `From<TimerMode>` (55-61). `PKGDATADIR` (`config.rs:6-7`, `#[allow(dead_code)]`) has no reader; it is also set in `build.rs:49-50`, `src/meson.build:14` and the Flatpak manifest (37).

### #85 Dead code, duplicates and a test that cannot fail
- Core: `Effect::EndPrep` (`effect.rs:21-24`, 149; emitted `mod.rs:508`, mentioned 474; asserts 1132, 1144, 1705, 1989, 2312; ARCHITECTURE.md:241; gtk comments `timer/imp.rs:2243, 2845, 2863, 4113`) is never consumed (superseded by #271, which removes it with the other state-only effects). `tick_box_breath` (`mod.rs:604-667`) calls `fire_due_bells` at 658-665 though core never gives Box Breath bells (test `box_breath_session_fires_interval_bells` 1541 injects them). `db_open_failure_key` (`format.rs:308-315`, tests 945-961) is unused and repeated in gtk `application.rs:178-187` (which matches gtk's own `DbError`). `Stopwatch` serde derives (`timer.rs:5-12`) and the restart test (86-113) are unused and assert a false claim (related: #273 touches `Stopwatch`). `list_box_breath_phases` (`box_breath_phases.rs:17-48`) repeats `get_box_breath_phase` (52-76; related: #285, whose callers should all use the list); seed uuids hard-coded at 142, 144 (use `seeds::BUNDLED_BOWL_UUID`, `BUNDLED_PATTERN_PULSE_UUID`). `list_bell_sounds_for_category` (`bell_sounds.rs:240-264`) copies `list_bell_sounds` (206-233) except the WHERE. `count_labels_from_db` (`labels.rs:65`, export `db/mod.rs:80`) is test-only.
- GTK: `play_starting_sound` (`sound.rs:291-320`) has no caller. `preload_end_bell` (270-286; calls `timer/imp.rs:680, 727, 1219`, `window/imp.rs:114`) builds media that `play_end_bell_uuid` (322) never uses; its only effect is stopping a ringing end bell when its settings change (decide). Stale preload docs: `sound.rs:10-15, 289-290, 331-332`, `timer/imp.rs:677-679, 1218`.
- Android: the label rename and delete handlers still repeat part of `refresh_after_label_change` (`ui.rs:7968-7971`, 8016-8022 vs 2377-2387). `read_signal_mode_for_mode` (`ui.rs:542-550`) repeats core `bells::signal_mode_override_from_db`.
- Test: `bell_rng_state_advances_deterministically_from_seed` (`mod.rs:1553-1584`) uses `jitter_secs: 0` (helper at 1090), so the RNG never moves a ring time; set a jitter and assert a different seed differs, or delete it.
- Android: `validate_label_name` (2485) is `validate_rename_label_name(n, 0)` (3043); `bell_name_taken` (1535) is `bell_sound_name_taken(n, "")` (3933); `read_global_setting` (3402-3406) is core `read_str` and its four `== "true"` callers (3541, 3594, 3619, 3719) are `read_bool`; the label-name map is built three times (609, 716, 763; a helper must take `&db`, a second `lock_db()` deadlocks); 4256-4259 repeats `push_session_length_to_ui` (2435-2438); 2644-2649 repeats `refresh_setup_label_name` (1795-1802).

### #87 Trimming the diagnostics log drops its 0600 permission
Related: #298 (one atomic-write helper keeps the mode) and #307.
- Where: `meditate-core/src/diag.rs:178` (`File::create(&tmp)` in `trim_to_tail`, renamed over the log at 184) vs `init` 76-82. `init` trims first, so the log stays 0644 for good.
- Fix: open the temp copy with `OpenOptions` write/create/truncate and `.mode(0o600)`, as `init` does.

### #157 Wrong or misplaced docs and comments
- `orchestrator.rs:373` says the batch file is `<batch_uuid>.json`; it is `<min_lamport:014>__<batch_uuid>.json` (as the header at 7 says).
- gtk `db/mod.rs:167-171` says new bundled sounds reach existing installs, but the seed is one-shot (`seeds.rs:23-37`); it also sits on `bundled_bell_resource` instead of `BUNDLED_BELL_SOUNDS` (182).
- core `db/mod.rs:191` says `busy_timeout=8s`; the app uses `APP_BUSY_TIMEOUT` = 1 s (33), 8 s is sync's (35).
- android `ui.rs`, docs on the wrong item: 492-496 (an in-flight session struct that no longer exists: delete), 803-823 (belongs on `try_widget_deep_link` 865; also say it drops the tap on an unsaved ended session and brings Timer forward), 1488-1492 (belongs on `present_guided_import_dialog` 1551), 3130-3135 (belongs on `delete_label_impact_text` 3168). False claims: 1805-1806 (sync is real, uses `is_in_flight`), 1862-1867 (renders through `Tr`), 4123-4129 (Timer length persists to the DB), 4274-4275 (Guided ships), 6189-6192 (keep-awake is real, `screen.rs:23`), 6226-6229 (the engines exist), 10077-10078 (asset paths done, `sounds.rs:166`).

### #167 Linux packaging: a Rust floor that's too low
- Where: `build.sh:77-78` (accepts 1.85); `meditate-gtk/Cargo.toml:6` (`rust-version = "1.75"`); `clippy.toml:1-5` (`msrv = "1.75"`, also in its comment).
- Problem: the locked gtk4 0.11.2, glib and gio 0.22.5 need 1.92, so the build fails halfway. The toolchain pin is 1.95, so nothing else moves.
- Fix: 1.92 in all three (Cargo.toml: ask first).

### #197 GTK: bell rows in a revealer warn on GNOME 51
- Where: `meditate-gtk/data/ui/timer_view.blp`: start bell 431-470 and end bell 533-570 (sound, volume, pattern), and the four Box Breath phases (620/633/641, 668/681/689, 716/729/737, 764/777/785): 18 `Gtk.Revealer`s inside `Adw.ExpanderRow`s, matching the 18 warnings. Toggled in `timer/imp.rs` 1567-1717 (`build_signal_mode_toggle_widget`, `apply_signal_mode_state`, the phase versions).
- Problem: libadwaita in GNOME 51 logs "AdwActionRow placed into GtkRevealer, can only be placed into GtkListBox" at every start. GNOME 50 has no such check.
- Fix: rows directly in the expander, `set_visible` as `bells.rs:879-881` does, volume rows follow their sound row; delete `attach_revealer_row_click` (1802-1819, attached 691-694, 1360-1361) or every tap opens the chooser twice; update `every_setup_bell_has_a_volume_row` (`timer/mod.rs:193-221`) and the docs at 685-690, 1323-1324. Loses the 220 ms slide.

### #171 The `synchronous=NORMAL` comment promises safety it doesn't have
- Where: `meditate-core/src/db/mod.rs:235-248` (pragma 249).
- Problem: "events are append-only and idempotent on re-sync" is only true for events already uploaded; a power cut can roll back a just-saved session that exists nowhere else.
- Fix: correct the comment; or, if wanted, `PRAGMA synchronous=FULL` around Save and Edit only.

### #175 Leftover config
- `meditate-android/manifest.yaml` is read by nothing (build.sh, rust-build.sh, Gradle, CI, F-Droid metadata); mentioned in `meditate-android/Cargo.toml:81-85`, `android/app/build.gradle:5` and AGENTS.md's protected list. `meditate-gtk/src/meson.build:3-9` sets `cargo_profile_args`, never used. `build-aux/io.github.janekbt.Meditate.json:25-26` talk to the Notifications services directly, but the app only uses `gio::Notification` (`timer/imp.rs:2426-2433`), which goes through the portal in Flatpak.
- Fix: delete them and check that notifications still show in the Flatpak. Cargo.toml, build.gradle and manifest.yaml are protected build environment: ask first.

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
