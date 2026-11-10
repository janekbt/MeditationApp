# Proposal: refresh-what-changed

P2, step 1 of 3 (maintainer, 2026-11-10: "drei Schritte, fang mit Schritt 1 an").
Refactors: #485, #331, #397 (the refresh half), #378.
Bugs riding along: #98 with #342 (GTK), #137 with #412 (Android), #346, #341.
Steps 2 (#484, #316, #283, #327, #340) and 3 (#418, #468, #353, rest of #397) follow as their own changes.

## Why
Both apps re-read every screen after every sync, and treat a failed sync as "maybe changed". Every write starts a sync, so a delete on Log page 3 jumps back to the first 15 rows a second later (#342), offline too on Android (#412). The new-day refresh skips the Timer page, so the streak shows yesterday's value (#346). The Linux recovery Undo fails silently and leaves the session on screen (#341). Labels, guided files and the recovery dialog each invalidate by hand, with a wrong comment and a second sync trigger (#378).

## What changes
- Core: a sync pass records what it applied, by kind of item (sessions, labels, settings, bells, sounds, presets, phases, patterns, guided files, downloaded audio files). It reports that even when the pass fails later (a failed push after a pull). A pass that fails before applying anything reports nothing. One core mapping turns that into the screens to refresh: Log, Stats with streak, Setup, guided list, labels. Wipe Local and a new day count as "everything".
- GTK: the sync worker ORs the passes; on the main thread one `data_changed(scope)` marks the stale views, refreshes the Timer page (its streak refresh reloads Setup too, cheap) and the visible Log or Stats; a hidden Log or Stats refreshes when shown (the existing dirty flags, as `take_*_dirty()`). A new day goes through the same method, Timer page included (#346). The hand `invalidate` + refresh pairs in labels, guided, preferences, log, timer and the recovery dialog become `data_changed`; the wrong comment, the second sync trigger, the dead "no app yet" branches and the source-text tests go. The recovery Undo shows "Delete failed" on failure and refreshes on success (#341; the text exists in all nine languages).
- Android: the drain ORs the changes; the tick re-reads only the touched screens, each helper once (no double `refresh_setup_summaries`, label name, `refresh_widget`, `refresh_bell_rows`); the bell chooser is rebuilt only when open; the label-conflict check stays after every sync (it is the only way a conflict left from a dialog that was open gets asked). Wipe Local refreshes everything.

## Decisions
- Split into three steps, this one first (maintainer).
- Minor, my call: a peer's session arriving while the Log is scrolled still reloads the Log from the top (rare: needs a second device's save; keeping pages is step 3 work); Android keeps refreshing hidden screens it touches (it has no dirty flags; adding them is not needed for the bugs); the GTK guided list keeps today's behaviour after a pull (not re-read; not one of this step's bugs).

## Dropped
- Per-row refresh on Android (step 3), GTK sync only after real changes (step 2).

## Reaches
core `sync/orchestrator.rs`, `sync/runner.rs`, `db/events.rs`; GTK `application.rs`, `window/imp.rs`, `log/mod.rs`, `stats/mod.rs`, `labels.rs`, `guided.rs`, `preferences.rs`, `log/imp.rs`, `timer/imp.rs`, `recovery_dialog.rs`, `sync_runner.rs`; Android `ui.rs`, `app.rs` (a source test pins the old drain line), `sync_runner.rs`. No new text, no translations, no schema or wire change, no migration, Android build environment untouched. Docs: ARCHITECTURE.md (sync section), REFACTORING.md, OPEN-ISSUES.md.
