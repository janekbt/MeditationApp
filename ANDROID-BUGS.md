# Android bugs (audit 2026-10-03)

Places where the Android app behaves differently from the GTK app for the
same action. Most share the pattern of today's two fixes (96c2a8a, c826686):
Android ignores something core returns, or keeps its own copy of a core
rule. Fix each with a failing test first and put the decision in
meditate-core, with both shells calling it.

Line numbers are from beta at e851bfa. "To confirm" means the code trace is
not conclusive and a device check is needed.

## Batch A — before the fall-back night, 2026-10-25

- [x] **Session start time becomes "now" when edited around a DST change.**
  Saving the edit dialog for a session that started in the fall-back hour,
  or picking a start time in the spring-forward gap, stores the save time.
  Android builds the time with chrono `.single()` and falls back to
  `unix_now()`. Android `lib.rs:9298-9310`, GTK `log/imp.rs:1074-1080`.
  Fix: a core local-date-time → unix helper (earliest for ambiguous, shift
  gap forward); keep the original start when the pickers are untouched.
- [x] **Insight Timer import aborts on a row in the spring-forward gap**
  (to confirm). Android `lib.rs:3437-3444`, GTK `data_io.rs:161-170`.
  Fix: same core helper, used by `parse_insighttimer_csv`.
- [x] **Synced custom bells and guided files play from the other device's
  path.** Android plays `bell_sounds.file_path` / `guided_files.file_path`,
  which is the importing device's absolute path. GTK builds
  `<sounds>/<uuid>.<ext>` itself. Android `lib.rs:3365-3376` (used at 315,
  8066, 10859), guided `lib.rs:1379, 4389, 5993-6007, 1242`; GTK
  `sound.rs:180-205`. GTK has the same bug for guided session start
  (`timer/imp.rs:576, 3409`). Fix: core path resolver from local dir + uuid
  + extension; never trust a synced `file_path`.
- [x] **Bundled bell rows may carry the other platform's path after sync**
  (to confirm: `SELECT uuid,file_path FROM bell_sounds WHERE is_bundled=1`
  on both devices). Seeds emit events with each device's path and
  `recompute_bell_sound` keeps the newest (`db/bell_sounds.rs:285-300`).
- [x] **A finished session is lost if Android kills the app on the Done
  screen.** Android clears the recovery snapshot when the session ends,
  not on Save/Discard, and drops the core `Session`. Android
  `lib.rs:4447-4450, 4516-4519, 4578-4581, 5240-5242`; GTK
  `timer/imp.rs:2661-2662, 2750-2758`. Confirm: stop a session, `am kill`
  on Done.

## Batch B — sync

- [x] **Sync requests during a running sync are dropped.** Android's
  `SYNC_IN_FLIGHT` skips them; GTK uses core `SyncCoordinator` and runs one
  more pass. Android `lib.rs:147-157`, GTK `application.rs:592-647`.
- [ ] **Most edits don't trigger a sync.** Labels, presets, settings, sound
  and guided import/rename/delete, vibration patterns. GTK syncs after
  every write via `with_db_mut`. Android labels `lib.rs:3091-3175`.
  Fix: one write-then-sync path on Android.
- [ ] **No sync when returning to the running app** (to confirm via
  `sync.trigger` in the diag log). GTK `application.rs:112`.
- [x] **Views aren't refreshed after a sync pulls changes.** Log, Stats,
  presets, bell rows, guided list, widget. After "wipe local" the screens
  stay empty. Editing a stale log card may write the old copy back (to
  confirm). Android `lib.rs:4767-4778, 9829-9844`; GTK
  `application.rs:649-665`.

## Batch C — running session

- [ ] **Unplayable guided file starts a fake session** that jumps to
  Overtime and saves the full planned length on Finish. GTK shows
  "Couldn't start playback". Android `kotlin/MeditateGuided.kt:59-65`,
  `lib.rs:5154-5176`, `guided.rs:263`; GTK `timer/imp.rs:2140-2160`.
- [ ] **Only one bell plays at a time.** Each bell cuts the previous one;
  core's `FireChannel` is ignored. Android `lib.rs:316-320`,
  `MeditateAudio.kt`; GTK `timer/imp.rs:4109-4120`, `sound.rs:339-353`.
- [ ] **Guided track keeps playing into Overtime** under the end bell.
  Android `lib.rs:245-283`; GTK `timer/imp.rs:2869-2876`.
- [ ] **Save and Discard don't stop a ringing bell or vibration.** Android
  `lib.rs:5341-5431, 8809-8822`; GTK `timer/imp.rs:2461, 2596`.
- [ ] **Saved duration is computed in the shell** instead of
  `Session::stop` / `EndBoxBreath.duration_secs`. Matches today, will
  drift. Android `app.rs:358-373`, `lib.rs:4417-4425, 5213-5239`; GTK
  `timer/imp.rs:2385-2387, 3040-3042`.
- [ ] **Recovery snapshot is incomplete:** none written at start, and
  `guided_file_uuid` is always None. Android `lib.rs:2855-2870,
  2889-2911`; GTK `timer/imp.rs:2269, 2695-2699`.

## Batch D — the rest

- [ ] **End Bell row stays on and editable in Stopwatch mode.** Android
  never calls core `end_bell_row_state`. Android `main.slint:3067-3070,
  3337-3340, 3537-3540`, `lib.rs:3527`; GTK `timer/imp.rs:1999-2025`.
- [ ] **Preset with a missing sound or pattern silently does nothing**
  (also from the widget). GTK shows "Please wait until fully synced…".
  Android `lib.rs:1427-1437, 6171-6178`; GTK `timer/imp.rs:3609-3614`.
- [ ] **Failed session save shows no message.** Android `lib.rs:3195-3245`;
  GTK `timer/imp.rs:2527-2565`.
- [ ] **Turning the label on doesn't store the mode's default label**, so
  presets saved that way have none. Android `lib.rs:6795-6799`; GTK
  `timer/imp.rs:758-773`.
- [ ] **No 10 MB limit on bell import**; an oversized file is never
  uploaded by sync. Android `lib.rs:4703-4721`; GTK `sounds.rs:408-413`.
- [ ] **Import/export errors lose their details; failed export shows
  nothing.** Android `lib.rs:4928-4935, 9980-9984`; GTK
  `preferences.rs:535-541, 649-650`.
- [ ] **Overtime display shows 00:00** (or keeps counting for guided).
  GTK freezes it at the planned length, but uses the Timer's target for
  guided instead of the file length. Android `app.rs:546-553`; GTK
  `timer/imp.rs:2924-2927`. Fix: core `display_secs` in Overtime.
- [ ] **Deleted sound shows a blank name instead of "Missing".** Android
  `lib.rs:3346-3357, 3384-3391, 3697-3700`; GTK `bells.rs:381-386`.
- [ ] **Log day headers and totals are hand-formatted** ("2026-10-03",
  "125 min" vs "Today", "2h 5m"). Android `lib.rs:2656-2661, 2836-2839`;
  GTK `log/imp.rs:579-617`.
- [ ] **Manage Bells count ignores Stopwatch mode.** Android
  `lib.rs:3745-3753`; GTK `timer/imp.rs:3181, 4019-4022`.
- [ ] **Interval-bell editor copies the core defaults and limits.** Matches
  today, will drift. Android `lib.rs:8205-8219, 8378-8382`.

## Not bugs, possible features

- [ ] "Meditation complete" notification when the app is in the background
  (GTK `timer/imp.rs:2884-2897, 3059-3069`).
- [ ] Rename and delete for custom bell sounds on Android.
