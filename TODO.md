# TODO — small items for later

Polish and UX items to tackle when convenient. Graduate each one out of this file as it lands in a commit — remove the entry rather than letting the list grow stale.

- **Nextcloud sync (option C: append-only event log).** Periodic auto-sync of session/label data between the user's devices via personal Nextcloud, robust under offline edits on multiple devices. See `Nextcloud-Sync.md` for the detailed plan: architecture, schema changes, WebDAV protocol, conflict rules, ~12 TDD cycles across 4-6 sessions.

- **Bells: starting bell, preparation time, interval bells, shared bell library, custom-file import, sync.** Six-phase rollout — each phase shippable on its own beta commit; aggregate ~8–10 commits.

  Decisions locked (2026-05-02):
  - Data model: new `bell_sounds(id, uuid, name, file_path, is_bundled, mime_type, created_iso)` SQLite table mirroring `labels`. New event kinds `bell_insert`, `bell_update`, `bell_delete`. Sessions reference a bell by `bell_uuid`, never by file path.
  - File storage: `$XDG_DATA_HOME/io.github.janekbt.Meditate/sounds/<uuid>.<ext>` locally; `Meditate/sounds/<uuid>.<ext>` on WebDAV — siblings of the existing `<lamport>__<batch>.json` event-log files.
  - Size cap: 10 MB per file (rejected with a toast on import; refused on pull from remote).
  - Chooser shell: `Adw.NavigationPage` pushed onto `nav_view` from the bell-selection row. Same view re-used as a new tab in the Preferences window for delete/rename management (no selection semantics there).
  - HIG wording on every new switch: titled noun + explanatory subtitle, never a question. "Starting Bell" / "Preparation Time" / "Interval Bell" — not "Use Starting Bell?".
  - The existing Completion Sound combo (timer setup row + Preferences sound chooser) goes away once B.4 lands; sessions store `completion_bell_uuid` referencing the shared library instead.

  Phases:

  - **✓ B.1 — Setup-page progressive-disclosure UI shell.** Landed on `beta` as `9e2f3ed..e75f549`. Used nested `Adw.ExpanderRow` (rather than `SwitchRow`) for the native slide animation; inner chevron suppressed via a one-line override in `data/style.css` to dodge a libadwaita 1.7 descendant-selector quirk on nested expander chevrons.

  - **✓ B.2 — Wire starting bell playback + prep-time delay.** Landed on `beta` as `e6c83de..11782d4`. Box Breathing is treated as fully separate (no starting bell, no prep). Stop during prep saves a real session row with duration = prep elapsed. Tick dispatches on `TimerState::{Preparing, Running}`; bell-cut polish via `sound::stop_all()` in Save/Discard.

  - **✓ B.3 — Interval bell library.** Landed on `beta` as `63f6b61..9bbf05c` (10 commits). User-managed library with three kinds (Interval with jitter, Fixed-from-start, Fixed-from-end), per-bell sound + enabled flag, master toggle in the timer setup. Adw.NavigationPage shell (list + edit), inline red-trash delete, save-as-you-go. Stopwatch mode greys out fixed-from-end (UI + tick). Concurrent bell playback via `INTERVAL_MEDIA: Vec<MediaFile>` so two cues colliding don't clip each other.

  - **B.4 — Bell library + bundled sounds.**
    - Land the `bell_sounds` table + the three new event kinds.
    - Seed 6–10 CC0 bundled bells on first run with stable UUIDs so they sync deduplicated across devices: Tibetan singing bowl (small + large), Zen bell, Burmese/Thai gong, tingsha, Indian brass bell, soft chime/pling. Sourced from freesound.org under CC0; credits in `data/sounds/CREDITS.md`.
    - Bell chooser as `Adw.NavigationPage` with per-row Play/Stop preview, reachable from every bell-selection row in the timer setup.
    - Same view added as a new `Adw.PreferencesPage` tab (`Sounds`?) in the Preferences window: delete + rename actions, no selection semantics.
    - Existing standalone Completion Sound combo + Preferences sound chooser deleted in this same commit.

  - **B.5 — Custom bell file import.**
    - "Choose your own…" entry at the top of the bell list → `Gtk.FileDialog`.
    - 10 MB size cap with toast on rejection.
    - Confirmation dialog: editable name field + the chosen file's filename as a preview.
    - Confirm: copy file into `$XDG_DATA_HOME/.../sounds/<uuid>.<ext>`, insert `bell_sounds` row, fire `bell_insert` event.
    - Local-only at this stage; B.6 makes it sync.

  - **B.6 — Bell file sync over WebDAV.**
    - Extend orchestrator: alongside the existing JSON event-log push/pull, push each local-only file to `Meditate/sounds/<uuid>.<ext>` and pull each remote-only file referenced by a `bell_insert` we've ingested.
    - New `known_remote_sounds` tracking table (mirrors `known_remote_files`).
    - 10 MB size cap re-enforced on inbound; refuse to pull anything bigger and surface a toast.
    - ~3–4 commits on its own — TDD by the book against `FakeWebDav`.

- **Sit-longer mode: keep going past the countdown.** When a countdown reaches 0:00, instead of auto-finishing, switch into open-ended elapsed mode (or just keep the tick going as if stopwatch had been on the whole time). User stops manually when they're done. Interval bells continue firing throughout. Useful for "I planned 30 min but I'm actually settled, give me more". Independent of presets — small change in `tick_running`'s done branch.

- **Named, full-fidelity presets.** Replace the current duration-only preset chips with named presets that bundle the full configuration: `{duration|∞, waiting, starting bell, completion bell, interval minutes, interval bell, optional label}`. New DB table; "Manage Presets" page reachable from the main menu; long-press / right-click on a chip to "Save current as preset". Land *after* the bell + interval work so the bundle has something to bundle.

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
