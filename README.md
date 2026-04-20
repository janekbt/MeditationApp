# Meditate

A simple meditation timer and session log for GNOME.

Meditate provides a countdown and stopwatch timer, a browsable session log, and statistics to help you build a consistent practice. It is designed to work on both desktop and mobile (360 px minimum width).

## Features

### Timer
- **Countdown** and **Stopwatch** modes
- Hours and minutes spin buttons for precise durations
- Configurable quick-preset buttons (1–5 presets, default: 5 / 10 / 15 / 20 / 30 min)
- Label selection before starting
- Daily streak display
- Full-screen running view with large time display
- Pause, Resume, and Stop controls
- Post-session form: optional note, label confirmation, Save or Discard
- Completion sound: Singing Bowl, Bell, Gong, custom audio file, or none
- System notification when the timer ends (only fired when the app is in the background)

### Log
- Scrollable session history
- Each row shows duration, date, label, and a note preview
- Swipe to delete with undo toast
- Add or edit sessions manually
- Filter by label or "only sessions with notes"

### Stats
- Monthly calendar — one dot per day with at least one session
- Bar chart with Daily / Weekly / Monthly / Yearly toggle
- Text stats: running average, longest streak, total meditation time

### Preferences
- Completion sound picker with live preview
- Running-average period (7, 14, or 30 days)
- Label management — create, rename, delete
- Timer preset editor — set 1–5 custom durations

### General
- Adaptive layout — works at 360 px (Linux phones) and scales to desktop
- Dark mode and high-contrast safe (uses `@accent_color` throughout)
- Keyboard shortcuts: `Space` start/pause/resume, `Ctrl+,` preferences, `Ctrl+?` shortcuts, `Ctrl+Q` quit

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

## Data

Sessions and settings are stored in a SQLite database at  
`~/.local/share/meditate/meditate.db` (or the Flatpak equivalent inside the sandbox).

## License

Meditate is free software released under the [GNU General Public License v3.0 or later](COPYING).
