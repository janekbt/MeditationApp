# Failed writes and stale pages (batch K)

## Why
When a database write fails (a sync holds the lock, the disk is full),
both apps often say it worked: "Deleted · Undo" for a pattern that is
still there, "Overridden" for a preset that wasn't, a guided file whose
audio is gone while its row stays, a dialog that no longer reacts. And
several pages keep old names or numbers after a change elsewhere: the
streak line, sound and pattern names, a deleted guided file still
selected, an open chooser after a sync, the Box Breath counter. A CSV
export over an old backup empties it before writing. OPEN-ISSUES #52,
#80, #78, #84, #124, #136, #140, #141, #170.

## What Changes
- #52 (GTK), #80 (Android): every write in presets, labels, Log
  delete, interval bells, sounds, patterns, guided files and the daily
  goal checks its result. On failure: "Save failed" or "Delete
  failed", no success message, no Undo, and the item stays as it is.
  A guided file's audio is removed only after its row is deleted. On
  Android a dialog closes with the message instead of going dead, and
  a Log delete that failed brings its row back.
- #78 (Android): Save, Edit and Recovery Undo refresh the Timer
  page's streak line and Stats.
- #84: after a sound or pattern is renamed or deleted, the Interval
  Bells list, the bell edit page (GTK) and Setup's bell rows show the
  new name or "Missing". Android: a renamed label shows its new name
  in the preset chips and the home-screen widget.
- #124: GTK: deleting or renaming the selected guided file in Manage
  Files, or a sync pulling that from another device, clears or
  renames the selection; Undo of that delete selects it again (both
  apps). Android: the same after a sync, and an open bell or pattern chooser refills after a sync.
- #136 (Android): the Box Breath counter shows the running session's
  length through Pause and Resume.
- #140 (Android): Back from the interval-bell editor stops its preview.
- #141 (Android): a bell or pattern preview that can't play shows
  "Playback failed" instead of a silent "Stop".
- #170 (core): CSV export writes a temporary file and replaces the old
  backup only once the new one is complete.
- Left out: about 20 settings writes on GTK's Timer page that fail
  silently but claim nothing; now #191 (Janek, 2026-10-13). The sync
  code's own atomic file write stays as it is (not broken).

## Decisions
- GTK says "Delete failed" for a failed delete, "Save failed" for any
  other failed write; the nine translations come from the Android app,
  which has the same text (Janek, 2026-10-13).
- The silent Timer settings writes become #191 (Janek, 2026-10-13).
- Undo of a deleted selected guided file selects it again, in both
  apps (Janek, 2026-10-13).
- A rename that clashes with an existing name keeps its "«X» is already
  taken" message. (my call)
- Android: a failed write closes its dialog and shows the message,
  as the label dialogs already do; the user opens it again to retry.
  (my call)
- A failed star toggle shows "Save failed"; the star shows the stored
  state at the next list rebuild. (my call)
- GTK's failed Log delete reloads the Log, so the card comes back.
  Android keeps the row and shows "Delete failed" once the Undo
  message is gone. (my call)
- #141 shows the existing "Playback failed" message. (my call)

## Capabilities
### New Capabilities
- `presets`: preset writes tell the truth.
- `backup-import`: an export never destroys the old backup.
### Modified Capabilities
- `labels`, `log`, `vibration`, `bells`, `guided`, `stats`: failed
  writes say so; names and selections follow renames and deletes.
- `timer`: the streak line follows a save.
- `box-breath`: the counter reads the running session.

## Impact
- Core: `data_io.rs` (export through a temporary file).
- GTK: `db/mod.rs` (one write-result helper), `presets.rs`,
  `labels.rs`, `log/imp.rs`, `vibrations.rs`, `bells.rs`, `sounds.rs`,
  `guided.rs`, `preferences.rs`, `timer/imp.rs`.
- Android: `ui.rs`, tests in `app.rs`.
- Translations: GTK gets "Delete failed" in nine languages (copied
  from Android). Android: none.
- Data: none.
- Android build environment: untouched.
- Docs: OPEN-ISSUES.md (batch K removed and ticked, #191 added).
