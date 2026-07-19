# Architecture

Three-tier split: portable logic core, GTK shell, and a parallel
Slint Android shell — three crates in one Cargo workspace. The
split is structural, not a convention.

## The crates

- **`meditate-core`** (`meditate-core/`) — pure Rust, zero GTK
  imports. Session state machines, breath patterns, bell scheduling,
  stats aggregation, event-sourced SQLite persistence, WebDAV sync.
  Everything driven by tests. `Cargo.toml` does not depend on
  `gtk4` or `libadwaita`, so an accidental import won't compile.
  This is what makes the core fully testable without a display
  server and portable to any future shell.
- **`meditate`** (`meditate-gtk/`) — thin GTK4/libadwaita shell.
  Owns the widgets, the gettext-localized strings, the
  `gtk::MediaFile` playback pipeline, the feedbackd DBus haptic
  calls, and the GLib main loop. No business logic; the GTK-side
  `Database` is a translation shim that maps i64-unix timestamps
  to core's ISO 8601 strings.
- **`meditate-android`** (`meditate-android/`) — parallel Slint +
  Material 3 shell consuming the same `meditate-core`. Rust UI
  glue in `src/` (lib.rs hosts the handlers + tick loop), the
  declarative UI in `ui/main.slint` (vendored Material components
  in `material-1.0/`), platform verbs in Kotlin (`kotlin/`), and a
  hand-maintained Gradle project in `android/` (minSdk 26, target
  34, arm64 by default — see BUILDING.md#android). F-Droid-
  oriented; the GTK shell stays the permanent Linux-first target.

`meditate-gtk/po/*.po` holds the GTK gettext translations;
`meditate-android/lang/<lang>/LC_MESSAGES/*.po` the Android ones
(bundled into the binary by slint at build time). Translatable
copy in core is returned as typed key enums; each shell maps
variants to its own catalogue (see "Typed-key i18n" below).

## Persistence model

State is event-sourced into a single SQLite DB owned by core. Every
mutation appends a row to the `events` table carrying:

- `event_uuid` — random v4, idempotency key
- `lamport_ts` — local Lamport clock at emission time
- `device_id` — this device's stable v4 uuid (assigned on first
  open, persisted in the `device` row)
- `kind` — string discriminant (`session_insert`,
  `label_rename`, etc. — full catalogue in `EVENTS.md`)
- `target_id` — the affected row's cross-device identity (uuid for
  most entities; setting key for `setting_changed`; phase name for
  `box_breath_phase_update`)
- `payload` — JSON blob holding the row's user-meaningful fields

The materialized cache tables (`sessions`, `labels`, `presets`,
`bell_sounds`, `interval_bells`, `vibration_patterns`,
`guided_files`, `box_breath_phases`, `settings`) are a **pure
function of the event log**. A `recompute_<entity>` family
materialises each row from "all events with this `target_id`,"
last-write-wins on `(lamport_ts DESC, device_id DESC)` with delete
events tombstoning. A peer can drop the cache and rebuild from the
log at any time.

Sync: `meditate-core::sync` bulk-PUTs pending events to a WebDAV
remote in batched JSON files (`events/<lamport>__<uuid>.json`) and
pulls peer batches back. Dedup is keyed by `event_uuid`. PUT is
atomic via `.tmp` + MOVE so a partial body can't poison the
puller. Path-traversal on `target_id` is rejected before dispatch.
Once `events/` exceeds 50 batch files, the device compacts them
into one consolidated batch (its full post-pull event log) and
records the swallowed batch uuids in `<base>/compacted.json`; a
peer whose known batches all vanished consults that manifest to
tell compaction apart from a genuinely wiped remote (an empty
events dir still raises the recovery dialog).

## No migrations

The schema is **additive-only**. There is no migration code path.
If a refactor breaks readability of existing remote files or the
local DB shape, the recovery path is **wipe-and-reimport**, not
migration: clear the WebDAV folder, drop the local
`~/.local/share/meditate/meditate.db{,-shm,-wal}` (or the
flatpak-prefixed equivalent on the Librem 5), relaunch.

`PRAGMA user_version` is stamped at `Database::init` and refused on
open if it exceeds the build's known version (downgrades risk
silent forward-only-column corruption).

Janek is the single user. Back-compat shims are dead weight that
obscure the new design; they do not exist anywhere in this
codebase.

## Decisions in core, mechanisms in shell

