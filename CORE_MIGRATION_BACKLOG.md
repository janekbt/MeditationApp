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

## Skipped (intentionally not migrating)

### `lookup_bell` walker in `src/bells.rs:763-769`
- 6-line `iter → find(|b| b.uuid == uuid)` over a `Vec`. Agent's
  read: no decision logic encoded; Android shell will write its own
  trivial walker against the same `list_interval_bells`. Not worth a
  helper.
