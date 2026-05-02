# TODO — small items for later

Polish and UX items to tackle when convenient. Graduate each one out of this file as it lands in a commit — remove the entry rather than letting the list grow stale.

- **Nextcloud sync (option C: append-only event log).** Periodic auto-sync of session/label data between the user's devices via personal Nextcloud, robust under offline edits on multiple devices. See `Nextcloud-Sync.md` for the detailed plan: architecture, schema changes, WebDAV protocol, conflict rules, ~12 TDD cycles across 4-6 sessions.

- **Starting bell, waiting period, interval bells (Countdown mode).** Three new options on the timer setup page:
  - *Waiting period*: silence for N seconds before the start bell, lets the user settle. 0 = off.
  - *Start bell*: rings at t=0 (after the waiting period). Pickable from the bundled bell library.
  - *Interval bells*: ring every N minutes during the session. Independent sound from start/end.
  Use gstreamer (`playbin`) for playback. Schedule via glib timeouts; cancel cleanly on stop/pause.

- **Bundled bell sound library.** Ship 6–10 CC0 sounds covering the major traditions: Tibetan singing bowl (small + large), Zen bell (Japanese), Burmese/Thai gong, tingsha, Indian brass bell, soft chime/pling. Source from freesound.org under CC0 with proper `data/sounds/CREDITS.md`. Bundle via gresource so flatpak needs no extra paths.

- **Named, full-fidelity presets.** Replace the current duration-only preset chips with named presets that bundle the full configuration: `{duration|∞, waiting, start bell, end bell, interval minutes, interval bell, optional label}`. New DB table; "Manage Presets" page reachable from the main menu; long-press / right-click on a chip to "Save current as preset". Land *after* the bell + interval work so the bundle has something to bundle.

- **Guided meditation pane.** New tab next to the merged timer and Box Breath. User imports an audio file; app probes duration via `gst-discoverer`, plays it via `playbin`, auto-labels the session "Guided", and uses the file's duration as the session length. Decision: copy imported files into the app data dir (robust against the user moving them) and offer a "remove guided file" UI. Independent of the other items — can ship any time once gstreamer is in the build.

## Closed as "not us to fix" — Phosh launcher splash for flatpak apps

On Librem 5 (Phosh 0.34 / Phoc 0.33, PureOS Crimson), the launcher
splash (app icon + spinner while loading) shows for APT-installed
apps like `org.gnome.clocks` and `org.gnome.Console` but not for
flatpak-installed apps like ours. We verified this is **not**
caused by anything in our `.desktop` file:

Tried on-device, no change:
- `StartupNotify=true` alone
- `StartupNotify=true` + `X-Purism-FormFactor=Workstation;Mobile;`
- `StartupNotify=true` + `X-Purism-FormFactor=...` + `X-Phosh-UsesFeedback=true`

Confirmed the same launcher-splash absence for `org.localsend.localsend_app` (the other flatpak app installed on the test device), ruling out a Meditate-specific bug.

Root cause is Phosh's splash not firing for flatpak-activated apps
on this release — likely an issue with the `xdg_activation_v1`
token propagation through `flatpak run`'s D-Bus activation path,
or simply a feature Phosh hasn't implemented for flatpaks yet.
File upstream with Phosh if we want this fixed.
