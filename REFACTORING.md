# Refactoring

Everything that would make the code simpler, smaller or faster without changing what the app does, in one place. Bugs stay in OPEN-ISSUES.md; a bug found here goes to "Bugs found" below until the maintainer moves it there. Numbers continue the OPEN-ISSUES numbering, so commit messages stay unambiguous.

Work goes package by package ("Order of work" below), each as a /spec change: tests that pin the behaviour first, then the refactor, then a phone check. Every item gets done; only the order is planned.

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
| [x] | 8 | gtk `timer/`, `timer_view.blp` |
| [x] | 9 | gtk window, log, stats, application, gtk `db/`, their `.blp` |
| [x] | 10 | gtk preferences, presets, bells, sounds |
| [x] | 11 | gtk vibrations, vibration editor, guided, labels, keychain, local folder, the rest |
| [x] | 12 | android `ui.rs`, first half |
| [x] | 13 | android `ui.rs`, second half |
| [x] | 14 | android `app.rs` |
| [x] | 15 | android Kotlin and the other `src/` files |
| [x] | 16 | android `main.slint`, first half |
| [x] | 17 | android `main.slint`, second half |
| [x] | 18 | across apps: logic in both shells that belongs in core |

## Order of work

Every item gets done; this is only the order. A package is one /spec change, or several when it is large, and goes with the bug batch named in OPEN-ISSUES.md when there is one, so one device test covers both. Within a package, do the general item first: a "Related" or folded-in step is part of it, not a change of its own. Tick a package when its last item is committed.

| Done | Package | Items | Do with bug batch |
|:---:|---|---|---|
| [ ] | P1. Core write paths | #234, #225, #235, #237, #215, #216, #217, #226, #230, #231, #370 | CC, K3 |
| [ ] | P2. Refresh after writes and syncs | #331, #378, #397, #418, #484, #485, #316, #283, #353, #468, #327 | Q, EE |
| [ ] | P3. Failure reporting and toasts | #320, #357, #373, #328 | EE, FF |
| [ ] | P4. Names and dialogs | #358, #374, #415, #486, #465 | DD, FF |
| [ ] | P5. Session engine | #269, #270, #271, #272, #273, #274, #275, #313, #394, #395, #429, #430 | HH |
| [ ] | P6. Bells, presets and breath | #281, #282, #284, #285, #286, #314, #354, #414, #466, #400, #455, #317, #289 | HH, K3 |
| [ ] | P7. Guided files, Local Folder and imports | #300, #355, #356, #371, #372, #402, #440, #480, #481, #439, #375, #288 | JJ, II |
| [ ] | P8. Sync core | #252, #253, #254, #255, #256, #257, #258, #259, #260, #261, #403 | W, Q |
| [ ] | P9. Stats and Log | #294, #295, #296, #297, #329, #330, #332, #334, #335, #398, #456, #457, #220, #236 | T |
| [ ] | P10. Typed texts and values | #301, #482, #431, #222 | T |
| [ ] | P11. Startup | #228, #336, #419, #483 | EE |
| [ ] | P12. Android screens (Slint) | #453, #454, #458, #459, #460, #461, #464, #467, #469, #470, #471, #404, #417 | GG, T, LL |
| [ ] | P13. Android Rust and Kotlin structure | #396, #399, #401, #405, #416, #420, #421, #433, #438, #441, #442 | FF, KK |
| [ ] | P14. GTK structure | #312, #315, #318, #319, #321, #337, #338, #352, #359, #360, #368, #369, #376, #377, #379, #287, #333, #302, #298 | MM, II |
| [ ] | P15. Core speed and smaller ones | #218, #219, #221, #223, #227, #229, #232, #238, #239, #299, #303, #487 |  |
| [ ] | P16. Tests | #224, #233, #240, #262, #276, #290, #304, #322, #339, #361, #380, #432 | with each package that touches them |
| [ ] | P17. Cleanup from the old list | #58, #61, #85, #87, #157, #167, #171, #175, #197, R12 |  |

## Findings

### Slice 1: core `db/sessions.rs`, `db/events.rs` (swept 2026-11-10 at 3c845d9)

