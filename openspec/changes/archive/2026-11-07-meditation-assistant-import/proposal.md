# Meditation Assistant import

## Why
GitHub issue #16: people moving from Meditation Assistant (F-Droid,
no longer clearly maintained) want to bring their session history. Its
CSV export has its own columns, so today neither import in Meditate
reads it.

## What Changes
- Both apps get a row "Import from Meditation Assistant" in the Migrate
  group, under "Import from Insight Timer". It reads Meditation
  Assistant's `sessions.csv` with the same ask-then-import flow as the
  other imports (unreadable lines reported, duplicates skipped).
- Left out: recognising the format inside the normal CSV import (more
  code, the Migrate row is the existing pattern); the Linux metainfo
  text (it still reads true, and would mean nine new translations).

## Decisions
- Separate row in the Migrate group, like Insight Timer (maintainer, 2026-11-07)
- Started is read as UTC: Meditation Assistant writes it in UTC (its source, `exportSessions`) (my call, from the code)
- Length `HH:MM` is the duration, minute-exact; Completed is not used (my call)
- Message becomes the session's note; no label; mode Timer (my call)
- A 00:00 length (a session under a minute) imports as 1 minute, so none is lost (maintainer, 2026-11-07)

## Capabilities
### Modified Capabilities
- `backup-import`: Meditation Assistant exports import

## Impact
- Core: `meditate-core/src/data_io.rs` (new parser)
- GTK: `meditate-gtk/src/preferences.rs` (row), `meditate-gtk/src/data_io.rs` (logged wrapper)
- Android: `meditate-android/ui/main.slint` (row, callback), `src/ui.rs` (tap, parser choice moved to `app.rs`), `src/app.rs`, `kotlin/MeditateFilePickerActivity.kt` (pick target)
- Translations: GTK "Import from Meditation Assistant", "Upload a Meditation Assistant CSV export", "Import Meditation Assistant CSV"; Android "Import from Meditation Assistant"; nine languages each
- Data: none (sessions written through the existing import)
- Android build environment: untouched
- Docs: README features line
