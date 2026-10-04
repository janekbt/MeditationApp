# TODO — small items for later

Polish and UX items to tackle when convenient. Graduate each one out of this file as it lands in a commit — remove the entry rather than letting the list grow stale.

- **Done screen note field: auto-grow instead of fixed 180 px scroller.** The Android shell ships an auto-expanding multi-line note input (a single-line `NoteField` Rectangle that grows line by line via the inner `TextInput.preferred-height`) — Janek prefers the layout breathing rather than scrolling inside a small fixed box. The GTK shell still uses `Gtk.ScrolledWindow { height-request: 180; ... TextView }` in `meditate-gtk/data/ui/timer_view.blp`. Replace the inner ScrolledWindow with the bare TextView (or a min-height-only container) so the Done page's vbox naturally extends downward as the user types. The Done page is already inside a Gtk.ScrolledWindow at the page level, so the outer page will scroll if the content runs past the viewport — no extra plumbing needed. Touch points: `meditate-gtk/data/ui/timer_view.blp` (drop the inner ScrolledWindow, keep `note_view` directly; drop `height-request`); double-check the `log-note-editor` CSS class still draws the rounded background without the inner scroller.

- **Source bundled Box Breath voice-cue sounds.** The Box Breath phase chooser filters to `category = 'box_breath'` and starts empty for new users — every bundled sound is in the general category. Source / record four short voice cues: **"Inhale"**, **"Hold"** (full), **"Exhale"**, **"Hold"** (empty). Reusing one "Hold" recording across the two hold phases is fine. Soft-spoken, mono, OGG/Vorbis like the existing bundled bells. Touch points: `meditate-gtk/data/sounds/<name>.ogg` + `meditate-gtk/data/io.github.janekbt.Meditate.gresource.xml`, the Android copies in `meditate-android/assets/sounds/` + `meditate-android/src/sounds.rs`, and the bundled-sounds seed list in `meditate-core/src/db/seeds.rs` (each row `BellSoundCategory::BoxBreath`, fresh stable UUIDs). Free-license source preferred (CC0 / CC-BY) — `meditate-gtk/data/sounds/CREDITS.md` records attribution. Until this lands, the chooser shows its "No sounds for this category yet" empty state.

- **Surface stale / missing bell-sound references at setup time, not session start.** Today, if a bell row's `sound` UUID points at a bundled-sound row that was deleted (e.g. a sync deleted a custom sound the user had picked), the bell silently produces no audio when its trigger fires — the user only finds out mid-meditation. Should refuse to play AND show the issue while the user is still in setup: red `.warning` chip on the bell row's subtitle ("Sound missing — pick another"), block Start Session if any active bell has a missing sound, error banner on the timer setup page summarising affected bells. Touch points: `meditate_core::bells::sound_name` already reports a stale UUID as missing — build the setup-time check on it in core, then decorate the rows in `meditate-gtk/src/timer/imp.rs::refresh_starting_bell_sound_subtitle` + `refresh_end_bell_sound_subtitle` + `meditate-gtk/src/bells.rs::sound_name`, gate session start, and mirror it in the Android shell. Same UX pattern will apply when vibration patterns ship — set the precedent here so the vibration version inherits it.

- **Daily goal — goal-history table + retroactive-vs-future dialog.** Replace the single `daily_goal_mins` setting with a small history table: `goal_history(effective_from TEXT NOT NULL, minutes INTEGER NOT NULL)` keyed by date. When the user changes the daily goal, prompt: apply only from today onward (append a new row whose `effective_from` is today) or retroactively (replace the most recent row's value). The contribution grid + any goal-met indicator queries the table for the value effective on each day rather than reading a single scalar. Two-choice AlertDialog is the user-facing change; the goal-history table + the per-day lookup helper is the schema/logic change behind it. Sync via the event log (`goal_history_insert / _update`). Needs a migration that seeds the table from the existing `daily_goal_mins` setting, tested against the previous release. Both shells.

- **Make `meditate_core::preset_config::apply` atomic via nestable transactions.** Currently apply runs each underlying `db.set_setting` / `db.delete_interval_bell` / `db.insert_interval_bell` / etc. inside its own `unchecked_transaction()` — if any write fails partway, settings + interval-bell library + box-breath-phase rows can end up out of sync with the cfg. Bug-for-bug parity with the prior gtk implementation but worth fixing once meditate-core has nestable-transaction infrastructure. Two real paths: (a) refactor every mutating method on `meditate_core::db::Database` from `unchecked_transaction()` to `savepoint()`, which requires changing the receiver from `&self` to `&mut self` and rippling that through the `with_db` API in gtk; (b) hand-roll a `SavepointGuard` helper that emits `SAVEPOINT … RELEASE … ROLLBACK TO` via raw `conn.execute()`, keeping `&self` signatures. Then apply (and any future multi-method transactional caller) wraps in `db.run_in_transaction(|db| { ... })`. Estimated ~30 method touchpoints either way.

- **Mindfulness-bell-during-the-day tab.** *Tentative.* New tab next to Timer / Box Breath / Guided. User configures an interval ("every 30 min from 09:00 to 18:00, weekdays only") and a bell sound; the app rings at those times as a presence cue while the user goes about their day. The hard part is background scheduling that survives the app being backgrounded — on the Librem (notifications via `org.freedesktop.Notifications`? a `feedbackd`-driven timer? a small daemon?) and on Android (exact alarms) — prototype that piece before promoting out of "maybe".

- **Android: rename and delete for custom bell sounds.** GTK's sound chooser lets the user rename and delete imported bell sounds; the Android bell chooser is select + import only. Core already has the rename / delete paths (with sync events), so this is shell UI. Android only.

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
