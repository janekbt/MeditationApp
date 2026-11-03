# Tasks: rusqlite-0-41

Base: 8647b2c492a6bc46ad97f1f287f118df3237fd0e; suites on it: green (meditate-core 1462 incl. integration, meditate-android 287, meditate 168)

## Loop
- [x] 0 Skills loaded, again after every /compact: `/ponytail ultra`; `slint:slint` (loaded, no Android UI touched)
- [x] 1 Understand
- [x] 2 Propose (approved: 2026-11-03)
- [x] 3 Build
- [x] 4 Review, then `/simplify` after the last review (no fixes)
- [x] 5 Specs, docs, list (check-specs ok; OPEN-ISSUES: #202 out, AA row #203-#208; no other doc names the version)
- [x] 6 Verify and hand over (phone and GTK dev build on a copy of the real data: maintainer passed both, 2026-11-03)
- [x] 7 Ship

## Review Focus
- An old database (release schema) opening under SQLite 3.54 -> 1.2 runs the existing release-fixture tests in `meditate-core/src/db/mod.rs` (`a_release_database_opens_with_every_bell_at_the_middle_volume`, `a_bell_from_the_last_release_keeps_its_times`, `the_release_can_still_open_an_upgraded_database`); 3.1 the maintainer's real data
- A u64/usize value bound or read without a cast -> the compiler finds it in 1.1
- An `execute`/`prepare` string with a second statement after the first `;` (migrations, schema, import) -> only shows at run time: 1.2 the full suites run every migration and import path; 1.1 greps those strings for an inner `;`
- The lock pulling a newer cc, or a crate that needs Rust > 1.95 -> 1.1 checks cc and kstring in the lock afterwards
- The Flatpak offline build with a stale cargo-sources.json -> 2.1
- The APK not byte-identical across paths with the new SQLite C build -> 2.2 repro probe
- The GTK app's own database code (`meditate-gtk/src/db/mod.rs`) -> 1.2 GTK suite

## 1. Bump
- [x] 1.1 `rusqlite = { version = "0.41", features = ["bundled"] }` in both Cargo.toml, then update only rusqlite in the lock (`-p rusqlite`); check Cargo.lock: cc 1.2.60, kstring 2.0.2; Android graph before/after (see Notes). Fix what fails to compile with the smallest change. Grep the SQL strings passed to `execute`/`prepare*` for a second statement. Proof: both clippy commands from AGENTS.md, zero warnings
- [x] 1.2 Proof: tests of meditate-core (all targets), meditate-android `--lib`, meditate: green, same counts as the base
- [x] 1.3 Regenerate `build-aux/cargo-sources.json` (RELEASE.md recipe). Proof: 2.1

## 2. Builds
- [x] 2.1 `flatpak.yml` run by hand on `deps-test` (commit object): all jobs green, bundles built
- [x] 2.2 Android debug build; `REF=<commit object> build-aux/repro-probe.sh`: payloads identical

## 3. Hand-over
- [x] 3.1 Debug APK on the FP5 (`adb install -r`, keeps its data) and the GTK dev build on a scratch data dir holding a copy of the maintainer's real database: the Log and Stats show the old sessions; one timed session saved and shown in the Log; one sync. Proof: maintainer says it works

## Notes
- Pre-build agent, answered as my call: (1) crates rusqlite adds to the lock only for targets we never build (wasm) are fine; what the Android build compiles is compared before/after (`cargo tree -p meditate-android --target aarch64-linux-android -e build,normal --prefix none`), and a new crate there with a build script that compiles C or looks up a system library stops the change for a question; shared crates moving (hashbrown, smallvec, bitflags) are fine if the repro probe stays identical. (2) A new warning or deprecation gets the smallest code fix; one outside the files under Impact is a question first. No `;` inside any `execute`/`prepare` string and no direct u64/usize binding found (agent scan).
- 1.1: lock update of rusqlite alone: rusqlite 0.32.1 -> 0.41.1, libsqlite3-sys 0.30.1 -> 0.39.1, hashlink 0.9.1 -> 0.12.2, added rsqlite-vfs 0.2.0 and sqlite-wasm-rs 0.6.2 (wasm only, not in the Android graph). cc 1.2.60, kstring 2.0.2 unchanged. Android graph (aarch64, normal + build) before/after: only those three versions move, ahash leaves it (still in the lock for other graphs); nothing new compiles C. No code change needed; both clippy commands: zero warnings (libsqlite3-sys rebuilt 13:3x).
- 1.2: meditate-core 1444 + 4 + 11 + 3, meditate-android 287, meditate 168: all green, same as the base.
- 1.3: cargo-sources.json regenerated (RELEASE.md generator); has rusqlite-0.41.1, libsqlite3-sys-0.39.1, hashlink-0.12.2, rsqlite-vfs-0.2.0, sqlite-wasm-rs-0.6.2.
- CI commit object 31d5706 (the four build files only) pushed to `deps-test`; flatpak.yml run 41637923701.
- Android debug APK 13:43 (66.2 MB).
- Review (fresh agent): 1 "proofs open" rejected: the agent read tasks.md before 1.1-1.3 were ticked; the Flatpak SDK's rust-stable vs rusqlite's Rust 1.89 is proven by 2.1. 2 the rusqlite-only lock update also moved tempfile's getrandom 0.3.4 -> 0.4.3 (already in the lock and cargo-sources.json; tempfile is a dev-dependency, not in the APK or the Flatpak app): kept, noted here. Set aside by the agent, checked in the sources: bundled build flags (only SQLITE_ENABLE_MEMORY_MANAGEMENT gone, unused), extended result codes still on, open flags, busy timeouts, Immediate transactions, statement cache, multi-statement check accepts a trailing `;`.
- /simplify (4 agents): nothing applied. Skipped: one `[workspace.dependencies]` rusqlite line instead of two (root Cargo.toml, build environment, new pattern there, and Cargo already refuses two libsqlite3-sys copies through `links`, so the two lines can't drift silently); repeated cc/kstring and scan lines inside this change folder (archived as is). No code changed, so no suite rerun.
- 2.2: repro probe on 31d5706 (rusqlite 0.41.1 and libsqlite3-sys 0.39.1 compiled in both clones): "payloads are byte-identical across both paths", 28/28. Probe clones deleted.
- 3.1 change (my call): the GTK copy does no sync. It has the Flatpak's device id, so syncing it next to the real install would put two writers under one device id on the server. Sync is tested on the phone.

## Hand-over
Artifacts: `meditate-android/android/app/build/outputs/apk/debug/app-debug.apk` (2026-11-03 13:43), `meditate-gtk/builddir/src/meditate` (13:56), data copy `~/.cache/meditate-202/` (the Flatpak's meditate.db with -wal and -shm, copied 13:5x; the Flatpak was not running).
- Phone: `adb install -r` the APK (closes Meditate if open; data stays). Log and Stats show the old sessions; one short timed session, saved, shows in the Log; one sync without errors.
- GTK: no Flatpak Meditate running; `XDG_DATA_HOME=~/.cache/meditate-202 LOCALEDIR=meditate-gtk/builddir/po ./meditate-gtk/builddir/src/meditate`. Log and Stats show the same old sessions as the Flatpak; one short timed session, saved, shows in the Log. No sync.

Proposed commit:
```
deps: rusqlite 0.41 (bundled SQLite 3.54)

rusqlite 0.32 -> 0.41 in core and the Linux app together. Its
libsqlite3-sys 0.39 builds with the held cc 1.2.60 and the pinned Rust.
No code had to change: nothing binds u64/usize directly, and every
multi-statement string already goes through execute_batch. Repro probe
identical; CI Flatpak and APK built.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_012WGZUPYHL7TurMz6YSFVJs
```
- 2.1: flatpak.yml run 41637923701 on `deps-test` (31d5706): all five jobs green (metadata, tests + clippy, Android APK, Flatpak x86_64 and aarch64).
