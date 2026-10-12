# Design

## Data
- `sessions.guided_file_name TEXT`, nullable, added by
  `ADDED_COLUMNS` (the same path `volume_pct` took). The running
  session's snapshot keeps the name in its existing `mode_payload`
  column, which both apps left as "{}", so it needs no column. The release before
  can still open and write the database; it ignores the column.
- Session payload gains `"guided_file_name": "<name>" | null`. Old
  peers ignore unknown fields on apply.
- Written for every guided session: the title the Guided page showed
  for the pick (library name, local file name without folder, Open File
  name). Non-guided sessions write null.
- Apply (`session_insert` / `session_update`): when the winning event
  carries no name, the name comes from the newest event of that
  session that carries one. A name never legitimately goes back to
  empty, and an older peer that edits a label or note sends no name;
  without this the edit would erase it on every device. Reading it
  from the events (not the stored row) also holds on a full replay.
  This is the one field where an update is not a full overwrite;
  EVENTS.md says so.
- A device that applied named sessions while still on the last
  release fills the names once, when the column is added: one UPDATE
  from the newest event that carries a name. (A first try bumped
  `CACHE_SCHEMA_VERSION` instead; that replayed all 21,900 events on
  the FP5 and made the first start slow.)
- CSV backup: a tenth column `guided_file_name`, last, so older
  readers still find theirs (the reader is already flexible).

## Log
`guided_file_name_for(uuid, stored_name, library_names)`: the library
name when the uuid resolves (renames show), else the stored name, else
nothing. Both Log cards call it.

## Local Folder refresh
- Busy = the folder walk runs or lengths are still being read. Both
  apps already track both (GTK `SCANNING`/`READING`/queue, Android
  `scanning`/`reading`).
- Core `RefreshProgress`, one per app: `tap()`, `queued(id)`,
  `finished(id)`, `write_failed()`, `percent()`, `settle(busy,
  unreachable)`. Ids are sets, so a file queued again (a pick's
  read-first) counts once. `percent()` is None while nothing is queued
  (the walk), else done*100/total rounded down. `settle` runs on every
  redraw (GTK `notify`, Android the poll when something changed); once
  idle it clears the run and returns true once if the user tapped, the
  folder was reachable and no write failed during the run.
- The tap counts only when a refresh is running after it, so a scan
  that could not start leaves no tap waiting.
- A save that fails while a refresh runs marks the run failed (in the
  apps' shared save helpers); "Save failed" is the message then.
- GTK: the refresh button's child is a box with icon, adw::Spinner and
  percent label, toggled by visibility. Android shows the spinner and
  percent in place of the icon, still tappable. "{n}%" is a translated
  string (de and fr write "42 %").
- `FolderLine.read` and the "reading" text of the folder line go.
