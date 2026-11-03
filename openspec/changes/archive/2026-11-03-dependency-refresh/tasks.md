# Tasks: dependency-refresh

Base: c8e6e423c0b6107f5feb8ec73cc8f061cb334e03; suites on it: green (meditate-core 1444, meditate-android 287, meditate 168)

## Loop
- [x] 0 Skills loaded, again after every /compact: `/ponytail ultra`; `slint:slint` (loaded, no Android UI touched)
- [x] 1 Understand
- [x] 1b Pre-build agent: cc question to the maintainer (hold); probe on a commit object; 2.2 after landing; java-jdk depName
- [x] 2 Propose (approved: 2026-11-03)
- [x] 3 Build
- [x] 4 Review, then `/simplify` after the last review (fixes were docs only: no second review)
- [x] 5 Specs, docs, list (check-specs: every named test exists)
- [x] 6 Verify and hand over (phone, GTK dev build, CI Flatpak: maintainer passed all three, 2026-11-03)
- [ ] 7 Ship

## Review Focus
- A crate whose new patch needs Rust > 1.95 -> 1.1 (build on the pinned toolchain; hold back with --precise)
- A new build-time tool or system library in the Android graph (F-Droid recipe) -> 1.2
- Path-patched Slint crates (`third_party/`, =1.18.1) -> 1.1 builds against them; slint stays 1.18.1 (newest)
- Flatpak offline build with a stale cargo-sources.json -> 2.1 CI run
- blueprint-compiler 0.22 rejecting our .blp files or needing a newer GNOME SDK -> 2.1 CI run builds the GTK app inside the GNOME 51 SDK
- A behaviour change hidden in a patch release (rusqlite stays 0.32, ureq 2, so DB and sync code keep their majors) -> 1.1 suites + 3.1 device test

## 1. Cargo.lock
- [x] 1.1 `cargo update`, then `cargo update kstring --precise 2.0.2` and `cargo update cc --precise 1.2.60` (and the same for any other crate that needs Rust > 1.95). Proof: `cargo test -p meditate-core -p meditate-android --lib`, `cargo test -p meditate`, both clippy commands from AGENTS.md: green, zero warnings
- [x] 1.2 Recipe check: the Android build's build-script crates before and after (`cargo tree -p meditate-android --target aarch64-linux-android -e build,normal --prefix none`; crates that compile C or look up system libraries): nothing new that needs a tool the recipe lacks (it has gcc, libc-dev, rustup 1.95, NDK r27c, JDK 17). Proof: list in Notes; the Android debug build succeeds; `REF=<commit object> build-aux/repro-probe.sh` reports identical payloads
- [x] 1.3 Regenerate `build-aux/cargo-sources.json` (RELEASE.md recipe). Proof: covered by 2.1

## 2. Outside the app
- [x] 2.1 blueprint-compiler 0.22.2 URL + sha256 in the Flatpak manifest; checkout v7 (tags in flatpak.yml, commit SHA in fresh-build.yml and renovate.yml), upload-artifact v7, setup-java v6. Proof: `flatpak.yml` run by hand on branch `deps-test` (full scope): all jobs green, bundles built
- [ ] 2.2 renovate-config.json5: `matchDepNames: ["java-jdk"]` allowedVersions 21.x; OPEN-ISSUES entries #202-#208 (one per major) and #209 (pins + kstring, needs a fdroiddata MR); BUILDING.md line. Proof: after the commit is on `beta`, an update-check run on `renovate-test` lists only the pins, the seven majors and the lock-file line (cc, kstring)

## 3. Hand-over
- [x] 3.1 Debug APK on the FP5, the GTK dev build, and the CI Flatpak bundle (`meditate-x86_64.flatpak` from run 41618396769: oo7 in the Flatpak takes the Secret portal path, and zbus moved): one timed session with start and end bell, a sync with the password already stored, the Log shows it. Proof: maintainer says it works

