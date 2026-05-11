# Core migration backlog

Items surfaced by audit but not yet implemented. Listed here so the
next audit pass can flag them as "known" and not re-surface them.

When you implement one, delete it from this list.

When you decide to permanently skip one, move it under "## Skipped"
with a one-line rationale.

## High-leverage (real duplication or substantive logic)

### `sync_indicator_state`
- `src/window/imp.rs:641-716` (`refresh_sync_status`) + `:586-608`
  (`wire_sync_status`'s click handler) each re-derive the same
  `(account, last_ts, last_error, is_data_lost, is_syncing) → state`
  tuple, then dispatch on overlapping subsets (`has_error`,
  `is_data_lost`, `account.is_none()`).
- Migration: new `meditate_core::sync::indicator` module with
  - `SyncIndicatorState` enum: Hidden / Syncing / Error / DataLost /
    OkWithTs(ts) / OkNoTs
  - `derive(account, last_ts, last_error, is_data_lost, is_syncing) →
    SyncIndicatorState`
  - `action_for(state) → SyncIndicatorAction { OpenRecovery /
    RetrySync / OpenPrefsData }`
- Shell does one match → icon/class/tooltip-key; click handler does
  one match → action.

### `sync_prefs_save_prep` + `sync_prefs_test_prep`
- `src/preferences.rs:252-280` (Test button) and `:336-408` (Save
  button) both run the same "trim url + user, reject empty, handle
  empty-password fallback to keychain" decision tree, plus the
  documented save ordering rule (keychain → account → trigger_sync).
- Migration: extend `meditate_core::sync::settings` with
  - `prepare_save(url, user, typed_pw) → Result<SaveSyncPlan,
    SaveSyncError>` where `SaveSyncPlan { trimmed_url,
    trimmed_username, password_action: Keep | Store(String) }` and
    `SaveSyncError::{EmptyUrl, EmptyUsername}`
  - `prepare_test(url, user, typed_pw, stored_pw) → Result<
    Credentials, TestPrereq::{EmptyUrl, EmptyUsername, NoPassword,
    KeyringFailed}>`
- Shell maps the typed errors to gettext and drives the
  keychain/db/trigger_sync sequence.

### `breath::phase_running_label_key`
- `src/window/imp.rs:472-477` — `match phase { In/HoldIn/Out/HoldOut
  → English string }` inside the tick callback.
- Migration: `meditate_core::breath::phase_running_label_key(Phase)
  → PhaseRunningLabelKey` (variants: `BreatheIn`, `Hold`,
  `BreatheOut`). Shell exhaustively matches → gettext.

### `format::box_breath_counter_label`
- `src/window/imp.rs:268-272` and `:485-491` — same "if stopwatch →
  M:SS, else → elapsed/target" formatter inlined at initial paint
  AND every tick.
- Migration: `format::box_breath_counter_label(elapsed: Duration,
  target: Option<Duration>) → String`. Internally reuses
  `format_time`.

### Audio-extension allow-list (cross-cutting: guided + sounds)
- `src/guided.rs:89` (FileFilter) + `:380` (OGG-vs-transcode branch
  in `do_import_io`) and `src/sounds.rs:331-333` (FileFilter) all
  encode the same set of supported audio extensions.
- Migration: `meditate_core::sound::IMPORTABLE_EXTENSIONS:
  &[&str]` const + `is_passthrough_ext(ext: &str) → bool` predicate.
  Both file-picker filter builders iterate the const; the transcode
  branch calls the predicate.

## Medium

### `breath::perimeter_point`
- `src/window/imp.rs:370-375` — `(phase, t∈[0,1], pad, side) → (x,
  y)` is pure perimeter trigonometry consumed by cairo.
- Migration: `meditate_core::breath::perimeter_point(phase, t, pad,
  side) → (f64, f64)` with corner-position unit tests at t=0 and
  t=1 for each phase.

### Weekly-goal bounds in core
- `src/preferences.rs:83-94` — `30/1000/15/150` literals next to the
  SpinRow builder.
- Migration: add `WEEKLY_GOAL_MIN: i64 = 30`, `WEEKLY_GOAL_MAX: i64
  = 1000`, `WEEKLY_GOAL_STEP: i64 = 15`, `WEEKLY_GOAL_DEFAULT: i64 =
  150` to `meditate_core::goal` (already houses `compute`). Optional
  thin readers: `read_weekly_goal_mins(db) → i64`,
  `write_weekly_goal_mins(db, i64)`.

## Small

### `format::format_duration_brief` (or fold into `format_time`)
- `src/guided.rs:1109-1118` — `u32 secs → "m:ss" / "h:mm:ss"`.
- Migration: check whether existing `meditate_core::format::format_time`
  already produces the same output for the h:mm:ss / m:ss cases; if
  yes, delete `format_duration_brief` and have the two shell callers
  use `format_time`. If the format differs subtly, add a sibling
  helper to `meditate_core::format` and move the existing unit tests
  (`src/guided.rs:1336-1350`) with it.

### `format::ellipsize`
- `src/guided.rs:1098-1107` — unicode-safe truncate-to-N-chars with
  trailing `…`.
- Migration: `format::ellipsize(s: &str, max_chars: usize) → String`
  in `meditate_core::format`. Pure `str::chars()`. Test "héllo" /
  short pass-through / exact-length passthrough.

## Defer (re-evaluate when Android editor lands)

These were flagged by the vibration-editor audit but the agent
recommended deferring — they're real but small wins that only pay off
once the Android pattern editor is actually being written. Until then
they sit here as a heads-up for the eventual port.

- **Duration bounds constants** in `src/vibration_editor.rs:33-34`
  (`DURATION_MIN_S = 0.5`, `DURATION_MAX_S = 10.0`) → consts in
  `meditate_core::vibration` next to the existing editor consts.
- **`intensity_from_drag(start_intensity, drag_dy_px, chart_height_px)
  → f32`** — the drag-y → intensity snap-and-clamp math at
  `src/vibration_editor.rs:381-385`.
- **`pick_handle_index(intensities, chart_rect, click, hit_radius_px)
  → Option<usize>`** — the closest-handle hit-test loop at
  `src/vibration_editor.rs:333-366`, plus the `chart_rect` inset math
  at `:600-606` (and the `Y_LABEL_W`/`PAD`/`X_LABEL_H` consts).
- **`format_seconds(f64) → "{:.1}s"`** — x-axis label formatter at
  `src/vibration_editor.rs:750-752`. One-liner to
  `meditate_core::vibration` (or `format`).

## Cross-cutting / real-duplication wins (second pass)

### `naming::NameValidity` unified predicate
- 5+ call sites do the same `trim + empty? + collision?` shape:
  `src/labels.rs:255-261` (create), `:318-323` (rename);
  `src/presets.rs:442-457` (create), `:502-515` (rename);
  `src/vibrations.rs:600-614` (rename);
  `src/sounds.rs:455-475` (import), `:782-800` (rename);
  `src/vibration_editor.rs` save-button gate.
- Per-entity collision predicates already exist in core
  (`db::is_label_name_taken`, `is_preset_name_taken`, etc.); the
  validation shape itself is uniform.
- Migration: `meditate_core::naming::validate(trimmed, collision_fn)
  → NameValidity::{Empty | Collision | Ok}` taking an entity-typed
  closure for the collision check. Shell maps the variants to
  button-enable / live error chrome.

### `vibration::PreviewToggle` state machine
- `src/vibrations.rs:282-356` (`add_play_button`) and
  `src/vibration_editor.rs:531-593` both run the same toggle / cancel
  / replace logic — shared preview slot + per-row generation counter
  + auto-revert timeout invalidating via generation comparison.
- Migration: `meditate_core::vibration::PreviewToggle` with typed
  transitions returning `Action::{StopOnly | StartAndStop(uuid)}` +
  generation-counter API. Shell wires its `PatternPlayback` and
  `glib::timeout` to the action variants.

### `goal::read_weekly_goal_mins(db) → i64`
- `src/stats/imp.rs:162-167` (`reload_goal_ring`) and `:208-213`
  (`reload_contrib_grid`) both run the same
  `get_setting("weekly_goal_mins", "150") → parse → filter(>0) →
  fallback DEFAULT_WEEKLY_GOAL_MINS` chain.
- Migration: `meditate_core::goal::read_weekly_goal_mins(db) → i64`
  paired with `write_weekly_goal_mins(db, i64)`. Promotes the
  "Weekly-goal bounds in core" note above from optional to mandatory.

### `settings_keys::read_keep_screen_awake(db, mode) → bool`
- `src/timer/imp.rs:1039-1049` (`refresh_keep_screen_awake_state`)
  and `:1062-1068` (`acquire_screen_awake_lock`) both run the same
  per-mode read + parse_bool + default-false.
- Migration: thin reader next to existing
  `keep_screen_awake_key_for_mode` in `meditate_core::settings_keys`.

### `preset_config::mode_supports_presets(SessionMode) → bool`
- 5 ad-hoc `match mode { Guided → early return, _ → SessionMode }`
  blocks across `src/timer/imp.rs:449-456, 475-479, 2376-2380,
  3079-3085, 3381-3388` — every preset-related path early-returns
  for Guided.
- Migration: `meditate_core::preset_config::mode_supports_presets(
  SessionMode) → bool` (or `TimerMode::as_preset_mode() →
  Option<SessionMode>` on the shell side, but the predicate is
  portable).

## Substantive but isolated (second pass)

### Bundled-bell-sound seed orchestration
- `src/db/mod.rs:34-106` (`BUNDLED_BELL_SOUNDS` 11-row table) +
  `:292-310` (`seed_bundled_bell_sounds`) — the seed-once flag
  (`BELLS_SEEDED_KEY`), the per-row `insert_bell_sound_with_uuid(
  ..., General)` loop, the idempotency contract.
- The decision logic mirrors the other `seed_*` helpers already in
  core. Only the GResource paths are gtk-only.
- Migration: lift orchestration into
  `meditate_core::db::seed_bell_sounds_with_paths(rows: &[(uuid,
  name, path, mime)])` — pure SQL, gated by `BELLS_SEEDED_KEY`. Shell
  keeps its platform-specific row table; the loop + flag + event-log
  behavior live in core.

### `Session::tick_summary(now) → (TickOutcome, Vec<Effect>)`
- `src/timer/imp.rs:2573-2590` (`tick_running`) walks the effects
  vec and folds into `(session_display: Option<u64>, entered_
  overtime: bool)`. Pure inspection of a typed enum slice.
- Migration: `meditate_core::session::TickOutcome { display_secs:
  Option<u64>, entered_overtime: bool, overtime_delta:
  Option<Duration> }` + `Session::tick_summary(now) → (TickOutcome,
  Vec<Effect>)`. All three tick callsites (tick_prep, tick_running,
  tick_overtime ~:2693-2710) share the fold.

### `labels::DeleteImpactKey`
- `src/labels.rs:357-371` (`present_delete_label_dialog`) branches on
  `session_count > 0` to assemble two distinct delete-dialog bodies
  ("Sessions tagged… : N" vs "not used by any").
- Migration: `meditate_core::labels::DeleteImpactKey::{InUse(i64) |
  Unused}` typed enum; shell maps each to gettext template. Same
  exhaustive-match-checked pattern as the existing typed keys.

### `db::rename_vibration_pattern(uuid, new_name) → Result<bool>`
- `src/vibrations.rs:600-614` reads the pattern row, then rewrites
  with the same duration/intensities/chart_kind but new name. Also
  contains a `name == trimmed → no-op` guard.
- Migration: `meditate_core::db::rename_vibration_pattern` mirroring
  the existing `db::rename_bell_sound`. Returns `false` on no-op
  rename so callers don't need the guard.

### `db::insert_interval_bell_returning_uuid` (or
### `find_interval_bell_by_id`)
- `src/bells.rs:189-196` re-walks `list_interval_bells` after every
  insert just to recover the new row's uuid (the DB returns rowid,
  not uuid).
