# Setup lists in the user's order

## Why
GitHub #22 asks to rearrange the starred guided files. Today Starred
Files lists the imported (library) files by import time, then the
local folder's files by name, with no way to move them. The starred
presets on the Setup page can only be moved on the Manage Presets page,
which is one step away from where they are used.

## What Changes
- Starred Files (Guided Setup, both apps): every row gets a drag
  handle at its left, the same as the Manage Presets rows; Linux also
  moves the focused row with Alt+Up / Alt+Down. Library and local
  files can stand in any mix.
- Only the library files' order among themselves syncs. Moving a file
  that changes it writes one sync update for the moved library file;
  anything else writes to sync nothing (except when two library files
  share one import time and leave no room: then every library file is
  re-timed, as for presets).
- The place of each local file is kept on the device only.
- Manage Imports (both apps): every library file row gets the same
  handle and keys; a move there is a synced move among all library
  files, and Starred Files follows it.
- Presets (Timer and Box Breath Setup, both apps): the starred presets
  list gets the same handle and keys; a move there is the same move as
  on the Manage page, so the order syncs as today.
- Code names: the Android preset list is still called "chips" from
  before 2026-05-04 (`refresh_preset_chips`, `preset-chip-tap` and
  their comments); they become "list"/"row" names, no behaviour
  change. Real chips (mode toggles, Log label chips) keep theirs.
- Left out: a handle for unstarred files (they are not in the list),
  auto-scroll while dragging (Slint has none; presets live with it),
  Guided from the widget and "remember the last file" (the other two
  wishes in #22, separate changes).

## Decisions
- Library and local files mix freely; only the library order syncs
  (Janek, 2026-11-07).
- Moved directly in the Setup lists, handle always visible (Janek,
  2026-11-07).
- The starred presets in Setup get the same handle in this change
  (Janek, 2026-11-07).
- Scrolling vs dragging: only the handle drags; on Android a quick
  swipe on the handle still scrolls, a short press then drag moves, as
  on the Manage Presets page (Janek asked; existing behaviour reused).
- A newly starred local file comes last. A local file unstarred and
  starred again comes last. A library file starred again returns to
  its old place, since its place is part of the synced file (my call,
  shown to Janek 2026-11-07).
- Local files starred before this change keep today's place: after the
  library files, by name. They get keys the first time any local file
  is moved or starred; from then on a library file imported later
  stands below them (my call).
- When another device reorders the library files, a local file keeps
  its place in the order, not its neighbour: it may end up between
  other library files (my call; see design.md).
- Rename the leftover preset "chip" names (Janek, 2026-11-07).
- Handles on Manage Imports too (Janek, 2026-11-07, with the go).
- No new strings: the Linux tooltip "Drag to reorder" exists; Android
  shows no tooltip (my call, as for presets).

## Capabilities
### Modified Capabilities
- `guided`: Starred Files stand in the user's order; only the library
  order syncs.
- `presets`: the starred presets on Setup can be moved there too.

## Impact
- Core: `db/presets.rs` (the order-key helpers move to a shared
  `db/order_key.rs`; a move by place among the starred presets),
  `db/guided_files.rs` + `db/local_guided_files.rs` + `local_folder.rs`
  (the mixed order, a move of a starred file), `db/schema.rs` (one
  added column).
- GTK: `timer/imp.rs` (Starred Files and Presets rows), `guided.rs` (Manage Imports rows), `presets.rs`
  (the handle and keys made shareable), `db/mod.rs` wrappers.
- Android: `ui/main.slint` (handle and drop areas on both Setup
  lists and Manage Imports), `src/ui.rs` (three move callbacks, refresh lists and widget).
- Translations: none, both apps.
- Data: one added local-only column, filled empty on old databases
  (the existing `ADDED_COLUMNS` path; no version bump). Sync: no new
  event kind or field; a moved library file is an ordinary guided file
  update whose `created_iso` changed, which v26.11.0 already applies.
- Android build environment: untouched.
- Docs: `openspec/specs/guided`, `openspec/specs/presets`;
  `ARCHITECTURE.md` (the column, the order key); `EVENTS.md`
  (a guided file's `created_iso` is a sort key, never a time);
  `README.md` features line if it mentions preset order.
