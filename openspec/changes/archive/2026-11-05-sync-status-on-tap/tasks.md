# Tasks: sync-status-on-tap

Base: 1eda85f1a1e61d11bc6fa004147ef6950ebbaa7b; suites on it: green (core 1454, android 287; `cargo test -p meditate-core -p meditate-android --lib`). GTK on the base: green (168 tests, package meditate).

## Loop
- [x] 0 Skills loaded, again after every /compact: `/ponytail ultra`; `slint:slint` (Android UI touched)
- [x] 1 Understand
- [x] 2 Propose (approved: 2026-11-03, with a Close button on Android)
- [x] 3 Build (MANUAL checks of 3.1/4.1 wait for step 6)
- [x] 4 Review, then `/simplify` after the last review (review, fixes R1-R6, fix review PASS, /simplify applied)
- [x] 5 Specs, docs, list (check-specs ok; OPEN-ISSUES batch W note; ARCHITECTURE and core README still right; the delta merges at ship)
- [x] 6 Verify and hand over (maintainer tested on the Fairphone and Linux 2026-11-05: all steps pass)
- [x] 7 Ship (delta merged, archived)

## Review Focus
- A database from 26.11.0 holding an error with an empty kind tag -> "An error stored by the previous release"
- `remote_data_lost` and `password_missing` kinds keep working next to the new reason values -> "Tapping after the server was wiped", "Tapping without a saved password"
- The panel stays open while a sync starts or ends (it must follow the state, never show stale buttons) -> refresh on each state change; wiped or Hidden closes it; manual check in step 6
- Sync Now while a sync is already in flight -> not offered in Syncing; `trigger_sync` already ignores a second start
- A long file name or a long translation in the not-uploaded list at 360 px -> MANUAL scenario (German, Russian)
- Keyring failure -> its own reason, "The server answers with an error status" (each_failure_records_its_reason)
- Hidden state (no account): no symbol, nothing to tap -> unchanged, existing `derive_hides_when_no_account`

## 1. Core: the reason
- [x] 1.1 A typed `SyncFailureReason` (Network, Unauthorized, NotWebDav, Server(u16), UnreadableReply, LocalDatabase, Keyring), stored in the error-kind tag (`network`, `unauthorized`, `not_webdav`, `server:<status>`, `unreadable`, `database`, `keyring`); the English text stays in `KEY_LAST_SYNC_ERROR` (`derive` reads failure from it). `record_outcome` maps `RunError` to it (Server/Conflict/RateLimited → Server with their status: 412, 429; NotFound → NotWebDav, since a per-file 404 never reaches the status; Redirected/ResponseTooLarge/MalformedResponse/SyncError::InvalidEvent → UnreadableReply; RunError::Db/SyncError::Db → LocalDatabase; Keyring → Keyring; OpenDb records nothing, as today). Rewrite `a_keyring_failure_shows_in_the_status` to expect the Keyring reason. Files: `meditate-core/src/sync/settings.rs`, `runner.rs`. Proof: `a_network_failure_records_its_reason`, `each_failure_records_its_reason`. Expected: FAIL then PASS
- [x] 1.2 `SyncIndicatorState::Error` carries `reason: Option<SyncFailureReason>` in place of the English `detail`; an unknown or empty tag gives `None`. Files: `indicator.rs`. Proof: `an_error_from_before_reasons_shows_no_reason`. Expected: FAIL then PASS

## 2. Core: what a tap does and which buttons show
- [x] 2.1 `action_for` returns `OpenRecovery` or `ShowStatus`; `status_buttons(&state)` returns the buttons in the proposal's table (`Preferences`, `SyncNow`, `EnterPassword`). Files: `indicator.rs`. Proof: `a_good_sync_offers_preferences_and_sync_now`, `a_running_sync_offers_no_second_sync`, `refused_files_are_listed_with_sync_now`, `a_missing_password_offers_only_enter_password`; `action_for_data_lost_error_opens_recovery` already exists (passes at once: existing behaviour). Rewrite `action_for_other_error_retries` and `action_for_non_error_states_opens_prefs` to the new two actions. Expected: FAIL then PASS

