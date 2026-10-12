# Adding a new sync-able entity

This walkthrough lists every code surface a new entity touches in
`meditate-core` and the order that lets you ship it without a
half-wired state. The shape mirrors the seven entity types already
in the workspace: `Session`, `Label`, `BellSound`, `IntervalBell`,
`Preset`, `GuidedFile`, `VibrationPattern`. `BoxBreathPhase` and
`Setting` are update-only kinds (see the notes at the bottom).

Use the quick checklist at the end. Several surfaces have no
compile-time check; missing one breaks sync silently.

## 1. Pick a name + typed UUID

Add a newtype to `meditate-core/src/db/uuids.rs` via the
`entity_uuid!` macro:

```rust
entity_uuid!(FooUuid, "Cross-device identity for a `Foo` row. …");
```

Re-export from `meditate-core/src/db/mod.rs`:

```rust
pub use uuids::{..., FooUuid};
```

Why: `FooUuid` and `BarUuid` can't accidentally be swapped at any
struct field or fn signature. Wire format is bare string via
`#[serde(transparent)]`.

## 2. Schema row

Add the `CREATE TABLE` to the format-string in
`meditate-core/src/db/schema.rs::schema()`. Defaults that reference
seed UUIDs should interpolate via `{SEED_CONST}` rather than
hardcoding the literal string.

UNIQUE constraints on the row's `uuid` column are mandatory; the
sync convergence story depends on UPSERTs keyed on uuid.

**Do NOT bump `SCHEMA_VERSION`** for a new table. Adding tables is
forward-compatible (older builds skip them). After a bump, every
older build refuses a database the new build has opened, so a
downgrade stops working. Only bump for non-additive changes that
older builds really can't read.

Bump `CACHE_SCHEMA_VERSION` in `schema.rs` so
`maybe_walk_events_for_cache_upgrade` replays foo events that older
builds recorded but skipped. A new column on an existing table goes
into `ADDED_COLUMNS`, not `SCHEMA_VERSION`.

## 3. Module + row struct

Add `meditate-core/src/db/foos.rs` with:

- `pub struct Foo { id: i64, uuid: FooUuid, name: String, ... }`
- `pub fn insert_foo`, `update_foo(&Foo)`, `delete_foo(uuid: &str)`
  as methods on `Database`.
- Readers `list_foos_from_db`, `find_foo_by_uuid_from_db` (free
  functions, re-exported from `db/mod.rs`).
- `pub(super) fn recompute_foo(&self, foo_uuid: &str) -> Result<()>`:
  pure function of the events table, reached through `recompute_for`
  from `apply_event_inner` and `replay_events`. Pattern: query the
  winning mutate event via `winning_mutate`, then UPSERT the cache
  row by uuid.
- If `name` is UNIQUE, do the UPSERT through
  `self.claim_name("foos", uuid, name, |name| upsert)` and add
  `(EntityKind::Foo, "foos")` to `reclaim_suffixed_names` in
  `events.rs`. See "Name collisions" in `EVENTS.md`.

Wire it up in `meditate-core/src/db/mod.rs`:

```rust
mod foos;
pub use foos::Foo;
```

CRUD methods on `Database` must:
- Open an `unchecked_transaction` before any mutation.
- Emit one event per mutation via `self.emit_event(&tx, ...)`; see step 5.
- Be idempotent on uuid via `existing_rowid_by_uuid` (re-inserting
  an existing uuid returns silently; deleting an unknown uuid
  silently no-ops AND emits no event).
- Return without an event on a rename to the current name. A fresh
  event would override a newer rename from a peer.
- Surface `DbError::DuplicateFoo(name)` if the row has a UNIQUE
  collision on a user-visible name column. Also map the new variant
  in `meditate-gtk/src/db/mod.rs` and `meditate-core/src/data_io.rs`
  (both are exhaustive matches).
- If the delete has an Undo toast, add `restore_foo(&Foo)` that
  re-inserts under the same uuid and `created_iso`, like
  `restore_preset`.

## 4. EventKind variants

Three new variants in `meditate-core/src/db/events.rs::EventKind`:

```rust
FooInsert,
FooUpdate,
FooDelete,
```

Wire their `as_db_str` / `from_db_str` strings (use `foo_insert`,
`foo_update`, `foo_delete`; snake_case matches the rest of the
log). Add an `EntityKind::Foo` variant and map the three to it in
`entity()`.

Add the three variants to the list in the
`event_kind_round_trips_through_db_strings` test. The compiler
forces `as_db_str` and `entity()`, but not `from_db_str`, which
falls through `_ => None`. A forgotten arm there means the kind is
silently recorded and never applied.

## 5. emit_event call sites

In each of the three CRUD methods (insert / update / delete),
build a JSON payload and call:

```rust
self.emit_event(&tx, EventKind::FooInsert, uuid_str, payload)?;
```

The payload schema is your choice, but every mutable field
should be present in `FooInsert` AND `FooUpdate` so the events
are self-sufficient (replay can materialise a row from a single
event without needing the insert event to have arrived first).

## 6. recompute_for dispatch arm

