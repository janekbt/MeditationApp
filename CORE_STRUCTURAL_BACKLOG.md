# Core structural backlog

Items surfaced by the post-migration structural audit. The migration
itself is drained (see `CORE_MIGRATION_BACKLOG.md`); these are
follow-up cleanups to the *shape* of `meditate-core` now that lots
of logic has landed inside it.

When you implement one, delete it from this list. When you decide
to permanently skip one, move it under "## Skipped" with a one-line
rationale.

## Tier 1 — High-impact, mostly mechanical

### Split `db.rs` (11.5k lines) into a `db/` directory
- File has 28 existing `// ── … ─` section dividers that map to
  clean per-domain boundaries. Promoting them to siblings (one
  `impl Database` block per file across `db/{sessions, labels,
  presets, bell_sounds, vibration_patterns, guided_files,
  interval_bells, box_breath_phases, sync, stats, seed, settings,
  types, mod}.rs`) is mostly cut-and-paste — Rust allows multiple
  `impl Database` blocks across files, no trait gymnastics.
- Tests (~7300 lines, `#[cfg(test)] mod tests` from line 4288 to
  EOF) follow their impl into the new files.
- `recompute_*` family stays together in `db/sync.rs` next to
  `apply_event_inner` — they share the dispatch table.

### Collapse the 7-way `recompute_*` template
- `recompute_session`, `_label`, `_interval_bell`, `_preset`,
  `_guided_file`, `_vibration_pattern`, `_bell_sound` in
  `meditate-core/src/db.rs:1187-1755` all follow the same shape:
    1. `SELECT MAX(lamport_ts) FROM events WHERE target_id=? AND kind=<X>_delete`
    2. `SELECT lamport_ts, payload WHERE target_id=? AND kind IN (...)
       ORDER BY lamport_ts DESC, device_id DESC LIMIT 1`
    3. `row_should_exist` via match on (mutate, delete_ts)
    4. UPSERT or DELETE
- Extract `fn winning_mutate(target_id, mutate_kinds, delete_kind)
  -> Result<Option<serde_json::Value>>`; each `recompute_X`
  collapses to ~10 lines of "destructure JSON → UPSERT" (the
  column lists differ per entity, so the UPSERT half stays
  per-entity).
- Estimated ~400 lines of mechanical repetition removed.

### Visibility fix on `db.rs`'s sync surface
- ~10 `pub fn`s are the WebDAV engine's internal contract but
  currently callable from any shell code: `apply_event`,
  `apply_event_inner`, `replay_events`, `append_event*`,
  `pending_events`, `mark_event_synced(s)`,
  `flag_all_events_unsynced`, `wipe_local_event_log`,
  `known_event_uuids`, `known_remote_*`, `record_known_remote_*`,
  `wipe_known_remote_*`, `device_id`, `lamport_clock`,
  `bump_lamport_clock`, `observe_remote_lamport`,
  `get_sync_state`, `set_sync_state`.
- Downgrade to `pub(crate)`. Re-export the small surface the shell
  actually needs (`device_id`, maybe `wipe_local_event_log` for
  the recovery menu) via `crate::sync`.

### Standardize DB-reader naming
- Four conventions in use today for "load X with default":
  - `bells::*_from_db` (5 sites)
  - `labels::persisted_*_for_mode`
  - `goal::read_*`, `settings_keys::read_*`
  - `sync::settings::get_*`
- Pick `*_from_db` (pairs with the existing `*::from_db_str`
  parse functions). Rename:
  - `labels::persisted_active_for_mode` → `label_active_from_db`
  - `labels::persisted_uuid_for_mode` → `label_uuid_from_db`
  - `goal::read_weekly_goal_mins` → `weekly_goal_mins_from_db`
  - `settings_keys::read_bool` → `bool_from_db` (or stays
    `read_bool` if we treat the `settings_keys::*` prefix as the
    distinguisher)
  - `sync::settings::get_nextcloud_account` →
    `nextcloud_account_from_db`, etc.
- Mechanical; no signature change.

### Deduplicate `read_bool` / `read_str` / `read_signal_mode`
- `settings_keys::read_bool` is `pub`; `bells.rs:297-308`
  reimplements `read_str` / `read_bool` / `read_signal_mode` as
  module-private; `preset_config::snapshot` inlines a fourth set
  as closures. All three share signature `(db, key, default)`.
- Promote `settings_keys::{read_str, read_signal_mode, read_u32}`
  to `pub`. Delete the bells.rs + preset_config copies.

