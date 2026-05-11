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

## Tier 1 — Second-pass additions

### Typed `EventKind` enum for sync emit + dispatch
- `meditate-core/src/db.rs` has 29 bare string literals at
  `emit_event(..., "session_insert", ...)` sites + a 9-arm `match`
  in `apply_event_inner` reading them back. A typo on either side
  is a silent sync-data-loss bug.
- Add `pub(crate) enum EventKind { SessionInsert, SessionUpdate,
  SessionDelete, LabelInsert, LabelRename, LabelDelete, ... }` in
  `db/sync.rs` (the same `as_db_str` / `from_db_str` convention
  every other db-tier enum uses).
- `emit_event` takes `EventKind`; the dispatch becomes an
  exhaustive match. Compile-time guarantee that emit + apply agree.
- Highest-leverage single new item in the second pass.

### `existing_rowid_by_uuid(table, uuid)` helper
- Idempotent-insert prelude (`SELECT id FROM <t> WHERE uuid = ?1`)
  repeats verbatim 5 times: `insert_label_with_uuid:1884-1890`,
  `insert_bell_sound_with_uuid:2246-2252`,
  `insert_preset_with_uuid:2459-2465`,
  `insert_guided_file_with_uuid:2773-2779`,
  `insert_vibration_pattern_with_uuid:3007-3013`.
- Extract `fn existing_rowid_by_uuid(&self, table: &'static str,
  uuid: &str) -> Result<Option<i64>>`. Two-line caller replaces
  ~6 lines × 5 sites.

### `map_unique_err(e, dup)` helper
- `Err(rusqlite::Error::SqliteFailure(err, _)) if err.extended_code
  == SQLITE_CONSTRAINT_UNIQUE => Err(DbError::Duplicate*)` repeats
  at `db.rs:1852, 1905, 2494, 2655, 2796, 2865, 3034, 3181` — 8
  sites. Each has its own `DuplicateX` variant constructor.
- Extract `fn map_unique_err(e: rusqlite::Error, dup: impl
  FnOnce() -> DbError) -> DbError`.

### `read_kv(table, key, default)` for setting + sync_state
- `Database::get_setting` (725) and `Database::get_sync_state`
  (1757) are byte-identical apart from the table name.
- Collapse into `fn read_kv(&self, table: &'static str, key: &str,
  default: &str) -> Result<String>`. Both setters likewise collapse
  to `write_kv`.

### Date-arithmetic `.expect()`s → `DbError::DateOutOfRange`
- `db.rs:3761, 3959, 3972` — `succ_opt().expect("date overflow")` /
  `pred_opt().expect(...)` on `chrono::NaiveDate`.
- Real fault risk: `import_csv` accepts user-supplied
  `start_iso`; a row with `start_iso = "9999-12-31"` panics.
- Add `DbError::DateOutOfRange` variant; map the three sites to it.

### Split `sync/settings.rs` (29 pub items) → `settings.rs` + `credentials.rs`
- Module currently mixes three concerns: (a) key consts +
  account CRUD + sync-status writers, (b) `prepare_save` /
  `prepare_test` / `Credentials` / `TestConnectionResult` /
  `SaveSyncError` / `TestPrereq` / `StoredPassword` / `PasswordAction`
  validation state machines, (c) `test_connection_with<W: WebDav>`.
- Split into `sync/settings.rs` (keys + account + status) and
  `sync/credentials.rs` (prepare/test types + the validation
  state machine). The Tier-2 "collapse SaveSyncError / TestPrereq"
  item gets cleaner once the types live next to each other.

### Read-only `Database` methods → `*_from_db` free fns
- ~30 `pub fn`s on `Database` take `&Database`, return data,
  never mutate. They should be free functions in their domain
  modules following the established `*_from_db` convention.
- Labels: `count_labels`, `list_labels`, `find_label_by_name`,
  `is_label_name_taken`, `label_session_count` → `labels::*_from_db`.
- Presets: `count_presets`, `list_presets*`, `find_preset_by_uuid`,
  `is_preset_name_taken` → `presets::*_from_db` (new module).
- Guided: `list_guided_files`, `find_guided_file_by_uuid`,
  `is_guided_file_name_taken` → `guided::*_from_db`.
