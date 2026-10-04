# Android bugs (fresh audit 2026-10-04)

Independent audit of the Android app at beta 344ad0e, done from the current
source only. Each item has been re-read in the code; "to confirm" means the
trace is solid but the user-visible effect needs a device check. Line numbers
are from 344ad0e. Fix each with a failing test first; decisions go in
meditate-core with both shells calling them.

## Batch A: crashes and lost data

- [x] **Diagnostics page crashes once the log passes 64 KB.** The tail cut
  slices the log at a byte offset that can land inside a multi-byte
  character (long dashes in sync errors, label names); `panic = "abort"`
  kills the app. Android `src/lib.rs:10623-10630`. GTK passes the whole log
  (`meditate-gtk/src/application.rs:345`). Fix: move the cut to a char
  boundary.
- [ ] **Activity recreate mid-session ends the session but leaves the
  service, wake lock and guided track running** (to confirm). Manifest
  `configChanges` (`AndroidManifest.xml:56`) misses
  `fontScale|locale|smallestScreenSize|keyboard|navigation|layoutDirection`,
  so a font-size or language change, split screen or a hardware keyboard
  destroys the activity; android_main re-runs, `open_database` finalizes the
  live snapshot (`lib.rs:11471`) and the app starts Idle. Nothing calls
  `service::stop` / `guided::stop` / `audio::stop` (`lib.rs:347-362` only on
  an Active→inactive edge). Check: start a guided session, change font size,
  return. Fix: add the flags; at startup stop leftover service and players
  when no session is active.
- [x] **Swiping the app away keeps the guided track playing** (to confirm).
  `kotlin/MeditateSessionService.kt:249-253` `onTaskRemoved` only stops the
  service. Fix: also `MeditateGuided.stopAudio` and `MeditateAudio.stop`.
- [x] **Losing audio focus pauses the timer but not the guided track.** A
  call or another media app freezes the timer while the voice track plays
  on, reaches its end early and forces overtime. `lib.rs:5553-5571` toggles
  and dispatches but never calls `guided::pause`. GTK
  `meditate-gtk/src/timer/imp.rs:2340-2361` pauses the player. Fix: pause the
  guided player in the focus-loss branch.
- [ ] **Back gesture and Discard on Done throw the session and note away
  without asking.** `lib.rs:11167-11178` (Back discards), `lib.rs:9326-9341`
  (Discard). GTK asks "Discard Session?" when the note is not empty
  (`timer/imp.rs:2595-2629`). Fix: confirm dialog when there is a note; Back
  should probably not discard at all.
- [x] **Editing an older session can be silently dropped after a sync.** A
  background pull resets the Log to page 1 (`lib.rs:5137-5145`, `2840`); Save
  looks the original up in `loaded_log_sessions` and skips the write when it
  is gone (`lib.rs:9850-9877`). GTK captures the Session on open
  (`log/imp.rs:799-810`). Fix: store the Session in the edit state on open.
- [ ] **Deleting the selected label while editing makes Save fail
  silently.** Label delete never clears `edit-label-id`/`done-label-id`
  (`lib.rs:9190-9200`); `update_session` then fails the foreign key, the
  error is only logged and the dialog closes (`lib.rs:9868-9874`,
  `9908-9910`; add path `9896-9901`). GTK has the same pattern
  (`log/imp.rs:1131-1138`). Fix: clear the stale id; show save errors and
  keep the dialog open.
- [x] **Saving an edit rounds the duration down to whole minutes.** The
  dialog is seeded with hours/minutes and Save always rebuilds
  `duration_secs` from them (`lib.rs:9719-9721`, `9808-9811`, `9864-9865`):
  a note-only edit of 20m34s stores 20m00s, a session under a minute stores
  0 s, and manual Add with 0h0m inserts a 0-second session. GTK shares it
  (`log/imp.rs:823-826`, `1101-1102`). Fix: keep the original seconds unless
  the duration was changed; refuse 0.
