# Core structural backlog

Items surfaced by the post-migration structural audit. The migration
itself is drained (see `CORE_MIGRATION_BACKLOG.md`); these are
follow-up cleanups to the *shape* of `meditate-core` now that lots
of logic has landed inside it.

When you implement one, delete it from this list. When you decide
to permanently skip one, move it under "## Skipped" with a one-line
rationale.

## Tier 1 — High-impact, mostly mechanical

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

### Remaining `bool` parameters in public APIs
High-leverage sites shipped (`DisplayMode`, `Channels`,
`StarredState`); the original twelve-bool sweep proposed three
more conversions that turned out marginal at the call-site level.
Re-evaluate each individually if a touching commit lands nearby:

- `bells::clamp_signal_mode_for_haptic(mode, haptic_available: bool)`
  — one consumer; an `enum HapticAvailability { Available, Absent }`
  buys little versus a verb-named param.
- `format::prep_target_duration(prep_active: bool, prep_secs: u32)`
  — entangled with the `SessionSettings.target_secs: Option<u32>`
  collapse below. Fold there.
- `db::set_interval_bell_enabled(uuid, enabled: bool)` — the
  method verb (`set_..._enabled`) already disambiguates the bool.
- `db::is_label_name_taken_from_db(name, except_id: i64)` — not
  actually a bool; the `0`-as-sentinel int is the footgun. Convert
  to `Option<i64>` or `Except::{Any, Exclude(i64)}` independently.

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

## Tier 2 — Medium impact, light design

### Consolidate `Phase` and `BoxBreathPhaseId` into one enum
- Both live in `meditate-core/src/breath.rs` after the domain-
  promotion pass, with identical four-variant shape (In / HoldIn
  / Out / HoldOut) but distinct roles: `Phase` is the in-cycle
  state from the breath state machine; `BoxBreathPhaseId` is the
  primary key of `box_breath_phases` rows. `session/mod.rs:738-741`
  carries the explicit conversion.
- Merging them removes the conversion ladder + the visually-
  confusing duplicate in `breath.rs`. The cost is that the
  resulting enum loses its tier-isolation marker (the column-
  identity role gets implicit). Net win because the variants
  are literally identical.
- Fix: rename the merged type to `Phase`, drop `BoxBreathPhaseId`,
  re-export from `meditate_core::db::BoxBreathPhaseId` as `Phase`
  for one release if any external callers care; ~98 call sites
  in core + shell rewrite via a mechanical `BoxBreathPhaseId` →
  `Phase` rename.

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

### Make `emit_event` take `&Transaction`
- `meditate-core/src/db/events.rs::emit_event` — the
  "must be inside an `unchecked_transaction`" precondition is
  doc'd but not enforced. Type-system enforcement would route the
  Lamport bump + event append + caller's data write through the
  same explicit transaction handle, making the partial-failure
  windows described in the doc impossible to introduce by
  accident.
- Tier 2 → Tier 1 per the meta-audit "load-bearing invisible
  precondition" note.
- Touches every call site of `emit_event` (~20) plus
  `bump_lamport_clock`, `append_event`, `device_id`,
  `mark_event_uuid_known`, etc. — all the per-write internals
  that currently borrow `self.conn` would take `&Transaction`
  instead. Not a one-pass change; do it as a focused PR.

### Sync orchestrator-test `Sync::new` boilerplate
- ~60 sites in `orchestrator.rs::tests` repeat
  `Sync::new(&db, &fs, "Meditate", PathBuf::new())`. Crate-private
  API (`pending_events`, `replay_events`) prevents moving to
  cargo's `tests/common/`, but an in-`mod tests` helper works.
- Fix: `fn sync<'a>(db: &'a Database, fs: &'a FakeWebDav) ->
  Sync<'a>` inside the existing test mod. ~120 lines saved.

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

### Performance: `get_median_duration_secs` materialises every duration in Rust
- `meditate-core/src/db.rs:3856-3867`. Reads all 10k
  `duration_secs` rows via `query_map` then sorts in Rust.
  Allocates `Vec<u32>` of length n per stats refresh.
- Fix: two-step SQL — `SELECT COUNT(*)` then `SELECT
  duration_secs ORDER BY duration_secs LIMIT 1 OFFSET (n-1)/2`.
  Add an index on `duration_secs` (none today).

### Security: secrets never zeroed
- `src/keychain.rs:124-176` `read_password` returns
  `Result<Option<String>>`; password lives in a plain `String`.
  `auth_header` (`webdav.rs:128`) retains base64-encoded creds
  for the agent's lifetime.
- Solo-user model so impact is low. Standard fix (`zeroize` /
  `secrecy` crate on the `String` / `Vec<u8>`) is cheap.

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

### Accessibility: typed `Announcement` / `ToastKey` enum for live regions
- `Effect`/`TickOutcome` lack a shell-portable surface for
  SR live-region intents (session saved, sync complete,
  undo toast, label saved, import done). Every shell composes
  its own toast string today; Android will diverge.
- Fix: typed `Announcement` enum (`SessionSaved`,
  `SessionDeleted{count}`, `SyncOk{secs_ago}`, `SyncFailed
  (SyncFailureKind)`, `ImportComplete{n}`, `UndoAvailable
  {kind}`). Mirrors the shipped `SyncSettingsError` pattern in
  `sync/credentials.rs` — typed key in core, shell renders.

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

## Tier 3 — Judgment calls

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

### `preset_config.rs:400-405, 430-435` — covered above in Tier 2.

### `Sync::new` test boilerplate — covered above in Tier 2.

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

### Split `format.rs` further
- After Tier 1 redistribution (scheduling math + prep helpers
  move to `bells`), `format.rs` will still hold both number
  formatting and the translatable-key enums.
- Could split into `format/{duration, i18n_keys}.rs` later, but
  the dust needs to settle from Tier 1 first.

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

### `Cargo.toml` optional features
- `ureq` and `roxmltree` are only used by `sync/webdav.rs`;
  `csv` is only used by `data_io.rs`; `mockito` is only used
  by sync tests.
- Could go behind `[features] sync` and `csv-io` so a future
  minimal-core consumer can skip ~MB of deps. Not urgent —
  there's only one binary today.

### Dep bumps when next touching the dep tree
- `ureq 2.12` → `3.x` (current is 3.x, released late 2024).
- `rusqlite 0.32` → `0.36+`. Each is a breaking API bump in
  rusqlite's versioning.
- No security advisory; no urgency. Worth bumping next time
  the dep tree is touched.

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
- **`Vec::contains(&String)` allocation per iter**: Tier 2 →
  **inline / Tier 3**.

### Merge duplicates
- **Test-fixture sprawl** is fragmented into 3 items (Tier 3 db,
  Tier 2 sync-orch boilerplate, Tier 3 scope-expansion). Merge
  into one Tier-2 "crate-wide `tests_common` module."
- **`*_from_db` free-fn migration + `stats/` consolidation +
  `get_running_average_secs` move** are the same direction. One
  PR.

## Skipped (intentionally not migrating)

(Empty for now — fill in as items get rejected.)
