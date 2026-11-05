# Sync status on tap

## Why
Only a tooltip tells you why the sync symbol shows a warning. On Linux
you see it only by hovering with a mouse. On Android the text is only
an accessibility label, so on a phone nobody ever learns what went
wrong. Tapping does something different in each state (retry, open the
settings, open the recovery dialog) and never explains why. The reason
for a failure is also stored as English text. Batch W (#164, #165)
needs a visible place for "skipped" items, so this change comes first.

## What Changes
- Clicking or tapping the sync symbol opens a status panel: a popover
  under the button on Linux, a Material dialog on Android. It shows the
  current state, the reason or the list of files, and buttons for the
  next step. A wiped server still opens the recovery dialog directly.
- A failed sync stores a typed reason, and each app shows it in its own
  language. The English detail stays in the diagnostics log, which
  already gets it (`sync.attempt err=...` in both apps).
- The Linux tooltip and the Android accessibility label show the
  panel's title. They no longer say "click/tap to retry", which stops
  being true.
- Left out: batch W's skipped items (batch W adds them later).

## Decisions
- Way A: this change comes first, and batch W later adds its lines to
  this panel (maintainer).
- Reason: a short translated reason, with the English detail in the log
  only (maintainer).
- Green status: the panel with "Synced N ago" and the buttons
  Preferences and Sync Now (maintainer).
- Look: a popover on Linux and a dialog on Android (maintainer accepted
  the mock-up).
- Each state shows this (my call):

  | State | Title | Below | Buttons |
  |---|---|---|---|
  | Syncing | Syncing with Nextcloud… (existing) | none | Preferences |
  | Never synced yet | Sync configured (waiting for first run) (existing) | none | Preferences, Sync Now |
  | Synced | Synced N ago (existing) | none | Preferences, Sync Now |
  | Failed | Last sync failed | reason, if known | Preferences, Sync Now |
  | Not uploaded | Some files weren't uploaded | one line per file (existing) | Preferences, Sync Now |
  | No password | No password saved | none | Enter Password |
  | Server wiped | recovery dialog, as today | | |

- Sync Now does what tapping does today: it clears the refused-upload
  list, starts a sync and closes the panel (my call).
- An open panel follows each state change, through the existing 2 s
  refresh on Linux and the sync start and end refresh on Android. On
  Android, "Synced N ago" may stay a minute behind while the dialog is
  open. When the server turns out wiped or the account is removed, the
  panel closes; the next tap opens the recovery dialog (my call).
- Linux keeps a plain button with a popover attached and opens it from
  the click handler, so a wiped server can go straight to the recovery
  dialog (my call).
- The Android dialog has a Close button (existing string "Close"), as
  do tapping outside it and Back (maintainer). With three buttons they
  stack, one per line and right-aligned, as Material's stacked dialog
  buttons do. With two buttons they sit side by side (my call).
- Reasons, from the sync's error (my call):

  | Error | Reason shown |
  |---|---|
  | no connection, timeout | Network error (existing) |
  | 401 | Authentication failed (existing) |
  | a server status, including 404 and 429 | Server error ({status}) |
  | a write conflict (405, 409 and 412 share one error) | Server error (existing) |
  | a redirect | The server redirected the address (maintainer, after the review) |
  | a too-large or unreadable reply, an unreadable batch file | The server's reply can't be read |
  | a local database error | Local database error |
  | the password can't be read from the keyring | The password couldn't be read |
  | a stored error from before this change | no reason line |

## Capabilities
### Modified Capabilities
- `sync`: the sync symbol shows a visible status with a translated
  reason.

## Impact
- Core: `sync/settings.rs` (the stored reason in the error-kind tag),
  `sync/runner.rs` (record the reason), and `sync/indicator.rs` (the
  reason in `Error`, the panel's buttons, and the tap action:
  `OpenRecovery` or `ShowStatus`).
- GTK: `data/ui/window.blp` (a popover attached to the button) and `src/window/imp.rs` (fill the popover, the tooltip).
- Android: `ui/main.slint` (a new `Modal.sync-status` dialog, declared
  after the app bar, plus the Tr functions) and `src/ui.rs` (fill it,
  wire the tap and the buttons).
- Translations: new in both apps, all nine languages: "Last sync
  failed", "Some files weren't uploaded", "No password saved", "Enter
  Password", "Sync Now", "Server error ({status})", "The server's reply
  can't be read", "Local database error", "The password couldn't be
  read", "The server redirected the address". A long body scrolls
  (240 px) in both apps. Removed in both apps: "Last
  sync failed, click/tap to retry\n{}", "Some files weren't uploaded,
  click/tap to retry\n{}", "No password saved, click/tap to enter it".
- Data: a new set of values in the local, unsynced error-kind tag. An
  old value reads as "no reason". No schema change, no sync change, no
  migration.
- Android build environment: untouched.
- Docs: `openspec/specs/sync/spec.md` (through the delta), and the
  OPEN-ISSUES batch W note.
