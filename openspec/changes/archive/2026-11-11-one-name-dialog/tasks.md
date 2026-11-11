# Tasks: one-name-dialog

Base: fb37085 (docs only since aa5b419); suites on aa5b419: core 1542 + 18, GTK 171, Android 300, all green. Lines at base: 60,392.

## Loop
- [x] 0 Skills loaded, again after every /compact: `/ponytail ultra`; `slint:slint` (Android UI touched)
- [x] 1 Understand (say-back and split approved 2026-11-11: "go"; three decisions answered)
- [x] 2 Propose (approved: 2026-11-11, "go")
- [x] 3 Build (builder agent; my own check: core 1549+4+11+3, GTK 175, Android 303 green)
- [x] 4 Review: three rounds (reviewer + `/code-review high` each; round 3 minor only), then `/simplify` (see Notes)
- [x] 5 Specs, docs, list (check-specs green; REFACTORING: #358 #374 #415 #486 out, P4 row, #288 and #301 bullets fixed by this change gone, #401 updated, #512-#515 deferred; OPEN-ISSUES: #423 #410 #428 out, FF note on silent import returns)
- [x] 6 Verify and hand over (laptop 2026-11-11: all passed, the chmod step skipped; phone: all passed)
- [x] 7 Ship (deltas merged; new area `names`)

## Review Focus
- An old or synced database with two bell sounds of one name:
  - Opening and seeding still work.
  - Renaming either one to its own name is not a clash.
  - The seed never trips the new check.
  -> scenario "Bell sounds that already share a name".
- Bundled bell seeds against a user sound with a bundled name: the seed inserts by uuid. It must not run through the new name check, or an upgrade fails at startup, the class of OPEN-ISSUES #247. -> same test, with a user sound named "Kansho" before the seed.
- Sync replay (`recompute_*`) writing a peer's name:
  - Replay is not a user create or rename and keeps today's conflict suffixing.
  - The new checks live only in the user-facing create and rename functions.
  -> the existing sync tests stay green; no new scenario.
- Rename to the same name in other case ("walk" → "Walk"): the item itself is not a clash (existing rule "Renaming to the same name changes nothing" for labels). -> covered by `every_kind_stores_a_name_without_outer_spaces` (it renames an item to its own name in other case).
- The taken check while typing fails to read: the dialog leaves Save on, and the write decides. -> `a_failed_save_keeps_the_name_dialog_open` (taken meanwhile).
- Another language: two Android msgids in nine `.po` files; GTK msgid dropped through `build-aux/update-translations.sh`; `cargo test -p meditate i18n::`.
- A second device: names sync as before; no wire change.
- A dialog that closes for another reason during a pending save (Back, Escape): Android writes synchronously on the UI thread, so nothing is pending; GTK the same.

## 1. Core: names inside the write (#486, #410)
- [x] 1.1 Trim plus empty refusal in one shared core check, called from every user-facing write: `insert_label`, `update_label`, `insert_preset`, `update_preset_name`, `rename_guided_file`, `insert_vibration_pattern`, `update_vibration_pattern`, `rename_vibration_pattern`, `rename_bell_sound`, and new import entries for bell sounds and guided files (both apps switch to them). The `_with_uuid` inserts stay raw (seeds, CSV restore, tests). A new `DbError` variant for the empty name, with Display text. Files: `meditate-core/src/db/*.rs`, `db/error.rs`. Proof: `naming.rs::every_kind_stores_a_name_without_outer_spaces`, `naming.rs::every_kind_refuses_an_empty_name`. Expected: FAIL then PASS.
  Result: Done: `naming::clean` (trim, `DbError::EmptyName`) in every listed write; new `import_guided_file` / `import_bell_sound` (both apps use them); `_with_uuid` stay raw. Both tests FAIL then PASS.
- [x] 1.2 Bell sounds: a core `is_bell_sound_name_taken(name, except_uuid)` (one query, both categories, NOCASE) used by the import entry and `rename_bell_sound` (after its "unchanged" early return), with a new `DuplicateBellSound`; both apps' typing checks call it instead of reading the whole library. One `DbError::is_name_taken()` for the five duplicate variants. Files: `db/bell_sounds.rs`, `db/error.rs`, `db/seeds.rs` if the seed shares the insert. Proof: `bell_sounds.rs::a_bell_sound_cannot_take_a_used_name`, `bell_sounds.rs::bell_sounds_that_already_share_a_name_keep_working`.
  Result: Done: `is_bell_sound_name_taken_from_db` (named like the other four checks), `DuplicateBellSound`, `DbError::is_name_taken()`; seed with a user "Kansho" passes. Both tests FAIL then PASS; extra `error.rs::only_the_duplicate_errors_say_the_name_is_taken`.
- [x] 1.3 `insert_label` returns the created `Label`; callers in both apps drop their read-back. Files: `db/labels.rs`, gtk `labels.rs`, android `ui.rs` (`create_label_in_db`). Proof: `labels.rs::creating_a_label_returns_it`.
  Result: Done: `insert_label` returns `Label`; both apps drop the read-back; core test callers take `.id`. FAIL then PASS.
- [x] 1.4 Core's user-facing preset create stars by `default_starred_on_save()` (a constant `true`, no setting), and both apps stop fetching it. Proof: `presets.rs::a_new_preset_is_starred`.
  Result: Done: `insert_preset` lost its `is_starred` parameter, stars by `default_starred_on_save()`; unstarred test rows use `insert_preset_with_uuid`. FAIL then PASS.

## 2. Android: one name dialog (#415, #428, #423)
- [x] 2.1 `app.rs`: the dialog's testable state. It covers target, text, validity, hint and error. Rules:
  - `changed` sets validity.
  - `confirm` takes the write's result. Ok closes. A duplicate error keeps it open with the hint. Any other error keeps it open with "Save failed".

  Proof: `app.rs::a_taken_name_turns_save_off_and_says_why`, `app.rs::a_failed_save_keeps_the_name_dialog_open`. Expected: FAIL then PASS.
  Result: Done: `app::NameField` (text, validity, `NameNote` None/Taken/SaveFailed; `changed`, `can_save`, `confirm`). The target lives in ui.rs (`NAME_DIALOG` holds both). Both tests FAIL then PASS.
- [x] 2.2 `main.slint`: one name-dialog Modal (title, field, hint line, Cancel and confirm label) replaces create/rename preset, create/rename label, guided rename, bell rename, and the name part of guided-import. Declared after what it covers. Proof: build plus the MANUAL walk; `dialogs_center_above_the_keyboard` stays green.
  Result: Done: one `Modal.name` card with `NameKind` (title, field label, confirm text from it), subtitle and progress for imports, hint line; declared at the old guided-rename place, after everything it covers. Source tests updated to it; android-target clippy compiles. MANUAL walk open.
- [x] 2.3 `ui.rs`: one `NameTarget` enum (PresetCreate, PresetRename(uuid), LabelCreate, LabelRename(id), GuidedRename(uuid), BellRename(uuid), PatternRename(uuid), Import(kind)). The rules:
  - one `taken(target, name)`;
  - one changed handler and one confirm handler, through 2.1;
  - `naming::validate` for every target;
  - a failed read counts as "free" (the write decides).

  Proof: 2.1 tests and the MANUAL walk.
  Result: Done: `NameTarget` without PatternRename (2.5 changed: no pattern rename on Android); one `taken`, one changed and one confirm handler; a failed read answers free. An import whose store fails keeps the converted file for the next Save; Cancel removes it.
- [x] 2.4 The pattern editor's Save gate goes through `validate` with the hint. Proof: `app.rs::the_pattern_editor_says_a_taken_name`.
  Result: Done: editor gate through `NameField`, `ve-name-taken` hint under the Name row; a taken-meanwhile save shows it, other failures still snackbar. FAIL then PASS.
- [x] 2.5 (changed by the maintainer, 2026-11-11) No Rename on bundled pattern rows; Android keeps its one pencil, "edit a copy", unchanged. No "Rename Pattern" msgid on Android. Proof: MANUAL "Editing 'Pulse' on the phone".
  Result: Done: nothing to build on Android (Edit-a-copy unchanged), no "Rename Pattern" msgid.
- [x] 2.6 Translations: "This name is already in use" in all nine Android `.po` files, copied from the GTK translations; `msgfmt --check` each.
  Result: Done: "This name is already in use" in nine Android .po files from GTK; `msgfmt --check` clean.

## 3. Linux: one name dialog (#358, #374)
- [x] 3.1 `name_dialog.rs`: `name_dialog(anchor, heading, initial, confirm_label, is_taken, save) where save: Fn(&str) -> Result<(), DbError>`.
  - It is an `adw::AlertDialog` with `can-close` false; it closes itself with `force_close` on Ok. The implementer checks that libadwaita 1.7 honours this on a response.
  - A hint label under the entry.
  - On a duplicate error it shows the hint. On another error it shows "Save failed" in the dialog through `show_in_view`.

  Proof: `name_dialog.rs::a_taken_name_turns_save_off_and_says_why`, `name_dialog.rs::a_failed_save_keeps_the_name_dialog_open` (with `adw::init`, as `log/mod.rs` does).
  Result: Done: libadwaita 1.7.6 keeps an `AlertDialog` open on a response with `can-close` false (verified: the test fails with `can-close` true). Escape needed handling: a close attempt with no response closes it (idle, since force_close inside close-attempt aborts). "Save failed" shows in the hint line, not `show_in_view` (dimmed under the dialog; matches Android). `save` returns `Option<Result>` (None = no database) and a `placeholder` parameter keeps the create dialogs' placeholders. GTK tests run on one GTK thread (`gtk_test::run`). Both tests FAIL then PASS.
- [x] 3.2 Callers use it: preset create and rename, label create and rename, guided import and rename, sound import and rename. Bundled pattern rows: the Rename button and its dialog go; the row gets the Edit pencil, which opens the editor as a new pattern copied from it (curve, length, kind, name "{name} copy" through a new msgid with nine translations copied from Android's "{} copy"), as Android does. Drop the msgid "Rename Pattern". The guided and sound import's extra rows stay theirs: the import dialogs keep their own frame and use the shared name row, hint and save rule. Drop the msgid "A guided file with this name already exists." and run `build-aux/update-translations.sh`. Proof: 3.1 tests, `cargo test -p meditate i18n::`, MANUAL walk.
  Result: Done: all callers through `name_dialog` / `NameRow`; bundled pattern rows: Rename and its dialog gone, Edit opens a copy (editor decides from `is_bundled`, "{name} copy", no Undo for a copy; extra test `vibration_editor.rs::a_bundled_pattern_opens_as_a_copy`, FAIL then PASS). Imports keep their frame, store failures keep dialog, name and converted file. update-translations.sh run; "{name} copy" from Android's "{} copy"; i18n tests green.
- [x] 3.3 The pattern editor's Save gate through `validate` with the hint. Proof: `vibration_editor.rs::the_pattern_editor_says_a_taken_name`.
  Result: Done: `gate_name` through core's `NameField::new` and `name_dialog::note_text` (after round 1), hint under the Name row, taken-meanwhile save shows it. FAIL then PASS.
- [x] 3.4 `clash_text` and the "«{name}» is already taken" toast: delete them if no caller is left, or keep them for the callers that remain. Proof: clippy warning-free.
  Result: Kept: `clash_text` stays for the remaining `write_ok` callers (Undo renames can still clash). Clippy clean.

## Notes
- Builder questions (fresh agent, 2026-11-11), answered as my calls:
  - Where the checks go: see 1.1.
  - There is no "star new presets" setting; the default is a constant, and scenario 1.4 is changed to match.
  - Bell names form one set over both categories.
  - A case-only rename onto a twin's name is refused.
  - The editor's create and edit are checked too.
  - `is_bell_sound_name_taken` is new in 1.2.
  - `DbError::is_name_taken()` tells a taken refusal from other errors; an Empty error shows "Save failed", since Save is off then anyway.
  - Undo toasts after a rename stay in the callers' save callbacks, and close on Ok only.
- Agent check of the base (2026-11-11):
  - #423: core already refuses a duplicate pattern (NOCASE UNIQUE plus a check), so no duplicate is stored. What is left: the shell counts a failed check as "free", and Save doesn't re-check.
  - #428's premise was wrong: on Android, guided rename, preset rename and bell rename also close on failure, not only labels.
  - GTK sound import (`sounds.rs:533-544`) also checks by hand.
- Build report (step 3, 2026-11-11):
  - Final runs: core 1549 + 4 + 11 + 3 doc; GTK 175; Android 303; all green. `clippy -p meditate -p meditate-core --all-targets`: no warnings. `clippy -p meditate-core -p meditate-android --lib -D warnings`: clean. Android-target clippy (`--target aarch64-linux-android`, no APK): clean. `check-specs.sh`: every named test exists.
  - Tests that passed at once: none of the named ones. `a_bundled_pattern_opens_as_a_copy` was written after its code: broken (`&& false`), seen failing, restored.
  - Deviations: `is_bell_sound_name_taken_from_db` (named like the other checks); NameField holds no target (ui.rs keeps `(NameTarget, NameField)`); no `PatternRename` target (2.5 changed); GTK `name_dialog` takes a `placeholder` and its `save` returns `Option<Result>`; GTK "Save failed" in the dialog's hint line, not `show_in_view`; Escape on a `can-close` false dialog needed a close-attempt handler; an import whose store fails keeps the converted file for the next Save (both apps), a failed conversion still closes; GTK tests needing a display share one GTK thread (`gtk_test::run`, the log stylesheet test moved onto it); Android's source-text tests rewritten to the one dialog.
  - Open: Android pattern editor: other save failures still go to the snackbar, not the hint; `meditate_core::sound::name_collides*` have no app caller left; the regenerated GTK .pot also dropped a stale "import worker died" entry.

## Review round 1 (reviewer agent + /code-review high, 2026-11-11)
Fixed (by me, in the main session):
- R1/CR (IMPORTANT): the name-field rule was in each app. It moved to core as `naming::NameField` and `NameNote`; GTK `NameRow` and the pattern editor now run on it, and so does Android. `confirm` takes `Option<Result>` (no database = failure) in both apps. The tests moved to `naming.rs`.
- R4 (MINOR): renaming one of two same-named bell sounds no longer calls its own name taken. (as of round 2: `NameField::renaming` exempts the item's own name; `NameField::new`, used by creates, imports and the editor, checks every name); a test was added and seen failing without the rule.
- CR (CRITICAL): GTK guided and sound import retry double-borrowed the `converted` RefCell (a panic). Fixed by taking the value before the call. No host test: building the import dialog needs a pick and a failing database; it goes to the laptop test.
- CR (IMPORTANT): GTK guided import counted a committed insert whose read-back failed as a failed save. The insert's result now decides, and the read only selects the row.
- R2/CR (IMPORTANT): the dead `NameKind.pattern-rename` and its untranslated "Rename Pattern" title are deleted from Slint.
- R3 (IMPORTANT): MODIFIED deltas for bells, guided and presets drop "rename" from the phone's "dialog closes" scenarios; the names spec covers renames.
- R5 (MINOR): the bell name check now runs inside the write transaction (import and rename); a `new_sound` helper avoids a seventh argument.
- R6/CR (MINOR): `sound::name_collides(_excluding)` and its three tests are deleted; `rename_vibration_pattern` is `#[cfg(test)]` (only tests use it).
- CR (MINOR): `find_or_create_label` trims before it looks up, as `insert_label` does.
- R8 (MINOR): the GTK editor test is named in its own scenario, and the empty-name test checks that no pattern row was added.

Rejected:
- R7 `let _ = weak.clone();`: about 50 such lines across `ui.rs` predate the change; not this change's scope.
- CR: `COLLATE NOCASE` folds ASCII only, while the old GTK and Android bell check folded Unicode. All five kinds now share one rule, NOCASE, as labels, presets, guided files and patterns always had. Two bell names that differ only in a non-ASCII case ("Ōm"/"ōm") are allowed again.
- CR: a converted file is left behind when the window closes during an open import dialog after a failed store. `remove_orphan_files` deletes it at the next start (`audio_files.rs:74`, run at startup in both apps).

## Review round 2 (fix review: reviewer agent + /code-review high, 2026-11-11)
Fixed:
- R/CR (CRITICAL): round 1's exemption of the opening name also let a picked file's name skip the taken check in all four import dialogs (both apps). Now `NameField::new` (create, import, editor) checks every name, and only `NameField::renaming` exempts the item's own name. GTK has `NameRow::new` for imports and `NameRow::renaming` in `name_dialog`; Android's `open_name_dialog` picks by target. The core test (import opening with a taken name) was seen failing first; an Android source assert pins the per-target choice.
- CR (MINOR): GTK `name_dialog`: a response after the dialog began closing could call `force_close` a second time. One `closing` flag now makes every later response a no-op (it replaces `escaped`). No new test: the double response can't be provoked deliberately on the host; the existing Escape and failed-save tests stay green.
- CR (MINOR): Android `name_saved` with no dialog open: a failed write now shows "Save failed" instead of passing silently (it is unreachable while an import is busy, since `dismiss_modal` keeps a busy dialog).
- CR (MINOR): `find_or_create_label` (no app caller) and `rename_vibration_pattern` (test-only) are deleted; their tests use `insert_label` and `update_vibration_pattern`.
- R (MINOR): the Slint `NameKind` enum no longer sits between `Modal` and its doc comment.
- R (MINOR): an Android source assert pins the pattern editor's gate and its taken-meanwhile path to core's `NameField`.

Rejected or deferred:
- CR: GTK's Escape and close handling depends on libadwaita's order of close-attempt and response. The order is pinned by `a_failed_save_keeps_the_name_dialog_open` (a failed save must stay open) and the Escape assertion in `a_taken_name_turns_save_off_and_says_why`, against libadwaita 1.7.6. A libadwaita that changes it fails those tests. A custom Save button in `extra_child` would drop the dialog's own default-response and Enter handling.
- CR: duplicated retry logic in GTK guided and sound import goes to /simplify. The "divergence" is on purpose: guided keeps Cancel live during the transcode (its cancel flag), while sounds lock it.
- CR: `gate_name` repeats `NameRow`'s hint display; this goes to /simplify.

Suites after round 2: core 1545+4+11+3, GTK 175, Android 300; both clippy runs clean; check-specs green.

## Review round 3 (fix review: reviewer agent + /code-review high, 2026-11-11): minor findings only, so no further round
Fixed (unreviewed; the laptop and phone tests cover them):
- Android: a Preset Create with no setup snapshot returned silently; it now fails through `name_saved` ("Save failed"). Pinned by a source assert.
- GTK: a response with no close attempt could leave `answered` set and swallow the next Escape; the response now clears it on idle. Test: Escape after a failed save closes (in `a_failed_save_keeps_the_name_dialog_open`).
- GTK: the name dialog logs `name.save: <heading>` instead of one tag for all.
- GTK: a new test `an_import_checks_the_picked_files_name` pins imports to `NameRow::new`; Android's source assert also pins the renaming arm.
- The stale `find_or_create_label` doc on `list_label_conflicts` is gone, and REFACTORING #232's entry for it is struck.
- These notes now describe the code as it is.

Rejected or deferred:
- NOCASE folding ASCII only for bell names (raised again): kept, for the reason above. Shown to the maintainer.
- A converted file left behind on quit after a failed store: the startup sweep removes it (raised again; the claim "nothing will ever delete it" is wrong, see `audio_files::remove_orphan_files`).
- Android's import kind kept in two places (`guided_import_kind` Cell and `NameTarget::Import`): goes to /simplify.
- `gate_name` repeating `NameRow`'s display: goes to /simplify.
- Seen, not this change: `name_dialog_confirm`'s import arm returns silently when the picked source, the Android app handle or its data path is missing. Those three returns are copied from the old guided-import confirm at base; they go into OPEN-ISSUES FF.

## /simplify (after round 3, 2026-11-11)
Applied:
- Core `NameField` derives Save from the text and the note; its `validity` field, `naming::validate`, `NameValidity` and `is_savable` are gone (no other user), as are their five tests and the `pub use naming::validate` re-export. The module doc is rewritten.
- `NameField::name()` is the trimmed name (Android's `name_to_save` uses it).
- `NameField::confirm(tag, result)` logs the failure in core; GTK `NameRow::saved`, Android `name_saved` and the editor save no longer log by hand.
- `DbError::taken_name()`: `is_name_taken` and GTK `clash_text` share it, so `clash_text` now covers `DuplicateBellSound` too (and its test checks that).
- GTK import retry: `name_dialog::pending_import` (keep the file, Cancel removes it) and `NameRow::import_failed` (re-enable, say why) are shared by guided and sound import.
- GTK import leak: the import button's click, the store closure and `on_savable` hold the dialog and button weakly, so a closed import dialog is freed.
- The never-firing `row.name().is_empty()` guards on the GTK import clicks are gone.

Skipped or deferred:
- Android `Import(u8)` and the `guided_import_kind` Cell: the Cell also remembers the kind across the file picker, so it can't simply go. Deferred as REFACTORING #512, with the core naming target.
- Core `PendingImport` for both apps: #513.
- GTK name dialog with its own Save button instead of the can-close mechanism: #514.
- The pattern editors on a kept `NameField` / `NameRow`, and one Slint note property: #515.
- Android `open_name_dialog`'s full match on the target: kept; adding a target then fails to compile, where a `matches!` would default it.
- Android's name check takes the DB lock on the UI thread per keystroke: the same as before the change, and probably uncontended (sync uses its own connection). Not this change.

Suites after /simplify: core 1540+4+11+3, GTK 176, Android 300; both clippy runs clean; check-specs green.

## Hand-over (2026-11-11)
Artifacts: GTK `meditate-gtk/builddir/src/meditate` (11:42), debug APK `meditate-android/android/app/build/outputs/apk/debug/app-debug.apk` (11:43). Lines: 60,392 → 59,727 (−665).

Laptop (`XDG_DATA_HOME=<scratch> LOCALEDIR=meditate-gtk/builddir/po ./meditate-gtk/builddir/src/meditate`):
1. Labels → Create, type " Walk " → saved as "Walk". Create again, type "walk" → "This name is already in use", Create greyed.
2. Rename that label: it opens with "Walk", Rename on; type the name of another label → the hint; Escape closes.
3. Presets: Save current setup under a used name → the hint; Rename a preset to its own name → nothing happens, it closes.
4. Bell sounds → Import a file whose name is already a sound (e.g. a copy of a sound named "Gong.ogg") → the hint at once, Import greyed, before any conversion; type another name → it imports.
5. Guided → import the same file twice → second time the hint at once.
6. Rename a guided file and a sound: own name OK, a taken one shows the hint.
7. Pattern chooser: a bundled pattern ("Pulse") has one pencil and no Rename; it opens "New Pattern" named "Pulse copy"; Save adds a new pattern, Pulse unchanged. In the editor, type "pulse" → the hint, Save greyed.
8. Failed save: open Create Label, type a new name, then in a terminal `chmod 444 <scratch>/meditate/meditate.db`, press Create → the dialog stays with the name and "Save failed"; `chmod 644` it back, Create again → saved.

Phone:
1. Labels: Create " Walk " → "Walk"; Create "walk" → the hint under the field, Save greyed. Rename: own name OK, taken name hint.
2. Presets: Save current setup under a used name → the hint; Rename the same way.
3. Rename a guided file and a sound: the same.
4. Import a sound or guided file whose name exists → the hint at once, before converting; another name → imports.
5. Every name dialog: Enter confirms while Save is on, the keyboard closes with the dialog, the hint text shows in full at phone width.
6. A bundled pattern's pencil still opens a copy, as before.

Proposed commit message:
    core, gtk, android: one name dialog per app, name rules in core

    Every create, rename and import naming goes through one dialog per
    app, run by core's NameField: trimmed, refused when empty or taken,
    "This name is already in use" under the field, and a failed save
    keeps the dialog open with the name (#428). Core refuses empty and
    taken names inside the write, bell sounds included; creating a
    label returns it (#410); a new preset is starred in core. Bundled
    patterns are changed through a copy in both apps. REFACTORING
    #358, #374, #415, #486; OPEN-ISSUES #423, #428, #410.

    Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
    Claude-Session: https://claude.ai/code/session_012WGZUPYHL7TurMz6YSFVJs