- Migration: either change `insert_interval_bell` to return the uuid
  directly, or add `find_interval_bell_by_id(rowid)` helper to core.

## Small constants / one-liner helpers (second pass)

### Default-secs constants
- `10 * 60` (Timer default) at `src/timer/imp.rs:316, 3483`; `5 * 60`
  (Breathing default) at `:322, 4151`. Each repeated twice.
- Migration: `meditate_core::session::{TIMER_DEFAULT_SECS,
  BREATHING_DEFAULT_SECS}` consts (or in `settings_keys`).

### `goal::daily_expected_mins(weekly_goal_mins) → i64`
- `src/stats/imp.rs:220` — `(goal_mins as f64 / 7.0).round().max(1.0)
  as i64`. One call site, but pairs naturally with `goal::compute`.
- Migration: add to `meditate_core::goal` alongside `compute`. Or
  fold the divisor into `contrib::build_grid` and have callers pass
  weekly goal directly.

### `sync::REMOTE_BASE_PATH = "Meditate"` const
- `src/sync_runner.rs:21` — string literal passed to `Sync::new` in
  both `run_sync_attempt` and `run_with_webdav`.
- Migration: `meditate_core::sync::REMOTE_BASE_PATH: &'static str`
  const. Shell `pub use`-s it.

### `date_math::parse_iso_date(s) → Option<NaiveDate>`
- `src/db/mod.rs:818-833` parses `"YYYY-MM-DD"` via
  `NaiveDate::parse_from_str` in two wrapper bodies, then calls core
  fns that already take `NaiveDate`.
