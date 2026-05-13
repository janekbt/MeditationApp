# Core structural backlog

Items surfaced by the post-migration structural audit. The migration
itself is drained (see `CORE_MIGRATION_BACKLOG.md`); these are
follow-up cleanups to the *shape* of `meditate-core` now that lots
of logic has landed inside it.

When you implement one, delete it from this list. When you decide
to permanently skip one, move it under "## Skipped" with a one-line
rationale.

## Up next (needs design discussion)

1. **Battery-death / OOM mid-session = whole session lost** — see
   Tier 1 eighth-pass. Needs design discussion: `session_in_progress`
   settings row written every ~60s + Resume/Save/Discard dialog on
   next launch.

## Tier 1 — High-impact, mostly mechanical

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
  `pending_events`, `mark_events_synced`,
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

### `preset_config.rs:400-405, 430-435` — covered above in Tier 2.

### `Sync::new` test boilerplate — covered above in Tier 2.

## Tier 4 — Third-pass additions

### `TIMER_DEFAULT_SECS: u64` vs `BREATHING_DEFAULT_SECS: u32`
- `session.rs:96/101`. Pick one width or document why each
  module picks its own.

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

## Tier 0 — Fourth-pass additions

### CSV import overflow in `parse_hms_duration`
- `meditate-core/src/format.rs:10, 16` — `m * 60 + sec.round() as
  u64` and `h * 3600 + m * 60 + …` use plain u64 multiplication,
  no checked/saturating. Pasted Insight Timer row with a large
  H:M:S value overflows in debug (panic on `import_csv`), wraps
  silently in release (`overflow-checks` off in `Cargo.toml`).
- Compounded by `meditate-core/src/data_io.rs:230`: `d.as_secs()
  as u32` truncates — a single legitimate ≥18.2h marathon import
  wraps to a tiny number.
- Fix: `m.checked_mul(60).and_then(|x| x.checked_add(...)).map(
  Duration::from_secs)`. Store duration as `u64`/`i64` in the
  import path.

### `csv::Writer` and pull-side `fs::write` aren't `sync_all`'d
- `meditate-core/src/data_io.rs:99-126` — `File::create → csv::
  Writer → flush → drop`. No `sync_all()`. Power-loss after
  `flush()` returns can leave a truncated or zero-byte CSV.
- `meditate-core/src/sync/orchestrator.rs:400` — pulled bell-sound
  files: `fs::write(&local, &bytes)` then `record_known_remote_
  sound`. Crash between write and DB commit leaves a zero-byte
  sound file marked as "known remote" — pull never retries it.
- Fix: wrap export in `File::create → BufWriter → csv → flush →
  into_inner → sync_all`. For the sound pull, `sync_all` before
  the DB record.

### `SyncCoordinator` drop-trigger race
- `meditate-core/src/sync/coordinator.rs:80-92`. Walkthrough:
  worker calls `should_run_again_after_pass() → false`. Concurrent
  thread calls `request()`: sets `re_trigger=true`, then
  `swap(true)` returns `true` (slot still taken) → `AlreadyRunning`.
  Worker calls `release()` → `in_flight=false`. `re_trigger=true`
  is now stranded with no worker observing it.
- The next user trigger picks it up; if auto-sync is off and no
  trigger arrives, the request is silently dropped.
- Also: the doc comment on `release()` claims it returns a `bool`
  but the signature returns `()`. Doc/signature drift.
- Fix: `release()` should CAS — clear `in_flight` only if
  `re_trigger` is false; otherwise return a "must loop again"
  signal to the worker.

### `streak_filtered` panics on `NaiveDate::MIN_DATE`
- `meditate-core/src/db.rs:3959, 3972` — `today.pred_opt().expect(
  "date underflow")` and `succ_opt`/`pred_opt` chains in the
  streak walk.
- Same fix family as the already-flagged `db.rs:3761`
  (`DbError::DateOutOfRange`); bundle in the same wave. Panic
  surface is the streak read-path, called from the Setup view on
  every open — not just the import path.

## Tier 1 — Fourth-pass additions

### Move ~40 core-behavior tests out of `src/db/mod.rs` into `meditate-core`
- `src/db/mod.rs:902-1604` — **700 lines of tests** that test
  *core semantics* through a one-line shell wrapper: streak
  math, daily-totals grouping by local date, seeding
  idempotency, label dedup, vibration-pattern duplicate
  handling, idempotency-on-reopen, median, running-average,
  label-totals.
- The vast majority would pass against a bare
  `meditate_core::db::Database` with no shell wrapper at all.
- Move to `meditate-core/src/db.rs::tests` (or per-domain test
  files after the Tier 1 `db/` split). The `test_db_in_memory()`
  fixture moves with them; drop `seed_bundled_bell_sounds` (gtk-
  specific GResource paths) in favor of `seed_all_non_audio`.
- Lines 907-992 (`session_data_to_core` round-trip tests) stay
  shell-side — they test the translation layer.

### Push four decision predicates to core
- `application.rs::trigger_sync` lines 391-407: the "is sync
  configured at all?" predicate. Add
  `meditate_core::sync::should_attempt(&Database) -> bool`.
- `timer/imp.rs::refresh_hero_for_idle` lines 1822-1840: the
  `match mode { Timer→countdown, Breathing→session_secs, Guided→
  pick.duration_secs }` dispatch feeds `format::idle_hero_label`.
  Add `meditate_core::format::idle_hero_label_from_modes(mode,
  countdown, breathing, guided_duration)` so the mode decision
  lives next to the formatter.
- `window/imp.rs::sync_indicator_state_now` lines 690-714:
  assembles a 4-tuple snapshot inline then feeds `indicator::
  derive`. Add `meditate_core::sync::indicator::snapshot(
  &Database) -> SyncIndicatorSnapshot` to dedupe the unwrap chain.
- `src/db/mod.rs::get_daily_totals` lines 822-831: filters by
  `since` after calling core. Push the filter into core
  (`get_daily_totals_since(since: NaiveDate)`); keep the
  stringification at the chart-axis rendering boundary.

### Crate-root `pub use` additions (counts from actual imports)
- The existing Tier 1 item lists the obvious cross-cutting types.
  Add these based on the actual shell import tally:
  - `meditate_core::diag::log` — **32 sites**, by far the most-
    imported symbol.
  - `meditate_core::settings_keys::{read_bool, format_bool}` —
    14 combined sites.
  - `meditate_core::naming::validate` — 6 sites across
    `labels.rs`, `sounds.rs`, `vibrations.rs`, `presets.rs`.
  - `meditate_core::sync::{WebDavError, WebDavResult, SyncError}`
    — 34 combined sites in `sync_runner.rs` alone. Already
    re-exported from `sync/mod.rs:26-27`, but not at the crate
    root.

## Tier 2 — Fourth-pass additions

### Eliminate `session_data_to_core` / `session_from_core`
- `src/db/mod.rs:164-191` — the only substantive translation in
  the shell DB wrapper. Reason: shell `Session.start_time: i64`
  (unix) vs core `start_iso: String`; shell field name `note` vs
  core `notes`.
