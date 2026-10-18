# Tasks: Slint MCP server in Android debug builds

Base: 77a2889; suites on it: green at 8916935 (android --lib 226, core 1378+18, GTK 151); 37c012d and 77a2889 changed docs and the skill only.

## Loop
- [x] 1 Understand
- [x] 2 Propose (approved: 2026-10-18; incl. the ask-first OK for rust-build.sh and build.sh)
- [x] 3 Build
- [x] 4 Review
- [x] 5 Specs, docs, list (skip_specs: no behaviour; docs in 1.4)
- [x] 6 Verify and hand over (no device test: developer tooling only; emulator checks in 1.5)
- [x] 7 Ship

## Review Focus
- A normal build must not change: rust-build.sh's cargo command is identical when the variable is unset -> repro probe 22/22 and a byte compare of the release .so built before/after on the same tree (1.5).
- No MCP in builds that may reach a phone: `--mcp` refused with release; a normal debug APK's .so has no MCP server strings (1.3).
- Gradle runs rust-build.sh in preBuild: the variables must reach it through build.sh's environment, and a later normal build must rebuild without the feature (cargo's feature set changes, so it relinks) (1.3).
- SLINT_MCP_PORT must be set before `slint::android::init_with_event_listener`, which starts the server; android_main re-runs on activity recreate (setting it again is harmless).
- `std::env::set_var` is unsafe in edition 2024: called first thing in android_main, before our threads; android-activity's own threads don't read the environment.
- SLINT_EMIT_DEBUG_INFO changes generated code: only set with --mcp.

## 1. Tasks
- [x] 1.1 rust-build.sh: `${MEDITATE_CARGO_FEATURES:+--features "$MEDITATE_CARGO_FEATURES"}` on both cargo lines. Proof: with the variable unset, `bash -x` prints the same cargo command as on the base.
- [x] 1.2 ui.rs android_main, first statement: if `option_env!("MEDITATE_MCP_PORT")` is set, `set_var("SLINT_MCP_PORT", port)` (SAFETY comment). Proof: android --lib tests, CI clippy, Android target clippy, zero warnings.
- [x] 1.3 build.sh: `--mcp` flag, Android + `--debug` only, else `die`; exports MEDITATE_CARGO_FEATURES=slint/mcp, MEDITATE_MCP_PORT=9315, SLINT_EMIT_DEBUG_INFO=1 and ABIS=x86_64 (rustup target add x86_64-linux-android if missing); saves Cargo.lock and restores it with a trap on EXIT. Proof: after an --mcp build `git status` shows Cargo.lock unchanged; `./build.sh android --mcp` and `--release --mcp` die with a message; a normal `--debug` APK's .so has no "Slint MCP server" string; an `--debug --mcp` one has it.
- [x] 1.4 BUILDING.md section + AGENTS.md Commands line: `ABIS=x86_64 ./build.sh android --debug --mcp`, install on the emulator, `adb -s emulator-5554 forward tcp:9315 tcp:9315`, `claude mcp add --transport http slint-app http://127.0.0.1:9315/mcp` or curl tools/list; never on a phone.
- [x] 1.5 Emulator (API 35): MCP tools/list answers; get_element_tree shows Meditate's Timer page; one click_element on a known element changes the page; after an activity recreate (font_scale) the server stops answering until the app restarts: a Slint limit, accepted and documented (maintainer, after review finding 2). Proof: the responses in the notes.
- [x] 1.6 Repro probe on the final tree: 22/22.

## Notes
- Pre-build agent: (1) Cargo.lock lacks the mcp crates; my earlier check was wrong (`cargo tree --locked` resolved them without error and without writing). Maintainer: save/restore the lock. (2) This proposal's approval is the ask-first OK for rust-build.sh and build.sh. (3) Emulator-only enforced via ABIS=x86_64 (my call). Also to check on the emulator: the server after an activity recreate (android_main re-runs on a new thread).
- 1.1: fake `cargo` on PATH recorded rust-build.sh's commands: without the variable identical to the base (`build -p meditate-android --target aarch64-linux-android [--release]`); with it `--features slint/mcp` appended. Empty-array-safe expansion `${FEATURES[@]+...}` for older bash under `set -u`.
- 1.2: android --lib 226 passed (a source-text test first failed because my comment named `init_with_event_listener`; reworded); Android target clippy clean.
- 1.3: `--mcp` without android --debug dies (3 cases). Normal debug APK 17:30: arm64 only, 0 "Slint MCP server" strings. `--debug --mcp` APK 17:33: x86_64 only, string present; `git status` clean for Cargo.lock afterwards. Also added (my call): build.sh deletes jniLibs before every Android build, so a library left from another ABI can't ship in the APK (it did before: an x86_64 lib from an emulator build stayed in jniLibs and went into the next arm64 APK).
- 1.5 (API 35 emulator): tools/list = 18 tools; list_windows -> 1 window; get_window_properties -> root element, scale 2.625; get_element_tree -> 1000 elements with ids (MainWindow::root), roles, labels ("Start Session" Button 363x56 at 24,334), positions; click_element on the "Stats" tab switched the page (take_screenshot shows Stats). Activity recreate (font_scale): the port stays bound but nothing answers (old event loop gone; Slint starts the server once per process). force-stop + relaunch fixes it; documented in BUILDING.md as a known limit.

## Review (2026-10-18)
- R1 held, CRITICAL: the --mcp build left its x86_64 MCP library in jniLibs, and the release steps in RELEASE.md run Gradle directly, so the next release APK would have shipped it (and broken F-Droid's verification). Deleted the leftover; build.sh's --mcp exit trap now removes jniLibs too (the APK is built by then). Proof pending: a new --mcp build, then jniLibs absent and the APK still with lib/x86_64.
- R2 held: the "still answers after a recreate" proof failed and was ticked anyway. Maintainer: accept the Slint limit, documented in BUILDING.md; task text corrected.
- R3 held: BUILDING.md now says to check git status and jniLibs after Ctrl-C or a killed build.
- R4 rejected: setting the trap before the `cp` would, if the copy failed, restore an empty file over Cargo.lock. The current order can at worst leave an empty temp file.
- Open question for the maintainer: rust-build.sh could clear jniLibs itself before its loop, which would also protect manual `ABIS=x86_64 ./gradlew` builds (BUILDING.md mentions that path). Not done: it's beyond the approved rust-build.sh change.
- Maintainer: rust-build.sh clears jniLibs itself before its loop (covers manual Gradle builds); the same line before the build in build.sh was removed as a duplicate, the --mcp exit trap still removes the MCP library after the build. Fake-cargo check: commands unchanged. --mcp rebuild: jniLibs absent afterwards, APK still lib/x86_64 only, Cargo.lock clean.
- Probe bd1d1af stopped (obsolete after the rust-build.sh change); rerun on the final tree.
- Second look (jniLibs fixes): PASS. Two BUILDING.md wording points applied (the killed-build note, rust-build.sh clearing jniLibs).
- 1.6: probe on a1b0a1d (final code; later edits are BUILDING.md wording and tasks): 22/22 identical.
