# Writes all or nothing (batch K3)

## Why
Applying a preset writes about twenty values one by one, and each app
writes the length after it. A write that fails partway (a sync holding
the database, a full disk) leaves half a preset on every device, and the
apps' "put the old setup back" can fail the same way and leaves inserted
bells behind (#193). A Box Breath pattern is saved in five steps (#306).
Saving the current setup while the bell list can't be read stores the
preset without its bells, silently (#291). A preset saved with the label on but none picked
keeps whatever label is picked now (#292). Picking a Box Breath phase's
sound or pattern can undo a just-dragged volume (#325). Undoing an
Auto-Finish save leaves the local file's play counted (#324).

## What Changes
- The database gets nestable transactions: every write opens one through
  a single helper that becomes a savepoint inside an outer one. The
  `x` / `x_in` pairs that existed only to share a transaction fold into
  one function each.
- Applying a preset is one transaction, its length included (core writes
  the Timer length or the Box Breath pattern and length). Both apps drop
  the re-apply of the old setup after a failure; "Save failed" stays.
- A Box Breath pattern (four phase lengths and the session length) is
  saved in one transaction; so is each seed with its done marker.
- "Save current setup" passes a failed read on: "Save failed", no preset.
  Tapping a preset whose pre-apply snapshot can't be read says "Save
  failed" and applies nothing.
- A preset saved with the label switch on but no label picked resets the mode's label to the mode default; one with the switch off only switches it off.
- Box Breath phase edits (switch, Type, sound, pattern) change only the
  field picked, on the row read inside the write, in both apps; Android
  no longer writes default sound and pattern when the read fails.
- Undo of an Auto-Finish save takes the local file's play back in the
  same transaction as the delete (Linux toast, Android snackbar).
- Left out: the session-start half of #291 (moved to #308 in batch EE,
  decided); lowering the play count on a Log delete (a session doesn't
  record its local file, so it would need a schema change).

## Decisions
- #291 at session start: moved to EE with #308 (maintainer, 2026-11-10).
- Nested writes use SQLite savepoints; the outermost stays an IMMEDIATE
  transaction so it waits for the lock as today (my call).
- An unreadable snapshot before a preset tap refuses the apply with "Save
  failed" rather than applying without Undo (my call: the apply would
  meet the same lock).
- "Mode default" for #292 is the empty label setting, which every reader
  already resolves to the mode's default label (my call).
- Undo never lowers a play count below zero (my call).

## Capabilities
### Modified Capabilities
- `presets`: apply is all or nothing with its length; save reports a failed read; a preset with the label on but none picked applies the default.
- `box-breath`: the pattern saves whole; a phase edit changes only the field picked.
- `guided`: Undo of an auto-finished save takes its local play back.

## Impact
- Core: `db/mod.rs` (transaction helper), every `db/*.rs` writer, `preset_config.rs`, `settings_keys.rs`, `db/box_breath_phases.rs`, `db/session_in_progress.rs`, `db/seeds.rs`
- GTK: `timer/imp.rs` (apply, phase pickers, Auto-Finish Undo), `db/mod.rs` wrappers
- Android: `src/ui.rs` (apply, phase write helper, Auto-Finish Undo), `src/app.rs` (PendingUndo carries the local file, source tests)
- Translations: none
- Data: none (no schema, no new event; a rolled-back write records no event)
- Android build environment: untouched
- Docs: ARCHITECTURE.md (transactions), OPEN-ISSUES.md (K3 out, #291 session-start half to #308), REFACTORING.md (#495 and the pair items it overlaps)