- Fix path A: add `meditate_core::db::Session::from_unix(unix,
  dur, …)` and a `start_unix()` accessor.
- Fix path B: rename core's `notes` field to `note` + ship a
  unix-seconds constructor.
- Saves ~30 lines + tests. Android shell would need this exact
  translation; doing it twice is the smell.

### `src/db/mod.rs::map_core_err` synthesises fake sqlite errors
- Lines 197-233 — re-builds `rusqlite::Error::SqliteFailure(
  SQLITE_CONSTRAINT_UNIQUE, …)` per `DbError::Duplicate*` variant
  to keep the shell's `Result = rusqlite::Result` alias.
- Impedance-matching debt: core already has typed duplicate
  variants.
- Fix path A: drop the alias and use `DbError` directly through
  the shell.
- Fix path B: add `DbError::is_unique_violation() -> bool` so
  shells stop forging fake sqlite errors.

### Drop three `*_key_for_mode` shims in `timer/imp.rs`
- Lines 1598-1612 — three two-line shims that translate
  `TimerMode → SessionMode` and delegate to the matching core
  fn. `From<TimerMode> for SessionMode` already exists (line
  52-58).
- Fix: drop the shims; call sites take `SessionMode` directly.
  8 call sites in `timer/imp.rs`.

### `Database::update_interval_bell` 8-arg → take `&IntervalBell`
- `src/bells.rs:599, 611, 625, 656, 681, 704` — every call site
  directly mutates fields on a `snap.borrow_mut()` then calls
  `update_interval_bell(uuid, kind, minutes, jitter_pct, sound,
  pattern_uuid, signal_mode, enabled)`. The 8-arg signature is
  precisely what you'd write to side-step the struct.
- Fix: `Database::update_interval_bell(&IntervalBell)` — the row
  is the API; the 8-arg drift risk is gone.

### `get_streak` / `get_best_streak` shell-side `i32→u32` clamp
- `src/db/mod.rs:792-801` — core returns `i32` (chrono artefact),
  shell clamps to `u32` via `.clamp(0).as u32`.
- Fix: have core return `u32` directly. The `i32` is not a
  semantic choice.

### `get_running_average_secs` is stats math in a shell wrapper
- `src/db/mod.rs:810-817` — computes `today - days + 1` and
  divides. Pure statistics math, not GTK glue.
- Fix: move to `meditate_core::stats` (paired with the existing
  `goal::*`).

## Tier 4 — Fourth-pass additions

### Storage waste on push retry
- `meditate-core/src/sync/orchestrator.rs:258-277` — single bulk
  PUT per push; `batch_uuid` minted before PUT, recorded only
  after success. If PUT succeeds server-side but network drops
  before client ack, next push uploads the SAME events under a
  NEW `batch_uuid`. Peers' `known_event_uuids` dedup catches
  them, but the remote dir accumulates N copies on N retries.
- Not a correctness bug (convergence holds), but quota waste on
  flaky networks.
- Fix: hash the event-id-set into the batch_uuid so a retry with
  the same events reuses the filename and a second PUT either
  overwrites idempotently or 412s.

### Pull `record_known_remote_file` loop in N separate transactions
- `meditate-core/src/sync/orchestrator.rs:204-211`. `replay_events`
  runs in one tx; the subsequent record loop runs in N separate
  transactions. A crash between recording 3 and 5 means next
  pull re-GETs the 2 missing files. Event-uuid dedup keeps
  state correct — only bandwidth/IO wasted.

### Lamport-clock drift growth pattern
- `meditate-core/src/db.rs:1117-1119` — `apply_event_inner`
  calls `observe_remote_lamport` per new remote event during
  replay. `observe_remote_lamport = max(local, remote) + 1`, so
  the local clock advances by at least 1 per remote event.
- Two peers receiving the same N events in different orders end
  up with DIFFERENT local clocks. Cache state still converges
  (recompute uses event lamport, not local), but peers'
  future-authored events have widely varying lamports.
- Not a correctness bug — an unintended O(N) growth pattern.

### `recompute_*` tiebreaker is weaker than `replay_events`
- `meditate-core/src/db.rs:1201` and the parallel lines in each
  `recompute_*` use `ORDER BY lamport_ts DESC, device_id DESC
  LIMIT 1`. `replay_events` (`db.rs:1171-1175`) sorts by
  `(lamport_ts, device_id, event_uuid)` for full determinism.
- Identical `(lamport_ts, device_id)` is only reachable via
  corrupted wire-format input (local `bump_lamport_clock`
  guarantees uniqueness). Cosmetic; depends on malformed input.

### `Session::final_duration_secs: u64` → DB `duration_secs: i64`
- `meditate-core/src/session.rs:535, 552, 733` — stored as
  `Option<u64>` and written to DB as `i64`. Implicit u64→i64
  widening with `overflow-checks` off.
- Impossible to hit in practice (millions of years), but every
  other `secs` field in `Session` is `u32`. Pick one width or
  document why each module picks its own.

## Tier 0 — Fifth-pass additions

### `unix_to_local_iso` produces variable-length strings for years outside 0000-9999
- `meditate-core/src/time.rs:73` — chrono's `%Y` format pads
  positive years to 4 digits but uses a sign or extra digits for
  negative / 5+ digit years.
- Multiple SUBSTR-based extractions assume character-stable
  positions: hour at `db.rs:4025`, day at `db.rs:3878, 3901, 3929`.
  CSV import (`data_io.rs:131-183`) accepts a raw `i64`
  `start_time_unix` from user CSV with no range check.
- Round-tripping `i64::MIN`/`i64::MAX` through `unix_to_local_iso`
  produces a year like `-100000-01-01T00:00:00`, after which
  every stat SUBSTR is off by N.
- Fix path A: clamp `start_unix` in `import_csv` to
  `[0, 253_402_300_799]` (year 0–9999) before `unix_to_local_iso`.
- Fix path B: add a length-19 assertion + reject path on
  `local_iso_to_unix` input.

## Tier 1 — Fifth-pass additions

### `Database::import_sessions_csv` admits unvalidated `start_iso` strings
- `meditate-core/src/db.rs:3771-3814`. Unlike
  `meditate-core/src/data_io.rs::import_csv` (which goes through
  `unix_to_local_iso`), this method takes column 0 as
  `start_iso = record.get(0)?.to_string()` and inserts directly.
- A row with `start_iso = "garbage"` is stored;
  `daily_totals_filtered` silently filters it out via
  `parse_from_str(...).ok()` so the row vanishes from stats
  while still counting in `count_sessions` and
  `get_longest_session`.
- **Two methods exist for "import sessions from CSV" with
  different validation policies** — symptomatic of historical
  drift.
- Fix: reuse `data_io::import_csv`'s path or call
  `local_iso_to_unix(&start_iso)` and reject on `0` sentinel.
  Distinct from the existing Tier-2 "inconsistent corrupt-row
  handling" (which is read-side); this is import-side admitting
  bad data in the first place.

## Documentation backlog (Tier 1 — onboarding blocker)

