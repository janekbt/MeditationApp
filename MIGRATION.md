# Migration: graduating `rewrite/` to replace top-level `src/`

## Strategy

Keep the app buildable at every commit. Migrate logic file-by-file to call into
`meditate-core` (the `rewrite/` crate). Once a file's logic is fully delegated,
delete the old internals.

## File mapping

| Old location | New replacement |
|---|---|
| `src/timer/imp.rs` (countdown / stopwatch / box-breath state) | `meditate_core::timer`, `meditate_core::breath` |
| `src/db/mod.rs` (Database, queries) | `meditate_core::db` |
| `src/data_io.rs` (CSV export/import) | `meditate_core::db::{export_sessions_csv, import_sessions_csv}` |
| `src/data_io.rs::parse_hms_duration` | `meditate_core::format::parse_hms_duration` |
| `src/data_io.rs::parse_insighttimer_datetime` | `meditate_core::format::parse_insighttimer_datetime` |
| `src/stats/imp.rs::minutes_to_level` | `meditate_core::format::minutes_to_level` |
| `src/stats/imp.rs::next_session_milestone` | `meditate_core::format::next_session_milestone` |
| `src/stats/imp.rs::format_hm_compact` / `_secs` / `_mins` | `meditate_core::format::format_hm_*` |
| `src/timer/imp.rs::format_time` | `meditate_core::format::format_time` |
| `src/window/`, `src/preferences.rs`, dialogs | **kept** — GTK shell, hand-tested |
| `src/main.rs`, `src/application.rs` | **kept** — entry points |
| `src/i18n.rs`, `src/diag.rs`, `src/sound.rs`, `src/vibration.rs` | **kept** — orthogonal concerns |

## Migration order

Smallest blast radius first. Each step is its own commit; tests stay green.

1. **Path dep wired**. Add `meditate-core = { path = "rewrite" }` to top-level
   `Cargo.toml`. App still builds, no behavioural change.
2. **Format/parse helpers**. Replace bodies in `src/stats/imp.rs` and
   `src/data_io.rs` parsers with calls into `meditate_core::format::*`. Pure
   functions, no side effects.
3. **DB layer**. Verify schema compatibility (column names, types, `mode`
   CHECK values). Swap `src/db/mod.rs::Database` to wrap or replace with
   `meditate_core::db::Database`. CSV export/import follows.
4. **Timer state machines**. Replace `src/timer/imp.rs` countdown/stopwatch
   logic with `meditate_core::timer::{Countdown, Stopwatch}` calls; the GTK
   widget owns the "now" clock and feeds elapsed into the core.
5. **Breath patterns**. Same shape as timer, using
   `meditate_core::breath::BreathSession`.
6. **Cleanup**. Delete unused helpers from old files; consolidate Cargo.toml
   deps (drop now-duplicate `rusqlite`/`csv`); regen `build-aux/cargo-sources.json`.

## Schema compatibility check (before step 3)

Compare per-column:

- `labels`: `id INTEGER PK AUTOINCREMENT`, `name TEXT NOT NULL UNIQUE`. Must match.
- `sessions`: `id`, `start_iso`, `duration_secs`, `label_id`, `notes`, `mode`.
  - `mode` CHECK: `meditate-core` uses `'countdown' | 'stopwatch' | 'box_breath'`.
    The existing app may use different strings — align before swap, possibly
    via a one-shot `UPDATE sessions SET mode = ...` migration.

If columns or types differ, write a migration step *before* swapping the DB
code, not as part of the swap.

## Android-readiness rules carry forward

The two rules from `ARCHITECTURE.md` continue to apply: shell layer reads the
clock (`Instant::now()`, `chrono::Utc::now()`) and feeds elapsed/wall-clock
times *into* the core. The core never reads a clock itself.
