# Architecture

Three crates in one Cargo workspace: a portable logic core, a GTK
shell for Linux and a Slint shell for Android. The split is
structural, not a convention. Rules, commands and the crate map are
in `AGENTS.md`; the reasons behind the standing rules are in
`DECISIONS.md`.

## The crates

- **`meditate-core`** (`meditate-core/`): pure Rust, no GTK imports.
  Session state machine, breath patterns, bell scheduling, stats,
  event-sourced SQLite persistence, WebDAV sync. Its `Cargo.toml`
  does not depend on `gtk4` or `libadwaita`, so an accidental import
  won't compile. This keeps core testable without a display server
  and shared by both shells. Module map: `meditate-core/README.md`.
- **`meditate`** (`meditate-gtk/`): the Linux shell, GTK4 +
  libadwaita. Widgets, gettext strings, `gtk::MediaFile` playback,
  a GStreamer pipeline for guided-file import and duration probing,
  feedbackd haptics over D-Bus, the GLib main loop. The GTK-side
  `Database` (`meditate-gtk/src/db/`) is a thin wrapper: row types
  with i64-unix times and a `note` field, plus its own error type.
  The unix/ISO conversion itself lives in core.
- **`meditate-android`** (`meditate-android/`): the Android shell,
  Slint + Material 3 on the same core. `src/app.rs` holds the
  testable logic, `src/ui.rs` the Android-only glue (callbacks and
  the 200 ms tick loop). The UI is `ui/main.slint` with the vendored
  Material components in `material-1.0/`; platform verbs are Kotlin
  (`kotlin/`); `android/` is a hand-maintained Gradle project
  (minSdk 26, target 34, arm64 by default, see BUILDING.md). It is
  published on F-Droid as a reproducible build, so its build
  environment is frozen (see `AGENTS.md`).

GTK translations are in `meditate-gtk/po/*.po`, Android ones in
`meditate-android/lang/<lang>/LC_MESSAGES/meditate-android.po`
(bundled into the binary by Slint at build time). Text that core
hands to a shell is a typed key; see "Typed-key i18n" below.

## Persistence model

Sync works on an event log, not on the database file. Uploading the
`.db` file would be whole-file last-write-wins: when two devices edit
offline, one device's changes are lost. Instead every change is one
self-contained event, and merging events is well defined. Conflicts
are resolved automatically by the rules in `EVENTS.md` ("Conflict
resolution"). There is deliberately no merge UI.

State lives in one SQLite DB owned by core. Every mutation appends a
row to the `events` table:

- `event_uuid`: random v4, the idempotency key
- `lamport_ts`: local Lamport clock at emission time
- `device_id`: this device's stable v4 uuid (assigned on first
  open, stored in the `device` table)
- `kind`: string discriminant (`session_insert`, `label_rename`,
  and so on; full catalogue in `EVENTS.md`)
- `target_id`: the affected row's cross-device identity (uuid for
  most entities, the setting key for `setting_changed`, the phase
  name for `box_breath_phase_update`)
- `payload`: JSON with the row's user-meaningful fields

The cache tables (`sessions`, `labels`, `presets`, `bell_sounds`,
`interval_bells`, `vibration_patterns`, `guided_files`,
`box_breath_phases`, `settings`) are a **pure function of the event
log**. The `recompute_*` family rebuilds a row from all events with
its `target_id`: the newest event by `(lamport_ts, device_id)` wins,
and a delete wins a tie with a mutate, so insert and delete arriving
out of order still end deleted. A peer can drop the cache and
rebuild it from the log at any time.

Device-local tables that never sync: `device`, `sync_state` (account,
cursors, cache version), the `known_remote_*` trackers and
`session_in_progress` (see "Crash resilience"). The diagnostics log
is a plain file (`<data>/diagnostics.log`) and does not sync either.

