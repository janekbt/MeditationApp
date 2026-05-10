# Android Port Plan

Status: Planning. Branch `android` (off `beta` at `cef9cd0`) not yet created.
Owner: Janek (solo). No PRs; same `beta`-first discipline as the GTK shell.

## Goal

Ship the meditation app on Android by reusing `meditate-core/` (timer,
db, format, breath, sync — pure Rust, no GTK) under a Slint UI built
with the Material 3 component library. The current GTK4 / libadwaita
shell is the permanent Linux-first shell — it is not slated for
replacement at any point. The Android port is a parallel, lower-priority
target.

## References (consult regularly during implementation)

- Slint Material 3 component docs and API reference: <https://material.slint.dev/getting-started/>
  Re-read before starting each milestone so we use current
  components / props / theming hooks rather than what the model has
  cached.
- Slint Android backend docs: <https://docs.slint.dev/> (search for "Android" — exact path subject to change between versions).
- The default-style consolidation post (Mar 21, 2026) — <https://slint.dev/blog/default-native-style-change> — for the explicit carve-out: Material is the mobile recommendation; Fluent is the desktop default.

## Decisions (settled)

| Decision | Choice | Rationale |
|---|---|---|
| UI framework | Slint + Material 3 components (`material.slint.dev`) | Rust end-to-end, terminal-first build, smallest second-codebase burden. Material 3 components went stable Sept 2025; the Mar 21, 2026 default-style consolidation post (which post-dates Material's stable release) explicitly carves Material out as the mobile recommendation, so the direction is a recent re-affirmation, not stale framing |
| Distribution | F-Droid eventually; no Play Store | Avoids Play Store target-SDK churn and signing-key custody; F-Droid signs with its own key |
| Workspace shape | Top-level becomes a Cargo workspace, GTK shell renamed to `meditate-gtk/` | Clean host/target split; single source of truth for shared deps |
| Branch flow | Long-running `android` off `beta`. Merge back when shippable. | Keeps `beta` releasable; Linux work continues uninterrupted |

## Known caveats accepted

1. **Material You dynamic color** (wallpaper-derived palette) is unlikely to be wired up by the Slint Material crate. We get canonical Material 3 colors, not the user's personalised scheme.
2. **Edge-to-edge insets, IME padding, predictive back gesture.** Slint runs on `NativeActivity`; rough edges expected. Verify on real device early.
3. **Accessibility / TalkBack** is thinner than Compose's. Acceptable for v1.
4. **Slint's Material crate could be deprecated.** Mitigation: pin a specific version; if abandoned the pinned version keeps working at its frozen feature set.

## SDK floor / ceiling

- `minSdkVersion`: 26 (Android 8.0). Covers ~98 % of in-use devices, dodges the SDK-25-and-below paths in `Vibrator` and notification APIs.
- `targetSdkVersion`: 35 (Android 15). F-Droid does not require a current target SDK, but staying current avoids accumulating compat shims later.
- NDK: r27 (matches what `cargo-apk` and `slint-android` test against; pin in setup script).

## Workspace restructure

Top-level `Cargo.toml` becomes a workspace manifest:

```
MeditationApp/
├── Cargo.toml             (workspace root, no [package])
├── meditate-core/         (unchanged — shared logic)
├── meditate-gtk/          (current src/, renamed from `meditate`)
│   ├── Cargo.toml
│   ├── src/
│   ├── data/              (Blueprint UI, .desktop, schemas)
│   └── build.rs
├── meditate-android/      (new — Slint shell, cdylib for cargo-apk)
│   ├── Cargo.toml
│   ├── ui/                (.slint files)
│   ├── src/lib.rs
│   └── android/           (AndroidManifest.xml, res/ if needed)
└── build-aux/             (shared scripts: dev-xbuild, setup-android, cargo-sources regen)
```

The host-only deps (`gtk4`, `libadwaita`, `gstreamer`, `oo7`,
`gettext-rs`) stay in `meditate-gtk/Cargo.toml` and never appear in
the Android target's dep graph. `meditate-core` already has only
portable deps and needs no changes.

`meson.build`, `build-aux/dev-xbuild.sh`, and the Flatpak manifest
update to point at `meditate-gtk/` (one-time path edit). Flathub
build keeps working unchanged otherwise.

## Dev tooling on this Debian laptop

Installed by `build-aux/setup-android.sh` (idempotent, Debian/Ubuntu only):

| Component | Source | Pinned version |
|---|---|---|
| OpenJDK 17 | `apt install openjdk-17-jdk-headless` | distro |
| Android command-line tools | direct download (Google) | latest, pinned by SHA |
| Android SDK platform | `sdkmanager "platforms;android-35"` | 35 |
| Android SDK build-tools | `sdkmanager "build-tools;35.0.0"` | 35.0.0 |
| Android platform-tools (`adb`, `fastboot`) | `sdkmanager "platform-tools"` | latest |
| Android NDK | `sdkmanager "ndk;27.2.12479018"` | r27 |
| Rust targets | `rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android` | per current toolchain |
| `xbuild` (`x` CLI) | `cargo install --git https://github.com/rust-mobile/xbuild.git` | pinned by git rev in script |

No Android Studio. No emulator image (opt-in via `--with-emulator`
flag — uses `sdkmanager "system-images;android-35;google_apis;x86_64"`
plus AVD creation). On-device testing via `x run --device adb:<id>`
over USB or local network, mirroring the Librem 5 cycle.

We deliberately do *not* write `~/.cargo/config.toml` cross-linker
entries: `xbuild` invokes the NDK linker itself via `ANDROID_NDK_ROOT`
detection. Adding global Cargo target stanzas would only matter for
direct `cargo build --target aarch64-linux-android` invocations
outside `x run`, which is not part of the documented Slint-on-Android
flow. Skip until proven necessary.

Env vars (`ANDROID_HOME`, `ANDROID_NDK_ROOT`, `JAVA_HOME`, plus
`PATH` extensions for `sdkmanager`, `adb`, etc.) live in a single
sourced file at `~/.config/meditate-android/env.sh` so the script
owns the file outright and can rewrite it idempotently. A tiny
marker-bracketed block in `~/.bashrc` sources it.

### Setup-script contract

`build-aux/setup-android.sh`:

- Refuses to run on non-Debian/Ubuntu (`/etc/os-release` check) with a clear message.
- Top-of-file `PINNED_VERSIONS` block; one place to bump.
- Each install step is gated on a presence check (`which`, `sdkmanager --list_installed`, `cargo install --list`).
- Writes `~/.profile.d/android-sdk.sh` with `set -u`-safe exports.
- Appends `~/.cargo/config.toml` only if the `[target.aarch64-linux-android]` block is missing (idempotent merge, not blind append).
- Final step: smoke test — `cargo apk --version`, `adb version`, print `ANDROID_NDK_ROOT/source.properties`.
- `--with-emulator` flag installs the system image and creates an AVD.
- Exit non-zero with a useful message on any step failure; do not partially-write state.

### Memory ceiling note

This laptop has 15 GiB RAM and ~1 GiB swap (see
`reference_dev_laptop` memory). A Slint LTO build uses similar memory
to the current GTK Linux build; running both an `aarch64-linux-android`
release build and the GTK Flatpak build in parallel will tip over.
The setup script does not mitigate this — it's a workflow note for
when iterating: pause Nextcloud sync before heavy builds, run one
target at a time.

## Milestones

Each milestone is one or more `beta`-style commits on the `android`
branch. Each ends in a state where the Linux build still passes.

1. **Workspace restructure.** No Android code yet. Just split the tree, update `meson.build` / `dev-xbuild.sh` / Flatpak manifest paths, verify host build + tests still pass. Single commit.
2. **`setup-android.sh` lands.** Run from a fresh state on this laptop, end up with a working toolchain. Re-run is a no-op. Document the flag set in the script's `--help`.
3. **Empty `meditate-android` crate.** Slint Material "Hello" — one screen, one button. `cargo apk run --target aarch64-linux-android` installs and launches on a real device. No `meditate-core` integration yet.
4. **Wire `meditate-core::timer::Countdown` to one screen.** Start / pause / stop a countdown; render mm:ss. No persistence, no bells, no haptics.
5. **Stopwatch and box-breath modes.** Same screen scaffold, different `meditate-core` types. Still no persistence.
6. **DB persistence.** `rusqlite` bundled feature compiles for `aarch64-linux-android` unchanged. Sessions list view; per-label stats query.
7. **Bells.** Audio playback via Android `MediaPlayer` JNI (or `oboe-rs`). Decision deferred to milestone 7.
8. **Haptics.** Android `Vibrator` / `VibratorManager` JNI. Reuse `meditate-core` envelope-quantising logic; map quantised levels to Android amplitude scale (0-255).
9. **Foreground service + notification.** Required so the timer survives screen-off. New code, no Linux equivalent.
10. **Keychain.** Android `KeyStore` JNI for the Nextcloud app-password. `oo7` is host-only.
11. **Sync.** `meditate-core::sync` already abstracts the HTTP layer (`ureq`). Plug in the JNI keychain and run.
12. **Polish: edge-to-edge, predictive back, IME insets, theming.** First real user-facing exposure of the `NativeActivity` rough edges from caveat (2).
13. **F-Droid metadata + reproducible build.** Manifest, fastlane structure, `metadata/io.github.janekbt.Meditate.yml` for fdroiddata once we want to submit.

## Platform-edge replacement matrix

| Concern | Linux (current) | Android (new) | Where the JNI lives |
|---|---|---|---|
| UI toolkit | GTK4 + libadwaita | Slint + Material 3 | n/a (Slint is Rust) |
| Audio | `gstreamer-rs` | `MediaPlayer` or `oboe-rs` | `meditate-android/src/audio.rs` |
| Haptics | `libfeedback` D-Bus | `Vibrator` / `VibratorManager` | `meditate-android/src/haptics.rs` |
| Keychain | `oo7` (Secret Service) | Android `KeyStore` | `meditate-android/src/keychain.rs` |
| i18n | `gettext-rs` | `.po` parsed in Rust, OR Android string resources | TBD milestone 12 |
| DB | `rusqlite` bundled | `rusqlite` bundled (works as-is) | nothing |
| Notifications | none (foreground app) | `NotificationManager` + foreground service | `meditate-android/src/service.rs` |
| Suspend-resilient timing | `clock_gettime(CLOCK_BOOTTIME)` via libc shim | `Instant::now()` (Rust 1.79+ uses `CLOCK_BOOTTIME` natively on Android) | `meditate-core/src/time.rs` cfg-gates the shim |

## Open questions (pre-milestone-1)

- `meditate-core` currently isn't `no_std`; not relevant on Android (linux-android target has full std), but worth noting for any future embedded port.
- The current `cargo-sources.json` regen flow exists for Flatpak's offline build; F-Droid's reproducible-build flow needs its own audit when we get to milestone 13. Out of scope for now.
