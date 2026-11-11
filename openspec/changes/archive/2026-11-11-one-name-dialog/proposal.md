# One name dialog

## Why
Every place where the user names something (preset, label, guided file, bell sound, vibration pattern; create, rename, import) has its own dialog code: nine in the Linux app, seven in the Android app. The name rules (trim, not empty, not taken) live in each dialog, not in core, so they drift. The results:
- A failed save on Android closes six of these dialogs and throws the typed name away (#428).
- Creating a label on Android can say "Save failed" for a label that was saved (#410).
- A failed name check counts as "name is free" (#423's rest).
- Only some dialogs say why Save is greyed out.
- Core lets bell sound names repeat (both apps' dialogs already refuse a used name, over both bell categories, but a sync or an old database can bring one in).
- Bundled vibration patterns differ: Linux renames them, Android edits a copy.

P4-a of REFACTORING.md: #486, #358, #374, #415. Bugs that ride along: #423, #428, #410.

## What Changes
- Core trims every created or renamed name and refuses an empty one or one already used by another item of the same kind, ignoring case. Bell sounds join this rule; duplicates that already exist stay.
- Creating a label returns the new label, so no app reads it back.
- Creating a preset stars it in core (today each app asks core for the "starred on save" default and passes it in).
- Linux gets one name dialog and Android gets one name dialog, used by every create, rename and import naming in that app.
- While the typed name is taken, the dialog shows "This name is already in use" and Save is off. While the name is empty, Save is off with no text.
- A failed save keeps the dialog open with the typed name.
  - A name taken meanwhile (by a sync) shows the hint.
  - Any other failure shows "Save failed".
  - Save again retries.
- The vibration pattern editor's Name field follows the same rules and hint.
- Bundled pattern rows get one pencil in both apps that opens a copy in the editor ("<name> copy"); Linux loses its Rename dialog for them. A bundled pattern is renamed by copying it.
- Left out:
  - #465, the Android dialog frame, with #474 and #475: P4-b, its own change.
  - Toasts and Undo after a bell sound rename (#509): decided as its own change.

## Decisions
- Failed save: the dialog stays open with the name (maintainer, 2026-11-11).
- Bell sounds: names must be free, as everywhere, on import and rename; existing duplicates stay (maintainer, 2026-11-11). What the user sees doesn't change: both dialogs already refuse a used name; core now refuses it too.
- Taken name: "This name is already in use" under the field in every name dialog of both apps; GTK's "A guided file with this name already exists." goes (maintainer, 2026-11-11).
- Bundled patterns: one pencil, "edit a copy", in both apps; no rename (maintainer, 2026-11-11, replacing the earlier "Rename on Android": "Beide Apps bekommen nur einen Stift der immer 'Kopie bearbeiten' bedeutet. Umbenennen kann man also nur durch das Anlegen einer Kopie.").
- (my call) Core reports the two refusals with the existing duplicate errors plus a new bell-sound duplicate and one "empty name" error, not a new error type.
- (my call) A name taken at save time shows the inline hint, not GTK's "«X» is already taken" toast, so both apps behave alike.
- (my call) The pattern editor is a page, not a dialog. It gets the hint too, so "everywhere" holds.
- (my call) The name dialog only asks and shows. Whether the name is free is decided by core inside the write, so a check that can't read while typing leaves Save on, and the write decides.

- (my call) The checks sit in the user-facing writes: create, rename, the pattern editor's create and edit, and a new import entry for bell sounds and guided files that both apps switch to. The `_with_uuid` inserts stay unchecked for seeds, CSV restore and tests, so startup seeding and restoring old data can't trip them.
- (my call) One name set for bell sounds over both categories, as both apps check today.
- (my call) Of two sounds that share a name, renaming one only in case is refused: the other holds the name.

## Capabilities
### New Capabilities
- `names`: how both apps take a name for a new or renamed item.
### Modified Capabilities
- `vibration`: bundled patterns are changed through a copy on both apps.

## Impact
- Core: `naming.rs`, `db/error.rs`, `db/labels.rs`, `db/presets.rs`, `db/guided_files.rs`, `db/vibration_patterns.rs`, `db/bell_sounds.rs`, `preset_config.rs` or wherever presets are created.
- GTK: a new `name_dialog.rs`; `vibrations.rs` (bundled rows: Edit a copy instead of Rename); `presets.rs`, `labels.rs`, `guided.rs`, `vibrations.rs`, `sounds.rs`, `vibration_editor.rs`, `db/mod.rs` (`clash_text` if it becomes unused).
- Android: `ui/main.slint` (one name dialog in place of seven Modal variants), `src/ui.rs` (one `NameTarget` with confirm and changed handlers), `src/app.rs` (the dialog's testable state).
- Translations:
  - Android gets "This name is already in use" in nine languages, taken from the GTK .po files.
  - GTK gets "{name} copy" in nine languages, taken from the Android .po files ("{} copy").
  - GTK drops "A guided file with this name already exists." and "Rename Pattern".
- Data: none. No schema change; the bell rule is a check in the write.
- Android build environment: untouched.
- Docs: REFACTORING.md (P4 row, #358 #374 #415 #486 out), OPEN-ISSUES.md (#423 #428 #410 out), new `openspec/specs/names/spec.md`.