## 3. GTK
- [x] 3.1 `sync_status_btn` stays a `Button`; a `Popover` parented to it, opened with `popup()` from the click handler: title label, a body label (wrap, selectable off), and a button row; `refresh_sync_status` fills it and sets the tooltip to the title; the button handlers: Preferences → `show_preferences_on_page(.., "data")`, Sync Now → `clear_refused_uploads` + `trigger_sync` + popdown, Enter Password → preferences on Data; a wiped server → `recovery_dialog::show` instead of the popover. Files: `meditate-gtk/data/ui/window.blp`, `src/window/imp.rs`. Update the source-text checks in `meditate-gtk/src/sync_runner.rs:52`. Proof: `cargo test -p meditate`, then MANUAL
- [x] 3.2 Strings: the new msgids from the proposal via `gettext`; the three old tooltip msgids go; `build-aux/update-translations.sh`; all nine `.po` filled; `msgfmt --check`; `cargo test -p meditate i18n::`

## 4. Android
- [x] 4.1 `Modal.sync-status` dialog in `ui/main.slint`, declared after the app bar, the navigation bar and every element it covers; title, body text (word-wrap), buttons from `status_buttons`; a Close button (existing "Close"), the scrim and Back close it; three buttons stack one per line, two sit side by side; it closes itself when the state turns wiped or Hidden. `sync-indicator-tap` opens it or the recovery dialog; button callbacks as on GTK. Accessible label = the title. Files: `meditate-android/ui/main.slint`, `src/ui.rs`; update the source-text check at `src/app.rs:2062`. Proof: `cargo test -p meditate-android --lib`, Android clippy per BUILDING.md, then MANUAL
- [x] 4.2 Strings: new Tr functions, the three old `*-tap` functions removed; all nine `meditate-android.po` files; `msgfmt --check`

## Review fixes (first review, all six findings held)
- R1 Redirect showed as an unreadable reply, against the living requirement "Server text stays out of the sync status". Maintainer chose its own text: `SyncFailureKey::Redirected`, "The server redirected the address", both apps, nine languages; case in `each_failure_records_its_reason` (mutation: mapping it to UnreadableReply fails the test).
- R2 A long not-uploaded list pushed the buttons off screen: the body scrolls at 240 px (GTK ScrolledWindow with propagate-natural-height, Android Flickable).
- R3 NotFound reaching the runner is a file's 404 (a missing folder lists as empty), and Conflict is 405/409/412: `NotWebDav` dropped, NotFound -> `Server(Some(404))`, Conflict -> `Server(None)` = existing "Server error".
- R4 The not-uploaded scenario now names `refused_uploads_show_by_name_until_retried` (through `state_from_db`, now also asserting the buttons); the hand-built test is gone.
- R5 The good-sync test goes through `state_from_db` and checks the stored time; the Error case moved to `a_network_failure_records_its_reason` (buttons); the old-database test ends with a new failure storing its reason.
- R6 Proposal Impact wording fixed (popover attached to a plain button).
- Fix review: PASS; one MINOR held: the GTK scroller now also propagates its natural width, so the body sizes the popover as before R2.
- After the fixes: core 1457, android 287, GTK 168 green.