`apply_event_record_only` records the event and validates
`target_id` via `target_id_is_well_formed_for`. Then `recompute_for`
dispatches to the right `recompute_*`. Add the arm
`EntityKind::Foo => self.recompute_foo(target_id)` to
`recompute_for` (an exhaustive match, so the compiler asks for it).

If Foo stores another entity's local id, its recompute has to run
after that entity's, like the Session-after-Label pass in
`replay_events`.

`target_id_is_well_formed_for` is the path-traversal validator.
It rejects `../etc` etc. before the recompute runs. Unless your
entity has a constrained target_id shape (like `BoxBreathPhase`'s
`in`/`holdin`/`out`/`holdout`), the default path-component
validator covers it.

## 7. wipe_local_event_log

`wipe_local_event_log` (the "wipe local to match remote" recovery)
clears the event log and every cache table except `settings`. Add
`DELETE FROM foos`. If Foo has bundled rows, use
`WHERE is_bundled = 0`, because the seed gate won't re-seed them.
If Foo's rows must always exist, re-seed them inline like the
box-breath phases. If Foo has audio files, also clear its
`known_remote_*` table.

This is the surface most often missed when adding an entity.
There's no compile-time check for it; the failure mode is "the
table looks fine until a user invokes recovery."

## 8. Seeds (if bundled rows exist)

If the entity ships bundled rows (like the bundled bell sounds or
vibration patterns), add:

- The UUID constants and `FOOS_SEEDED_KEY` to
  `meditate-core/src/seeds.rs`.
- A `seed_default_foos` method to `meditate-core/src/db/seeds.rs`,
  gated on `FOOS_SEEDED_KEY` (so a deleted seed row stays deleted
  across reopens; without the gate, the seed resurrects the row and
  the resurrect event's higher Lamport ts overrides the user's
  delete on every synced peer). The gate is a synced setting.
- Add the call to `seed_all_non_audio()` if the seed payload is
  platform-agnostic. Audio files stay shell-side (the file paths
  are per-shell).

## 9. Preset / preset_config payload (if reachable from a preset)

If a `Foo` is referenced from a saved preset's `config_json`:

- Add the field with `#[serde(default)]` to the relevant struct in
  `meditate-core/src/preset_config.rs`, so older configs still
  parse.
- Read it in `snapshot(db, mode, timing)`.
- Write it back in `apply(db, cfg, mode)`, which writes references
  unchecked (Setup shows a missing one as Missing).

## 10. File sync (if Foo has audio files)

Add pull and push of the files in `meditate-core/src/sync/orchestrator.rs`
(like `pull_custom_sound_files` / `push_custom_sound_files`), a
`known_remote_*` table, and a case in
`remove_files_of_pulled_deletes`.

## 11. EVENTS.md

Add the kinds, payload fields and defaults to the catalogue in
`EVENTS.md`.

## BoxBreathPhase and Setting: the update-only kinds

`BoxBreathPhase` is a singleton-per-phase row (always exactly 4
rows, keyed on `phase IN ('in', 'holdin', 'out', 'holdout')`). It
has no insert / delete events, only `BoxBreathPhaseUpdate`. Skip
step 4's insert/delete variants if your entity has the same
shape. `target_id` is constrained to the four phase strings; a
constrained `target_id` needs its own arm in
`target_id_is_well_formed_for` (`meditate-core/src/db/error.rs`).
Seeding emits no events. `recompute_box_breath_phase` leaves the
seeded defaults alone until an update arrives.

Why a table rather than settings keys: each phase has five
properties (enabled, signal_mode, sound, pattern, volume), so
settings would need 20 keys. A table gets CHECK constraints on
`phase` and `signal_mode`, has the same shape as `interval_bells`,
and a new per-phase property is one column. The master switch
`boxbreath_cues_active` stays a setting.

`Setting` is the other update-only kind: `setting_changed`, the key
as `target_id`, no tombstone.

## Quick checklist

- [ ] uuid newtype + re-export
- [ ] CREATE TABLE, plus a `CACHE_SCHEMA_VERSION` bump
- [ ] Row struct, CRUD and `*_from_db` readers
- [ ] `EventKind` variants with `as_db_str`, `from_db_str` and
      `entity()`, a new `EntityKind`, and the round-trip test list
- [ ] `emit_event(&tx, …)` in each CRUD method
- [ ] `recompute_foo` and its `recompute_for` arm; `claim_name` and
      `reclaim_suffixed_names` if the name is unique
- [ ] `restore_foo` if the delete has an Undo
- [ ] **wipe_local_event_log DELETE FROM foos** ← easy to miss
- [ ] Seeds (if bundled)
- [ ] preset_config (if a preset can point at it)
- [ ] File sync (if Foo has audio files): pull, push and
      `remove_files_of_pulled_deletes`
- [ ] Catalogue entry in `EVENTS.md`

## Why this guide exists

The compiler forces `as_db_str`, `entity()` and `recompute_for`.
Nothing forces `from_db_str`, the round-trip test, the wipe,
`reclaim_suffixed_names`, the cache-version bump or the
`EVENTS.md` entry. This checklist catches those.