`Database::open` applies the schema, turns on WAL, and sets a 1 s
busy timeout (the sync worker raises its own connection's timeout).
Bundled rows are seeded by the caller after open:
`seed_all_non_audio` plus the shell's `seed_bell_sounds_with_paths`.
A `Database` wraps a single connection and is not thread-safe.

## Sync

`meditate-core::sync` talks to a folder `Meditate/` on the user's
own WebDAV server (for example Nextcloud). It is eventually
consistent for one person with several devices: not real-time and
not multi-user.

- **Order.** A pass pulls first, then pushes. Exception: when the
  pull fails because this build can't apply a batch
  (`SyncError::Db` or `SyncError::InvalidEvent`), the pass still
  pushes before returning the error, so local sessions keep
  uploading (`sync/orchestrator.rs`, `sync_with_progress`).
- **Event batches.** Each push writes one file
  `events/<min_lamport:014>__<batch_uuid>.json` holding a JSON array
  of events. Pull lists `events/`, skips batches recorded in
  `known_remote_files` and replays the rest. Events dedup by
  `event_uuid`. A `target_id` with path traversal is rejected before
  dispatch.
- **Audio files.** Custom bell sounds go to `sounds/<uuid>.<ext>`
  and guided files to `guided/<uuid>.ogg`, each its own file,
  tracked in `known_remote_sounds` and `known_remote_guided_files`.
  The rows themselves travel as events.
- **Atomic upload.** Every PUT goes to `<path>.tmp` and is then
  MOVEd to its name, so a partial body never sits under a real name.
- **Compaction.** Once more than 50 batch files this device has
  already pulled or pushed sit in `events/`, the
  device writes one consolidated batch (its full post-pull log),
  deletes the swallowed batches and records their uuids in
  `compacted.json`. A peer whose known batches all vanished checks
  that manifest to tell compaction apart from a wiped remote. An
  empty `events/` still raises the recovery dialog.
- **Plaintext.** Batches are plain JSON on the user's own server.
  Encryption is deliberately deferred. Adding it changes the wire
  format, so it needs a migration (`DECISIONS.md` rule 3).
- **Who runs it.** `sync::runner` is the one pass both apps run. It
  runs on a worker thread with its own DB connection, so a long sync
  never holds the UI's connection. Every local write bumps a
  `LocalChanges` counter (`db/local_changes.rs`). A shell polls a
  `LocalChangeWatch` from any thread to learn that something needs
  pushing. `sync::coordinator` keeps at most one pass in flight and
  collapses a burst of triggers into one follow-up.

## Migrations

The rule (`AGENTS.md` hard rules, `DECISIONS.md` rule 3): a change
older data can't be read under ships a migration in the same commit,
tested against a database written by the last release. Prefer
additive changes. A fresh DB only exercises first-time init, not
the upgrade path.

The mechanisms in `db/schema.rs`:

- **`SCHEMA_VERSION`** is stamped into `PRAGMA user_version` on open.
  A DB with a higher version is refused (`DbError::SchemaVersionTooNew`),
  because a downgrade could silently drop data it doesn't know.
  Bump it only for changes an older build cannot read.
- **`ADDED_COLUMNS`**: additive, defaulted columns.
  `add_missing_columns` adds them to older databases on open.
  `SCHEMA_VERSION` stays put, so the previous release can still open
  the DB.
- **`CACHE_SCHEMA_VERSION`**: bump it when `apply_event_inner` learns
  a new event kind or changes the columns a kind fills. On open, a
  lower stored value replays the whole log. Unknown kinds are
  recorded but not applied until then.
- **Upgrade tests** start from a frozen schema of an earlier release
  in `db/fixtures/` (currently only `schema_v26_8_5.sql`, used in
  `db/mod.rs`); add the last release's schema when a change needs it.

## Decisions in core, mechanisms in shell

New behaviour goes into `meditate-core` by default. Rule and reasons:
`DECISIONS.md` rule 1. Decisions reach a shell as a
`session::Effect`, a typed return value or a core function. The
shell only does the platform mechanism.

## Session, SessionSettings, Effect

Three types in `meditate-core/src/session/` carry one meditation
session from Start to Save or Discard:

```
SessionShape + DB  ->  SessionSettings  ->  Session  ->  Effect
 (user's setup)          (frozen plan)    (decisions)   (output)
```

The shell picks a `SessionShape`, calls
`SessionSettings::from_db(db, shape)`, starts a `Session`, then calls
`tick(now)` on a repeating timer (1 s on GTK, 200 ms for GTK Box Breath and on Android) and
dispatches the effects that come back.

### SessionSettings: the frozen plan

Everything a session needs, captured when the user presses Start and
unchanged for the session's lifetime. Changing something in the Setup
view mid-session affects only the next session.

Fields: `shape` (one of `TimerCountdown`, `TimerStopwatch`,
`BoxBreathCountdown`, `BoxBreathStopwatch`,
`Guided { duration_secs, count_up_display }`, carrying the target and
breath pattern), `prep_secs`, the interval-bell schedule `bells` with
its `bell_rng_seed`, the per-mode `signal_mode_override`,
`starting_bell`, `end_bell` and `box_breath_cues`.

The shell only chooses the shape. `SessionSettings::from_db` reads
everything else from the stored settings, and both shells call it
exactly once (tests in each shell enforce that). It is plain data:
clonable, with no Database, clock or shell reference.

### Session: the in-flight state machine

Exists from Start until Save or Discard. It owns a `SessionPhase`
(Prep, Running, Overtime, Stopped; `Paused` is reserved and
unreached, pause is a separate flag), a pause-aware `Stopwatch` phase
clock, the bell schedule with its fired state, and for Box Breath
the current breath phase.

Each `tick(now)` moves the machine forward and returns a
`Vec<Effect>`. The shell never asks "is it time for a bell?"; it
hands over `now` and reads the answers. Session never reads a clock,
plays audio, vibrates, draws or touches the database. The same
inputs always give the same effects, so a 40-minute session can be
driven through every boundary in a test in microseconds.

Public surface: `Session::start(settings, now)` (returns the session
plus its start effects, so a starting bell can't be lost), `tick` /
`tick_summary`, `pause` / `resume`, the terminators `stop`,
`finish_overtime` and `add_overtime_and_finish`, `enter_overtime`
(a guided track ended on its own), `ui_state()` and
`display_secs(now)` for refreshes outside the tick.

### Effect: the output instruction set

Each variant is one thing the shell should do this tick. All in
`session/effect.rs`:

- `UpdateDisplay { secs }`: refresh the big time label; Session has
  already rounded.
- `EndPrep`: prep silence over, now Running.
- `EnterOvertime`: target crossed; Pause becomes Finish, Add
  appears, the hero label freezes.
- `StopGuidedAudio`: Guided reached its planned length; stop the
  track now, before the end bell. Emitted first in the
  Running-to-Overtime transition.
- `UpdateOvertimeLabel { overtime }`: render the Add button's
  overtime count.
- `FireStartingBell { sound_uuid, vibration_pattern_uuid,
  signal_mode, volume }`: play the starting bell. `signal_mode` is
  already the effective mode (see below); the shell dispatches with
  no extra gating.
- `FireEndBell { … }`: same shape, at the end of the target.
- `FireBell { … }`: an interval or fixed bell reached its time.
- `FireBoxBreathCue { phase, … }`: a Box Breath phase boundary was
  crossed and a cue is set for the new phase.
- `EndBoxBreath { duration_secs }`: Box Breath reached its
  cycle-aligned target.
- `EndSession { duration_secs }`: the session is over (any path);
  `duration_secs` is what the saved row records.
- `StopActiveSignals`: cut any bell or vibration in flight. Emitted
  on Pause, Stop, Finish and Add.

`Effect::fire_route()` turns each of the four Fire variants into a
`FireRoute` (channel slot, log tag, sound uuid, vibration pattern
uuid, effective signal mode, volume), so the shell's dispatch is one
loop over `effects.iter().filter_map(Effect::fire_route)`.

**Signal mode has two levels.** Each bell and cue has its own
`signal_mode` (Sound, Vibration or Both; stored default Sound). Each
mode has a "what plays" override on the Setup view (default Both).
`bells::channel_allowed` ANDs the two per channel, and Session puts
the result into the Fire effect. The override defaults to Both
because it is a cap: Both means "no extra cap", so each bell's own
choice decides. On a device without a vibration motor the stored
value is never rewritten. Vibration is just skipped at dispatch, and
the shell shows a note under the row saying what still plays
(`bells::no_vibration_note`).

Effects are typed values, not side effects, so tests assert on the
exact stream, and the shells are pure routers: if the Android shell
shouldn't have to think about something, it belongs in core.

## Typed-key i18n

Core never calls `gettext`. Helpers that produce user-visible text
return a typed `*Key` enum (or a struct of them), and each shell maps
every variant to its own catalogue: GTK via gettext, Android via its
Slint `.po` files. Rule and reasons: `DECISIONS.md` rule 2.

## Testing

Strict TDD and the test commands: see `AGENTS.md`. New core logic
lands with tests for the happy path, every state-machine back-edge
and the boundaries (zero, empty, max, mid-cycle).

## Lifecycle and timing rules

Both shells run on phones, where the OS suspends the device or kills
the process.

### 1. Crash resilience

A running session must survive a crash, an OOM kill or a dead
battery. The shell snapshots it into the device-local
`session_in_progress` row (start time, accumulated seconds, mode and
a mode payload) at start and then about once a minute. The row is
outside the event log, so sync sees nothing while a session runs. On
the next launch core's `finalize_session_in_progress` saves what was
done as an ordinary session and clears the row.

No `std::time::Instant` in any persisted state: it means nothing
after a restart. Persist a wall-clock start and an accumulated
`Duration` instead.

### 2. Timing takes `now` as input, not from a clock

Core functions that answer "where are we in time", such as
`BreathPattern::phase_at(elapsed)`, `Stopwatch::elapsed(now)` and
`Session::tick(now)`, take the time as a parameter and never read a
clock themselves.

The shells own the clock, and both read
`meditate_core::time::boot_time_now()`, a wrapper around
`clock_gettime(CLOCK_BOOTTIME)`. `std::time::Instant` is
CLOCK_MONOTONIC on Linux and on Android and stops during suspend, so
never use it for session time (`DECISIONS.md` rule 6). The shell
passes `now` on each tick; the pause-aware `Stopwatch` in core turns
it into elapsed time.

## Module layout

The full module map is in `meditate-core/README.md`. The three
subtrees that hold the architecture:

```
meditate-core/src/
├── db/              SQLite, event log, recompute_* dispatch
│   ├── mod.rs                 Database, open/init, cache-upgrade walk
│   ├── schema.rs              SCHEMA, SCHEMA_VERSION, CACHE_SCHEMA_VERSION, ADDED_COLUMNS
│   ├── fixtures/              last release's schema for upgrade tests
│   ├── error.rs               DbError, name-collision suffix, target_id validator
│   ├── events.rs              event log, apply_event_inner, replay_events, winning_mutate
│   ├── device.rs              device_id, Lamport clock, observe_remote
│   ├── uuids.rs               seven typed entity UUID newtypes
│   ├── local_changes.rs       local-change counter + LocalChangeWatch
│   ├── session_in_progress.rs crash-safe running-session snapshot
│   ├── sessions.rs            sessions CRUD, stats queries, recompute
│   ├── labels.rs              labels CRUD, sync edge cases, recompute
│   ├── presets.rs             presets, recompute
│   ├── bell_sounds.rs, interval_bells.rs, vibration_patterns.rs,
│   ├── guided_files.rs, box_breath_phases.rs   per-entity CRUD + recompute
│   ├── settings.rs            key/value settings, recompute
│   ├── sync_state.rs          device-local sync KV
│   ├── known_remote.rs        batch files, sounds, guided files seen on the remote
│   ├── seeds.rs               bundled-row seeders
│   └── test_helpers.rs        #[cfg(test)] helpers
├── sync/            WebDAV push/pull
│   ├── mod.rs                 REMOTE_BASE_PATH, re-exports
│   ├── orchestrator.rs        pull/push/compaction, audio files
│   ├── runner.rs              one sync pass, shared by both apps
│   ├── webdav.rs              HTTP layer (ureq + roxmltree)
│   ├── coordinator.rs         one pass in flight, bursts collapse
│   ├── backoff.rs             exponential backoff
│   ├── settings.rs            persisted account + sync status
│   ├── credentials.rs         credential validation, connection test
│   ├── indicator.rs           sync-indicator state
│   └── fake.rs                in-memory WebDav for tests
└── session/         the session state machine
    ├── mod.rs                 Session, SessionPhase, UiState, TickOutcome
    ├── effect.rs              Effect, FireRoute, FireChannel
    └── settings.rs            SessionShape, SessionSettings::from_db
```

`meditate-gtk/src/` is GTK glue only: widget bindings, file choosers,
`gtk::MediaFile` playback, the GStreamer import and probe pipeline,
feedbackd haptics, gettext lookups.

`meditate-android/src/` is glue only too. Android-specific patterns
worth knowing before reading it:

- **App-classloader JNI bridge.** A native thread attached via
  `attach_current_thread` only sees the system classloader, so every
  Kotlin helper (`MeditateAudio`, `MeditateHaptics`,
  `MeditateKeychain`, …) is loaded through the activity's classloader
  (`getClassLoader().loadClass(dotted)`) and called as a static
  method. One Rust module per Kotlin object (`audio.rs`,
  `haptics.rs`, `keychain.rs`, …).
- **Drop-file + tick-poll.** `NativeActivity` never forwards
  `onActivityResult` or `onNewIntent` to native code. So whatever a
  Kotlin activity or receiver produces (SAF picks, transcode results,
  widget deep links, audio end or focus loss) is written to a small
  file under the app's internal `meditate/` folder and picked up once
  by the 200 ms tick loop.
- **Foreground service + wake lock.** `MeditateSessionService`
  (mediaPlayback foreground service, MediaSession, partial wake lock)
  keeps the process alive and the CPU awake for the session, so bells
  ring on time under Doze. The Rust tick loop stays the only timer.
  The Linux app does the same with a GTK suspend inhibit (plus an
  idle inhibit with "Keep screen awake"). On both, a pause releases
  the lock and the screen-on hold, and resume takes them back.
- **Legacy back, on purpose.** Back arrives as `Key.Back`, caught by
  the root `FocusScope` in `main.slint` and routed to the
  `back-pressed` callback (`ui.rs`, `on_back_pressed`). The manifest
  does not set `enableOnBackInvokedCallback`: with it, Android hands
  back to the predictive-back callback instead of a key event, and
  this path would stop working.
- **Vibration without amplitude control.** Some phones (the
  Fairphone 5, for example) can only switch the motor on or off. When
  `haptics::has_amplitude_control` is false, the pattern editor
  offers only the Bar style and snaps points to on/off
  (`vibration::binarize`). Amplitude is not emulated.
- **Guided import format.** Android converts imports to Opus in Ogg,
  because MediaCodec can't encode Vorbis, and conversion needs
  Android 10 (API 29) or later. GTK converts to Ogg/Vorbis. Both apps
  play both, so synced files work either way.
- **Colours.** The Material palette is rebuilt from one of six fixed
  accent seeds (`theme.rs`, `ACCENTS`). There is no wallpaper-based
  dynamic colour.

Reading order for a new contributor: `lib.rs` → `session/` →
`db/events.rs` → `sync/orchestrator.rs`.

## What is not tested

GTK widgets, signal wiring, dialogs, cairo drawing, GStreamer
pipelines, feedbackd calls and glib-dependent locale rendering; on
Android the Slint UI, Kotlin services, audio and vibration. These are
tested by hand on devices (a Linux laptop or Librem 5, an Android
phone). Android logic that admits a unit test lives in
`meditate-android/src/app.rs`.

TDD applies to **everything in `meditate-core`**, including sync,
which is tested against `FakeWebDav` (`sync/fake.rs`).

## See also

- `EVENTS.md`: per-kind JSON payloads and conflict rules; the
  wire-format spec for any peer.
- `ENTITIES.md`: how to add a new synced kind of item.
- `DECISIONS.md`: the standing design rules and why.
- Vibration pipeline: `meditate-core/src/vibration.rs` (pattern math,
  envelopes) and `meditate-gtk/src/vibration.rs` (feedbackd
  playback).
