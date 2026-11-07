# Proposal: gtk-preferences-one-page

## Why
On Linux the Preferences dialog splits six groups over two tabs (General,
Data). The maintainer wants them on one page, one below the other.

## What changes
- GTK only: one `AdwPreferencesPage`, groups in today's order: Statistics,
  Bluetooth, Backup, Migrate, Nextcloud Sync, Danger Zone. With one page
  libadwaita hides the dialog's tab switcher itself.
- The headerbar sync status (Preferences / Enter Password) used to open the
  Data tab; it now opens the page scrolled to Nextcloud Sync (maintainer's
  pick). On the laptop no cursor stays in the field; the maintainer prefers
  that, so the delta says so.
- `show_preferences_on_page(app, Option<&str>)` becomes a flag for "go to
  sync"; the page titles "General" and "Data" leave the code.

## Places it reaches
- `meditate-gtk/src/preferences.rs`, `meditate-gtk/src/window/imp.rs` (caller).
- Translations: no new strings; the two unused msgids drop out of
  `meditate.pot` and the nine `.po` via `build-aux/update-translations.sh`.
- Android: none (already one page). Core, migration, Android build env: none.
- Docs: grep found no doc naming the two tabs; the comments in
  `preferences.rs` get updated.

## Dropped
- No scroll position memory, no search, no regrouping.

## Decisions
- Sync status click: scroll to Nextcloud Sync, focus the URL field (maintainer).
- Minor (my call): the page keeps no title; focus via `grab_focus` after
  `present`, the page's viewport scrolls the focused row into view.
- Proposed (pre-build agent): "Enter Password" focuses the App password
  row instead of the URL, since that is what is missing. The cursor lands
  without selecting the saved text, so a keystroke can't wipe it.
- My call: `show_preferences(app)` stays; `show_preferences_at(app,
  Option<SyncField>)` replaces `show_preferences_on_page`. The grab runs
  once from an idle callback after `present`; the laptop test shows
  whether libadwaita's own first focus wins.