- Migration: `meditate_core::date_math::parse_iso_date` (chrono is
  already a core dep). Shell wrapper maps `None` to its synthetic
  rusqlite error.

### `format::format_count(singular, plural_template, n) → String`
- `src/preferences.rs:652-658` (`pluralize_sessions`) and any future
  per-shell consumer of `SessionCountKey` re-implements the same
  template-substitution. Mechanical and generic.
- Migration: `meditate_core::format::format_count(singular: &str,
  plural_template: &str, n: usize) → String` that does the
  `session_count_key` match + `replace("{n}", …)`. Every shell
  drops its own version.

### `preset_config::StarVisualState`
- `src/presets.rs:328-348` (`build_star_button`) — three parallel
  `if is_starred { …_on } else { …_off }` blocks (icon name, css
  class, tooltip text).
- Migration: `meditate_core::preset_config::StarVisualState { icon:
  &'static str, css_class: &'static str, tooltip_key: TooltipKey }`
  returned by `star_visual_state(is_starred: bool)`. Shell maps to
  its widget.

### `bells::bell_row_switch_state(enabled, kind, stopwatch_on)`
- `src/bells.rs:232-240` composes `(enabled && !inert, !inert)` for
  the per-bell list row's switch state. Parallels the existing
  `bells::end_bell_row_state` shape.
