# Design: Guided local folder

## Context
Library files (`guided_files`) sync: a row event plus `<uuid>.ogg` on the
server. Local folder files must never sync, since the other device lacks
them. Today the GTK app plays an Open File pick in place (portal path);
Android copies it to `transient.<ext>` and plays a file path.

## Goals / Non-Goals
- Goals: browse and play a user folder in place on both apps; length,
  play count and star per file per device; nothing lost when a file or
  the folder goes away for a while.
- Non-Goals: sync of any of it; several folders at once; following moves.

## Decisions
- **One local-only table** `local_guided_files`: `root` (the folder
  location as the platform gives it: a path on Linux, a tree address on
  Android), `rel_path` (folder path plus file name, `/`-separated),
  `size`, `mtime`, `duration_secs` (NULL until read), `is_starred`,
  `play_count`, `missing`. Unique on (`root`, `rel_path`). It writes no
  event and `wipe_local_event_log` leaves it alone (it can't be pulled
  back). Rejected: rows in `guided_files` with a flag; every sync path
  would need to skip them, and one missed path leaks a row to peers.
- **Folder location** in `sync_state` under `local_guided_folder`, which
  is already local-only and kept by Wipe Local. Rejected: `settings`
  (synced) and a second new table for one value.
- **Identity is (`root`, `rel_path`).** A moved or renamed file is a new
  row; the old one becomes missing. Rejected: matching by size and
  length, which merges different talks of equal length.
- **Scan reconcile in core:** the app walks the folder and hands core
  (`rel_path`, `size`, `mtime`) for every audio file; core inserts new
  rows, clears `missing` on found ones, sets `missing` on the rest, and
  sets `duration_secs` to NULL where size or date changed. A walk that
  fails (folder not reachable) changes nothing. No row is ever deleted,
  so stars and plays survive. The app then reads lengths for rows with
  NULL, one at a time in the background, and writes each to core.
- **Views in core:** core builds the rows for one folder level (direct
  subfolders with their direct folder and file counts, then files) and
  the List view (sorted; ties by name; unknown length sorts last under
  Length), and the Starred Files list merging library and local rows.
  Missing rows are not shown in either view; starred missing rows show
  in Starred Files only. The apps draw what core returns.
- **Play count:** `insert_session_clearing_snapshot` gains an optional
  local file key and bumps its `play_count` in the same transaction.
- **Walking:** Linux walks with `std::fs` on a worker thread and does
  not follow symbolic links to folders (a link loop would never end).
  Android walks with `DocumentsContract` children queries in Kotlin on a
  worker thread and writes the result for Rust to read, like the
  existing pick drop-file.
- **Lengths:** both apps use their existing probe, which the session
  length depends on (a short length rings the end bell mid-talk). Linux:
  `probe_duration_secs` on the worker thread, never the main thread.
  Android: the existing `probe` (max of header and last frame time; the
  header alone was tried and was wrong for VBR MP3), opened from the
  content address instead of a copied file. Rejected: header only.
- **Picking a file whose length isn't read yet:** it moves to the front
  of the reading queue; the current file shows "reading length" and
  Start stays disabled until the length is in (about a second).
- **Play key:** the picked file's row id travels with the session to
  save. Rows are never deleted, so a folder change during the session
  still counts the play on the right file.
- **View and sort** are ordinary `settings` keys in core, used by both
  apps like the rest of the UI state (so they follow the user across
  devices). Rejected: GSettings (Linux only).
- **Android scan entry:** Rust calls a static Kotlin method over JNI
  that runs the walk and the length reading on a worker thread, like
  `guided::start_import`; new Kotlin files need no Gradle or manifest
  change.
- **Transactions:** the play-count update runs inside the existing
  `insert_session_clearing_snapshot` transaction, through the same
  connection, like its snapshot delete.
- **Playing:** Linux plays the file path as today. Android's
  `startAudio` gains a content-address form
  (`setDataSource(context, uri)`); library and transient files keep the
  path form.
- **Folder access:** Android takes persisted read access to the tree.
  Linux: `FileDialog::select_folder` through the portal; the app keeps
  the returned path.

## Risks / Trade-offs
- The Flatpak may lose the portal's folder access after a restart. Must
  be checked on the device early (task 1.0). If it does not keep it, the
  bar "Folder not reachable" with Choose Again covers it, at the cost of
  choosing again after each restart; then ask the maintainer whether to
  add a `--filesystem=xdg-music:ro` finish-arg instead.
- The Flatpak may hand back a new portal path when the same folder is
  chosen again; then stars and plays would stay under the old root.
  Checked in task 0.1. If so, key the root by the host path the
  Documents portal reports (`GetHostPaths`) instead; ask before that.
- A first scan of hundreds of files reads every file once (frame walk
  on Android, a GStreamer probe on Linux); it runs in the background
  with progress.

## Migration Plan
`CREATE TABLE IF NOT EXISTS` adds the table when a database from 26.10.1
opens; no column of an existing table changes, so `SCHEMA_VERSION` stays
1 and 26.10.1 can still open the database (it ignores the table). Proof:
`a_database_from_the_last_release_gains_the_local_folder_table`, which
opens a database written with the 26.10.1 schema, checks the table
exists and that sessions and library files are unchanged.

## Open Questions
- None blocking. Flatpak folder access after restart is checked in task 1.0.
