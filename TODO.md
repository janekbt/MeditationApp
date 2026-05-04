# TODO — small items for later

Polish and UX items to tackle when convenient. Graduate each one out of this file as it lands in a commit — remove the entry rather than letting the list grow stale.

- **Stop bell on Finish / Add overtime.** On the Done page (after tapping Finish during Overtime) and immediately after an "Add X:XX" overtime extender, any currently-playing bell should stop right away. Right now the end bell keeps ringing after the user has already acknowledged the session, which is jarring. Add an explicit stop on the audio sink in both transition paths. Independent of everything else; small fix once the audio handle is reachable from the Done / Add code.

- **Interval bells: "Create new…" top row instead of header `+`.** The interval-bell library page currently uses a `+` action in the header bar to add a bell. Replace with a synthetic "Create new interval bell…" row at the top of the list, matching how the label chooser and sound chooser handle creation. Removes a header-bar affordance, makes the page consistent with the rest of the chooser pages, and is more thumb-reachable on a phone-sized screen.

- **Daily goal (rename from "Weekly goal") + retroactive-vs-future dialog.** Rename the goal knob throughout Stats and Preferences from "Weekly" to "Daily". When the user changes the value, prompt: should the new goal apply only from today onward (leaving historic streak math against the old value) or retroactively (recomputing past days against the new value)? Implies storing goal as a small history `[(effective_from, minutes)]` rather than a single scalar, so the streak/goal-met indicator can read the right value per day. The two-choice dialog is the user-facing change; the goal-history table is the schema change behind it.

- **Mindfulness-bell-during-the-day tab.** *Tentative — revisit after the Guided Meditation pane lands.* New tab next to Timer / Box Breath / Guided. User configures an interval ("every 30 min from 09:00 to 18:00, weekdays only") and a bell sound; the app rings at those times as a presence cue while the user goes about their day. The hard part is background scheduling that survives app backgrounding on the Librem (notifications via `org.freedesktop.Notifications`? a `feedbackd`-driven timer? a small daemon?) — prototype that piece before promoting out of "maybe".

- **Named, full-fidelity presets.** Replace the current duration-only preset chips with named presets that bundle the full configuration: `{duration|∞, waiting, starting bell, completion bell, interval minutes, interval bell, optional label}`. New DB table; "Manage Presets" page reachable from the main menu; long-press / right-click on a chip to "Save current as preset". Land *after* the bell + interval work so the bundle has something to bundle.

- **Guided meditation pane.** New tab next to the merged timer and Box Breath. User imports an audio file; app probes duration via `gst-discoverer`, plays it via `playbin`, auto-labels the session "Guided", and uses the file's duration as the session length. Decision: copy imported files into the app data dir (robust against the user moving them) and offer a "remove guided file" UI. Independent of the other items — can ship any time once gstreamer is in the build.

- **Vibration patterns as a per-bell property.** New `vibration_patterns(uuid, name, pattern_json, is_bundled)` table parallel to `bell_sounds`; `bell_sounds` gains a nullable `vibration_pattern_uuid` column. Pattern is a sequence of `{duration_ms, intensity}` segments — long-pulse, double-buzz, heartbeat-style, etc. Three or four bundled patterns under stable UUIDs (None, Short, Long, Heartbeat). Every bell row (Starting / Interval entries / End) gets a "Vibration on mobile" toggle modeled on the existing show-enable-switch expanders; flipping it on reveals a sub-row pointing at a vibration-pattern chooser built like the sound / label / interval-bell choosers (synthetic "Create new pattern…" top row, per-row tap-to-pick + rename + delete, bundled-rows-non-deletable, custom-first ordering). Bells can also flip a "Mute" toggle so playback is vibration-only — useful in shared spaces. Sync over the existing event log via `vibration_pattern_insert/update/delete` events. The Librem's `org.gnome.SettingsDaemon.Vibrator` D-Bus interface (or `feedbackd` directly) drives playback; laptop preview is a no-op stub. Independent of presets / guided meditation.

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
