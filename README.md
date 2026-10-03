# Meditate

A meditation timer and session log for Android and Linux.

Countdown and stopwatch, a browsable log, and daily-goal stats to help you build a consistent practice. Adaptive for desktop and Linux phones, with a native Android app that syncs over your own Nextcloud.

## Features

### Timer
- Countdown, stopwatch, Box Breath, and Guided modes
- Box Breath: pick a pattern (4-4-4-4, 4-7-8-0, 5-5-5-5) or dial in each phase; the running view traces a dot around an accent-tinted square as you breathe in, hold, out, hold
- Guided: play your own guided-meditation audio; the end bell rings when the track finishes
- Bells: an optional starting bell, interval bells (at fixed times, every N minutes, or randomised), and a per-mode end bell — each as sound, vibration, or both, with bundled or imported sounds, a volume slider for every bell, and a visual vibration-pattern editor
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
- On [F-Droid](https://f-droid.org/packages/io.github.janekbt.Meditate/); build instructions in [`BUILDING.md`](BUILDING.md#android)

### General
- Translated into 10 languages (English, German, Spanish, French, Italian, Dutch, Polish, Brazilian Portuguese, Russian, Simplified Chinese) — on Linux and Android alike
- Keyboard shortcuts for the common actions
- Dark-mode and high-contrast safe; follows your system accent colour on Linux — on Android, pick from six accent colours in the app
- About → Troubleshooting view with a rolling diagnostics log, for attaching to bug reports
  - Log file lives at `~/.var/app/io.github.janekbt.Meditate/data/meditate/diagnostics.log` on Flatpak, `~/.local/share/meditate/diagnostics.log` otherwise — useful when the About dialog itself can't be opened

## Installation

### Android

Install **[Meditate from F-Droid](https://f-droid.org/packages/io.github.janekbt.Meditate/)**.
F-Droid rebuilds every release from source and verifies it matches the APK
published here, so you get the same signed app either way; updates arrive
through F-Droid.

The APK is also attached to every [release](../../releases) as
`Meditate-<version>.apk` (same signature as F-Droid's — the two update each
other). Unreleased builds from CI (`meditate-android.apk` on the
[Actions](../../actions) page) are signed with a throwaway debug key: they
can't update an installed release, and installing one over a release
requires uninstalling it first, which deletes the app's data.

### Linux (Flatpak)

Pre-built Flatpak bundles for **x86_64** and **aarch64** are attached to every
[release](../../releases) as `meditate-<version>-<arch>.flatpak`. (Flathub is
not an option for this app.) CI runs attach the same bundles on the
[Actions](../../actions) page for unreleased builds; those expire after 90
days and need a GitHub login.

```sh
flatpak install --user meditate-<version>-<arch>.flatpak
flatpak run io.github.janekbt.Meditate
```

### Building from source

```sh
./build.sh gtk --debug       # Linux app → ./meditate-gtk/builddir/src/meditate
./build.sh android --debug   # Android APK → meditate-android/android/app/build/outputs/apk/debug/app-debug.apk
```

Drop `--debug` for optimized builds. `build.sh` installs missing
dependencies on Debian/Ubuntu, Fedora and Arch (it asks for your password
once) and checks the GTK 4.18 / libadwaita 1.7 floor before compiling. The
Android build needs **JDK 17 exactly** — see [`BUILDING.md`](BUILDING.md#android),
which is the full reference: prerequisites, the Flatpak build, tests,
translations, the Librem 5 cross-build and CI.

The workspace has three crates: the portable
[`meditate-core`](meditate-core/README.md) (persistence, sync and session
logic — most non-UI contributions land here), the GTK app in
[`meditate-gtk/`](meditate-gtk/) and the Android app in
[`meditate-android/`](meditate-android/). See
[`ARCHITECTURE.md`](ARCHITECTURE.md) for the full map.

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
