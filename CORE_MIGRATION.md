# Core migration

Status: planning. Nothing migrated yet. Work paused on `android` branch
milestone 5+ until this list is fully drained — see `ANDROID_PORT.md`.

## Goal

Move logic that the meditate-core crate *should* have owned all along
out of `meditate-gtk` and into `meditate-core`, so the meditate-android
shell (and any future shell) doesn't have to duplicate, vendor, or
re-implement it. The GTK shell is supposed to be the *thinnest viable
view layer* on top of `meditate-core`; this list is the gap between
that intent and reality, surfaced during the Android port.

## Ground rules

- **Behaviour parity, not redesign.** Each migration is a pure
  refactor: same logic, same tests, different module address. If a
  test changes shape, document why in the commit body.
- **Don't add new public API along the way.** If you need a new
  helper to make the migration land cleanly, that's fine — but a
  redesign of the moved code is a separate task. Keep migrations
  boring.
- **Per-item commits.** One item per commit (or per small commit
  series), so a failure can be reverted in isolation.
- **Tests come along.** Wherever the logic moves, its tests move
  with it. `cargo test --workspace` must stay green at every step.
- **GTK shell still ships.** After every migration the host build
  works unchanged: `cargo build` at the workspace root, the Flatpak
  build via `build-aux/io.github.janekbt.Meditate.json`, and the
  Librem 5 cross-build via `build-aux/dev-xbuild.sh` all keep
  passing. On-device verification follows the existing
  `feedback_meditate_test_before_commit` rule.
- **No backwards compat.** Per the standing project rule
  (`feedback_meditate_no_compat`), don't add fallback shims. The new
  module address is *the* address; update every caller in one go.

## Quick wins (do first — pure moves, near-zero risk)

### [x] 1. `PresetConfig` + sibling structs → `meditate_core::preset_config`

**From:** `src/preset_config.rs` (285 lines).
**Pulls along:** the 10 pub structs/enums (`PresetConfig`,
`PresetTiming`, `PresetLabel`, `PresetStartingBell`,
`PresetIntervalBells`, `PresetIntervalBell`, `PresetEndBell`,
`PresetBoxBreathCues`, `PresetBoxBreathPhase`) and their tests.
**Why:** wire format for preset persistence — the schema is shared
between any shell that reads or writes the presets table. Today the
file imports only `serde::{Deserialize, Serialize}`; no GTK deps.
**Migration:** `git mv` the file to `meditate-core/src/preset_config.rs`,
add `pub mod preset_config;` to `meditate-core/src/lib.rs`, replace
imports in `src/{presets.rs, timer/imp.rs, db/mod.rs,
sync_runner.rs}` with `use meditate_core::preset_config::*`.
**Risk:** trivial.

### [x] 2. `diag.rs` ring-buffer log → `meditate_core::diag`

**From:** `src/diag.rs` (164 lines + tests).
**Pulls along:** `init`, `log`, `read_all`, the `MAX_LINES` cap, and
the panic hook.
**Why:** Android needs diagnostics too. The module has only
`std::{fs, io, path, sync}` deps — pure logic.
**Migration:** `git mv`, update imports in
`src/{main.rs, application.rs, db/mod.rs}` and any other
sites that call `crate::diag::*`.
**Risk:** trivial. The panic hook installs once at startup; verify
ordering still works after the move.

### [x] 3. `boot_time_now()` → `meditate_core::time::boot_time_now`

**From:** `src/timer/imp.rs` (~5 LOC private fn).
**Pulls along:** the libc dependency for the `clock_gettime` call.
**Why:** the existing `reference_clock_boottime` memory documents
that this is the suspend-resilient time source we want on both
targets. meditate-android currently uses `Instant::now()` which is
correct on Android (CLOCK_BOOTTIME since Rust 1.79) but wrong on
desktop dev builds. A shared helper is what the memory recommends.
**Migration:** new file `meditate-core/src/time.rs` with the function
moved verbatim, `pub mod time;` in `lib.rs`, replace the gtk private
helper with `use meditate_core::time::boot_time_now`.
**Risk:** trivial. `libc` is already a top-level dep of the GTK shell
crate (`Cargo.toml`); add it to `meditate-core/Cargo.toml`.

### [x] 4. `format_time(secs: u64)` consolidation

