# meditate-core

Portable Rust logic core for the [Meditate](../README.md) app — the
GTK-free crate that owns persistence, sync, session state machines,
and every other concern that isn't UI-specific. The gtk-rs shell at
the workspace root consumes it; a future Slint / Android shell will
consume the same crate so behaviour stays consistent across surfaces.

## Architecture in one paragraph

State is event-sourced into a single SQLite DB (`db/`). Every
mutation appends a row to `events` carrying a Lamport timestamp and
the authoring device's UUID; the `recompute_*` family materialises
the cache tables from the log. The sync layer (`sync/`) bulk-PUTs
pending events to a WebDAV remote and pulls peer batches back, with
deduplication keyed by `event_uuid`. The cache tables are a pure
function of the event log, so a peer can drop them and rebuild from
the log at any time.

## Build / test

From the workspace root:

```bash
cargo build -p meditate-core           # build just the core crate
cargo test -p meditate-core            # 1000+ unit tests, all in-process
cargo test --workspace                 # full workspace
cargo run -p meditate-core --example smoke           # one-shot harnesses
cargo run -p meditate-core --example sync_pipeline_smoke
```

The crate has no GTK dependency — `cargo build -p meditate-core` runs
on stock CI without flatpak / libadwaita / gstreamer in scope. The
examples under `examples/` are ad-hoc developer harnesses (not wired
into CI).

## Conventions

- **Decisions in core, mechanisms in the shell.** The shell glues
  widgets together; behaviour ("when do we fire a bell, what does the
  streak read") lives here.
- **Typed keys for translatable copy.** Helpers that produce
  user-visible text return a typed `*Key` enum; the shell maps each
  variant to `gettext`.
- **`_from_db` suffix** marks helpers that touch the DB so call sites
  read like reads vs. computations.
- **`apply_event_inner` is forward-compatible.** Unknown event kinds
  are recorded but not dispatched; on the next cache-schema bump
  (see `CACHE_SCHEMA_VERSION`) all events are replayed so newly-
  understood kinds materialise.
- **Seconds-numeric-type convention.** `u32` for a single session
  duration (always < 86_400); `i64` for DB-aggregated totals (chrono /
  SQLite both speak i64); `u64` where `Duration::as_secs()` feeds the
  value directly.
- **Typed entity UUIDs.** `LabelUuid`, `BellSoundUuid`,
  `VibrationPatternUuid`, `PresetUuid`, `GuidedFileUuid`,
  `IntervalBellUuid`, `SessionUuid` are distinct newtypes so a
  `BellSound`'s identity can't be mistakenly used where a `Label`'s is
  expected. Wire format is `#[serde(transparent)]` — bare strings on
  disk and on the wire.

## Module map

| Module | Role |
| --- | --- |
| `db` | SQLite cache, event log, apply/replay, `recompute_*` dispatch. Owns `Database`. |
| `sync/` | WebDAV push/pull engine, settings, coordinator. |
| `session` | Session-mode state machine (`Session`, `Effect`, `TickOutcome`). |
| `bells` | Interval / starting / end bell scheduling. |
| `breath` | Box-breath phases and perimeter math. |
| `vibration` | Vibration pattern editor + envelope helpers. |
| `format` | Translatable typed keys + plain formatters (durations, counters, mini-stats). |
| `goal` | Weekly-goal snapshot logic. |
| `insights` | Derived stats (week-over-week, milestones). |
| `contrib` | Contribution-heatmap data model. |
| `preset_config` | JSON-encoded preset payload (Timer / BoxBreath / Guided). |
| `time` | `boot_time_now` (suspend-resilient) + ISO-to-unix helpers. |
| `diag` | Ring-buffer log to `<data>/diagnostics.log`. |
| `data_io` | CSV import / export of sessions. |
| `seeds` | Bundled vibration patterns + default presets. |
| `labels`, `naming`, `settings_keys`, `sound`, `timer`, `date_math` | Supporting utilities. |

For the canonical public API surface, see the crate-root re-exports
at the bottom of `src/lib.rs`. Anything findable at `meditate_core::*`
is "the contract"; anything deeper is implementation detail.

## Adding a new sync-able entity

See [`ENTITIES.md`](../ENTITIES.md) at the workspace root.

## Design-decision rationale

See [`DECISIONS.md`](../DECISIONS.md) at the workspace root.
