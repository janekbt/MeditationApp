# Meditate

A meditation timer and session log for GNOME.

Countdown and stopwatch, a browsable log, and weekly-goal stats to help you build a consistent practice. Adaptive for desktop and Linux phones.

## Features

### Timer
- Countdown, stopwatch, and Box Breath modes
- Box Breath: pick a pattern (4-4-4-4, 4-7-8-0, 5-5-5-5) or dial in each phase; the running view traces a dot around an accent-tinted square as you breathe in, hold, out, hold
- Quick presets plus custom durations
- Per-mode labels: each mode remembers the label you last used for it
- Optional post-session notes
- Pause, resume, discard
- Completion sound — built-in bowls/bell/gong, your own audio, or none
- Daily streak and a system notification when you're away from the app

### Log
- Date-grouped card feed of every session
- Filter by label, or sessions with notes
- Add, edit, or swipe to delete — with undo
- Import from Insight Timer, and CSV import/export for backups

### Stats
- 13-week contribution heatmap, with stars for days that cleared your weekly goal
- Weekly-goal ring showing this week's progress
- Bar or line chart across week / month / 3 months / year
- Per-label breakdown: totals and session counts for each label
- Streak, total time, and session count at a glance

### Preferences
- Weekly goal and completion sound
- Manage your labels and timer presets

### General
- Translated into 10 languages (English, German, Spanish, French, Italian, Dutch, Polish, Brazilian Portuguese, Russian, Simplified Chinese)
- Keyboard shortcuts for the common actions
- Dark-mode and high-contrast safe; follows your system accent colour
- About → Troubleshooting view with a rolling diagnostics log, for attaching to bug reports

## Installation

### Flatpak (recommended)

Pre-built Flatpak bundles for **x86_64** and **aarch64** are attached to every CI run on the [Actions](../../actions) page (and as release assets once the app is on Flathub, install from there instead).

1. Download `meditate-<arch>.flatpak` from the latest passing run.
2. Install and run:

```sh
flatpak install --user meditate-<arch>.flatpak
flatpak run io.github.janekbt.Meditate
```

### Building from source

**Dependencies**

- GTK 4.18+, libadwaita 1.7+, GStreamer (with base plugins)
- [blueprint-compiler](https://gitlab.gnome.org/GNOME/blueprint-compiler) ≥ 0.20
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
meson setup build
ninja -C build
./build/src/meditate          # run from the build directory
```

**Install system-wide**

```sh
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

## Data

Sessions and settings are stored in a SQLite database at
`~/.local/share/meditate/meditate.db` (or the Flatpak equivalent inside the sandbox).

## License

Meditate is free software released under the [GNU General Public License v3.0 or later](COPYING).