- Vibration: `list_vibration_patterns`,
  `find_vibration_pattern_by_uuid`, `is_vibration_pattern_name_taken`
  → `vibration::*_from_db`.
- Stats (~25 methods): `get_streak*`, `daily_totals*`,
  `month_total_secs`, `hour_buckets`, `active_months`,
  `active_days_in_month`, `total_*`, `get_median_duration_secs`,
  `get_running_average_secs`, `label_totals_seconds`,
  `count_sessions_by_label`, `total_minutes_by_label`,
  `get_longest_session` → `stats/*::*_from_db`.
- Aligns with the existing `stats/` consolidation in Tier 3.

## Tier 2 — Second-pass additions

### Move sync-tier types out of `db.rs`
- `Event` (lines 423-437) is used **only inside `db.rs`** — it's
  the sync wire envelope. Should follow into `db/sync.rs` when
  Tier 1 split lands.
- `SessionFilter` (line 441) is pure pagination state — belongs
  in `db/sessions.rs` next to `query_sessions`.
- `ChartKind` is consumed by `vibration` only — move there.
- `BoxBreathPhaseId` / `BoxBreathPhase` are consumed by `breath`
  and `session` — move to `breath.rs`.
- (Same logic as `SignalMode` already in backlog Tier 3.)

### `DbError::Csv` rename / split
- Used as a stringly-typed catch-all for **three** distinct
  things: CSV parse errors (3775, 3784), JSON parse errors in
  recompute helpers (1216, 1280, 1320, 1377, 1459, 1534, 1597,
  1651), and serde serialize errors (3015, 3167).
- Rename to `DbError::Decode(String)` (covers all three) or
  split into `Csv` + `Json` variants.

### Inconsistent corrupt-row handling
- Some sites `.unwrap_or(default)` (silent coercion of bad
  enum value), others `.expect()` (panic), others
  `DbError::Csv`. Pick one rule:
  - `.expect()` for CHECK-constrained columns (the constraint
    guarantees the variant exists).
  - `DbError::Decode(...)` for free-text columns (CSV import or
    hand-edited rows can hit a typo).
- Audit `from_db_str` call sites in `db.rs` and align.

### `Session` phase-variant payloads hoist
- `session.rs:535, 684` — `target_secs.expect("Overtime requires
  a target")`. State-machine invariant only enforced by code path.
- `session.rs:703` — `breath_pattern.expect("tick_box_breath
  called without breath_pattern")`. Same shape.
- Hoist into the phase-variant payload so the type system
  enforces presence:
  ```rust
  enum SessionPhase {
      Prep,
      Running,
      Overtime { target_secs: u32 },
      BoxBreath { pattern: BreathPattern },
      ...
  }
  ```
- Deletes 3 `.expect()`s; rules out a class of "wrong phase"
  bugs at compile time.

### Move `format::minutes_to_level` + `next_session_milestone` → `goal` / `stats/scoring`
- After Tier 1's prep-math redistribution, `format.rs` still
  leaks these two stats-domain functions. `minutes_to_level` is
  the contrib-heatmap level threshold; `next_session_milestone`
  is the milestone math.
- Move to `goal.rs` (or a new `stats/scoring.rs` if the `stats/`
  consolidation lands first).
- After this + the Tier 1 prep-math move, what's left of
  `format.rs` is genuine display formatters + the i18n typed-
  key enums. Rename to `display.rs` then.

### Rename `bells::sound_label` / `pattern_label` → `*_name`
- The functions return names; "label" is overloaded
  vocabulary in the crate (`Label` is its own row type).
- Rename to `sound_name` / `pattern_name`. Combine with the
  Tier 2 `Option<String>` change so both happen in one sweep.

## Tier 3 — Second-pass additions

### Missing `//!` doc comments
- `meditate-core/src/lib.rs`, `db.rs`, `format.rs`, `timer.rs`
  all lack a top-of-file `//!` doc. The four biggest /
  highest-traffic modules in the crate.
- Add one-paragraph `//!` to each so the rustdoc index is
  navigable.

### Move `src/bin/*_smoke.rs` to `examples/` or delete
- Five `bin/` smoke harnesses (`breath_smoke`, `record_smoke`,
  `smoke`, `sync_smoke`, `sync_pipeline_smoke`) are not
  referenced from CI / README / scripts. Cargo builds them
  on every workspace `cargo build`.