/simplify (four angles) and /code-review (high) on the whole files; every item below re-checked against the code. Ordered by gain.

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
- Related: #335 (GTK commits pending deletes one transaction each: add a batch delete here).
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
- Fix: per table, build the payload once from the row (`*_payload(&row)`), `emit_event`, then call the existing `recompute_*` in the same transaction; the write SQL in insert, rename, star and update goes away and local rows are built the way peers build them. Insert and rename keep a name check first (`is_*_name_taken_from_db`), since recompute suffixes a clash instead of returning `Duplicate*`. The general form of #225 (presets) and of the session payload, which this item now holds (folded in from #214, 2026-11-10).
- Sessions (was #214): the eight-field sync payload is written out three times, `sessions.rs:458-467` (`insert_session_tx_less`), `576-585` (`insert_sessions_in`), `692-701` (`update_session_in`); `guided_file_name` had to be added to all three, and missing one would sync without the field and lose it on peers. The smaller step: `fn session_payload(uuid, &Session, label_uuid) -> String` used by all three, with the field set byte-identical (key order doesn't matter to readers). Rejected once as part of R13 ("too little gain for touching the sync payload"); all four /simplify angles and /code-review raised it again.

#### #235 One row reader per table
- Related: #401 (label lookups by id in both apps).
- Related: #375 (a keyed bell-sound lookup for every bell played).
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
- Related: #403 (Android's own network retry).
- Where: `orchestrator.rs:949-1000` (`put_with_rate_limit_retry`), `1015-1035` (`put_atomic_with_rate_limit_retry`), all of `backoff.rs` (266 lines).
- Problem: only PUT retries a 429; PROPFIND, GET, MOVE, DELETE and MKCOL don't (see #267). `BackoffState` is built fresh per PUT and dropped, so its window, reset and counters reduce to one formula; three of its eleven tests cover paths that can't happen.
- Fix: retry inside `HttpWebDav::send` for every verb (sleep `retry_after`, else `min(1 << n, 30)` s, at most 8 tries); delete `put_with_rate_limit_retry` and most of `backoff.rs`. Fixes #267.

#### #253 One pull helper, one cap, for bells and guided files
- Where: pull loops `orchestrator.rs:716-766` and `885-918`; push `824-848` and `924-946` (push half-shared through `push_file` 782); paths and dirs `659-676`, `858-875`; caps `67` (`MAX_SOUND_GET_BYTES = 11 MB`, hard-coded) and `73`, and the second size check after the read (`758`, `910`).
- Problem: every pull rule is written twice (create dir, skip if present, NotFound and TooLarge mean skip, cap check, write, record known). Two caps for one limit; the bell one won't follow `sound::MAX_CUSTOM_BELL_BYTES` if that changes.
- Fix: `pull_file(local, remote, cap)` mirroring `push_file`, or a small per-kind descriptor (dirs, paths, cap, known-set) so each loop exists once; `get(&remote, MAX_CUSTOM_*_BYTES)` and drop the second check. Do together with #265.

#### #254 The sync failure state
- Related: #333 (the GTK indicator polls it every 2 s).
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
- Related: #403 (Android parks the result as a number).
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
- Related: #430 (Android's `completion_notice_secs`, GTK keyed by hand).
- Where:
  - Core: five paths set `phase = Stopped` and `final_duration_secs = Some(..)` by hand (`mod.rs:451, 483, 500, 535, 694`); `stop` and `add_overtime_and_finish` are the same body. The end is signalled as `EndSession`, `EndBoxBreath` or `AutoFinish`, all with the same `duration_secs`, and stored again in `final_duration_secs`. Box Breath's end (`691-705`) repeats `reach_end`'s Auto-Finish and end-bell rule (`528-544`). `finish_overtime` (`476-482`) re-derives `shape.target_secs()` by hand.
  - Android: `ended_duration_secs` and `completion_notice_secs` scan the effects (`app.rs:189-215`); the "was active, now finished: save and notify" block is in the tick handler and again in the guided-track-end handler (`ui.rs:5137-5165`, `5064-5080`); `finish_overtime`/`add_overtime` re-check `ui_state() == Overtime` (`app.rs:708-735`) although core already ignores them outside Overtime.
  - GTK: reads `final_duration_secs()` (`timer/imp.rs:2579`), `core_session_end` scans only `EndSession` (3137-3142), and it branches on `AutoFinish` / `EndBoxBreath` / `EnterOvertime` in three places (2262/2268, 3007/3011, 3044/3047). Both apps call `completion_duration_secs()` at the end for the notification (gtk 2557, 3089; android `app.rs:213`, `ui.rs:5183`), so "notify only when it ended by itself" is decided in each app.
- Fix: one private `fn end(&mut self, secs, how)` in core; one `Effect::Ended { duration_secs, how: Hand | Natural | AutoFinish }` (`EndBoxBreath` keeps its meaning as `Natural` in Box Breath: GTK keeps that page up with Save); `phase: Stopped { duration_secs }` instead of the second field; the notification decision as a core field of the end. Both apps then go to "ended" when the phase is `Stopped` after any call. Also fixes #277 and #278.
- Related: #273 (do together), #271.

#### #271 Effects that only repeat state
- Where: `effect.rs:20, 24, 84, 96` (`UpdateDisplay`, `EndPrep`, `UpdateOvertimeLabel`, `EnterOvertime`); `mod.rs:62-80, 798-810` (`TickOutcome`, `tick_summary`).
- Problem: all four can be read from `display_secs(now)`, `ui_state()` and `end_buttons()`, which core already offers. `tick_summary` exists to fold three of them back into state and has one caller (gtk `timer/imp.rs:3034`); gtk `tick_prep` (2986) and `tick_overtime` (3104-3114) fold them by hand, Android draws from `display_secs` (`app.rs:833`). So the display rule is kept twice in core: in `display_secs` and in the `UpdateDisplay` built by `tick_prep`, `tick_running` and `tick_box_breath` (which pushes `elapsed.as_secs()` itself, 729). "Seconds left, rounded up" is written inline four times (573-603, 872-879, 1059-1063, 970-984). `TickOutcome::overtime_delta` is never read; the free fn `ui_state(Option<&Session>)` (58-60) is test-only.
- Fix: effects carry only what an app must do (bells, stop signals, stop guided audio, the end, #270); after each call the apps redraw from the pull API. Delete `TickOutcome`, `tick_summary`, the four variants and the effect-scanning loops in both apps; one `secs_left(target, elapsed)` next to `ceil_secs`. `EndPrep` is never consumed; with it go its asserts (old numbers 1132, 1144, 1705, 1989, 2312), ARCHITECTURE.md:241 and the gtk comments (`timer/imp.rs:2243, 2845, 2863, 4113`) (folded in from #85).

#### #272 The session shape is built in each app
- Related: #312 (the other decisions the GTK timer page makes).
- Where: gtk `timer/imp.rs:2191-2292`, android `ui.rs:4463-4510` (its comment: "Mirrors GTK's SessionShape::Guided build"); core `settings.rs:28-72` has `mode()`, `stopwatch_on()` and `target_secs()` but no constructor.
- Problem: both apps repeat the decision tree, including `pattern.cycle_aligned_target_secs(..)` for a Box Breath countdown and "Guided: the stopwatch flag means count-up display". They already differ: GTK returns early on a 0 target, Android reads Guided's stopwatch flag from the db and the other modes' from the UI. Against "decisions go in core".
- Fix: `SessionShape::new(mode, stopwatch_on, target_secs, pattern)` next to `from_db`, used by both apps. Related: slice 18.

#### #273 Pause in one place
- Related: #429, #437 (Android's `toggle` decides pause itself).
- Where: `mod.rs:218-220` (`SessionPhase::Paused`, never set; arms at 563, 856, 981, 1019), `253` (`is_paused`), `timer.rs:18` (`Stopwatch::Paused`); `pause`/`resume` swap in a dummy clock (`mod.rs:400-403, 416-418`) because `paused_at`/`resumed_at` take `self`.
- Problem: three representations of one fact, two of which can disagree with the phase (#277).
- Fix: delete `SessionPhase::Paused`; derive `Copy` on `Stopwatch` (only `Duration`s) and write `self.phase_clock = self.phase_clock.paused_at(now)`; derive `is_paused` from the clock. Also the comment at `mod.rs:486-487` (old numbers) and ARCHITECTURE.md:207 (folded in from #58). Related: #85 (`Stopwatch` serde), #270.

#### #274 The ring is rebuilt every frame
- Where: `mod.rs:883-960` (`bell_ring`), `bells.rs:210` (`rings_before` allocates a `Vec` of every planned ring from 0); called each frame by gtk `window/imp.rs:250` and at least 5 times a second on Android (`ui.rs:535`).
- Problem: a 5 s bell over an hour builds and drops two vectors of about 720 elements per frame, plus the counters the frame callback ignores. Already marked as a known `ponytail:` shortcut (gtk `window/imp.rs:246-248`).
- Fix, only if the Librem 5 janks (the iterator half is also in #289): `rings_before` as an iterator, and a cheap `ring_filled(now)` for the frame path; beads only when a bell rings or the phase changes.

#### #275 Smaller ones
- `mod.rs:613`: `self.settings.shape.clone()` on every tick just to branch; match by value (the fields are `Copy`).
- `display_secs_for_running` (`mod.rs:1059-1077`): two arms return `elapsed.as_secs()`; Guided's count-up can be a guard.
- `tick_prep` (573) and `prep_remaining_secs` (872-880) compute the same rounding; the `Prep` part of the `display_secs` arm at 1019 is unreachable (early return at 1015). Goes with #271.
- `settings.bell_rng_seed.max(1)` (358) repeats `bells::xorshift64`'s guard (`bells.rs:997`); the docs at `settings.rs:92-94`, `mod.rs:264-266` name the wrong owner.
- `Session::toggle_action(ui_state)` takes a value both callers get from the session (gtk `timer/imp.rs:2383, 4245`, `mpris.rs:97`): make it `&self`.
- Stale docs: `effect.rs:5` ("future Android"); `effect.rs:148-151` leaves `AutoFinish` and `StopGuidedAudio` out of the non-bell list; `mod.rs:213-214` ("Stage 0a"); `mod.rs:426` (`stop` returns two effects, not one); `TickOutcome` "three values" vs `tick_summary` "two pieces"; `mod.rs:275-282`, eight lines on `u64` vs `u32`.

#### #276 Tests
- `settings.rs:177-197`: `by_hand` copies `from_db` line for line, so `from_db_builds_what_the_shells_built` compares the function with itself. Delete `by_hand`, `same` and that test; `from_db_gates_prep_starting_bell_and_cues_by_mode` (239-252) checks the real rules; add one assert on `stopwatch_on()` per shape.
- `mod.rs:1186-1265`: `timer_settings_with_prep`, `timer_countdown_settings`, `timer_stopwatch_settings` (and inline literals at 1976, 2328, 2747, 3148, 3184) spell out all nine fields; use `..Default::default()` as Android's tests do (`app.rs:619, 5010`). `timer_settings_with_prep(p)` is `timer_prep_settings(p, 600)` (16 call sites).
- Seven near-identical `BellCue` literals (`mod.rs:1235, 2445, 2558, 2587, 2914, 2961, 2981`): one `cue(sound)` helper.

### Slice 6: core bells, preset config, vibration, breath, sound, bell volume (swept 2026-11-10 at 3c845d9)

/simplify (four angles) and /code-review (high); every item re-checked against the code. Ordered by gain.

#### #281 One reader and writer per bell slot
- Related: #414 (Android's chooser maps the slot by hand four ways; one address per bell).
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
- Related: #316 (the GTK half: the list is rebuilt on every local write).
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
  - `Phase` and `BoxBreathPhaseId` (`breath.rs:16-36`) are the same enum twice; `phase_to_id` (`mod.rs:1033-1040`) bridges them; until the enums merge it belongs in `breath.rs` as `impl From<Phase> for BoxBreathPhaseId` (its comment at 23-29 lists it; folded in from #275).
  - `phase_at` (263-306): the tail after the loop is unreachable, and its doc says a zero cycle panics only in debug, but `% cycle_nanos` panics in release too (only test code builds the 0-0-0-0 `Default`).
  - gtk `timer/imp.rs:384` writes 4-4-4-4 by hand instead of `BreathPattern::box_breath()`.
- Fix: `BreathPattern::adjusted(phase, delta) -> Option<Self>` in core over `clamp_from_raw`; one phase enum; `unreachable!()` and a correct doc. Supersedes the `phase_to_id` bullet of #275.

#### #287 Vibration editor math in core and again in GTK
- Related: #369 (duration limits, the throwaway pattern row, one save helper).
- Where: `vibration.rs:339-415` (`EDITOR_HIT_RADIUS_PX`, `nearest_point`, `snap_intensity`) is used only by Android; gtk `vibration_editor.rs:38, 350-363, 391` keeps its own radius, hit test and snap. Two snaps to the 10 % grid: `quantise_amplitude` (78, with a `+1e-6` bias) and `snap_intensity` (390, without), which can differ at a half step; the lerp exists twice (`sample_line_at` 196, `resample_envelope` 271-279, which also repeats `point_x_fraction` 343). `chunk_start_offset_ms` (184) recomputes the stride `split_into_chunks` (159) uses; its only caller (gtk `vibration.rs:221`) calls both.
- Fix: GTK uses the core helpers; one snap; one `lerp_at`; `split_into_chunks` returns `(offset_ms, chunk)` pairs.

#### #288 sound.rs
- Related: #355 (bell-sound import, delete and name checks into core: do the name rule there).
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
- Related: #332 (the GTK half).
- Where: `insights.rs:51-80` (`input_from_db`, eight queries, plus a ninth for the running average that `totals` already holds), `goal.rs:89-96` (`today_from_db`, another day-totals read), the apps' mini stats re-read best streak and session count (gtk `stats/imp.rs:507-509`, android `ui.rs:2048-2050`), and the heatmap reads the whole history (gtk 196, android 2172).
- Fix: read the day totals once from the oldest window needed and derive streaks, the goal, the 30/60-day sums, the 7-day average, the heatmap and the chart from that list. Related: #220 (the query half: do together), #58 (the heatmap's `daily_totals`).

#### #298 One atomic-file-write helper
- Where: `data_io.rs:126-141` (`replace_file`), `sync/orchestrator.rs:1066-1081` (`write_whole_file`), `diag.rs:167-185` (`trim_to_tail`, which has drifted: no `sync_all`, leaves a `.log.tmp` behind on failure, and loses the 0600 mode, #87).
- Fix: one `write_atomically(path, suffix, write)` in core for all three. Supersedes the `write_whole_file` bullet of #261. Related: #87, #302.

#### #299 Settings readers that bypass `read_*`
- Related: #399 (Android's bell rows read keys by hand).
- Where: `labels.rs:24-27` (`label_active_from_db` is `read_bool(db, key, mode == Guided)` by hand); `goal.rs:35-40` (also never clamps to `DAILY_GOAL_MAX`); `format.rs:633-646` (`prep_plan_from_db`/`parse_prep_secs`, repeated by gtk `timer/imp.rs:3249-3254` and android `ui.rs:3765-3769`); `settings_keys.rs:19-131`, ten per-mode key functions with the same three-arm match, and seven distinctness tests (391-570) where one test over all keys would also catch cross-family collisions; `set_last_mode` (302-307) and `guided::remember_guided` (47-54) share "write `sync_state` only if changed".
- Fix: the existing readers everywhere; a `prep_secs_from_db`; one key table per suffix; `Database::set_sync_state` skips equal values itself. Related: #281 (key constants for the bell family).

#### #300 The three CSV importers
- Where: `data_io.rs:280-442`: `parse_csv`, `parse_insighttimer_csv`, `parse_meditation_assistant_csv` each repeat the reader, `line_of`, the field closure, both `unreadable.push` branches and a full `Session { .. }` literal (312-321, 386-395, 429-438; again in tests at `insights.rs:308, 339`, `goal.rs:361`). `parse_insighttimer_csv(path, parse_dt)` takes a test seam as a public parameter (363; both apps pass `insighttimer_started_at`). `DataIoError::Parse` is never built, `From<rusqlite::Error>` has no user, and `From<DbError>` (70-88) restates every variant as English, which GTK mirrors to translate (`meditate-gtk/src/data_io.rs:17-63`). Export sorts with `local_iso_to_unix` inside the comparator and converts each row again (166-176); import lowercases every label name per row (245-255).
- Fix: one row loop with a per-row closure and a `Session::timer(start_iso, secs, notes)` constructor (fixes #305 on the way); a private `_with` seam; `DataIoError::Db(DbError)` and a typed key instead of English (fixes #310); `sort_by_cached_key`; a lowercase-name map.

#### #301 One decision, a different output per app
- Related: #482 (insights, streak dash, sync tone).
- `mini_stat_or_dash` (GTK) and `mini_stat_value` (Android) (`format.rs:430-444`): Android groups digits, GTK doesn't. `format_time_of_day` (GTK, 24 h only) and `time_of_day_key` (Android, follows the 12/24 h setting) (674-692).
- `NameValidity` (`naming.rs:132-170`) is only ever reduced with `is_savable()` (14 call sites, all trimming first); no collision hint is shown though the doc says so.
- `InsightKey::glyph` (`insights.rs:135-152`) says it keeps the icons portable, but Android has its own `insight_icon_id` (`ui.rs:2370`).
- Fix: GTK renders the typed keys and the string helpers go; `is_savable_name(s, taken) -> bool`; say in the glyph doc it is GTK-only.

#### #302 The diagnostics log
- Where: `diag.rs:168-187` reads the whole log into one `String` per line on every start, even when no trim is needed, and writes 2000 lines unbuffered when it trims; `log` (100-109) makes five or six write calls per line.
- Fix: `read_to_string`, find the start of the last 2000 lines, one `write_all` (fixes #307); `format!` the line and one `write_all`. Related: #298, #87.

#### #303 Smaller ones
- Related: #487 (lead-in noise built per bell on Android), #481 (sort order).
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

### Slice 8: gtk `timer/`, `timer_view.blp` (swept 2026-11-10 at 3c845d9)

/simplify (four angles) and /code-review (high); every item re-checked against the code. Paths are under `meditate-gtk/` unless named. Ordered by gain.

#### #312 Decisions the timer page makes that belong in core
- Related: #395 (the Android side: three statics, the label rule four times).
- Where and problem:
  - The session shape (`timer/imp.rs:2190-2289`): #272.
  - The saved session and the crash-recovery snapshot are built separately from the same facts: `on_save` (2609) and `write_in_progress_snapshot` (2835), each with the "guided uuid only in Guided" rule and a hand-written `TimerMode` to `SessionMode` match; Android has both again (`ui.rs:3072`, `app.rs:320`). The 60 s heartbeat re-reads the Setup label each time.
  - What the Save button does (`end_button_clicked` 2521-2527: Overtime finishes, Done does nothing, else stop), again in Android `AppState::save_end`.
  - Which ending gets a completion notice (`notify_complete` 2547, called from 2534, 3015, 3073), again in Android `completion_notice_secs`.
- Fix: build the record once at Start (core already turns a snapshot into a session in `finalize_session_in_progress`); the heartbeat only updates `accumulated_secs`, Save saves it with the final seconds and note. A core `Session::press(EndButton, now)`. The notice decision with the end (#270). Related: #270, #272, slice 18.

#### #313 One reaction to a tick
- Related: #394 (Android writes the transition tail seven times).
- Where: `tick_prep` (2971), `tick_breathing` (3004), `tick_running` (3023), `tick_overtime` (3099) and the Guided end-of-stream closure in `on_start` (~2244) each call `tick` or `tick_summary`, dispatch, and check their own subset of `AutoFinish`, `EndBoxBreath`, `EnterOvertime`, `EndPrep`, `UpdateOvertimeLabel`; the tick call itself is written three times (2979-2984, 3108, and `box_breath_session_tick` 3181, which has nothing Box Breath about it).
- Problem: a new effect has to be wired five times; the source-grep test `save_joins_pause_when_preparation_ends` exists because of it.
- Fix: one `session_tick()` and one `react(effects)` for every lifecycle effect; end-of-stream calls the same `react`. Do after #271 and #270 (fewer effects to react to).

#### #314 One bell block instead of six
- Where: `timer_view.blp:516-890`: six copies of signal toggle host, sound, volume and pattern revealers (starting bell, end bell, four phases; about 42 template children, 28 of them for the phases, 157-186); the matching wiring in `timer/imp.rs:762-949` (start and end bell choosers) and `1390-1506` (phases, with redundant `let phase_for_sound = phase;` copies); three Sound/Vibration/Both toggle builders (1632, 1708, 1806, plus `bells.rs:632-650`) and two apply variants (1684, 1787; the first re-implements `settings_keys::read_signal_mode`); the "loading writes nothing" `get_app` closure four times (1050, 1092, 1127, 1516); the phase widgets listed by hand four times (1255, 1340-1375, 1542-1573, 1616).
- Problem: every chooser-flow change (volume was the last) is made six or seven times; `every_setup_bell_has_a_volume_row` counts copies to catch drift. The blp comment that `Adw.Toggle` "isn't ergonomic from Blueprint" (613-616) is stale: `mode_toggle_group` (72-88) is declared there.
- Fix: one composite `BellRows` widget built per `BellSlot` with one wiring path, the toggle group declared in Blueprint, a `writable_app()` helper. Do together with #197 (the same rows must leave their revealers) and #281 (one reader per slot in core).

#### #315 One setup reload
- Where: `refresh_streak` (3209) reloads the whole Setup page under a streak name; `on_mode_switched` (1921) repeats most of it by hand; `reload_setup` (4715) calls it and then `refresh_setup_label_chooser_subtitle`, which it already ran. Inside: `refresh_volume_rows` twice (1529, 3361), the interval-bell count twice (3255-3264/3316-3319 and `refresh_stopwatch_dependent_ui` 2132), `refresh_auto_finish_state` twice (2135 and 3332; `on_mode_switched` 1956 runs it once with a stale stopwatch flag), the starting-bell sound read and thrown away (3245, 3302; in #61), the prep and bell keys read by hand where core has readers (3244-3279), the stopwatch reload block four times (1986, 3284, 3921, 3936). A tab switch also maps the view, so `refresh_bell_row_names` (391) resolves eight names that `refresh_streak` resolves again. `on_save` reloads twice (`reset_mode`, then 2704).
- Fix: one `load_setup()`; a mode switch is "save the last mode, `load_setup()`". Related: #61, #299.

#### #316 The preset list is rebuilt after every local write
- Related: #331 (one refresh policy for the whole window).
- Where: `timer/imp.rs:396-410`: a 500 ms timeout for the app's lifetime; on any local change it rebuilds the whole starred-preset list (three queries, every row recreated with its drag controllers, focus saved and restored), on top of the explicit rebuilds after chooser picks (580, 601), moves (3427), mode switches (1968) and `refresh_streak` (3348).
- Problem: every toggle or slider save costs a full rebuild, often two within 500 ms; the app wakes twice a second while idle (Librem 5 battery).
- Fix: on a change, recompute only the active marks on the existing rows; keep full rebuilds for list changes, which already call it; poll only while the page is mapped. Related: #283 (the core half).

#### #317 Applying a preset snapshots the setup twice
- Where: `on_preset_row_activated` builds `snapshot` (3976) for Undo, then `apply_config` builds the same `before` (3894) for its rollback; each is about 25 setting reads plus the bells and phases. The rollback itself (3893-3902, Android `ui.rs:1797`) is OPEN-ISSUES #193.
- Fix: with #193 the rollback goes; until then pass the one snapshot to both.

#### #318 Dead parts of the timer page
- The `timer-paused` signal, its emit and `connect_timer_paused` (`mod.rs:108`, `imp.rs:371, 2415`): nobody listens (`window/imp.rs:171` says so).
- `show_paused_ui` (2067-2077) and the `UiState::Paused` arm of `on_mode_switched` (2003): the running page covers Setup while paused, so `show_idle_ui` (2017-2024) re-enables widgets nothing visible disabled. Check on a device that Setup is never visible while paused.
- `end_in_flight` returns a duration no caller reads (2421, callers 2518-2525); the `find_map` in `core_session_end` (3129-3145) is dead.
- `view_stack` in `timer_view.blp:6-11` has one page; `set_visible_child_name("setup")` (2015, 2069) does nothing. `countdown_inputs` (165-175) is an empty box kept "for future knobs", still toggled (2017, 2036, 2070).
- Template children never read: `guided_selected_group`, `cues_signal_mode_row`, `starting_bell_signal_mode_row`, `end_bell_signal_mode_row`, `guided_inputs`. `stopwatch_mode_row.set_visible(true)` (1948, 2021): nothing hides it.
- Unused parameters: `label_merged(_from, _into)` (4202), `apply_preferred_label_for_mode(_mode)` (4707, in #61), the `_obj` of the three tick functions (2971, 3023, 3099); `reset_mode`'s match with two empty arms (2788-2798).
- Related: #61 (the older GTK dead-code list).

#### #319 Redundant state and copied switch handlers
- `stopwatch_toggle_on` (255) mirrors `stopwatch_mode_row.is_active()`, set together in five places with a set-guard-set-set-unguard ritual.
- `tick_mode` (208) duplicates `Session::mode()`; `on_pause` (2401) reads it, `on_resume` (2355) reads `current_mode()`.
- `TimerMode` (36-67) renames core's `SessionMode` one to one (Android `app.rs:25-52` too); two hand-written matches (2609, 2843) copy its `From` impl.
- The stopwatch, keep-awake and auto-finish switch handlers (443-495) and the starting-bell, prep and interval-bell expander handlers (880-988), plus the cues master (1323-1336): seven copies of about 15 lines; `read_bool(stopwatch_key_for_mode(..))` five times across both apps, with no core `stopwatch_from_db`.
- 53 `*_loading` guards (fields 246, 258, 264) suppress writes core already skips when unchanged (`db/settings.rs:20-30`); one missed guard caused the Cues bug. Caveat: the first save of a default value still records an event.
- Fix: read the switch; the session's mode; `SessionMode` directly; a `bind_per_mode_switch(row, key_fn)`; a core `stopwatch_from_db`; drop guards whose only job is the write, after checking the first-save case.

#### #320 One toast helper
- Related: #373.
- Related: #357 (the same in preferences, presets and sounds, plus an undo button).
- Where: `timer/imp.rs:4031-4035` (`toast`), 2664-2671, 2682-2689, 3634-3636, 3802-3804 use `adw::Toast::new` and `add_toast` directly; `db::show_toast` (`db/mod.rs:144`) wraps the title, replaces the last toast and finds the overlay of a pushed page. The two `on_save` error arms (2656-2691) differ only in kind, detail and text.
- Fix: `show_toast` everywhere; one error arm. Fixes #326.

#### #321 Smaller ones
- The Import File callback (654-663) builds what `library_pick(&row)` (4453) returns.
- `lookup_sound_name_for_setting`/`lookup_pattern_name_for_setting` (4426-4447) repeat `bells.rs:440-453` and turn a read error into "" instead of the default uuid. Related: #284.
- `BreathPattern { 4, 4, 4, 4 }` by hand (384-386): in #286.
- The fallback `PresetConfig` (3869-3879) spells out every field; `..Default::default()`.
- `get_app()` (4206) is the same as in Log (`log/imp.rs:1298`) and Stats (`stats/imp.rs:569`); `sound.rs:387` has `app()`.
- Overtime rebuilds both end buttons every second (3120 via `refresh_end_buttons` 2484); only the Add label changes.
- `follow_selected_guided_file` (3752-3763) looks the file up twice; `refresh_media` (2449-2457) clones the guided name each tick; `refresh_guided_selected_row` (3690) reads the remembered file on every refresh (the write is skipped when unchanged).
- `show_custom_time_dialog` (4078-4150): both arms compute h:m the same way, `mode_for_response` copies `mode`, the Guided arm is `unreachable!()` in one place and `{}` in the other.
- Docs on the wrong item or cut off: 51-55, 156, 1240-1253, 2824-2830, 2890-2901, 3147, 3203-3206, 3889-3892, 4254-4257, 4500, `mod.rs:89`; `timer_view.blp:380-383, 420-429`. Mid-file `use`s (4257, 4513-4521).

#### #322 Tests
- Five source-slicing helpers across `mod.rs:237-286, 431` and `imp.rs:4798-4807` (`method` and `body` are the same); "split imp.rs at `#[cfg(test)]`" inline about six times; some `mod.rs` tests check only `window/imp.rs` or `application.rs` (353-382).
- Most tests grep source text (exact signatures, counts of `.duration_value_label`), so #312 to #319 would break them without a behaviour change. Where a decision moves to core, replace the grep with a core unit test.

### Slice 9: gtk window, log, stats, application, gtk `db/`, their `.blp` (swept 2026-11-10 at 9c72061)

/simplify (four angles) and /code-review (high); every item re-checked against the code. Paths are under `meditate-gtk/` unless named. Ordered by gain.

#### #327 Let the compiler find dropped write results
- Related: #484 (`with_db_mut` goes away).
- Where: `db/mod.rs:1074-1145` (`dropped_writes` / `no_write_result_is_dropped`: a brace-matching text scan over ten named files) and `application.rs:490-585` (`with_db*` return `Option<R>`, which loses `Result`'s `#[must_use]`).
- Problem: the scan misses files and patterns: `window/imp.rs:727` drops a write through `with_db` (#340), and `application.rs:419-426` only logs a failed delete (#341).
- Fix: `#[must_use]` on `with_db`, `with_db_mut`, `with_db_blocking(_mut)`; delete the scanner (about 70 lines). Fixes #340 and #341 on the way.

#### #328 The shell's `DbError` and the pass-through `Database`
- Where: `db/mod.rs:31-133` (a `DbError` that copies core's variants), `359-371` (`map_core_err`), `120-133` (`clash_text` downcasts both types and maps core to shell again), about 60 one-line wrappers (445-944) next to 88 direct `db.core()` calls; guided.rs:1191 already matches core's type.
- Also: `list_presets`, `active_months`, `active_days_in_month` have no caller (seven more wrappers look unused by grep; confirm with clippy); `let _ = BellSoundCategory::General;` (439); `test_db_in_memory` (952-961) seeds one by one instead of `seed_all_non_audio()`; docs still describe `rusqlite::Result` and the retired "delete the old DB" rule (12-16, 403-406, 661-663, 707-710); the seed-list doc sits on `bundled_bell_resource` (174-178).
- Fix: core's error with a free `duplicate_toast_text(&core::DbError)`; callers use core; keep only the GTK-specific bits (bundled-bell seed table, the `Session` unix and ISO shim).

#### #329 Log day sections from one grouping
- Related: #433 (Android's `group_log_sessions`).
- Where: `log/imp.rs:238-275` (append), `282-344` (prepend), `798-840` (delete) each keep their own count, total and caption; `build_section_frame` returns a tuple and two `DateSection` literals are built (251-257, 327-333); `loaded_count` (43) always equals `sessions.len()`; `application.rs:747` asserts `log_card_total_secs(` appears exactly five times.
- Problem: three copies of the count and total rule (#35 and the "counted twice" fix came from it); Android computes the same as a pure, tested `group_log_sessions` (`app.rs:1040`), including the hidden-pending-delete rule GTK re-implements with `set_visible` twice.
- Fix: `group_log_sessions` (without Slint types) into core; GTK derives a section's caption from it, or at least a `DateSection::add/remove`. Delete `loaded_count`. Related: slice 18.

#### #330 The Log filter
- Where: GTK `window/imp.rs:992-1049` reaches into `log_view.imp()` fields (`filter_label_ids`, `building_filter`, `filter_label_id`); `log/imp.rs:143-147` and `355-360` both clear a deleted filter label (the second then calls `refresh()`); labels and places are reloaded four ways (139-149, 289-298, 348-356, 1168-1171; the label map built twice, 224, 300); `has_filter` written three times (164, 834, `log/mod.rs:43`); two guards (`is_active()` and `building_filter`) for one problem; every Log tab switch runs `refresh_filter_labels` (`window/imp.rs:980`) though the popover rebuilds on show (`log/imp.rs:1012-1020`). Android (`ui.rs:2859-2872, 3241-3271, 9008-9021`) re-queries `all_labels_ordered()` on pick, the stale-mapping bug GTK fixed with `filter_label_ids`.
- Fix: a core `LogFilter { notes_only, label_id }` with `has_active()`, `dropdown(&labels)`, `pick(index, &ids)`, `reconcile(&labels)`; LogView gets one `set_filter_from_index`; one `reload_labels(&app)`; drop the tab-switch call. Related: slice 18.

#### #331 One "data changed" refresh policy
- Related: #484 (GTK syncs after every write), #485 (a sync says what changed).
- Related: #397 (Android's double refreshes after a pull).
- Related: #378 (the hand `invalidate` calls in labels, guided and recovery).
- Where: `application.rs:632-664`: every finished sync invalidates ALL and refreshes timer, Stats and Log, hidden or not, whether or not anything was pulled (`drain` throws away the result; Android ORs `brought_changes()`, `ui.rs:197`); three `visible-child` handlers (`window/imp.rs:155, 969, 1142`); `reload_for_new_day` (778-790) refreshes only Log or Stats; the map handler pre-warms everything (91-134). A filter change sets the app-wide `log_dirty` to force a re-query (`log/mod.rs:37-49, 60-72`); `stats_dirty`/`log_dirty` have pub fields plus four accessors used only as check-then-clear (`application.rs:22-26, 532-550`); `LogView::refresh`'s "no app yet" branch is dead (`log/mod.rs:16-24`).
- Problem: since every write triggers a sync, every edit ends in a full timer, Stats and Log rebuild (#342); the new day misses the timer page (#346).
- Fix: OR `brought_changes()` over the passes (an error counts as changed); one window method `on_data_changed(scope)` that invalidates and refreshes only the visible page, one visible-child handler; `take_*_dirty()`; the filter keeps its own dirty bit. Fixes #342 and #346.

#### #332 One read for a Stats reload
- Related: #398 (Android reads streak and count twice, the whole history for the heatmap).
- Where: `stats/imp.rs:140-151, 153-558`: six `get_app()` + `with_db` calls; day totals read four times (goal, heatmap over the whole history, insights, chart), `daily_goal_mins` twice, best streak and session count twice.
- Fix: one `with_db` per reload over the core list from #297. Related: #297 (core half), #220, #58.

#### #333 The sync indicator polls every 2 s
- Where: `window/imp.rs:760-777, 795-862`: 4-6 `sync_state` reads per tick (with a refused upload, all bell sounds and guided files too), and every widget property re-applied, CSS classes removed and re-added. The new-day check rides on this poll, pinned by a source grep (1218-1230).
- Fix: keep the last `SyncIndicatorState` (it is `PartialEq`) and return early; refresh from the sync worker's main-loop hop and when a sync starts; a separate day watch. Related: #254 (one read in core).

#### #334 Editing a session rebuilds the Log
- Where: `log/imp.rs:1262-1285`: Save calls `refresh()` (labels, places, guided names, page 1, every card), losing "Load more" pages and the scroll, though `cards_by_id` (54-56) exists for in-place edits; its failure path builds its own plain toasts (1272, 1277) instead of `db::write_ok`; the dialog title is built twice (1145, 1152) and 912 re-parses `init_dt`.
- Fix: replace the one card (edit) or `prepend_session` (add); `write_ok(.., Failed::Save)` (fixes #349). Related: #320.

#### #335 Pending Log deletes: one transaction per row
- Where: `log/imp.rs:776-786`: N deletes are N transactions and N `trigger_sync` calls on the main thread.
- Fix: one core batch delete in one transaction, one sync. Related: #217 (the delete paths in core).

#### #336 Startup work
- Where: `application.rs:137-231`: database open, recovery finalise, orphan sweep and the haptic D-Bus probe (up to 500 ms) run one after another before the first window; `window/imp.rs:91-134` pre-warms Stats and Log on every map, not only the first; the ShortcutsWindow is parsed at build (1060-1074).
- Fix: probe haptics on a thread joined before the UI reads it; pre-warm once (or not: both views load lazily); the orphan sweep can wait for the first frame as long as it runs before the first sync. Related: #228 (core's startup work).

#### #337 Finding the app, the window and the accent colour
- `get_app()` is the same in Log (`log/imp.rs:1298`), Stats (`stats/imp.rs:569`) and Timer (`timer/imp.rs:4206`); `application().and_then(downcast)` five more times in `window/imp.rs` (123, 700, 722, 781, 797) and twice in `application.rs`; `build_card` looks the window up three times (`log/imp.rs:554-596`) though `get_window` (1305) exists. One `MeditateApplication::of(widget)`. Related: #321.
- `accent_rgb` (`window/imp.rs:946`, unused `_widget`) and two inline copies (`stats/imp.rs:603-608, 680-685`) plus `vibration_editor.rs:622`: one parameterless helper; drop `let _ = area;` (679).
- The note editor (caption, TextView, ScrolledWindow, LabelledBy) is built twice (`window/imp.rs:549-575`, `log/imp.rs:1009-1050`).

#### #338 Smaller ones
- `window/imp.rs:673-682`: the running page prunes the stack with a hard-coded tag list (`"setup-session" | "setup-bells" | "interval-bells"`); mark the pages where they are built.
- Box Breath: each frame computes `phase_at` twice and sets three labels with `gettext` even when unchanged (411-463, 497-534); the pattern is kept twice (`pattern_cell`, `pattern_for_tick`, 407-409, 493); the stopwatch target is computed twice (355, 526).
- The bell ring's 250 ms timer keeps running unmapped and after Stop (290-329). Related: #274.
- Stats: `x_label_text` rebuilds the month list for every bar (707-712, see #294); `series`/`values`, `max_snap`, `chart_h` (442-474); `cells_built` (47) is `contrib_cells.is_empty()`; the heatmap formats five class names per cell per reload (214-251); `while first_child` loops where `remove_all()` exists (269-271, 531-533); the progress clamped twice (100, 699); the best-streak dash by hand (515-521, Android `ui.rs:2050-2056`, see #301).
- "Sync Now" (`window/imp.rs:726-729`, Android `ui.rs:9734`) clears refused uploads then syncs in both apps: one core `retry_now()` returning `Result`.
- The Stats icon is set in code (80-82) and in `window.blp:144`; `switcher_bar` is an unread template child; `imp::add_toast` vs `toast_overlay` directly (1052, `mod.rs:27`); `ObjectSubclassIsExt` imported inside 17 methods; `gdk::Display::default()` twice (`application.rs:209, 217`).
- Pass-throughs to core: `log/imp.rs:650, 677`, `stats/imp.rs:582`, `db/mod.rs:377`. `SessionCountKey`'s two arms (`log/imp.rs:683-688`): see #296.
- Docs on the wrong item: `log/mod.rs:26-32`, `window/imp.rs:905-911`; stale: `window/imp.rs:1158` ("E.5"), `window.blp:50-56`.

#### #339 Tests
- `window/imp.rs:1156-1207` re-test core's `synced_ago_key` through English substrings, with an `ago` helper that re-implements `unix_now()`.
- Source-reading helpers again (`window/imp.rs:1211`, `log/mod.rs:84-96`; about ten inline `read_to_string` in `application.rs`); most `application.rs` tests (698-883) pin other files. Related: #322.

### Slice 10: gtk preferences, presets, bells, sounds (swept 2026-11-10 at 9c72061)

/simplify (four angles) and /code-review (high); every item re-checked against the code. Paths are under `meditate-gtk/src/` unless named. Ordered by gain.

#### #352 Closures that keep closed pages alive
- Related: #368 (the Rebuilder part, done once with a weak self-reference), #391 (the same leak in Local Folder).
- Where:
  - Each `Rebuilder` closure captures a strong `Rc` of the `Rebuilder` that owns it (`bells.rs:62-79`, `sounds.rs:92-105`, `presets.rs:107-121`), and nothing sets it back to `None`: every open of Interval Bells, a sound chooser or the preset chooser keeps its group, rows, `on_changed` closures, preview state and, in Save mode, a `PresetConfig` per row.
  - The bell edit page's Delete handler captures its own page and `prefs_page` strongly (`bells.rs:893-933`); the Volume row's `save` closure captures the row that owns the scale (`bells.rs:744-755`, `volume_row.rs:203-229`), keeping a GStreamer `MediaFile` per previewed edit page; rows captured in their own handlers (`bells.rs:293, 816, 827, 847, 858`, `sounds.rs:350`); each preset row's `move_to` holds the `rows` Vec (`presets.rs:245-253`).
  - Create, rename and import dialogs: the validate closure on the entry holds the dialog that owns the entry (`presets.rs:526, 584`, `sounds.rs:578, 928`).
- Problem: reference cycles, so these pages and dialogs are never freed; memory grows with every visit (Librem 5). Seen in the code, not measured.
- Fix: `Rc::downgrade` for the rebuilder, `#[weak]` in `clone!` for widgets (as `preferences.rs:275` does). Check with a heap or refcount test before and after. labels.rs and vibrations.rs use the same Rebuilder pattern (slice 11).

#### #353 The bell list rebuilds too often
- Where: `write_back_bell` (`bells.rs:535-554`) rebuilds the hidden list on every spin step, kind or signal change and settled volume, though the list rebuilds on `connect_shown` (87-91) when the edit page pops; the rebuilder already calls `on_changed` (78) and the callers call it again (357, 553, 924), so the count subtitle is read twice per write; opening the page builds the list twice (`rb()` at 81, then `connect_shown`); `build_create_row` (202) the same. Each row's subtitle loads the whole bell-sound library (239, `bells.rs:440-445`; see #284).
- Fix: rebuild only on `connect_shown`; one `on_changed`; load the library once per rebuild and use core's pure `sound_name`.

#### #354 Interval-bell rules in core
- Related: #422 (the Android bug the three writes cause), #421 (one editor fill).
- Where: `bells.rs:126-133, 249-253` builds the stopwatch display mode by hand where core has `display_mode_from_db` (Android uses it, `ui.rs:3694, 3950, 4025`); GTK hard-codes Timer, Android passes the setup page's mode, so the two decide differently (latent while Box Breath hides interval bells). `insert_interval_bell` returns a rowid, so GTK queries the uuid back (183-213) and Android queries and then updates for the volume (`ui.rs:8117-8130`); the new-bell defaults are assembled in each app. The kind-to-index mapping is written both ways inline (590-594, 769-773) and again in Android (`app.rs:267-284`) with a different fallback.
- Fix: a core `interval_bell_display_mode(db)`; `insert_interval_bell(.., volume) -> uuid`; `IntervalBell::new_default()`; one ordered kind list in core. Also core's own `preset_config::apply` inserts a re-added bell and then updates it for the volume (744-758): two writes, two sync events (folded in from #289).

#### #355 Bell-sound import, delete and name checks in core
- Related: #440 (the bundled bell list kept in both apps).
- Related: #402 (Android's import flow).
- Related: #372 (the same for guided files: one core helper for both).
- Where: "insert the row, delete the orphaned file if the insert fails" (`sounds.rs:624-648`; Android `insert_bell_sound_import`, `ui.rs:1619-1653`, with the mime hard-coded as `audio/ogg`); "delete the row, then the files" (`sounds.rs:997-1000`, Android `ui.rs:6930`); the import folder rebuilt by hand (`sounds.rs:704-706`) next to `sync_runner::local_sounds_dir()`; name checks load the whole library on every keystroke (`sounds.rs:547-551, 936-942`), and the import dialog skips `validate` (539-560), while presets use one `is_preset_name_taken` query.
- Fix: core `import_bell_sound`, `delete_bell_sound_with_files`, `is_bell_sound_name_taken(name, except)`; `local_sounds_dir()` everywhere; `validate` in every dialog. Related: #288 (the name rule).

#### #356 One preview button, one transcode
- Related: #416 (Android's preview toggles).
- Related: #375 (the guided preview slot and `PreviewState`).
- Where: `sounds.rs:260-334` (`add_play_button`, `PreviewState` 30-37) is a near copy of `guided.rs:871-930`, and `vibrations.rs:286-331` is a third; the "revert to Play" pair twice (296-299, 322-326). The transcode copy (`sounds.rs:742-846` vs `guided.rs:461-556`) is already #61; the busy spinner (597-606) copies `guided.rs:302`.
- Fix: one preview-button helper with a play closure and a tooltip. Related: #61.

#### #357 Toasts and dialogs
- Related: #373 (guided, Local Folder and sound toasts; the undo toast three ways).
- `sounds.rs:451-463, 671-682` post to the main window's overlay, hidden behind the chooser (#362); `preferences.rs:792-795` (`data_toast`, used inline at 371-372) uses a plain title; presets keep their own one-at-a-time `ToastSlot` and two undo-toast variants (`presets.rs:30-35, 726-797`) next to `db::show_toast`'s own, and `guided.rs:1214-1287` has a superset of `build_undo_toast`.
- `if let Some(root) = anchor.root() { if let Ok(window) = root.downcast::<gtk::Window>() { dialog.present(..) } }` about 17 times (presets 553, 620, 689, 719; sounds 663, 968, 1006; bells 360, 928; more in other files): `dialog.present(Some(anchor))` does it and never silently skips.
- The "Delete Bell?" dialog and its delete flow twice (`bells.rs:328-365`, 899-933).
- Fix: `show_toast` with an optional undo button; `present(Some(anchor))`; one `confirm_and_delete(.., after)`. Related: #320, #337.

#### #358 One name dialog
- Related: #486 (the name rule inside the write).
- Related: #415 (Android's six name dialogs).
- Related: #374 (the vibration editor and guided import also skip `validate`).
- Where: create and rename dialogs build the same entry, AlertDialog and validate closure: `presets.rs:502-559, 561-626`, `sounds.rs:902-974`, plus `labels.rs:303`, `guided.rs:942`, `vibrations.rs:527`. Callbacks wrapped as `Rc<Box<dyn Fn>>` (`presets.rs:505/544, 699/713`, `sounds.rs:390/401/470`). "Import failed:" + msg (`sounds.rs:676`) is a third msgid next to "Import failed: {error}" and "Import failed".
- Fix: one `name_dialog(anchor, heading, initial, response, is_taken, on_ok)` (the leak in #352 is fixed once there); reuse "Import failed: {error}".

#### #359 Preferences
- The `SyncSettingsError` toast mapping twice (`preferences.rs:305-326, 416-435`; Android twice more, `ui.rs:9119-9140, 9638-9660`); the keychain read wrapped in a 10-line match for an MSRV note that no longer applies (291-303 vs 411-415).
- The lead-in is read through core but written raw (118-121, 137-140; Android `ui.rs:6644-6658`): core writers like `goal::write_daily_goal_mins`. `f64::from(lead_ms) / 1000.0` (127) next to `ms()` (102).
- Spin rows write on every tick (lead-in 132-144, daily goal 77-88): #363.
- CSV export, parse and import run on the main thread (614, 722, 777); `with_db_blocking`/`spawn_blocking` exist (Test uses them, 344).
- The closed handler (525-541) repeats `refresh_main_window` (797-805); after an import, `refresh_streak` runs twice. Three import buttons wired by the same `clone!` with a seven-parameter `wire_data_actions` (629-653); the CSV filter built twice (599-602, 708-711). Stale tombstones (37-51).

#### #360 Smaller ones
- `presets.rs`: `chooser_mode` tested four times (233-267, one `match`); Save mode clones everything twice per tap and deep-copies the `PresetConfig` per row (267-339); `toast_slot` threaded through to be dropped (434-436, also in #61).
- `bells.rs`: `let _ = populating;` and its wrong comment (935-940, also in #61), and the signal handler's check can't fire (878-880); the doc of `push_edit_page` split around `write_back_bell` (529-559); `type Rebuilder = crate::Rebuilder` carrying `rebuild_list`'s doc (95-99); "once B.3.3.B lands" (22); `lookup_bell` lists all bells to find one (945-951); two copies of the chevron suffix (663-667, 681-685) and of the sound/pattern row handlers with double clones (809-864).
- `if let Some(rb) = rebuilder.borrow().as_ref() { rb(); }` 35 times: one `crate::rebuild(&r)`.
- `sounds.rs`: `SelectionContext` is pub but local, the module doc describes a mode that doesn't exist (9-10, 203-211); orphan comment (377-381); `std::cell::RefCell` by full path (80-84).
- `preset_subtitle.rs:56-67`: the `One` arm (see #296); "Stopwatch" twice (33, 46).
- `volume_row.rs:146-150` re-implements `sound::app()`.

#### #361 Tests
- `volume_row.rs:305-383`, `preferences.rs:818-838`, `bells.rs:1005-1026` assert exact source substrings and their order, so #353 to #359 each need test rewrites without a behaviour change. The volume settle, hold and release decision (231-280) could be a pure state machine with real unit tests. Related: #322, #339.

### Slice 11: gtk vibrations, vibration editor, guided, labels, keychain, local folder, the rest (swept 2026-11-10 at 9c72061)

/simplify (four angles) and /code-review (high); every item re-checked against the code. Files: `vibrations.rs`, `vibration_editor.rs`, `vibration.rs`, `guided.rs`, `labels.rs`, `keychain.rs`, `local_folder.rs`, `announcement.rs`, `config.rs`, `data_io.rs`, `format.rs`, `i18n.rs`, `main.rs`, `mpris.rs`, `recovery_dialog.rs`, `sound.rs`, `sync_runner.rs`, `time.rs`, `data/ui/shortcuts.ui`. Paths are under `meditate-gtk/src/` unless named. Ordered by gain.

#### #368 One Rebuilder that frees itself
- Where: `if let Some(rb) = rebuilder.borrow().as_ref() { rb(); }` about 38 times in 7 files; the `Rebuilder` alias (`main.rs:44`) documents the self-capture as the pattern, and every chooser holds a strong `Rc` to itself (`vibrations.rs:81-97`, `labels.rs:76-87`, `guided.rs:614-630`), with about ten `*_for_rb` clones each. `local_folder.rs:365-380` already uses `Rc::downgrade`.
- Also: the guided rebuilder calls `on_changed` itself (`guided.rs:629`) and every caller calls it again (674, 720, 829, 1022, 1051, 1123, 1179), so the Setup view's starred list rebuilds twice per action; after creating a label the chooser is popped and then rebuilt (`labels.rs:131-137`).
- Fix: a `Rebuilder` newtype holding a weak self-reference with `rebuild()`; drop the second `on_changed` and the rebuild after `pop()`. This is the general form of the Rebuilder part of #352: do it once here. Related: #352, #386.

#### #369 Vibration editor rules and the throwaway pattern row (both apps)
- Related: #420 (Android's editor).
- Where: pick, snap and point spacing are #287. On top: the 0.5-10 s limits (`vibration_editor.rs:33-34`) are written again in Android (`main.slint`, 5..100 tenths); Save rounds the duration (`.round()`, 499), Preview truncates (`as u32`, 581), Android multiplies tenths; both editors build a `VibrationPattern` with blank id, uuid, name and times only to call `build_master_envelope` (`vibration_editor.rs:531-570`, `vibration.rs:170-182`, android `ui.rs:7263-7283`), and both write the same insert-or-update branch (`vibration_editor.rs:492-511`, android `ui.rs:7610-7630`). The aliases `EDITOR_* as …` and `resample_envelope as resample` exist for readability of pre-migration code (26-31, 56).
- Fix: `EDITOR_DURATION_MIN_MS`/`MAX_MS` and one `duration_ms(seconds)` in core; `build_master_envelope(duration_ms, &[f32], ChartKind)` and `PatternPlayback::play` taking the same; a core `save_vibration_pattern(db, edit_uuid, ..) -> uuid`. Related: #287.

#### #370 Saving a vibration pattern says whether it changed
- Where: `vibrations.rs:374-395` reads the row back after every editor Save and compares it field by field (`patterns_equivalent`, core `vibration.rs:212`, five tests) to decide whether to show Undo, because `update_vibration_pattern` returns `()` and always writes; `rename_vibration_pattern` already returns `Result<bool>`. Android shows Undo without the check.
- Fix: the unchanged guard in `update_vibration_pattern` returning `Result<bool>`; delete the read-back and `patterns_equivalent`. Comes free with #234's guard, which also fixes #184 and #248.

#### #371 Local Folder: rebuild only when the list changes
- Related: #481 (the job in core, its slice 18 note).
- Related: #439, #443 (Android's lengths buffer).
- Where: every scan step and every length read calls `notify()` (`local_folder.rs:103, 115, 140, 171, 287`), and every live page's listener runs a full `fill_page`: four DB reads (folder, `files_of`, list view, sort) and every row rebuilt, so reading N lengths costs N full rebuilds of N rows. The focus and scroll repair (453-461, 493-509, `named` 612-618) exists to undo that rebuild. The queue dedup is `q.iter().any` per file (128-133). The toggle and sort handlers carry a weak self-rebuild (365-381) where `notify()` re-fills every page anyway.
- Fix: rebuild on scan, sort and view toggle only; update a length on its row (`file-{id}`) in place, or coalesce into one idle; a `HashSet` for the queue; toggle and sort call `notify()`. Then the refocus code goes. For slice 18: the job state (scanning, queue, failed, unreachable, the "folder changed during the walk" rule) is kept again in android `ui.rs:1270-1290`; only `RefreshProgress` is in core. Related: #391.

#### #372 Guided import, delete and transcode
- Related: #402 (Android's import flow).
- Where: "insert, remove the orphan if the insert fails, read the row back" (`guided.rs:312-345`) and "delete the row, remove the file when the Undo toast ends" (1094-1210) are written again in Android (`ui.rs:1166-1185, 5840, 10179`), with different error formatting; #355 moves the same into core for bell sounds only. `transcode_to_ogg_preserve_channels` (461-556) and `do_import_io` (400) copy `sounds.rs:742, 691` (#61). The `AudioFileError` to toast text is written three times (`guided.rs:155-163, 351-361`, `sounds.rs:649-653`; Android has one `audio_file_error_text`, `ui.rs:1123`), and cancel travels as `AudioFileError::Other("cancelled")` (450). The display name is built by hand (141-145) where `sounds.rs:472` uses `display_name_from_path`; `format!("{new_uuid}.ogg")` (413) copies `guided_file_path`, which 1166 uses.
- Fix: widen #355 to one core library-file helper for both kinds (`import_* -> Row`, and a delete that names the file to drop when Undo expires); one `transcode_to_ogg(src, dest, normalise, cancel)`; one `audio_error_text`; an `AudioFileError::Cancelled` variant. Related: #355, #61, #356.

#### #373 Toasts, undo toasts and the Announcement type
- Where: `guided.rs:1283-1294` (`window_from`, `add_toast_to_window`, about 10 calls; `window_from` again in `presets.rs:794`), `local_folder.rs:47-54, 77-79` and `sound.rs:270-290` post plain toasts past `db::show_toast`, which wraps the text; the undo toast is built three ways (`guided.rs:1214-1280`, `presets.rs:737-790`, `vibrations.rs:457-525`, the last two near copies of each other). `Announcement` (`announcement.rs`) is built and rendered only in GTK, Android doesn't use it, and guided builds the same toasts by hand ("File renamed" 1036, "Deleted"+name 1131); `PatternUpdated`/`PatternDeleted` and guided concatenate `"{} {}"`, so translators can't reorder (the others use `{name}`).
- Fix: `show_toast` with an optional undo button for all of them (#357's fix, widened); delete `Announcement` and write `gettext("… {name}")` at each call site, or make it the only path. Related: #320, #357, #392.

#### #374 Name checks through core's `validate`
- Where: the vibration editor's Save gate (`vibration_editor.rs:453-470`), the guided import dialog (`guided.rs:213-245`) and rename (980-1013) check empty and taken by hand (`!trimmed.is_empty() && collision`, where `collision` already implies non-empty); labels, sounds and the vibrations rename use `meditate_core::validate`.
- Fix: fold into #358's one name dialog with `validate`; #358 named `guided.rs:942` but not these two. Related: #358, #288.

#### #375 sound.rs
- Related: #85 (`preload_end_bell` was listed there first; do it once).
- Related: #397 (Android's `bell_sound_path` and the interval bell list).
- `preload_end_bell` (294-305) builds a `MediaFile` that is never played: `play_end_bell()` is gone and `play_end_bell_uuid` makes its own; four callers pay two DB reads and a player build each (window/imp.rs:126, timer/imp.rs:745, 792, 1284); stale comments at 286-293, 311-315, 326 and timer/imp.rs:742. Delete it.
- `lookup_bell_sound_by_uuid` (205-216) lists and decodes the whole bell library for every bell played, and Android does the same (`ui.rs:4032-4035`): a core `find_bell_sound_by_uuid`, as vibration patterns and guided files have. Related: #235.
- `play_preview_for_guided_file` (165-181) copies `play_preview`'s slot swap and has drifted (#390): one `play_in_preview_slot(media)`. Related: #356.
- `PreviewState` is defined three times (`guided.rs:55`, `vibrations.rs:178`, `sounds.rs:31`) over the same core `PreviewToggle`; the "set Play icon and tooltip" pair appears about 8 times: part of #356's one preview button.
- The end-bell uuid read by hand (`get_setting(end_bell_sound_key_for_mode ..)`, 295-299) is `bell_volume::sound_uuid(.., BellSlot::End(mode))`.

#### #376 keychain.rs
- `store`, `read` and `delete` repeat "open the chosen backend, mark it, on `WeakKey` log and retry on the file backend" (105-156): one `async fn with_backend(may_fall_back, op)`, and a `BackendChoice` enum for the three `u8` constants; the store-only fallback rule is then a signature instead of a source-grep test (413-425).
- Every call opens a new backend (a D-Bus connection and Secret Service session, or a re-read of the master key and the keyring file), and `read_password` runs on every sync attempt (`sync_runner.rs:14`): cache the opened backend. `preferences.rs:294, 412` call this blocking API on the main thread.
- `KeychainError` is a one-variant enum; stale docs (271 `with_keyring`, 404 `bin/keychain_smoke`); the test at 593-608 has the same body as 532-542.
- Related: #387 (the fallback itself).

#### #377 vibration.rs
- The feedbackd `Vibrate` call (bus name, path, interface, flags) is written three times (94-105, 203-214, 253-264), and `build_vibrate_args` builds `a(du)` by hand where `(APP_ID, segments.to_vec()).to_variant()` does it: one `vibrate(conn, &[(f64, u32)])`.
- `probe_haptic` is a `call_sync` with a 500 ms timeout on the startup path (`application.rs:226`), though `has_haptic()` is first read when the editor opens: `call_future` in `spawn_local`.
- The early-return handles keep `cancel_on_drop: true` (176, 180), so dropping one on the laptop still calls D-Bus, against the doc; `cancel_on_drop: false`.
- `probe_haptic`'s doc sits on `mod tests` (14-31); a history note (108-113); `chunks.iter()` + `clone()` copies each chunk (190-191).

#### #378 Refreshing views after a write
- Where: each chooser action calls `app.invalidate(..)` by hand (`labels.rs:284, 356, 416, 477-480`; `guided.rs:1021, 1050`), checked by tests that count the text in the source (`labels.rs:515-558`); the label merge invalidates ALL and then refreshes Stats and Log itself; `recovery_dialog.rs:125-127` says `with_db_mut` already invalidates, which it doesn't (`application.rs:565-572`), and 129 triggers a sync `with_db_mut` already triggered.
- Fix: through #331's one refresh policy (or invalidate inside `with_db_mut`); delete the hand calls, the wrong comment, the second trigger and the source tests. Related: #331.

#### #379 Smaller ones
- `config.rs:6-7`: dead `PKGDATADIR` under `#[allow(dead_code)]` (#393), with plumbing in `build.rs:49-50` and `src/meson.build:14`: delete all three.
- `glib::user_data_dir().join("meditate")` six times (`main.rs:50`, `keychain.rs:355`, `sync_runner.rs:22, 29`, `application.rs:284`, `sounds.rs:704`): one `data_dir()`; `sounds.rs:704` should call `local_sounds_dir()`.
- `data_io.rs:17-63` copies core's `DataIoError` variant by variant to add `NoDatabase`; `From<rusqlite::Error>` and others have no user: `enum { Core(core::DataIoError), NoDatabase }`. The export file name differs from Android's (`meditate-backup-…` vs `meditate-sessions-{today}.csv`, `ui.rs:108`); core could own it. Misplaced doc on a test (159-164).
- `recovery_dialog.rs`: the push and wipe runners differ only in the prepare call and tag (66-83, 117-140): one `run_recovery(app, tag, prepare)`, which also fixes #382.
- `mpris.rs`: `gettextrs::gettext` where every other module uses `crate::i18n::gettext` (6); `.replace("{time}", &t(s))` five times (101-111); `update` builds each changed property twice to compare (238-243): compare the `Shown` fields.
- `i18n.rs`: `potfiles()` and `languages()` share a body (155-163, 310-318), `get(fields, key)` twice. For slice 18: Android checks named msgids one test at a time (android `app.rs:1806-3583`); one checker over both catalog folders would cover it.
- `main.rs:59-75`: the two `setup_gettext` failure branches format and log the same way.
- `time.rs:22-28`: a hand-written `YYYY-MM-DD` parser where chrono is a dependency.
- `shortcuts.ui:25-43` repeats the accelerators of `application.rs:434-439`; `GtkShortcutsShortcut` with `action-name` shows the bound one.
- `vibration_editor.rs`: `edit_uuid` is never changed after construction (no `RefCell`); `name` mirrors `name_row.text()` on every change; the name, shape and banner clamps built three times; `draw_chart` allocates and formats every label on each drag motion (659-660, 725-747); the module doc says "snap to 5%", the step is 10 %.
- `guided.rs`: `push_undo_toast` is a one-call wrapper (1214-1223); `DuplicateGuidedFile(name)` taken apart and rebuilt for `write_ok` (1176-1191); `insert_err` mutated inside a closure (327-347); dead `_nav_view_for_create` and an unused `nav_view` parameter (715, 720); the playbin wrapped in a new Pipeline (1338-1345); stale docs (24-27, `_watch`, the `ellipsize` doc on a `pub use` 1278-1281).
- `labels.rs:409-411`: `invalidate(ALL)` plus explicit refreshes.
- `vibrations.rs`: stale module doc ("lands in the next phasing step", 9-13); `show_undo_edit_toast` and `show_undo_delete_toast` differ in two lines (456-523, see #373); `rebuilder` cloned twice (363, 369); rename reads the row for Undo and core reads it again (575-579).
- `local_folder.rs:73-82` `save` drops the error without logging it; `write_ok` logs and toasts.

#### #380 Tests
- Related: #432 (Android's source-text tests).
- Copies of core's tests: `vibration_editor.rs:742-841` (`max_points_for_duration_s`, `resample_envelope`, core `vibration.rs:723-760`); `sync_runner.rs:81-229` (`test_connection_with` with a hand-copied `AlwaysErrs` and a 13-line `clone_err` that breaks on every new `WebDavError`; core has `sync::fake::AlwaysErrs` and `credentials.rs:217-260`). Move what core lacks (single sample, resample to 0, the `detail()` cases) there and delete the rest; `TestConnectionResult`'s English `Display` is shown nowhere (`preferences.rs:355-361` translates the variants).
- Source-grep tests: `keychain.rs:413-425`, `data_io.rs:157-190` (checks `preferences.rs`), `sync_runner.rs:49-79` (checks other files, counts `gettext`), `labels.rs:503-558`, `sound.rs:543-580` (about 34 `contains`), `guided.rs:1652-1679`. They pin code shape and break on the refactors above; replace each with a test of a function, in core where the logic moves there.
- Seven copies of the "read a source file" helpers and three directory walkers (`i18n.rs:47-62`, two in `sound.rs:656-684, 728-747`), the i18n tests re-walk the tree six times: one `#[cfg(test)] mod test_src` with a cached walk, while such tests remain.
- Related: #322, #339, #361.

### Slice 12: android `ui.rs` lines 1-5627 (swept 2026-11-10 at 9c72061)

/simplify (four angles) and /code-review (high); every item re-checked against the code. Paths are `meditate-android/src/ui.rs` unless named. Already covered and only linked: the session shape (#272), the saved session and snapshot built separately (#312). Ordered by gain.

#### #394 One path for every session transition
- Related: #429 (the `AppState` API the seven swaps come from).
- Where: "swap the state out, run the transition, write back, dispatch effects, react to active to inactive (`end_session`, `on_auto_finish`, `on_state_changed`), refresh" is written seven times (4546, 4648, 5064, 5095, 5140, 5324 and around): Stop, natural end, guided end of track, focus loss and the tick; the end-of-track path already calls `on_state_changed(true, false)` directly instead of the was/is pair. Pause and resume add `wake_follows_pause` and `guided_follows_pause` in two blocks under the same condition (4612-4628, 5106-5107).
- Also: `AppState::toggle(shape, now)` (`app.rs:610`) takes a shape only for its Idle branch, which its comment calls a test convenience; so focus loss passes a made-up `TimerStopwatch` (5100-5103) and the action tap reads every Setup property to build a shape on each pause (4463).
- Fix: one `apply(ui, &state, |s| s.tick(now))` that owns the edges; `AppState::pause_or_resume(now)`, with `start_session` the only start. Related: #313, #270.

#### #395 The running session in one place
- Where: three statics `SESSION_GUIDED_FILE`, `_NAME`, `_LOCAL_FILE` (3023-3065), always set together (4575-4584), in Mutexes on the UI thread; Save recomputes the guided uuid from the live selection with its own Guided gate (5285-5294) instead of the value fixed at start by `app::snapshot_guided_file` (#407); "the label if the mode's switch is on" four times (2507-2511, 2538, 3077-3081, 5274-5278), each locking twice, and GTK has it as `setup_selected_label_id` (`timer/imp.rs:4167`).
- Fix: build one core `SessionInProgress` at Start and keep it in one cell; heartbeat, hold and `finalize_session` read it (that is #312's fix); a core `active_label_id_for_mode(db, mode)` for both apps. Related: #312.

#### #396 The tick loop
- Related: #439 (the general form: an in-memory event queue, files only where they must survive).
- Where: about ten drop files are polled every 200 ms in every state, idle and in the background too (`drop_file::take` builds the path and fails an `open` each): widget launch, guided pick, sound pick, folder pick, local scan, lengths, export result, CSV pick, end of track, focus loss, session tap. The import result is already polled only while an import runs (4939).
- The import-finalize branch sits 8+ levels deep in the tick closure (4939-5043), reads the kind twice, and builds a `GuidedSel` that copies `guided_file_by_uuid` (1721-1737).
- `send_media_buttons` (320-337) rebuilds the whole notification payload (palette, guided name under a lock, header, a `Tr` call per button) five times a second and clones it to compare, for text that changes once a minute.
- Fix: poll each drop file only while its action is pending (a flag set by the picker, export, scan; end of track and focus loss only while guided audio plays; the widget launch on Resume); extract `poll_import_result`; compare without the clone and build the payload only when the minute, buttons or pause state changed. Related: #409.

#### #397 Refreshes after a pull or a local change
- Related: #468 (chooser lists rebuilt per tap).
- Related: #418 (the per-change refresh helpers).
- Where: `refresh_after_pull` (2837-2855) runs `refresh_bell_rows` directly and again inside `refresh_after_pattern_change` (4158), `refresh_setup_summaries` twice (2806, 2523), resolves the label name twice (2791, 2522), `refresh_widget` twice (1199, 2544); `repopulate_bell_chooser` (4845) queries with the chooser closed.
- `refresh_bell_rows` (3710-3800) takes about 12 separate `lock_db()` calls and rebuilds the interval-editor ranges from core constants as two new `VecModel`s each time (3775-3786).
- `populate_interval_bells` lists the whole bell library once per bell (4026-4037; #408 for the missing name). `bell_sound_path` (3612-3630) lists it for every bell that rings (#375's keyed lookup).
- Lists that refresh often get a new `VecModel` each time (784-793, 1193-1199, 1251-1263, 2139-2216), though `put_rows` (518) updates in place.
- Fix: one refresh per area per event (#331's policy for Android); one lock for the bell rows; ranges set once in `build_ui`; `resolve_sound_name` with the library read once; `put_rows`. Related: #331, #375.

#### #398 Stats and Log reads
- Related: #456 (chart and heatmap work), #457 (the Log feed).
- `refresh_stats` reads the best streak and the session count (2048-2050), then `insights::input_from_db` reads both again (2113); the 91-cell heatmap loads the daily totals of the whole history (2171-2175) where `get_daily_totals_since_from_db` exists for this (GTK does the same, `stats/imp.rs:196`); after the block the DB is dropped and `refresh_chart` locks it again (2170, 2224).
- `render_log_feed` (2959-2976) takes three locks and rebuilds three lookup maps (label names, colour places, guided names) on every render, including each delete, each Undo and the midnight re-render.
- Fix: use `input.best_streak`/`session_count`; the `_since` read; cache the maps until a label or guided change. Related: #332, #297.

#### #399 Settings read and written by hand
- Related: #414 (one address per bell), #85 (the same helpers listed earlier: `read_signal_mode_for_mode`, `read_global_setting`).
- Where: `refresh_bell_rows` reads `"starting_bell_active"`, `"starting_bell_sound"`, `"starting_bell_pattern"`, `"preparation_time_secs"` and others with literal keys and defaults (3710-3800) that core's `preset_config.rs:548-611` and `bells.rs:710, 729` already read into typed structs (they return `None` when the bell is off, which is why they aren't reused); `read_global_setting(k, "false") == "true"` four times (3712, 3765, 3790, 3890) next to `settings_keys::read_bool`; `read_global_setting` is `settings_keys::read_str` (3573-3577); `read_signal_mode_for_mode` is `bells::signal_mode_override_from_db` (632-640); `read_stopwatch_for_mode` is `display_mode_from_db(..).is_stopwatch()` (603-607); `write_stopwatch/keep_awake/auto_finish/signal_mode_for_mode` repeat the `RELOADING` guard (609-630, 3432-3440) that `write_global_setting` (3580) has.
- Fix: a core "bell config regardless of active" reader under the cue readers; the core readers; one-line writers over `write_global_setting`. Related: #299.

#### #400 Box Breath phases
- Related: #455 (the phase limits from core).
- Where: each phase is five separate Slint properties (`bbc-in-*`, `bbc-holdin-*` ..), so Rust sets 20 properties in four copied branches (3886-3935) and Slint names the phase by a text tag (`bb_phase(tag)`, 3804) that fails silently on a typo; the default row (bowl, pulse, Sound, off) is written twice (3817-3829, 3857-3872); `write_bb_phase` takes four `Option`s and every caller sets one (6721, 6729, 6821, 7040), merging by hand, as GTK does too (`timer/imp.rs:1411-1455`); the stepper's index to phase mapping and clamp (5338-5361) is GTK's `adjust_phase` (`timer/imp.rs:4646`).
- Fix: a `[BbCueRow]` model of four rows by index, filled through `put_rows`; core `update_box_breath_phase(db, phase, patch)` with the defaults and #234's unchanged guard; core `BreathPattern::adjusted(index, delta)`. Related: #248.

#### #401 Labels and preset rows
- Related: #85 (the validator and `bell_name_taken` duplicates).
- Where: ten wrappers around `list_labels_from_db` (700, 807, 845, 2603, 2615, 2670, 3230, 3280, 3294, 3420), each swallowing errors its own way; `lookup_label_uuid`, `lookup_label_name`, `first_label` scan the list for one id, and `create_label_in_db` lists again to read back the uuid (#410), as GTK does (`db/mod.rs:452-463`); the uuid-to-name map built three times (699, 806, 844); `list_starred_presets_for_mode` and `list_presets_for_mode` differ only in the query and `selected` (689-717, 798-823), and `widget_rows` is a third copy (840-871); `validate_label_name` is `validate_rename_label_name(n, 0)` (2632, 3214).
- Fix: core `find_label_by_id` and `insert_label -> Label` for both apps; one `preset_items(..)`; one validator. Related: #235.

#### #402 The import flow
- Related: #480 (one Guided selection in core).
- Where: one import is spread over five cells (4369-4385: source, finalize, kind, category, create-import) with the kind and the bell category as bare `u8`s (`kind == 1`, `category == 1` about ten times: 1629, 1669, 1686, 4094, 4975, 4989, 5033, 5575); the `u8` to `BellSoundCategory` mapping three times; each caller sets the kind cell and passes the same literal to `present_guided_import_dialog` (1108-1116, 4774-4782, 5450-5457).
- "Insert, or remove the orphaned file" twice (`insert_guided_file` 1167-1186, `insert_bell_sound_import` 1623-1654).
- The storage layout by hand: `internal_data_path().join("meditate")` (99, 171-172, 5568-5576) next to `meditate_dir()` (3634), and `<uuid>.ogg` (5573-5576) next to `audio_files::guided_file_path`; the `local:{id}` key built and parsed by hand (1221, 4199, 5474, 5499) where `StarredGuidedId::key`/`from_key` exist (used at 855); the default name strips the extension with `rsplit_once('.')` (1697-1702) where core has `display_name_from_path`.
- Fix: `enum ImportKind { Guided, Bell(BellSoundCategory) }` in the finalize slot; the dialog sets it; the core import helper of #355/#372; the core path and key helpers. Related: #355, #372.

#### #403 Sync worker results
- The connection test result is turned into a `u8` 0-4 (producer near 9676) and decoded by a second `match` in the tick loop (4860-4869); `TestConnectionResult` is `Send`: park it and match once (typed-key rule). `OnceLock<Mutex<..>>` (74, 82) can be `static Mutex::new(None)` as the others are.
- The retry with backoff on network errors (164-224: 5 s, then 10 s) exists only in Android; GTK and core don't retry. A sync policy: into core's runner. Related: #252, #260.

#### #404 Pages as one enum
- Related: #464 (closed pages drawn through; `visible` per page is the small step).
- Related: #417 (Back calling each page's close first).
- Where: main.slint has about 15 separate `*-page` booleans, and the list that closes them for a widget start (929-946) must name each new one or it covers the session (bug-audit #9).
- Fix: a page enum or back stack, as `Modal` already is.

#### #405 Smaller ones
- Related: #482 (the preset subtitle as typed parts), #480 (the `local:` key).
- Related: #58 (the dead `let _` lines, listed earlier), #85 (`validate_label_name`, `bell_name_taken`).
- Dead: `let _ = current_mode.get();` (5241), `let _ = weak.clone();` (5421, 5460, 5518, 5595, 5624, and past 5627), with `weak` captured only for it (5410, 5511).
- Misplaced docs: `try_widget_deep_link`'s on `close_modal` (885-905), "Snapshot of an in-flight session" alone (582-586), `present_guided_import_dialog`'s on `insert_bell_sound_import` (1613-1617), `delete_label_impact_text`'s on `check_label_conflicts` (3301-3306). Stale: "is_syncing is hard-false" (1929-1938), "i18n isn't wired" (2008-2013), "persistence lands when settings wiring arrives" (4296-4298), "Timer keeps its value in-memory only" (4305-4307), "Guided still gated .. phase 5" (4459-4461), the i18n comment above the clock code (4234-4240).
- `SessionMode` to `TimerMode` by hand (986-992) where `From` exists (`app.rs:45`), and the `presets_supported` guard after it can't fire; `configured_duration` compares `== 1` (490); `current_mode` (78 uses) mirrors `ui.setup-mode`, and the action tap reads both (4463, 4524): check whether the cell can go.
- `ticked_pattern_uuid` copied inline (4151-4156); `push_session_length_to_ui` copied (4441-4445); `refresh_setup_label_name` copied (2791-2796); the `hm(secs)` closure three times (2152, 2340, 2401); `ClockFormat::parse` twice (4247, 4257); `Local::now().date_naive()` (3558-3565) for `today_local()`; `bell_name_taken` is `bell_sound_name_taken(n, "")` (1660, 4104).
- `type Slot<T>` (1607) exists but about 14 places spell out `Rc<RefCell<Option<..>>>`; `Rc<RefCell<Vec<(i64, Session)>>>` 11 times; redundant braces (4308-4312 and others); `now_since_epoch` (500-514) is a misnamed wrapper of `boot_time_now`; `presets_supported` (681) a pass-through.
- For slice 18: `preset_subtitle` (725-778) repeats GTK's label lookup and pattern text; the chart's month parse from ISO strings (2285-2288) is GTK's `stats/imp.rs:709`.

### Slice 13: android `ui.rs` lines 5628-10584 (swept 2026-11-10 at 9c72061)

/simplify (four angles) and /code-review (high); every item re-checked against the code. Paths are `meditate-android/src/ui.rs` unless named. Already covered and only linked: the interval-bell insert, find, update (#354, bug #422), the connection test as a `u8` (#403), page booleans (#404), the Box Breath cue properties (#400), the bell category `u8` (#402), the `SyncSettingsError` mapping twice (#359), and the dead `let _ = weak.clone();` lines (#58). Ordered by gain.

#### #414 One bell, one address
- Related: #431 (typed slots instead of strings and codes).
- Where: `ChooserTarget` is `BellSlot` plus the interval editor, but every path maps it by hand: `on_bell_chooser_pick` (6797-6845) and `on_pattern_chooser_pick` (7009-7048) each have a four-arm match writing literal keys (`"starting_bell_sound"`, `end_bell_sound_key_for_mode(..)`) or `write_bb_phase` with three `None`s; eight openers (6590-6632, 6740-6768, 6983, 7000, the `bbc_phase_handlers` macro arms, 8020-8053) each re-read the current uuid with their own key and default and set target, category, current, populate and page; `chooser_bell_volume` (10445) is a third map, `app::volume_slot` (`app.rs:351`) a fourth. Core has `bell_volume::{read, write, sound_uuid}(db, BellSlot)` for the volume only.
- Also derivable: `bell_chooser_category` and `bell_chooser_current` (4389-4390) follow from the target and the ticked row, as `ticked_pattern_uuid` does for patterns.
- Fix: `ChooserTarget::{Bell(BellSlot), IntervalEditor}`; core `pattern_uuid`, `write_sound`, `write_pattern` by `BellSlot` next to `sound_uuid`; one `open_bell_chooser(ui, target)`, one `open_pattern_chooser(ui, target)`, one pick arm per chooser. Related: #399, #400, #281.

#### #415 One name dialog (Android)
- Related: #465 (one dialog frame).
- Where: create preset, rename preset, rename guided file, rename bell sound, create label and rename label (5907-5966, 6097-6160, 6300-6365, 6849-6911, 8206-8300) each have their own Slint properties, `Modal` variant, pending-id cell and tap, changed and confirm handlers around `naming::validate(name.trim(), |n| taken(n, except)).is_savable()` (11 times); the re-check on confirm is written four times, labels have their own wrappers (2632, 3214); the vibration editor's Save gate is hand-rolled (`ve_recompute_save` 7245: trim, empty, taken, a DB error counted as "not taken") and Save doesn't re-check (#423). Bell rename checks the whole library per keystroke (4104), where the others use an indexed `is_*_name_taken_from_db`.
- Fix: one Slint name dialog and a Rust `NameTarget` enum carrying the id; one `taken(target, name)`, one changed and one confirm handler, `validate` everywhere; core `is_bell_sound_name_taken`. The Android side of #358. Related: #358, #374, #355.

#### #416 Previews
- Where: the pattern and bell preview toggles (7701-7772, 7773-7833) run the same `PreviewToggle` steps by hand (request, stop and start, a failure notice, a generation-guarded revert); `if dur_ms > 0` (7746) can't be false there; the guided preview stop is written three times (5762-5770, 5833-5839, `stop_all_previews` 10359), the bell delete stops the bell preview inline (6916-6923) while the pattern delete calls `stop_all_previews`; `VOLUME_PREVIEW_GEN` is a second hand counter next to `VE_PREVIEW_GEN`; the editor preview and the chooser preview each do `haptics::cancel`, `build_master_envelope`, `vibrate_waveform` and a revert timer (7665-7695, 7703-7745).
- Fix: one `toggle_preview(..)` over `PreviewToggle`; `stop_bell_preview`, `stop_guided_preview`; one `vibrate_preview(app, &pattern) -> u32`. Related: #356, #375.

#### #417 Back calls each page's own close
- Where: `on_back_pressed` (9840-9938) calls `invoke_*` for diag, preferences, guided manage, local and the interval editor, but copies the bodies of edit cancel, labels back, preset, bell and pattern chooser back, interval bells back and vibration editor cancel. They already differ: Back stops all previews in the editor, Cancel only the editor's.
- Fix: `invoke_*` for every page (check each callback does what Back does today); the step before #404's page enum.

#### #418 One refresh per change
- Where: "after a bell sound changed" (`repopulate_bell_chooser`, `refresh_bell_rows`, `populate_interval_bells`, the editor's sound name) is written in rename, delete and the tick (6889-6892, 6932-6935, 4845); the vibration editor's Save tail (7594-7660) is `refresh_after_pattern_change` (4149) written out; `populate_preset_chooser` + `refresh_preset_list` + `refresh_widget` five or more times (6223, 6251, 6345, 6380, near 8590), each reading all labels and presets again (three label scans, two preset queries per action); `refresh_guided_manage` + `refresh_guided_files` (+ `follow_guided_sel`) about six times (5793, 5822, 5940, 6279, 8608); the label rename and delete patch the edit label by hand after `refresh_after_label_change` already did (8286-8294, 8335-8343; #85).
- Fix: `refresh_after_bell_sound_change`, `presets_changed(ui, mode)` with one read, `guided_files_changed(ui, sel)`; call the pattern helper. Part of #397.

#### #419 Startup
- Related: #483 (one startup sequence in core), #490.
- Related: #435 (the import files the cleanup deletes).
- Where: `open_database` (10479-10584) runs in full on every activity recreate (`android_main` re-runs, 9998-10004): a second connection, diag trim, seeding, bundled-sound extraction, `finalize_session_in_progress`, orphan and stale-import cleanup; then `DATABASE.set` fails silently and the new connection is dropped (#420). The Setup surface is loaded for Timer (8498-8506) and then again through `invoke_mode_changed` once or twice for a remembered mode (9936-9948), with a folder scan and two `last_mode` writes when a guided file is remembered; the Log feed and the whole Stats page are built at startup (8501, 8503) and rebuilt on first visit (9800-9811).
- `DATABASE` is `OnceLock<Arc<Mutex<..>>>` whose `Arc` is never cloned (10086, with a 20-line comment defending it); `RECOVERED_SESSION` is a static between two functions run one after the other on the same thread (74, 10544-10551, 9958): return it.
- Fix: open and clean up once per process; seed once for the final mode; at startup compute only the streak line; plain `OnceLock<Mutex<..>>`; pass the recovered session as a value.

#### #420 Vibration editor
- Related: #471 (the plot inset in Slint, the dead height handler).
- Related: #454 (the editor hears its own writes).
- `ve_rebuild_curve` (7123-7241) rebuilds the x-axis labels on every drag move (7531, 7561): a `Tr::secs` per point, `select_xlabel_indices`, a new `VecModel`, so Slint re-creates every label; they depend only on count, duration and width. Path strings via `push_str(&format!(..))`.
- Save rebuilds what `ve_transient_pattern` builds (7206-7229, 7615-7621); `ChartKind` to and from the chip index three times (7081, 7263/7274, 7594/7604); the plot inset math three times (7123, 7451-7488, 7499, 7543); the plot size in an `Rc<Cell>` while the stats chart keeps it in Slint properties.
- Fix: build the labels in the size, duration and points handlers only; Save uses `ve_transient_pattern`; one index pair; `ve_inner(w, h)`. Related: #369.

#### #421 Smaller ones
- Related: #453 (one switch-row header, the one place for the Switch echo fix).
- Switch echo: Material `Switch` reports Rust-made changes (`switch.slint:130`), so the writers carry a `RELOADING` guard with a next-turn reset (10185-10229, about 10 checks) and an end-bell guard (6577). Emitting `toggled` only on user input in the app's own Switch wrapper would remove both (check chips and `CuesRow` too).
- `log` state: `loaded_log_sessions` and `pending_deletes` are clones of the `LOG_LOADED`/`LOG_PENDING_DELETES` thread-locals passed into about 12 handlers and three helpers (4347-4348, 8278-8777, 9257, 9297, 9440), where `write_import` and `commit_undo` read the thread-locals; the session lookup by id twice (8546-8552, 8699-8704); chrono to Slint `Date`/`Time` by hand twice (8659-8680, 8714-8736) and "clear the edit label" three times (2531, 8340, 8677): one `open_edit_session(ui, Option<Session>)`.
- Interval editor create and edit fill the same nine fields (7942-7980, 7983-8010): one `show_interval_editor(ui, &bell)` with #354's `IntervalBell::new_default()`; opening the editor lists all bells to find one (7986-7992).
- Preferences: `on_preferences_tap` locks the DB about six times (9028-9063) and repeats `reload_setup`'s loads; the keychain read and the error text twice (9115-9145, 9627-9662, #359); the `'save` block converts `SharedString` to `String` and back about ten times.
- `if value { "true" } else { "false" }` five times (6570, 6587, 6708, 7843, 7889) next to `format_bool`; the bell setting keys have no core constants (also in `format.rs:635`, `bell_volume.rs:236`, `session/settings.rs:207`).
- `on_local_star_tap` lists every file of the folder to read one flag, then lists again (5729-5757): pass `row.starred` or use `find_local_file_from_db`. The sort chip index mapping (5698-5704) is the inverse of `refresh_local_page`'s array (1470): one `const LIST_SORTS`.
- The accent ring at startup (9496) re-implements `theme::accent_ring`, which `reload_setup` (10215) uses.
- Three CSV import handlers differ only in a tag (9358-9380): one callback with the tag.
- `on_edit_save_tap` (8885-8895) logs ad hoc and nothing on `None`, while Done-screen saves use `session_save_failure_log_message`.
- About 60 handlers wrap their body in a block that scopes nothing; mis-indented blocks (9283-9320, 7651-7653).
- Stale comments: 7058 "Save / Preview land in V-4", 6448 "phase-8", 6481 "phase 5", 6186 "rows inert here", 8694 "L-4a", 8963 "L-5a", 9089 "SY-4", 6146-6149/6233 a `let res = { .. }` scope that's gone, 10539 and 10567 "Phase 3", 10445 `chooser_bell_volume`'s numeric targets, 77 the `(toast_copy, diag_detail)` doc; the "Daily-goal commit" comment 50 lines above its handler (9262-9268).

### Slice 14: android `app.rs` (swept 2026-11-10 at 9c72061)

/simplify (four angles) and /code-review (high); every item re-checked against the code. Paths are `meditate-android/src/app.rs` unless named. About 1350 of its 6195 lines are production code; the rest are tests. Ordered by gain.

#### #429 The state machine API
- Where: `Transition::{toggle, tick, stop, finish_overtime, enter_overtime, add_overtime, dismiss}` and `prepend` (478-544) exist so tests can chain ("The shell never chains", 495); the consuming `self -> Transition` API makes ui.rs swap the state out with `std::mem::replace(&mut *s, AppState::idle())` seven times (4546, 4648, 5066, 5097, 5140, 5324, 8470). `toggle(shape, now)` (606-640) has an Idle start branch only tests use, so production passes a shape it ignores (ui.rs 5101 makes one up); its Active branch decides pause or resume itself instead of core's `Session::toggle_action`, which treats Overtime as no-op (#430). `remaining()` (765-771) has no production caller, with four tests. `media_tap`/`void_tap` are `pub` with one internal caller (114, 133); `is_running_page` is `!is_idle()`, `finish_overtime` is `pub` for `save_end` only (710-777).
- Fix: transitions as `&mut self -> Vec<Effect>` (drops `Transition`, `Deref`, `prepend`, the forwarders and every `mem::replace`), or at least `#[derive(Default)]` and `mem::take`; `pause_or_resume(now)` through `toggle_action`; tests start through `start_session`; delete `remaining`. Related: #394.

#### #430 Effects read the same way in both apps
- Where: `ended_duration_secs` (189-198) reads the saved length from `EndSession | EndBoxBreath | AutoFinish`, GTK's `core_session_end` (`timer/imp.rs:3139-3142`) from `EndSession` only; `completion_notice_secs` (199-216) decides the "Meditation complete" notice while GTK keys `notify_complete` by hand on three effects (2538, 3017, 3076); `snapshot_guided_file` (320-325) is the gate GTK writes inline twice (2618, 2849); the Idle `hero_label` (819-837) re-derives core's `idle_hero_label_from_modes`, whose doc names Android as its caller; the headphone button (116-120) rebuilds Pause/Resume where core's `toggle_action` decides it (GTK's MPRIS uses it).
- Fix: `Effect::ended_duration_secs(&[Effect])` and one natural-end marker in core; the guided gate in core; call `idle_hero_label_from_modes` and `toggle_action`. Related: #270, #312.

#### #431 Typed values instead of index and string codes
- Related: #456 (insight icon id), #459 (`setup-mode` codes).
- Where: four two-way index maps written as match pairs (75-95 end buttons, 267-284 bell kind, 332-346 signal mode chips, 360-384 `TimerMode` chips), each order twice with its own round-trip test; GTK uses one const table (`MODE_NAMES`, `timer/imp.rs:56`); the bell-kind order is also inline in GTK (`bells.rs:590-594, 768-772`) with another fallback (#354). End buttons travel as bare `i32` codes inside Rust and Slint decodes `kind == 0`/`kind == 4` (main.slint 176, 687). Volume rows are named by strings ("start", "holdin" ..) kept in step with main.slint by a source-grep test (845-860, test at 2421), unknown strings falling to the interval-editor case. `TimerMode` is `SessionMode` with one variant renamed (17-53), converted back and forth 22 times in ui.rs.
- Fix: `const ORDER: [T; N]` for each; keep `EndButton` typed to the Kotlin boundary, a Slint `export enum` for buttons and volume slots (main.slint already has `Modal`, `LabelTarget`); use `SessionMode`. Related: #414.

#### #432 Tests
- Related: #438 (the JNI table and its source parser), #441 (`alarm_volume.rs` source tests).
- Where: about 250 of 255 tests check the text of ui.rs, main.slint, Kotlin, `.po` and `res/` (about 600 `.contains`, call counts like `code.matches("end_session(").count() == 4` at 2282). They break on renames and miss regressions spelled differently. They reopen files inline (`Path::new(env!("CARGO_MANIFEST_DIR"))` 112 times, `read_to_string` 181 times) next to the helpers `slint_code`, `ui_code`, `res`, `source`/`code` (5052, 5461, 5821, 5988); `ui_code` and about 25 tests split ui.rs on `#[cfg(test)]\nmod tests`, which ui.rs doesn't have; the nine languages are listed about 8 times; byte-offset slices (`&slint[at - 300..at]`, 1720, 1891, 1895, 2058, 2063, 2071, 3077, 3080, 4760, 4837) panic when the offset lands in a multibyte character (`─`, `→`, `·` are in these files); a test's doc sits on the wrong test (2538, belongs to 4117).
- Fix: lift each guarded decision (end steps, failure reporting, snapshot order) into a function in app.rs or core and test its behaviour; move the remaining real structure rules (JNI table 5934, no long dash, translations) into their own `source_rules.rs` with one `source(rel)` helper and one language list; slice on char boundaries. Related: #322, #339, #361, #380.

#### #433 Smaller ones
- `truncate_note_for_card` (1113-1122) is core's `format::ellipsize` written out, and appends "…" (#434); Slint 1.18 has `max-lines` with `overflow: elide`, matching GTK's `lines: 2; ellipsize: end`.
- `group_log_sessions` (1040-1104) converts each row to local time three times and formats a `String` day key nobody reads outside the loop, the section total in both branches; `day_header` (229-238) converts "now" per section; `local_date` uses `.earliest()` where core uses `.single()`. Day grouping is also GTK's (`log/imp.rs:244-332`): #329.
- `tree_folder_name` (929-950) is a third percent-decoder (core `sync/webdav.rs:440`) guessing a name Kotlin could write from `DocumentFile.fromTreeUri(..).name`.
- `remove_stale_import_files` (955-964) hard-codes the three prefixes of `guided.rs:37-44` and Kotlin; a subfolder per run wiped at startup would need none (#435).
- `month_day_count` (247-257) where chrono has `num_days_in_month()`; `arc_param` is a closure called once (1157-1168); `group_digits` overflows on `i64::MIN` (432, `unsigned_abs`).
- `void_tap` computes `media_status` for every code (136-149); `media_tap`/`void_tap` allocate a Vec for a `contains`.
- Stale docs: the module header (9-11: auto-finalise, no Add time), `Guided` "placeholder until phase 5" (29-31), `Active` holding Overtime transiently (456-461), `is_active` "Running OR Paused" (570-574), `stop` and a Done screen (642-649), "four-state" `AppState`, "Restart" in `toggle` (601).

### Slice 15: android Kotlin, the other `src/` files and `res/` (swept 2026-11-10 at 9c72061)

/simplify (four angles) and /code-review (high); every item re-checked against the code. Paths are under `meditate-android/` unless named. No finding touches the protected build files except where marked. Ordered by gain.

#### #438 One JNI call path
- Where: about 30 `invoke_*` bodies repeat `load_class`, `new_string` per argument, `call_static_method` with a hand-typed signature, then "log FAILED, return a fallback" (`about.rs:125-195` four string getters differing in the method name and whether a Context is passed; `insets.rs:38`; `keychain.rs:97`; `audio.rs:137` and `164` twins; the `(Context)V` helpers in `audio.rs:247`, `haptics.rs:143`, `guided.rs:368`, `widget.rs:163`); the `CALLS` table and a ~190-line source parser in `app.rs:5939-6195` check that table against the code. `service.rs::call` loads the class by hand (#58) and sends a bool as `"true"`/`"false"` (`service.rs:98`, `MeditateSessionService.kt:251`) because it only knows two shapes. String returns use `""` for "none" (`keychain.rs`, `insets.rs`, `about.rs`) though `guided.rs:475-477` maps null in three lines; Kotlin `MeditateAbout` and Rust both fall back to the same values; insets come as `"top,bottom"` and the time format as `"12|AM|PM"`. Every call loads the class again (`getClassLoader`, `loadClass`); the class-loader caveat is re-explained in eight module docs. `format_date` is one JNI call per date with a new `SimpleDateFormat` each (`about.rs:59-84`, `MeditateAbout.kt:68-78`), dozens per Stats or Log refresh. `system_colors.rs:21-41` makes 94 `getIdentifier` and 94 `getColor` calls at startup.
- Fix: one `const Method { class, name, sig }` per Kotlin method in `jni_call.rs`, one `call(app, &M, args)` and `call_string -> Option<String>` that loads (and caches) the class and logs once; each bridge one line; the table test loops over the consts and the source parser goes. That answers the earlier "merging the five `(Context)V` helpers needs a test rewrite" objection (S, open decisions). One Kotlin call returning the system colours as an `IntArray`; cache the date formatter per skeleton.

#### #439 Process-local signals without files
- Where: drop files exist because NativeActivity has no callbacks and a widget tap or a picker result must survive process death; six signals that mean nothing after one use them too: end of track, focus loss, session tap, import progress, import cancel and Local Folder lengths (`guided.rs:58-62, 254-314`, `service.rs`, `MeditateGuided.kt:161-198`, `MeditateSessionService.kt:371-382`, `MeditateLocalFolder.kt`, `MeditateGuidedImport.kt:57-73`). They cost file I/O on every tick (#396), leave stale files that need patches (#413), and the lengths buffer is never cleared, rewritten whole after each file (O(n²) bytes and O(n²) autocommitted `UPDATE`s on the UI thread, #442). `audio::preview_playing` already polls Kotlin memory over JNI.
- Kotlin builds `File(File(filesDir, "meditate"), name)` about 13 times (`MeditateLocalFolder.kt:29` has a private helper); Rust rebuilds the folder in `guided.rs:294, 307` and `widget.rs:113` next to `drop_file::take`; `markEos` and `markFocusLoss` write with plain `writeText`; `take_pending_launch` and `take_folder_pick` both "take, trim, None if empty".
- Fix: one Kotlin `MeditateEvents` queue (plus a progress map and a cancel set) drained by one JNI `take()` per tick; files only for the widget launch and picker, export and CSV results; `MeditateDropFile.file(context, name)`/`write(context, name, text)` and Rust `drop_file::{path, put}`. The general form of #396's polling item. Related: #396, #413, #371.

#### #440 Bundled bells in core
- Where: the eleven bundled bells (uuid, name, file, mime) are listed in `src/sounds.rs:40-118` and again in `meditate-gtk/src/db/mod.rs:189-256`; the uuids repeat core's `seeds.rs:25-29`; mime is `audio/ogg` for all; both apps look audio up by uuid and never read the stored path; the names differ ("Kansho" here on purpose for the phone's font, "Kanshō" in GTK, #444); `bundled_file(&dir, b.uuid)` searches the table for the row in hand (146); the OGGs are both compiled in with `include_bytes!` and extracted (1.2 MB twice; shipping them as Gradle assets **needs maintainer approval**); only the size decides whether an extracted file is current.
- Fix: `seeds::BUNDLED_BELL_SOUNDS: &[(uuid, name, file_stem)]` in core and a seed without paths; each app joins its folder or resource prefix; one name decision (#444).

#### #441 Decisions in Kotlin
- Bell volume: raise, taken over, restore and per-output keys (`MeditateAudio.kt:474-551`) are the most data-sensitive logic here, covered only by about 230 lines of source-text tests in `alarm_volume.rs:152-382`; `LEAD_RATE`/`LATE_MS` copy core's `lead_in` constants under a grep test (366). Move the decision to a pure `alarm_volume` function tested by value; Kotlin only sets the stream and writes prefs. The first raise commits `SharedPreferences` two or three times per bell on the UI thread (410-443); the route is looked up again in `applyLocked` though `startLocked` has it.
- Import passthrough: `MeditateGuidedImport.kt:109-121` decides `ogg` or `wav` passthrough itself (inferring "guided" from the destination folder name) where core has `is_guided_passthrough_ext`, which GTK calls. Decide in Rust and pass a flag.
- Void-Controls gesture detection (`MeditateSessionService.kt:76-86, 579-639`): a state machine with seven tuning constants and no test. A pure Rust `Detector::feed(t_ms, x, y, z)`, fed through a native method in `lib.rs` (not the protected `main.rs`); Kotlin unit tests instead would need Gradle changes (**maintainer approval**).
- Picker routing: Rust sends `"import-insight"`, Kotlin maps it to `"insight"` and writes it back (`MeditateFilePickerActivity.kt:138-156, 208-221`); the CSV targets are listed twice (64, 140-142). Name the drop file after the target.
- Notification texts: buttons and line come translated from Rust, but the fallback title and line, the channel names and "Meditation complete" come from `res/values*/strings.xml` (9 languages), so one surface has two catalogues. Send them in the payload; `strings.xml` keeps `app_name` and `widget_empty`.

#### #442 Smaller ones
- Local Folder: every file's address is found again segment by segment with a child-list query each (`MeditateLocalFolder.kt:131-152`), though the walk had the document id: store it next to `rel_path`.
- `MeditateAudio.kt`: `preview`, `starting`, `end` and `intervals` handled field by field in six functions (54-57, 140-178, 340-347, 428-452); `isIdleLocked()` is `levels.isEmpty()`. One slot map.
- `MeditateSessionService.kt`: six companion functions each build the intent (130-265), the launch intent three times (also `MeditateWidgetProvider.kt:86`, `MeditateWidgetLaunchReceiver.kt:26`), `buildContentIntent()` a one-line alias (718), `SHOW_BUTTONS` and `TAP` joined then split (501); a 1024×1024 bitmap kept and rescaled each minute for the media session (412-420, 463-466): draw it at the shown size.
- Long-lived listeners and the import thread hold the Activity context (`MeditateGuided.kt:45-90, 127-140`, `MeditateGuidedImport.kt:47-90`): capture `applicationContext`, as `MeditateAudio` does.
- `MeditateGuidedImport.kt`: `bytesPerFrame > 0` always true (273), `SDK_INT < Q` repeats `!canConvert` (123), `Resampler.channels` aliases `outChannels` (393); the passthrough path copies each chunk into a new buffer and resampling allocates per chunk (395-445).
- `MeditateFilePickerActivity.kt`: the copy-in block twice (160, 213), `"err:" + ..` four times (also `MeditateGuidedImport.kt:87`), `probe(f)` a one-call wrapper, `probeWith` used by `MeditateLocalFolder` but living here.
- `MeditateKeychain.kt:104` `clearPassword` is dead (and `keychain.rs:63` explains why).
- `theme.rs`: the 48 roles listed three times (131, 226-280, 509-525), one macro could generate `tonal_spot`; `swatch` builds full light and dark schemes for one colour (45-47); the test re-implements `to_hsl` (289-298).
- `audio.rs:81-95`/`alarm_volume.rs:56-66`: three getter and setter pairs by method name; startup reads all three curves (about 40 binder calls) before the first frame though they are first needed at the first bell; `levels` clones all three curves per bell.
- `guided.rs:145-158` parses `"ok"`/`"err:"` by hand next to `sound::parse_import_result`; the string-array loop twice (193-205, 436-440).
- `lib.rs:31-32` makes `theme` `pub` to dodge host dead-code warnings, where `widget` and `alarm_volume` use `#[cfg(any(target_os = "android", test))]`.
- `sync_runner.rs`: one function, one caller (`ui.rs:173`).
- `res/layout/widget_root.xml:36`: `@+id/widget_lotus` unused.
- Stale or misplaced docs: `MeditateSessionService.kt:7-11, 268-274, 327-332, 673-674` "Phase 1 has no audio yet"; `service.rs:8-11` says notification permission isn't handled; `MeditateFilePickerActivity.kt:53` and `MeditateLocalFolder.kt:4` point at a `src/local_folder.rs` that doesn't exist; `take_eos`'s doc sits on `take_focus_loss` (`guided.rs:254-265`).
- Tests: `alarm_volume.rs` repeats the `source(rel)` and function-body slicing about 16 times (part of #432's `source_rules.rs`).

### Slice 16: android `main.slint`, lines 1-5135 (swept 2026-11-10 at 9c72061)

/simplify (four angles) and /code-review (high); every item re-checked against the code. Covers the globals, structs, shared components, the enums and `MainWindow` up to the Edit-Session overlay (Setup, Log, Stats). Paths are `meditate-android/ui/main.slint` unless named. Ordered by gain.

#### #453 One reveal and one switch-row header
- Related: #466 (one bell signal body), #471 (a fourth Switch header copy).
- Where: the reveal scaffold (`reveal` property, 250 ms animation, clip box, column pinned at y=0, 1px divider) is written five times: ExpanderRow (1214-1316), ExpanderGroup (1327-1439), RevealChooser (1454-1494), RevealStepper (1901-1959, which also re-declares StepperRow's nine properties), RevealVolume (2073-2149, a hand-summed `full-height: 105px`, 2090, see #463). ExpanderRow's body is RevealChooser again, and ExpanderRow is ExpanderGroup with one ChooserRow.
- The title, subtitle and 52x32 Switch header is written three times (SwitchRow 908-967, ExpanderRow 1243-1286, ExpanderGroup 1357-1402) and has drifted: SwitchRow dims the row with `MaterialPalette.disable-opacity`, ExpanderGroup only the title with a literal 0.38 (1378), ExpanderRow has no disabled state. ExpanderGroup's "16 + 16 = 32" indent comment (1431-1433) is wrong next to ChooserRow's own 32px (48px; not checked on a screenshot).
- CompactToggle (976-1024) is ModeToggle (1036-1085) except the segment width; one caller (CuesRow, 1121).
- Fix: one `Reveal { in property <bool> shown; @children }` measured by `preferred-height` (ExpanderGroup's body already is that form); SwitchRow as the header ExpanderGroup embeds; ExpanderRow becomes `ExpanderGroup { ChooserRow }`; one toggle with an `equal-width` flag. About -250 lines, and the one header is where the Switch echo fix of #421 goes.

#### #454 The vibration editor hears its own writes
- Where: `changed ve-duration-tenths` / `changed ve-points` (3266-3276) bridge the steppers to Rust, saying StepperRow has "no change callback"; it has `edited()` (1746), used at 1954 and 8671. So each Rust `set_ve_duration_tenths`/`set_ve_points` (`ui.rs` 7302-7308, 7385, 7454) echoes back: the curve is rebuilt twice on open, `on_ve_points_changed` needs its `row_count() != n` guard (7470) only for the echo, and Save fires both callbacks again by hand (7355-7356).
- StepperRow's -/+ call `clear-focus()` then `commit-text()` (1828-1829, 1867-1868), and the deferred `changed has-focus` commits again (1858); `commit-text` and `changed value` both call `sync-text`.
- Fix: `edited => ..` on the two steppers, drop the two `changed` handlers, the guard and Save's calls; Rust resamples itself where it clamps. One commit per tap. Related: #420.

#### #455 Box Breath limits from core
- Related: #471 (Prep, lead-in and Points limits, same fix).
- Where: PhaseTile's `enabled: root.seconds < 20` (2297) is core's `PHASE_MAX_SECS`, the callers' `min-seconds: 1/0/1/0` (4235-4261) are `BreathPattern::phase_min_secs`; Rust already clamps with both (`ui.rs` 5342-5355). The comment "Slint can't import const values from Rust" ignores the seeded `goal-min-mins`, `ie-time-range` and the volume ranges.
- Fix: seed `bb-phase-max` and the minima from core like those. Related: #400 (the per-phase properties as one model, which also turns the four PhaseTile and four PhaseCue blocks into one `for`).

#### #456 Stats chart and heatmap work
- Where: the Bars/Line toggle (4893-4900, `ui.rs` 9781) calls `refresh_chart`, a DB read of up to 365 days and a new bar model, though Slint switches renderers off `stat-chart-kind` itself and the handler says the data is the same. Every size report (4949-4963, `ui.rs` 9752) runs `refresh_chart` too, including a second one at startup (the hidden Stats page is built, `visible` at 4600) and one per rotation; `changed height` is dead (fixed 120px, 4943); `stat-plot-w`/`stat-plot-h` (3579-3580) are window properties only Rust reads, where the vibration editor keeps the same size in an `Rc<Cell>`.
- `stat-contrib-cols` (always 91 cells, `ui.rs` 2215) and `stat-chart-bars` (2359) get new `VecModel`s on every `refresh_stats` (13 callers), so Slint rebuilds every cell and bar.
- The heatmap level-to-colour chain is written twice (cells 4722-4727, legend 4749-4754); `InsightRow.icon-id` is an int decoded by a nine-way ladder (4808-4817).
- Fix: delete `chart-kind-changed` and its handler; on a size change rebuild only the path strings from the cached series, size in a Rust `Cell`; `put_rows` (`ui.rs` 518); one `pure function level-color(int)`; the icon as a Slint `export enum` (#431). Related: #398, #397.

#### #457 The Log feed rebuilds every card
- Where: the feed (4363-4590) is a `for section` / `for item` in a plain Flickable, so every loaded card (15 per Load more) is a live item tree, kept while another tab shows (`visible`, 4300). Every render (`push_log_sections_to_ui`, `ui.rs` 2984-3017: Load more, each delete, Undo, save, navigation, the midnight tick) wraps everything in new `VecModel`s, so Slint drops and rebuilds every card.
- The two empty states (4317-4338, 4340-4361) differ only in two strings.
- Fix: one flat row model (header or card) in Material's `ListView`, which the compiler virtualizes, updated in place (`put_rows`); one empty state with ternary texts. Related: #398 (the three maps per render), #418, #397.

#### #458 The session ring redraws every frame
- Where: `BellRingView` (596-601) animates `shown-filled` over 200 ms, and Rust sets `ring-filled` every 200 ms tick (`ui.rs` 482, 538), so the animation never ends while a session runs: continuous frame redraws, each re-tessellating the arc `Path`, for a glide of under a pixel per second on a 20-minute session (worse with Keep Screen Awake).
- Fix: bind `filled` directly and let the ticks repaint; keep the glide for preparation only if it shows. Estimate from frame counts, not measured on the phone.

#### #459 Whether Start is allowed is decided in Slint
- Where: `start-enabled` (3149-3153) decides in the layout that Timer and Box Breath need Stopwatch or a non-zero duration and Guided a file with a known length; GTK leaves Start on and returns silently on a zero countdown (`timer/imp.rs:2199`), greying it only while reading (3737). The mode is compared as `setup-mode == 0/1/2` 15 times.
- Fix: core `can_start(mode, stopwatch, target_secs, guided_ready)` for both apps; Rust sets `start-enabled`; the mode as an enum (#431). Related: #430.

#### #460 Reorderable rows and letting go of focus
- Related: #467 (the RowDrop copy in the preset chooser).
- Reorder: the preset rows (3910-3947), the starred guided rows (4072-4102) and two lists after 5135 each repeat RowDrop, DragHandle, `changed dragging` and the 0.4 opacity rule; the guided list moves its rows through `preset-drag-data`/`preset-drag-uuid` (4083, 4098). Fix: a `ReorderRow` and one generic `drag-data`/`drag-key` pair.
- Focus: closing pages let go of a focused field five ways (3683-3762: two `release-*-inputs` helpers, `edit-note.release()`, `running-note.release()`, `root-focus.focus()` for Preferences), and a new page with a field needs its own, or the cut/copy/paste popup floats over the next page. Fix: `root-focus.focus()` on every page close, which works for any field, and drop the `release()` functions of NoteField, StepperRow and RevealStepper. This weakens R15(c)'s "no bug" reason (Considered and rejected).

#### #461 Smaller ones
- Related: #465 (ConfirmDialog as the frame of all dialogs), #467 (list rows), #471 (more stale comments and dividers).
- `MenuItem` is imported from `material-1.0/ui/items/menu_item.slint` (20-23) with a comment saying it isn't exported; `material.slint:59` exports it.
- `Tr.start-session()` (174) is never called (its comment cites a nonexistent `AppState::primary_action`) while "Start Session" is written at 2401 and 3876; defaults copy Tr wording instead of calling it: 2542 (`import-guided-title`), 3127 (`none-enabled`), 3206 (`ve-new-pattern`), 3359 (`edit-session-title`).
- Material's `HorizontalDivider` is the 1px outline-variant Rectangle written about 40 times (static ones at 1481, 1941, 2104, 3905, 4068, 4277, 4787, 5098); the library's own, not the rejected new `Divider` component. The 14px/700 section heading (3893, 3999, 4058, 4207 and about 10 after 5135) and the 11px eyebrow (4653, 4692, 4775, 4883, 5086) have no component.
- Window callbacks only Rust invokes, as an internal event bus: `stop-tap`, `save-end-tap`, `add-tap`, `save-tap` (2934-2947). Window properties Slint never reads: `delete-label-id` (2807), `rename-label-id` (2797), `filter-label-id` (3618), `ie-sound-uuid`, `ie-pattern-uuid` (3184, 3191), `guided-duration-secs` (2523, whose comment says it drives the hero).
- `stat-plot-size(..)` three times (4949-4963); ConfirmDialog builds its two buttons twice (1645-1680); PageHeader's two `if` IconButtons differ only in icon and tooltip (1530-1541); PreviewPill's only use passes `playing: true` (1686), so its Play label and two-label sizing are dead.
- The 15 `X-offset` plus `animate 300ms` pairs (3426, 3634-3737) are R15(c), rejected; VerticalSpinBox (762-839) repeating StepperRow is R15(d), rejected, but it lacks StepperRow's `release()`, so the Duration and Prep dialogs miss the popup fix (safe today only because both are `if`-built).
- Stale comments: no-op stubs and phase placeholders that are built (1213, 2962, 3463, 3486-3491, 3347, 3471, 3503, 4594, 3858, 3870, 3991, 2740, 2753, 3118); `ve-point-drag` (3223, no such callback); "1000x100 viewbox" (3225, 3567) against 3260 and 3575; the note "fixed-height + clip wrapper" (4402-4411) that doesn't exist (#434); doc comments split from their item (375-378 and 401-423, InsightRow's glyph paragraph twice, 1497-1498 above ScrimArea, 42-47 above KeyboardArea instead of Tr); "select-only", "read-only", "i18n shell-deferred" (316, 393, 409).

### Slice 17: android `main.slint`, lines 5136-9540 (swept 2026-11-10 at 9c72061)

/simplify (four angles) and /code-review (high); every item re-checked against the code. Covers `MainWindow` from the Edit-Session overlay to the end: pickers, app and navigation bars, the running page, every slide-over page, the dialogs and the snackbar. Paths are `meditate-android/ui/main.slint` unless named. Ordered by gain.

#### #464 Closed pages are drawn through on every frame
- Where: 13 page roots have neither `visible` nor `clip` (Session 5644, Bells 5759, Interval Bells 6078, Labels 6642, Interval editor 6830, Bell chooser 6985, Pattern chooser 7146, Vibration editor 7306, Preset chooser 7633, Guided Manage 8035, Local Folder 8218, Preferences 8494, Diagnostics 8897); Edit-Session (5147) and Running (6257) are gated by tab only. A closed page sits at `x = parent.width`, and the renderer still walks every item of an off-screen non-clip item's subtree (`third_party/i-slint-core/item_rendering.rs:216-240`); only `visible` (lowered to a Clip) prunes it. So every frame of a session, a slide or a spinner walks all pages, including every chooser row and the whole Local Folder list, which is kept after the page closes (`on_local_back`, `ui.rs` 5643-5657).
- Fix: `visible: root.<x>-offset > 0px;` on each page (visible while sliding, pruned when closed). One line per page, no frame rework, so not R15(c). Not measured on the phone. Related: #404, #458.

#### #465 One dialog frame
- Where: eight dialogs write ConfirmDialog's frame (1588-1680: scrim, ScrimArea, card at `KeyboardArea.card-y`, radius 28, swallow TouchArea, 24px padding, 22px title) by hand: filter sheet 5568, Duration 6530, Prep 7965, guided import 9007, Goal 9219, Recovery 9294, Sync status 9366, Label conflict 9469. They drift: widths 280, 320 and 340; six titles don't wrap (#474); Recovery and Sync status stack their buttons by hand though ConfirmDialog has `stacked` (1596); guided import re-implements `accept()` (9051). Material's `Dialog` is a PopupWindow, which collapses on Android (5307-5313), so a house frame is right.
- The Duration dialog knows both its callers: Set branches on `edit-session-page` to write `edit-duration-*` or `setup-*` (6609-6621), `max-value` has an edit-only case (6577), and the two callers seed it differently (Slint at 5213, Rust through `duration-tap`).
- Fix: `DialogFrame { title; width; @children }` split out of ConfirmDialog, used by all eight (about -250 lines; the filter sheet stays `visible`-gated for its DropDownMenu); Duration, Prep and Goal are ConfirmDialog with a "Set" button; one `duration-set(h, m)` routed by Rust, like `LabelTarget`. Related: #415 (one name dialog), #461 (ConfirmDialog's buttons twice), #470.

#### #466 One bell signal body
- Where: the four rows Type, Sound, Volume, Pattern are PhaseCue (2158-2210) and are written again for the Starting Bell (5812-5851), the End Bell (5899-5934) and the interval editor (6935-6973), each with the same nine volume bindings. The sound row says "Sound" in PhaseCue and "Bell Sound" in the others.
- Fix: `SignalBody` from PhaseCue (with `child-shade`); PhaseCue and End Bell are `ExpanderGroup { SignalBody }`, Starting Bell adds the Preparation row, the editor uses it directly. About -100 lines. Related: #453, #400.

#### #467 One list row
- Create rows: six by hand (6112, 6692, 7019, 7179, 7667, 8068), text on-surface in two and primary in four, the pattern one without "+"; all draw "+" as text, and 6715-6718 says there is no add SVG, but `assets/add.svg` is used in the AppBar (5441). Five say "Create ..." with a "…" that AGENTS.md bans for Android (6129, 6709, 7033, 7681, 8082); "Testing…" and "Converting… {}%" are the accepted progress form.
- Library rows: bell sounds (7048-7133) and patterns (7203-7293) are the same row line for line (name, fixed check slot, Play/Edit/Delete); labels (6735-6815) and Guided Manage (8097) repeat it, and the labels chooser still uses the `if item.selected` check form the pattern chooser's comment (7241-7247) says ended up behind the buttons. The divider rule differs per list.
- Stars: presets (7766-7787) and Guided Manage (8131-8153) build a 40x40 TouchArea and Image; Local Folder (8473) and Setup (4145) use `IconButton` with a tooltip (#476).
- The preset chooser rebuilds RowDrop (7713-7731) with `can-drop` on `save-mode` instead of `drag-index >= 0`; RowDrop works there as is.
- Fix: `CreateRow { text; tapped }` with add.svg and without "…"; `LibraryRow` (title, subtitle, selected slot, preview, edit and delete flags, tooltips set once); the IconButton star everywhere; `RowDrop`. About -300 lines. Related: #460, #476.

#### #468 One tap rebuilds a whole list
- Where: Local rows (8416, `ui.rs` 1509), Guided Manage (8097, `ui.rs` 1260), the preset chooser (7696, `ui.rs` 831) and Interval Bells (6144, `ui.rs` 4052) get a new `VecModel` per refresh. A Local star reads every local file to learn the current state, then reads them all again and rebuilds every row (`ui.rs` 5708-5720); each batch of lengths in `poll_local_folder` (1597-1601) does the same; the preset star runs `preset_subtitle` for every preset again (6223-6241); an interval-bell Switch rebuilds every row and destroys the Switch just tapped (7898-7903).
- Fix: `put_rows` (`ui.rs` 518); pass the row's state from Slint (`local-star-tap(key, !row.starred)`). Related: #397, #418, #443.

#### #469 Edit Session decisions in the layout
- Where: the Save gate `edit-original-secs > 0 || hours > 0 || minutes > 0` (5193) re-derives core's `format::log_edit_duration_secs(..).is_some()`, which GTK uses (`log/imp.rs:1115-1121`) and Save runs anyway (`ui.rs` 8804); they disagree for an edited session set to 0:00, unreachable only because the Duration dialog refuses 0 (6609). The date row builds `yyyy-mm-dd` by hand (5231-5233), the only untranslated date in the app, while the time row beside it goes through Rust (`render-time`, `ui.rs` 4260) and GTK shows `short_date` (`log/imp.rs:1315`).
- Fix: Rust sets `edit-save-enabled` from the core call; `pure callback render-date(Date)` over `short_date`/`locale_date` (`ui.rs` 2712, 2722).

#### #470 Prep Time and Daily Goal as inline steppers, like GTK
- Where: GTK has an `Adw.SpinRow` for both (`timer_view.blp:575-584`, `preferences.rs:62`); Android opens a modal: Prep steps by 1 where GTK steps by 5 (7963 admits it), Goal is a free TextField parsed and validated in Slint (`goal-valid`/`set-goal`, 2468-2480; `is-float()` takes "20.5" and rounds it). Rust clamps Prep through a string round trip (`parse_prep_secs(&ui.get_prep_time_secs().to_string())`, `ui.rs` 7868).
- Fix: Prep as `ExpanderGroup { StepperRow }`, Goal as a StepperRow, as Lead-in already is (8659-8672); drops `Modal.prep`, `Modal.goal`, both dialogs (about 125 lines), four properties and `on_prep_time_tap`'s seeding (`ui.rs` 7850-7858); `n.clamp(PREP_SECS_MIN, PREP_SECS_MAX)`. A UI change: needs the maintainer's yes. Related: #465.

#### #471 Smaller ones
- Limits typed in Slint that core has: Prep 5..300 (8002-8003, `format::PREP_SECS_MIN/MAX`), lead-in 10..500 hundredths (8668-8669, `lead_in::MIN_MS/MAX_MS`), Points `min-value: 3` (7411, `vibration::EDITOR_POINTS_MIN`, while its max is seeded). Same as #455.
- The vibration plot: `changed height` is dead (fixed height, 7491-7498); the 14px/28px inset is written seven times (7507, 7560-7562, 7603-7604) and must match `VE_PLOT_PAD` (`ui.rs` 7095; the comment at 7558 says lib.rs). Seed it from Rust. The Slint half of #420.
- The window watcher (9531-9538) is a 0x0 Rectangle with `watch-w`/`watch-h`; both fire on a rotation, so the insets are read over JNI twice. `changed width`/`changed height` on the root (which already has `changed height`, 2459), or one handler.
- The Box Breath halo and dot each animate `x` and `y` from the same `bb-dot-x/y` (6450-6475): one 22x22 container with one `animate x, y`.
- Three StartBars with the same seven bindings (5744, 6059, 6235).
- AppBar: an inner Rectangle repaints the outer's colour (5380-5384); two `if root.nav-page == 2` blocks (5438, 5445); the sync state an int decoded by three `if`s (5407-5435, #431). NavigationBar items set `selected-icon` equal to `icon` (5526, 5531, 5536), which the library already falls back to.
- `value-text: ""` repeats DurationRow's default seven times (8715-8882); `root.height < 560px` six times on the running page (6290-6366); "Converting… {}%" twice (9089, 9109).
- Recovery's Wipe sets `modal = none` and at once `recovery-wipe-confirm` (9346-9347). `interval-bell-delete` is the only delete confirm that closes in Slint and passes a Slint-held uuid (6223, 9190-9192).
- The interval-bell row's 52x32 Switch box (6198-6213) is a fourth copy of #453's header.
- BoxedList's doc says it draws dividers (2305-2321); callers write 32 here, the conditional ones repeating their row's condition (5694, 5801, 5871, 5890, 7418).
- Stale comments: "an `if` child inside BoxedList doesn't re-render, so `if` can't be used" (5260-5269, 6881-6884), while 5688, 5694, 5793, 5812, 5880 and 7418-7422 do exactly that; phase notes for shipped work (5138-5140, 5345-5349 and 5493-5495 "Running / Done / Labels", 6073-6074, 6825-6826, 6982-6983, 7303-7305, 7630, 7735); "1000x100 viewbox" (7463) against 7512; the 28-line snackbar comment (5463-5490) 3500 lines from its element (8959).

### Slice 18: across apps, logic in both shells that belongs in core (swept 2026-11-10 at 9c72061)

/simplify (four angles) and /code-review (high, asked for places where the apps decide the same thing differently); every item re-checked against the code. `gtk` is `meditate-gtk/src/`, `android` is `meditate-android/src/`. Only items earlier slices didn't have, or a deeper form of one. Ordered by gain.

#### #480 One Guided selection in core
- Where: the pick type is kept twice with the same fields and the same `reading_length()` (`local_id.is_some() && duration_secs == 0`, which holds Start): gtk `guided.rs:65-90` `GuidedFilePick`, android `ui.rs:4166-4191` `GuidedSel`. Applying `follow_selection` (gtk `timer/imp.rs:3747-3777`, android `ui.rs:1749-1773`, both with one `Follow::Refresh | Follow::Reselect(_)` arm, so the payload is never read), the remember key (gtk 3690-3696, android 4199-4206), the restore (gtk 3704-3733; android 4325-4328 and 9936-9947, which turns the id into a key and parses it back) and the Local Folder pick (gtk `local_folder.rs:182-195`, android `ui.rs:1400-1432`) are each written twice. GTK builds `local:{id}` by hand too (`timer/imp.rs:3522`, where the loop already holds `entry.id().key()` at 3515, and 3548). The "found" rule is in core twice and android once (`ui.rs:1405`); GTK's restore doesn't check it.
- Fix: core `GuidedPick` (`from_library`, `from_local` holding the found rule, `reading_length`, `key`) and `GuidedChoice` (`follow`, `select` which remembers, `restore`); merge `Refresh` and `Reselect`. Each app keeps only its rows. Related: #402, #405, #312, #395.

#### #481 The Local Folder job in core
- Where: the job state (scanning, queue, reading, failed, unreachable), "another folder was chosen during the walk, scan again" and the unread filter `!f.missing && f.duration_secs.is_none()` are in gtk `local_folder.rs:14-34, 98-180` and android `ui.rs:1272-1296, 1354-1366, 1515-1608`; both re-read the whole table right after `apply_local_scan`, which had every row in its transaction. They already differ: after a failed write GTK reads the lengths anyway (`unwrap_or(true)`, 117), Android doesn't (1546). The "a failed write says Save failed" helper is written twice (gtk 73-82, android 1310-1328).
- The row texts are copied: length (gtk 198-203, android 1334-1339), counts (212-221, 1341-1349), plays (205-210, 1480-1483), the file subtitle (286-295, 1474-1488), the starred local row (gtk `timer/imp.rs:3596-3603`, android `ui.rs:1214-1223`); the sort order `[Name, Length, MostPlayed, LeastPlayed]` three times (gtk 549, android 1470 and the index match 5699-5704).
- Fix: `apply_local_scan` returns what still needs a length; a core `LocalFolderJob` next to `RefreshProgress`; typed row parts and `ListSort::ORDER`. Each app keeps the walk and the probe. Related: #371, #439, #443, #303, #431.

#### #482 Composed texts as typed parts
- Where: core hands typed keys, and each app rebuilds the same composition around them. Preset subtitle: gtk `preset_subtitle.rs:22-69` and android `ui.rs:725-778` line for line (order, the `in-hold-out-hold` text, the label lookup, the bell count, the " · " join). Interval bell title: gtk `bells.rs:383-421` and android `ui.rs:3989-4020`, the jitter and step suffix glued as `" ±"`, `", +"`, so translators can't reorder it. Import question: gtk `preferences.rs:751-774`, android `ui.rs:10149-10168`. Stats insights: the streak choice (record, best was, keep going) and up or down from `pct >= 0` in gtk `stats/imp.rs:298-337` and android `ui.rs:2398-2440`, a third time for the icon (2370-2385); the "keep going" arm relies on `is_record` meaning one day. The streak tile's dash by hand (gtk `stats/imp.rs:517-522`, android `ui.rs:2052-2057`) next to `mini_stat_or_dash`. The sync tone: `OkNoTs` is neutral in GTK (`window/imp.rs:812-848`) and a primary check on Android (`ui.rs:1950-1964`); both compute `now - ts` before `synced_ago_key`.
- Fix: core returns ordered parts (the pattern `session_summary` already uses), `StreakNote` and `Trend` in `InsightKey`, a streak `MiniStatValue`, `SyncIndicatorState::tone()`; each app maps leaves to text, one translatable template per sentence. About 150 lines written twice. Related: #405 (its slice 18 note), #296, #301.

#### #483 Startup in core
- Where: open, seed non-audio rows, seed the bundled bells, `finalize_session_in_progress` with its Undo toast, `remove_orphan_files` and the log lines are written in gtk `application.rs:137-197` with `db/mod.rs:405-420` and android `ui.rs:10479-10584`. They drift: a failed seed fails the whole open on Linux and is only logged on Android (#490); the seed order differs.
- Fix: core `open_for_app(path, sounds_dir, guided_dir, bundled) -> Result<Opened { db, recovered }, DbOpenFailureKey>`; the orphan sweep then has one place to move after the first frame. Related: #419, #336, #440, #228, #85.

#### #484 Linux starts a sync after every write
- Where: `with_db_mut` and `with_db_blocking_mut` (gtk `application.rs:565-585`, about 86 callers) call `trigger_sync()` after the closure, even when nothing changed (`set_setting_in` skips an equal value and emits no event); each trigger reads the account on the main thread first (607). Android syncs only when core's `LocalChangeWatch` sees an event land (`ui.rs:245-248, 4815-4819`), and GTK already runs that watch, only for the preset list (`timer/imp.rs:395-408`).
- Fix: one app-level watch in GTK that triggers the sync and the preset rebuild; `with_db_mut` becomes `with_db`, which empties #327's list; Sync Now, startup and resume stay explicit triggers. Related: #316, #331, #327.

#### #485 A sync says what it changed
- Where: `SyncStats` (core `sync/orchestrator.rs:134-148`) has only counts, so GTK refreshes every screen after any sync (`application.rs:635-661`) and Android re-reads setup, bells, patterns, interval bells, guided files, labels, Log and Stats on any pull (`refresh_after_pull`, `ui.rs:2837-2855`); `list_label_conflicts` runs after every sync in both.
- Fix: replay knows each event's kind; return a per-area changed set, and both apps refresh only those areas. The concrete form of #331 and #397. Related: #418.

#### #486 Name checks inside the write
- Where: before every create or rename both apps trim, `validate`, and check again on confirm (gtk `labels.rs:279-281, 338-340`, android `ui.rs:6126-6133` and #415's sites), while core's `insert_label`, `update_label`, `insert_preset` and `rename_*` take untrimmed or empty names; each app fetches `default_starred_on_save()` and passes it to `insert_preset` (gtk `presets.rs:187-189`, android `ui.rs:6138-6140`).
- Fix: core create and rename trim and return a typed `NameError` (empty, taken); `create_user_preset(name, mode, json)` applies the starred default. One name dialog (#358, #415) then holds no rule. Related: #374.

#### #487 Smaller ones
- Android builds the lead-in noise for every bell (`lead_in()`, `ui.rs:10376-10379`: DB lock, up to about 480k samples, a 1 MB JNI copy, `audio.rs:147, 175`), but Kotlin uses it only on Bluetooth (`MeditateAudio.kt:265`); GTK checks the route first and caches one WAV per length (`sound.rs:395-420`). Pass it only on a Bluetooth route, or the length and let Kotlin cache. Related: #303.
- The export file name: gtk `meditate-backup-%Y-%m-%d_%H%M%S.csv` (`data_io.rs:66-70`), android `meditate-sessions-{today}.csv` (`ui.rs:108`). One core `export_file_name(now)`.
- The CSV format choice: android `app.rs:863-873` matches strings, gtk `data_io.rs:96-118` has one wrapper per button. A core `CsvFormat` with `parse`. Related: #300, #441.
- `locale_week_start_dow` (core `date_math.rs:42-62`) is glibc `nl_langinfo` code used only by GTK (`stats/imp.rs:582-584`); it belongs in gtk `format.rs`.
- Media: the bar anchor rule twice (gtk `mpris.rs:95`, android `app.rs:175-182`), `MediaStatus::anchor(now)`; Android's `media_tap` (`app.rs:114-123`) finds Pause or Resume in the offered buttons where GTK uses core's `toggle_action` (`mpris.rs:97, 126-128`). Related: #339.
- `set_breathing` (core `settings_keys.rs:234-244`) writes the length unclamped and clamps on read; GTK clamps before (`timer/imp.rs:4634`), Android doesn't (`ui.rs:1847-1855`). Clamp inside.
- Android writes a preset snapshot to JSON to parse it back (`apply_preset_json`, `ui.rs:1809-1810`): `snapshot_setup -> PresetConfig`, `.to_json()` only where stored. Related: #283, #317.
- The default import name: GTK guided `guided.rs:142-146` copies `display_name_from_path` with an untranslated "Untitled"; Android strips the extension itself (`ui.rs:1697-1702`, #405); core's own fallback is the English "Custom sound" (`sound.rs:215-221`). Return an `Option` or a key.

## Bugs found

The bugs the sweep turned up moved to OPEN-ISSUES.md on 2026-11-10, in batches CC to MM and the existing K3, Q, T and W; their numbers stay. "Checked and not kept" findings are at the end of OPEN-ISSUES.md.

## S. Cleanup (moved from OPEN-ISSUES.md on 2026-11-10)

Re-checked against 9d3ecf6 (2026-10-22); line numbers below are current. Base suites green then (core 1390, android 261, gtk 162). Open decisions before a proposal: #197 plain show/hide without the slide; #85 whether a ringing end bell keeps ringing when its settings change; #171 comment only or `synchronous=FULL` around Save and Edit; #167/#175 protected files (ask). Proposed to drop: merging the five `(Context)V` JNI helpers (the batch-R JNI table test only sees `helper(app, "method")` calls and would need a rewrite), moving `snapshot_setup_json` timing to core (Android reads the UI and a Cell, core the DB), and `tick_mode` (not dead: read at `timer/imp.rs:2313, 2814`).

### #58 Dead and duplicated code (core and Android)
- Related: #438 (`service.rs::call` and every other JNI bridge).
- Related: #405, #421.
- Core: `daily_totals()` / `get_daily_totals_from_db` (related: #297) (`db/sessions.rs:179-181, 735-761`, tests 3472, 3480, export `db/mod.rs:91`) feed only the 91-cell heatmaps (gtk `stats/imp.rs:196`, android `ui.rs:2026`); call the `_since` version with the grid's first cell (`cur_week_start - 12 weeks`, locale week start) and delete it.
- Android: 59 `let _ = weak.clone();` in `ui.rs` (first at 5220); in 7 closures it is the only use, so their `let weak = ui.as_weak();` goes too (5209/5220, 5310/5317, 5603/5610, 7251/7255, 9064/9069, 9073/9078, 9187/9195). 4 `let _ = current_mode.get();` (5045, 7908, 7978, 8029). Other no-ops: `let _ = &weak;` 9018, `let _ = value;` 6319, 7539, 7585, `let _ = v;` 6394, 6406, `let _ = i;` 6420. `AppState::remaining` (`app.rs:640`) is test-only. Kotlin `MeditateKeychain.clearPassword` (`MeditateKeychain.kt:104-106`) has no caller.
- gtk `keychain::delete_password` (141-156) with `Backend::delete` (313-325) has no caller (edit the source-text test at `keychain.rs:422`); core `settings::clear_nextcloud_account` (`sync/settings.rs:91-95`) has none either; then fix the comment at android `keychain.rs:63-67`. Stale `chooser_bell_volume` doc (`ui.rs:10016-10018`, talks of integer targets 0..6).
- `service.rs::call` (105-152) repeats `jni_call::load_class` (112-125).

### #61 GTK dead code and the duplicated transcode pipeline
- Related: #372 (the guided transcode and import, widened).
Related: #356 (the preview button is copied the same way) and #318 and #315 (more dead parts of the timer page; do the `timer/imp.rs` bullet with them).
- Dead: in `bells.rs` the `populating` guards at 760 and 872 can only fire on re-entry (it is set at 770/773 for the guard at 788), and the comment at 926-930 with `let _ = populating;` (931) is stale. Unused `toast_slot` (`presets.rs:332, 357-359`, one call site near 239); `chart_h` (`stats/imp.rs:443, 474`); `area` (`stats/imp.rs:669, 679`, caller 101); `let _ = BellSoundCategory::General` (`db/mod.rs:431`; check the import). Half-deleted "Throwaway" comments at `timer/imp.rs:1175-1180` and 153 (`setup_vibration_proto` no longer exists).
- Duplicate: `sounds.rs:742-846` (`transcode_to_ogg`) and `guided.rs:461-556` (`transcode_to_ogg_preserve_channels`) differ only in the `audioloudnorm` branch and guided's cancel-aware 250 ms `timed_pop` loop. `audioloudnorm` never runs (not in the runtime, the manifest adds no GStreamer module): delete `sounds.rs:763-783, 795-801`; then one `transcode_to_ogg(source, dest, cancel: Option<&AtomicBool>)` with the polling loop. Fix guided's doc (452-455, claims the sounds version forces mono). Tests: `sounds.rs:1019-1045`, `guided.rs:1601-1641`.
- `timer/imp.rs` (related: #281, one reader per bell slot): the `starting_bell_sound` read with its `"bowl"` fallback bound to `_starting_bell_sound_legacy` (3110-3112, 3133, 3141, 3167); `apply_preferred_label_for_mode(_mode)` (4451, callers 1891, 3230); `build_breathing_setup` (4268) and `breath_elapsed` (3040; keep the public wrapper `timer/mod.rs:126`) only pass through; 2484-2486 and 2717-2719 hand-copy `From<TimerMode>` (55-61). `PKGDATADIR` (`config.rs:6-7`, `#[allow(dead_code)]`) has no reader; it is also set in `build.rs:49-50`, `src/meson.build:14` and the Flatpak manifest (37).

### #85 Dead code, duplicates and a test that cannot fail
- Related: #375 (`preload_end_bell`), #399, #401, #405, #418.
- Core: `tick_box_breath` (`mod.rs:604-667`) calls `fire_due_bells` at 658-665 though core never gives Box Breath bells (test `box_breath_session_fires_interval_bells` 1541 injects them). `db_open_failure_key` (`format.rs:308-315`, tests 945-961) is unused and repeated in gtk `application.rs:178-187` (which matches gtk's own `DbError`). `Stopwatch` serde derives (`timer.rs:5-12`) and the restart test (86-113) are unused and assert a false claim (related: #273 touches `Stopwatch`). `list_box_breath_phases` (`box_breath_phases.rs:17-48`) repeats `get_box_breath_phase` (52-76; related: #285, whose callers should all use the list); seed uuids hard-coded at 142, 144 (use `seeds::BUNDLED_BOWL_UUID`, `BUNDLED_PATTERN_PULSE_UUID`). `list_bell_sounds_for_category` (`bell_sounds.rs:240-264`) copies `list_bell_sounds` (206-233) except the WHERE. `count_labels_from_db` (`labels.rs:65`, export `db/mod.rs:80`) is test-only.
- GTK: `preload_end_bell` (`sound.rs:294`; calls `timer/imp.rs:745, 792, 1284`, `window/imp.rs:126`) builds media that `play_end_bell_uuid` (322) never uses; its only effect is stopping a ringing end bell when its settings change (decide). Stale preload docs: `sound.rs:10-15, 289-290, 331-332`, `timer/imp.rs:677-679, 1218`.
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
Related: #314 (one bell block instead of six): do together, or the six copies get fixed one by one.
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
- R15(c), one `SlidePage` frame for 13 pages: every page reaches into its own Flickable, so each needs rework and a retest, for a repeated frame and no bug. #460 found a reason: each page lets go of focus its own way.
- R15(d), merging `VerticalSpinBox` and `StepperRow`: about -40 lines in the focus and keyboard code the cursor bugs came from. Note (#461): VerticalSpinBox lacks `release()`.
- R2b, all database writes off the UI thread: about 175 call sites, and no lock wait noticed in practice.
- Closing the file picker before its copy ends (#37): the read grant on the picked file can end with that screen, so every pick path would need the file opened first; a local copy takes about a second.
- Typed `BellSlot` through Slint: large, with no host safety net.
- Moving the whole sync account Save/Test into core: the keychains are per app.
- Moving the Log edit build into core: low value; reconsider when fixing #5/#6.
- Splitting `ui.rs` per screen or the big core files: cosmetic.
- A `Divider` component, a page stack model, caching JNI classes, moving the CAN_DUCK policy to Rust, a typed `SettingKey` enum, a core pending-Undo queue.
- Back on the Done screen discarding the session (decided behaviour).