**From:** `src/timer/imp.rs:4616` (~10 LOC pub fn that
takes `u64` seconds).
**Conflict:** `meditate_core::format::format_time(d: Duration)`
already exists with the same logic but a `Duration` signature and
zero-padded hours (`{h:02}` vs `{h}`).
**Migration:** harmonise to one signature. Since core is the
canonical place, prefer `Duration` and leading-zero hours. Replace
gtk callers (`format_time(secs)`) with
`format_time(Duration::from_secs(secs))`. Delete the gtk version.
Verify no visible-string regression — the `{h}:` vs `{h:02}:`
difference is only visible for sessions ≥ 1h, and the GTK shell
displays MM:SS in most places anyway. **Surface to Janek if any
visible string actually changes.**
**Risk:** low — but the formatting-string regression is the catch.

## Medium effort (substantial logic + tests, bounded)

### [x] 5. Vibration envelope / chunk logic → `meditate_core::vibration`

**From:** `src/vibration.rs`, specifically:
- `quantise_amplitude(v: f32) -> f64`
- `rle_consecutive(...)`
- `build_master_envelope(p: &VibrationPattern) -> Vec<(f64, u32)>`
- `split_into_chunks(...)`
- `chunk_start_offset_ms(...)`
- `sample_line_at(...)` — linear interpolation of the intensity envelope at a time offset (lines ~448–456)
- `patterns_equivalent(a, b) -> bool` from `vibrations.rs` (lines 467–475) — structural equality with floating-point tolerance, used for undo-toast gating

plus the existing ~15 unit tests + tests for the new additions.

**Stays in gtk:** `probe_haptic`, `PatternPlayback`,
`should_fire_sound`, `fire_pattern_if_allowed` — all D-Bus /
feedbackd-bound.

**Why:** Android `Vibrator` / `VibratorManager` takes the same
`(amplitude, duration_ms)` shape. Reusing the math is what makes
milestone 8 of the Android port mostly transport-glue. The math
operates on `meditate_core::db::VibrationPattern` which is already
in core, so no schema-side conversion needed.

**Migration:** new module `meditate-core/src/vibration.rs` (or
`/vibration/mod.rs` if it grows), pull the 5 functions and their
tests, leave `vibration.rs` in gtk with just the transport.
**Risk:** medium. The tests carry the invariants; if any test fails
at the new address it's a real regression to chase.

### [x] 6. Breath module consolidation (option C)

**From:** `src/timer/breathing.rs` (192 LOC + ~25 tests)
↔ `meditate-core/src/breath.rs` (current slim API).
**Direction:** option C from the prior discussion — replace both
APIs with a unified `phase_at(elapsed) -> PhaseInfo { phase,
elapsed_in_phase, total, remaining }` on a single shared
`BreathPattern`. Phase enum collapses to one set of names.
**Stays in gtk:** the `Cell<BreathPattern>` field on the timer view
imp; the duration-spin-row read/write code; the settings-key
persistence.
**Why:** unblocks meditate-android Box Breath. Eliminates the
parallel-implementation maintenance burden.
**Migration touchpoints:** ~30 field-access sites in
`src/timer/imp.rs` (each reads `p.in_secs` / `p.hold_in`
/ etc.), 3 import lines elsewhere, plus the test rewrite in core.
**Risk:** medium. The 30 imp.rs sites are mechanical; the API design
of `PhaseInfo` is the only real call to make. Test the GTK box-breath
running page on the Librem 5 after landing.

## Larger / structural (do as the relevant Android milestone surfaces them)

### [x] 7. CSV import/export logic