- Move to `examples/` (only built on `cargo run --example`) or
  delete. Net build-time win.

### Dead code audit
- `format::running_text` (line 570) — no callers anywhere in
  the workspace; tested but never rendered. Delete or document
  why it stays.
- `format::overtime` free fn (line 192) — no shell callers
  (shells use `overtime_button_label` which computes overtime
  internally).
- `timer::Countdown` — only used by the `bin/*_smoke.rs`
  harnesses. After Q lands, drop to `pub(crate)` or delete.
- `sync::fake::FakeWebDav` — `pub use`'d at `sync/mod.rs` for
  the smoke binaries; gate to `#[cfg(any(test, feature =
  "test-fakes"))]` after Q.

### `pub const` audit for `pub(crate)` downgrades
- Tunables that look internal:
  - `sync::backoff::MAX_BACKOFF_SECS` — used only inside its
    own module.
  - `vibration::EDITOR_INTENSITY_STEP` / `EDITOR_MIN_POINT_SPACING_MS`
    — UI editor consts; check shell callers, downgrade if none.
  - `breath::PHASE_MAX_SECS` / `MIN_CYCLE_SECS` — used by
    `clamp_session_secs` only.
- Wire-format consts stay `pub` (all `seeds::*`, `paths::*`,
  `sync::REMOTE_BASE_PATH`, `sync::settings::KEY_*`).

### `src/db/mod.rs:111` shell-side seeds re-export shim
- Shell crate `pub use`s `meditate_core::seeds::{BELLS_SEEDED_KEY,
  BUNDLED_BELL_UUID, ...}` to keep existing callers working
  through `crate::db::*` imports.
- Disappears once Tier 1 adds the crate-root `pub use seeds::*`
  (or once shell callers update to import from
  `meditate_core::seeds` directly).

### Three doc-tests on entry-point types
- The crate has **zero** runnable doc examples. Conspicuously
  missing on:
  - `Database::open_in_memory` — entry point of the entire
    crate.
  - `Session::start_running` — state-machine surface.
  - `PresetConfig::to_json` / `from_json` — wire-format
    contract.
- Add three doctests. Even three is a marked step up from zero
  and forces the example to stay in sync with the API.

### `assert_matches!` test macro
- ~15 `other => panic!("expected X, got {other:?}")` sites
  across `db.rs`, `preset_config.rs`, `vibration.rs`,
  `sync/webdav.rs`, `sync/settings.rs`, `sync/orchestrator.rs`,
  `bin/sync_pipeline_smoke.rs`.
- Add an `assert_matches!` mini-macro in a `test_macros` module
  so call sites become
  `assert_matches!(err, SyncError::WebDav(WebDavError::Unauthorized))`.

### `BreathPattern::new` validation audit
- `breath.rs:247` — `phase_at` panics on a zero-length cycle.
  Constructor-validated? If `BreathPattern::new` rejects all-
  zero patterns, this is dead code; otherwise it can fire.
- Audit `BreathPattern::new`; either confirm validation +
  add a comment at the panic site, or change `phase_at` to
  return `Option<PhaseInfo>` so the failure mode is type-
  visible.

### Schema-default UUIDs hardcoded as string literals
- `db.rs:513, 607, 608` — SQL `DEFAULT 'f0c2e8a1-…'` etc.
  Comments say "kept literal to avoid plumbing shell-side
  const into core schema string", but now that
  `crate::seeds::BUNDLED_*_UUID` lives inside core, the
  rationale is stale.
- Either `format!`-build the schema string at init with the
  seed consts inlined, OR drop the column DEFAULT entirely
  (writes always supply the value anyway).

### `seed_default_presets` body of literal `PresetConfig` data
- `db.rs:3447-3539` — 92 lines of `PresetConfig` literal
  constructors. Data, not behavior.
- Move the three `PresetConfig` constructors into
  `crate::seeds` next to the UUID consts. `seed_default_presets`
  shrinks to a loop over a static
  `&[(&str, &str, SessionMode, fn() -> PresetConfig)]`.

### `query_sessions` four near-identical branches
- `db.rs:4180-4248` — four `prepare_cached` branches each
  hardcode the same 8-column SELECT + ORDER BY; only the
  WHERE clause varies.