- [ ] **A start time inside today's DST gap opens the editor at 1 Jan 1970**
  (to confirm, shared with GTK). `meditate-core/src/time.rs:94-111` returns 0
  for a gap time; `lib.rs:9725-9728` and the day header (`lib.rs:2659`) use
  it, and saving a note moves the session to 1970. Fix: core shifts gap
  times forward like `local_naive_to_unix`.
- [ ] **Interval bells are deleted on one tap.** `ui/main.slint:5962` →
  `lib.rs:8801-8826`. GTK asks "Delete Bell?" (`meditate-gtk/src/bells.rs:304`).
  Fix: confirm dialog or Undo snackbar.
- [x] **Java exceptions from failed JNI calls are never cleared.** In every
  bridge the `?` after `call_static_method` returns before the
  `exception_check`/`exception_clear` block (`src/service.rs:124-151`, same
  in audio, haptics, guided, screen, widget, keychain, insets, about). The UI
  thread stays attached, so the pending exception poisons the next JNI call
  (abort in debuggable builds). Uncaught Kotlin entry points:
  `MeditateSessionService.start`/`stop`, `notifyComplete`,
  `MeditateHaptics.vibrateWaveform`. Fix: clear on the error path in a shared
  helper.

## Batch B: wrong behaviour

- [ ] **No per-bell on/off switch.** A bell disabled on GTK (via sync) shows
  as a normal row, never rings, and can't be re-enabled.
  `IntervalBellRow` (`main.slint:272-276`) has no `enabled`; `lib.rs:4082-4108`
  ignores it. GTK `bells.rs:220-252` + core `bell_row_switch_state`.
- [ ] **"Create new label" from the edit dialog sets the Setup label.**
  `lib.rs:9077-9086` only handles chooser target 1; target 2 (Edit) falls
  into the Setup branch. Compare `on_label_picked` `lib.rs:9294-9303`.
- [ ] **Label rename/delete leaves stale state.** The list is rebuilt with
  the Setup selection (`lib.rs:9153-9156`, `9195-9198`, `2319-2327`), the
  edit/Done row keeps the old name, and deleting the label the Log filter
  uses leaves "No Matching Sessions" (`lib.rs:2849-2858`, `3193-3222`; to
  confirm the filter part).
- [ ] **Undo of a preset override after a mode switch shows the wrong
  mode's presets.** `lib.rs:9589` refreshes with the captured mode; delete
  Undo uses `core_mode` (`lib.rs:9546`).
- [ ] **Import button stops working after a failed import; bell import
  failure is silent.** `guided_import_src` is taken before the import
  (`lib.rs:6085-6089`) and never restored; the bell failure branch only
  logs (`lib.rs:5401-5412`); the guided error snackbar sits under the dialog
  backdrop (snackbar `main.slint:8023` declared before the dialog `8071`).
  GTK shows a toast and closes (`meditate-gtk/src/sounds.rs:636-651`).
- [ ] **"Import File" is enabled for a guided file already in the library**,
  making a duplicate. `main.slint:3324`, `lib.rs:5958-5987`; GTK enables it
  only for a new pick (`timer/imp.rs:3464-3468`).
- [ ] **Two guided deletes within 5 s leak the first file on disk.**
  `lib.rs:6327-6334` overwrites `pending_guided_delete` without
  `discard_pending_guided_delete` (`lib.rs:1336`). GTK `guided.rs:1165-1191`.
- [ ] **Screens don't refresh when a sync pulled changes and then the push
  failed.** `lib.rs:249-283` raises `SYNC_PULLED_CHANGES` only on `Ok`, and a
  retry pulls nothing new. GTK refreshes after every drain
  (`application.rs:635-654`).
- [ ] **Sync runs with no account set up and floods the diagnostics log.**
  `lib.rs:209-217` has no `meditate_core::sync::should_attempt` check (GTK
  `application.rs:599-604`); the comment at `lib.rs:11321` is wrong.
