# AGENTS.md

Read this first, whichever AI tool you use. It holds the rules that
are not visible from the code. The deeper docs are listed at the end.
Where an older doc disagrees with this file, this file wins.

## What this is

Meditate is a meditation timer (Timer, Box Breath and Guided modes,
with bells, vibration, a Log and Stats) that syncs between devices
through the user's own WebDAV server, such as Nextcloud. It is
published: the Android app on F-Droid as a reproducible build, the
Linux app as Flatpak bundles on GitHub releases. People have real data
in it, so data loss is the worst bug there is.

| Crate | What it is | Where things live |
|---|---|---|
| `meditate-core` | All decisions: timer and session state machine, bells, stats, the SQLite database, the event log and sync. No UI. | `src/session/` timer engine, `src/db/` tables and events, `src/sync/` WebDAV sync |
| `meditate-gtk` (cargo package `meditate`) | Linux app: GTK4 + libadwaita 1.7 | `src/` Rust, `data/ui/*.blp` layouts, `po/` translations |
| `meditate-android` | Android app: Slint (Material 3) + a little Kotlin | `src/app.rs` testable logic, `src/ui.rs` Android-only glue, `ui/main.slint` layout, `kotlin/` services, `lang/` translations, `android/` Gradle project |

## How a change is made

Every change goes through the `/spec` loop in
`.claude/skills/spec/SKILL.md`: plan files with WHEN/THEN scenarios in
`openspec/`, an approved proposal, tests first, an independent review,
docs, a device test by the maintainer, then one commit. Other tools:
read that file and follow it the same way.

- Branches: work lands on `beta`; `main` only moves at a release
  (fast-forward). The maintainer commits directly; a contributor opens
  a pull request against `beta` with the change folder in it, and the
  maintainer does the device test and the merge.
- Commits: `<area>: <change>`, short, the body says why. One logical
  change per commit. `Fixes #N` only for real GitHub issues, never for
  the numbers in OPEN-ISSUES.md.
- Open work, bugs and ideas: `OPEN-ISSUES.md`, one list.

## Commands

Run from the repo root. On a machine with little memory, set
`CARGO_BUILD_JOBS=4`.

```sh
cargo test --workspace                                   # all tests; needs GTK/libadwaita dev packages, blueprint-compiler and a display
cargo test -p meditate-core -p meditate-android --lib    # without GTK packages (what CI runs)
cargo clippy -p meditate -p meditate-core --all-targets  # must be warning-free
cargo clippy -p meditate-core -p meditate-android --lib -- -D warnings   # what CI runs
build-aux/check-specs.sh                                 # every test named in a spec exists
./build.sh gtk --debug                                   # Linux app
./build.sh android --debug                               # debug APK; needs JDK 17 (check: echo $JAVA_HOME)
./build.sh android --debug --mcp                         # emulator APK with Slint's MCP server (BUILDING.md); never on a phone
```

Setup, Flatpak builds, Android target clippy and troubleshooting:
`BUILDING.md`. After an Android build, run `./gradlew --stop` in
`meditate-android/android/` to free the Gradle daemon's memory.

## Hard rules

**Data**
- A schema or sync wire-format change that older data can't be read
  under ships a migration in the same commit, tested against a
  database written by the last release. Prefer additive changes (a
  new column, a new event kind), which need none.
- Multi-step writes are all-or-nothing: one transaction, or a clear
  message saying what landed.
- Every failure tells the user what happened. A failure that only
  reaches the log is a bug, and so is anything the user must repeat
  or clean up afterwards.

**Code**
- Decisions go in core; the apps only do what is platform-specific
  (widgets, audio, haptics). If the Android app would need the same
  logic, it belongs in core.
- Strict TDD: a failing test first, then the code.
- No warning suppression (`#[allow(...)]`): fix the cause, use
  `#[cfg(test)]` for test-only code, delete dead code.
- Timer code takes time from `crate::time::boot_time_now()`, never
  `Instant::now()`, so it survives system suspend.
- Text that core hands to the apps travels as a typed `*Key` enum;
  each app translates it.

**Android build environment: ask the maintainer first.** F-Droid
only ships the APK while its own rebuild matches it byte for byte.
Don't change any of these without asking:
`meditate-android/android/rust-build.sh`, anything Gradle in
`meditate-android/android/`, `meditate-android/build.rs`,
`meditate-android/manifest.yaml`,
`meditate-android/android/app/src/main/AndroidManifest.xml`,
`meditate-android/src/main.rs`, `third_party/`,
`meditate-android/material-1.0/`, `rust-toolchain.toml`, `Cargo.toml`
and `Cargo.lock` (no new or bumped crates), `build-aux/repro-*` and
`build-aux/fdroid-*`. Ordinary Rust, Kotlin, Slint and `.po` changes
are fine.

**User interface**
- The Android app mirrors the Linux app: before building an Android
  screen, read how the GTK one looks and behaves (`.blp` plus its
  Rust) and match its layout and behaviour as closely as Material 3
  allows. The wording may differ.
- Linux: prefer the native libadwaita pattern before anything custom.
- Slint has no z-index: an overlay, chooser, dialog, popup or snackbar
  is declared after everything it covers, including the app bar and
  navigation bar. Never hide the covered element instead.
- Android text: no symbol characters (✓ ✗ → … •); the phone's font
  lacks them. Use ASCII or a bundled SVG.
- No long dash (—) in new UI text or translations; use a comma or
  colon.
- No cut-off text and no "…" ellipsis: every text shows in full at
  phone width (360 px) in every language. Wrap, stack or shorten.
- A real trade-off that belongs to the moment of use is the user's
  choice in the app, not a hidden default.

**Translations**
- Nine languages: de, es, fr, it, nl, pl, pt_BR, ru, zh_CN. Every new
  or changed user-visible string gets all nine in the same change, in
  both apps where it appears.
- Linux: `build-aux/update-translations.sh` only (never the host
  meson targets), write `gettext("…")` directly (date patterns:
  `date_pattern("…")` in `format.rs`), and run
  `cargo test -p meditate i18n::`. Android: add to
  `meditate-android/lang/<lang>/LC_MESSAGES/meditate-android.po`.
- Check each touched file: `msgfmt --check -o /dev/null <file>`.

**Before saying done**
- Run the commands above and read their output to the end.
- Check the built file's timestamp; a "Finished" line alone doesn't
  prove it was rebuilt.
- Behaviour you can't test on the host (sound, vibration, layout,
  background behaviour on the phone) is only fixed once a person
  has tried it on a device.

## Doc map

When a change touches the left column, update the right one in the same
change (`/spec` step 5).

| Change | Update |
|---|---|
| Behaviour of an area | `openspec/specs/<area>/spec.md` (through the change's deltas) |
| A sync event's kind, payload or conflict rule | `EVENTS.md` |
| A new synced kind of item | follow and update `ENTITIES.md` |
| Crate layout, persistence, migrations, the session engine | `ARCHITECTURE.md` |
| A standing design rule | `DECISIONS.md`, and the hard rules above |
| A build, test or translation command | `BUILDING.md`, and Commands above |
| Release steps | `RELEASE.md` |
| A feature users see | `README.md` features list |
| A bug fixed or found | `OPEN-ISSUES.md` |

Also: `CONTRIBUTING.md` (for people), `meditate-core/README.md` (the
core crate). Finished plans and reports are not kept as files; git
history has them.