- Build the WHERE clause from the filter; rusqlite's
  `prepare_cached` already caches by generated SQL text. Cuts
  ~50 lines.

## Tier 4 — Second-pass additions

### Document Rust-tier referential-integrity contract
- Only `sessions.label_id` has a SQL `REFERENCES`. Every other
  cross-row reference (`sessions.guided_file_uuid`,
  `interval_bells.vibration_pattern_uuid`,
  `box_breath_phases.{sound_uuid, pattern_uuid}`) is enforced
  in Rust only.
- Adding FKs blindly would break sync replay races. Instead:
  - Document the Rust-tier enforcement contract on `Database`
    (top-of-file comment + per-method `// Caller must ensure`
    notes).
  - Consider an on-startup sweep that nulls dangling refs.

### `interval_bells.sound` legacy free-text column
- Column stores `"bowl"` / `"bell"` / `"gong"` (legacy enum)
  alongside the modern uuid-keyed sound resolution; two
  parallel plumbings exist.
- Either migrate the column to a uuid (one-shot SQL migration
  + `bell_sounds` lookup at apply-event time) or document the
  hybrid as an intentional aliasing layer.

### `Cargo.toml` optional features
- `ureq` and `roxmltree` are only used by `sync/webdav.rs`;
  `csv` is only used by `data_io.rs`; `mockito` is only used
  by sync tests.
- Could go behind `[features] sync` and `csv-io` so a future
  minimal-core consumer can skip ~MB of deps. Not urgent —
  there's only one binary today.

## Tier 0 — Correctness bugs (third pass, jump-the-queue)

### `presets.mode` SQL CHECK excludes `'guided'`
- `meditate-core/src/db.rs:546` — `CHECK (mode IN ('timer',
  'box_breath'))`. Per memory `feedback_meditate_guided_presets`,
  Guided IS a first-class preset mode in `preset_config::snapshot`
  / `preset_config.rs:236-244`. A peer running a build that emits
  a `preset_insert` event for a Guided preset crashes
  `recompute_preset` with a CHECK-constraint violation.
- Not caught by the planned `EventKind` enum (Tier 1): this is
  runtime data drift, not a wire-format mismatch.
- Fix: add `'guided'` to the CHECK list. Schema change; existing
  rows survive (none have `mode='guided'` today because the bug
  blocks them).

### `wipe_local_event_log` deletes an incoherent subset of tables
- `meditate-core/src/db.rs:1013-1024` wipes `events`, `sessions`,
  `labels`, `bell_sounds`, `interval_bells`, `known_remote_*` —
  but NOT `presets`, `guided_files`, `vibration_patterns`,
  `box_breath_phases`. All four are event-log-driven (their state
  is reconstructed via `recompute_*`).
- After a remote-data-lost → wipe-and-pull recovery, stale local
  rows in these four tables survive and overlap with peer-pulled
  data. Silently masks divergence.
- Fix: add the four missing DELETEs.
- High-impact bug on the recovery path.

### `push_custom_sound_files` lacks the 10 MB cap that pull enforces
- `meditate-core/src/sync/orchestrator.rs:419-450` reads the
  local file with no size gate and PUTs it. The puller (`:397`)
  drops oversized files only after consuming bandwidth.
- Cap is per-design symmetric; pull-only enforcement is silent
  asymmetry.
- Fix: `if bytes.len() as u64 > MAX_CUSTOM_BELL_BYTES { continue; }`
  after the `fs::read`.

## Tier 1 — Third-pass additions

### `sessions` table missing indexes on `start_iso` + `label_id`
- Every other entity table has UNIQUE-index coverage on its hot
  column. `sessions` reads run `ORDER BY start_iso DESC`
  (`query_sessions` four branches at `db.rs:4207/4218/4229/4240`)
  and `WHERE label_id = ?1` (`label_session_count:1799`,
  `list_sessions_for_label`).
- Under 10k rows still ms-budget today; gets worse at scale.
- Fix: `CREATE INDEX sessions_start_idx ON sessions(start_iso DESC)`
  and `CREATE INDEX sessions_label_idx ON sessions(label_id)
  WHERE label_id IS NOT NULL`.