Default destination for any new behaviour is `meditate-core`, not
the GTK shell. Decisions ("when to fire a bell," "what does this
streak read," "what state goes into the timer view") live in core
and reach the shell via a `Session::Effect`, a typed return value,
or a core API. Mechanisms ("how to play this OGG via
`gtk::MediaFile`," "how to invoke feedbackd over DBus," "how to
build this `AdwPreferencesRow`") stay in the shell, triggered by
core effects.

Heuristic: if the same logic would need to exist on the Android
shell, it belongs in core.

## Session, SessionSettings, Effect — the in-flight trio

The concrete instantiation of the rule above. Three types in
`meditate-core/src/session/` carry one meditation session from
Start to Save/Discard:

```
SessionSettings  →  Session  →  Effect
   (input)         (decisions)    (output)
```

Reading order: settings, then Session, then Effect. The shell
builds the settings, hands them to a fresh Session, then drives
`Session::tick(now)` once per UI frame and dispatches the Effects
that come back.

### SessionSettings — the frozen plan

The input packet. Every choice a session needs, captured at the
moment the user presses Start, immutable for the session's whole
lifetime.

Holds: mode (Timer / BoxBreath / Guided), prep seconds, target
seconds (`None` for stopwatch sessions), display direction,
breath pattern (BoxBreath only), the pre-built interval-bell
schedule + RNG seed for jitter draws, the per-mode signal-mode
override, and the resolved starting/end/box-breath cues.

Deliberately doesn't hold: current elapsed time, current phase,
bell-firing progress, any Database / clock / shell reference. It's
pure data — serializable, comparable by value, trivially clonable.

Why frozen: the user contract is "what I chose when I tapped
Start is what I get." Toggling something in the Setup view
mid-session never affects this in-flight session; the next one
will see the new state.

Why composed at the shell-core boundary: SessionSettings is the
**one place** where shell-specific composition (read DB, capture
UI state) meets core's portable logic. Anything that enters the
packet is portable from there onward. Each shell's job is exactly
"build this packet"; core's job is "run on it."

### Session — the in-flight state machine

The thing that exists from Start until Save/Discard. Owns a
phase enum (Prep / Running / Overtime / Paused / Stopped), an
elapsed-time bookkeeping clock, the bell schedule's
"which-have-fired" cursor, and (for BoxBreath) the current
breath-phase tracker.

Every UI tick, `tick(elapsed)` walks the state machine forward
and returns a `Vec<Effect>` — the things the shell should do
this tick. The shell doesn't ask "is it time for a bell?" — it
just hands Session the new elapsed time and reads the answers.

Deliberately doesn't do: read the system clock, play audio,
vibrate, draw widgets, touch the database, know that a UI exists.
It's a pure state machine: `(settings, phase, elapsed)` in,
`Vec<Effect>` out.

Why this shape — three reinforcing reasons:

1. **Testing.** A 40-minute meditation has hundreds of bell-
   firing decisions, multiple phase transitions, dozens of
   breath-cycle boundaries. By taking elapsed time as input, the
   entire lifecycle can be driven through every boundary in
   microseconds — pass `Duration::from_secs(2399)`, then `2400`,
   assert the transition.
2. **Two shells, one brain.** The Android port consumes the same
   Session unchanged. If Session called GTK's audio pipeline or
   feedbackd's DBus, every decision would have to be reimplemented
   for Android.
3. **Predictable state.** No I/O, no clock, no DB — the same
   `(settings, phase, elapsed)` always produces the same Effects.
   Bugs are reproducible because the inputs are reproducible.

Public surface is small: `start_prep` / `start_running`
constructors, `tick(now)`, `pause(now)` / `resume(now)`,
`add_overtime_and_finish(now)`, `display_secs(now)` for refreshes
outside the tick loop.

### Effect — the output instruction set

The typed enum Session uses to talk to the shell. Each variant
represents one observable user-visible thing that should happen
*this tick*. The shell loops through the returned `Vec<Effect>`
and matches on each variant.

Variants today, all in `session/effect.rs`:

- `UpdateDisplay { secs }` — refresh the big time label. Session
  has already done the rounding; the shell just renders.
- `EndPrep` — prep silence over, transition to Running.
- `EnterOvertime` — target crossed; morph Pause → Finish, reveal
  Add, freeze hero label.
- `UpdateOvertimeLabel { overtime }` — render "Add MM:SS ?".
- `FireStartingBell { sound_uuid, vibration_pattern_uuid,
  signal_mode }` — play the starting bell. The `signal_mode` is
  the *effective* mode (per-bell AND per-mode override already
  AND-ed by Session); the shell dispatches directly with no extra
  gating.
- `FireEndBell { … }` — same shape, at end-of-target.
- `FireBell { … }` — interval / fixed bell crossed its ring
  boundary.
- `FireBoxBreathCue { phase, … }` — BoxBreath phase boundary
  crossed AND a cue is configured for the new phase.
- `EndBoxBreath { duration_secs }` — BoxBreath cycle-aligned
  target reached naturally.
- `EndSession { duration_secs }` — terminal: the session is over
  (any path). `duration_secs` is what the saved row should record.
- `StopActiveSignals` — cut any in-flight bell / vibration
  immediately. Emitted at lifecycle boundaries (Pause / Stop /
  Finish / Add-overtime) so the user's in-flight feedback stops
  when they signal the session is interrupted.

A small `FireRoute<'a>` helper collapses the four "fire
something" variants into one dispatch loop in the shell: each
Fire-* variant resolves to a routing record (channel slot, diag
tag, sound uuid, vibration pattern uuid, effective signal_mode),
and the shell becomes `for r in effects.iter().filter_map
(Effect::fire_route) { play(r.channel, r.sound_uuid); … }`.

Why an enum (not booleans, not direct side-effects):

- Returning typed values keeps the contract testable and
  exhaustive — tests assert on the exact stream of Effects.
- Returning *resolved* fire-* values (sound uuid + vibration
  pattern uuid + AND-ed signal_mode) means the shell has zero
  decision logic in its dispatch loop. It's a pure router. The
  Android shell inherits the same routing for free.
- The shell-relevant rule of thumb: "if a future Android shell
  shouldn't have to think about this, it doesn't belong in
  Effect."

## Typed-key i18n

Core never calls `gettext`. Helpers that produce user-visible text
return a `pub enum *Key` (or `pub struct` of enum fields) capturing
every choice the shell needs to render. The shell `match`-es on the
typed key and emits the right `gettext(...)` per variant.

Tests in core assert on the typed value, not on rendered strings.
`.po` files and `xgettext` scanning stay shell-side. Each future
shell (Slint Material 3 on Android, etc.) defines its own
variant-to-string mapping.

## TDD workflow

Standard Red → Green → Refactor:

1. **Red** — write a failing test that describes one slice of
   behaviour. Compile failure counts; the missing types are part
   of the design.
2. **Green** — write the simplest code that makes the test pass.
   Resist anticipating the next test.
3. **Refactor** — clean up with the test net active.

Run with `cargo test --workspace` from the repo root. Core has no
GTK deps, so its tests run anywhere — including in CI without a
display.

Coverage today: ~970 tests across the workspace. New logic in
`meditate-core` is expected to land with tests covering happy
path, every state-machine back-edge, and the relevant boundary
conditions (zero, empty, max, mid-cycle).

## Android-readiness constraints

Two design rules that come from "ship on Android someday." Both
also make testing easier, so they earn their keep on day one.

### 1. State is serializable and resumable

Any long-lived state — a timer running, a session in progress,
partial form input — must serialize to disk and restore cleanly.
On Android the OS kills backgrounded processes routinely (doze,
low-memory); on Phosh it's rare but possible.

**Concrete rule:** no `std::time::Instant` inside serializable
state. `Instant` is process-local — its value is meaningless after
a restart. Use serializable types (a wall-clock `DateTime` for
"when did this start," an accumulated `Duration` for "how much
time has elapsed") and let the shell convert from monotonic deltas
at query time.

### 2. Timing takes elapsed time as input, not from a clock

Core functions that compute "where are we in time" —
`timer.remaining(...)`, `pattern.phase_at(...)`,
`session.is_finished_at(...)` — accept the elapsed `Duration` as a
parameter. They never call `Instant::now()` themselves.

The shell owns the clock. On GTK that's
`meditate_core::time::boot_time_now()` — a libc shim around
`clock_gettime(CLOCK_BOOTTIME)` that, unlike `std::time::Instant`,
counts time the system was suspended. (Rust 1.79+ uses
`CLOCK_BOOTTIME` natively on Android, so the Android shell uses
plain `Instant::now()` under `#[cfg(target_os = "android")]`.)

The shell samples the monotonic source on every UI tick, computes
elapsed since the last paused/resumed transition, and passes it
into core. Core stays pure with respect to time.

**Why this matters:**

- **Testing:** drive a 40-minute timer through its lifecycle in a
  microsecond — pass `Duration::from_secs(2399)`, then `2400`,
  assert the transition.
- **Android doze / wake:** the shell handles wall-clock jumps on
  resume once; core is unaffected.
- **Two shells, one core:** monotonic time has different APIs on
  GTK and Android. Keeping the choice in the shell means core
  never has to care.

## Module layout

```
meditate-core/src/
├── lib.rs           — module declarations + crate-level //! intro
├── db/              — SQLite + event log + recompute_* dispatch
│   ├── mod.rs       — Database + init/open + cache-schema walk
│   ├── schema.rs    — SCHEMA const + version sentinels
│   ├── error.rs     — DbError + collision suffix + target_id validator
│   ├── events.rs    — Event log + apply_event_inner + replay_events
│   ├── device.rs    — device_id + lamport_clock + observe_remote
│   ├── sessions.rs  — sessions CRUD + stats queries + recompute
│   ├── labels.rs    — labels CRUD + sync edge cases + recompute
│   ├── presets.rs   — preset templates + recompute
│   ├── bell_sounds.rs, interval_bells.rs,
│   ├── vibration_patterns.rs, guided_files.rs,
│   ├── box_breath_phases.rs    — per-entity CRUD + recompute
│   ├── settings.rs              — key/value settings + recompute
│   ├── sync_state.rs            — sync-side KV (URL, last-pull, etc.)
│   ├── known_remote.rs          — files/sounds tracked as seen on remote
│   ├── seeds.rs                 — bundled-row seeders
│   └── test_helpers.rs          — shared #[cfg(test)] synth_event etc.
├── sync/            — WebDAV push/pull engine
│   ├── mod.rs, settings.rs, credentials/  — config + account
│   ├── orchestrator.rs  — push/pull coordinator
│   ├── webdav.rs        — HTTP layer (ureq + roxmltree)
│   ├── coordinator.rs   — request-dedup state machine
│   ├── backoff.rs       — exponential backoff
│   ├── fake.rs          — in-process WebDav for tests
│   └── indicator.rs     — sync-indicator polling state
├── session/         — meditation-session state machine
│   ├── mod.rs       — Session, TickOutcome, Phase
│   ├── effect.rs    — Effect enum the shell dispatches against
│   └── settings.rs  — SessionSettings (target_secs, breath, bells)
├── bells.rs         — interval/starting/end bell scheduling math
├── breath.rs        — box-breath phases + perimeter math
├── vibration.rs     — vibration pattern editor + envelope helpers
├── format.rs        — translatable typed keys + plain formatters
├── goal.rs          — daily-goal snapshot logic
├── insights.rs      — derived stats (week-over-week, milestones)
├── contrib.rs       — contribution-heatmap data model
├── preset_config.rs — JSON-encoded preset payload (Timer / BB / Guided)
├── time.rs          — boot_time_now (suspend-resilient) + iso helpers
├── diag.rs          — ring-buffer log to <data>/diagnostics.log
├── data_io.rs       — CSV import / export of sessions
├── seeds.rs         — bundled vibration patterns + default presets
└── labels.rs, naming.rs, paths.rs, rng.rs,
    settings_keys.rs, sound.rs, timer.rs, date_math.rs
                     — supporting utilities
```

`meditate-gtk/src/` mirrors a small subset of these names
(`db/`, `sounds.rs`, `guided.rs`, `application.rs`, `window/`,
`timer/`, `sync_runner.rs`) but contains only GTK-side glue:
widget bindings, file-chooser plumbing, gst pipelines, gettext
lookups.

`meditate-android/src/` follows the same rule — glue only. Three
Android-specific patterns worth knowing before reading it:

- **App-classloader JNI bridge.** A native thread attached via
  `attach_current_thread` only sees the system classloader, so
  every Kotlin helper (`MeditateAudio`, `MeditateHaptics`,
  `MeditateKeychain`, …) is resolved through the activity's
  classloader (`getClassLoader().loadClass(dotted)`) and invoked
  as a static method. One Rust module per Kotlin object
  (`audio.rs`, `haptics.rs`, `keychain.rs`, …).
- **Drop-file + tick-poll.** `NativeActivity` never forwards
  `onActivityResult`/`onNewIntent` to native code, so anything a
  Kotlin Activity/receiver produces (SAF picks, transcode
  results, widget deep-links, audio EOS/focus-loss) is written to
  a small file under `<filesDir>/meditate/` and picked up
  single-shot by the 200 ms Slint tick loop.
- **Foreground service + wake lock.** `MeditateSessionService`
  (mediaPlayback FGS + MediaSession pin + partial wake lock)
  keeps the process alive and the CPU awake for the session's
  duration so bells ring on time under Doze; the Rust tick loop
  remains the only timer authority.

Reading order for a new contributor: `lib.rs` `//!` → `session/` →
`db/events.rs` → `sync/orchestrator.rs`.

## What is NOT tested

GTK widgets, signal wiring, dialogs, cairo rendering, gst pipelines,
feedbackd DBus calls, locale-dependent rendering that depends on
the glib type system. Those live in the GTK shell and stay
hand-tested on the dev laptop and the Librem 5.

The TDD discipline applies to **everything in `meditate-core`** —
including its sync layer (a `FakeWebDav` lives in
`sync/fake.rs` for in-process tests). The shell's Rust glue is
covered where it admits a unit test; widgets and runtime feel are
verified on device.

## See also

- `EVENTS.md` — per-event-kind JSON payload schemas; the wire-
  format spec for any peer shell.
- `Nextcloud-Sync.md` — the sync design document (original
  design; the bulk-batch + compaction current state is summarised
  in its header and above).
- `VIBRATION_ARCHITECTURE.md` — vibration pipeline (editor + envelope
  + feedbackd dispatch).
- `CORE_STRUCTURAL_BACKLOG.md` — outstanding audit-surfaced cleanup
  items (skipped + deferred).
