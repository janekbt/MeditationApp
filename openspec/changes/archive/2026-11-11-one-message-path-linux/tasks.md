# Tasks: one-message-path-linux

Base: 4c599c7ece318c7db68a19e15301e11eabc5beb3. Suites on it: core 1544 + 18, GTK 171, Android 300, all green. Lines at base: 61,577.

## Loop
- [x] 0 Skills loaded: `/ponytail ultra`, `slint:slint` (loaded; no Android UI in this step)
- [x] 1 Understand (split approved 2026-11-11: "go"; wording: "Wie Presets")
- [x] 2 Propose (approved: 2026-11-11, "go")
- [x] 3 Build (core 1541 + 18 (three ellipsize tests deleted with it), GTK 174, Android 300 green; clippy GTK+core all-targets and CI lib -D warnings clean; check-specs green; lines 61,013)
- [x] 4 Review: five rounds (reviewer + `/code-review high` each), then `/simplify` (see Notes)
- [x] 5 Specs, docs, list (check-specs green; REFACTORING: #320 #357 #373 out, P3 row says step 1 done, stale ellipsize/Announcement/undo-toast notes trimmed; OPEN-ISSUES: #362 #311 #326 #349 #392 out, EE intro and #388 updated; core README row gone)
- [x] 6 Verify and hand over (laptop test 2026-11-11: all steps passed; long-name and margin fixes approved: "jo passt so")
- [x] 7 Ship (deltas merged into the new `openspec/specs/messages/spec.md`)

## Review Focus
- An undo toast replaced by an unrelated one:
  - guided delete: the audio file is still removed;
  - Log delete: the deletes are committed;
  - preset delete: the row stays deleted.
  -> Decision "one undo offer at a time" (proposal); each dismiss handler still runs.
- A plain error toast arriving during an undo offer ("Couldn't play bell sound" after "Session saved") -> it queues and does not dismiss the offer: plain and undo toasts have separate slots.
- A dismiss handler that posts a toast (the Log commit fails and calls `write_ok`) -> `post_toast` records and adds the new toast before it dismisses the old one, so the inner call replaces only toasts of its own kind.
- A quick second Log delete -> the Log sets `active_delete_toast` before `post_toast` dismisses the old toast, so the old handler skips the commit, as today.
- The preset Override pops the chooser that holds the toast's overlay -> its undo toast is anchored on the main window, as today.
- An anchor removed from the tree before the toast (a deleted guided row) -> the callers grab the overlay or window before the delete, as today; `show_toast` on an anchor with no root posts nothing (unchanged).
- A dialog's anchor -> every dialog is presented from a button press, while the button's page is still in the window, so the anchor has a root. `present(Some(anchor))` takes the same window the guard found; it only drops the silent `else`.
- Preferences open over a pushed page -> the message goes to Preferences' own overlay; the scenario "A message from Preferences".
- Another language -> the new msgid in all nine `.po` files; `cargo test -p meditate i18n::`.
- Old database, second device -> no data touched.

## 1. Shared path
- [x] 1.1 Test first: `every_toast_goes_through_the_shared_path`. Outside `db/mod.rs` and `window/imp.rs`, no production source builds or posts a toast itself (`Toast::new`, `Toast::builder`, `.add_toast(`). Rewrite the old source tests that assert on toast code: `application.rs:714-715`, `guided.rs:1632`, `timer/imp.rs:4831`. Expected: FAIL (about 40 places), then PASS.
- [x] 1.2 In `db/mod.rs`, write `plain_toast`, `undo_toast(text, on_undo, on_untaken)`, `post_toast(anchor, toast)` and `show_toast`, as the proposal describes:
  - two slots, one for plain toasts and one for undo toasts;
  - record and add the new toast first, then dismiss the old one;
  - `on_untaken` runs on any dismissal without Undo;
  - the target is the anchor's overlay, else a Preferences dialog's, else the main window's.
- [x] 1.3 Move every caller over. Delete `ToastSlot` ×2, both `build_undo_toast`s, `push_undo_toast*`, `window_from` ×2, `add_toast_to_window`, `present_toast`, `present_import_error_toast`, `data_toast`, the timer's `toast`, and `MeditateWindow::add_toast` if it has no caller left. The Log delete toast and the apply toast keep their own current-toast state. Files: as in the proposal.

## 2. Dialogs
- [x] 2.1 Test first: `dialogs_open_on_their_anchor`. No `root.downcast::<gtk::Window>()` guard around `present`, and "Delete Bell?" appears once in `bells.rs`. Expected: FAIL, then PASS.
- [x] 2.2 Write `present(Some(anchor))` everywhere. Make one `confirm_bell_delete(anchor, app, uuid, after)`, used by the list row and the edit page.

## 3. Texts
- [x] 3.1 Test first: `no_message_cuts_or_glues_a_name`. No `ellipsize(` call and no `format!("{} {}", gettext(` in production sources; `"'{name}' updated"` is used. Expected: FAIL, then PASS.
- [x] 3.2 Delete `Announcement` in both crates and `format::ellipsize`. Write the texts at the call sites: guided and pattern delete say "'{name}' deleted", pattern update says "'{name}' updated", the sound chooser says "Import failed: {error}".
- [x] 3.3 Translations: `build-aux/update-translations.sh`; "'{name}' updated" in de, es, fr, it, nl, pl, pt_BR, ru, zh_CN. Check: `msgfmt --check` on each, then `cargo test -p meditate i18n::`.

## Notes
- Pre-build questions (fresh agent, 2026-11-11), all answered as my call:
  - (1, 2) the Log's slot order and toasts posted from dismiss handlers: see `post_toast` in 1.2 and the Review Focus.
  - (3) plain toasts queue behind undo offers.
  - (4) the pattern toast quotes the new name.
  - (5) `window/imp.rs` is on the test's allow-list.
  - (6) a rootless anchor never happens, so the requirement no longer claims it.
- Found while building (my call, inside the approved requirement "on the page in view"):
  - Guided import errors from Manage went to the main window's overlay, behind the pushed page (the same class as #362). `pick_file_for_open` and `import_picked_file` now take the tapped widget instead of the window.
  - The import dialogs' "Importing:" + name was glued like "Deleted" + name, and `no_message_cuts_or_glues_a_name` caught the bell sound one. Both are now one string, "Importing: {name}", in nine languages. English reads the same.
- `current_apply_toast` in the timer went too: the shared undo slot replaces the last offer, which was its only job.
- `MeditateWindow::add_toast` stays as `post_toast`'s way to the main overlay; the test allows `window/`.
- The three failing tests failed for the right reason: about 40 stray toast sites, 16 window guards plus a second "Delete Bell?", and the glued and cut names.

- Review round 1 (our reviewer and `/code-review high`), fixed:
  - Background messages (bell sound error, session save failure, Folder refreshed) went to the main overlay behind a pushed page or Preferences. `post_toast` now picks the last mapped overlay in the window, or Preferences' own when the anchor is inside it. With no root it uses the main window. `MeditateWindow::add_toast` is gone. New MANUAL scenario: a failure from the background while a pushed page is open.
  - `post_toast` dismissed the old toast even when it found no overlay. It now returns before touching the slot.
  - One Undo slot app-wide let a preset tap end "Session saved / Undo". Slots are per area again, as before (`post_toast(anchor, slot, toast)`).
  - Three dialog guards were left (Discard Session, custom time, Log edit), and `get_window` went with them. The guided picker no longer skips when it has no window. The test now bans `.present(Some(&win))` and `.present(Some(&window))`.
  - The test cut files at the first `#[cfg(test)]`. `vibration.rs` and `sync_runner.rs` keep production code after their test module, so it now cuts out only the `mod tests` block.
  - The wrap scenario's test now also pins `.wrap(true)` and `custom_title` in `plain_toast`; the glue test also checks guided's "'{name}' deleted".
  - Toasts keep `title` next to `custom_title`, for screen readers.
  - Glued error details ("Import failed", "File picker error", "Couldn't read audio file", "Couldn't start playback" + ": " + error) are now one sentence each, `...: {error}`, in nine languages. "Import failed: {error}" already existed. The test bans `format!("{}: {` and `"{} {} ({})"`.
  - pt_BR "'{name}' deleted/updated": the preset text was feminine and now names files and patterns too. Now "Excluído: '{name}'" and "Atualizado: '{name}'".
  - `present_import_error_toast` inlined; leftover blank lines and the `parent_window` shadow gone; two `main.slint` comments named the deleted `Announcement` (comment only).
- Review round 1, rejected:
  - `show_undo_edit_toast` reads the pattern again for its new name. The editor's callback gets only the uuid, so the read stays (one indexed read per save).
  - LAST keeps an Undo toast until it is dismissed, if a popped page's overlay does not dismiss its toasts. Same as before, when the chooser's slot did the same; the guided audio cleanup waits on the same dismissal as before.
  - The Manage create row as the anchor of a guided import: a rebuild between pick and import could remove it. `post_toast` then falls back to the main window's visible overlay, and `present(Some(row))` gets its own window from libadwaita instead of skipping. Only a sync rebuild in those seconds gets there.
- Review round 2 (fix review: a fresh reviewer and `/code-review high`), fixed:
  - "The last mapped overlay" was wrong during navigation:
    - The editor's "'X' updated / Undo" and chooser write failures were posted just before a pop, onto the leaving page.
    - The preset Override Undo went onto the chooser while it slid out.
    - Toasts during a session, whose page has no overlay, were dropped.
    - Now the anchor's own page wins again, as at base. Only a window anchor or a rootless one goes to the page in view (`MeditateWindow::overlay_in_view`: Preferences, else the visible page's overlay, else the main page's). Background senders pass the window: the bell error, Local Folder, the recovery toast and the timer's save failures.
  - The shared plain slot let a second failure end the first at once (a bell error, then a failed save). Now the plain slot is keyed by the message: only the same message replaces itself.
  - Setting `title` next to `custom_title` was dead: custom-title clears title. Removed. A screen-reader check of wrapped toasts is in the test steps.
  - The guided Manage import now hangs its dialog and messages on the navigation view, which a row rebuild can't remove. The visible Manage page then gets the message.
  - The toast ban also catches `add_toast(` written as a full path.
- Review round 2, rejected:
  - Slots as an enum instead of strings: eight literal names at their own call sites. A typo would only make two offers of one area queue instead of replacing each other; no message is lost.
  - Eight `|| {}` for `on_untaken`: a second constructor would add more than it removes.
  - Walking the widget tree for every toast: gone; `overlay_in_view` only looks into the visible page.
  - A dialog that is still closing when a background toast arrives: only a background toast in the half second of a closing Preferences could land in it (unverified whether `visible_dialog()` still returns a closing dialog). A message posted from a page has its own overlay and is unaffected.
  - `LAST` holding the last toast of each slot until the next one: a memory point only, with no case the user sees. The guided audio is cleaned up on the next offer, not never.
- Review round limit raised to five by the maintainer (2026-11-11); this is round 3.
- Review round 3 (fix review: a fresh reviewer and `/code-review high`), fixed:
  - The running page and the Local Folder page had no overlay, so a failed save during a session left dead buttons and no message, and "Folder refreshed" never showed. Both pages now wrap their toolbar in a `ToastOverlay`; the wrap test checks both.
  - `overlay_in_view` takes any open dialog's overlay (the Log editor too), not only Preferences'.
  - An anchor in a popped page found its detached overlay; now only an anchor still in the window uses its own page.
  - The timer's "Couldn't read audio file", "Couldn't start playback" and keep-awake messages come from the background, so they now go to the window, which means the page in view.
  - Pattern edit and delete offers queued before; now each pattern and kind has its own slot, so deleting A then B keeps A's Undo.
  - The bell error keys its slot like `show_toast` ("plain:" + message).
  - `win_for_pick` renamed to `anchor`.
  - Test: the edit page's Delete must call `confirm_and_delete`.
  - The background scenario named a page a session removes; it now uses Local Folder's Refresh.
- Review round 3, rejected:
  - Screen readers and `custom_title`: before this change, failure toasts already used it. Whether Orca reads a custom title is in the test steps; if not, a follow-up gives the label an accessible description.
  - `dismiss_toasts` at Start clears only the main overlay. A "Session saved / Undo" left on a pushed page that Start was pressed on stays there; its Undo still deletes the right session.
  - "Importing: {name}" carries "name (12:00)" as one unit; a language that needs the length elsewhere is not among the nine.
  - The dialog-guard test matches two spellings only; no other spelling exists, and every dialog is now presented on its anchor.
- Review round 4 (fix review: a fresh reviewer and `/code-review high`), fixed:
  - At Start the keep-awake and starting-bell failures were posted before the running page was pushed, so the session covered them. Toasts for the page in view are now placed from an idle callback, after the handler that pushes or pops.
  - A bell error in the moments before the running page pops goes with that page. The once-per-run latch made that final, so it is now once per session: Start re-arms it, and the next session tells again.
  - The plain slot is keyed per overlay as well (a "Save failed" on another page no longer ends this one). `plain_slot` is shared with the bell error.
  - `first_overlay` goes into a nested navigation view only through its shown page (the Log editor's label chooser).
  - A failed Log commit runs from a toast's end, so it goes to the page in view.
  - `post_toast` lost the redundant window check.
  - The tests read the sources once and check that both new overlays are the pages' children.
- Review round 4, rejected:
  - AlertDialog over Preferences: a background toast goes to the page under both. `visible_dialog()` gives only the top dialog, and a confirm on top of Preferences is open for seconds.
  - `post_toast` without any main window (shutdown): an undo toast's cleanup never runs. This was the same at base; at shutdown nothing is shown anyway.
  - `Result` as the overlay-or-Preferences type: two arms, read in one place.
  - Deleting `Announcement` against the typed-key rule: that rule is for text core hands to the apps. `Announcement` was built and rendered only in GTK; Android never used it. Told to the maintainer.
- Round 5 is the last within the limit.
- Review round 5 (fix review: a fresh reviewer and `/code-review high`), no critical or important behaviour finding left; fixed:
  - The round-4 fixes had no test: new `messages_with_no_page_go_to_the_page_in_view` (the idle pick, keys per overlay, only a nested view's shown page, every open dialog, the re-arm at Start, the Log commit's window anchor). Proved by breaking the Start re-arm once: FAIL, restored: PASS.
  - Preferences' own overlay is an `AdwToastOverlay` (read from the libadwaita template in the installed library). So the target is always an overlay: `post_toast` takes an anchor's dialog's first overlay, `overlay_in_view` walks every open dialog from the top (an AlertDialog over Preferences no longer hides a toast), and the `PreferencesDialog` arm is gone. No two keys for one surface any more.
  - Pattern offers queue again, as at base: each offer has its own slot. A second edit of one pattern no longer ends the first Undo.
  - Plain toasts use the libadwaita default timeout (5 s); the `Toast::new` sites had it, `show_toast` had 4 s.
  - A bell preview re-arms the bell error too (each preview is one tap).
  - The folder chooser opens without a window instead of skipping.
  - The sound import's worker-died message is the existing "Import worker died", not glued.
  - The tests read the sources once each; the doc on `wire_audio_error_handler` says once per session or preview.
- Review round 5, rejected:
  - Undo offer on a popped chooser never dismissed: base had per-chooser slots too, and the startup sweep removes an orphaned guided audio file. Whether libadwaita times out toasts on an unmapped overlay is in the test steps.
  - Undo toasts lose `title` for screen readers: in the test steps (Orca); custom titles keep the full text visible.
  - A `show_in_view` helper for the window-as-anchor call sites: left to `/simplify`.
- No sixth round: round 5 found no behaviour fault, and its fixes are tests, a simplification and small items; the device test covers the rest (maintainer, 2026-11-11: more rounds only if I find them worth it).
- `/simplify` (four angles), fixed:
  - `show_in_view(text)` and `post_in_view(slot, toast)` name "no page of its own", instead of three root and window lookups (timer ×5, Local Folder ×2, the bell error).
  - `post_toast` takes `Option<&str>`, where `None` queues; the pattern offers' pointer keys are gone.
  - `LAST` holds weak references, so a popped page with a toast up is freed.
  - `first_overlay` uses `iter::successors`; stale "window toast" docs, `parent_for_*` names and double blank lines are fixed.
  - The bell error now uses the default timeout (5 s instead of 6 s), through `show_in_view`.
- `/simplify`, skipped:
  - Dropping the bell-error latch (the slot would replace repeats): it changes what the user sees during a failing session, so it is a question for the maintainer, not a cleanup.
  - The Log's own dismiss logic in `on_untaken`: its identity check needs the toast inside its own handler, which is a cycle.
  - Passing the new pattern name instead of reading it: the editor's callback gives only the uuid (rejected twice in review).
  - `no_write_result_is_dropped` keeping its own walker: its cut rule differs; merging the test walkers is test cleanup outside this change.
  - Source tests that pin code text: the plan's choice, since GTK can't run a second widget test thread.
- Final runs: core 1541 + 18, GTK 175, Android 300 green; clippy GTK+core all-targets and CI lib -D warnings clean; check-specs green; nine `.po` pass `msgfmt --check`. Lines 61,577 → 61,040 (−537).

- Laptop test 1 (2026-11-11, all steps passed), fixed:
  - Long names with no space didn't wrap and widened the page, and Undo showed as "Rüc…". Titles now wrap inside words (`WordChar`), and Undo toasts cap their title at 20 characters. New scenario "A long name with no spaces, next to Undo" (test first: FAIL, then PASS).
  - The text sat close to the pill's left edge: the label has a 6 px start margin.
  - Bell sounds' rename and delete never had a message, in either app: OPEN-ISSUES #509 (its own change with Undo, maintainer's choice).

## Hand-over
- Laptop build: `meditate-gtk/builddir/src/meditate` (2026-11-11 09:02), started in German with the scratch data dir `~/.cache/meditate-datetest`.
- Test steps (general):
  1. Guided, Manage: import a file with a long name, delete it. The message names it in full ("'...' gelöscht"), and Undo brings it back.
  2. Vibration: rename a pattern and save. "'Neuer Name' aktualisiert" appears; Undo restores the old name.
  3. Bells: open a bell, then Sound, then Import, and pick a file over 10 MB. The message shows on the chooser.
  4. Guided, Local Folder: choose a folder, tap Refresh. "Ordner aktualisiert" shows on that page.
  5. Preferences, Sync: Save with an empty password. The message shows inside Preferences.
  6. Delete a bell from the list and once from its edit page: the same dialog both times. Delete a preset and press Undo; Override a preset and press Undo on Setup.
  - Optional, with Orca on: does it read a toast aloud?
- Proposed commit message:

      gtk: one path for toasts and dialogs

      Every Linux toast goes through db::show_toast, undo_toast and
      post_toast: titles wrap, a toast lands on its own page (or, with
      no page of its own, on the page in view), and only the same
      message or the same area's Undo replaces another. Dialogs open on
      their anchor instead of skipping without a window, Delete Bell? is
      built once, Announcement and ellipsize are gone, and names and
      error details sit in one translatable sentence.

      REFACTORING #320 #357 #373; OPEN-ISSUES #362 #311 #326 #349 #392.

