# Writes that keep another device's edit

## Why
Saving something without changing it still sends a full copy to the other devices, with a newer clock. The edit another device made in the meantime is lost on both devices: a renamed preset, a changed Box Breath phase, an interval bell, a vibration pattern, a starred guided file. Some smaller sync faults sit in the same code: old name conflicts are solved again on every sync, a damaged duration from a peer turns into a wrong length, merging labels leaves presets pointing at the deleted label, guided session titles stay empty after a round trip through an older release, and the bundled bell is called "Kanshō" on Linux and "Kansho" on Android. OPEN-ISSUES batch CC: #248, #244, #241, #242, #243, #245, #444, #184 (#249 not kept, see Decisions). REFACTORING P1, first half: #234 (with #225 and the session payload folded in), #370, #226, #218, #230.

## What Changes
- A save that changes nothing writes nothing and sends nothing, for every synced kind: presets, interval bells, Box Breath phases, vibration patterns, guided files, bell sounds and sessions.
- A local save builds its row the way the other device builds it from the event, so what one device stores is what every device stores.
- A name conflict that is permanent is not solved again on each sync.
- A peer's length or time (session, guided file, vibration pattern, interval bell) that is too large or not a whole number is clamped as a local one is.
- Merging labels also moves the presets that use the merged-away label to the kept one.
- Rows an older release changed are rebuilt on the next start of this release, for every synced kind (replaces the two special cases for interval bells and guided file names). The rebuild after a cache upgrade does each item once.
- The bundled bell is "Kansho" in both apps; a stored "Kanshō" on it is shown as "Kansho", without a sync event.
- Linux shows Undo after saving a vibration pattern only when it changed (Android has no Undo there).
- A local save never sends this device's "(conflict-...)" suffix as the item's name, so the suffix can still heal.
- Left out: the pure clean-up of P1 (one reader and one delete path per table, #235, #216, #237, #215, #217, #231) is the second change of this package, as decided.

## Decisions
- #444: "Kansho" everywhere (maintainer).
- #218 moves from P15 into this change, since #226 builds on it (maintainer).
- Two changes in a row: this one first, then the P1 clean-up (maintainer).
- #249: too unlikely for a new column; moves to "Checked and not kept" (maintainer).
- #230 (one helper for the conflict suffix) moves into this change, since the #241 fix needs to strip the suffix (my call).
- A stored "Kanshō" on the bundled bell becomes "Kansho" where the row is built from its event and, for a row without events (after Wipe Local), at start; on every device alike, with no event, so none can undo a peer's rename. A user who types exactly "Kanshō" for it gets "Kansho"; any other name they give it stays (my call, after review: a sent rename could undo a peer's).
- Android has no Undo after a pattern edit today; adding one would be a new feature with a new string, so it is left out (my call, pre-build question 4).
- Name checks: every write that sets a name (insert, rename, an update carrying a name) checks it first and returns `Duplicate*`, since the rebuild would suffix a clash silently (my call).
- "Unchanged" for a pattern compares points with the existing 1e-6 tolerance (the editor resamples); for a preset config it compares the parsed JSON, the text only when it doesn't parse (my call).
- The shared writer drops this device's own "(conflict-<uuid>)" suffix from the name it sends; the rebuild puts it back where the clash still exists (my call, pre-build question 8).
- Sessions get the shared payload and the guard, not the rebuild-on-write: imports of thousands of rows stay one INSERT each (my call).
- #243: the merge rewrites the presets in its own transaction and sends their updates; no lookup-time mapping (my call, the issue's first option). Known limit, kept by the maintainer after review: a preset edited on another device at the same time can lose that edit or point at the deleted label again; one person on one device at a time makes this very unlikely, and a new sync field for it would be overkill.
- "Unchanged" compares the stored row with the new row before the edit time is stamped, so a save that only would bump `updated_iso` counts as unchanged (my call).

## Capabilities
### Modified Capabilities
- `sync`: saves without a change send nothing; local rows equal peer rows; a permanent name conflict is settled once; a peer's duration is clamped.
- `labels`: a merge moves presets too.
- `log`: guided session names come back after a round trip through an older release.
- `bells`: one name for the bundled Kansho bell.
- `vibration`: Linux offers Undo after Save only for a change.

## Impact
- Core: `db/` presets, interval_bells, box_breath_phases, vibration_patterns, guided_files, starred_order, bell_sounds, sessions, labels, events, error, schema, mod, seeds.
- GTK: `vibrations.rs`, `vibration_editor.rs` (Undo only on change), `db/mod.rs` (bundled name, wrapper return type); callers of the changed return types.
- Android: none beyond callers of changed return types (its bundled name is already "Kansho").
- Translations: none (no new or changed strings; "Kansho" is a bell name, not translated).
- Data: no schema change, no wire-format change (same event kinds and payload keys). One new `sync_state` key (the last applied event id).
- Android build environment: untouched.
- Docs: EVENTS.md (the unchanged rule, local rows built from the event), ARCHITECTURE.md (start-up rebuild), OPEN-ISSUES.md (CC removed, #249 moved), REFACTORING.md (#218 and #230 into P1, done items removed).