### Move scheduling math out of `format.rs` into `bells.rs`
- `bells` today imports `format::next_interval_ring_secs` +
  `format::fixed_from_*_target_secs` — a presentation→domain
  inversion (`format` is the render tier; `bells` is the domain).
- Move `next_interval_ring_secs`, `fixed_from_start_target_secs`,
  `fixed_from_end_target_secs` from `format.rs` to `bells.rs`.
- Also fold `PREP_SECS_DEFAULT` / `PREP_SECS_MIN` / `PREP_SECS_MAX`
  + `parse_prep_secs` + `prep_target_duration` + `prep_plan_from_db`
  from `format` into `bells` (they're bell-domain timing, not
  formatting).

### Add `pub use` at crate root
- `lib.rs` has zero re-exports today; callers write
  `meditate_core::db::SignalMode`, `meditate_core::db::Session`.
- Add:
  ```rust
  pub use db::{Database, Session, SessionMode, SignalMode,
               BellSound, Label, VibrationPattern, IntervalBell};
  pub use time::{boot_time_now, unix_now, today_local};
  ```

## Tier 2 — Medium impact, light design

### Split `session.rs` (2.2k lines) into `session/` directory
- Two existing `impl Session` blocks at lines 409 + 1010 mark a
  natural fault line.
- Proposed: `session/{mod, effect, settings}.rs`:
  - `mod.rs` — state machine, `Session` struct, tick / pause /
    resume / stop / overtime / start_prep / start_running.
  - `effect.rs` — `Effect` enum, `FireChannel`, `FireRoute`,
    `Effect::fire_route()`, `TickOutcome`, `ToggleAction`,
    `UiState`, `ui_state()`.
  - `settings.rs` — `SessionSettings` + `TIMER_DEFAULT_SECS` /
    `BREATHING_DEFAULT_SECS` consts.

### `bells::sound_label` / `pattern_label` / `resolve_sound_name` / `resolve_pattern_name` should return `Option<String>`
- Currently return `String` with `""` as the missing sentinel
  (`bells.rs:139/151/168/179`). The empty-string footgun forces
  shells to special-case `if name.is_empty() { gettext("Missing") }`
  per call site — exactly the i18n problem typed keys were meant
  to prevent.
- Change to `Option<String>` (or a `SoundName::{Resolved(String),
  Missing}` enum if shells want exhaustive match).

### Standardize `*Key` suffix for translatable enums
- `feedback_meditate_i18n_typed_keys` convention says `*Key`.
  Stragglers: `DateGroupKind` (`format.rs:612`), `BellsPart`
  (`format.rs:342`), `TimingPart` (`format.rs:358`). Same intent
  as `StreakKey` / `SyncedAgoKey` / `BellTitleKey` / ….
- Rename `DateGroupKind` → `DateGroupKey`, `BellsPart` →
  `BellsCountKey`, `TimingPart` → `TimingKey`.
- `XLabelKind` in `date_math.rs:129` is genuinely different (a
  layout decision, no translation) — leave it as `*Kind`.

### Collapse `SaveSyncError` / `TestPrereq` / `StoredPassword` into one shape
- `TestPrereq { EmptyUrl, EmptyUsername, NoPassword, KeyringFailed }`
  is a strict superset of `SaveSyncError { EmptyUrl, EmptyUsername }`,
  and `StoredPassword::{Missing, Failed}` is the same axis as
  `TestPrereq::{NoPassword, KeyringFailed}`.
- Rename `TestPrereq` → `SyncSettingsError`; `prepare_save`
  returns the same enum but only emits the two empty-input
  variants.

## Tier 3 — Judgment calls

### Move `SignalMode` out of `db.rs`
- Used by `bells`, `vibration`, `session`, `settings_keys`,
  `preset_config` — its placement in `db` is purely because the
  column reads it.
- Move to a new `signal.rs` (or into `bells.rs` since that's its
  primary consumer); `db` re-exports for the shell crates that
  still import via `db::SignalMode`.

### Extract `vibration::PreviewToggle` into generic `preview.rs`
- Currently named after its first consumer (vibration patterns).
  Sound chooser will want the same toggle / cancel / auto-revert
  state machine when Android sound preview lands.
- Move to `preview.rs`. Both `vibration` and `sound` `pub use`.

### Consolidate `labels` + `goal` + `contrib` under `stats/`
- All three are pure stats/score helpers (224, 210, 191 lines).
- Move under `stats/{labels, goal, contrib}.rs`. Re-export at
  crate root via `pub use stats::*` to avoid breaking shell
  imports of `meditate_core::labels::*` / `goal::*` / `contrib::*`.

