# Tasks: gtk-preferences-one-page

Base: 0e1c5ec0594ec55bcd013fd6afa5b1ca01ee284a; suites on it: GTK 171 green (core and Android untouched by this change)

## Loop
- [x] 0 Skills loaded: `/ponytail ultra`; `slint:slint` (loaded by the gate; no Android UI touched)
- [x] 1 Understand
- [x] 2 Propose (approved: 2026-11-07)
- [x] 3 Build
- [x] 4 Review (R1, five MINOR: see below), then `/simplify` (merged the two sync-status arms, deleted the stale "Labels page removed" note; reuse, efficiency, altitude: nothing). Suite 171 green, clippy clean after it
- [x] 5 Specs, docs, list (check-specs green; no doc names the tabs; OPEN-ISSUES has no entry)
- [x] 6 Verify and hand over (maintainer 2026-11-07: all fine; it opens at Sync but no cursor stays in the field, "thats how I prefer it": delta and comment follow)
- [x] 7 Ship (delta merged into openspec/specs/preferences/spec.md)

## Review Focus
- Sync status click while Preferences is already open -> a second dialog, as today (unchanged)
- Short window -> the page scrolls; the focus jump still lands on Sync
- Another language -> no new strings; "General"/"Data" leave the .po files only if nothing else uses them (grep: nothing else does)
- The source-reading Bluetooth test names `general_page`; it follows the rename

## Tasks
- [x] 1 Update `the_bluetooth_group_holds_the_lead_in` to the single page (failed on the old code: no single page)
- [x] 2 One page in `preferences.rs`; `show_preferences_on_page` -> sync flag; caller in `window/imp.rs`; suite 171 green, clippy clean
- [x] 3 `build-aux/update-translations.sh`; GTK `i18n::` tests; msgfmt check; "General"/"Data" now obsolete (#~) in the nine .po, msgfmt clean, i18n 11 green

## Review R1 (2026-11-07): Quality PASS, five MINOR
- 1 Short window: scroll-to-focus may leave Save below the fold. Kept for the laptop test (short window step); fix only if it shows.
- 2 Focus may lose to libadwaita's first focus: the laptop test decides; the retry sentence left the proposal.
- 3 Proposal named the old function form: fixed.
- 4 Source-order test covers only Statistics < Bluetooth < Backup: rejected, a full source-order assert is a change detector; the scenarios are MANUAL.
- 5 Android's sync status does not jump to Sync: out of scope (delta is Linux only, the apps already differed); shown to the maintainer.

## Hand-over
Artifact: `meditate-gtk/builddir/src/meditate` (2026-11-07 20:23), started with
`XDG_DATA_HOME=/home/janek/.cache/meditate-prefs1 LOCALEDIR=meditate-gtk/builddir/po`.

Test steps (Linux):
1. Menu, Preferences: no tabs; Statistics, Bluetooth, Backup, Migrate, Nextcloud Sync, Danger Zone one below the other.
2. Enter a dummy sync account (any URL, user, password), Save; the headerbar shows the sync status. Click it, then Preferences: the page opens at Nextcloud Sync, cursor at the end of the WebDAV URL, nothing selected.
3. Same with a short window: Sync and its Save button reachable without hunting.
4. Enter Password (only shown when the password is missing): cursor in App password.

Commit message:
```
gtk: one Preferences page

The General and Data tabs split six groups over two pages; they now
sit one below the other. The headerbar sync status opens the page at
Nextcloud Sync.
```
