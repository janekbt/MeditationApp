# rusqlite 0.41

## Why
Both apps store everything in SQLite through rusqlite 0.32, nine releases
behind; the SQLite bundled with it is 3.46, the newest is 3.54. Old
majors get no more fixes. OPEN-ISSUES #202.

## What Changes
- rusqlite 0.32 -> 0.41 in `meditate-core` and `meditate-gtk` together
  (libsqlite3-sys 0.30 -> 0.39, bundled SQLite 3.46 -> 3.54).
- The code adapts where the new API asks for it; nothing anyone sees
  changes. The breaking changes that could reach us: u64/usize no longer
  bind or read directly (0.38), `execute` rejects a trailing second
  statement and `prepare` rejects several (0.35), the statement cache is
  a default feature now (0.38; we keep default features).
- `Cargo.lock` with cc still at 1.2.60 and kstring at 2.0.2;
  `build-aux/cargo-sources.json` regenerated.
- Left out: a binary database file written by the old SQLite as a test
  fixture. The file format has been stable for years, the release-schema
  tests already exist, and the maintainer opening their own data on both
  apps is the real proof.

## Decisions
- Cargo.toml and Cargo.lock may change for this (maintainer, 2026-11-03).
- No new fixture file (maintainer agreed with the say-back).
- Only rusqlite and the crates it pulls in move in the lock: an update of
  rusqlite alone after the Cargo.toml bump, no general update (my call).
- Proof by a CI run of `flatpak.yml` on a commit object pushed to
  `deps-test` (Flatpak offline build with the new cargo-sources.json, CI
  APK); no fresh-build run, since no system library changes (my call).

## Capabilities
None: no behaviour changes (`skip_specs: true`).

## Impact
- Core: `meditate-core/src/db/`, `data_io.rs`, only where the new API needs it.
- GTK: `meditate-gtk/src/db/`, `data_io.rs`, `application.rs`, same.
- Android: no code (it uses the database through core); the APK's
  native library changes with the new SQLite.
- Translations: none. Data: none (same schema; SQLite reads old files).
- Android build environment: both Cargo.toml and `Cargo.lock` (asked);
  recipe, toolchain and Gradle untouched; cc ^1.2.27 and Rust 1.89 are
  enough for libsqlite3-sys 0.39 and rusqlite 0.41; the repro probe runs.
- Docs: OPEN-ISSUES.md (#202 leaves, AA row).
