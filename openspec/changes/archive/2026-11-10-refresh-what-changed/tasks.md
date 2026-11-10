# Tasks: refresh-what-changed

Base: 97869caf40af1fd501a10e71b746491dd8821c61; suites on it: core 1540 + 18, GTK 171, Android 299 green (K3's final run on this exact tree, 2026-11-10; nothing changed since)

## Loop
- [x] 0 Skills loaded: `/ponytail ultra`, `slint:slint` (Android glue in `ui.rs`, no `.slint` planned)
- [x] 1 Understand (confirmed 2026-11-10: "ja, drei Schritte, fang mit Schritt 1 an")
- [x] 2 Propose (approved 2026-11-10: "go")
- [x] 3 Build (core 1539+18, GTK 170, Android 299 green; clippy GTK+core all-targets, CI lib -D warnings, Android target: clean; check-specs: only the labels test the delta replaces at archive)
- [x] 4 Review: rounds 1-10 (reviewer agent + `/code-review high` each), round 11 stopped by the maintainer; `/simplify` done (see Notes)
- [x] 5 Specs, docs, list (deltas sync, timer, log ADDED, labels MODIFIED; ARCHITECTURE sync paragraph; OPEN-ISSUES #98 #137 #346 #341 #491 out; REFACTORING #331 #378 #485 out, #397 trimmed, #505-#507 new, P2 row)
- [x] 6 Verify and hand over (device test 2026-11-10: steps 1-3 passed, "passt alles"; steps 4-12 skipped by the maintainer, so CR10-1 and CR10-2 stay untested)
- [x] 7 Ship

## Review Focus
- A pass that fails after applying (upload, file download, compaction) -> its changes still reach the screens: `a_failed_upload_still_reports_what_the_download_applied`.
- A pass that fails before applying (offline, no password, keyring, listing error, remote data lost) -> nothing: `an_attempt_that_fails_before_the_server_refreshes_nothing`.
- Several passes in one drain (a write during a sync) -> the changes are ORed; none is lost by a later empty pass. Both apps' drain code; checked on the device.
- Names reclaimed during replay (a suffixed label or preset gets its name back) -> they belong to a kind the same batch touched (the delete that freed the name); a reviewer should confirm no reclaim renames a kind outside the batch.
- Wipe Local, a new day -> everything refreshes (`Changes::all`).
- A hidden GTK page -> marked stale, refreshed when shown; the visible one at once.
- A replayed event, an old database, a second device -> no event, payload or schema change.
- Another language -> no new text; "Delete failed" exists in all nine.

## 1. Core: a sync says what it changed (#485, #137, #412)
- [x] 1.1 `Changes` (kinds touched + downloaded files) collected by `replay_events` and the file downloads into the pass, readable after a failed pass; `screens()` maps it to log / stats (with streak) / setup / guided / labels. Files: `db/events.rs`, `sync/orchestrator.rs`. Proof: `a_pulled_session_refreshes_the_log_and_stats`, `a_pulled_setting_refreshes_setup_and_stats`, `a_downloaded_audio_file_refreshes_the_guided_list_and_setup`, `a_sync_that_only_uploads_refreshes_nothing`. Expected: FAIL (stub) then PASS.
- [x] 1.2 A failed upload after a pull keeps the pull's changes. Proof: `a_failed_upload_still_reports_what_the_download_applied`.
- [x] 1.3 `run_attempt` returns the changes beside its result; every error before the pass reports none; `brought_changes` goes. Files: `sync/runner.rs`. Proof: `an_attempt_that_fails_before_the_server_refreshes_nothing`.

## 2. GTK: one refresh policy (#331, #378, #98/#342, #346, #341)
- [x] 2.1 Sync worker ORs the passes' changes; main thread calls one `data_changed(scope)`: marks stale views, refreshes the visible page, asks about label conflicts after every sync as today. `take_*_dirty()` replaces the four accessors; the "no app yet" branches stay (not shown to be dead), folded into one `map_or` line each. Files: `application.rs`, `window/imp.rs`, `log/mod.rs`, `stats/mod.rs`, `sync_runner.rs`. Proof: manual (spec sync, "Linux, the page that is not shown").
- [x] 2.2 New day through `data_changed(all)`, Timer page included (#346). Proof: manual (spec timer).
- [x] 2.3 Hand `invalidate` + refresh pairs (labels, guided, preferences, log/imp, timer/imp, recovery_dialog) become `data_changed`; delete the wrong comment, the second `trigger_sync` and the source-text tests in `labels.rs`. Proof: suites green; device test.
- [x] 2.4 Recovery Undo: `write_ok(.., Failed::Delete)` and `data_changed(all)` (#341). Files: `application.rs`. Proof: manual (spec log).

## 3. Android: refresh only what changed (#397 refresh half, #137, #412)
- [x] 3.1 Drain ORs `Changes` into the shared slot instead of the bool; the tick runs `refresh_after_pull(ui, &changes)` calling each helper once and only for touched screens; bell chooser only when open; label-conflict check on each sync edge as today; Wipe Local passes `Changes::all()`. Files: `ui.rs`, `sync_runner.rs`; replace the source test `screens_re_read_after_a_sync_brings_changes` (`app.rs`), which pins the old drain line. Proof: core tests above; device test.

## 4. Docs
- [x] 4.1 ARCHITECTURE.md sync section; REFACTORING.md: #485, #331, #378 done, #397 refresh half done (rest stays for step 3); OPEN-ISSUES.md: #98/#342, #137/#412, #346, #341 out.

## Notes
Build:
- 1.1 `a_sync_that_only_uploads_refreshes_nothing` passed at once with the stub: the behaviour existed (`brought_changes` was false for pushes). The other five failed on the stub for the right reason (`Refresh` empty).
- Old `brought_changes` tests went with it (four counter tests, `sync_reports_a_peer_session_as_a_change`, `sync_of_our_own_edit_reports_no_change`); the late-sound test became `a_downloaded_audio_file_refreshes_the_guided_list_and_setup`. `PutFails` moved to module level for reuse.
- Core gained `Refresh` (`of(kind, target)`, `NONE`, `LOG`, `STATS`, `ALL`, `SOUND_FILE`, `GUIDED_FILE`; named `Changes`/`screens()` in the proposal and 1.1 before building); GTK's `InvalidateScope` and `invalidate` are gone (`data_changed`), the three page-switch handlers are one, `reload_for_new_day` calls `data_changed(ALL)`.
- Android: `refresh_after_label_change`'s tail is `refresh_label_users`, so the pull re-reads the label name and summaries once (through `load_setup_for_mode`).
- Source tests updated where they pinned the old lines (GTK auto-finish Undo, Log filter, Android drain, label rows, guided follow); `name_changes_refresh_log_and_stats` deleted (#378), its spec proof is MANUAL in the labels delta. The new-day test kept its name (specs point at it).
Pre-build questions (fresh agent, 2026-11-10), answered, all my call:
- Q1 shape: inside `Sync` a `changes` cell filled during the pass, read with `sync.changes()` after it; `run_attempt` returns `(Changes, Result<SyncStats, RunError>)`; errors before the pass give `Changes::default()`.
- Q2 timing: recorded right after `replay_events` commits (a replay error rolls the batch back, so nothing); each downloaded file marks `files` as it lands, not from the final count.
- Q3 mapping (`Changes::screens`): Session -> log, stats. Label -> log, stats, setup, labels. Setting -> setup, stats. IntervalBell, BellSound, Preset, BoxBreathPhase, VibrationPattern -> setup. GuidedFile -> guided, setup, log (cards name the file). Downloaded files -> guided, setup. Android's widget follows setup or guided.
- Q4 label conflicts: unchanged, after every sync (once on Android); see proposal.
- Q5 GTK setup scope: `refresh_streak` (it reloads the whole Setup page, REFACTORING #312-area note); guided scope on GTK maps to nothing, as today.
- Q6 reclaim: `reclaim_suffixed_names` adds its kind to the touched set for every row it recomputes (may over-report, never under-report).
- Q7 streak: `data_changed` with stats or setup refreshes the Timer page always (cheap), which also fixes #346 for the new day.
- Q8 unknown kinds (a newer peer's): not a change; nothing shows them.
- Q9 GTK drain: `drain` takes `FnMut`, a local `Changes` ORs the passes and moves to the main loop.
- Q10 `SyncStats` keeps its counts for the diag log; `brought_changes` goes, its tests become the new ones.

Review round 1 (reviewer agent: PASS, 4 minor; `/code-review high`: 9):
- R-spec (scenario said Setup not re-read on a pulled session, GTK did re-read it): fixed by CR7 below; GTK now re-reads only the streak chip for `stats`.
- R1 runner path for a failed upload untested: `an_attempt_that_fails_before_the_server_refreshes_nothing` now runs a pass whose upload fails and expects the label; mutation check (moving `*refresh = ..` below the `?`) makes it fail. `PutFails` moved to `sync/fake.rs`.
- R2 guided download untested: the audio test now downloads a guided file too.
- R3 = CR3 daily-goal spin re-read Setup and Stats per step: the row only marks Stats (`mark_stale`); the Preferences close re-reads the shown page (`refresh_shown(setup)`), Stats only if marked.
- R4 window close did full re-reads: with CR7 a close re-reads at most the shown page's streak chip or a marked Log or Stats. Kept.
- CR1 Android: a guided file pulled during a session was never followed: a `guided_follow_due` flag in the tick follows it once idle.
- CR2 GTK new day went through `active_window()`: `reload_for_new_day` marks and calls its own window's `refresh_shown`.
- CR4 filter callers' refresh became dead: the filter and the filtered prepend only mark (`mark_stale`), as before; the callers' refresh does the work again.
- CR5 Android labels nested under setup: unnested.
- CR6 over-refresh: `Refresh::of(kind, target)`; a setting marks Stats only for the daily goal (`goal::DAILY_GOAL_KEY`); bell-sound downloads `SOUND_FILE` (Setup), guided downloads `GUIDED_FILE` (Setup, guided). Tests extended first (failed on the old mapping).
- CR7 GTK `stats` re-read the whole Setup: `refresh_shown` re-reads only the shown page; Timer: `refresh_streak` for setup, the new `refresh_streak_label` for stats.
- CR8 API: `BitOr` added, `mark` is one line, the `sync::Refresh` re-export dropped. Kept `NONE` (const for Android's static `Mutex`; `Default` stays for `Cell`).
- CR9 rejected: `attempt`'s out-parameter stays; the R1 test now fails if the read moves below the `?`, which is the case it raised.

Review round 2 (fix reviewer: PASS, no findings; `/code-review high`: 10):
- CR2-1 reclaim reported a kind for every recomputed row, also when the name stayed: it now compares the name after the recompute. Test first: `a_reclaim_that_changes_no_name_reports_nothing` (failed with labels+setup reported).
- CR2-2 `data_changed` reached the window only through `active_window()`: it now finds the main window among `windows()`; `reload_for_new_day` goes back to `data_changed(ALL)`.
- CR2-3 rejected: a peer's new label or starred guided file also reloads the Log. The event kind can't tell a guided star from a rename (both updates), a new label from another device is rare, and a Log reload on a peer's change is the accepted case in the proposal (my call, like a peer's session).
- CR2-4 the merge's `label_merged` call was dead after `data_changed(ALL)`: removed with the method and its wrapper; the source test asserts `data_changed(Refresh::ALL)` instead.
- CR2-5 session save re-read the streak twice: `mark_stale(STATS)` there (`reset_mode` re-read it).
- CR2-6 a created label reloaded the shown Log: `mark_stale(LOG)` as before.
- CR2-7 stale Android comment naming `InvalidateScope`: fixed. CR2-8 `Refresh::labels` doc said it drives the conflict check: fixed.
- CR2-9 notes named API that doesn't exist: corrected above; the docs in step 5 use the code's names.
- CR2-10 rejected: the tick locks an uncontended `Mutex` five times a second; not measurable next to the tick's other work.

Review round 3 (fix reviewer: PASS, 3 minor; `/code-review high`: 9):
- R3-1 orphaned `label_merged` doc comment on `get_app`: removed.
- R3-2 the reclaim's "name changed" branch untested: `a_pair_suffixed_the_old_way_heals_on_the_next_pull` (labels.rs) now asserts `refresh.labels`; mutation check (dropping the mark) fails it. A hand-built legacy state in events.rs tripped the UNIQUE index and was dropped; the existing healing test builds it the realistic way.
- R3-3 = CR3-4 the conflict prompt after a sync and the session save looked the window up through `active_window()`: one `MeditateApplication::main_window()` (first `MeditateWindow` among `windows()`) for `data_changed`, the conflict prompt and the save.
- CR3-1 a new day (or any `setup` change) re-read Setup on the Timer page under a paused session: `refresh_shown` re-reads Setup only when the session is idle (`reset_mode` re-reads it at the end); the streak chip alone is fine any time.
- CR3-2 Android: a pulled setting that switches the Setup label left an open chooser's tick: `refresh_label_users` runs for `setup` too.
- CR3-3 the save relied on `reset_mode` for the streak, which skips a mode that isn't current: back to `data_changed(STATS)` (one streak query; reverses CR2-5).
- CR3-5 the Log edit Save could reload the Log twice: `mark_stale(STATS)`, the dialog's own `imp.refresh()` re-reads the Log.
- CR3-6 `commit_all_pending` called `data_changed` twice: one call, `log: failed`.
- CR3-7 rejected: the page switch to Timer re-reads all of Setup every time; unchanged from the base (the old timer handler did the same); a Setup dirty flag belongs to P14 #312's one `load_setup()`.
- CR3-8 rejected: `SYNC_PULLED` can't be poisoned (nothing panics while it is held: a `|=` and a `mem::replace` on a `Copy` struct).
- CR3-9 `Refresh` constants now use `..Self::NONE`.

Review round 4 (fix reviewer: 3 minor; `/code-review high`: 8):
- R4-1 the save's two failure toasts still used `active_window()`: `main_window()`.
- R4-2 the CR3-2 source assert was true before the fix: it now pins `if refresh.setup || refresh.labels {` before `refresh_label_users`.
- R4-3 the CR3-1 idle guard had no test: `setup_re_reads_only_while_no_session_runs` (window/imp.rs, source test as GTK views can't run on the host).
- CR4-1 GTK: a pulled label change left the shown Log's filter list: `refresh_shown` re-reads the filter labels too when `labels` (GTK's first use of the field).
- CR4-2 the save marked and re-read a shown stale Log before adding the card, so the session could show twice: the card is added first, then `data_changed(STATS)`.
- CR4-3 the recovery toast at activate re-read Setup before the window's own staggered refresh: `mark_stale(ALL)` (the map handler's idle refresh runs after activate and takes the marks).
- CR4-4 the guided rename and its Undo used `ALL`: `GUIDED_FILE | LOG`.
- CR4-5 Preferences close and the recovery toast still used `active_window()`: `main_window()`.
- CR4-6 rejected: Android's `refresh.setup || refresh.labels` equals `setup` today, but it is written so a later mapping change (labels without setup, CR5 of round 1) can't skip the label rows.
- CR4-7 #491 was #341 again: removed from OPEN-ISSUES (EE row, its section, the "same class" line); REFACTORING lost the gone `label_merged` parameter and `labels.rs` invalidate lines.
- CR4-8 the `trigger_sync` doc described the old completion: rewritten; the loop line above updated.

Review round 5 (fix reviewer: PASS, 2 minor; `/code-review high`: 8):
- R5-1 the save's card-then-`data_changed` order unpinned: the auto-finish source test asserts it.
- R5-2 a new test sat between another test's doc and its fn: moved back.
- CR5-1 the idle guard sat in one caller of `refresh_streak` only (the map handler and the page switch skipped it): the guard is in `refresh_streak` itself (under a session only the chip; `reset_mode` drops the session first, so its re-read still runs); `refresh_shown` lost its copy; the source test moved to `timer/imp.rs`.
- CR5-2 #331's filter dirty bit and the `pub` dirty fields were dropped with the entry: REFACTORING #505 (P2 step 3).
- CR5-3 2.1 claimed the "no app yet" branches were removed: the text now says they stay (not shown dead), one `map_or` line each.
- CR5-4 stale docs (`commit_all_pending`, the save comment): fixed; `invalidate_for_filter` keeps its name (it marks the Log, which the doc says).
- CR5-5 the save looked up the window twice: once.
- CR5-6 `push_from` repeated `peer_pass`: calls it. CR5-7 skipped: `run`, `run_broken`, `refresh_of` in runner tests differ in server type and which half they return; one generic helper reads worse.
- CR5-8 the blank line between `PullStats` and `PushStats` is back.

Review round 6 (fix reviewer: PASS, 1 minor; `/code-review high`: 6):
- R6-1 the guard test didn't check the branch returns and sat in the Auto-Finish test: own test `setup_re_reads_only_while_no_session_runs` (timer/imp.rs), asserts the `return;` before `load_timer_settings`.
- R6-note (device): Android's `refresh_after_pull` re-reads Setup during a session as the base did; checked on the phone in the test steps.
- CR6-1 under a session `refresh_streak` skipped the Setup label row and summaries too: it re-reads them with the chip (neither touches the session's time).
- CR6-2 the deleted labels source test also pinned the Log edit dialog's close re-read, which label create now relies on: `the_edit_dialog_re_reads_the_log_when_it_closes` (log/mod.rs).
- CR6-3 Android ran the label-conflict check only on an end edge with no sync in flight, so back-to-back syncs postponed it: back to every edge, as the base (proposal updated).
- CR6-4 the page-switch handler repeated `refresh_shown`'s arms: it calls `refresh_shown(Refresh::ALL)` (no marking, so Log and Stats still re-read only when stale).
- CR6-5 LogView's "no app yet" branch is dead (`imp().refresh()` returns without an app): `is_some_and`; StatsView's stays (it builds cells with defaults).
- CR6-6 label rename and delete re-read all of Setup on the Timer page under the open chooser: `Refresh { log, stats, labels }`; the chooser's rebuilder and its close update Setup's row, as the base. The merge keeps `ALL` (rare; its `label_merged` call is gone).

Review round 7 (fix reviewer: PASS, 2 minor; `/code-review high`: 7):
- R7-1 the session branch's label row and summaries untested: the guard test asserts both calls inside it.
- R7-2 = CR7-3 a label rename or delete from the Log edit dialog's chooser re-read the shown Log twice: rename and delete only mark Log and Stats (`mark_stale`, as the base); the dialog's close re-reads the Log.
- CR7-1 a guided rename and its Undo re-read all of Setup under the open chooser: `mark_stale(LOG)`; the chooser's rebuilder updates its rows, as the base.
- CR7-2 a guided delete and its Undo marked nothing, where the base's post-write sync re-read everything: both mark the Log (cards name the file).
- CR7-4 rejected: a new day with the Log shown reloads it from the top; the base did the same (`reload_for_new_day` invalidated and refreshed the Log). Android's headers-only re-render is a separate improvement, not this change's bug.
- CR7-5 Android kept a failed pass's changes through the 5 s and 10 s retries: each attempt hands its changes to the tick at once.
- CR7-6 runner test lacked the remote-data-lost case: added (own database, so the laptop's known batches stay).
- CR7-7 the recovery toast's `mark_stale(ALL)` did nothing (Log and Stats start stale): removed, the comment says why.
- Not checked by the review (noted for the device test): bell, preset, sound and pattern edits used to get a Setup re-read from the sync after every write; each must update its own rows.

Review round 8 (fix reviewer: FAIL on one, 2 findings; `/code-review high`: 5):
- R8-1 (IMPORTANT) GTK's Bells page kept a renamed or deleted sound's or pattern's old name after a chooser pop: the rows sit on `bells_page` (#20), which a pop shows without mapping TimerView; the base's sync re-read hid it for sync users (without sync it was already stale). `bells_page.connect_shown` re-reads them; `setup_rereads_bell_rows_when_shown_again` pins that signal (it pinned the TimerView map, the wrong widget).
- R8-2 the remote-data-lost runner case didn't check the pass failed that way: asserts `RemoteDataLost` too.
- CR8-1 the Log edit Save re-read the Log, then the close handler again: Save marks Log and Stats, the close handler is the one re-read.
- CR8-2 GTK handed a pull's changes over only after all follow-up passes: each pass hands its own to the main loop (as Android, CR7-5); the conflict prompt after the drain.
- CR8-3 `pull()` added up into `refresh` across calls: `pull` resets it (sync calls pull).
- CR8-4 rejected: `Refresh` (screens) lives in `db/events.rs` and `replay_events` returns it. Moving the mapping to `sync` needs `EntityKind` public and `replay_events` to return kinds; one core mapping either way, and replay is the one place that knows the kinds.
- CR8-5 the Android tick test sliced 600 bytes and pinned whitespace: it cuts at the next tick step and checks `pulled,` inside the call.

Review round 9 (fix reviewer: PASS, 2 minor; `/code-review high`: 4):
- R9-1 TimerView's `connect_map` re-read bell rows the main page doesn't show: removed; the Bells page's `connect_shown` is the one hook.
- R9-2 the Bells test searched past the closure: it stops at the closure's end.
- CR9-1 Android Wipe Local re-read every screen but never followed the guided selection (GTK's `data_changed(ALL)` does): it hands `Refresh::ALL` to the tick, which re-reads and follows the selection once idle; its stale comment went with it.
- CR9-2 the GTK Wipe Local doc still said "invalidate" and "kick a fresh sync": rewritten.
- CR9-3 `push_from` was an alias of `peer_pass`: removed, the tests call `peer_pass`.

Review round 10 (fix reviewer: PASS, no findings; `/code-review high`: 7):
- CR10-1 a shown Log filtered on a label a pull deleted re-read twice: `refresh_shown` re-reads the filter labels first (a dead filter clears itself there), then the Log once.
- CR10-2 Android: an open preset chooser kept a preset a pull deleted or renamed (the base too): `refresh_after_pull` refills it when open, like the bell chooser.
- CR10-3 REFACTORING #359 named the deleted `refresh_main_window`: reworded to the double `refresh_streak` as it is now.
- CR10-4 rejected for now: dropping the filter-label re-read from `refresh_shown` (the popover rebuilds on show) would leave a Log filtered on a deleted label empty until the popover opens; that cleanup belongs to the REFACTORING Log-filter item.
- CR10-5 "nothing changed" was spelled three ways: `Refresh::NONE` everywhere but the `Cell` default.
- CR10-6 `{ log, stats }` by hand in four places: `Refresh::SESSIONS`, used by `Refresh::of(Session)` too.
- CR10-7 full `meditate_core::db::Refresh` paths at the GTK call sites: left to `/simplify`.

Review round 11 (on CR10-1..CR10-7): stopped by the maintainer before it reported ("stop, jetzt reicht es langsam mal mit den Runden!", 2026-11-10). CR10-1 (filter labels before the Log) and CR10-2 (Android refills an open preset chooser) are the unreviewed fixes; both go into the device test.

/simplify (four angles, 2026-11-10):
- Applied: `Refresh::SETUP` (replaces `SOUND_FILE` and the hand-written setup-only literals in `Refresh::of` and Preferences); Android `mark_pulled()` for the two slot writes (the `any()` guard went: OR-ing NONE changes nothing); `use meditate_core::db::Refresh` in the six GTK files with several uses; the recovery-toast comment cut to one line.
- Deferred to REFACTORING (maintainer: "in die passende Stelle in den nächsten beiden Schritten"): write sites marking their own scope -> #484 note (step 2); one stale model incl. Setup in `refresh_shown` -> #505 note (step 3); reclaim's extra name query -> #506 (step 3); one app lookup and `main_window()` at the last three sites -> #507 (step 2); Android bell chooser into `refresh_after_pull` -> #418 note (step 3).
- Skipped: one runner test helper for `run`/`refresh_of`/inline `run_with` (CR5-7); the two GTK `invoke`s per pass folded into one (the conflict prompt belongs after the drain, not per pass); a lock-free `AtomicU8` for `SYNC_PULLED` (CR2-10); an early stop in `replay_events` once `ALL` (negligible).
- Suites after it: core 1540 + 18, GTK 172, Android 299 green; clippy GTK+core all-targets, CI lib -D warnings, Android target: clean; check-specs: only the labels test the delta replaces at archive.

## Hand-over (step 6)
Built 2026-11-10: GTK `meditate-gtk/builddir/src/meditate` (20:08:35), APK `meditate-android/android/app/build/outputs/apk/debug/app-debug.apk` (20:09:46, debug). Suites as after /simplify above.
Lines (non-test, each .rs cut at its first `#[cfg(test)]`): whole app 61,863 -> 61,930 (+67); the 22 touched files 24,192 -> 24,259.

Test steps (both devices with sync on the same account):
1. Phone, Log: Load more twice, delete a session on page 3, wait past Undo and a sync: the Log stays on page 3. Same with Wi-Fi off (#412).
2. Laptop, Log: same as 1 (#342).
3. Save a session on the phone, sync the laptop while it shows the Timer page: the streak updates at once; open the Log: the session is there; Stats too.
4. Phone: change the daily goal on the laptop, sync the phone: Stats shows the new goal; change a bell on the laptop: Setup's bell rows follow, the Log does not jump.
5. Phone: open the preset chooser, delete a preset on the laptop, sync: it leaves the open chooser (CR10-2, unreviewed). Bell chooser the same with a custom sound.
6. Phone, Guided: during a session rename the selected guided file on the laptop and sync; after Save or Discard Setup shows the new name. Pull a changed session length during a paused session: the running screen keeps its time (R6-note).
7. Laptop, a paused session across a pull (or a new day): the paused time stays.
8. Laptop, Log filtered on a label, delete that label on the phone, sync: the filter clears and the Log shows all (CR10-1, unreviewed).
9. Laptop, Setup -> Bells -> End Bell -> Sound: rename the sound, Back: the row shows the new name; delete a custom sound in use: "Missing" (R8-1). Same for a pattern.
10. Laptop: rename a label in the Setup chooser and in the Log edit dialog's chooser; the Log chips, filter and Stats show the new name when shown.
11. Laptop: bell, preset, sound and pattern edits update their own Setup rows without a sync (they used to get a Setup re-read from it).
12. Laptop, kill the app during a session, restart: Undo on "Session recovered" removes it from Log, Stats and streak; the Timer streak is right after midnight with the app left open.

Proposed commit (after the device test and a go):
core: a sync says which screens it changed, and both apps re-read only those

Both apps re-read every screen after every sync, failed ones too, and
every Linux write starts a sync: a delete on Log page 3 jumped back to
the first rows a second later, on Android offline too. A sync pass now
records what it applied (Refresh: Log, Stats, Setup, guided list, label
lists), also when it fails afterwards; GTK marks those views stale and
re-reads the shown one (data_changed), Android re-reads them once each.
Also: the Timer streak follows a new day on Linux, the recovered-session
Undo says when it fails, the Bells page re-reads renamed sounds and
patterns, and the hand-written invalidate calls are gone.

P2 step 1 (REFACTORING #331, #378, #485, #397 refresh half); fixes
OPEN-ISSUES #98/#342, #137/#412, #346, #341/#491.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_012WGZUPYHL7TurMz6YSFVJs

