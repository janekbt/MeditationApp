# Proposal: sync-after-real-writes

P2, step 2 of 3. Refactors: #484, #316, #283, #327, #507. Bug riding along: #340 (GTK half; the Android half is #427).
Maintainer goal: the line count goes down ("wehe wenn sich die LOC wieder erhöhen").

## Why
The Linux app starts a sync after nearly every write, also when nothing changed, through `with_db_mut` / `with_db_blocking_mut` (84 call sites); a 500 ms timer wakes the app for its whole life to rebuild the starred-preset list; about 20 write sites pick by hand which views go stale, and a forgotten one is a stale screen. Core's preset "active" check reads the starred list that both callers read again. A 70-line text scan guards dropped write results; the compiler can flag the common case. Five files look the app up by hand (`Application::default()` downcast), and three toasts go to a help window when it has the focus (#507). "Sync Now" drops a failed write silently (#340).

## What changes
- Core: the database remembers, per committed outermost write, which screens its local events touch (`Refresh::of`, as a sync does); a write that records no event (an unchanged value, an undone write) touches nothing. `take_committed()` hands it over and clears it. The `changed: Cell<bool>` becomes that `Cell<Refresh>`; Android keeps its `LocalChangeWatch` count as is.
- Core: `active_presets(db, mode, &starred)` takes the list the caller already read. The app snapshots keep building their timing from what the screen shows (it can differ from the database after a failed write); the stored-timing builder stays inside core.
- GTK: `with_db` and `with_db_blocking` take what the write committed, after the lock is released: anything → `trigger_sync()`, `mark_stale(refresh)`, and when Setup changed the starred-preset list rebuilds. A write queues the rebuild 300 ms after the writes pause (a held spin button or dragged slider rebuilds once); an explicit rebuild (mode switch, startup, preset tap's reload) runs at once and cancels the queued one. Changes written at startup before the window exists are taken and dropped there (startup starts its sync explicitly, as today). `with_db_mut`, `with_db_blocking_mut`, the 500 ms timer and the hand `mark_stale` calls go; writes that need a sync but record no event (Wipe Local, Push My Data, Sync Now, a saved account) keep their explicit `trigger_sync()`. The Log's own deletes keep its pages: `commit_all_pending` restores the Log mark it had before its write, so a sync's mark is not lost. Only Log and Stats have stale marks; the label lists and the guided list keep their own rebuilds. The session save's hand-prepended Log card goes: the Log is marked and re-reads from page 1 when shown (maintainer: accepted). `#[must_use]` on `with_db`/`with_db_blocking`; the `dropped_writes` scanner goes, and a 20-line `no_write_result_is_dropped` checks every GTK file for `let _ =` on a database call and a log-only `if let Err` on a setting write (maintainer, after the clippy lint proved too broad). Every statement the compiler then flags is handled the way the surrounding code handles a failed write.
- GTK: `MeditateApplication::get()` replaces the five hand lookups; `main_window()` at `sound.rs` and `local_folder.rs` (#507).
- GTK: Sync Now shows "Save failed" when clearing the refused uploads fails (#340), existing text.

## Decisions
- The preset list is rebuilt once after a real Setup write, no marks-only update (maintainer).
- The dropped-write scanner is replaced by a smaller check over every GTK file (maintainer, 2026-11-10: the clippy lint flagged 52 deliberate `let _ =`, none a write).
- After a session save the Log reloads from page 1 (maintainer).
- The scanner also caught `.ok()` / `.and_then` / `.unwrap_or` on a write and a log-only `if let Err` on a setting write; those are left to review (`surface_duplicate_toast`, its third check, no longer exists).
- Minor, my call: GTK checks right after each write instead of polling, so no timer; `get()` replaces only the five `Application::default()` lookups (widget `.application()` lookups stay); #283's shared timing builder stays core-only (see above).

## Dropped
- Android: no change beyond the new `active_presets` signature (it already syncs only after real writes). #283's shared timing builder for the app snapshots (they read the screen, not the database).

## Reaches
core `db/mod.rs`, `db/events.rs`, `preset_config.rs`; GTK `application.rs`, `db/mod.rs`, `window/imp.rs`, `timer/imp.rs`, `timer/mod.rs`, `sound.rs`, `volume_row.rs`, `local_folder.rs`, `recovery_dialog.rs`, `preferences.rs`, and every file with a `with_db_mut` call (presets, data_io, vibrations, guided, labels, bells, sounds, vibration_editor, log/imp); Android `ui.rs` (`list_starred_presets_for_mode`), `app.rs` (source tests that pin the call). Source tests to update: GTK `application.rs` `the_active_preset_is_marked`, `sync_runner.rs` `refused_uploads_show_by_name_and_retry_sends_them_again`, `timer/imp.rs` prepend test, `timer/mod.rs` `with_db_mut` check. No new text, no translations, no schema or wire change, no migration, Android build environment untouched. Docs: ARCHITECTURE.md (writes start a sync), REFACTORING.md, OPEN-ISSUES.md (#340 GTK half).
