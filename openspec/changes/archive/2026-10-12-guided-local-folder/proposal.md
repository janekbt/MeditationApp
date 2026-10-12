# Guided: a local folder

## Why
GitHub #4: a user keeps guided meditations in a folder tree they manage
outside the app and has little space. Importing copies every file, and on
Android even Open File copies the whole file and reads it frame by frame,
so data grows and each pick is slow. The system file picker cuts off long
names. They want to browse their own folders in the app and see each
file's length and how often they played it.

## What Changes
- Guided gets a Local Folder page (button next to Manage Files, which is
  renamed Manage Imports so the two are told apart). The user
  chooses one folder; its audio files are played where they are, never
  copied, and nothing about them syncs.
- Folders view: one folder level at a time at any depth, subfolders first
  ("2 folders, 5 files"), then files, by name. List view: every file in
  one list with its folder path, sorted by Name, Length, Most played or
  Least played. Sorting exists only in List view. The app remembers view
  and sort like its other settings.
- Each file shows its length and play count. A length is read once and
  read again only when the file's size or date changes. The first scan
  shows "reading N/M" and the list is usable meanwhile; a file
  picked before its length is read is read first, and Start waits for it.
- A saved Guided session from a local file counts one play on this device.
- Local files can be starred anywhere they appear. Starred ones join
  Starred Files on the Guided screen as "Local file", on this device only.
- A starred local file that is gone stays in Starred Files, highlighted
  as "Local file · not found"; tapping it says so and selects nothing; it
  can be unstarred there. A folder that can't be reached shows a bar with
  Choose Again and changes no file's star or plays.
- Changing the folder keeps the old folder's stars and plays; they come
  back if it is chosen again.
- Left out: search; watching the folder for changes (Refresh button);
  syncing play counts or local stars; counting plays of library files;
  importing from the Local Folder page (Open File and Import exist);
  following a file that was moved or renamed outside the app (it counts
  as a new file); counting a session recovered after a crash.

## Capabilities
### New Capabilities
- `guided`: the Local Folder, its views, lengths, plays, stars and missing files.

## Impact
- Core: new `db/local_guided_files.rs` (table, scan reconcile, stars,
  plays, lengths), new `local_folder.rs` (Folders and List view rows,
  sort, subtitle keys), session save gains an optional local file whose
  play is counted in the same transaction, the Starred Files list is
  built in core from library and local rows.
- GTK: `guided.rs` (Local Folder page, folder chooser, background scan
  and length reading), `timer/imp.rs` (Starred Files rows, current file,
  play count on save), `data/ui/timer_view.blp` (button).
- Android: `ui/main.slint` (button, page, rows), `src/ui.rs` and
  `src/guided.rs` (glue), `kotlin/MeditateFilePickerActivity.kt` (a
  folder mode: ACTION_OPEN_DOCUMENT_TREE with persisted read access),
  a scan and length reader in Kotlin, `kotlin/MeditateGuided.kt` (play
  from a content address, not only a path).
- Translations: about 25 new strings (page, views, sort names, counts
  with plurals, "Local file", "not found", folder bar, toasts), all nine
  languages in both apps.
- Data: one new local-only table; the folder location in the local-only
  `sync_state`. No sync event, no schema version bump (design.md).
- Android build environment: untouched. No new permission, activity or
  manifest entry: the folder picker runs in the existing picker activity.
- Docs: README features, ARCHITECTURE.md (local-only table), EVENTS.md
  (note: local folder writes no events), `openspec/specs/guided/spec.md`.
