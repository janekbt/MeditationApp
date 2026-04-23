# TODO — small items for later

Polish and UX items to tackle when convenient. Graduate each one out of this file as it lands in a commit — remove the entry rather than letting the list grow stale.

- **Log edit: calendar button renders as a black square.** When editing a log entry, the button that opens the date picker shows as a solid black rectangle instead of a calendar icon. Likely a missing/wrongly-named symbolic icon reference in the edit-entry blueprint — check the `icon-name` on the date button and confirm the icon is shipped in the gresource bundle (or falls back to a stock Adwaita one).

- **Session-end screen: note field auto-focuses and pulls up the on-screen keyboard, covering Save/Discard.** When a session ends, the post-session note entry grabs focus automatically, which on phones triggers the OSK and hides the action buttons below. Drop the `grab_focus()` (or `has-focus`/`can-focus` wiring) on the note field so Save/Discard stay visible; user can tap the field explicitly if they want to type a note.

- **In-app changelog shows initial-release text, not the current release's.** Wherever the app displays its changelog (About dialog / "What's new" screen / similar), it's rendering the 26.4.1-era copy instead of the 26.4.3.1 entry in `data/io.github.janekbt.Meditate.metainfo.xml.in`. First places to look: any code that parses the metainfo `<release>` list (likely picks the *last* entry instead of the *first*, or sorts ascending instead of descending), or a hardcoded string bundled from an older version. Verify the metainfo itself orders releases newest-first (it does) and that the renderer takes `releases[0]`, not `releases[-1]`.

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
