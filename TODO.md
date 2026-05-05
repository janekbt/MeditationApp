# TODO — small items for later

Polish and UX items to tackle when convenient. Graduate each one out of this file as it lands in a commit — remove the entry rather than letting the list grow stale.

- **Guided mode: label toggle on by default.** In Guided mode the Setup view's Label expander currently starts with `enable-expansion = false` like every other mode — the user has to flip it on for each session. Default it ON in Guided so a fresh session auto-tags with the seeded "Guided Meditation" label (UUID `DEFAULT_GUIDED_LABEL_UUID`). Touch point: `src/timer/imp.rs::apply_preferred_label_for_mode` (or wherever the per-mode default is read on visit) — for the Guided arm, fall back to `label_active = true` instead of `false` when no persisted setting exists. Persisted user choice still wins on subsequent visits, so the default only fires on first-ever entry into Guided mode (or after a DB wipe).

- **Daily goal — rename from "Weekly goal" + reorient the math.** Rename the goal knob throughout Stats and Preferences from "Weekly" to "Daily". The hero ring on the Stats tab currently shows week-total vs the weekly setting; switch to today-total vs the daily setting. The contribution grid currently derives a daily target via `weekly_goal_mins / 7` and colours each cell against that — after the transition the daily setting feeds in directly with no /7 step. Touch points: `src/preferences.rs` (SpinRow title + setting key + adjustment range — current 30..1000 weekly becomes something like 5..180 daily, default 30), `src/stats/imp.rs::reload_goal_ring` and `reload_contrib_grid` (read `daily_goal_mins`, drop the `/7` derivation), accessibility strings ("Weekly goal: …" → "Daily goal: …"). Setting key change forces a DB wipe (no compat shim per the standing rule).

- **Daily goal — goal-history table + retroactive-vs-future dialog.** Builds on the rename above. Replace the single `daily_goal_mins` setting with a small history table: `goal_history(effective_from TEXT NOT NULL, minutes INTEGER NOT NULL)` keyed by date. When the user changes the daily goal, prompt: apply only from today onward (append a new row whose `effective_from` is today) or retroactively (replace the most recent row's value). The contribution grid + any goal-met indicator queries the table for the value effective on each day rather than reading a single scalar. Two-choice AlertDialog is the user-facing change; the goal-history table + the per-day lookup helper is the schema/logic change behind it. Sync via the event log (`goal_history_insert / _update`). Independent of the rename — but only meaningful once the rename has landed.

- **Mindfulness-bell-during-the-day tab.** *Tentative — revisit after the Guided Meditation pane lands.* New tab next to Timer / Box Breath / Guided. User configures an interval ("every 30 min from 09:00 to 18:00, weekdays only") and a bell sound; the app rings at those times as a presence cue while the user goes about their day. The hard part is background scheduling that survives app backgrounding on the Librem (notifications via `org.freedesktop.Notifications`? a `feedbackd`-driven timer? a small daemon?) — prototype that piece before promoting out of "maybe".

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