**From:** `src/data_io.rs` (401 LOC, tests included).
**Move:** the parsing/writing logic that operates on `&Database` + a
path. **Stay:** the `MeditateApplication` glue, file-chooser plumbing.
**Depends on:** item 14. The CSV format stores `start_time_unix` as
i64; gtk's `SessionData::start_time` is i64; but `meditate_core::db::
Session::start_iso` is a `String`. Moving the import/export functions
to core needs unix↔ISO conversion at the boundary — that conversion is
item 14's deliverable (`unix_to_local_iso` / `local_iso_to_unix` from
`src/time.rs`). Land 14 first, then revisit. The Insight Timer
importer in particular also wants item 14 to drop its glib::DateTime
dependency.

### [x] 8. Bell-scheduling tick logic + `BellSchedule` struct

**From:** `src/timer/imp.rs` (~5028 LOC monster).
**Move:** the `BellSchedule` struct + its `next_due_at` / `tick`
logic. **Stay:** the `glib::timeout_add_local` glue that calls the
tick.
**Triggered by:** Android milestone 7 (audio bells) and milestone 8
(haptic patterns). Surgical lift required — current code reaches
into `MeditateApplication`. Plan for a `&meditate_core::db::Database`
parameter instead.

### [x] 9. Bundled UUID constants + bundled seed lists

**From:** top of `src/db/mod.rs`.
**Move:** the `BUNDLED_*_UUID`, `DEFAULT_*_LABEL_UUID`, and
`DEFAULT_*_PRESET_UUID` constants (pure strings). **Stay:** the
seed-list closures (each row carries a GResource path which is
GTK-only — split per row or strip the path field at the boundary).
**Triggered by:** any Android milestone that needs to write to the
bundled-bell or default-preset rows. Probably milestone 7.

### [x] 10. Sit-longer overtime math + add-button label generation

**From:** scattered in `src/timer/imp.rs` (the
`add_overtime_and_finish`, `Add MM:SS ?` button label, overshoot-detection
predicate).
**Move:** the math and label string. **Stay:** the gtk button widget.
**Triggered by:** an Android milestone that re-implements the
overshoot-then-add UX.

### [x] 11. Small pure helpers in `timer/imp.rs`

**From:**
- Per-mode setting-key dispatchers: `setting_key_for_mode`,
  `keep_screen_awake_key_for_mode`, `stopwatch_key_for_mode`,
  `label_active_setting_key`, `label_uuid_setting_key` — 5 free
  functions taking `TimerMode` + a knob name, returning the
  settings-table key string.
- `mode_default_label_uuid(mode)` (lines 4913–4918) — return the
  seeded default-label UUID per mode.
- `unix_now() -> i64` (line 4627) — `SystemTime::now()` to unix-epoch
  seconds.
- Subtitle formatters: `intervals_count_subtitle(usize)` (4548),
  `preset_subtitle(...)` (4561).

**Why:** Pure functions; same logic any future shell needs.
**Migration:** lift to `meditate_core::settings_keys` + extend
`meditate_core::time` (for `unix_now`) + `meditate_core::format`
(for the subtitle helpers). All micro-tests carry along.
**Risk:** trivial. Each is a self-contained move.

**Layout:**
- Per-mode key dispatchers live in `meditate_core::settings_keys`,
  keyed by `SessionMode`. Gtk gained a `From<TimerMode> for SessionMode`
  impl so existing call sites that pass `TimerMode` keep working
  through thin one-line wrappers.
- `unix_now()` lives in `meditate_core::time` next to `boot_time_now`.
- `intervals_count_subtitle` and `preset_subtitle` retrofitted to the
  typed-key pattern (see `feedback_meditate_i18n_typed_keys` memory):
  `meditate_core::format::intervals_count_key(n) -> IntervalsCountKey`
  and `meditate_core::format::preset_subtitle_parts(json) ->
  Option<PresetSubtitleParts>`. The gtk callers `match` on the
  variants and emit gettext-translated strings per branch — i18n
  preserved, structural decision lifted to core.

### [x] 12. Preset snapshot / apply walkers

**From:** `snapshot_current_setup` (3367) and `apply_config` (3518)
in `timer/imp.rs`. Each walks every per-mode setting + DB row to
produce a `PresetConfig`, and the reverse to apply one.
**Why:** the *walker* is portable once you abstract the settings
store (a `trait SettingsStore { fn get(key) -> Option<String>; fn
set(key, value); }` is enough). The Android shell needs the same
walker for preset persistence — duplicating the field-by-field walk
in two shells is a recipe for divergence.
**Depends on:** items 1 (`PresetConfig` in core) and 11 (per-mode
setting-key helpers in core).
**Migration:** introduce `meditate_core::preset_config::{snapshot,
apply}` taking `&dyn SettingsStore + &Database`. The GTK shell
wraps its existing settings access in a thin `SettingsStore` impl;
the walker logic moves over verbatim.
**Risk:** medium. The trait shape is the design call here — discuss
before coding.

### [x] 13. Session-tick decision logic

**From:** the pure-decision part of `tick_running` (2696),
`tick_prep` (2674), and `tick_overtime` (2838) in `timer/imp.rs`,
plus the predicates they call:
- `fire_due_bells_at` (4201) and the per-bell `should_fire` gates
  (`crate::vibration::should_fire_sound`, etc.).
- `breath_is_finished` (2996) — box-breath finish predicate.
- The phase-cue boundary detector (have we crossed from one breath
  phase into the next since last tick?).

**Why:** the *decision* of "given current state, what should happen
this tick?" is portable. The state inputs (elapsed, mode, bell
schedule, settings) are already in core or moving there via earlier
items. The output (a list of `Effect` enum values like `FireBell(uuid)`,
`FirePhaseCue(phase)`, `EndSession`, `RefreshDisplay`) gets dispatched
by the gtk shell to its widgets / D-Bus / audio.

**Why this is the big one:** today the tick functions are imperative,
mixing decisions and side effects. Lifting them out turns
`tick_running` into "compute effect list, dispatch effect list",
which is what milestone 7 + 8 of the Android port wants too — same
effect list, different dispatcher.

**Depends on:** items 6 (breath consolidation), 8 (BellSchedule),
9 (UUID constants), 11 (key helpers).
**Migration:** new `meditate_core::session::{Tick, Effect}` module.
TDD-heavy — every state-machine path gets a test.
**Risk:** higher. Not a one-shot move; staged commits per state
slice (prep tick first, then running tick, then overtime tick).
This is the migration that actually thins `timer/imp.rs` from ~5000
to ~2000 lines. **Substantial — surface design before coding.**

### [x] 14. `time.rs` portable timestamp conversions → `meditate_core::time`

**From:** `src/time.rs`, the `unix_to_local_iso(secs)` and
`local_iso_to_unix(iso)` functions (the entire portable surface of the
file). They use `chrono::Local` defensively (DST-ambiguous moments
return the epoch sentinel rather than panicking) — already pure Rust;
the `now_local() -> glib::DateTime` wrapper at the top is the only
glib-bound entry point and stays in gtk.
**Why:** Every session read/write crosses this boundary because
core stores ISO strings while gtk uses i64-unix internally for
ergonomic display + math. Android needs the same conversions and
must stay byte-equivalent with the GTK shell on round-trip.
**Migration:** new module `meditate-core/src/time.rs` (sibling to where
item 3's `boot_time_now` lands), moves these two fns + their tests.
Update gtk imports to `use meditate_core::time::{unix_to_local_iso,
local_iso_to_unix}`.
**Risk:** trivial — already chrono-only.

### [ ] 15. Sync-settings + connection-test helpers → `meditate_core::sync::settings`

**From:** `src/sync_settings.rs` (~588 lines) — the
portable surface, which is most of the file:
- `NextcloudAccount` struct + `get_nextcloud_account()` /
  `set_nextcloud_account()` (lines 36–80). The setter wipes dedup
  trackers + known-remote-sounds atomically when URL or username
  changes — that wipe-on-account-change invariant is currently only
  in the gtk wrapper, so any shell that re-implements an account
  setter risks subtly diverging.
- State recorders: `record_successful_sync()`, `record_sync_error()`,
  `record_remote_data_lost()`, `is_last_sync_remote_data_lost()`,
  `get_last_sync_error()`. Enforce the "don't clobber the last-success
  timestamp on failure" invariant.
- Recovery preparers: `prepare_push_local_recovery()`,
  `prepare_wipe_local_recovery()` — safe compositions of core's
  primitives (`flag_all_events_unsynced`, `wipe_known_remote_files`,
  `wipe_local_event_log`).
- From `src/sync_runner.rs`: the `TestConnectionResult`
  enum + the generic `test_connection_with<W: WebDav>(creds, &W)` fn
  (lines ~235–302). The `WebDav` trait is already in core, so the
  test-connection fn slots in cleanly.

**Why:** None of these touches `gtk::`, `adw::`, `glib::`, or `gio::`.
Android's sync path will need every one of them. Centralising avoids
silent drift on the wipe-on-account-change and last-success invariants.
**Stays in gtk:** `SyncRunnerError` (the variant for `PasswordMissing`
is keychain-tied), the `MeditateApplication` glue, the sync-settings
preferences-page widgets.
**Migration:** new sub-module `meditate-core/src/sync/settings.rs`,
move the struct + 6 helper fns + the test-connection generic + their
tests. The gtk shell's `sync_settings.rs` shrinks to UI glue.
**Risk:** medium — the wipe-on-account-change invariant must survive
the move. Pin it with a fresh test in core.

### [ ] 16. Stats date math → `meditate_core::format` (or new `core::date_math`)

**From:** `src/stats/imp.rs`:
- `locale_week_start_dow()` (line 692) — `nl_langinfo(_NL_TIME_FIRST_WEEKDAY)`
  bridge translating POSIX (Sun=1..Sat=7) to GLib (Mon=1..Sun=7).
  Pure libc call.
- `days_since_week_start(now: &glib::DateTime) -> i32` (722) — the
  arithmetic part lifts to `(day_of_week, locale_week_start) -> i32`.
- `week_over_week(daily_totals, now)` (733) — the aggregation logic
  is pure once the date strings are pre-supplied; only the
  `add_days(...)` walk is glib-bound.
- The chart-x-label format-decision logic embedded in `x_label_text`
  (line 868) — "if days==7 use weekday, if days==28 use day-of-month"
  etc. The decision part is pure; `glib::DateTime::format("%b")`
  stays at the gtk boundary.

**Why:** Every stats roll-up shares these primitives. Android's stats
view won't have the same glib datetime API but will need the same
"what's the start of this week, in this locale" logic.
**Migration:** new `meditate-core/src/date_math.rs` (or extend
`format.rs`). Caller computes day-of-week once with whatever facility
its platform offers and passes the integer in. ~30–60 LOC + tests.
**Risk:** medium. Rewrite of signatures (DateTime → integers) is the
main work.

### [x] 17. Log date-grouping helpers → `meditate_core::format`

**From:** `src/log/imp.rs`:
- `date_group_key(unix_secs)` (line 570) — formats a unix timestamp
  to `YYYY-MM-DD` for HashMap keys.
- `date_group_display(unix_secs)` (line 580) — "Today" / "Yesterday"
  / "Apr 17" / "Apr 17, 2025" with logic dependent on now.
- `format_time_of_day(unix_secs)` (line 597) — local hour-minute
  rendering for log card timestamps.

**Why:** All three operate on i64 unix timestamps; pure once `chrono::Local`
replaces `glib::DateTime`. Android's log view needs identical grouping.
**Migration:** new module or extension of `meditate_core::format`. Uses
`chrono` (already a core dep). Group-display predicates (`is_today`,
`is_yesterday`, `is_this_year`) split out as testable predicates;
formatting strings come from a small format helper.
**Risk:** low — straight chrono port.

### [ ] 18. Vibration-editor pure helpers → `meditate_core::vibration` (extends item 5)

**From:** `src/vibration_editor.rs`:
- `max_points_for_duration_s()` (lines 34–37) — point-count constraint
  given duration and `MIN_POINT_SPACING_MS`.
- `resample()` (lines 63–87) — linear resampling of the envelope when
  Points spinner changes. Pure f32 vec → f32 vec.
- Editor constants (lines 21–40): `DEFAULT_POINTS`, `DEFAULT_DURATION_S`,
  `POINTS_MIN`, `POINTS_MAX`, `MIN_POINT_SPACING_MS`, `INTENSITY_STEP`.
- The chart-x-label thinning algorithm (lines 759–798 inside
  `draw_chart`) — greedy left-to-right "minimum gap" interior-label
  selection. The decision part is pure; the cairo `move_to`/`show_text`
  calls stay in gtk.

**Why:** Android's vibration editor (whenever it ships) wants the same
math + the same editor invariants. Cairo drawing stays gtk-only.
**Migration:** the constants and `max_points_for_duration_s` /
`resample` move to `meditate-core::vibration` next to item 5's
envelope code. The label-thinning becomes a pure function returning
the indices to render. Cairo glue stays in `vibration_editor.rs`.
**Risk:** small.

### [x] 19. Sound-import validation helpers → `meditate_core::db` or new `core::sound`

**From:** `src/sounds.rs`:
- File-extension → output-extension/MIME mapping (lines 619–623) —
  `wav`/`ogg` pass through, everything else becomes ogg/vorbis. Pure
  string match.
- Custom-sound name-collision predicate (lines 463–480, 802–807) —
  case-insensitive name compare against `bell_sounds` rows, used to
  gate Import + Rename buttons.
- `MAX_CUSTOM_BELL_BYTES` constant + size-validation predicate (line 350).
- File-stem → display-name extraction (lines 387–391).

**Why:** Android's sound import needs the same constraints. None of
this touches GTK.
**Stays in gtk:** the file-chooser dialog, the gstreamer transcoding
that does the actual ogg conversion.
**Migration:** small new module or extension of an existing one.
~20–30 LOC + tests.
**Risk:** trivial.

### [x] 20. Bell display formatters → `meditate_core::db` extension

**From:** `src/bells.rs`:
- `bell_title(kind: IntervalBellKind)` (lines 346–358) — pattern-match
  → "Every 5 min" / "At 10 min" / "5 min before end" with optional
  jitter suffix. Pure string format on a portable enum.
- `sound_label(uuid: &str, library: &[BellSound])` (lines 364–375) —
  UUID → name lookup; permissive (returns `""` if missing).
- `pattern_label(uuid: &str, library: &[VibrationPattern])` (lines
  378–387) — same, for vibration patterns.
- The signal-mode capability-clamping logic at lines 489–498 — when
  haptic is unavailable, force `signal_mode` to `Sound`.
- `label_color_class_index(name: &str) -> usize` from
  `src/log/imp.rs:557` — DJB-style hash mapping a label
  name to a stable color-class index 0..7. The CSS class name is
  gtk-side, but the *index function* is the deterministic part.

**Why:** Android's bell list, label dispatch, and log view will all
re-use these formatters and predicates. The lookup helpers are why
the gtk shell currently re-implements UUID → name walks at every
call site — moving them gives a consistent contract.
**Migration:** module-level helpers next to the matching domain types
in `meditate-core::db` (or a new `core::display`).
**Risk:** small.

### [ ] 21. Display-second rounding + per-mode display dispatcher

**From:** `src/timer/imp.rs`:
- The ceiling-vs-floor rounding decision in `tick_running` (lines
  ~2699–2727) — countdown uses ceiling so a fresh "10:00" doesn't
  flicker to "09:59"; stopwatch uses floor.
- `elapsed_secs_for_mode` (lines 2425–2451) — central dispatcher
  that returns the right elapsed reading per mode (Timer countdown
  vs Timer stopwatch vs Box-Breath vs Guided), with prep-override
  and `final_duration_secs` override.
- `current_display_secs` (lines 4040–4072) — the same shape one
  layer up, returning the secs the running label should show.

**Why:** Android's running page needs the exact same "what number do
I show right now?" decision. Currently this logic is split across
two helpers and the tick body in gtk.
**Migration:** one pure fn `display_secs(state: &SessionState,
settings: &ModeSettings, now: Duration) -> u64` that consolidates
all three. Lives next to item 13's session-tick decision logic.
**Depends on:** items 11 (key helpers) and 13 (tick decision logic).
**Risk:** small once 13 lands; meaningless before then.

### [x] 22. Xorshift64 RNG for interval-bell jitter → `meditate_core::format`

**From:** `src/timer/imp.rs:4266–4281` — `next_random_unit`,
a lazy-seeded xorshift64 returning `f64` in `[0, 1)`. Pure algorithm.
**Why:** the interval-bell jitter math (already in `core::format::next_interval_ring_secs`)
takes a unit-uniform `f64` from the caller. Right now gtk supplies
its own RNG; Android would have to re-derive the same algorithm to
get matching jitter sequences for tests / determinism.
**Migration:** small. Move the function + its `Cell<u64>` state
abstraction (or expose a stateless `xorshift64(seed: u64) -> (f64, u64)`
that the caller threads). Test-friendly.
**Risk:** trivial. Determinism is preserved if callers seed the same way.

## Notes on `timer/imp.rs` post-migration

After items 8 + 10–13 land, the file still hosts:
- GTK template-children + `dispose` / `connect_*` boilerplate.
- The view-stack page-transition code (Setup ↔ Running ↔ Done).
- The mode-switching widget refresh (which widgets to show / hide).
- All the `glib::timeout_add_local` / `Cell<>` / `RefCell<>`
  reactive plumbing that turns the `meditate-core` decision output
  into pixel updates.

That's still ~2000 lines, but every one of them is genuinely
GTK-shell-bound. The other 3000 lines are what the migration is
hauling out.

## Branch strategy

Open question to settle before the first migration commit: do these
land on `beta` (each as a normal beta-track commit, then merged into
`android`) or directly on `android`?

- **Beta path** matches what the migrations actually are: pure
  refactors of GTK-shell code that improve `meditate-core`'s reach
  independently of the Android port. They're useful even if Android
  were cancelled. Beta-track commits get the usual on-device check
  on the Librem 5 before being declared good.
- **Android path** keeps the migrations co-located with the work
  that motivated them. Cleaner provenance ("this landed for the
  Android port") at the cost of GTK-shell churn accumulating on the
  android branch before the eventual merge back.

Recommendation: **beta**. Each migration is independently shippable,
benefits both shells, and follows the existing branch flow. Land on
beta, merge into android afterward (or rebase android onto beta).

Confirm before the first commit.
