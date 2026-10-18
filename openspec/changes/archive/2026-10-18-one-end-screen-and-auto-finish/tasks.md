# Tasks: one end screen and Auto-Finish

Base: 6a4152270c9ff6f9c1d7f1771b84e8d82784040a; suites on it: green (`cargo test --workspace`, all passed, 2026-10-18)

## Loop
- [x] 1 Understand (GitHub #6 and Janek's comment; decisions in proposal.md)
- [x] 2 Propose (approved: 2026-10-18, subtitle "Speichert die Sitzung ohne Nachfrage, wenn die Zeit um ist")
- [x] 3 Build
- [x] 4 Review (two reviewers, fixes R1-R8; fresh look at the fixes below)
- [x] 5 Specs, docs, list (check-specs: only the two removed label tests in the living timer spec, gone at archive; README, ARCHITECTURE; #6 is not in OPEN-ISSUES)
- [x] 6 Verify and hand over (device test passed 2026-10-18, round 2 after the four notes)
- [x] 7 Ship (deltas merged, archived 2026-10-18)

## Review Focus
- Preset from 26.10.1 without the field -> "A preset from an older version".
- Second device on 26.10.1 overrides the preset -> drops to off; accepted, design.md Risks.
- Auto-Finish turned on while a session already runs -> the session uses the value read at Start (settings are copied into the session); no scenario, Setup is locked during a session.
- Guided file ends before its stored length (EOS) -> "A guided file ends by itself".
- Auto-Finish with a 0-length countdown -> can't start (Setup needs a length > 0); none.
- Paused at exactly the end -> a paused countdown never reaches its end; none.
- App killed between the end and the auto-save -> recovery snapshot still offers the session at next start, as today; none new.
- Undo after a sync already pushed it -> delete event syncs like the recovery Undo; same path, no new scenario.
- Guided play count after Undo -> same as the recovery Undo today (checked while building, noted below).
- Label deleted mid-session -> labels delta "The label is deleted while a session runs or is edited".
- Note typed, then Resume, then overtime -> note keeps its text; "How it looks" (MANUAL).
- Another language at 360 dp -> "How it looks" (MANUAL, German) plus the row wrap rule already in timer spec.

## 1. Core
- [x] 1.1 End buttons per state: a core function returns the button set (row of two, Discard below, note shown) from the session state and shape. Files: `meditate-core/src/session/mod.rs`. Proof: `the_end_buttons_follow_the_session`, Expected FAIL then PASS
- [x] 1.2 Save while paused = stop from Paused gives the elapsed time. Proof: `save_while_paused_keeps_the_time_so_far` (may pass at once: behaviour exists, then note it)
- [x] 1.3 `auto_finish` in SessionSettings; Timer countdown end -> Stopped + end bell + AutoFinish{target}. Proof: `a_countdown_with_auto_finish_ends_at_its_length`; regression `tick_running_countdown_at_target_enters_overtime` stays green
- [x] 1.4 Guided: length reached and `enter_overtime` (file end) both AutoFinish. Proof: `a_guided_file_with_auto_finish_ends_at_its_length`
- [x] 1.5 Box Breath countdown end -> AutoFinish instead of EndBoxBreath. Proof: `a_box_breath_countdown_with_auto_finish_saves_by_itself`
- [x] 1.6 Per-mode key + reader. Files: `settings_keys.rs`. Proof: `auto_finish_is_remembered_per_mode`
- [x] 1.7 Preset field, capture, apply, seeds, active-preset mask. Files: `preset_config.rs`, `seeds.rs`, `format.rs` test fixtures. Proof: `a_preset_keeps_auto_finish`, `a_preset_from_before_auto_finish_applies_it_off`, `a_hidden_auto_finish_does_not_change_the_active_preset`

## 2. GTK
- [x] 2.1 Running page draws the core button set; note TextView below; Discard (asks with a note, existing AlertDialog); Save from paused, overtime, Box Breath end; Done stack page and its label rows deleted; untranslated "Pause"/"Stop and Save Session" go. Files: `src/window/imp.rs`, `src/timer/imp.rs`, `data/ui/timer_view.blp`. Proof: `the_running_page_is_the_only_end_screen`, `space_never_ends_a_session`, `only_a_successful_save_leaves_the_running_page` (renamed from `only_a_successful_save_leaves_done`), `escape_pauses_only_where_the_pause_button_would` updated
- [x] 2.2 Remove `resolve_persist_action` at Save and `a_label_default_that_was_not_kept_says_so` with it. Proof: suite green, check-specs green
- [x] 2.3 Setup Paused layout: check reachability; delete or relabel (my call, note result)
- [x] 2.4 Auto-Finish row in session options (hidden for stopwatch), AutoFinish effect saves and shows "Session saved" toast with Undo. Proof: `an_auto_finished_session_is_saved_with_undo`

## 3. Android
- [x] 3.1 Running overlay draws the core button set; NoteField below; Done overlay and `done-label-*` removed; AppState Finished stays on the running page. Files: `ui/main.slint`, `src/ui.rs`, `src/app.rs`. Proof: `the_running_page_is_the_only_end_screen`, `discard_returns_to_idle_without_saving`, `a_failed_save_keeps_the_session_and_says_so` and `discarding_a_note_and_deleting_a_bell_ask_first` updated
- [x] 3.2 Remove the label persist at Save and `a_label_default_that_was_not_kept_says_so`. Proof: suite green, check-specs green
- [x] 3.3 Auto-Finish row (hidden for stopwatch); AutoFinish saves, notice "Session saved" with Undo (recovery Undo's delete); a failed auto-save stays with Save/Discard. Proof: `an_auto_finished_session_is_saved_with_undo`

## 4. Translations
- [x] 4.1 "Auto-Finish", "Saves the session without asking when the time is up", "Session saved" in nine languages, both apps; GTK via `build-aux/update-translations.sh`; `msgfmt --check` each; `cargo test -p meditate i18n::`

## 5. Docs
- [x] 5.1 README features (Auto-Finish, one end screen), ARCHITECTURE session engine (AutoFinish effect, no Done page)

## Notes
Fresh read before showing (2026-10-18), 9 questions, all my call (listed in proposal.md Decisions): note kept on auto-save; prep-pause has no Save; failed save shows [Save][Discard]; Space never saves; Guided shows the switch in every display; early file end saves stored length; notification kept, Undo only in view; play count stays on Undo, failed Undo says "Delete failed"; hidden Auto-Finish ignored for the active preset.
- The core button function takes an "ended, unsaved" input too, since Android's AppState::Finished drops the Session; the shell passes it.
- Build 1.2: `save_while_paused_keeps_the_time_so_far` passed at once (behaviour existed, as the plan expected). Mutation: pause() keeping the running clock fails it; restored, passes.
- GTK build (2026-10-18): the four new GTK tests each failed first; none passed at once. `escape_pauses_only_where_the_pause_button_would` unchanged and still holds (Esc goes through on_pause's guard). `a_failed_setting_says_so_and_springs_back` write_ok count stays 2 (Undo delete replaces the label default write).
- 2.3 result: the Setup Paused layout could not be reached (running page can't be popped); `resume_btn` / `stop_from_pause_btn` deleted.
- Core: `ToggleAction::FinishOvertime` removed; Overtime maps to NoOp (Space never saves).
- Track-end Auto-Finish on GTK is deferred to an idle callback so the playbin isn't dropped inside its own bus callback.
- Left as before (scope): untranslated "Meditating" page title and "Meditation Complete" notification.
- Android build (2026-10-18): the three new tests each failed against a stub first; none passed at once. 10 tests updated, `label_changes_reach_the_done_and_edit_rows` renamed `label_changes_reach_the_edit_rows`; `PrimaryAction`, `primary_label`, `can_stop` and their 8 tests removed as dead.
- Found while building: a guided session auto-finished by the file's own end did not release the foreground service and the track; that path now releases them (tested in `an_auto_finished_session_is_saved_with_undo`).
- Beyond the proposal (to show Janek): the recovery Undo's failed delete now says "Delete failed" instead of "Save failed" (shared path).
- Slint callback `finish-tap` renamed `save-end-tap`.
- check-specs stays red until archive: the living timer spec names the two removed `a_label_default_that_was_not_kept_says_so` tests; the delta drops those scenarios.

## Review (2026-10-18, two reviewers: data and sync; completeness and rules)
Fixed, each test first (failed, then passed):
- R1 CRITICAL, both reviewers: the save path silenced every signal, so the Auto-Finish end bell was cut at once in both apps. Now only a tapped Save silences. Tests `an_auto_save_lets_the_end_bell_ring` (GTK, Android); scenarios added.
- R2: Auto-Finish while "Discard Session?" is open saved behind the dialog and the later Discard did nothing. The question now closes when the session saves itself; "Session saved" with Undo shows. Tests `a_discard_question_closes_when_the_session_saves_itself` (GTK, Android); scenarios added.
- R3: asking "Discard Session?" silenced a ringing bell before Cancel. Signals now stop only on confirm or a direct Discard (same tests as R2; "Discard with a note asks first" THEN extended).
- R4: "Session saved" showed while the window was unfocused / the app in the background, against the proposal. Now only when in view (GTK `win.is_active()`, Android `APP_IN_FOREGROUND`). Asserted in `an_auto_finished_session_is_saved_with_undo` (both); scenarios split per device.
- R5: GTK Undo of an auto-save did not refresh the streak chip. Now calls `refresh_streak()`; asserted in the GTK auto-finish test.
- R6: "Pause" was translated as "Paused" in es, fr, it, nl, pl, pt_BR, zh_CN (both apps; now visible on the GTK Timer page). Fixed to the verb. No test (translation fix).
- R7: dead `resolve_persist_action` / `PersistAction` and their three tests deleted; stale Android comments removed; orphaned Android msgid "Label not kept for next time" removed from the nine .po files.
- R8: living guided spec said "on Done"; guided delta added (MODIFIED "The selected guided file follows renames and deletes").
Rejected:
- Visibility rule of the switch in core (`setup_visibility`): the Slint binding has to stay a reactive expression and the preset mask only sees modes with presets (Guided has none), so a core helper would add plumbing for a one-liner that agrees in all three places today.
- Behavioural Android Undo test: Undo reuses the recovery delete, whose event and row removal are covered by core `delete_session_appends_a_session_delete_event`; the new part is the wiring, which the source test pins.
- `save_while_paused_keeps_the_time_so_far` checks only the duration: the note and the label at Save are the shared save path, pinned by the app tests; the scenario's THEN is about the length kept.
- Timer spec Purpose ("then overtime") is not a requirement; updated by hand at archive.
- Fresh look at the fixes: PASS, one MINOR fixed: on Android the auto-finish flag could stay set when Discard ran before the deferred save, making the next tapped Save skip the silence and show "Session saved". save_tap now reads it once with `take()`; `an_auto_save_lets_the_end_bell_ring` and `an_auto_finished_session_is_saved_with_undo` updated first (failed, then passed).

## Hand-over (2026-10-18)
Suites, each run alone: core 1377 + 4 + 11 + 3 doc, Android 220, GTK 149, both clippy runs clean.
Artifacts: `meditate-android/android/app/build/outputs/apk/debug/app-debug.apk` (07:41), `meditate-gtk/builddir/src/meditate` (07:41).

Test steps (phone and laptop, German):
1. Timer 1 min, Auto-Finish off. Start: [Pause] [Verwerfen], no note. Pause: [Fortsetzen] [Speichern], [Verwerfen] below, note below. Type a note, Fortsetzen: note hidden. Pause again: note text still there.
2. Let it run out: [Speichern] [+X hinzufügen?], [Verwerfen], note. Speichern: Log shows 1 min with the note and the Setup label; Setup shows again.
3. Again, run out, wait 30 s, tap hinzufügen: Log shows about 1:30.
4. Start, tap Verwerfen without a note: back to Setup, nothing in the Log. With a note (pause, type, resume): Verwerfen asks; Abbrechen keeps it running, an interval bell that rings keeps ringing.
5. Pause during the preparation time: only [Fortsetzen] [Verwerfen].
6. Session options: "Automatisch beenden" with its subtitle under the label row; hidden with the stopwatch on; shown for Geführt. Turn it on for Timer, 1 min: the end bell rings to its end, the app returns to Setup with "Sitzung gespeichert" and Rückgängig; Rückgängig removes it from the Log and the streak.
7. Same with the screen off / window unfocused: the bell rings, the session is in the Log, no notice waits.
8. Box Breath countdown, Auto-Finish off: at its end [Speichern] [Verwerfen] and the note. On: it saves itself.
9. Geführt with Auto-Finish: at the file's end it saves the file's length.
10. A preset saved with Auto-Finish on brings it back when applied.
11. Laptop: Space in overtime does nothing; Esc pauses only while running.
12. Nothing cut off at phone width; the note with the keyboard up is reachable.

Proposed commit message:
    core, gtk, android: One end screen, and Auto-Finish

    Ending a session took two screens: Finish on the running page,
    then Save on the Done page. The running page now saves, adds or
    discards itself, with the note; the label comes from Setup. Stop
    becomes Discard, which asks only with a note.

    Auto-Finish (per mode, in presets too) saves a countdown at its
    planned length when the time is up, lets the end bell ring, and
    offers Undo. Add now reads "Add X and Save"; the phone's Guided
    buttons sit side by side again.

    Fixes #6

## Device test round 1 (Janek, 2026-10-18): all steps passed, four notes
Each test first (failed, then passed):
- GTK: Undo on "Session saved" did not update an open Log; the Undo now calls `log_view.refresh()` (asserted in `an_auto_finished_session_is_saved_with_undo`).
- Both: "Add X ?" reads "Add X and Save" (German "{time} hinzufügen und speichern", my wording), two lines where needed. GTK wraps the button's label; Android gets a house `WrapButton` in main.slint (Material buttons clip and are build environment). Tests `the_add_button_says_that_it_saves` (GTK, Android).
- Android: Manage Imports and Local Folder side by side again in WrapButtons. Test `the_guided_buttons_sit_side_by_side`. (Janek links this to "buttons not tall enough" in an open issue; #159 lists text-fit cases, none names these buttons, so OPEN-ISSUES is left as is.)
- Both: the Auto-Finish row is greyed out for a stopwatch instead of hidden (GTK `set_sensitive`, Android `SwitchRow.enabled` added, like DurationRow). Asserted in both `an_auto_finished_session_is_saved_with_undo`.
- Android tidy: NoteField's doc comment, displaced above EndButtonView by the build, is back on NoteField.