### `events_synced_idx` should be a partial index
- `db.rs:648` indexes the full `synced` column (boolean).
  Steady-state ~99% of rows have `synced = 1`; the only scan is
  `WHERE synced = 0` (`pending_events`).
- Fix: `CREATE INDEX events_pending_idx ON events(synced) WHERE
  synced = 0`. Same lookup speed, one to two orders of magnitude
  smaller after first push.

### `breath::BreathSession` is an entirely dead struct
- `meditate-core/src/breath.rs:277-309` — `BreathSession::new`,
  `phase_info`, `pause`, `resume`. Zero callers in the workspace.
  Tested but never used.
- Bigger than backlog's existing `timer::Countdown` finding.
- Delete the whole struct + its tests.

### `timer.rs` is dead-by-transitive-closure
- The 263-line module's only non-test consumers are the
  `bin/*_smoke.rs` harnesses + `breath.rs`'s dead `BreathSession`
  (above). Once both move/delete, the whole module becomes a
  deletion candidate, not just `timer::Countdown`.
- Re-audit after Tier 3 smoke-bin cleanup + the BreathSession
  deletion above.

### `breath::BreathPattern::{four_seven_eight, from_durations, last_phase}` + `Phase::index` are dead
- Only tested, no callers. Delete or downgrade to `pub(crate)`.

### `bells::build_active_bells` should be `pub(crate)`
- `meditate-core/src/bells.rs:518` — only called by
  `bells::session_bells_from_db`. No shell callers.

### `mark_event_synced` (singular) has zero callers
- `meditate-core/src/db.rs:978`. Plural `mark_events_synced` is
  the live API. Delete the singular form.

## Tier 2 — Third-pass additions

### `preset_config::snapshot` (~96 lines) + `apply` (~184 lines)
- `preset_config.rs:267-362` (`snapshot`): three closure readers
  + four payload-builder blocks (label, starting/end/interval
  bells, box-breath phases, settings). The Tier 1 `read_*_from_db`
  consolidation already removes ~12 lines via the closure dedup;
  per-section block extraction (`snapshot_starting_bell(db)`,
  `snapshot_interval_bells(db)`) shrinks the body further.
- `preset_config.rs:378-562` (`apply`, ~184 lines) mixes three
  concerns: UUID validation (383-442), settings writes (444-492),
  phase rows + bell library replay (494-559). Split into
  `validate_referenced_uuids() -> Result<(), ApplyError>`,
  `write_settings(...)`, `replay_interval_bell_library(...)`,
  `write_box_breath_phases(...)`.

### `emit_event` takes no `&Transaction` reference
- `db.rs:1053-1066`. Implicitly relies on every caller having
  opened `unchecked_transaction` first. Currently honored across
  ~20 call sites by inspection; nothing prevents a future caller
  from skipping it.
- Fix: take `&Transaction` (or document the precondition in the
  doc-comment with an audit-trail rule).

### `WebDavError::MalformedResponse` doesn't capture the raw body
- `webdav.rs:54` + `:304` — variant carries only the parser's
  error string. A hand-debug of a broken Nextcloud response
  needs the raw body.
- Fix: extend to `MalformedResponse { detail: String,
  body_excerpt: String }`.

### Sync orchestrator-test `Sync::new` boilerplate
- ~60 sites in `orchestrator.rs::tests` repeat
  `Sync::new(&db, &fs, "Meditate", PathBuf::new())`. Crate-private
  API (`pending_events`, `replay_events`) prevents moving to
  cargo's `tests/common/`, but an in-`mod tests` helper works.
- Fix: `fn sync<'a>(db: &'a Database, fs: &'a FakeWebDav) ->
  Sync<'a>` inside the existing test mod. ~120 lines saved.

### `preset_config.rs` `Vec::contains(&String)` allocation per iter
- `preset_config.rs:400-405` and `:431-435` — `missing_sounds.
  contains(&u.to_string())` builds a fresh `String` per
  iteration. Cold path, small N — not a perf bug, but a clippy-
  friendly cleanup.
- Fix: `missing_sounds.iter().any(|m| m == *u)` or a `HashSet`
  pre-built.

## Tier 3 — Third-pass additions