### Merge `paths` and `rng` micro-modules
- `paths` (44 lines, four `&str` constants) — fold into `seeds`
  (the wire-format constant module) or `db` (the file-path
  consumer).
- `rng` (86 lines, one stateless xorshift64) — inline into
  `bells` (its sole intended consumer per its doc comment) or
  pair with `time::seed_now`.

### Shared test fixture for `db/` tests
- ~379 inline `Database::open_in_memory().unwrap()` calls across
  the test suite. Each test reconstructs the same fixtures by
  hand.
- Add `db/tests_common.rs` (a `pub(super)` helper module gated by
  `#[cfg(test)]`) with at minimum:
  ```rust
  pub(super) fn db() -> Database { Database::open_in_memory().unwrap() }
  pub(super) fn ts(start_iso: &str, dur_secs: u32) -> Session { … }
  pub(super) fn insert(db: &Database, start: &str, dur: u32,
                       label: Option<i64>) -> i64 { … }
  pub(super) fn make_event(kind: &str, target: &str, ts: i64,
                           payload: serde_json::Value) -> Event { … }
  ```
- Sheds ~3-5 lines per test × hundreds of tests.

### Per-entity CRUD tombstone helper
- `delete_interval_bell` / `delete_bell_sound` / `delete_preset` /
  `delete_guided_file` / `delete_vibration_pattern` (and the
  box-breath / session equivalents) follow the same "tombstone-
  if-exists, no-op otherwise" pattern: open tx → check exists →
  DELETE → emit tombstone event → commit. Repeated 7 times.
- Extract `fn emit_tombstone_if_exists(table, uuid, kind) ->
  Result<bool>` in `db/sync.rs`. Two-line callers replace ~10
  lines × 7 sites.

### Per-entity `to_event_payload(row) -> serde_json::Value` helpers
- `insert_*_with_uuid` and `update_*` build the same JSON payload
  twice — once for the local INSERT/UPDATE, once for the event
  payload. Drift between them is a silent peer-divergence bug
  (sync replay rehydrates differently than the local row).
- Add `fn to_event_payload(row: &<Entity>) -> serde_json::Value`
  per entity. The insert/update sites build the row first, then
  call the helper for the payload. Now any drift is a compile
  error.

## Tier 4 — Defer / discuss

### `Result<()>` on writes that callers always `let _ =`
- `labels::persist_active_for_mode`, `persist_uuid_for_mode`,
  `goal::write_weekly_goal_mins` — own doc says callers should
  `let _ =` since failure here only means the next launch shows
  the prior value. By contrast `sync::settings` writes correctly
  propagate (sync flow is transactional).
- Option A: keep `Result<()>` (honest), add a `log_on_err(ctx)`
  extension trait so call sites become `.log_on_err("persist
  label active")` rather than mute `let _ =`.
- Option B: add a sibling `*_silent(db, …)` variant that logs to
  `diag::log` and returns `()`.
- Subjective; pick one when next touching these helpers.

### `signal_mode_override_from_db` takes `key: &str` while sibling readers take `SessionMode`
- `bells::signal_mode_override_from_db(db, key)` requires the
  caller to resolve via `settings_keys::signal_mode_key_for_mode(
  mode)` first. Every other per-mode reader (`read_keep_screen_
  awake`, `persisted_active_for_mode`, `persisted_uuid_for_mode`)
  takes `SessionMode` directly.
- Change signature to `signal_mode_override_from_db(db, mode:
  SessionMode)`. Trivial; one call site in shell.

### `seed_default_presets` body of literal `PresetConfig` data
- `meditate-core/src/db.rs:3447-3539` — 92 lines of `PresetConfig`
  literal constructors. This is data, not behaviour.
- Move the three `PresetConfig` constructors into `crate::seeds`
  next to the UUID consts. `seed_default_presets` body shrinks
  to a loop over a `&[(&str, &str, SessionMode, fn() ->
  PresetConfig)]`.

### `query_sessions` four near-identical branches
- `meditate-core/src/db.rs:4180-4248` — four `prepare_cached`
  branches each hardcode the same 8-column SELECT + ORDER BY;
  only the WHERE clause varies.
- Build the WHERE clause from the filter; rusqlite's
  `prepare_cached` already deduplicates by generated SQL text.
  Cuts ~50 lines.

### Split `format.rs` further
- After Tier 1 redistribution (scheduling math + prep helpers
  move to `bells`), `format.rs` will still hold both number
  formatting and the translatable-key enums.
- Could split into `format/{duration, i18n_keys}.rs` later, but
  the dust needs to settle from Tier 1 first.

## Skipped (intentionally not migrating)

(Empty for now — fill in as items get rejected.)