## /simplify
- Applied: the reason is stored with serde (the `UploadRefusal` pattern) instead of two hand tables; the always-failing test server `AlwaysErrs` moved to `sync/fake.rs` and is shared by the credentials and runner tests; stale comments in `indicator.rs` and `settings.rs` fixed; GTK's idle-icon lines folded into `show_idle_icon`.
- Skipped: `derive` still builds `Error { reason: None }` and `state_from_db` fills the reason (passing it into `derive` changes a tested signature for no behaviour); the kind tag read three times per refresh and the state derived twice per tap (in-memory SQLite, per tap or per 2 s while failing); one core function for Sync Now's clear+trigger (the duplication predates this change); a shared Android card component (third hand-built card, a separate refactor); renaming `sync-indicator-tooltip` (it is still the accessible label); "Preferences in the side-by-side row is dead" (false: Syncing shows Close + Preferences).
- After /simplify: core 1457, android 287, GTK 168 green; clippy (all three runs) clean.

## Notes
- 3/4: the old tooltip msgids are gone from both apps; GTK po regenerated with build-aux/update-translations.sh, the seven fuzzy guesses replaced. Android source checks updated (scrim, card-y, min(340px) counts, the hand-built modal list, sync reasons). Suites: core 1458, android 287, GTK 168 green; clippy (GTK+core all targets, core+android -D warnings, android aarch64) clean.
- The reason enum is `SyncFailureKey` (AGENTS.md: text core hands out is a `*Key`), in `indicator.rs`; `state_from_db` fills it after `derive`, so `derive` kept its signature.
- 1.1/1.2: the runner tests failed on the stub (reason always None), green after. `an_error_from_before_reasons_shows_no_reason` passed on the stub too (None is the stub's answer); it guards the tag parser: mapping an empty or `server:abc` tag to a reason fails it.
- 2.1: the four button tests failed on an empty stub, green after. Core suite 1458 green.
- Pre-build questions (fresh agent) answered as my call, see proposal Decisions: plain Button + Popover, panel closes on wiped/Hidden, Conflict is 412, NotFound → NotWebDav, OpenDb records nothing, keyring gets its own reason, InvalidEvent → unreadable reply, Android: Close button (maintainer overruled my no-Close call), "Synced N ago" may lag a minute.
- The tap-action rename touches every caller of `SyncIndicatorAction`: grep both apps before 2.1.

## Hand-over (step 6)
- Suites: core 1457, android 287, GTK 168 green; clippy clean (GTK+core all targets, core+android -D warnings, android aarch64).
- Linux build: `meditate-gtk/builddir/src/meditate` (2026-11-03 20:46, carries "The server redirected the address"). Start: `XDG_DATA_HOME=/home/janek/.cache/meditate-statustest LOCALEDIR=meditate-gtk/builddir/po ./meditate-gtk/builddir/src/meditate`.
- APK: `meditate-android/android/app/build/outputs/apk/debug/app-debug.apk` (2026-11-03 20:47, the .so carries "status dialog (sync now)"); `adb install -r`.
- Test steps, both apps:
  1. After a good sync, tap the sync symbol: "Synced N ago" with Preferences and Sync Now (Android also Close, the three stacked). Sync Now closes it and syncs; Preferences opens the sync settings.
  2. During a sync, tap it: "Syncing with Nextcloud" with Preferences only (Android: Close and Preferences side by side).
  3. Address with http:// instead of https:// (or a wrong path that redirects): after the sync, red; the status says "Last sync failed" and "The server redirected the address".
  4. A host that doesn't exist: "Network error". A wrong password: "Authentication failed".
  5. Android: Close, a tap beside the dialog and Back all close it; all text shows in full in German (and Russian, if you like).
  6. Linux: the tooltip shows the same title; the popover fits its text.
  - Not testable by hand without effort: a missing password and a list of refused files (core tests cover both).
- Proposed commit message:

  sync: the sync symbol shows the status on tap

  The status text was only a tooltip, so on a phone nobody could read
  why the symbol warned, and a tap did something different in each
  state. A click or tap now opens the status (a popover on Linux, a
  dialog on Android): when it last synced or why it failed, the files
  not uploaded, and buttons for the next step. A wiped server still
  opens the recovery dialog. A failed sync stores a typed reason that
  each app shows in its own language; the English detail stays in the
  log. Batch W's skipped items will show here too.
