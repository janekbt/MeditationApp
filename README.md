# Meditate

A meditation timer and session log for GNOME.

Countdown and stopwatch, a browsable log, and weekly-goal stats to help you build a consistent practice. Adaptive for desktop and Linux phones.

## Features

### Timer
- Countdown and stopwatch modes
- Quick presets plus custom durations
- Labels and optional post-session notes
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
- Streak, total time, and session count at a glance

### Preferences
- Weekly goal, completion sound, running-average period
- Manage your labels and timer presets

### General
- Translated into 10 languages (English, German, Spanish, French, Italian, Dutch, Polish, Brazilian Portuguese, Russian, Simplified Chinese)
- Keyboard shortcuts for the common actions
- Dark-mode and high-contrast safe; follows your system accent colour

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

- GNOME Platform / SDK 50 (GTK 4.18+, libadwaita 1.7+, GStreamer)
- [blueprint-compiler](https://gitlab.gnome.org/GNOME/blueprint-compiler) ≥ 0.20
- Rust (stable toolchain) + Cargo
- Meson ≥ 0.62

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

```sh
flatpak-builder --user --install --force-clean flatpak_app \
    build-aux/io.github.janekbt.Meditate.json
flatpak run io.github.janekbt.Meditate
```

**Cross-compile for aarch64 (developer iteration)**

If you're working on Linux-phone perf, `build-aux/dev-xbuild.sh` cross-compiles a Librem 5–compatible binary in ~15 seconds on an x86_64 host — avoiding the 20–35 minute `flatpak-builder --arch=aarch64` QEMU build. Output goes to `target/aarch64-unknown-linux-gnu/release/meditate`, ready to `scp` straight over a Flatpak-installed binary on the phone for testing. One-time prerequisites are documented at the top of the script.

## Data

Sessions and settings are stored in a SQLite database at
`~/.local/share/meditate/meditate.db` (or the Flatpak equivalent inside the sandbox).

## License

Meditate is free software released under the [GNU General Public License v3.0 or later](COPYING).