The five passes confirmed the crate is internally well-commented at
the type/function level, but **the map is missing**. A non-Janek
contributor (or Janek six months from now, or a future Claude
session with cleared memory) cannot reconstruct design intent from
the code alone.

### No `meditate-core/README.md`; root README doesn't mention the crate
- A contributor entering via the subdirectory has zero entry
  point. Root `README.md` has zero `meditate-core` hits.
- Fill: a `meditate-core/README.md` with the same overview as the
  `lib.rs` `//!`, plus a "build / test from the workspace root"
  line. Add a paragraph in the root README pointing here.

### Stale `//!` on `session.rs:5`
- Comment claims "Currently implements the Prep phase only.
  Subsequent stages will extend...". Module is 2.2k lines and
  covers all phases. Leftover from B.5 work.
- Update or delete.

### Critical-path APIs lack `///` doc-comments
- `Database::open` / `Database::open_in_memory` (`db.rs:687-712`)
  — zero `///`. The 17-line internal comment explains WAL but
  not "are seeds run here?" (no, caller drives them), "is the
  DB ready for reads?" (yes), "thread-safe?" (no).
- `Sync::pull` (`orchestrator.rs:145`) and `Sync::push` (`:220`)
  — no caller-facing contract: what state mutates, what events
  emit, what errors are recoverable.
- `Session::start_running` — has a doc but doesn't describe the
  lifecycle from the caller's perspective (call `tick(now)`
  every second, observe `Effect`s, terminate on
  `TickOutcome::Done`).

### `ENTITIES.md` — how to add an 8th sync-able entity
- Adding one touches 9 surfaces: schema CHECK list, insert/
  update/delete CRUD, `existing_rowid_by_uuid` integration,
  `emit_event` kind, `apply_event_inner` dispatch arm,
  `recompute_*` writer, `wipe_local_event_log` DELETE (per
  Tier-0 bug), seed UUIDs in `seeds.rs`, payload struct +
  `to_event_payload` (per Tier-3 backlog item).
- Tribal knowledge today; the `EventKind` enum gives compile-time
  coverage of the emit/apply axes but doesn't replace the guide.
- Fill: an `ENTITIES.md` walkthrough or a `//!`-block at the top
  of `db/sync.rs` after the Tier-1 split.

### Naming-convention doc (post Tier-1)
- After the four-way DB-reader rename lands (existing Tier-1
  item), add a one-line `lib.rs` `//!` note locking the
  convention: "Read-only DB helpers are free functions named
  `*_from_db` in their domain module. Mutating helpers are
  methods on `Database`."

### `DECISIONS.md` cross-referencing memory rules
- 6+ binding design decisions live in
  `/home/janek/.claude/projects/-home-janek-Claude/memory/`:
  - `feedback_meditate_decisions_in_core`
  - `feedback_meditate_no_compat`
  - `feedback_meditate_keep_i18n`
  - `feedback_meditate_guided_presets`
  - `feedback_meditate_i18n_typed_keys`
  - `reference_clock_boottime`
- **None are surfaced in the codebase.** A non-Claude
  contributor wouldn't see them.
- Fill: a `DECISIONS.md` (or `lib.rs` `//!`-block) capturing
  each as a one-paragraph statement.

### `BUILDING.md` for cross-build + deploy
- `build-aux/dev-xbuild.sh` is mentioned in root README:144 (one
  paragraph), but `xbuild` quirks (lib-name suffix, NDK
  `llvm-readobj` on PATH — per `reference_xbuild_quirks`) and
  the Librem deploy cycle (wipe local DB after scp, wrap SSH in
  `timeout 8` — per `feedback_meditate_librem_deploy`) are
  tribal.
- Fill: extend root README or add `BUILDING.md`.

### `CONTRIBUTING.md`
- `git log --oneline -20` shows consistent `area: short
  imperative` style. Not documented anywhere.
- One-line `CONTRIBUTING.md` locks the convention for outside
  contributors / future Claude sessions.

## Meta-audit notes — backlog reorganization (apply when implementing)

Captured here, applied at implementation time rather than now to
avoid silently losing items in a rewrite.

### Re-tier
- **Workspace declaration**: currently Tier 1 → **Tier 0**.
  Foundation, not structural cleanup.
- **`emit_event` takes no `&Transaction`**: Tier 2 → **Tier 1**.
  Load-bearing invisible precondition.
- **`streak_filtered` panics on `NaiveDate::MIN_DATE`**: Tier 0
  → **Tier 2**. Bundle with the import-side fix (Tier 1
  `DbError::DateOutOfRange`); real surface is `import_csv`, not
  the streak path which only hits at year ±262144.
- **`breath::BreathSession` dead struct + `timer.rs` dead-by-
  transitive + `BreathPattern::four_seven_eight` etc.**: Tier 1
  → **Tier 3**. Hygiene, not high-impact structural.
- **`Vec::contains(&String)` allocation per iter**: Tier 2 →
  **inline / Tier 3**.

### Merge duplicates
- **`seed_default_presets` literal-data** appears twice (Tier 4
  + Tier 3). Dedupe.
- **`query_sessions` four near-identical branches** appears
  twice (Tier 4 + Tier 3). Dedupe.
- **Test-fixture sprawl** is fragmented into 3 items (Tier 3 db,
  Tier 2 sync-orch boilerplate, Tier 3 scope-expansion). Merge
  into one Tier-2 "crate-wide `tests_common` module."
- **`SaveSyncError`/`TestPrereq` collapse + `sync/settings.rs`
  split** are the same PR. Merge into one Tier-1 item.
- **`sound_label → sound_name` rename + `Option<String>` return**
  are explicitly the same sweep. Merge.
- **`*_from_db` free-fn migration + `stats/` consolidation +
  `get_running_average_secs` move** are the same direction. One
  PR.

## Tier 0 — None (sixth-pass)

## Tier 1 — Sixth-pass additions

### Performance: `recompute_*` uses `query_row` (uncached) — bulk-replay parse storm
- Every `recompute_X`'s two SQL reads use plain `query_row` which
  reparses SQL on every call. `replay_events` over 30k pulled
  events = 2×30k = 60k parse-and-plan cycles for ~14 distinct
  SQL texts. Dominates first-launch and wipe-and-pull recovery
  on Librem 5 eMMC.
- Affected: `meditate-core/src/db.rs` recompute family
  (lines 1187-1755).
- Fix: convert each query to `prepare_cached`, bump
  `set_prepared_statement_cache_capacity` to ~32 to fit them all
  alongside the existing cached statements.

### Performance: hot stats queries use `prepare` not `prepare_cached`
- `meditate-core/src/db.rs:3900` (`daily_totals_filtered`) and
  `:3928` (`distinct_session_days_ascending`). Called on every
  Insights / contrib heatmap / streak refresh; with ~10k
  sessions the SUBSTR + GROUP BY runs unindexed and the SQL
  gets reparsed each call.
- Fix: flip both to `prepare_cached`. Optionally add an expression
  index `CREATE INDEX sessions_day_idx ON sessions(SUBSTR(start_iso,
  1, 10))` for the GROUP BY (the existing Tier-1 `sessions_start_idx`
  helps ORDER BY DESC but not GROUP BY).

