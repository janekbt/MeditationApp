# Tasks: sync-after-real-writes

Base: ef8329b119468ef57d1bafb406d0e9335e09e3de; suites on it: core 1540 + 18, GTK 172, Android 299 green (refresh-what-changed's final runs on this exact code; the commit only added specs and the archive). Lines at base: 61,930.

## Loop
- [x] 0 Skills loaded: `/ponytail ultra`, `slint:slint` (Android glue in `ui.rs`, no `.slint` planned)
- [x] 1 Understand (confirmed 2026-11-10: "go"; decisions: rebuild once, delete the scanner whole)
- [x] 2 Propose (approved 2026-11-10: "go")
- [x] 3 Build (core 1543+18, GTK 169, Android 299 green; clippy GTK+core all-targets clean)
- [x] 4 Review: round 1 + fix review (reviewer + `/code-review high` each); R2 fixes unreviewed; `/simplify` below
- [x] 5 Specs, docs, list (deltas sync ADDED, log MODIFIED; ARCHITECTURE sync paragraph; REFACTORING #484 #316 #283 #327 #507 out, P2 row, #334 note; OPEN-ISSUES #340 now Android only)
- [x] 6 Verify and hand over (device test 2026-11-10: "Passt alles")
- [x] 7 Ship

## Review Focus
- A write that needs a sync but records no event (Wipe Local, Push My Data, Sync Now, saving the account, a data import if it writes without events) -> explicit `trigger_sync()` stays; every removed `with_db_mut` site checked for this.
- A nested write undone inside a committed one -> only the kept part reports (`changed_before` becomes a `Refresh`).
- A write on the blocking pool (`with_db_blocking`) -> its change is taken on that thread with the lock held and acted on back on the main thread.
- `trigger_sync()` reads the DB itself -> called only after the lock is released (no deadlock).
- Log delete on page 3, Log edit -> the Log keeps its pages (the step 1 bug #342 must not come back).
- A replayed event from a sync -> never counted as local (the sync uses its own connection; `replay_events` emits nothing).
- Old database, second device, another language -> no schema, payload or text change.

## 1. Core
- [x] 1.1 `Database.changed: Cell<Refresh>`, ORed in `emit_event` with `Refresh::of(kind.entity(), target)`; nested undo restores the outer value; commit moves it into a `committed: Cell<Refresh>` and bumps the count; `take_committed()`. Files: `db/mod.rs`, `db/events.rs`. Proof: `a_write_that_changes_nothing_reports_nothing`, `a_committed_write_reports_the_screens_it_touches`, `an_undone_write_reports_nothing`.
- [x] 1.2 `active_presets(db, mode, &starred)`. Files: `preset_config.rs`. Proof: existing `active_presets` tests, adjusted.

## 2. GTK
- [x] 2.1 `with_db`/`with_db_blocking` act on `take_committed()`; delete `with_db_mut`, `with_db_blocking_mut`, the 500 ms timer, the hand `mark_stale` calls; keep explicit triggers where no event is written; Log keeps its pages. Proof: suites green; manual.
- [x] 2.2 `#[must_use]`, clippy `let_underscore_must_use`; handle what they flag; delete `dropped_writes` + `no_write_result_is_dropped`. Fixes #340 GTK. Proof: clippy clean.
- [x] 2.3 `MeditateApplication::get()`; `main_window()` at `sound.rs`, `local_folder.rs` (#507). Proof: suites green.
- [x] 2.4 New `active_presets` call in `timer/imp.rs`; rebuild coalesced on idle; drop the explicit rebuilds that only followed a write; startup drops its own committed changes; session save's prepend goes.

## 3. Android
- [x] 3.1 `list_starred_presets_for_mode` (ui.rs:689) passes its starred list to `active_presets`. Update the source tests in `app.rs` that pin the old calls. Proof: Android suite green.

## 4. Docs
- [x] 4.1 ARCHITECTURE.md (when a write syncs); REFACTORING.md #484 #316 #283 #327 #507 out, P2 row; OPEN-ISSUES.md #340 GTK half.

## Notes
- Pre-build questions (fresh agent, 2026-11-10), my calls: startup changes taken and dropped; Log restores its mark around its own deletes; snapshots keep live-UI timing (DB can lag after a failed write), so no shared `current_timing` in the apps; rebuild coalesced on idle; clippy lint as crate attribute; `get()` only for the five `Application::default()` lookups; the three core tests go in `db/events.rs` as the spec names them. #340's line is `window/imp.rs:715`.
- Build notes: the clippy `let_underscore_must_use` lint was tried and dropped: it flagged 52 deliberate `let _ =` (cairo drawing, GStreamer state, file cleanup), none a write; `#[must_use]` on `with_db` flagged two snapshot writes, now `if let Some(Err(e))` (still log-only, #323). The presets chooser's `on_changed` callback went (every call followed a write that now re-marks itself); a failed preset move still rebuilds. `prepend_session` went (Log marked by the save, re-reads when shown). Vacuous source test `snapshot_writes_do_not_start_a_sync` deleted (core decides: the snapshot records no event).
- Lines (same script both sides, non-test): 61,926 -> 61,755 (-171).
- Review round 1 (reviewer agent + /code-review high), fixes:
  - R1-1 Living specs named the deleted scanner test: a smaller `no_write_result_is_dropped` (every GTK file, `let _ =` or `.ok();` on a DB call) keeps the seven specs true (maintainer chose it); Log filter scenario MODIFIED to MANUAL (delta `specs/log`).
  - R1-2 Lint dropped without asking: asked; see R1-1.
  - R1-3 Log reloads from page 1 after a session save: asked, accepted by the maintainer.
  - R1-4 Idle rebuild painted a stale frame (mode switch, startup): explicit rebuilds synchronous again; only a write queues one, 300 ms after the writes pause, cancelled by an explicit rebuild (also fixes /code-review's rebuild per slider step).
  - R1-5 `wrote` unpinned: the source test checks its four steps.
  - R1-6 Stale comments (Android watch, format.rs test string).
  - CR: `data_changed(SESSIONS)` after the save -> `refresh_shown` (the write marked already); `Tx::commit` uses `take()`.
  - Rejected: Log's own delete clearing its mark by hand (CR altitude: a "views I update myself" parameter on `with_db` is more code for one caller); snapshot writes log-only (#323, pre-existing, EE batch).
- Fix review (round 2: reviewer agent + /code-review high), fixes NOT reviewed again (maintainer rule: no third round unasked):
  - R2-1 Preset move rebuilt only after 300 ms, so a second Alt+Up/Down used stale indices: every move rebuilds at once again.
  - R2-2 Save/Override in the presets chooser popped to a stale starred list: `back_to_setup` rebuilds, then pops.
  - R2-3 Scanner: cuts at `#[cfg(test)]\nmod` (code after a test helper is scanned again); the #191 check is back (`if let` + `Err(` on a setting write).
  - R2-4 preferences.rs account-save comments no longer name `with_db_mut`.
  - Accepted, told the maintainer: starring or reordering a guided file marks the Log (same event kind as a rename, which the Log cards show), so a Log scrolled to page 3 reloads from page 1 after it; the Manage star toggle shows its new row 300 ms after going back.
  - Rejected: multi-line `let _ =`/`.and_then` on writes (the scanner is line-based; `#[must_use]` catches the statement form); Log delete's mark restore (altitude, one caller); snapshot writes log-only (#323); presets rebuild on any Setup write (debounced to once per pause; the rebuild keeps focus).
- /simplify (4 agents): applied: `back_to_setup` placed above the chooser's doc; `data_changed(SESSIONS)` after the save again (one entry point; the earlier CR line reverted); core `changed`/`changed_before` docs; the scanner's word list inlined; proposal wording of the scanner. Skipped: reuse of i18n's `rust_sources` (tried: making its test module visible breaks the i18n tests' `mod tests` cut); the duplicated cancel line (needed in both: the queue restarts its delay); `back_to_setup` rebuilding when nothing is queued (one extra small rebuild); Log delete's mark restore (as above).
- Final: core 1543+18, GTK 170, Android 299 green; clippy GTK+core all-targets and CI lib -D warnings clean; check-specs: only the Log test the delta replaces at archive.

## Hand-over (step 6)
Built 2026-11-10: GTK `meditate-gtk/builddir/src/meditate` (21:05:15), APK `meditate-android/android/app/build/outputs/apk/debug/app-debug.apk` (21:06:02, debug).
Lines (non-test, same script both sides): 61,926 -> 61,764 (-162).

Test steps:
1. Laptop, Setup: set the Timer length to a starred preset's length: its row shows as active a moment later; set it back: the mark goes.
2. Laptop: Save Settings, create a preset, back on Setup the new row is there at once. Focus a starred preset, press Alt+Up twice quickly: it moves up two places.
3. Laptop, Log: Load more twice, delete a session on page 3, wait past Undo: the Log stays on page 3.
4. Laptop: change a setting; the phone gets it after its next sync (sync still starts after writes).
5. Phone: the starred preset list still marks the active preset.

Proposed commit (after the device test and a go):
gtk: sync only after a write that changed something

The Linux app started a sync after nearly every write, also when
nothing changed, polled every 500 ms for the preset list, and marked
stale views by hand at about 20 places. Core now records what each
committed local write touches (the same Refresh a sync reports);
GTK's with_db takes it and, when anything changed, starts the sync,
marks Log and Stats and queues the starred-preset rebuild.
with_db_mut, with_db_blocking_mut, the poll, the hand marks, the
chooser's change callback and the Log's hand-added card are gone; a
smaller scan replaces the dropped-write scanner. active_presets takes
the starred list its callers already read; one app lookup
(MeditateApplication::get); three toasts reach the main window while
a help window has the focus; Sync Now says when it can't prepare.

P2 step 2 (REFACTORING #484, #316, #283, #327, #507); OPEN-ISSUES #340
(Linux half).

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_012WGZUPYHL7TurMz6YSFVJs
