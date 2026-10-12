# Refresh says nothing false when length reading can't start

## Why
Second review of f4c6b81: on Android, when the length reading of new
files can't be started, the app drops it silently and a tapped refresh
still says "Folder refreshed" while the files keep no length.

## What Changes
- Android: a length reading that can't start shows "Couldn't read
  audio file" and the tapped refresh shows no success; a picked file
  whose reading can't start is not selected.
- Core: a failure marks only a refresh the user tapped and that waits
  for its message; a failure with no tap pending (a pick) is forgotten,
  so it can't silence a later refresh. Replaces the apps' busy check.
- OPEN-ISSUES #188: non-JSON event payloads can stop the database from
  opening (fill, recompute_session, labels); recorded, not fixed.
- Left out: the GTK app has no such path (its reads always start).

## Decisions
- Reuse "Couldn't read audio file", no new text (maintainer, 2026-10-12).
- Gate on the pending tap instead of on busy (my call).

## Impact
- Core: `local_folder.rs`. GTK: `local_folder.rs` (drop busy gate).
  Android: `ui.rs`. Translations: none. Data: none. Build env: untouched.
- Docs: OPEN-ISSUES #188 (batch W); restores the #183 heading f4c6b81 cut.