### Security: unbounded body read on WebDAV GET
- `meditate-core/src/sync/webdav.rs:182-184` calls
  `resp.into_reader().read_to_end(&mut body)` with no cap.
  `pull` then `serde_json::from_slice` on the result
  (`orchestrator.rs:192`). Custom-sound pull enforces
  `MAX_CUSTOM_BELL_BYTES` *after* the read.
- A malicious / compromised Nextcloud (or one a redirect points
  to) can OOM the client by serving a multi-GB body. The 10-MB
  size check on sound bodies is post-buffer.
- Fix: cap reads via `Read::take(N)` with caps per response type
  (64 MB for event bundles, 11 MB for custom sounds), reject on
  hit.

### Security: `ureq` Agent has no redirect or scheme policy
- `meditate-core/src/sync/webdav.rs:122-129` builds the Agent
  with only timeouts. Default = follow 5 redirects.
  `Authorization` header IS preserved on same-host redirects.
- ureq 2.x strips `Authorization` on cross-host redirects since
  2.4, mitigating most credential-exfil — but mixed-case host
  comparisons and IDN tricks have historical bypasses.
- Fix: `.redirects(0)` on the Agent builder; surface a redirect
  explicitly as a sync error so the user knows their server
  config changed.

### Security: `http://` URL silently accepted
- `meditate-core/src/sync/settings.rs:307-330` (`prepare_save`)
  trims + rejects empty. A typo'd `http://nc.example.com`
  sends the Basic-auth password in cleartext on every request.
- Fix: reject scheme ≠ `https://` in `prepare_save`, OR surface
  a save-time confirmation dialog so the user can opt in.

### i18n: `format_hm_compact` / `format_hm_mins` / `format_hm_secs` return English-baked strings
- All three (`format.rs`) return `"1h 4m"` / `"1h"` / `"4m"`
  with hardcoded English unit suffixes. Called directly into
  user-visible labels: `src/stats/imp.rs:170, 547` (weekly-goal
  subtitle, mini-stat tile, Y-axis ticks).
- Single biggest i18n leak in the codebase.
- Fix: convert to typed `HmKey::{Zero, MinsOnly(u64),
  HoursOnly(u64), HoursMins(u64,u64)}`; shell maps each variant
  to `gettext("{h}h {m}m")` etc. Same three call sites collapse
  to one match.

### i18n: 2-form plural ceiling + no `ngettext` anywhere in the shell
- Every "many" arm of every typed-key enum (`StreakKey::{One,
  Many}`, `SessionCountKey::{One, Many}`, `IntervalsCountKey`,
  `BellsPart`, `SyncedAgoKey`, `DeleteImpactKey`,
  `InsightKey::CurrentStreak{days}`) is binary singular /
  plural. The shell's `pluralize_sessions`
  (`src/preferences.rs:675`) openly comments "we don't need
  full ngettext support because English plurals are trivial."
- That ships a Polish / Russian / Arabic regression the moment
  a non-English `.po` is dropped in.
- Per `feedback_meditate_keep_i18n`, translations are a planned
  publish goal.
- Fix: core keeps its variant + count (already correct).
  Shell mapper calls `ngettext(singular_msgid, plural_msgid,
  count)` instead of `gettext().replace("{n}", count.to_string())`.

## Tier 2 — Sixth-pass additions

### Performance: `get_median_duration_secs` materialises every duration in Rust
- `meditate-core/src/db.rs:3856-3867`. Reads all 10k
  `duration_secs` rows via `query_map` then sorts in Rust.
  Allocates `Vec<u32>` of length n per stats refresh.
- Fix: two-step SQL — `SELECT COUNT(*)` then `SELECT
  duration_secs ORDER BY duration_secs LIMIT 1 OFFSET (n-1)/2`.
  Add an index on `duration_secs` (none today).

### Performance: `diag::log` opens the file per call
- `meditate-core/src/diag.rs:52-58`. Each call:
  `OpenOptions::new().create.append.open(path)` + `writeln!` +
  `format!` for the timestamp. 33 call sites; sync flows fire
  5-10 lines/pass. eMMC cost is hundreds of µs per call.
- Fix: keep an `OnceLock<Mutex<File>>` opened once at `init()`.
  Trade-off: loses "file recreated if user deleted it" —
  acceptable since the only legitimate deleter is `init()`
  itself.

### Security: secrets never zeroed
- `src/keychain.rs:124-176` `read_password` returns
  `Result<Option<String>>`; password lives in a plain `String`.
  `auth_header` (`webdav.rs:128`) retains base64-encoded creds
  for the agent's lifetime.
- Solo-user model so impact is low. Standard fix (`zeroize` /
  `secrecy` crate on the `String` / `Vec<u8>`) is cheap.

### Security: CSV-injection in export
- `meditate-core/src/data_io.rs:101-122`. A label or note
  starting with `=`, `+`, `-`, `@`, or `\t` becomes a formula
  when the CSV is opened in Excel / LibreOffice / Sheets.
- Janek-only today; if exported and shared, the risk
  materializes.
- Fix: OWASP-recommended prefix-with-single-quote on first-char
  match.

### Security: `meditate.db` umask perms (0644 by default)
- `meditate-core/src/db.rs:693-711` uses `Connection::open`
  with no perm step. On a non-flatpak Librem 5 install the DB
  lands 0644 — session contents (incl. notes) world-readable.
- Flatpak installs are private by app sandbox so the practical
  impact is bounded; non-flatpak is the exposed case.
- Fix: `OpenOptions::open` with mode 0600 on a touched file
  before `Connection::open`, or `chmod` post-open.

### Security: `diagnostics.log` umask perms
- `meditate-core/src/diag.rs:54` uses default `OpenOptions` →
  0644. Currently logs no secrets (every `diag::log` call site
  audited — clean), but the umask gap exists for future callers.
- Fix: belt-and-braces 0600 at `init()`.

### i18n: `format::overtime_button_label` bakes word order
- Takes a `prefix` and emits `"{prefix} MM:SS ?"`. German
  "Hinzufügen 00:30 ?" works; Japanese / Hungarian want
  "00:30 を追加 ?".
- Fix: return typed `OvertimeButtonParts { overtime: Duration }`;
  shell calls `gettext("Add {duration} ?")` with its own word
  order.

### i18n: `date_math::month_letter` returns hardcoded English ASCII
- Returns `"J"/"F"/"M"/…` single letters used by
  `src/stats/imp.rs:763` for the long-period X-axis. Doc says
  "locale-independent" but is really Anglocentric — Japanese
  months are "1月"-"12月", Russian uses Cyrillic.
- Fix: return `MonthLetterKey { month: u32 }`; shell calls
  `glib::DateTime::format("%b")` truncated to one char.

## Tier 3 — Sixth-pass additions

### Performance: `append_event_returning_newness` extra SELECT for rowid
- `meditate-core/src/db.rs:840-865`. After `INSERT OR IGNORE`,
  unconditionally runs `SELECT id FROM events WHERE event_uuid
  = ?1` — even on the new-row path where
  `conn.last_insert_rowid()` would answer for free.
