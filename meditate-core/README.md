# meditate-core

Portable Rust logic core for the [Meditate](../README.md) app: the
GTK-free crate that owns persistence, sync, the session state machine
and every other concern that isn't UI-specific. Both apps consume it:
the GTK shell (`meditate-gtk/`, cargo package `meditate`) and the
Slint Android shell (`meditate-android/`), so behaviour stays the same
on both.

## Architecture in one paragraph

State is event-sourced into a single SQLite DB (`db/`). Every
mutation appends a row to `events` carrying a Lamport timestamp and
the authoring device's UUID; the `recompute_*` family materialises
the cache tables from the log. The sync layer (`sync/`) pushes and
pulls event batches through a WebDAV server, deduplicated by
`event_uuid`. The details (sync order, compaction, migrations, the
session engine) are in [`ARCHITECTURE.md`](../ARCHITECTURE.md).

## Build / test

Commands: see [`AGENTS.md`](../AGENTS.md). The core alone needs no
GTK packages:

```bash
cargo test -p meditate-core
cargo run -p meditate-core --example sync_smoke           # developer harnesses,
cargo run -p meditate-core --example sync_pipeline_smoke  # not run in CI
```

## Conventions

- **Decisions in core, mechanisms in the shell**
  ([`DECISIONS.md`](../DECISIONS.md) rule 1).
- **Typed keys for translatable text.** Helpers that produce
  user-visible text return a typed `*Key` enum; each shell maps every
  variant to its own translations (`DECISIONS.md` rule 2).
- **`_from_db` suffix** marks helpers that touch the DB, so call
  sites read like reads vs. computations.
- **`apply_event_inner` is forward-compatible.** Unknown event kinds
  are recorded but not applied; on the next cache-schema bump (see
  `CACHE_SCHEMA_VERSION`) all events are replayed so newly understood
  kinds materialise.
- **Seconds-numeric-type convention.** `u32` for a single session
  duration (always < 86_400); `i64` for DB-aggregated totals (chrono /
  SQLite both speak i64); `u64` where `Duration::as_secs()` feeds the
  value directly.
- **Typed entity UUIDs** (`db/uuids.rs`). `LabelUuid`,
  `BellSoundUuid`, `VibrationPatternUuid`, `PresetUuid`,
  `GuidedFileUuid`, `IntervalBellUuid`, `SessionUuid` are distinct
  newtypes so a `BellSound`'s identity can't be used where a `Label`'s
  is expected. Wire format is `#[serde(transparent)]`: bare strings on
  disk and on the wire.

## Module map

| Module | Role |
| --- | --- |
| `db` | SQLite cache, event log, apply/replay, `recompute_*` dispatch, migrations, the crash-safe running-session snapshot, the local-change watch. Owns `Database`. |
| `sync/` | WebDAV push/pull and compaction (`orchestrator`), the one sync pass both apps run (`runner`), account settings, credential checks, the sync-status indicator, the in-flight coordinator. |
| `session` | Session state machine (`Session`, `SessionSettings`, `Effect`). |
| `bells` | Interval / starting / end bell scheduling, signal-mode AND rule. |
| `bell_volume` | Per-bell volume levels, spaced evenly in decibels. |
| `breath` | Box-breath phases and perimeter math. |
| `vibration` | Vibration pattern math: envelopes, chunking, binarize. |
| `preview` | Single-slot preview state machine (toggle, supersede, auto-revert). |
| `audio_files` | Where a synced sound or guided file lives on this device. |
| `announcement` | Typed completion announcements (toast + screen reader). |
| `format` | Translatable typed keys + plain formatters (durations, counters, mini-stats). |
| `goal` | Daily-goal progress math + status partition. |
| `insights` | Derived stats (week-over-week, milestones). |
| `contrib` | Contribution-heatmap data model (13 weeks: 12 earlier plus the current one). |
| `preset_config` | JSON-encoded preset payload (Timer / BoxBreath / Guided). |
| `release_notes` | About-dialog release notes from the AppStream metainfo. |
| `time` | `boot_time_now` (suspend-resilient) + ISO-to-unix helpers. |
| `diag` | Ring-buffer log to `<data>/diagnostics.log`. |
| `data_io` | CSV import / export of sessions. |
| `seeds` | Stable bundled UUIDs, bundled vibration patterns, default presets. |
| `labels`, `naming`, `settings_keys`, `sound`, `timer`, `date_math` | Supporting utilities. |

For the canonical public API surface, see the crate-root re-exports
at the bottom of `src/lib.rs`. Anything findable at `meditate_core::*`
is "the contract"; anything deeper is implementation detail.

## Adding a new sync-able entity

See [`ENTITIES.md`](../ENTITIES.md) at the workspace root.

## Design-decision rationale

See [`DECISIONS.md`](../DECISIONS.md) at the workspace root.
