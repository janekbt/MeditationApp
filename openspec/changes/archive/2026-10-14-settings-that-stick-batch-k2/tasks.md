# Tasks: Settings that stick (batch K2)

Base: e3978e51b561d2d965479ae74aa0354c57c5d674; suites on it: green (`cargo test --workspace`, 1735 passed)

## Loop
- [x] 1 Understand (#191 re-checked: still present, 25 sites in GTK `timer/imp.rs` plus the volume write; the phone has the same gap in its `write_*` setting helpers, volume and preset star, found in step 1, approved by Janek)
- [x] 2 Propose (approved: 2026-10-14)
- [x] 3 Build (1739 passed; clippy clean on host, CI form and Android target)
- [x] 4 Review (first review R1-R3 held and fixed, R3 by Janek; second review on the fixes: PASS, R4 fixed, R5 rejected)
- [x] 5 Specs, docs, list (check-specs green; OPEN-ISSUES: K2 ticked and removed, #193 added as K3; no other doc names the changed code)
- [x] 6 Verify and hand over (1742 passed; laptop steps 1-3 and phone step 8 passed for Janek on 2026-10-14; lock steps 4-7 skipped by Janek)
- [x] 7 Ship (delta merged into openspec/specs/timer/spec.md, archived, check-specs green)

## Review Focus
- A slider (preparation time, volume, length) failing on every step -> one message at a time: GTK `write_ok` dismisses the previous failure toast; Android replaces the snackbar text. No new scenario: existing behaviour of both helpers.
- A failure while an Undo snackbar is shown on the phone -> the Undo is committed, as every batch K failure does (`show_notice`). No change.
- The reload writing again -> it sets controls under the page's existing loading guards on GTK (`bells_loading`, `stopwatch_loading`, `labels_loading`, `*_populating`). On Android, a Slint `changed` handler that forwards a property to a write would fire on the reload too; such a write stores the saved value again (harmless) but must not loop if it fails again. Checked in build task 2.1 and in the review.
- No database at all -> counts as a failure, as in batch K.
- A setting change during a running session -> the Timer setup controls are not reachable then; nothing new.
- Old database, second device, another language -> no schema or sync change, no new text.

## 1. Laptop (GTK)
- [x] 1.1 Add `timer/imp.rs` to `no_write_result_is_dropped` in `meditate-gtk/src/db/mod.rs`, and let the scan flag a write whose error only reaches the log (`bell_volume::write` in `if let Err`). Expected: FAIL (the 25 sites plus volume), then PASS.
- [x] 1.2 `a_failed_setting_says_so_and_springs_back` in `timer/imp.rs` tests (source text; GTK can't run on the test host): every Timer page write goes through one helper that calls `crate::db::write_ok` and, on false, schedules the page reload (`refresh_streak`, `load_timer_settings`, `load_breathing_settings`, `refresh_setup_label_chooser_subtitle`, `refresh_volume_rows`) with `glib::idle_add_local_once`. Expected: FAIL, then PASS.

## 2. Phone (Android)
- [x] 2.1 `a_failed_setting_says_so_and_springs_back` in `meditate-android/src/app.rs` (source text; ui.rs only builds for Android): every `write_*` setting helper, the bell volume handler, the preset star and the accent pick go through `write_ok` and on false schedule `load_setup_for_mode` + `refresh_bell_rows` (+ Box Breath phase rows; for the accent: `prefs_accent` + `apply_accent` from the saved value) on the next turn; no `write FAILED` log-only branch and no `let _ =` on these writes is left. Expected: FAIL, then PASS.

## 2b. Label default at Save (found while building, Janek: own text)
- [x] 2b.1 `a_label_default_that_was_not_kept_says_so` in GTK `timer/imp.rs` and Android `app.rs`: the Save flow's label persist shows "Label not kept for next time" (GTK `Failed::LabelDefault`, Android `invoke_label_not_kept`). FAIL, then PASS.
- [x] 2b.2 The string in both apps, nine translations each; `cargo test -p meditate i18n::` and `msgfmt --check` green.

## 3. Done
- [x] 3.1 OPEN-ISSUES: remove #191 and the K2 section, tick K2; add #193 (K3).

## Review (fresh agent, 2026-10-14)
- R1 held: Slint runs the Switches' change handlers after the timers in the same turn (i-slint-core 1.16.1 `update_timers_and_animations`), so `RELOADING` was already false and the reload wrote again. Fixed: cleared via `invoke_from_event_loop` (cleared at once if that can't post). Test: `a_failed_setting_says_so_and_springs_back` (FAIL, then PASS).
- R2 held: GTK start bell, end bell and phase signal-mode toggles had no `bells_loading` guard, so every page refresh (and the reload) wrote the shown value again. Fixed: the guard as on the Cues toggle; the two refresh functions set the flag and put the caller's back. Test: `showing_a_saved_signal_mode_writes_nothing` (FAIL, then PASS).
- R3 held, and is wider: core `apply` itself saves value by value (no transaction), so a preset can stop halfway anywhere. Janek: no Undo; "Save failed" and the setup from before is applied again. Both apps: `apply_config` / `apply_preset_json` snapshot first and re-apply on failure. Test: `a_preset_that_fails_halfway_is_put_back` in both apps (FAIL, then PASS). The re-apply can fail under the same lock; all-or-nothing is #193 (K3).

- Second review (fresh agent, on R1-R3 and 2b): PASS, two minor.
  - R4 held: Android parsed the preset inside the apply, so a config that doesn't parse still re-applied the setup from before (pointless rewrite, maybe a second failure). Fixed: `apply_preset_json` parses first; `apply_preset_once` takes the parsed config. Test: `a_preset_that_fails_halfway_is_put_back` (FAIL, then PASS). GTK already parsed first.
  - Review of R4: PASS, one minor (R6, held): the put-back test didn't check that it runs only on failure. Assertion added in both apps' tests.
  - R5 rejected: a preset tap takes the setup snapshot twice (Undo's and the put-back's). A few settings reads per tap, behaviour correct; passing it through would widen both apps' apply signatures for no visible gain.

## Hand-over
Artifacts: laptop Flatpak installed 2026-10-14 11:19 UTC (binary contains the new text); debug APK `meditate-android/android/app/build/outputs/apk/debug/app-debug.apk` built 2026-10-14 13:21 (the .so contains the new text). Final run: 1742 passed, clippy clean on host, CI form and Android target, check-specs green.

Test steps (Janek):
Laptop, normal use (no lock):
1. Timer page: flip Starting Bell, End Bell, Preparation, Interval bells, Stopwatch, Keep screen awake; pick Sound / Vibration / Both for the start and end bell; change a volume; pick a label. Close the app, open it: everything as set.
2. Box Breath: change a phase cue's Sound / Vibration / Both and a phase length. Restart: kept.
3. Tap a starred preset: "'…' applied" with Undo, Undo works as before.
Laptop, locked database (optional; `( echo "BEGIN EXCLUSIVE;"; sleep 30; echo "COMMIT;" ) | ~/Android/Sdk/platform-tools/sqlite3 ~/.var/app/io.github.janekbt.Meditate/data/meditate/meditate.db`, then act within the 30 s):
4. Flip Starting Bell: after about 8 s "Save failed", the switch flips back, one message only.
5. Pick Vibration for the start bell: "Save failed", it goes back to the old choice, one message only.
6. Move a volume slider: "Save failed", no bell plays, slider back.
7. Tap a starred preset: "Save failed", no Undo, setup as before (or, if the lock still holds, as saved).
Phone, normal use:
8. Same as 1 to 3 on the phone, plus: star and unstar a preset in the preset chooser; pick another accent colour in Preferences. Restart: kept.

- Build: the GTK test helper `body()` cut a top-level fn at its first inner `    }`; the test slices `setting_saved` to `\n}\n` instead (test fix, not code).
- Build: Android's Switch reports Rust-made changes (comment at `on_end_bell_toggled`), so a reload would write again; the `RELOADING` flag in every `write_*` helper stops that.
- Build: Android `apply_preset_config_json` merged into `apply_preset_json` (the wrapper only added the message; failures now say so through `setting_saved`, a bad JSON still says "Save failed").
- Build: Android duration Set only puts a length in the live cell after it saved, so `reload_setup` needs no cell.
- Question pass (fresh agent, three questions). My calls:
  - Applying a preset or its Undo whose length or Box Breath pattern fails to save counts as a failed apply: one "Save failed", no "applied" message and no Undo (both apps: GTK `apply_config` -> `set_countdown_target`, Android `apply_preset_config_json` returns false then).
  - A volume that fails to save plays no preview, since the slider springs back (both apps).
  - Phone: the reload is only `load_setup_for_mode` + `refresh_bell_rows` + Box Breath phase rows, not the whole `refresh_after_pull` (it also reloads Log and Stats). Laptop: `refresh_streak` already calls `load_timer_settings` and `load_breathing_settings`.
- Phone risk: `write_ok` -> `show_notice` -> `commit_undo` can take the database lock; every helper drops its own lock before `write_ok`, or the app deadlocks.
- Phone: an 8 s busy wait on the main thread is past Android's 5 s ANR limit. Not new with K2; the manual lock test may show "app isn't responding" first.