- Fix: `if was_new { conn.last_insert_rowid() } else { SELECT id }`.
  Halves DB calls per event on bulk import.

### Performance: PRAGMA `cache_size` and `mmap_size` at SQLite defaults
- `meditate-core/src/db.rs:709-718`. Default `cache_size = -2000`
  (~2 MB). On Librem 5 with 3 GB RAM, bumping to `-16000` (~16
  MB) lets the events index stay resident.
- Fix: `PRAGMA cache_size = -16000` in `Database::open`. Defer
  `mmap_size` until benched.

### Performance: `pending_events` / `known_event_uuids` use `prepare` (uncached)
- `meditate-core/src/db.rs:872, 901`. Both run once per
  push/pull cycle. Easy `prepare_cached` flip; share their
  cache slot with no one else.

### i18n: number formatting in pre-rendered strings
- `format::format_count` does `n.to_string()` then
  `.replace("{n}", …)`. Loses locale digit grouping (Polish
  "1 234", German "1.234"). Invisible until totals cross 4
  digits.
- Fix path: wrap as `MiniStat::{Dash, Value(i64)}`; shell
  applies locale digit grouping.

### i18n: `format_time_of_day` is 24-hour-only
- Doc says shells that want 12-hour bypass core, but every
  consumer (log card, goal row) uses the function with no
  signal that the string was already finalised.
- Fix: change return to typed `TimeOfDay { unix_secs: i64 }`
  so shell can always run through `glib::DateTime::format` for
  the active locale.

### i18n: Polish / German label sort uses ASCII fold
- Labels do `ORDER BY name COLLATE NOCASE`. "ż" sorts past "z";
  "ä" sorts past "a" instead of with it.
- SQLite locale-aware collation requires an extension; defer
  until a user with non-ASCII label names actually complains.

## Tier 0 — Seventh-pass additions

### Seed-application logs nothing on first launch
- `db.rs:3547 seed_all_non_audio` + `seed_bundled_*` are
  silent. A user's first-ever launch shows `db open ok` then
  jumps to runtime events. If `seed_box_breath_phases` fails
  to create 4 rows, the bug report "Box-Breath uses wrong
  defaults" arrives with zero diagnostic context.
- Fix: one summary line per seed at the call site near
  `application.rs:104` — `seeded box_breath_phases: 4 rows`,
  etc.

## Tier 1 — Seventh-pass additions

### `sync::backoff` uses `Instant::now()` (wall-clock-frozen on suspend)
- `meditate-core/src/sync/backoff.rs:29,49,69,77` and the
  sleep in `sync/orchestrator.rs:466-468` use `Instant`
  (CLOCK_MONOTONIC, freezes during suspend).
- A 30-s server-asked backoff that straddles a 10-min
  suspend ends ~30 s after **resume**, not 30 s after the
  429. Inconsistent with the codebase's deliberate
  `boot_time_now()` discipline elsewhere.
- Fix: switch to `Duration` (boot-time); pass `now: Duration`
  to a `wait_for(now)` API. Drop `wait_until_now`/
  `note_429` convenience wrappers — require callers to pass
  a `boot_time_now()` value.

### Sync-indicator polling timer fires every 2s even when window unmapped
- `src/window/imp.rs:607-616` is the only periodic non-
  session timer in the whole shell. Battery-hostile on
  Librem 5 / Android in background.
- Fix: gate on `obj.is_mapped()` or replace with the half-
  built `notify::is_syncing` + `connect_map` refresh at
  `:598`. Removes it leaves the app fully event-driven when
  idle.

### `DbError::Sqlite`/`Csv` dead-end at user UI; `DuplicateLabel(name)` not surfaced
- `src/db/mod.rs:200,229` maps every `DbError::Sqlite` to a
  raw `rusqlite::Error` → generic toast. `DuplicateLabel`/
  `Preset`/`GuidedFile`/`VibrationPattern` carry the name
  but get folded back into a synthesized `SqliteFailure`
  with developer-language "UNIQUE constraint failed" message.
- Fix: map `DuplicateLabel(name)` directly to a translatable
  user toast at `src/db/mod.rs:197`, NOT to a fake rusqlite
  error.

### `SyncCoordinator::request` has zero multi-thread tests
- All 6 tests in `sync/coordinator.rs:108-173` are single-
  threaded — exercise the API, not the atomic ordering. The
  Tier-0 drop-trigger race already in backlog slipped past
  because nothing pumps two `request()`s from different
  threads.
- Fix: loom (or `std::thread::spawn` × N + barrier) test
  that 1000 races never lose a request.

### Panic-hook installation has no test verifying it writes
- `diag.rs:102 install_panic_hook` is wired; only test
  (`log_without_init_is_noop`) admits OnceLock prevents
  real testing. A future refactor could silently drop the
  hook and CI wouldn't notice.
- Fix: forked-process test (`std::process::Command`
  invoking the test binary with a `MEDITATE_PANIC_TEST=1`
  env switch, asserting post-mortem log content).

### Zero integration tests — `meditate-core/tests/` does not exist
- All 933 tests are `#[cfg(test)] mod tests` inline. Cross-
  module flows (create label → save preset → close DB →
  reopen → sync push → wipe local → pull → verify preset
  still references label by UUID) aren't covered. The 9-arm
  `apply_event_inner` dispatch is tested per-arm but never
  end-to-end across all entity types in one transaction.
- Fix: start `meditate-core/tests/sync_roundtrip.rs` with one
  such cross-cutting scenario; grow over time.

## Tier 2 — Seventh-pass additions

### Accessibility: `DurationSpeechKey` companion to the `format_*` family
- `format::format_time`/`format_hhmm`/`format_duration_brief`
  return display strings the shell hands to SR verbatim.
  Orca/TalkBack voice "twelve colon thirty-four" or "zero
  zero colon one zero" — not "ten minutes".
- For the idle hero (`idle_hero_label("00:10")` for a 10-
  min target) and Box-Breath counter, SR should say "ten
  minutes" / "ten minutes of five minutes".
- Fix: add typed companion `DurationSpeechKey::{HoursMinutes
  {h,m}, MinutesSeconds{m,s}, MinutesOnly{m}, Zero}` — same
  input as the formatter, emits speech intent. Shell maps
  each to `ngettext`. Fold with the open sixth-pass `HmKey`
  item — one PR.

