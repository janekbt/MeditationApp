# Nextcloud URL help

## Why
GitHub issue #18: the sync settings ask for a "Server URL", but the app
needs the full WebDAV address and an app password made in Nextcloud. A
casual Nextcloud user can't tell either from the screen.

## What Changes
- The first field is labelled "WebDAV URL" in both apps.
- One help sentence under the group description says where Nextcloud
  shows the WebDAV address and where to create the app password.
- Left out: building the WebDAV path from a plain server address. The
  name in the path isn't always the login name (login by e-mail), other
  WebDAV servers would get a wrong address, and saved accounts would
  change meaning.

## Decisions
- Label + help text, no example address (Janek, 2026-11-02).
- English: "Use the WebDAV address from Files → Files settings, and an
  app password from Personal settings → Security." on Linux; Android writes ">"
  for "→", since the phone's font has no arrow (my call).
- GTK appends the sentence to the group description as its own catalog
  text, so the existing description keeps its nine translations (my call).
- The menu names in the translations follow Nextcloud's own wording in
  that language where I know it (my call).

## Capabilities
### Modified Capabilities
- `sync`: the settings say which address and password to enter

## Impact
- Core: none
- GTK: `src/preferences.rs`, `po/` (template + nine catalogs)
- Android: `ui/main.slint`, `lang/*/LC_MESSAGES/meditate-android.po`
- Translations: "WebDAV URL" and the help sentence, nine languages, both apps
- Data: none (the saved address is used as before)
- Android build environment: untouched
- Docs: none name the label
