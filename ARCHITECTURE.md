# Architecture (tdd-rewrite branch)

A from-scratch, test-driven rewrite of the Meditate app. Lives in `rewrite/` as a
separate Cargo crate so the existing app on this branch still builds for
side-by-side reference. When the rewrite reaches feature parity, `rewrite/`
graduates to replace the top-level `src/`.

## Two-tier split

The rewrite enforces a hard line between logic and presentation:

- **`meditate-core`** (`rewrite/`) — pure Rust, zero GTK imports. Breath
  patterns, session timing, stats aggregation, label CRUD, SQLite persistence,
  CSV import/export. Everything driven by tests.
- **`meditate-app`** (future) — thin GTK4/libadwaita shell that calls into
  `meditate-core`. Hand-tested on device. No business logic here.

The split is structural, not a convention: `meditate-core/Cargo.toml` does not
depend on `gtk4` or `libadwaita`, so an accidental import won't compile. This
is what makes the core fully testable without a display server.

## TDD workflow

Standard Red → Green → Refactor:

1. **Red** — write a failing test that describes one slice of behaviour.
   Compile failure counts as Red; the missing types are part of the design.
2. **Green** — write the simplest code that makes the test pass. Resist the
   urge to anticipate the next test.
3. **Refactor** — clean up with the test net active. Keep tests green.

Run tests with `cd rewrite && cargo test`. The crate has no GTK deps, so this
runs anywhere — including in CI without a display.

## Module layout (planned)

```
rewrite/src/
├── lib.rs           — module declarations
├── timer.rs         — countdown / stopwatch state machine (first feature)
├── breath.rs        — breath patterns (box, 4-7-8, …) and phase calculation
├── session.rs       — session model: a completed meditation + metadata
├── label.rs         — label CRUD with DB-level uniqueness
├── stats.rs         — streaks, totals, heatmap levels, milestones
├── importers/       — CSV, Insight Timer
└── db/              — SQLite schema, migrations, query helpers
```

Modules appear when a test drives them into existence, not before. The order
above is roughly the order they'll be built: timer first because it's the
core user interaction, everything else stacks on top.

## Android-readiness constraints

Two design rules that come from "ship on Android someday." Both also make
testing easier, so they earn their keep on day one.

### 1. State is serializable and resumable

Any long-lived state — a timer running, a session in progress, partial form
input — must serialize to disk and restore cleanly. On Android the OS kills
backgrounded processes routinely (doze, low-memory pressure); on Phosh it's
rare but possible. A meditation that vanishes mid-session because the app
was suspended is unacceptable on either platform.

**Concrete rule:** no `std::time::Instant` inside serializable state.
`Instant` is process-local — its value is meaningless after a process
restart. Use serializable types (a wall-clock `DateTime` for "when did this
start", an accumulated `Duration` for "how much time has elapsed") and
let the shell layer convert from monotonic deltas at query time.

### 2. Timing takes elapsed time as input, not from a clock

Core functions that compute "where are we in time" — `timer.remaining(...)`,
`pattern.phase_at(...)`, `session.is_finished_at(...)` — accept the elapsed
duration as a parameter. They never call `Instant::now()` themselves.

The shell layer owns the clock: it samples a monotonic source on every UI
tick, computes elapsed since the last paused/resumed transition, and
passes that into the core. The core is pure with respect to time.

**Why this matters:**
- **Testing:** drive a 40-minute timer through its lifecycle in a microsecond
  — pass `Duration::from_secs(2399)`, then `2400`, assert the transition.
- **Android doze / wake:** when the device wakes from sleep the wall clock
  may have jumped forward; the shell handles that once and the core is
  unaffected.
- **Two platforms, one core:** monotonic time has different APIs on Phosh
  (glib's `g_get_monotonic_time`) and Android (`SystemClock.elapsedRealtime`).
  Keeping the choice in the shell means the core never has to care.

These two rules are the Android-readiness contract for the core. Every
test should assume them, every API should respect them.

## What is NOT tested

The same exclusions as `TESTING.md` on `beta`: GTK widgets, signal wiring,
dialogs, cairo rendering, locale-dependent formatting that depends on the
glib type system. Those live in the GTK shell and stay hand-tested on device.