- [ ] **Failed CSV / Insight Timer copy gives no feedback.**
  `kotlin/MeditateFilePickerActivity.kt:104-118`, `166-178` return before
  writing `csv_pick`. GTK shows "Import failed: …"
  (`preferences.rs:649-650`).
- [ ] **Delete All shows no result, even on failure.** `lib.rs:10508-10530`.
  GTK `preferences.rs:590-603`.
- [ ] **Preset save/override/rename errors are hidden.** Override shows
  "Preset overridden" even on failure (`lib.rs:6816-6846`); Create takes the
  snapshot before the insert so a retry does nothing (`lib.rs:6716-6719`);
  Rename closes silently (`lib.rs:6965-6975`). GTK `presets.rs:536-539`.
- [ ] **Wake lock is capped at 4 h; sessions can run 23 h.**
  `MeditateSessionService.kt:221`; past 4 h with screen off, bells can ring
  late. Fix: size from the target or re-acquire.
- [ ] **Untranslated visible strings.** Stats period and chart buttons and
  mini-stat captions (`main.slint:4716-4719`, `4755-4756`, `4898-4900`);
  box-breath phase labels (`lib.rs:5644-5648`); widget empty text
  (`res/layout/widget_root.xml:49`); "{} copy" (`lib.rs:8144`); diagnostics
  header and share subject (`lib.rs:10632`, `10681`); picker fallback
  "Audio file" (`MeditateFilePickerActivity.kt:122`).
- [ ] **A failed import can keep its new labels** (to confirm, core).
  `meditate-core/src/data_io.rs:337-357` creates labels before
  `bulk_insert_sessions` outside one transaction.

## Batch C: polish

- [ ] **Log groups by stored date but heads groups by current-zone date**,
  so after a time-zone change headers and card times disagree.
  `lib.rs:2620`, `2659`, `2768-2780`; GTK uses `format::date_group_key`.
- [ ] **Pattern list ticks the newly saved pattern, not the bell's.**
  `lib.rs:8427`; GTK `vibrations.rs:79-91`.
- [ ] **Deleting a preset closes Manage Presets.** `lib.rs:7052`; GTK keeps it
  open (`presets.rs:578-581`).
- [ ] **Deleting a guided file doesn't stop its preview** (to confirm).
  `lib.rs:6273-6311`; bell-sound delete does (`lib.rs:7636-7643`).
- [ ] **Cancelled export leaves `export-transient.csv` with all notes.**
  `lib.rs:168`, `MeditateFilePickerActivity.kt:99-102`.
- [x] **Service and guided JNI failures are invisible.** `src/service.rs:48-77`
  uses `eprintln!` (stderr is discarded on Android); `src/guided.rs:281-289`
  uses `let _ =`. Fix: `meditate_core::log`.
- [x] **Every bridge call leaks a few JNI local refs** on the never-detached
  UI thread (`resolve_class` copies, e.g. `guided.rs:150-170`). Fix:
  `with_local_frame`.
- [ ] **Kotlin drop files are not written atomically**, so a 200 ms tick can
  read an empty file and lose a pick or widget tap (very rare).
  `MeditateFilePickerActivity.kt`, `MeditateWidgetProvider.kt`,
  `MeditateGuided.kt`. Fix: write `.tmp` then rename, as `widget.rs` does.
- [ ] **Long dashes in 11 user-visible msgids** (`main.slint:59, 61, 62, 63,
  76, 91, 149, 7766, 7846, 8458`, plus `lib.rs:10632`) and in most .po
  translations (de has none). Tests in `app.rs:1168-1198` assert three of
  them.
- [ ] **Small i18n leftovers.** English property defaults that flash before
  Rust sets them (`main.slint:355, 1683, 1694, 1748, 2276, 2323, 2478,
  2664`); `app_name` should be `translatable="false"`; check "−" (U+2212) on
  the steppers renders on the FP5 (`main.slint:476, 1235, 1631`).
