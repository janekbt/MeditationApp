# Adding a new sync-able entity

This walkthrough lists every code surface a new entity touches in
`meditate-core` and the order that lets you ship it without a
half-wired state. The shape mirrors the seven entity types already
in the workspace: `Session`, `Label`, `BellSound`, `IntervalBell`,
`Preset`, `GuidedFile`, `VibrationPattern`. `BoxBreathPhase` is a
special-case eighth entity (singleton-per-phase, update-only, no
insert/delete events) — see notes at the bottom.

If you've added a fully sync-able entity, you'll have touched **9
surfaces**. If you've added 8 (or 7, or 6) you've broken sync
silently in a way the type system won't catch. Use this as the
checklist.

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
forward-compatible (older builds skip them); bumping the version
makes the new build refuse to open older DBs unnecessarily. Only
bump for non-additive changes that older builds genuinely can't
read.

## 3. Module + row struct

Add `meditate-core/src/db/foos.rs` with:

- `pub struct Foo { id: i64, uuid: FooUuid, name: String, ... }`
- `pub fn insert_foo`, `update_foo(&Foo)`, `delete_foo(uuid: &str)`,
  `list_foos`, `find_foo_by_id`, `find_foo_by_uuid` (if needed)
- `pub(super) fn recompute_foo(foo_uuid: &str)` — pure function of
  the events table, used by `apply_event_inner` and
  `replay_events`. Pattern: query the winning mutate event via
  `winning_mutate`, then UPSERT the cache row by uuid.

Wire it up in `meditate-core/src/db/mod.rs`:

```rust
mod foos;
pub use foos::Foo;
```

CRUD methods on `Database` must:
- Open an `unchecked_transaction` before any mutation.
- Emit one event per mutation via `self.emit_event(...)` — see step 4.
- Be idempotent on uuid (re-inserting an existing uuid returns
  silently; deleting an unknown uuid silently no-ops AND emits
  no event).
- Surface `DbError::DuplicateFoo(name)` if the row has a UNIQUE
  collision on a user-visible name column.

## 4. EventKind variants

Three new variants in `meditate-core/src/db/events.rs::EventKind`:

```rust
FooInsert,
FooUpdate,
FooDelete,
```

Wire their `as_db_str` / `from_db_str` strings (use `foo_insert`,
`foo_update`, `foo_delete` — snake_case matches the rest of the
log). Add the three to `EntityKind::Foo`'s `entity()` mapping.

## 5. emit_event call sites

In each of the three CRUD methods (insert / update / delete),
build a JSON payload and call:

```rust
self.emit_event(EventKind::FooInsert, &foo_uuid, payload)?;
```

The payload schema is your choice — but every mutable field
should be present in `FooInsert` AND `FooUpdate` so the events
are self-sufficient (replay can materialise a row from a single
event without needing the insert event to have arrived first).

## 6. apply_event_inner dispatch arm

`apply_event_inner` reads `event.kind`, validates `target_id` via
`target_id_is_well_formed_for`, and dispatches to the right
`recompute_*`. Add an arm to `recompute_for(EntityKind::Foo, ...)`
calling `self.recompute_foo(target_id)`.

`target_id_is_well_formed_for` is the path-traversal validator —
it rejects `../etc` etc. before the recompute runs. Unless your
entity has a constrained target_id shape (like `BoxBreathPhase`'s
`in`/`holdin`/`out`/`holdout`), the default path-component
validator covers it.

## 7. wipe_local_event_log

`Database::wipe_local_event_log` is the recovery-flow primitive
that drops every event-sourced table so the next sync rebuilds
them from the remote. **Add a `DELETE FROM foos`** line — every
new entity must wipe alongside the seven that already do, or the
recovery flow leaves stale `foos` rows the next pull won't
overwrite.

This is the surface most often missed when adding an entity.
There's no compile-time check for it; the failure mode is "the
table looks fine until a user invokes recovery."

## 8. Seeds (if bundled rows exist)

If the entity ships bundled rows (like the four box-breath
phases, or the bundled bell sounds), add:

- UUID constants to `meditate-core/src/seeds.rs`.
- A `seed_default_foos` method on `Database` that's gated on a
  `FOOS_SEEDED_KEY` settings flag (so a deleted seed row stays
  deleted across reopens — without the gate, the seed resurrects
  the row and the resurrect event's higher Lamport ts overrides
  the user's delete on every synced peer).
- Add the call to `seed_all_non_audio()` if the seed payload is
  platform-agnostic. Audio files stay shell-side (the file paths
  are per-shell).

## 9. Preset / preset_config payload (if reachable from a preset)

If a `Foo` is referenced from a saved preset's `config_json`:

- Add a field to the relevant nested struct in
  `meditate-core/src/preset_config.rs`.
- Update `snapshot()` to read from the DB into the field.
- Update `apply()` to validate the UUID is locally present (or
  return `ApplyError::SyncPending`) and write the value back.

## BoxBreathPhase: the eighth-entity special case

`BoxBreathPhase` is a singleton-per-phase row (always exactly 4
rows, keyed on `phase IN ('in', 'holdin', 'out', 'holdout')`). It
has no insert / delete events — only `BoxBreathPhaseUpdate`. Skip
steps 4's insert/delete variants if your entity has the same
shape. `target_id` is constrained to the four phase strings;
`target_id_is_well_formed_for` knows about this enum.

## Quick checklist

- [ ] Typed UUID newtype + re-export
- [ ] Schema CREATE TABLE
- [ ] Row struct + CRUD methods
- [ ] EventKind insert / update / delete variants
- [ ] emit_event call sites in CRUD methods
- [ ] apply_event_inner dispatch arm + recompute_foo
- [ ] **wipe_local_event_log DELETE FROM foos** ← easy to miss
- [ ] Seeds (if bundled)
- [ ] preset_config payload (if preset-reachable)

## Why this guide exists

`EventKind` gives compile-time coverage of the emit/apply axes — a
new variant forces an arm in every match. But adding `EventKind::
FooInsert` doesn't force a `CREATE TABLE foos`, doesn't force a
`DELETE FROM foos` in wipe, doesn't force a seed gate. The compiler
catches the in-event-loop wiring; this checklist catches the
out-of-event-loop wiring.
