# Design: Setup lists in the user's order

## Context
Library guided files sort by `created_iso` as text, like presets
(0a5a72a), and their sync events already carry it; v26.11.0 applies
it from updates. Local files live in `local_guided_files`, a local
table with no sync events, sorted by `rel_path`.

## Goals / Non-Goals
- Goal: one mixed Starred Files order; the library part syncs, the
  local part never leaves the device.
- Non-goal: a synced place for local files (other devices don't have
  them), changing the preset key scheme.

## Decisions
- One key space. Local files get a local `star_key TEXT` column
  holding a key from the same scheme as `created_iso`. Starred Files
  sorts all rows by key (library `created_iso`, local `star_key`),
  ties by uuid / id; a local row with no key sorts after all keyed
  rows, by `by_name` (`local_folder.rs`: file name, then path; today's
  order). Rejected: a local "sits after library
  file X" anchor, which needs its own rules when X is unstarred,
  deleted or moved on another device.
- The key helpers (`key_between`, `just_above`, `count_up`,
  `digits_below`) move unchanged from `presets.rs` to
  `db/order_key.rs` as `pub(crate)`; the fuzz-tested scheme is
  reused, not changed. Local keys take their tail from the row id as
  8 hex digits.
- A move of a starred file, by identity (library uuid or local id)
  and new place:
  1. Build the new mixed order.
  2. If the moved row is a library file and the order of the starred
     library files among themselves changed (unstarred library files
     don't count; they keep their keys and stay where they are among
     the others on Manage Imports, as unstarred presets do): its new `created_iso` =
     `key_between(previous row's key, next row's key)` in the new
     mixed order; one guided file update event, as presets do. When
     there is no room (equal neighbours), every library file is
     re-timed 1 ms apart (unstarred ones where they stand, starred ones
     in the new order), as `move_preset` does, and step 4 then
     repairs the local runs in the same transaction.
  3. If the moved row is a local file: its `star_key` =
     `key_between(previous key, next key)`; local write only.
  4. Then every run of local rows, including one at the top (no lower
     bound) or the bottom (no upper bound), whose keys no longer rise
     strictly inside the keys of the library rows around it gets new
     keys, chained with `key_between` from the lower bound; local
     writes only. This
     is what makes a library file moved only past local files write
     no sync event: its key stays, the passed local rows move.
  Before step 1, starred local rows without a key get keys after the
  highest key, in their current order (local writes), so every row
  has a key to place between.
- Starring a local file sets `star_key = key_between(highest key of
  the starred rows, None)`; unstarring clears it. A library file
  starred again later keeps its old key, so it may stand above that
  local file.
- The core move takes a typed identity, `StarredGuidedId::Library(uuid)`
  or `::Local(id)`; each app maps its own row key (`local:{id}`) to it.
- Unkeyed local rows get their keys at the first move or local star,
  from that moment's time; a library file imported after that stands
  below them. A library file keeps `created_iso`.
- Manage Imports: `move_guided_file(uuid, to)`, `to` a place in the
  list of all library files. Like `move_preset`: only the moved file
  gets `key_between` its new neighbours there (one event), the no-room
  fallback re-times all library files; then step 4 repairs the local
  runs of Starred Files in the same transaction.
- Presets on Setup: a new `move_starred_preset(uuid, to)` turns a place
  among the mode's starred presets into the place `move_preset` takes
  (before the starred preset now at `to`, or after the one before
  it), then calls it. Same events, same order everywhere.

## Risks / Trade-offs
- Another device reorders library files: local rows keep their keys,
  so a local file stays at its place in the key order. It can end up
  next to different library files than before. Accepted (proposal).
- Manage Imports lists all library files by the same key, so a move in
  Starred Files moves the file there too, on every device; the
  unstarred ones keep their keys (my call, as presets on the Manage
  page follow Setup moves).
- The no-room fallback of a Manage Imports move keeps each local file
  at its row in Starred Files (the starred library files fill their
  rows in the new order), while the normal path keeps it at its key.
  The same drag can then read differently in the rare case of equal
  import times. Accepted: equal times need two imports in one
  nanosecond or a hand-edited database (my call, fix review).
- A library file's `created_iso` stops being a time once moved, like a
  preset's. Nothing shows it as a date (grepped GTK, Android, export).
- The library move in step 2 uses mixed neighbours, so its new key may
  sit next to a local key. That key stays between the library
  neighbours too, so other devices see the right library order.

## Migration Plan
`("local_guided_files", "star_key", "TEXT", None)` in `ADDED_COLUMNS`:
a database from v26.11.0 gets the column with NULL on open, so its
order is today's. `SCHEMA_VERSION` stays; v26.11.0 can still open the
database (it ignores the column). Proof: the existing
`an_upgraded_database_has_the_same_columns_as_a_fresh_one`, and the new
`starred_files_from_an_older_database_keep_their_order`. No fixture
has `local_guided_files` yet (v26.10.1 predates it), so a new
`fixtures/schema_v26_11_0.sql` holds v26.11.0's schema exactly
(from `git show v26.11.0:meditate-core/src/db/schema.rs`, as the
v26.10.1 one was made), with a `release_v26_11_0_database()` helper
holding two starred library files and two starred local files.

## Open Questions
None.
