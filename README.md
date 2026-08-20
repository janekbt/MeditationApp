# Meditate

A meditation timer and session log for GNOME.

Countdown and stopwatch, a browsable log, and daily-goal stats to help you build a consistent practice. Adaptive for desktop and Linux phones, with a native Android companion app that syncs over your own Nextcloud.

## Features

### Timer
- Countdown, stopwatch, Box Breath, and Guided modes
- Box Breath: pick a pattern (4-4-4-4, 4-7-8-0, 5-5-5-5) or dial in each phase; the running view traces a dot around an accent-tinted square as you breathe in, hold, out, hold
- Guided: play your own guided-meditation audio; the end bell rings when the track finishes
- Bells: an optional starting bell, interval bells (at fixed times, every N minutes, or randomised), and a per-mode end bell — each as sound, vibration, or both, with bundled or imported sounds and a visual vibration-pattern editor
- Quick presets plus custom durations
- Per-mode labels: each mode remembers the label you last used for it
- Optional post-session notes
- Pause, resume, discard
- Daily streak and a system notification when you're away from the app

### Log
- Date-grouped card feed of every session
- Filter by label, or sessions with notes
- Add, edit, or swipe to delete — with undo
- Import from Insight Timer, and CSV import/export for backups

### Stats
- 13-week contribution heatmap, with stars for days that cleared your daily goal
- Daily-goal ring showing today's progress
- Bar or line chart across week / month / 3 months / year
- Per-label breakdown: totals and session counts for each label
- Streak, total time, and session count at a glance

### Sync
- Optional sync between all your devices — Linux and Android — via your own Nextcloud (WebDAV, app password)
- Offline-first: everything works without a network; changes merge whenever you're back online, with conflict handling that never loses a session

### Preferences
- Daily goal and completion sound
- Manage your labels and timer presets