### `Database::open` PRAGMA-ordering is load-bearing but undocumented
- `db.rs:718-720` does `PRAGMA foreign_keys=ON` BEFORE
  `execute_batch(SCHEMA)`. This is load-bearing: FK enforcement
  is per-connection — `PRAGMA` set AFTER the schema parse
  doesn't retroactively enforce existing rows.
- Add a one-line comment so a future refactor doesn't reorder it.

### `breath.rs` mixes method-on-enum vs free-fn for the same `Phase`
- `Phase::index()` is a method (24), but
  `phase_running_label_key(Phase)` (46) is a free fn. Both do a
  4-arm match on `Phase`.
- Pick one (method-on-Phase is the more idiomatic choice).

### `breath.rs` constants use three prefix conventions in one module
- `PHASE_MAX_SECS` (no prefix), `MIN_CYCLE_SECS` (`MIN_` prefix),
  `SESSION_MIN_SECS` / `SESSION_MAX_SECS` (`SESSION_` prefix).
- Standardise to suffix-ordering matching siblings:
  `CYCLE_MIN_SECS`, `PHASE_MAX_SECS`, `SESSION_MIN_SECS`,
  `SESSION_MAX_SECS`.

### Doc-comment gaps on named `pub` items
- ~12 items lack `///` despite well-documented surrounding types:
  - `bells::end_bell_row_state`, `bell_row_switch_state`,
    `interval_bells_count`.
  - `preset_config::PresetConfig::{to_json, from_json}`,
    `StarVisualState::from_is_starred`.
  - `vibration::PreviewToggle::{new, active_id, is_playing}`.
  - `breath::Phase::index`, `BreathPattern::{from_durations,
    duration_for}`, `BreathSession::*` (if not deleted).
  - `labels::delete_impact_key`.
- Mechanical add-docs sweep.

### `seeds.rs:16` cites closed `B.4.4 migration site` phase-marker
- Last stale phase-marker in the smaller modules. Replace
  "(B.4.4 migration site, etc.)" with "(the legacy-key
  compatibility layer)".

### `preset_config.rs:400-405, 430-435` — covered above in Tier 2.

### `Sync::new` test boilerplate — covered above in Tier 2.

## Tier 4 — Third-pass additions

### `TIMER_DEFAULT_SECS: u64` vs `BREATHING_DEFAULT_SECS: u32`
- `session.rs:96/101`. Pick one width or document why each
  module picks its own.

### `UiState::Default` derive is dead
- `session.rs:34` — `#[derive(Default)] enum UiState { #[default]
  Idle, ... }`. `UiState::default()` is never called in the
  workspace.
- Drop the derive (or document the intended use).

### Document the seconds-numeric-type convention at the crate root
- `u32` everywhere a single session duration is involved;
  `i64` for DB-aggregated totals (chrono uses `i64`); `u64`
  where `Duration::as_secs()` feeds the value.
- Defensible split, but worth a one-line `lib.rs` comment so a
  future contributor doesn't pick yet another width.

### Dep bumps when next touching the dep tree
- `ureq 2.12` → `3.x` (current is 3.x, released late 2024).
- `rusqlite 0.32` → `0.36+`. Each is a breaking API bump in
  rusqlite's versioning.
- No security advisory; no urgency. Worth bumping next time
  the dep tree is touched.

## Test-fixture sprawl — scope expansion

The existing Tier 3 `db/tests_common.rs` proposal extends beyond
`db.rs`:
- `bells::tests` has 5 fixture constructors (`interval_row`,
  `cue`, `row`, `fixed_bell`, `interval_bell`).
- `preset_config::tests::cfg_with_known_uuids` repeats the
  same row constructions.
- `data_io::tests::fresh_db`, `preset_config::tests::fresh_db`,
  `goal::tests`, `settings_keys::tests` all open
  `Database::open_in_memory().unwrap()` per test.
- `vibration::tests::pattern`, `sound::tests::sound`,
  `labels::tests::label` are one-liner row constructors that
  could share a fixture trait.

Expand scope: crate-wide `tests_common` module (feature-gated
`#[cfg(test)]`) with `db()`, `interval_bell_row()`,
`vibration_pattern()`, `bell_sound()`, `label()`, `ts(start_iso,
dur)`, `make_event(...)` helpers. Importable from every test
module.

## Skipped (intentionally not migrating)

(Empty for now — fill in as items get rejected.)
