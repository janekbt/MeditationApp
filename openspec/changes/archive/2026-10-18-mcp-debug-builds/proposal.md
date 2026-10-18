# Slint MCP server in Android debug builds

## Why
UI checks on the emulator are done blind today: tap coordinates, take a
screenshot, crop, guess again. Slint 1.18's embedded MCP server (Cargo
feature `slint/mcp`) exposes the running app's element tree with
geometry and text, and drives it by element (click, drag, set value,
keys, screenshot). That makes checks like "is any text cut off at 360 px
in all nine languages" (#159) or "which controls are under 48 dp" (#147)
readable instead of guessed. No app behaviour changes.

## What Changes
- `./build.sh android --debug --mcp` builds a debug APK with the MCP
  server (feature `slint/mcp`, element ids kept via
  `SLINT_EMIT_DEBUG_INFO=1`, port baked in). `--mcp` without `--debug`
  is refused.
- `rust-build.sh` passes optional extra Cargo features from
  `MEDITATE_CARGO_FEATURES`; unset (every normal and F-Droid build) it
  runs exactly the same command as today.
- `android_main` sets `SLINT_MCP_PORT` from a port baked in at build time
  (`MEDITATE_MCP_PORT`), before Slint starts; without it, nothing happens.
- BUILDING.md: how to build, install on the emulator, forward the port
  and connect; AGENTS.md Commands gets the build line.
- Left out: MCP on the FP5 (the maintainer's daily phone), host runs of
  the Android UI (the host `main` shows the bare window without
  `ui.rs`), `slint-viewer` MCP for single `.slint` files (useful later,
  needs no code).

## Decisions
- Emulator only, debug builds only (maintainer, at the say-back).
- `--mcp` refuses `--release` and the default optimised build: a server
  that reads and drives everything, including the sync settings, has no
  place in a build that might end up on a phone (my call).
- Cargo.lock: an `--mcp` build leaves the committed lock untouched
  (saved and restored by build.sh) instead of adding the MCP crates
  (maintainer).
- `--mcp` builds x86_64 only (`ABIS=x86_64`, adding the rustup target if
  missing), so the APK can't be installed on the arm64 FP5 (my call,
  from the pre-build questions).
- Port 9315 on 127.0.0.1, reached with `adb forward` (my call; Slint's
  default bind address, its documented example port).
- Compile-time `option_env!` for the port, not a runtime switch: an APK
  never sees the environment it was built in (Slint PR #12447 notes the
  same) (my call).

## Capabilities
None (developer tooling; `skip_specs`).

## Impact
- Core, GTK, translations, data: none
- Android: `src/ui.rs` (`android_main`, a few lines)
- Android build environment: `android/rust-build.sh` (optional
  features), `build.sh` (`--mcp`). The committed Cargo.lock stays
  unchanged: `slint/mcp` needs crates that aren't in it (prost,
  async-net, pbjson, ...), so an `--mcp` build saves the lock and restores
  it on exit, also on error or Ctrl-C (`trap`). Repro probe.
- Docs: BUILDING.md, AGENTS.md Commands.