### Android app
- Native Android port (Slint + Material 3) in [`meditate-android/`](meditate-android/): the same modes, log, stats, presets, and Nextcloud sync on your phone, sharing the identical `meditate-core` logic — a session recorded on one device converges to all of them
- Home-screen widget for one-tap preset starts, foreground-service timer that survives screen-off, guided-audio import with on-device Opus transcode
- Build instructions: [`BUILDING.md`](BUILDING.md#android)

### General
- Translated into 10 languages (English, German, Spanish, French, Italian, Dutch, Polish, Brazilian Portuguese, Russian, Simplified Chinese) — on Linux and Android alike
- Keyboard shortcuts for the common actions
- Dark-mode and high-contrast safe; follows your system accent colour on Linux — on Android, pick from six accent colours in the app
- About → Troubleshooting view with a rolling diagnostics log, for attaching to bug reports
  - Log file lives at `~/.var/app/io.github.janekbt.Meditate/data/meditate/diagnostics.log` on Flatpak, `~/.local/share/meditate/diagnostics.log` otherwise — useful when the About dialog itself can't be opened

## Installation

### Flatpak (recommended)

Pre-built Flatpak bundles for **x86_64** and **aarch64** are attached to every
[release](../../releases) as `meditate-<version>-<arch>.flatpak`. The same bundles are
attached to every CI run on the [Actions](../../actions) page if you want an
unreleased build, though those expire after 90 days and need a GitHub login.

1. Download `meditate-<version>-<arch>.flatpak` from the latest release.
2. Install and run:

```sh
flatpak install --user meditate-<version>-<arch>.flatpak
flatpak run io.github.janekbt.Meditate
```

### Android (sideload)

Every CI run also attaches `meditate-android.apk` on the
[Actions](../../actions) page — download it and install with
`adb install -r meditate-android.apk`, or copy it to the phone and
open it (allow "install unknown apps" once). It is signed with the
standard debug keystore; the F-Droid listing (in preparation) will
be the properly published channel.

### Building from source

The hands-off path is a single script that takes care of
everything from dependencies to the finished binary:

```sh
./build.sh gtk               # GTK app (release) → meditate-gtk/builddir/src/meditate
./build.sh android           # Android APK (release) → app/build/outputs/apk/release/app-release.apk
./build.sh gtk --debug       # faster unoptimized build for hacking
./build.sh android --debug   # debug APK (what you want while iterating)
```

What it does for you:

- **Dependencies** — detects apt / dnf / pacman (Debian/Ubuntu,
  Fedora, Arch) and installs what's missing; if everything is
  already present it never touches `sudo`. Other distros get the
  exact list of what to install by hand.
- **Fails early instead of mid-build** — GTK 4.18 / libadwaita 1.7
  floors are checked up front (with the Flatpak fallback spelled
  out if your distro is too old), same for blueprint-compiler.
- **Rust** — uses your rustup or distro toolchain when suitable and
  installs rustup automatically when needed (the Android
  cross-target requires it).
- **Android toolchain** — first run bootstraps the pinned SDK, NDK,
  Gradle and Kotlin compiler (~3 GiB download, one time) into
  `~/Android`; later runs skip straight to the build. Release APKs
  are signed with the standard debug keystore — fine for
  sideloading, and F-Droid re-signs with its own key anyway.
- **Low-RAM guard** — on machines under ~20 GiB it caps cargo's
  parallelism so heavy LTO link steps can't freeze the box.

Everything below is the manual equivalent, for reference or
unsupported distros.

The workspace splits into three crates: the portable
[`meditate-core`](meditate-core/README.md) (persistence, sync, and
session logic — most non-UI contributions land here), the GTK shell
in [`meditate-gtk/`](meditate-gtk/), and the Android shell in
[`meditate-android/`](meditate-android/). See
[`ARCHITECTURE.md`](ARCHITECTURE.md) for the full map.

**Dependencies**

- GTK 4.18+, libadwaita 1.7+, GStreamer (with base plugins)
- [blueprint-compiler](https://gitlab.gnome.org/GNOME/blueprint-compiler) ≥ 0.16
- Rust (stable toolchain) + Cargo
- Meson ≥ 0.62, Ninja, pkg-config, a C compiler

`meson setup build` will fail fast with the name of anything missing. To install everything in one go:

> **Heads-up: GTK 4.18+ / libadwaita 1.7+ is required.** Older stable releases (e.g. Ubuntu 24.04 LTS, Fedora 40, Debian bookworm) ship GTK 4.14, which the build will reject with a `Package 'gtk4' has version '4.14.x', required version is '>= 4.18'` error during the Rust build. If your distro is below that floor, use the **Flatpak build (local)** path below instead — it pulls the GNOME 50 runtime and ignores system library versions entirely.

<details>
<summary>Debian / Ubuntu / PureOS</summary>

```sh
sudo apt install build-essential meson ninja-build pkg-config \
    libgtk-4-dev libadwaita-1-dev \
    libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev \
    blueprint-compiler rustc cargo
```

If the distro's `rustc`/`cargo` are too old, install via [rustup](https://rustup.rs) instead.
</details>

<details>
<summary>Fedora</summary>

```sh
sudo dnf install gcc meson ninja-build pkgconf-pkg-config \
    gtk4-devel libadwaita-devel \
    gstreamer1-devel gstreamer1-plugins-base-devel \
    blueprint-compiler rust cargo
```
</details>

<details>
<summary>Arch</summary>

```sh
sudo pacman -S --needed base-devel meson ninja pkgconf \
    gtk4 libadwaita \
    gstreamer gst-plugins-base \
    blueprint-compiler rust
```
</details>

**Build**

```sh
cd meditate-gtk
meson setup build
ninja -C build
./build/src/meditate          # run from the build directory
```

**Install system-wide**

```sh
cd meditate-gtk
meson setup build --prefix=/usr
ninja -C build
sudo ninja -C build install
```

**Flatpak build (local)**

One-time setup — install `flatpak-builder` and wire up the Flathub remote so the GNOME 50 runtime/SDK can be pulled automatically:

```sh
# Debian / Ubuntu / PureOS
sudo apt install flatpak flatpak-builder
# Fedora:  sudo dnf install flatpak flatpak-builder
# Arch:    sudo pacman -S --needed flatpak flatpak-builder

flatpak remote-add --if-not-exists --user flathub https://flathub.org/repo/flathub.flatpakrepo
```

Build and install the app. `--install-deps-from=flathub` tells flatpak-builder to fetch `org.gnome.Platform//50`, `org.gnome.Sdk//50`, and the `rust-stable` SDK extension on first run, so you don't need to install them by hand:

```sh
flatpak-builder --user --install --force-clean \
    --install-deps-from=flathub \
    flatpak_app build-aux/io.github.janekbt.Meditate.json
flatpak run io.github.janekbt.Meditate
```

**Cross-compile for aarch64 (developer iteration)**

If you're working on Linux-phone perf, `build-aux/dev-xbuild.sh` cross-compiles a Librem 5–compatible binary in ~15 seconds on an x86_64 host — avoiding the 20–35 minute `flatpak-builder --arch=aarch64` QEMU build. Output goes to `target/aarch64-unknown-linux-gnu/release/meditate`, ready to `scp` straight over a Flatpak-installed binary on the phone for testing. One-time prerequisites are documented at the top of the script.

See [`BUILDING.md`](BUILDING.md) for the full cross-compile + Librem 5 deploy cycle (including the kill-the-app + DB-wipe steps) and the Android build pipeline.

## Data

Sessions and settings are stored in a SQLite database at
`~/.local/share/meditate/meditate.db` (or the Flatpak equivalent
inside the sandbox). On Android it lives in the app's private
storage; use the CSV export or Nextcloud sync to get data out.

## Privacy

No telemetry, no analytics, no accounts. Your sessions live in a
local SQLite database on your device. Network access happens only
if you configure sync against your own Nextcloud, and only to that
server. On Android, cloud backup of the database is disabled — the
supported backup paths are your Nextcloud and the CSV export.

## License

Meditate is free software released under the [GNU General Public License v3.0 or later](COPYING).