## Hand-over
Artifacts: `meditate-android/android/app/build/outputs/apk/debug/app-debug.apk` (2026-11-03 11:17; later edits were docs only), `meditate-gtk/builddir/src/meditate` (12:45), `~/.cache/meditate-ci/meditate-x86_64.flatpak` (from run 41618396769).
Test, on each of the three: one timed session with start and end bell; a sync with the Nextcloud password already stored (no new login asked); the session shows in the Log.
- Phone: `adb install -r` the APK.
- GTK: `./meditate-gtk/builddir/src/meditate` (no Flatpak Meditate running, see the GTK test-run memo).
- Flatpak: `flatpak install --user ~/.cache/meditate-ci/meditate-x86_64.flatpak` over the installed 26.11.0; afterwards it stays the beta build until the next release replaces it.
fresh-build run 41630944002 (commit object f576aba, GTK legs only, maintainer's go): gtk on Ubuntu 26.04, Debian 13, Fedora, Arch all green. Delete branch `fresh-build` at ship. Phone test passed (maintainer, 2026-11-03).

Proposed commit:
```
deps: refresh Cargo.lock, blueprint-compiler 0.22, actions

Every crate to its newest version the Cargo.toml ranges allow, with
kstring (needs Rust 1.96) and cc (compiles the APK's C code) held for
the pinned toolchain. libadwaita 0.9.2 gates Accessible on its widgets
behind a GTK feature, so adw now names our GTK 4.18 floor too. Repro
probe identical; CI Flatpak and APK built.

blueprint-compiler 0.20.4 -> 0.22.2; checkout v7, upload-artifact v7,
setup-java v6. The update list shows the CI JDK only as 21.x (Gradle
8.5). The majors and the toolchain pins are OPEN-ISSUES #202-#209.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_012WGZUPYHL7TurMz6YSFVJs
```

## Notes
- 1.1: `cargo update` (262 changes), then cc held at 1.2.60 and kstring at 2.0.2. Core 1444 and Android 287 green at once. GTK failed to compile: libadwaita 0.9.2 gates `IsA<gtk::Accessible>` on its widgets behind its `gtk_v4_10` feature (two calls to `update_state` on `adw::ActionRow` in `timer/imp.rs`). Fix: `features = ["v1_7", "gtk_v4_18"]` on adw in meditate-gtk/Cargo.toml (we require GTK 4.18 already; GTK-only, not in the Android build). Then GTK 168 green; clippy (GTK and the CI command) zero warnings.
- 1.2: Android graph (aarch64, normal + build) before/after: new icu_locale_fallback, icu_locale_fallback_data, polycool, utf_types; gone icu_locale, icu_locale_data, tinyvec_macros. Only icu_locale_fallback_data has a build script, and it only sets cfgs (same as the icu_locale_data one it replaces): no C, no system library, no new tool. Android debug build ok (app-debug.apk 2026-11-03 11:17, 66.2 MB).
- 1.3: cargo-sources.json regenerated with RELEASE.md's generator (1387 entries; cc-1.2.60 and kstring-2.0.2 in it).
- Repro probe runs on commit object (Cargo.lock + meditate-gtk/Cargo.toml only), not on a branch. CI test pushed as commit object a7d8cdc to `deps-test`.
- Repro probe on 2489429 (commit object with the new lock; clone checked: uuid 1.29.0): "payloads are byte-identical across both paths", 28/28 entries. Probe clones deleted afterwards.
- 2.1: flatpak.yml run 41618396769 on `deps-test` (a7d8cdc): all five jobs green (metadata, tests + clippy, Android APK, Flatpak x86_64 and aarch64); artifacts: both .flatpak bundles, both tarballs, the APK.
- Review (fresh agent): 1 Flatpak keyring path untested -> added the CI bundle to 3.1. 2 fresh-build.yml not run although system-deps 7 -> 9 changes the host pkg-config lookups -> asked the maintainer (GitHub minutes). 3 rejected: #202 "Android uses rusqlite" is only two comments (`widget.rs:5`, `ui.rs:9906`). 4 blank lines around AA fixed. 5 kstring/cc hold is lock-only -> #209 says to re-hold after a bare update.
- /simplify (4 agents): fixed in docs only: #169 cites the new tags and lines; #209 drops the copied pin numbers, points AGP/NDK 28 to #201, names the CI APK's own JDK/NDK and its rule, and the rust-version fallback as the general kstring fix; AA entries lose the repeated "follow the changelog" and "ask first", table row #202-#208, #204 Where without the file list, #203 points at #192; BUILDING.md rewrapped. Skipped: merging #203/#207 (one entry per major was the maintainer's choice); `^21(\.|$)` vs `^17\.` (setup-java's value is a bare `21`, the `$` branch is needed); CI APK taking JDK 17 from setup-android.sh and the rust-version fallback (build environment, own change: in #209); start renovate.yml from main instead of `renovate-test` (wrong: the workflow isn't on main yet); proposal.md repeats (archived as is).