- Migration: add as a sibling helper in `meditate_core::bells`.

### `vibration::PointsSubtitleKey { max: u32 }`
- `src/vibration_editor.rs:188-194, 412-416` — same gettext subtitle
  template "(up to N for this duration)" inlined twice; the `max`
  number is derived from `max_points_for_duration_s` (in core).
- Migration: typed key returned from core; shell renders gettext at
  the call sites. Tiny but kills the duplication.

### `meditate_core::paths` filesystem convention
- The shape `<data-root>/meditate/<subdir>/<filename>` recurs across
  `src/sync_runner.rs:181-183`, `src/sound.rs:131-134`,
  `src/guided.rs:376`, `src/keychain.rs:339-347`,
  `src/application.rs:102-104`. The data root is shell-specific
  (`glib::user_data_dir()` vs Android `getFilesDir()`); the subdir
  names (`sounds`, `guided`, `meditate.db`) are convention.
- Migration: new `meditate_core::paths` module exposing
  `pub const SOUNDS_SUBDIR: &str = "sounds"`,
  `pub const GUIDED_SUBDIR: &str = "guided"`,
  `pub const DB_FILENAME: &str = "meditate.db"`, etc.; shell
  composes against its native data-root path. Prevents drift between
  shells on filename convention.

## Skipped (intentionally not migrating)

### `lookup_bell` walker in `src/bells.rs:763-769`
- 6-line `iter → find(|b| b.uuid == uuid)` over a `Vec`. Agent's
  read: no decision logic encoded; Android shell will write its own
  trivial walker against the same `list_interval_bells`. Not worth a
  helper.

### `lookup_bell_sound_by_uuid` in `src/sound.rs:105-114`
- Same shape as `lookup_bell` above — `if uuid.is_empty() { None }
  else { list_bell_sounds().iter().find(...) }`. Skipped on the
  same rationale.

### Notification-target `is_focused` predicate in `src/timer/imp.rs:2641, 2817`
- `!app.active_window().map(|w| w.is_active()).unwrap_or(false)` —
  single-line GTK-bound expression; the decision content is just
  `!is_focused`. Not worth a helper.

### `has_filter` two-field disjunction in `src/log/imp.rs:144, 744-745`
- `self.filter_notes_only.get() || self.filter_label_id.get().is_some()`
  — two-Cell boolean OR. No decision content beyond the OR.

### Bundled-vs-custom suffix row affordances in `src/vibrations.rs:244-264` (and `src/sounds.rs:214`)
- `pattern.is_bundled ? [rename] : [edit, delete]`. One-line
  boolean fold; not worth a helper unless multiple shells render the
  same affordance set.

### `accent_color_rgba` unpack
- `src/window/imp.rs:336, 797` + `src/stats/imp.rs:652-657, 729-734`
  — `adw::StyleManager::default().accent_color_rgba()` + unpack to
  `(f64, f64, f64)`. The `StyleManager` half is gtk-bound; the
  unpack is three field accesses. Not worth a helper.

### Save-as-you-go clamp in bell editor
- `src/bells.rs:613-633` — `row.value().round().clamp(MIN as f64,
  MAX as f64) as u32`. The bounds constants live in core already;
  the round/clamp idiom is shell-language-specific.

### Import-form tri-state in `src/sounds.rs:455-475`
- `(import_btn_sensitive, collision_label_visible)` two-state
  visibility decision. Deferred — only worth migrating if the
  Android shell renders the same dual-state.
