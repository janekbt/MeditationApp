# Tasks: writes-all-or-nothing

Base: f23960b89a62741eaa4ce600edd4ad26a08b550c; suites on it: core 1512 + 18, GTK 171, Android 298 green (run before its commit on 2026-11-10; tree unchanged since)

## Loop
- [x] 0 Skills loaded: `/ponytail ultra`, `slint:slint` (loaded; no `.slint` touched, Android Rust glue only)
- [x] 1 Understand (confirmed 2026-11-10 "Go"; #291 session start -> EE; review R6-1/R6-2 load-path defaults -> EE with #308)
- [x] 2 Propose (approved: 2026-11-10 "Go", after narrowing #292 to switch-on presets)
- [x] 3 Build (core 1526+18, GTK 171, Android 298 green; clippy GTK+core all-targets, CI lib -D warnings, Android target: clean)
- [x] 4 Review: eight rounds (reviewer agent + `/code-review high` each), then `/simplify` (see Notes)
- [x] 5 Specs, docs, list (deltas: presets, box-breath, guided, timer MODIFIED; check-specs green but for the two old timer tests the timer delta replaces at archive; ARCHITECTURE, ENTITIES; OPEN-ISSUES: K3 ticked, #308 extended, #498 #499 new; REFACTORING: #317 done and out, #495 note, #500-#504 deferred)
- [x] 6 Verify and hand over (device test FP5 + laptop, maintainer: "alles läuft", 2026-11-10)
- [x] 7 Ship (deltas merged, archived 2026-11-10; committed after the go)

## Review Focus
- A write nested in an outer one that fails -> the whole outer write rolls back: `a_preset_that_fails_halfway_stores_nothing`, `a_breath_pattern_that_fails_halfway_stores_nothing`.
- A nested write whose own error a caller catches and goes on -> its savepoint rolls back only its part: 1.1.
- Events and the Lamport clock of a rolled-back write -> none stored, nothing sent: asserted in both "fails halfway" tests (pending events unchanged). The change signal is counted only when the outermost write commits, and an undone (nested) write takes back its own mark (CR2-5, CR4-4).
- The database lock -> the outermost write stays IMMEDIATE (waits for the lock as today); savepoints only inside it: 1.2.
- A replayed event / old database / second device -> no event, payload or schema change; the round trip `every_local_write_is_stored_as_a_peer_stores_it` stays green.
- An empty preset (no bells, no label) -> covered by the #292 test and the existing apply tests.
- The session already gone before Undo (Log delete, sync tombstone) -> no play taken back: `undo_after_the_session_is_gone_takes_no_play_back`.
- A deleted local file before Undo -> the update matches no row, the delete still happens: 5.1 asserts the session goes.
- Crash recovery's Undo (Android `Recovery`) -> counted no play, takes none back: 5.2.
- Another language -> no new text.

## 1. Nestable transactions (#193 infrastructure)
- [x] 1.1 `Database::tx()` guard (planned as `in_tx(|| ..)`, see Notes): outermost an IMMEDIATE transaction, nested a SAVEPOINT; error rolls back to its own start. Files: `db/mod.rs`. Proof: `a_nested_write_that_fails_rolls_back_only_itself` and `an_outer_write_that_fails_rolls_back_the_nested_ones`. Expected: FAIL (stub) then PASS.
- [x] 1.2 Every `unchecked_transaction` in `db/` goes through `tx()`; the `_in(tx, ..)` twins fold into their plain function where they only existed to share a transaction. Files: `db/*.rs`. Proof: suites green; `grep unchecked_transaction meditate-core/src/db` shows none.
- [x] 1.3 `seed_once` writes the seed and its done marker in one `tx()` (#306 note). Files: `db/seeds.rs`. Proof: `a_seed_that_fails_leaves_no_done_marker`.

## 2. Preset apply (#193, #292)
- [x] 2.1 `apply` runs in one `tx()`, writes the length (`set_timer_session_secs` / `set_breathing`) clamped as the apps clamp it today (`clamp_session_secs`, `BreathPattern::clamp_from_raw`), switches bells via the bell edit, not one transaction per bell. Files: `preset_config.rs`. Proof: `a_preset_that_fails_halfway_stores_nothing`, `applying_a_preset_stores_its_length`.
- [x] 2.2 GTK `apply_config` and Android `apply_preset_json` drop the re-apply and the `before` snapshot; their timing step only updates widgets. Files: `meditate-gtk/src/timer/imp.rs`, `meditate-android/src/ui.rs`. Proof: the two `a_failed_preset_apply_is_not_written_twice` (replace `a_preset_that_fails_halfway_is_put_back`).
- [x] 2.3 #292: label switch on and no uuid writes the empty label setting (= mode default); switch off writes only the switch. Proof: `a_preset_without_a_label_picks_the_mode_default`, `a_preset_with_the_label_off_keeps_the_label_behind_it` (pin).

## 3. Snapshot passes errors on (#291)
- [x] 3.1 `snapshot` returns `Result`; every read in it (settings, label, bells, phases) passes its error on; a value that reads but doesn't parse keeps today's default. Callers: GTK `snapshot_current_setup` (save, preset tap Undo, override), Android `snapshot_setup_json`; each failure says "Save failed" and stores or applies nothing. Proof: `a_setup_whose_bells_cannot_be_read_is_not_saved`.
- [x] 3.2 OPEN-ISSUES: the session-start half of #291 moves to #308 (EE).

## 4. Box Breath (#306, #325)
- [x] 4.1 `set_breathing` in one `tx()`. Files: `settings_keys.rs`. Proof: `a_breath_pattern_that_fails_halfway_stores_nothing`.
- [x] 4.2 `edit_box_breath_phase(phase, Fn(&mut BoxBreathPhase))`: compares on a read first and locks only for a change, reads again inside the write, unchanged sends nothing, a row not seeded yet is made first (R2-1, CR2-3); `set_box_breath_phase` callers that change one field use it. Files: `db/box_breath_phases.rs`, GTK `timer/imp.rs` (switch, Type, sound, pattern), Android `ui.rs` phase helper (no defaults on a failed read), the volume write. Proof: `editing_one_field_keeps_a_newer_volume`; MANUAL step in the delta.

## 5. Auto-Finish Undo (#324)
- [x] 5.1 Core `undo_session_save(uuid, local_file_id)`: delete plus `play_count = MAX(play_count - 1, 0)` in one `tx()`, the play taken back only when the delete removed a row. Files: `db/session_in_progress.rs`. Proof: `undoing_a_saved_session_takes_its_local_play_back`, `undoing_a_save_never_counts_below_zero`, `undo_after_the_session_is_gone_takes_no_play_back`.
- [x] 5.2 GTK toast Undo and Android's Auto-Finish snackbar pass the saved session's local file id; Android's crash-recovery Undo passes none. Files: `meditate-gtk/src/timer/imp.rs`, `meditate-android/src/ui.rs`, `app.rs` (`PendingUndo`). Proof: suites green; device test.

## 6. Wrap up
- [x] 6.1 Clippy (GTK + core all-targets, CI lib `-D warnings`, Android target); GTK and Android suites; LOC touched and whole app against f23960b into Notes.

## Notes
- Base suites not re-run: same commit, clean tree, full suites ran on it before its commit today.
- Pre-build questions (fresh agent), answered (all my call): (1) every read in `snapshot` passes its error on, not only bells and phases, since a failed label read would now reset the label; (2) a missing phase row in `edit_box_breath_phase` is an error so the pick says "Save failed" instead of vanishing; (3) Undo takes the play back only when the delete removed a row; (4) core clamps the length it now writes, as the apps do today, so the preset still shows as active.
- 1.1 built as a guard, `Database::tx()` returning `Tx` (commit, or undo on drop, also on a panic), not the closure `in_tx(|| ..)` the plan named (my call): every one of the 37 sites keeps its `let tx = ...; tx.commit()` shape, and the `tx` witness that `emit_event` needs stays. Tests 1.1 failed with a stub (no transaction), pass now.
- 1.2: twins folded: `set_setting_in`, `delete_synced_row_in`, `edit_preset_in`, `edit_guided_file_in`, `delete_label_in`, `update_session_in`, `insert_label_in`, `insert_bell_in`/`update_bell_in`/`edit_bell_in` (now `insert_bell`/`edit_bell`), `insert_session_tx_less` (now `insert_new_session`); `label_for_import` and the starred-order retimers lost their `tx` parameter. `set_transaction_behavior(Immediate)` is gone: `tx()` says BEGIN IMMEDIATE itself. Shared write helpers that take `&Tx` (`write_*_in`, `emit_and_rebuild_in`, `insert_new_in`, `insert_sessions_in`) stay.
- 2.3: `a_preset_with_the_label_off_keeps_the_label_behind_it` passed at once (the behaviour existed, a pin); with switch-off writing the default it failed, restored.
- 2.2: Android `apply_preset_json` drops the timing step entirely (`load_setup_for_mode` reloads the length from the DB); GTK (final, after review rounds 1-3): `apply_config` is core's apply, then `refresh_streak`, which reloads length, pattern, stopwatch and hero from the DB. Source tests `a_preset_that_cannot_apply_says_so` (Android) and the GTK failed-save test follow. Android `ui.rs` only compiles for the Android target: checked by its clippy at step 6.
- 3.1: `snapshot` returns `Result`; GTK `snapshot_current_setup` and Android `snapshot_setup_json` return `None` after saying "Save failed" (tag `preset.snapshot`); "Save current setup" then opens no chooser, a preset tap applies nothing. `active_presets` reads it too: unreadable shows no preset active (a read path, EE/#308). The GTK fallback config for "no app" is gone with it.
- 4.2: `edit_box_breath_phase(phase, Fn)` in core (FnOnce until CR2-3); `bell_volume::write` for a phase goes through it (a row not seeded yet is made first, R2-1). GTK: the wrapper `set_box_breath_phase` became `edit_box_breath_phase`; switch, Type, sound and pattern use it, `p_for_pick` is gone. Android: `write_bb_phase` takes the edit; `write_bb_phase_locked` and its default row are gone. Tests `editing_one_field_keeps_a_newer_volume` and `editing_a_missing_phase_says_so` failed with a do-nothing stub.
- 5.1: tests live in `db/local_guided_files.rs` (its fixtures), spec paths follow. `undo_after_the_session_is_gone_takes_no_play_back` passed with the do-nothing stub (it guards against a decrement); with the guard removed it failed, restored.
- 5.2: GTK `save_ended_session` returns `(Session, uuid)`; the toast's Undo calls `undo_session_save(&uuid, local_file_id)`. Android `PendingUndo::Recovery(uuid, Option<i64>)`: auto-finish passes `session_local_file()` (set only at session start), crash recovery `None`.
- LOC (non-test, `loc.py f23960b`, after all review rounds and /simplify): touched files 22,853 -> 22,800, net -53; whole app 61,406 -> 61,353 (no files added or removed). After the build step it was -131; the review fixes added the write-depth bookkeeping, the read-first paths and the fallible readers.

### Review round 1 (reviewer agent R + /code-review high CR)
- Fixed, R1 = CR2 (IMPORTANT): GTK `apply_config` wrote the length again after core's apply (`set_countdown_target`, `set_breathing_duration_secs`); a lock there said "Save failed" with the preset stored and no Undo. Now under `timer_populating` / `breathing_populating`, widgets only; the GTK source test asserts the guards (FAIL first).
- Fixed, R2: `a_preset_without_a_label_picks_the_mode_default` now also asserts the preset shows as active.
- Fixed, R3: `a_breath_pattern_that_fails_halfway_stores_nothing` starts from a stored pattern and asserts it stays; failed with the write bracket removed, restored.
- Fixed, CR1: `seed_once` took the write lock at every start. It now reads the marker first (and again inside the write); `seed_box_breath_phases` (which already took it before this change) counts its rows first. Test `a_seeded_database_seeds_without_the_write_lock` (FAIL first, on the phase seed).
- Fixed, CR3: `Tx::drop` logs a failed undo (`db.tx`); before it was silent, as rusqlite's own drop was.
- Fixed, CR4: `active_presets` logs an unreadable setup (`preset.active`); a message for it stays with #308 (EE).
- Fixed, CR6: one phase write path: `set_box_breath_phase` is a whole-row `edit_box_breath_phase`, one read per write.
- Deferred to /simplify, CR5: `snapshot`'s fallible read closures copy the parse rules of `settings_keys::read_*`.
- Rejected, CR7 (savepoint per row in merge, retime, import): a SAVEPOINT/RELEASE inside one transaction is a journal mark without fsync; each row already records an event and rebuilds a row, which costs far more.
- Rejected, CR8 (`undo_session_save` checks the session, then the delete checks again): one indexed lookup on a tap; one source of truth would change all seven `delete_synced_row` callers.
- Rejected, CR9 (Android Undo reads `session_local_file()` after the save): the slot is set only at session start, the save and the notice run back to back on the UI thread, and the save reads the same slot.

### Review round 2 (fresh agent on the round-1 fixes + /code-review high)
- Fixed, R2-1: my pre-build call "a missing phase row is an error" rested on a wrong premise; the rebuild upserts, so a write to an unseeded phase used to create it. `edit_box_breath_phase` now makes the row as the seed does (INSERT OR IGNORE) and the change lands. Test `editing_a_phase_not_seeded_yet_keeps_the_change` (replaces `editing_a_missing_phase_says_so`; FAIL first). The two test setups that seeded phases for round 1 are back as they were.
- Fixed, R2-2: the lock test is renamed to what it proves (`a_seeded_database_seeds_without_the_write_lock`). That opening the database may still take the lock (`PRAGMA user_version` at every open) is older than this change and unverified: OPEN-ISSUES at step 5.
- Step 5, R2-3: `active_presets` on an unreadable setup (log only, no preset active) goes into #308's text.
- Fixed, CR2-1 (#292 after a label merge): a preset saved with the label on and none picked wrote the empty setting even when the mode's bundled default had been merged away, so the mode lost the kept label and sessions saved without one. `apply` now writes the empty setting only while the default label exists; otherwise the mode keeps what the merge set. Test `a_preset_without_a_label_keeps_the_label_its_merged_default_went_into` (FAIL first); `a_preset_without_a_label_picks_the_mode_default` seeds the default labels, as every app start does.
- Fixed, CR2-2 = R2-1.
- Fixed, CR2-3: `edit_box_breath_phase` took the write lock before seeing that nothing changes (Setup echoes stored values back). It now compares on a read first and takes the lock only for a change (`edit` is `Fn`, run once on the read, once inside the write). Test `an_unchanged_phase_write_takes_no_lock` (FAIL first).
- Fixed, CR2-5: the local-change signal fired at `emit_event`, inside the open write, so a sync started on it could find nothing committed and the signal was spent; one preset apply holds about 20 events. `emit_event` now marks the change, the outermost `Tx::commit` counts it, an undone write drops it. Test `a_change_is_seen_once_its_write_commits` (FAIL first).
- Fixed, CR2-8: GTK `apply_config` no longer borrows the loading guards: `show_countdown_target` / `show_breathing_duration_secs` update the widgets, the `set_*` setters are show + save. The source test asserts `show_*` and no `set_*` in `apply_config`.
- Step 5, CR2-6 = R2-3 (#308).
- Deferred to /simplify, CR2-7 = CR5.
- Rejected, CR2-4 (after SQLite rolls back a whole transaction on its own, a caller that catches the nested error and goes on would commit later writes one by one): no caller catches a nested error inside a write (rounds 1 and 2 grepped `let _ =`, `.ok()`, `if let Err` in db/); the seeds swallow only duplicate-name errors, which never roll back a transaction.
- Rejected, CR2-9 (`label_for_import` checks the uuid, `insert_label_with_uuid` checks again): one indexed query per new imported label.
- Clippy noted MSRV 1.75 against `Option::is_none_or`; written with `is_some_and`.

### Review round 3 (fresh agent on the round-2 fixes + /code-review high)
- Fixed, R3-1 (IMPORTANT): CR2-1's check only kept the old #292 bug alive after a merge (a later pick, "Walking", stayed when the preset was applied; the preset never showed as active). The merge itself now carries it: `retarget_label` also pins a preset of the mode whose default is merged away, label on and none picked, to the kept label (synced as a preset edit). `apply`'s check stays for presets from merges before this change. The merge test now picks "Walking" after the merge and asserts "Sitting" plus the preset active (FAIL first).
- Fixed, R3-2 and R3-3: GTK `apply_config` is now apply, then `refresh_streak` (which reloads length, pattern, phase tiles and stopwatch from the database) and, in Box Breath, `refresh_hero_for_idle`. The `show_*` helpers are gone; both setters are as on the base. The source test asserts no `set_*` and no `PresetTiming::` in `apply_config`.
- Fixed, R3-4: note 4.2 says `Fn`.
- CR3-1 = R3-1 (fixed above).
- Rejected, CR3-2 (Undo of a preset can't bring back an empty label behind a switched-off switch: the snapshot stores switch off + none, and `apply` leaves the label behind an off switch alone): that is the behaviour the maintainer approved ("switch off only switches it off", 2026-11-10), and the base did the same. Shown to the maintainer.
- Deferred, CR3-3 (`set_setting`, `edit_bell`, `edit_preset`, `edit_guided_file`, `update_label`, `update_session` take the write lock before seeing nothing changes): as on the base (each opened its transaction first there too); only the phase edit, which this change had made worse, compares first now. A general "read first, lock only for a change" goes to OPEN-ISSUES (EE) at step 5.
- Fixed, CR3-4: `Tx::drop` skips its undo when the connection is already back in autocommit (SQLite undid the whole write itself, disk full): no misleading "undo failed" lines. A caller going on after such an error stays rejected as CR2-4 (none exists).
- Rejected, CR3-5 (`seed_box_breath_phases` counts rows of unknown phases): the table's CHECK allows only the four phases, so no other row can exist.
- CR3-6 = R2-3 (#308, step 5). CR3-7 = CR5 (/simplify).
- Fixed, CR3-8: `apply`'s default-label check is one keyed lookup (`existing_rowid_by_uuid`, now `pub(crate)`) instead of the whole label list.

### Review round 4 (fresh agent on the round-3 fixes + /code-review high)
- Fixed, R4-1 (IMPORTANT): the CR2-1 guard (presets from merges before this change) had no test left. `a_preset_from_before_a_merge_keeps_the_kept_label` (a pin: passed at once; with the guard removed it failed, restored).
- Step 5, R4-2: deleting (not merging) the mode's default label leaves a "default" preset unable to pick a label, as on the base: OPEN-ISSUES.
- Fixed, R4-3: GTK's extra `refresh_hero_for_idle` is gone; `refresh_streak` already refreshes the hero through `refresh_stopwatch_dependent_ui`.
- Fixed, R4-4: `apply` returns `Result<()>` (no caller read the timing any more); its doc and GTK `apply_config`'s doc say what happens now; the stale notes 2.2 and 4.2 are corrected.
- Fixed, R4-5: `merging_a_default_away_leaves_other_presets_alone` (a Box Breath "default" preset and a Timer preset with the label off keep their JSON; a pin, failed with the mode check removed, restored).
- R4-6: CR3-4 (`Tx::drop` skipping the undo in autocommit) has no test by design: only a disk-full or I/O error makes SQLite roll back a whole write itself, and the effect is a log line not written.
- Fixed, CR4-1 (a failed outer ROLLBACK left the connection in a transaction no write owned, and every later write nested in it as a savepoint that never committed; before this change `unchecked_transaction` would have refused): `Database` counts its open writes (`depth`); the outermost write is decided by that count, and one that finds a transaction no write owns ends it first (logged). Test `a_transaction_left_open_by_a_failed_undo_does_not_swallow_later_writes` (FAIL first).
- Rejected, CR4-2 (= R4-2, a deleted default label): as on the base; the guard can't tell a deleted default from one merged before this change, and writing the empty setting would label sessions with nothing after such a merge. OPEN-ISSUES at step 5.
- Fixed, CR4-3: Android's "Save current setup" in Box Breath read the pattern and length through `read_breathing`, which turns a failed read into 4-4-4-4. Core has `try_breathing_from_db`, `try_read_bool`, `try_read_u32` (the old readers wrap them); Android reads the timing inside the snapshot's lock with the error passed on; `snapshot`'s closures use the `try_*` readers, so the parse rules have one copy (CR5 done). Test `an_unreadable_breath_pattern_is_an_error_not_the_default` (FAIL first with a stub).
- Fixed, CR4-4: a nested write that is undone takes back only its own change mark (`Tx` remembers the mark from before it); test `a_nested_write_undone_sends_no_change_signal` (FAIL first).
- Fixed, CR4-5: `write_bb_phase`'s doc and note 4.2 describe the made-first row.
- CR4-6 = R2-3 (#308). CR4-7 = CR5, done in CR4-3.

### Review round 5 (fresh agent on the round-4 fixes + /code-review high)
- Fixed, R5-1: Android's phase-length step (`on_bb_adjust`) and Box Breath length commit read the pattern through `read_breathing` (4-4-4-4 on a failed read) and wrote all five values back, so a failed read stored and synced the defaults (as on the base). Both now start from `breathing_to_change`, which reads with the error passed on and says "Save failed".
- Fixed, R5-2: source test `a_breath_pattern_that_cannot_be_read_is_never_written_back_as_the_default` pins `try_breathing_from_db` in `snapshot_setup_json` and `breathing_to_change`, and both handlers on it (FAIL first).
- Fixed, R5-3: `an_unchanged_write_after_a_change_keeps_the_change_signal` (a pin: passed at once; with `Drop` clearing the mark always, it failed, restored).
- Rejected, CR5-1 (GTK preset Undo after a mode switch applies the old mode's snapshot): `on_mode_switched` already dismisses every toast (`window.dismiss_toasts()`), as Android closes its snackbar.
- CR5-2 = CR4-2/R4-2 (rejected, OPEN-ISSUES). CR5-4 = R2-3 (#308). CR5-5 = CR3-3 (deferred, OPEN-ISSUES). CR5-7 = CR8 (rejected).
- Fixed, CR5-3: a nested write started after SQLite rolled the whole write back by itself (disk full) would have opened a top-level transaction and committed alone; `tx()` now refuses it (SQLITE_ABORT). Test `a_write_inside_one_sqlite_undid_itself_fails` (FAIL first).
- Fixed, CR5-6: Android's Undo of an auto-finished save reloads the Local Folder page (when open) and the guided lists, so the play taken back shows; the source test `an_auto_finished_session_is_saved_with_undo` asserts it (FAIL first). GTK's Undo invalidates everything already.
- Rejected, CR5-8 (`set_box_breath_phase` is a whole-row wrapper; a phase write reads the row up to three times): `apply` and about twenty tests call the whole-row form; the reads are four keyed rows per Box Breath preset.
- Fixed, CR5-9: the new seed tests use `test_helpers::sql` like the other new tests.

### Review round 6 (fresh agent on the round-5 fixes + /code-review high)
- Decided (maintainer, 2026-11-10: with #308 in EE), R6-1 = CR6-1, and R6-2: Linux still loads the Box Breath pattern through the reader that turns a failed read into 4-4-4-4, then writes all five values from those cells on a phase step or a length commit, and snapshots from them; in both apps the Timer length cell holds the default after a failed read and the snapshot takes it from there. Goes into #308's text at step 5.
- Fixed, R6-3: the doc comment of `setup_values_load_from_one_place` is back on it.
- Fixed, R6-4: the source test asserts `breathing_to_change` says "Save failed", gives `None` and names no default.
- Fixed, CR6-2: `finalize_session_in_progress` (every start) reads first and takes the lock only when there is a session to recover. Test `a_start_with_nothing_to_recover_takes_no_lock` (FAIL first).
- Fixed, CR6-3: a nested write whose undo failed may leave its writes inside the outer one; `Database` marks that, and the outer write then fails as a whole instead of committing half of it. Test `a_nested_write_whose_undo_failed_fails_the_whole_write` (FAIL first).
- Rejected, CR6-4 (Android reads the pattern and writes it under two locks; a sync landing in between is undone): two devices editing at once, which is fixed only when almost free (one-user sync); here it would need a core edit for the pattern; as on the base.
- Fixed, CR6-5: Android's dropped-write source check also forbids `let _ = db.edit_box_breath_phase(`.
- CR6-6 = R2-3 (#308).
- Fixed, CR6-7: `edit_box_breath_phase` has one helper for "the edited row, or none when unchanged".
- Fixed, CR6-8: task 4.2 and the Review Focus describe the shipped code.

### Review round 7 (fresh agent on the round-6 fixes + /code-review high)
- Agent: Quality PASS. Fixed, R7-1: the source test for `breathing_to_change` asserts its failure arm ends in `None` and that it names no defaulting reader (`read_breathing(`, `::breathing_from_db(`, `box_breath()`, `BREATHING_DEFAULT_SECS`).
- Rejected, CR7-1 (an outermost write that ends without a commit and whose ROLLBACK fails returns Ok and holds the lock until the next write): a ROLLBACK in WAL mode practically never fails; the failure is logged and the next write ends the stray transaction (CR4-1). Before this change rusqlite's drop ignored the same error.
- Fixed, CR7-2: the #292 requirement says what a merged default does, with the two merge tests as scenarios (`a_preset_without_a_label_keeps_the_label_its_merged_default_went_into`, `a_preset_from_before_a_merge_keeps_the_kept_label`). A default deleted without a merge stays as on the base (R4-2, OPEN-ISSUES).
- Fixed, CR7-3: GTK `snapshot_current_setup` gives none in Guided instead of a made-up 0-second Timer timing that `apply` would now store.
- Fixed, CR7-4: the `snapshot` / `apply` section comment and `snapshot`'s doc say what they do now.
- Deferred to /simplify, CR7-5 (three copies of "read, or say Save failed and give none").
- Rejected, CR7-6 (Undo lowers the count as it is, not "the play this save added"): within the 8 s Undo window only another play of the same file could change it, and taking back one play is then still right in number.
- Deferred to /simplify, CR7-7 (GTK `save_ended_session` returns the uuid in a tuple instead of looking it up from the rowid).
- Fixed, CR7-8: an unchanged write to a phase not seeded yet keeps the row it made (commits), so the next echo is a read again. Test `an_unchanged_write_to_a_phase_not_seeded_yet_seeds_it` (FAIL first).
- Step 5: `openspec/specs/timer/spec.md` names the old `a_preset_that_fails_halfway_is_put_back` tests (both apps); that requirement needs a MODIFIED delta.

### Review round 8 (fresh agent on the round-7 fixes + /code-review high)
- Agent: Quality PASS. Fixed (doc): `snapshot_current_setup`'s doc names "Save current setup" and the Guided `None`.
- CR8-1 = R6-1 (EE with #308, decided). CR8-2 (the read-or-fail rule sits in Android's shell): the reader is core's `try_breathing_from_db`; the GTK half goes with #308 by the maintainer's decision, where it belongs in core for both apps. CR8-3 = R4-2 (OPEN-ISSUES). CR8-4 = R2-3 (#308). CR8-7 = CR8 (rejected).
- Fixed, CR8-5: `edit_box_breath_phase` returns an error instead of `expect` if the row it just made can't be read back (unreachable: the table's only constraints are the phase key and its CHECK). No further review round for it: it replaces a panic on an unreachable path with an error.
- To /simplify: CR8-6 (savepoint statements through `execute_batch`, not cached), CR8-8 (the SQLITE_ABORT error built twice).

### /simplify (after round 8; four agents: reuse, simplification, efficiency, altitude)
- Fixed: one `aborted(why)` for the SQLITE_ABORT error (was built twice); `Tx` does its bookkeeping (depth, change mark, broken mark) in `Drop` only, `commit` sets `committed` (no `mem::forget`); `delete_synced_row` returns whether it deleted, so `undo_session_save` asks once; `edit_box_breath_phase` makes a missing row through `seed_box_breath_phases`, and only when the first read found none; `breathing_from_db` falls back to `BreathPattern::box_breath()`; GTK `Session` carries its `uuid` (no `(Session, uuid)` tuple); GTK `apply_config` checks `setting_saved`'s result as Android does; Android `read_or_say` holds "the value, or say Save failed and give none" for the snapshot and `breathing_to_change` (the GTK snapshot keeps its one copy). Suites and all clippy runs green after it.
- Skipped, to REFACTORING (deferred, step 5): one "read first, lock only for a change" helper for the seeds, recovery, the phase edit and `set_setting`/`edit_*` (altitude; ties to CR3-3); readers that return `Result` instead of `x`/`try_x` pairs (altitude); a core `edit_breathing(db, |pattern, secs|)` so both apps change the pattern inside one write (altitude; with #308 for the GTK half); savepoint statements through the statement cache (efficiency, microseconds today); presets always carrying a real label uuid instead of "none = the mode's default" (altitude; changes stored presets).
- Skipped, kept: the `broken` mark (a full ROLLBACK on a failed nested undo instead would lean on a second ROLLBACK not failing); `set_box_breath_phase` (used by `apply`); the shells' dropped-write lists keep `set_box_breath_phase(` (core still offers it); per-row savepoints in `merge_labels` (a rare action, about 1-3 ms per 1000 sessions).

## Hand-over (step 6)
Artifacts: GTK `meditate-gtk/builddir/src/meditate` (17:14), APK `meditate-android/android/app/build/outputs/apk/debug/app-debug.apk` (17:15, debug). Suites: core 1540 lib + 18 integration, GTK 171, Android 299; clippy GTK+core all-targets, CI lib -D warnings and Android target clean.

Test steps (both apps unless named):
1. Box Breath, a phase's volume: drag Inhale's volume, then at once open Inhale's sound and pick another. Reopen: the new sound and the dragged volume (laptop is the MANUAL scenario; phone the same).
2. Timer: tap a preset with another length. The length shown is the preset's; Undo brings the old length back.
3. Box Breath: tap a preset with another pattern and length. Tiles and length are the preset's; Undo brings the old ones back.
4. Label: save a preset with the label switch on and the mode's own label (not picked by hand). Pick "Walking", tap that preset: the mode's label is back and the preset shows as active. A preset saved with the label switch off only switches it off; "Walking" stays behind the switch.
5. Box Breath on the phone: +/- on a phase and changing the length work as before.
6. Local Folder: play a file with Auto-Finish on until it saves itself, note its play count, tap Undo: the session is gone and the count is one lower again (Local Folder page shows it).
7. Everything else as before: saving settings, switching modes, sync.

Proposed commit message:
core: presets, Box Breath patterns and seeds are written all or nothing

Writes nest now: Database::tx() opens a transaction, or a savepoint
inside one, so a preset with its length, a Box Breath pattern and a
seed with its marker land whole or not at all, and the x/x_in pairs
fold into one function each. Saving the setup reports a failed read
instead of storing defaults; a preset saved with the mode's default
label sets it again, also after a merge; a phase change keeps a
volume saved since; Undo of an Auto-Finish save takes its local play
back. The sync signal counts a change only once it is committed.

K3: #193, #306, #291, #292, #325, #324.
