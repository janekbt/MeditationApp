# Refactor and simplification audit (2026-10-06)

Planning only: nothing here has been changed yet. The audit ran on beta at a8a5de5.

**Inputs:**
- the open list ANDROID-BUGS.md (cited as B3#n);
- the two earlier, already fixed lists: the GTK-parity audit at da65931 (B1) and the fresh audit at 9ada6e4 (B2). Together that makes 101 bugs.

**Method:**
- Six read-only reviewers covered: the bug history, core, Android lib.rs, Android bridges and Kotlin, main.slint, and GTK-versus-Android duplication.
- Two further reviewers then tried to refute every candidate against the code. Some candidates were rejected, shrunk or split as a result.
- Line estimates below are the corrected ones from that check.

**Ground rules every item respects:**
- Decisions go in core.
- There is no DB schema change and no sync wire-format change, and nothing touches the Android build environment. Cargo.toml, Cargo.lock, build.rs, main.rs, gradle, the manifest and rust-build.sh all stay as they are.
- msgids are kept unless an item says otherwise.
- TDD.

---

## 1. Root-cause map (all 101 bugs)

| Cause | B1 | B2 | B3 open | Trend |
|---|---|---|---|---|
| K1 Shell re-implements or ignores a core rule | 13 | 4 | #26 | Solved by moving rules into core; keep doing that |
| K2 Logic bug inside core (hits both apps) | 3 | 2 | #2 #3 #7 #10 #18 #24 #25 #35 #36 | Now the largest open group |
| K3 Error swallowed, no path to the UI | 4 | 3 | #4 | |
| K4 One snackbar shared by six Undo slots | | 2 | #8 #17 | Structural, still open |
| K5 Write side effects (sync, view refresh) wired by hand per handler | 2 | 5 | #6 #22 #34 | Sync half solved (LocalChanges in core), refresh half not |
| K6 Session lifecycle state scattered, no single start/end owner | 4 | 1 | #33 | |
| K7 Overlays as separate booleans, kept in three hand-written lists | | 3 | #12 #16 | |
| K8 Kotlin media players without an owner (EOS/focus untagged) | | 2 | #1 #14 #15 | |
| K9 Ad hoc Kotlin→Rust drop-file protocol | | 3 | #9 #19 #20 #37 | |
| K10 JNI plumbing | | 3 | | Solved by jni_call.rs (a556ade) |
| K11 i18n (Rust formats English, concatenated sentences, content) | | 3 | #11 #27-#32 | |
| K12 Value rebuilt from a bounded widget | | 1 | #5 #13 | |
| K13 One-offs | | 4 | #21 #23 #38 #39 | |

**What the history says:**
- Rules moved into core stayed fixed: shell drift fell from 13 bugs to 4 to 1 across the three audits.
- Fixes done as point patches in the shell rotted. Examples:
  - 86711d1 was a hand refresh fan-out; B3#6 is the same class again.
  - 228c3f3 stopped the preview on delete only; B3#1 is the same class.
  - 307c9c3 added `err:` to the CSV pick only; the audio pick in the same activity is B3#19.
  - dc6bd8e paused the guided track from two call sites; B3#14 and B3#15 follow.
  - c5caed4, 0f2fa61, 33b8864 and b248cd3 each added one global flag per toast.
- Good templates to copy: 635b54f, a304463, 1368fa0, 0f13b80, a8b8cf5, cc28e0a, 3975d68 (logic moved to core) and a556ade (shared helper).

**Tests are the main hazard for any refactor.** 63 of the 157 tests in meditate-android/src/app.rs read lib.rs or main.slint as text and assert snippets, some down to indentation (app.rs:876-903, and app.rs:1357 counts exactly 3 copies of a binding). These tests guard real past bugs, and the Android UI code can't be unit-tested on the host. Policy for all items below:
- Move the decision into a host-testable unit (core or app.rs) first.
- Write behaviour tests there.
- Only then retire the source-text test it replaces.
- Never delete one without a replacement.

## 2. Proposed refactors, ranked

### R1 Core `save_ended_session` / `discard_ended_session`
- **Evidence:**
  - Android inserts (lib.rs:3530-3560) and only later clears the snapshot (lib.rs:5858).
  - GTK `on_save` (timer/imp.rs:2459-2588) persists the label (2519), then spawns the insert on the blocking pool (2537). `reset_mode` has already cleared the snapshot synchronously (2661-2662).
  - The 0 s skip and the Guided-only uuid gate exist in both apps (GTK 2467/2491, Android 3517/5841).
  - The core doc says the clear happens "inside the same transaction that records the session" (session_in_progress.rs:29-33). No API does that.
- **Change:**
  - One core call, one transaction: skip 0 s, attach the guided uuid only for Guided, apply the label pick, insert, clear the snapshot.
  - The clear must be conditional on the snapshot's start (`WHERE start_iso = ?`). GTK writes a new snapshot synchronously at Start (imp.rs:2268), so "Save, then Start at once" must not delete the new one.
  - `PersistAction` leaves the public API; its tests (labels.rs:215-240) move into the new tests.
- **What gets simpler:** about -85 lines in the apps and +60 in core, so roughly -25 net. The value is that the save steps can no longer be partial or out of order.
- **Bugs:**
  - B3#10.
  - GTK latent: a kill or failed insert after Save loses the session today, confirmed.
  - B1 classes "session lost on Done" and "failed save shows no message".
- **Touch:**
  - GTK on_save and reset_mode;
  - Android Save handler and finalize_session;
  - the GTK db wrapper (db/mod.rs:743).
- **Tests:** add tests that
  - insert and clear are atomic;
  - a failed insert keeps the snapshot;
  - 0 s inserts nothing;
  - a non-Guided save drops the uuid;
  - a stale clear doesn't touch a newer snapshot.
- **Risk:** medium, from the GTK async ordering. **Decision for Janek:** GTK keeps a whitespace-only note (imp.rs:2475) while Android trims it (lib.rs:5796). Pick one rule.
- **Effort:** M

### R2 Core `write_tx()` with IMMEDIATE transactions
- **Evidence:**
  - 38 `unchecked_transaction()` sites across 12 core db files, all DEFERRED.
  - `busy_timeout` is 8 s (db/mod.rs:226), but it doesn't help when a read transaction tries to upgrade to a write.
  - rusqlite 0.32 already has `Transaction::new_unchecked(.., Immediate)`, so no new dependency is needed.
- **What gets simpler:** about +15 lines. Not a reduction, but there is one way to open a write.
- **Bugs:** B3#2, and the whole class "edit fails at once while sync writes".
- **Touch:** core db only; both apps benefit.
- **Tests:** a two-connection test. B holds a write for 200 ms, and A's update must succeed.
- **Risk:** low. Trade-off: on GTK `with_db_mut` and on Android, writes run on the UI thread, so a write that fails at once today can block up to 8 s during a long sync replay.
- **Effort:** S
- **Status:** done, smaller than planned. rusqlite's `Connection::set_transaction_behavior(Immediate)` in `Database::init` covers all 38 sites with one line, so there is no `write_tx()`. To keep the UI-thread wait short, the app's connections wait 1 s for the lock (was 8 s for everyone), and the sync workers raise their own connection to 8 s (`SYNC_BUSY_TIMEOUT`).

### R2b All database writes off the UI thread (later, only if needed)
- **Why:** both apps write on their UI thread: 65 `with_db_mut` sites in GTK (only the finished-session save uses `with_db_blocking_mut`) and all 111 `lock_db()` sites on Android. Writes are milliseconds, so this is normally invisible. But a write that has to wait for sync's lock freezes the UI for that time. After R2 that freeze is capped at 1 s, and only during a long sync write (first full pull, wipe-and-pull recovery). In that case the save then fails with the existing error.
- **Change:** one background writer per app that owns its write connection and takes writes from a queue, with results handed back to the UI loop (GTK: `with_db_blocking_mut` everywhere; Android: a writer thread plus `slint::invoke_from_event_loop`). Each save gets an in-between state (pending, then done or failed).
- **Size:** about 175 call sites across both apps; L. A project of its own.
- **When:** only if the 1 s cap is noticeable in practice, or when a feature needs long writes. Not before.

### R3 Core builds the session: `settings_from_db(db, shape)`, a guided-only EOS, and a start refusal
- **Evidence:**
  - Android `build_session_settings` (lib.rs:513-566).
  - GTK builds the same struct three times (imp.rs:2086-2104, 2172-2193, 2201-2224) through five wrappers (3947-4006). That takes the DB lock 5-6 times per Start.
  - A past prep-gate divergence is recorded at lib.rs:529-538.
  - `Session::enter_overtime` (session/mod.rs:412) checks only the phase, not the shape.
  - GTK refuses a 0-length target (imp.rs:2053-2057, 2116-2119). Android's Start `enabled` is computed in Slint (main.slint:3020-3024) and lets a 0 s guided file through.
- **Change:** three separate commits:
  1. `settings_from_db`, with the display mode derived from the shape.
  2. `enter_overtime` (or a renamed `guided_audio_ended`) is a no-op unless the shape is Guided.
  3. `SessionShape::from_setup` returns None for a 0 target; Rust pushes `can-start` to Slint.
- **What gets simpler:** about -70 lines in GTK and -30 in Android.
- **Bugs:** B3#1 (core half), B3#20, B1 "End Bell stays on in Stopwatch", and the class "the two apps assemble cue config differently".
- **Tests:**
  - `from_db` per shape: prep only for Timer, box-breath cues only for Box Breath.
  - The overtime no-op on Timer and Box Breath shapes.
  - A 0 target is refused.
  - About 6 existing tests drive Timer through `enter_overtime` (mod.rs:1843, 2018-2062) and must switch to `tick`.
- **Risk:** low.
- **Effort:** S-M
- **Status:** 2 and 3 done. `enter_overtime` ignores non-Guided sessions. Android refuses a guided file with a 0 s length at Start with the existing "Couldn't start playback" message; GTK already refused it, and its own check stays. Step 1 (`settings_from_db`) is deferred: no current bug, and it changes how both apps start every session type, so it needs a full test round of all modes (prep, bells, interval bells, Box Breath cues, Guided) on the phone and the desktop. Do it together with other work on session start.

### R4 Android preview owner: one owner for all five preview kinds
- **Evidence:** five separate mechanisms, none of which stops the others:
  - two PreviewToggles (lib.rs:7372, 7380);
  - the `ve_preview_gen` counter (7359);
  - the `VOLUME_PREVIEW_GEN` thread-local (11563-11618);
  - the guided preview, which exists only as a Slint property.

  `close_transient_overlays` (lib.rs:1036-1073) clears the ids but stops nothing. The tick applies the guided EOS and focus files to whatever session runs (5594-5617).
- **Change:** one kind-tagged `PreviewToggle` with `stop_all()`, called from the session start edge, close-all and Back. Guided EOS is routed to the preview when a guided preview is the active kind. Core already holds the generation rule (`timer_should_revert`).
- **What gets simpler:** -60 to -80 lines, and two hand-written generation counters and one thread-local go.
- **Bugs:** B3#1, B3#16 (the audio part), B3#15 (the Rust part); B2 "preview survives delete". Class: a preview that outlives its page or leaks into a session.
- **Tests:** pure tests that a new kind supersedes the old one, that `stop_all` reports the kind, and for the EOS routing (in app.rs).
- **Risk:** S-M. Small behaviour change: one preview at a time across audio and haptics. GTK keeps a toggle per kind (gtk sounds.rs:32, vibrations.rs:179). It is harmless because the choosers are separate pages.
- **Effort:** S-M
- **Status:** done. Preview state is module-level and `stop_all_previews` stops all five kinds; the kind-tagged toggle and EOS routing were not needed for the bugs.

### R5 Android notice controller: one pending Undo instead of six slots and four toast flags
- **Evidence:**
  - Four toast statics (lib.rs:139-161) exist only because "the handler lacks the snackbar's undo slots" (comment at 137-139). The tick polls them (5171-5188).
  - There are six Undo slots (4468-4555 plus `recovery_uuid`) and 14 snackbar raise sites, each 10-20 lines.
  - The raise sites are inconsistent. 6682 keeps the preset Undo. Delete All (10720-10740) clears nothing, so a pending guided delete keeps its .ogg.
  - The Undo handler (9667-9800) tries six slots in turn.
- **Change:** `enum PendingUndo { LogDeletes, Recovery, PresetApply, PresetDelete, GuidedDelete, Override }` with `commit()` and `undo()`, owned by one snackbar owner. Raising a new notice commits the previous one. Handlers call it directly, so the statics go. It lives as a pure struct in app.rs.
- **What gets simpler:** about -200 lines; 6 slots and 4 statics removed.
- **Bugs:** B3#17 (drop it on the session start edge), B3#8 (`MainEvent::Pause` flushes it, lib.rs:11510), B3#4 (one place to show a write error); B2 "override Undo wrong mode" and "two guided deletes leak". Class: a toast that leaves another kind's Undo armed.
- **Tests:** a new notice commits the old one; Undo touches only its own kind; a session start drops a preset Undo. These replace several source-text tests.
- **Risk:** M (delete and Undo timing; keep 5 s for deletes and 8 s for recovery). Needs a device run before the commit.
- **Effort:** M
- **Status:** done. `app::Notice` holds the one pending Undo; `show_notice` commits what it replaces; the four failure flags are gone; Pause makes the Undo final.

### R6 Android dialogs: one `Modal` enum and one ordered table for Back and close-all
- **Evidence:**
  - The 21 `*-dialog-open` booleans are listed by hand three times:
    - `close_transient_overlays` (lib.rs:1036-1073), which misses bell rename, bell delete, interval-bell delete and discard;
    - the Back chain (lib.rs:11187-11395, about 200 lines);
    - the Slint z-order.
  - Dialogs never stack: Recovery → Wipe swaps one for the other (main.slint:8685-8686).
- **Change:**
  - `enum Modal` plus a single `modal` property.
  - Back and close-all become one exhaustive `match` that does each entry's teardown, using R4's `stop_all` and R5's dismiss.
  - One `changed modal => root-focus.focus()`.
  - The 13 pages do stack (labels over Edit Session, vibration editor over pattern chooser), so they stay booleans and join the ordered table only.
  - Two existing rules must be kept:
    - the guided-import dialog stays open while busy (lib.rs:1067-1072);
    - sync-raised label-conflict and recovery dialogs only open when `modal == none`.
- **What gets simpler:** about -150 Rust lines and -20 root properties. A newly added dialog can't be forgotten, because the compiler enforces the match.
- **Bugs:** B3#16, B3#12; the Back-chain gaps from B1/B2; B2 "Create new label sets the Setup label" (via the typed target in R10).
- **Tests:** rewrite `discarding_a_note_and_deleting_a_bell_ask_first` (app.rs:2033); add pure Back-order tests and a test that close-all keeps a busy import.
- **Risk:** M. Slint 1.16 supports enums in `if`, and dialogs stay where they are declared, so z-order is unchanged. Needs a device run.
- **Effort:** M. Do it after R4 and R5.
- **Status:** done. `enum Modal` and one `modal` property replace the 20 dialog booleans; Back and close-all call `dismiss_modal`. The `changed modal => focus` part became the general text-field focus fix (dd704f2). The label conflict only opens when no dialog is open; the recovery dialog opens only on a tap, so it needs no gate.

### R7 Android session lifecycle: one end path, then a session record
- **Evidence:**
  - Stop, Finish and Add (lib.rs:4851-4905, 4920-4965, 4976-5021) and the tick end (5672-5692) are four near-identical copies of the end steps:
    - take `session_start_unix`;
    - set `pending_done`;
    - stop the snapshot timer;
    - hold the snapshot;
    - set the elapsed text, clear the note, mirror the label;
    - call `on_state_changed`.
  - Only the tick copy resets `bb_target_secs` and `bb_running_active`.
  - The session's data lives outside `AppState::Finished`, which is a unit variant (app.rs:291), in `session_start_unix`, `pending_done` and the `SESSION_GUIDED_FILE` static (lib.rs:3107).
- **Change:**
  - (a) One `end_session(transition)` helper in lib.rs: about -120 lines, low risk, no test churn.
  - (b) Later, together with or after R1: `Active { session, PendingSession }` and `Finished(PendingSession)`, where `PendingSession` = start, mode, guided uuid and Box Breath target. Leaving Finished emits `StopActiveSignals` in one place (today it is pasted at 5792, 9477 and 9494). About -30 more lines, and about 54 `.toggle(` test chains need rewording.
- **Bugs:** B3#33; B1 "snapshot incomplete" and "Save/Discard don't silence the bell"; B2 "activity recreate". Class: one end path forgets a step.
- **Risk:** M. Needs a device run.
- **Effort:** (a) S, (b) M
- **Deferred (2026-10-08):** (b) fixes no open bug (every bug it lists is fixed) and rewrites about 54 test chains; do it only together with other work on the session state. Earlier status: (a) done. `end_session` holds the end steps and the running-screen resets; Stop, Finish and Add are one `end_tap` with their core transition. (b) waits for R1's follow-up, as planned.

### R8 Kotlin guided player owns its audio focus
- **Evidence (MeditateGuided.kt):**
  - The completion and error listeners (46-54) release the player but keep focus.
  - `resumeAudio` (~84) never re-requests focus.
  - `requestFocus` (~103) overwrites the old request without abandoning it.
- **Change:** `release` always abandons focus; resume requests focus and then starts. A resume that is denied focus reports through the existing focus-loss file.
- **What gets simpler:** one release path instead of four; about +5 lines. This is a bug fix more than a simplification.
- **Bugs:** B3#14, B3#15.
- **Risk:** low. No gradle change. Device check only: play a preview to its end, then a guided session, then a notification sound.
- **Effort:** S
- **Status:** done. The request is reused, so resume needs no second abandon. A notification ducks the guide to 20% instead of pausing it (the system does not auto-duck speech).

### R9 Drop-file helper and the missing failure paths
- **Evidence:**
  - The read-then-remove pattern is copied about 8 times (guided.rs:105-165, 288-405; widget.rs:123-129).
  - The picker's catch-all (MeditateFilePickerActivity.kt:119-121) writes nothing for guided or bell picks.
  - When `start_import` fails over JNI, busy stays true and the dialog spins forever (lib.rs:6179-6194).
  - Imports share fixed file names, which forces three `clear_import_*` helpers.
  - The atomic write (MeditateDropFile.kt) and most parsers (core `sound::parse_pick`, app.rs `parse_csv_pick`) already exist.
- **Change:**
  - Rust `drop_file::take(name)`.
  - The picker writes `err:` for every target.
  - `start_import` returns a bool and resets busy.
  - Import files carry the import's uuid in their name, so the three clear helpers go. The refutation pass dropped a general run token; only imports keep the uuid naming, because that is exactly B3#9.
- **What gets simpler:** about -40 lines.
- **Bugs:** B3#19, B3#9, B3#37 (adjacent); B2 "failed CSV copy silent".
- **Tests:** an `err` for each kind; a stale import uuid is ignored.
- **Risk:** S. Drop files are transient IPC, so no migration.
- **Effort:** S
- **Status:** done. `drop_file::take` serves all seven readers. A failed start keeps the import dialog open with "Import failed", like a failed import. The worker removes its own progress and cancel files, so no clear helpers remain.

### R10 Typed targets in the Android Rust code
- **Evidence:**
  - `bell_chooser_target` and `pattern_chooser_target` are u8 codes 0-6 (lib.rs:7580-7615, 26 references).
  - The label `chooser_target` uses 0/1/2 (2404, 9205, 9432).
  - The two signal-mode index mappings disagree on their fallback: lib.rs:3756-3790 falls back to Sound, app.rs:163-170 to Both.
  - Core already has `bell_volume::BellSlot` (bell_volume.rs:142).
- **Change:** `ChooserTarget::Slot(BellSlot) | IntervalEditor`, `LabelTarget { Setup, Done, Edit }`, and one signal-mode mapping. Slint is untouched.
- **What gets simpler:** about -80 lines; magic numbers go.
- **Bugs:** B2 "Create new label from the edit dialog sets the Setup label", which is this exact class.
- **Tests:** round-trip tests in app.rs.
- **Risk:** low.
- **Effort:** S
- **Status:** done. The label bug was already fixed before this. `LabelTarget` is a Slint enum (setup, done, edit); `ChooserTarget` in app.rs (StartingBell, EndBell, IntervalEditor, BoxBreathCue(phase)) uses core's `BoxBreathPhaseId` instead of `BellSlot`, which has no interval-editor case; signal mode maps through app.rs's two functions only.

### R11 Preset timing: refuse to apply during a session, then persist the timing in core
- **Evidence:**
  - Both apps write the timing under 20 hand-typed keys, none of them in `settings_keys`: GTK imp.rs:3687, 3701, 4403, 4432; Android lib.rs:2442-2509.
  - `apply` returns the timing for the app to write back, and `snapshot` needs timing that the app builds.
  - GTK clamps `breathing_session_secs` on load (imp.rs:4402); Android doesn't (lib.rs:2455-2470).
- **Change:** two commits:
  1. `apply` refuses while a `session_in_progress` row exists. That fixes B3#17 in both apps, about +20 lines.
  2. The keys move into `settings_keys`; `apply` writes the timing; `snapshot(db, mode)` reads it; reads clamp. About -60 lines. The key strings stay the same, so there is no wire change.
- **Bugs:** B3#17. GTK likely has it too: its Undo toast isn't dismissed on Start (imp.rs:3636-3667), which needs a device check. Also the clamp divergence.
- **Risk:** low. GTK's `breathing_populating` guard must still prevent echo writes.
- **Effort:** S + M
- **Status:** both steps done. Step 2 shrank: core `settings_keys` owns the six timing keys (`timer_session_secs_from_db`, `set_timer_session_secs`, `breathing_from_db` clamped, `set_breathing`) and both apps call them. `apply` writing the timing and `snapshot` reading it were left out: it would change GTK's apply flow and its echo guards for no visible gain.
- **Decision (2026-10-08):** step 1 does not refuse in core. Instead, a session start hides the preset Undo, so it can't be tapped during a session at all. Android needs one place for that on every start path (button, widget, starred preset), which R5's notice controller provides, so it is done with or right after R5. GTK can do it any time: dismiss `current_apply_toast` in its start path.

### R12 Sync runner into core; generic blob transfer in the orchestrator
- **Evidence:**
  - Android sync_runner.rs:30-195 copies GTK sync_runner.rs:28-227: the same error enum, the same `record_outcome`, and Display strings with long dashes.
  - GTK's tests cover `run_with_webdav`, not the path production runs.
  - The network-retry policy exists only on Android (lib.rs:261-300).
  - `pull/push_custom_sound_files` and `pull/push_custom_guided_files` (orchestrator.rs:682, 847 and their pushes) are near-identical.
- **Change:**
  - Core `run_attempt` takes a password closure, because the keychains are per app.
  - Add `SyncError::is_transient_network()`.
  - The error is a typed enum.
  - One blob routine over a descriptor (remote dir, extension rule, size cap). Remote paths stay byte-identical so 26.10.1 peers still work.
- **What gets simpler:** about -150 lines in the apps and +100 in core; about -90 in the orchestrator.
- **Bugs:** none open. It closes the class behind B1 "sync requests dropped", B2 "sync with no account" and "no refresh after failed push".
- **Tests:** move GTK's runner tests into core.
- **Risk:** medium, because it is sync. No wire change.
- **Effort:** M
- **Deferred (2026-10-08):** no open bug, and sync is where a mistake costs the most. Do it when sync needs work anyway.

### R13 Core housekeeping: dead code and small duplicates
**Status:** dead code removed, together with the then always-empty label filter on the streak and daily-total queries. The small duplicates below were left out on purpose; each note says where it goes instead.

- **Dead code**, used by tests only or not at all:
  - the second CSV format `import/export_sessions_csv` (sessions.rs:741-834);
  - `total_minutes_from_db` and `total_minutes_by_label_from_db`;
  - `get_streak_for_label_from_db`, `get_best_streak_for_label_from_db` and `get_daily_totals_for_label_from_db`;
  - `count_sessions_by_label_from_db` and `count_presets_from_db`;
  - `DisplayMode::is_countdown` (bells.rs:47) and `parse_iso_date` (date_math.rs:19).

  The test at local_changes.rs:149 must switch to `insert_session`. About -250 to -400 lines including tests.
- **Small duplicates:**
  - `hm_mins_key` and `hm_secs_key` fold into one; `hm_compact_key` stays. *Left out: about 12 lines, and the two behave differently at 0.*
  - One `session_payload()` helper for the three session `json!` copies (sessions.rs:527, 590, 704). *Left out: it touches what goes over the wire for about 20 lines. Do it the next time the session payload changes.*
  - `insights::input_from_db` and `goal::from_db` replace the copies in GTK stats/imp.rs:153-165, 285-302 and Android lib.rs:1827-1836, 1876-1913, about -40. *Left out: it changes both apps' Stats and needs a phone round. Do it with the Stats bug fixes (ANDROID-BUGS.md #18, #24, #35).*
  - The Log day caption becomes the sum of the cards' `log_card_minutes`, which fixes B3#35 in both apps. One rounding rule alone would not fix it: two 10m40s cards show 11 + 11 = 22, while a rounded sum says 21. *Left out: a bug fix, not a cleanup. Noted at ANDROID-BUGS.md #35.*
- **Risk:** low.
- **Effort:** S

### R14 Android mechanical cleanups (do these first; they shrink every later diff)
- **(a) `with_db` helper:**
  - Evidence: 110 copies of `DATABASE.get()/lock()` in lib.rs. GTK already has one (application.rs:475).
  - Change: map a poisoned lock to None and per-site defaults to `unwrap_or`. Add a debug-only check that panics on a nested lock. It replaces the source test app.rs:2264-2276, which guards a real deadlock.
  - About -200 lines.
- **(b) UI code into `src/ui.rs`:**
  - Use a file-level `#![cfg(target_os = "android")]`, as jni_call.rs:23 does. Not an inline module: re-indenting would break dozens of source tests.
  - Keep the host `main()` running the UI, because it is the reason the bin exists (Cargo.toml:15-17).
  - Keep the `any(android, test)` gates on widget and alarm_volume.
  - It removes most of the 674 cfg gates and the host-only `let _ =` silencers, about -750 lines of noise.
  - Cargo.toml, main.rs and build.rs stay untouched. Source tests change their path from src/lib.rs to src/ui.rs.
  - Cost: lib.rs code is then type-checked only by the Android build, which is already almost true today.
- **Status:** all done. (d) `load_setup_for_mode` serves startup, mode switch, sync pull and preset apply (a fourth copy); a pull now reloads stopwatch, keep-awake, signal mode, length and tiles. (c) shipped with R9 as `jni_call::load_class`. Earlier note: (a) and (b) done. (c) deferred: no bug behind it, and every bridge needs a phone check, so it rides along with R8/R9 (step 4), which touch the bridges and need a phone round anyway.
- **(c) `jni_call::load_class`:**
  - Evidence: `resolve_class` is copied 8 times (audio.rs:110, screen.rs:32, guided.rs:169, widget.rs:139, haptics.rs:102, insets.rs:38, keychain.rs:69, about.rs:84), plus the `(Context)V` wrappers.
  - Change: add it to the existing jni_call.rs.
  - About -150 lines. Needs a device smoke test of bell, vibration, guided, widget, About and keychain.
- **(d) One `load_setup_for_mode`:**
  - Evidence: three drifting copies (lib.rs:5939-5994, 1628-1640, 9512-9531); 1628 lacks `set_stopwatch_on`.
  - Change: `refresh_after_pull` (2913) calls it too.
  - About -40 lines.
- **Risk:** low for all four.
- **Effort:** S each.

### R15 Slint deduplication
- **(a) Reuse dialogs:**
  - Evidence: `ConfirmDialog` (main.slint:1132) is used only twice; 6 hand-built confirm dialogs and 6 text-entry dialogs repeat the same skeleton. 320 vs 340 px widths and a centred vs left body have drifted.
  - Change: build a `NameDialog`; keep every `@tr` string as it is, so no msgid churn.
  - About -300 lines. It is a visible change: pick one look.
- **(b) Shared Setup rows:**
  - Evidence: Stopwatch, Label and Keep Awake are copied 3 times (3156-3206, 3547-3578, 3725-3778); a drift has already been fixed once (01d8b61). The End Bell group is copied 3 times (3329, 3600, 3801).
  - Change: one component with a `show-duration` flag. Rewrite app.rs:1357.
  - About -60 to -100 lines.
- **(c) `SlidePage` and `PageHeader`:**
  - Evidence: 10 slide-in page skeletons and 8 back headers; the three Cancel/Title/Save headers drifted. Only Edit Session elides (5168-5176), and the interval editor (6142) and vibration editor (6642) overflow, which is B3#23.
  - Change: bind `x` inside the component on `self.width`, and keep every instance at its declaration position (z-order comments at 5724-5731).
  - About -200 lines.
- **(d) Merge the number fields:**
  - Evidence: `VerticalSpinBox` (424-508) and `StepperRow` (1238-1368) duplicate the commit/release logic.
  - Change: merge the two duplicates, keep clamping on commit, and add the missing `.commit()` to the vibration editor's Save (~6652).
  - About -40 lines.
- **Bugs:** B3#13, B3#23, and B3#39 if the header carries accessible labels.
- **Risk:** S-M. The UI can only be verified on the device.
- **Effort:** (a) S-M, (b) S, (c) M, (d) S
- **Status:** (a) done: `ConfirmDialog` is the one card (320 px, centred title and body) with `confirm-enabled` and an `@children` slot for a name field; 15 dialogs use it, about -500 lines. Recovery and the label conflict stay hand-built at 340 px for their button rows; the pickers and the guided import stay too. (b) done, without a component: every mode shows its own content first, so one Session and one Bells group sit below it, with mode-guarded rows (no Cues in Box Breath, no Duration in Guided, Starting and Interval Bells in Timer only). About -245 lines; Guided and Box Breath now get the same 20 px group gap as Timer.

### R16 i18n: no UI text formatted in Rust, no concatenated sentences
- **Evidence:**
  - `render_hm` (lib.rs:1782-1790) and `"{dur} · {n} sessions"` (1949) format English in Rust.
  - Concatenated sentences in Slint: main.slint:1712, 3294, 5202-5203, 7382-7383, 7494-7495, 7907.
  - Hand-built date and 24 h time (5224-5235).
- **Change:**
  - Add `Tr.hm`, `Tr.n-sec` and similar, called from Rust through `ui.global::<Tr>()`, the pattern 68 call sites already use.
  - Add one `@tr("… '{}' …", name)` for each split sentence.
  - Add a source lint next to `user_visible_text_has_no_long_dash` (app.rs:1730) that fails on `@tr(..) +` and `+ @tr(`.
- **Bugs:** B3#27, #28, #31, #26, and #29 in part. B3#11, #30 and #32 are translation or platform content and aren't covered.
- **Risk:** msgid churn in every .po. Do it after R15(a), so the strings churn only once.
- **Effort:** S (lint) to M (all fixes)

## 3. Suggested order of work

1. **Enablers, all mechanical, no behaviour change:** R14(a) with_db → R14(b) ui.rs → R13 dead code. (R14(c) load_class moved to step 4.)
2. **Core first, both apps gain:** R2 write_tx → R1 save_ended_session → R3 session from core. (R11 step 1 moved to step 3, after R5.)
3. **Android state owners, each needing a device run before the commit:** R4 previews → R5 notice → R11 step 1 (a session start hides the preset Undo, both apps) → R6 Modal → R7(a) end path. R10 typed targets can go anywhere in this stage.
4. **Kotlin and IPC:** R8 focus → R9 drop files, with R14(c) load_class in the same phone round.
5. **Deeper moves:** R7(b) session record (after R1) → R11 step 2 → R12 sync runner → R14(d).
6. **UI:** R15(b) → R15(a) → R15(d) → R15(c) → R16.

**ANDROID-BUGS.md items that become trivial or disappear once a step lands:**
- After step 2: #2, #10, #20, #17, and the core half of #1.
- After step 3: #1, #16, #12, #8, #4 and #33.
- After step 4: #14, #15, #19 and #9.
- #35 is not part of R13 after all; it is fixed as a bug (see its entry).
- After step 6: #13, #23, #26, #27, #28 and #31.

**Coverage:** the refactors cover 21 of the 39 open items, and #29 and #37 in part. The rest are standalone logic, content or platform bugs, which are better fixed directly:
- #3 sync compaction;
- #5 and #6 Log edit;
- #7 and #36 CSV;
- #11 and #32 translation content;
- #18, #24 and #25 date logic;
- #21 HE-AAC;
- #22 and #39 unwired properties;
- #30 platform pickers;
- #34 filter name;
- #38 codec leak.

## 4. New findings outside ANDROID-BUGS.md

- **Corrupt sync compaction manifest:**
  - Cause: orchestrator.rs:563-571 reads a manifest that fails to parse as `unwrap_or_default()`, and 581 then overwrites the remote with only the current batch.
  - Effect: a peer whose batches were swallowed earlier gets a false "remote data lost" dialog. If the user then wipes local data, that becomes data loss.
  - Fix: abort compaction on a parse error, about +3 lines plus a FakeWebDav test. Real but unlikely.
- **Deleting a custom bell leaves its audio file on disk:**
  - Both apps are affected: GTK sounds.rs:987, Android lib.rs:7737, core bell_sounds.rs:184. Guided delete does remove its file (gtk guided.rs:1125), and peer tombstones leak the same way.
  - The fix belongs where the sounds dir is known.
- **GTK copies of Android bugs:**
  - B3#4: Log edit closes the dialog even when the write failed (log/imp.rs:1168-1172). Confirmed.
  - B3#5: confirmed, and worse than on Android. The SpinRow max is 23 (log/imp.rs:815), so even a note-only edit of a session of 24 h or more saves 23 h.
  - B3#17: plausible, needs a device check.
  - The Save snapshot-loss window: confirmed, fixed by R1.
  - B3#6 in GTK: unlikely.
- **GTK i18n:** the sync Test toast shows core's English `Display` (preferences.rs:311, credentials.rs:42-53).

## 5. Rejected or shrunk

- **Typed serde structs for every sync event payload (L):** the silent defaults are deliberate compatibility tolerance, no payload bug appears in any audit, and there is wire risk. Only the `session_payload()` helper survives (in R13).
- **A change-counter-driven view refresh:** `LocalChanges` is per connection, and the sync worker has its own connection, so pulls would never bump it. Polling would also reload the Log under an open edit. Replaced by R14(d).
- **A run token on every drop file:** no bug needs it outside imports (R9).
- **Number fields that commit on every keystroke:** out-of-range intermediate values either go stale or clamp mid-typing. Replaced by R15(d).
- **Typed `BellSlot` through Slint (54 root items, L):** it is possible in Slint 1.16, but it is large with no host safety net. Optional later; the Rust half is R10, and the End Bell copies are in R15(b).
- **Moving the whole sync account Save/Test into core:** the keychains are per app. Only the narrow error enums survive, which removes the four `unreachable!` arms (GTK preferences.rs:281, 373; Android lib.rs:10340, 10972). Fold that into R12.
- **Moving the Log edit build into core:** about -40 lines, and it doesn't fix the GTK B3#4/#5 copies, which are app-side error handling and a spin limit. Low value; reconsider when fixing #5/#6.
- **Slint dead properties:** `streak-text` and `sync-indicator-tooltip` are not dead; they are B3#22 and B3#39, which need wiring up. The rest is cosmetic.
- **Splitting lib.rs per screen, or splitting sessions.rs, orchestrator.rs or session/mod.rs:** cosmetic. The big core files are mostly tests. lib.rs gets real structure from R4-R7, which give each concern private state that the `android_main` lifecycle listener can reach for pause and teardown.
- **Others:**
  - a `Divider` component;
  - a stack model for pages (pages really do stack);
  - caching JNI classes;
  - moving the CAN_DUCK policy to Rust;
  - a typed `SettingKey` enum (R11 removes most of the literals);
  - a core pending-Undo queue (GTK uses toast closures, so no logic is shared).
