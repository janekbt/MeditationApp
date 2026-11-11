# Tasks: one-database-type-linux

Base: 55358162bd2f84099db5d0d314d1faeb11e2e335. Suites on it: core 1541 + 18, GTK 175 green (2026-11-11); Android not run (untouched; 300 green at this commit). Lines at base: 61,048.

## Loop
- [x] 0 Skills loaded: `/ponytail ultra`, `slint:slint` (loaded; no Android UI)
- [x] 1 Understand (say-back approved 2026-11-11: "go"; no decision for the maintainer)
- [x] 2 Propose (approved 2026-11-11: "go"; builder questions answered, see Notes)
- [x] 3 Build (final after /simplify: core 1542 + 18, GTK 171, Android 300 green; clippy GTK+core all-targets and CI lib -D warnings clean; check-specs green)
- [x] 4 Review (round 1: reviewer + `/code-review high`, fixes need no second round), then `/simplify` (see Notes)
- [x] 5 Specs, docs, list (no deltas, skip_specs; check-specs green; REFACTORING: #328 out, P3 ticked, #410 pointer to labels.rs; ARCHITECTURE crate line)
- [x] 6 Verify and hand over (laptop test 2026-11-11: "Alle Tests bestanden"; step 2 can't show the toast, the dialogs' live check blocks a taken name first, as intended; found an older bug, the swipe-back on the interval bell's sound chooser, noted as OPEN-ISSUES #511 by the maintainer's choice)
- [x] 7 Ship (no deltas, skip_specs; archived 2026-11-11; commit after the go)

## Review Focus
- A name clash on create or rename (label, preset, guided file, vibration pattern) -> still "«X» is already taken"; pinned by 1.1 and the label/pattern collision tests.
- A database from a newer version -> the recovery window still names both versions; core's `db_open_failure_key` (tested in core, `format.rs`) maps it.
- `open` on a missing folder -> still creates it; the mkdir failure stays a `Decode` error and lands in `DbOpenFailureKey::Other` as today.
- Seeding order -> bundled bells first, then `seed_all_non_audio`, as today; `test_db_in_memory` seeds the same set as `open` (1.2).
- A wrapper that did more than pass on (`create_label` reads the row back, `get_streak`/`get_best_streak` pass today's local date, `edit_interval_bell` lives in `preset_config`) -> its callers keep that work; the compiler finds every caller of a removed method.
- An old database, a second device, another language -> no data, event or text changes.

## 1. Pins
- [x] 1.1 `a_core_name_clash_keeps_its_text` checks only core's error: each of the four duplicate variants gives a text with the name, `DateOutOfRange` gives none. Change it to call `duplicate_toast_text(&core::DbError)`; stub it to compile. Expected: FAIL on the stub, PASS once moved; mutation (return `None`) fails it.
- [x] 1.2 (deleted by /simplify: `test_db_in_memory` lost its last user, so `open` is the only seed path) `test_db_in_memory_matches_open`: a test database has the same bell sounds, labels, presets, patterns and Box Breath phases as one from `crate::db::open` on a temp path. Expected: PASS at once (pin, noted); mutation (drop one seed) fails it.
- [x] 1.3 `the_app_opens_its_database_with_the_bundled_bells`: no production source outside `db/mod.rs` calls `Database::open(`; `application.rs` calls `db::open(`. Source test (the opener runs at app start, which the host test can't drive). Expected: FAIL (application.rs calls `Database::open`), PASS after 2.1.
- [x] 1.4 Core `db_error_displays_its_cause`: `DbError::DuplicateLabel("Zen")` displays "duplicate label: Zen", `SchemaVersionTooNew {5,2}` "db schema_version=5 exceeds build schema_version=2". Stub `Display` with `todo!()`-free empty text first. Expected: FAIL, then PASS with the moved impl.

## 2. One type
- [x] 2.1 Delete the shell `DbError`, `map_core_err`, the `Result` alias, the struct and its pass-through methods; re-export core's `Database`; `open`, the seed table and the four session shim functions stay as free functions. The label and pattern collision tests match core's variants.
- [x] 2.2 Move every caller: drop `.core()`; renamed wrappers become core's `*_from_db` calls; `get_streak`/`get_best_streak` pass `meditate_core::time::today_local()`; `create_label` callers read the row back as the wrapper did (or use core's insert + list if only the id is needed).
- [x] 2.3 `application.rs` uses `db_open_failure_key`; `data_io.rs` converts core's error.
- [x] 2.4 The small #328 items: `BellSoundCategory::General` line, `seed_all_non_audio` in `test_db_in_memory`, module docs, the seed doc's place.
- [x] 2.5 Whole GTK suite, clippy GTK+core all-targets, CI lib clippy.

## Notes
- Builder questions (fresh agent, 2026-11-11), my calls:
  - Re-exported `Database::open` would silently be core's (no bells): free `db::open`, pinned by 1.3; `sound.rs:626` test uses it too.
  - Core `DbError` has no `Display`: moved from the shell (1.4), texts unchanged.
  - CSV `Decode` text: first routed through core's `DataIoError` mapping, reverted in review round 1 (it also changed Delete All's text); the shell keeps `Db(e.to_string())`, same text as before.
  - `update_session` shim stays a free function with that name; `create_label` read-back miss returns `DbError::Sqlite(QueryReturnedNoRows)`, as today.
- Build notes:
  - 1.1 failed on the stub (no text), passes with the moved function. 1.2 passed at once (pin); mutation (test DB without the bell seed) failed it. 1.3 failed (application.rs used core's opener), passes. 1.4 failed on the empty Display, passes.
  - Five GTK wrapper tests deleted with the wrappers (`create_label_*` x3, `shell_vibration_pattern_*` x2); core covers the same: `inserting_duplicate_label_returns_err`, `update_label_to_case_variant_of_other_name_returns_duplicate_error`, `insert_vibration_pattern_with_duplicate_name_returns_duplicate_error` and the pattern round-trip tests.
  - Core also got `Debug` for `Database` (moved from the shell; the app struct derives Debug).
  - Core's `From<DbError> for DataIoError` now uses the moved `Display` instead of repeating its texts (same texts).
  - The one `create_label` caller (`labels.rs`) inserts and reads the row back, as the wrapper did.
  - `.core()` removed at 90 sites; `clippy --fix` (redundant_closure) turned 16 `|db| f(db)` into `f`.
- Review round 1 (reviewer agent + `/code-review high`), all checked and fixed:
  - CRITICAL (both): two source tests (`sound.rs` lead-in, `sync_runner.rs` retry) still expected the closure form `clippy --fix` removed; a third (`application.rs` guided names) the same, found by the rerun. Test strings updated; whole suite rerun after the last edit.
  - IMPORTANT (/code-review): routing the shell's `From<DbError>` through core's `DataIoError` also changed Delete All's text for a `Decode` error. Reverted to `Db(e.to_string())`, the old text.
  - MINOR: duplicated `clash_text` doc; `core_db` alias in `timer/imp.rs`; `test_db_in_memory` doc; `local_folder.rs:68` redundant closure; core Display test now pins all eight texts; REFACTORING #85 and #400 and OPEN-ISSUES `data_io.rs:74` pointers updated.
  - No fix changes behaviour, data or sync (the data_io revert restores the base text), so no fix-review round.
- /simplify (four angles), applied:
  - `duplicate_toast_text` folded into the private `clash_text` (one caller).
  - `test_db_in_memory` had no user left after the wrapper tests went: deleted, and with it pin 1.2 (one seed path now, nothing to drift).
  - Stale re-export comments; dead re-exports deleted (`BoxBreathPhase`, 14 seed constants nobody imports from `crate::db`).
  - Android comments that named the removed GTK wrapper (`ui.rs` create-label and opener) now name `labels.rs` / `db::open`.
- /simplify, skipped:
  - A core `open_seeded` with an `Io` error for the mkdir: core API and Android change beyond this refactor; REFACTORING #510.
  - A core create-label returning `Label`: already #410.
  - `use meditate_core::db;` per file to shorten the `meditate_core::db::*_from_db` paths: style churn in ~15 files, no gain in behaviour.
- Lines: 61,048 -> 60,392 (-656). Total since 16b1e43: 62,729 -> 60,392 (-2,337).

## Hand-over
Build: `meditate-gtk/builddir/src/meditate` (2026-11-11 09:56). Run with a fresh data dir: `XDG_DATA_HOME=/home/janek/.cache/meditate-dbtest LOCALEDIR=meditate-gtk/builddir/po`.

Test steps (laptop):
1. First start on an empty data folder: the bell lists (Starting, Interval, End bell) offer all eleven built-in sounds; a preview plays. Labels, presets, vibration patterns and Box Breath phases show their defaults.
2. Create a label named like an existing one ("Meditation" in another case too): "«…» is already taken". Same for a preset, a vibration pattern, a guided file rename.
3. Create a new label: it appears and is selected.
4. Run a short Timer session and save: it shows in the Log with Undo; Undo removes it. Save again, edit it in the Log (time, note): the change shows.
5. Stats and the streak on the Timer page count the new session.
6. Export CSV and import it again: the toast says how many came in.
7. Restart the app: everything is still there, no extra default rows.

Commit message:
```
gtk: use core's database directly

The Linux app wrapped core's database in its own type with a copied
error enum and about 60 pass-through methods, while 90 calls already
went around it. Callers now use core's Database and DbError, as
Android does; only the bundled-bell opener and the unix-time session
shim stay. DbError's Display moves to core. REFACTORING #328, which
completes P3.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_012WGZUPYHL7TurMz6YSFVJs
```