### Accessibility: `SoundName::{Resolved,Missing}` enum
- The pending Tier-2 `sound_label`/`pattern_label` → `Option
  <String>` sweep is a fine start, but a typed `Missing`
  variant lets the shell announce the SR affordance ("Bell
  sound: missing, double-tap to re-pick") distinct from the
  visual empty-subtitle.
- Fix: roll into the existing `sound_label` migration —
  return `SoundName::{Resolved(String), Missing}` instead
  of bare `Option<String>`.

### Accessibility: typed `Announcement` / `ToastKey` enum for live regions
- `Effect`/`TickOutcome` lack a shell-portable surface for
  SR live-region intents (session saved, sync complete,
  undo toast, label saved, import done). Every shell composes
  its own toast string today; Android will diverge.
- Fix: typed `Announcement` enum (`SessionSaved`,
  `SessionDeleted{count}`, `SyncOk{secs_ago}`, `SyncFailed
  (SyncFailureKind)`, `ImportComplete{n}`, `UndoAvailable
  {kind}`). Bundle with the pending `SaveSyncError`/
  `TestPrereq` collapse.

### Accessibility: `MiniStatValue::{NoData,Value}` to keep dash glyphs out of SR
- `format::mini_stat_or_dash` returns `"–"`; SR reads "minus"
  or "dash".
- Fix: `MiniStatValue::{NoData, Value(i64)}`. Shell renders
  `set_visible(false)` on the value label and puts "no data
  this week" on the tile's accessible-name. One-line variant
  change at the call site.

### Accessibility: `contrib::grid_summary` + `ContribCell::speech_role`
- 91 cells × per-cell SR labels = a wall when Orca enters
  the heatmap. Add `ContribCell::speech_role: CellRole::
  {Future, Today, Past}` (shell already derives this from
  two bools) and `contrib::grid_summary(&cells) ->
  GridSummary { weeks_with_data, total_mins, best_day }` so
  the shell can put the summary on the grid container's
  accessible-description and `accessible-hidden=true` the
  individual cells (Orca skips them, Tab lands on summary).

### Performance: `BackoffState` switch to `Duration` (subsumed by Tier-1 sync::backoff fix)
- See Tier-1 sync::backoff item — same code change resolves
  both the suspend correctness gap and the inconsistency
  with the rest of the codebase's `boot_time_now` discipline.

### Operational: diag-log format not machine-parseable
- `diag.rs:57 writeln!(f, "{} {msg}", timestamp())` produces
  `2026-05-11 14:23:01 sync: …`. For an eventual Android
  Sentry/Bugsnag bridge or `meditate logs --json` CLI this
  requires regex parsing.
- Fix: keep human format but prefix a structured tag:
  `2026-05-11T14:23:01Z [sync.push] pulled=3 pushed=12`.
  Backwards-compat for current 33 call sites = single-day
  refactor.

### Operational: diag-log timestamps drift across suspend
- `meditate-core/src/diag.rs:74` uses wall-clock
  (`chrono::Local::now`). After a suspend crossing midnight
  or DST, adjacent log lines have wildly different
  timestamps with no visible "device was suspended" signal.
- Fix: append `[+Ns since prev]` boot-time delta, or emit
  one `boottime jumped from X to Y (suspend?)` heuristic
  line when consecutive boot-time deltas exceed 5 s.

### Operational: `SyncRunnerError` UX collapses 3 distinct conditions
- `application.rs:432` logs every `SyncRunnerError` variant
  with the same `sync: {e}` prefix — keychain failure, server
  500, transient network, `Unconfigured`, `PasswordMissing`,
  `RemoteDataLost`. Display impls differ
  (`sync_runner.rs:62-77`), but user-facing toast doesn't
  distinguish them.
- Fix: add `user_message()` to `SyncRunnerError` returning
  `(severity, msg_key)`; shell maps to toast level +
  translatable string per variant.

### Operational: no proptest / fuzz harness
- `Cargo.toml` lists no `proptest`/`quickcheck`. Replay
  convergence ("events in any random permutation must yield
  the same state") is the canonical proptest use-case — the
  existing `apply_event_*_replay_round_trip_across_peers`
  tests at `db.rs:10178,10201` hand-roll a fixed sequence.
  CSV import is the second screaming candidate.
- Fix: add `proptest` dev-dep; one property for sync replay
  invariance.

### Operational: recovery flow has no integration test from user POV
- `wipe_local_event_log` is unit-tested at `db.rs:7534`
  (and the Tier-0 subset-bug it ships with is in backlog).
  But the **recovery flow** (wipe → sync → pull → reach
  steady state) only exists in `recovery_dialog.rs` glue
  with no test.
- Fix: integration test driving the full sequence against
  a `FakeWebDav`.

### Operational: wall-clock test timing in `time.rs:114,138`
- Two tests sleep 10 ms / 1 s and assert `>= 5 ms` /
  monotonic. On a loaded CI runner the 5 ms assertion is
  flake-prone.
- Fix: inject a clock; `boot_time_now` is a unit-testable
  function that should take an optional `clock_source`.

## Tier 3 — Seventh-pass additions

### Accessibility: `breath::PhaseProgressKey { phase, remaining_secs }`
- Visual Box-Breath has the orbiting dot + countdown; SR
  hears only "Breathe in", "Hold", "Breathe out". A blind
  user can't anticipate phase transitions.
- Fix: add `PhaseProgressKey { phase: PhaseRunningLabelKey,
  remaining_secs: u32 }` so the shell can announce "Hold, 3
  seconds" on a slower cadence via a live region. Strictly
  additive; visual flow unchanged.

### Power: interval bells coalesce on resume (doc-only)
- `meditate-core/src/bells.rs:75-79`. Suspend across e.g.
  three 5-min interval bell targets fires only ONE ring
  post-resume, then rerolls from current elapsed. Likely
  desired (don't fire-storm 3 bells on wake), but the
  invariant isn't documented.
- Fix: doc-comment on `ActiveBell::tick`.

### Power: Box-Breath frame-clock has no 1Hz fallback
- `src/window/imp.rs:440` (`drawing_area.add_tick_callback`)
  drives both drawing AND `Session::tick` for cue dispatch
  via `tick_box_breath`. The 1 Hz timer at `:2481` covers
  timer-mode sessions; Box-Breath has no fallback if the
  compositor stops compositing while the window stays
  mapped.
- Real-world the device suspends anyway. Sev: low.
- Fix: doc-comment on `tick_box_breath`'s caller; consider
  a 1 Hz safety net only if it bites in practice.

### Power: verify screen-awake cookie survives suspend
- `src/timer/imp.rs:1054-1066` calls `gtk::Application::
  inhibit`. The cookie remains valid across suspend in
  theory; verify once on Librem 5 by adding a
  `diag::log("inhibit cookie=…")` on inhibit/uninhibit,
  manual suspend-resume cycle, drop the log line if
  confirmed.

### Operational: no `cargo test` / `clippy` in CI
- `.github/workflows/flatpak.yml` runs `appstreamcli
  validate`, `desktop-file-validate`, `flatpak-builder` —
  but never `cargo test --workspace` or `cargo clippy --
  -D warnings`. The "cargo test must pass before deploy"
  memory rule is enforced only by Janek's terminal.
- Add a `cargo-test` and `cargo-clippy` job to flatpak.yml.

### Operational: diag-log on-disk path documented nowhere
- `README.md:40` mentions the About → Troubleshooting view
  and `application.rs:181` has a toast hint. But the
  absolute on-disk path (`~/.var/app/io.github.janekbt.
  Meditate/data/meditate/diagnostics.log` on Flatpak) is
  documented nowhere — if the About dialog itself crashes,
  user has nowhere to look.
- Fix: one line in README listing the path on Flatpak and
  non-Flatpak.

## Tier 1 — Eighth-pass additions

### Stringly-typed UUIDs across 7 entity types — root cause of Tier-0 path traversal
- `meditate-core/src/db.rs:43, 114, 180, 327, 347, 399` +
  `Event.target_id:429`. `session_uuid`, `label_uuid`,
  `preset_uuid`, `bell_sound_uuid`, `interval_bell_uuid`,
  `vibration_pattern_uuid`, `guided_file_uuid`, `event_uuid`,
  `device_id`, `batch_uuid` — all `String`.
- Sync::push passes `target_id` (event-level uuid) to
  `record_known_remote_file` (file uuid); different
  domains, compiles silently. The Tier-0 path-traversal bug
  (`recompute_bell_sound` accepts `target_id =
  "../../../etc"`) is **directly enabled** by this — with
  `BellSoundUuid::try_new(&str) -> Result<...>` at the
  parse boundary, validation lives in the type, not in
  scattered call sites.
- Fix: single newtype-macro file `meditate-core/src/ids.rs`
  with `define_uuid!(SessionId); define_uuid!(LabelId);
  ...`. `String` storage, `Display + Deref<Target=str>`,
  `try_new` validates v4-shape. Mechanical rollout.

### 12 sites of `bool` parameters in public APIs
- `bells::end_bell_cue_from_db(db, stopwatch_on: bool)`,
  `bells::interval_bells_count(db, stopwatch_on)`,
  `bells::end_bell_row_state(db, stopwatch_on)`,
  `bells::is_bell_inert_in_stopwatch(kind, stopwatch_on)`,
  `format::idle_hero_label(stopwatch_on, target_secs)`,
  `format::prep_target_duration(prep_active, prep_secs)`,
  `bells::clamp_signal_mode_for_haptic(mode,
  haptic_available)`, `bells::channel_allowed(per_bell,
  per_mode)` returns `(bool, bool)`,
  `db::set_interval_bell_enabled(uuid, enabled)`,
  `db::update_preset_starred(uuid, is_starred)`,
  `db::set_guided_file_starred(uuid, is_starred)`,
  `db::is_label_name_taken(name, except_id)`.
- Call sites read `foo(true)` with no clue what the flag
  means.
- Fix: `enum DisplayMode { Countdown, Stopwatch }`,
  `enum HapticAvailability { Available, Absent }`, `enum
  StarredState { Starred, Unstarred }`. The `(bool, bool)`
  return from `channel_allowed` wants `Channels { sound:
  bool, vib: bool }`.

### `SessionSettings.target_secs: Option<u32>` collapses 3 distinct domain concepts
- `None` means "stopwatch session, no end" (Timer),
  "stopwatch box-breath" (BoxBreath), or "shouldn't
  happen" (Guided always has it). Three `.expect("Overtime
  requires a target")` panics at `session.rs:535, 684` are
  the symptom. The phase-payload-hoist already in backlog
  is the same root cause surfacing in `Session`; this is
  the same in `SessionSettings`.
- Fix: collapse `SessionSettings.mode + .target_secs +
  .breath_pattern + .stopwatch_display` into one
  `SessionShape` sum type: `Timer { target: SessionLength
  }, BoxBreath { pattern, target }, Guided { duration_secs
  }`. Eliminates four `.expect()`s and three "ignored
  otherwise" doc comments.

### Free-fn vs method inconsistency on the same enum
- `breath::Phase::index()` is a method (`breath.rs:24`);
  `breath::phase_running_label_key(Phase)` is a free fn
  (`:46`); `breath::perimeter_point(Phase, t, pad, side)`
  is free (`:62`). All three do a 4-arm match on `Phase`.
- Also: `session::Session::toggle_action(ui_state)` is
  associated fn but `session::ui_state(Option<&Session>)`
  is free — symmetric API, asymmetric placement.
- Also: `bells::ActiveBell` has methods AND a free
  `bells::session_bells_from_db` builder.
- Fix: pick "method-on-enum/struct when the receiver type
  is the primary subject; free fn otherwise" and audit.
  `Phase::running_label_key()`, `Phase::perimeter_point(t,
  pad, side)`, `BellCue::resolve_sound_name(library)`.

### `Audio device disappears mid-playback` — no error wiring
- `src/sound.rs`. `gtk::MediaFile::for_file(...)` returned
  objects never have `connect_error` or `notify::error`
  hooked. Headphones unplug mid-session = bell silently
  doesn't ring; next bell creates new MediaFiles with the
  same dead pipeline. Session can't tell the user "bells
  are no longer playing".
- Fix: `media.connect_error(|_, _| diag::log(...))` plus a
  one-shot toast.

### Battery-death / OOM mid-session = whole session lost
- Session is only persisted on `on_complete`
  (`timer/imp.rs:2380`). No "tick the duration into a
  session-in-progress table" hook. A 50-min user session
  interrupted at minute 49 by OS-kill has zero persisted
  state.
- Fix: write a `session_in_progress` settings row every
  ~60s with running duration; on startup if it's set,
  surface a "Resume / Save / Discard" dialog.

### No conditional PUT — concurrent push from two peers silently clobbers
- `orchestrator.rs:91-94` trait `put()` is unconditional;
  `HttpWebDav::put` (`webdav.rs:197-214`) emits no
  `If-None-Match` / `If-Match`.
- Bulk batch files use freshly-minted v4 uuids in the
  filename, so two concurrent pushes land on different
  files (safe). **But** the sounds path (`<base>/sounds/
  <uuid>.<ext>`) IS deterministic on the bell uuid; two
  peers re-uploading the same custom bell concurrently —
  the second wins blindly. Low probability; consequence is
  a corrupted audio file if the two PUTs race at TCP-byte
  level on a non-atomic server.
- Fix: add `If-None-Match: *` on sound PUTs.

## Tier 2 — Eighth-pass additions

### `DbError` over-broad on read-only fns
- `Result<i64, DbError>` where `DbError::DuplicatePreset`
  etc. are unreachable from `list_labels`, `get_streak`,
  `find_label_by_name`. Shell-side match-arms either
  swallow them or write unreachable code. Tier-2 backlog
  item `map_core_err` re-fabricates fake sqlite errors
  because of this width — type-narrowing fixes both.
- Fix: `enum ReadError { Sqlite, Decode }` for readers,
  keep wide `DbError` for writers.

### 5 more public functions with 5-8 args (sibling to existing Tier-2)
- `db::insert_interval_bell(kind, minutes, jitter_pct,
  sound, vibration_pattern_uuid, signal_mode)` 6 args,
  `db::insert_bell_sound_with_uuid(uuid_str, name,
  file_path, is_bundled, mime_type, category)` 6 args,
  `db::insert_vibration_pattern_with_uuid(uuid_str, name,
  duration_ms, intensities, chart_kind, is_bundled)` 6
  args, `db::update_vibration_pattern(uuid_str, name,
  duration_ms, intensities, chart_kind)` 5 args,
  `db::set_box_breath_phase(phase, enabled, signal_mode,
  sound_uuid, pattern_uuid)` 5 args.
- Same fix as the existing 8-arg item: take
  `&IntervalBell`, `&BellSound`, `&VibrationPattern`,
  `&BoxBreathPhase`.

### `PartialEq` includes `updated_iso` on entities
- `VibrationPattern.updated_iso`,
  `GuidedFile.updated_iso`, `Preset.updated_iso` — every
  `derive(PartialEq)` includes the timestamp. Tests like
  `assert_eq!(get_pattern(), expected_pattern)` pass today
  only because seeds use a known value. After the pending
  `to_event_payload` lands, the asymmetry between local-
  write and replay-write `updated_iso` becomes a silent
  test failure.
- Fix: manual `PartialEq` excluding `updated_iso`, OR a
  `same_logical_row(&self, other)` helper used in tests,
  OR document equality is timestamp-sensitive.

### Truncated GET body treated as fatal pull failure
- `webdav.rs:181-184` `read_to_end` returns whatever
  arrived; no Content-Length verification. A truncated
  transfer produces partial JSON that `serde_json::
  from_slice` rejects → `SyncError::InvalidEvent`. Same
  stuck-on-first-failure dynamic as the name-UNIQUE
  Tier-0 (file not recorded in `known_remote_files`,
  retried forever, fails forever).
- Fix: distinguish transient parse failure from
  structurally-invalid file; retry-with-backoff on the
  former.

### Two app instances racing — stats invalidation per-process
- `gtk::Application` is primary-instance on phosh, but on
  desktop two windows can open same DB. WAL allows it
  (per-conn `busy_timeout` already in backlog Tier-1),
  but `InvalidateScope` is per-process. Window B's writes
  don't dirty Window A's caches → A shows stale stats
  until manual nav.
- Per `feedback_meditate_solo_user_framing` this is
  doc-only. Add to `ARCHITECTURE.md`: "two simultaneous
  windows is not supported."

### `parse_batch_uuid_from_filename` accepts anything past `__`
- `orchestrator.rs:493-498` accepts ANY non-empty string
  after `__` as a `batch_uuid` and inserts it into
  `known_remote_files`. A peer naming a file
  `00000000000001__../../etc/passwd.json` ingests cleanly
  into the dedup tracker.
- Not a fs-write primitive (path is only a tracking key),
  but pollutes the known set with garbage. Pair with the
  Tier-0 path-traversal in `target_id` for the broader
  "trust the peer's strings" pattern.
- Fix: validate the post-`__` segment as a v4 uuid.

### `Lifetime 'a leaks` into pub `Sync` API
- `sync::orchestrator::Sync<'a, W: WebDav>` — the `'a`
  ties to `&'a Database` AND `&'a W`. Shell writes the
  lifetime out at the binding site.
- Question for the Android port: borrowed-`&` forbids
  ownership transfer to a worker thread → forces shell to
  manage Arc itself. Likely deliberate. Document, or
  switch to `Arc<Database>` + `Arc<W>` and drop the
  lifetime.
- `session::FireRoute<'a>` — borrowed `sound_uuid: &'a
  str` keeps the effect-pump loop short, but the Android
  shell sending these across a JNI boundary needs an
  `into_owned()` variant.

### Error context lost — no row/uuid in `DbError::Sqlite`
- `DbError::Sqlite(rusqlite::Error)` — a failed
  `update_label(id, name)` produces `DbError::Sqlite
  (SQLITE_BUSY)` with no record of *which* label id. Same
  for every update fn.
- `DataIoError::Io(io::Error)` drops the path.
  `import_csv(db, path)` failing with `Io(file not found)`
  doesn't say which file.
- Fix: `DbError::Sqlite { source: rusqlite::Error,
  context: &'static str }` (compile-time string keeps the
  variant `Copy`-ish). `DataIoError::Io { source, path:
  PathBuf }`.

## Tier 3 — Eighth-pass additions

### `device_id` collision undetected
- `db.rs:774` `Uuid::new_v4()`; no schema constraint on
  `events(lamport_ts, device_id)`. Astronomically unlikely.
- If it happened, `recompute_*` tiebreak `ORDER BY
  lamport_ts DESC, device_id DESC LIMIT 1` (`db.rs:1201`)
  picks non-deterministically — already in backlog as
  "weaker than replay_events tiebreaker."

### Naming-convention drift in stats family
- `total_minutes_by_label` vs `count_sessions_by_label`
  vs `month_total_secs` vs `hour_buckets` — same shape,
  four naming conventions.
- Fix: lock the rule once `stats/` split lands. Rule:
  `find_X_by_Y` returns `Option<Row>`; `list_X` returns
  `Vec<Row>`; `count_X` returns `i64`; `total_X_secs`
  returns aggregates; `is_X_<predicate>` returns `bool`.

### `list_*` ordering invariants undocumented
- `list_labels()` doc says "alphabetical" — good.
- `list_interval_bells()` reads `ORDER BY id` (insertion
  order) — undocumented at `db.rs:2145`. Users would
  expect kind-then-minutes ordering?
- `list_bell_sounds()` — `ORDER BY name COLLATE NOCASE`,
  undocumented.
- `list_presets_for_mode` / `list_starred_presets_for_mode`
  — doc says "ordered" but not by what.
- Fix: every `list_*` should have a one-line `/// Ordered
  by X.`

### Bundled-asset rollback breaks rows pointing at gresource
- A future release removing a bundled sound: row stays in
  DB (`BELLS_SEEDED_KEY` gates re-seed), `file_path`
  still points at missing gresource, `gtk::MediaFile::
  for_resource` silently fails.
- Per `feedback_meditate_no_compat`: future rollback
  should ship a `bell_sound_delete` event in the next
  migration, not silently break the row. Doc-only.

### Timezone change mid-session — stored start_iso vs displayed clock
- `time.rs` flagged for DST round-trip; the active-session
  piece is separate. `Session.start_iso` computed at
  session-start, `duration_secs` from a boot-time
  monotonic delta. If the user crosses timezones during
  the session, stored `start_iso` reflects pre-flight TZ
  but the user's clock shows post-flight TZ — log entry
  looks "wrong" by 6 hours. Not data loss; user-confusion
  only.

### `Default` derive: `BoxBreathCueConfig` undocumented semantics
- `bells.rs:260` derives `Default` constructing
  `master_enabled: false, in_phase: None, ...` — coherent
  ("cues off") but doc-comment doesn't say so.
- Fix: add `///` confirming the default represents "cues
  disabled."

### `f64` rng callback in `ActiveBell::tick`
- `ActiveBell::tick(elapsed_secs: u64, rng: &mut impl
  FnMut() -> f64)` — `f64` in `[0.0, 1.0]` is the API but
  nothing in the type says so. A caller's `|| 1.5`
  compiles.
- Fix: wrap in `struct Unit(f64)` with private
  constructor, OR document more loudly. Low priority —
  RNG callers are all in-crate.

## Skipped (intentionally not migrating)

(Empty for now — fill in as items get rejected.)
