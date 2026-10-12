# Log file name and Local Folder refresh

## Why
A session played from the Local Folder or from Open File shows only
"Guided" in the Log, because only library files have a name that
syncs. The maintainer wants the file name on the card, on every
device. Separately, the refresh button on the Local Folder page gives
no sign that it is working or that it finished; the maintainer wants
it to spin until the refresh is done and then say so.

## What Changes
- A guided session stores the name of the file it played and syncs
  it; the Log shows the library name when the file is in the library,
  otherwise this stored name. Also kept through a crash recovery and
  a backup restore.
- An older app that edits such a session (label, note) does not erase
  the name.
- The Local Folder refresh button spins while the folder is read and
  while new files' lengths are read (also when the page started the
  read itself). While lengths are read, a percentage stands next to
  the spinning icon: the share of the files this refresh has to read
  that are done (failed reads count as done). While the folder itself
  is walked the total is unknown, so the icon spins without a number.
- The folder line goes back to "N files" only; its "reading N/M" is
  replaced by the percentage (one place for progress, one string
  less). After a tapped refresh ends, a toast (GTK) or snackbar
  (Android) says "Folder refreshed". An unreachable folder shows its
  existing banner instead.
- Left out: per-file Stats for local and Open File sessions (not
  asked); counts in the toast such as "3 new" (not asked); a toast
  after a refresh the page started itself (only a tap asks for one).

## Capabilities
### New Capabilities
- `log`: a guided session's card names its file, on every device.
### Modified Capabilities
- `guided`: the Local Folder refresh shows that it runs and when it ends.

## Impact
- Core: `db/sessions.rs`, `db/session_in_progress.rs`, `db/schema.rs`
  (one added column), `db/guided_files.rs` (Log name), `data_io.rs`
  (one CSV column), `local_folder.rs` (when the refresh toast shows).
- GTK: `timer/imp.rs` and `db/mod.rs` (pass the name on save),
  `log/imp.rs` (card), `local_folder.rs` (spinner, toast).
- Android: `ui.rs` (save, Log card, refresh state, snackbar),
  `ui/main.slint` (spinner in place of the refresh icon).
- Translations: "Folder refreshed" and "{n}%" added, "reading {read}/{total}"
  removed; nine languages, both apps.
- Data: one added nullable column (`sessions.guided_file_name`)
  through `ADDED_COLUMNS`, no SCHEMA_VERSION bump; the snapshot's
  unused `mode_payload` holds the name; one added optional field in
  the session payload; names an older release received are filled once
  when the column is added. See design.md.
- Android build environment: untouched.
- Docs: EVENTS.md (payload field), ARCHITECTURE.md (added columns),
  README (if the Log feature line names files), OPEN-ISSUES none.
