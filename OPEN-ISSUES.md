# Open issues (2026-10-09)

One list for everything still open: the Android bugs (formerly ANDROID-BUGS.md) and the refactors left from the audit (formerly REFACTOR-AUDIT.md). Done items are removed; the git history has both old files (`git show 6e81927:ANDROID-BUGS.md`, `git show 6e81927:REFACTOR-AUDIT.md`). Bug numbers stay as they were, because commit messages refer to them. Line numbers in "Where" date from the audit and have moved since (`lib.rs` UI code is now in `ui.rs`).

**Ground rules:**
- Decisions go in core.
- No DB schema or sync wire-format change without a migration, and nothing touches the Android build environment (Cargo.toml, Cargo.lock, build.rs, main.rs, gradle, the manifest, rust-build.sh) without asking.
- msgids stay unless the item says otherwise; new ones get all nine translations.
- TDD, then a phone check before the commit.
- Before the next release: run `build-aux/repro-probe.sh`.

## Batches, in suggested order

Each batch touches one area and needs one test round. Mark a batch `[x]` when it is committed.

| Done | Batch | Bugs | Why this place |
|:---:|---|---|---|
| [x] | A. Sync compaction | #3, corrupt manifest | data loss |
| [x] | B. Log editing and labels | #4, #5, #6, #34 | data loss |
| [ ] | C. CSV | #7, #36 | data loss |
| [ ] | D. Locale | #29, #26, #25, #30 | visible every day in German |
| [ ] | E. Stats | #18, #24, #35, #22 | wrong numbers |
| [ ] | F. File import (Kotlin) | #21, #37, #38 | rare files and edge cases |
| [ ] | G. Bell file cleanup | bell audio left on disk | wasted storage |
| [ ] | H. Layout and wording | #23 rest, #11, #32, GTK sync toast | cosmetic |

The deferred refactors come last, each together with the batch named there.

---

## A. Sync compaction

Core sync only. Check: host tests with the fake WebDAV, then a two-device sync.

### #3 Sync compaction can delete another device's batch before anyone pulled it
- **Where:** `meditate-core/src/sync/orchestrator.rs:521-617` (`maybe_compact_events`).
- **What happens:**
  - Compaction takes a fresh listing after push and swallows every batch in it.
  - But the consolidated file contains only this device's `all_events()`.
  - A batch that a peer uploaded between our pull and our compaction is deleted without ever having been pulled. Its events are lost for every other device.
  - This is core code, so GTK is affected too.
- **Repro:** More than 50 remote batches. Device B pushes while device A is between its pull listing and its compaction.
- **Fix idea:** Only swallow and delete batches whose uuid is in `known_remote_file_uuids()`.
- **Confidence:** likely (the race window is confirmed in code).

### Corrupt sync compaction manifest (do with #3)
- **Where:** `meditate-core/src/sync/orchestrator.rs:572` reads a manifest that fails to parse as `unwrap_or_default()`, and then overwrites the remote with only the current batch.
- **What happens:** A peer whose batches were swallowed earlier gets a false "remote data lost" dialog. If the user then wipes local data, that becomes data loss.
- **Fix idea:** Abort compaction on a parse error, about +3 lines plus a FakeWebDav test.
- **Confidence:** real but unlikely

## B. Log editing and labels

Edit Session, the Log and the label chooser; #4 and #5 in GTK too. Check: phone and desktop.

### #4 Edit/Add Session closes as if saved when the DB write fails
- **Where:**
  - `lib.rs:10039-10043` and `:10065-10071` only log the error; `:10079-10081` then closes the page.
  - The same pattern appears in create label (`:9199-9230`), interval bell save (`:9077-9086`), and preset/guided-delete Undo (`let _ =` at `:9715`, `:9738`).
- **What happens:** On a full disk or SQLITE_BUSY (see item 2), the page closes, the edit is gone and nothing tells the user. Done-screen Save already handles this properly.
- **Fix idea:** On error, keep the page open, restore `editing_session`, and raise the existing save-failed snackbar.
- **Confidence:** confirmed
- **GTK has it too:** Log edit closes the dialog even when the write failed (`log/imp.rs`, around the save handler).

### #5 Opening the duration dialog on a session of 24 h or more silently cuts it to 23 h
- **Where:**
  - `lib.rs:9881` seeds `hours = total/3600`.
  - The dialog has `max-value: 23` (`main.slint:5667`).
  - `commit()` clamps on Set.
  - `log_edit_duration_secs` sees a change and saves it.
- **What happens:** A 25h 10m session, for example from a long stopwatch run or a GTK sync, becomes 23h 10m just by tapping Duration → Set → Save. Manual add is capped at 23:59 too.
- **Fix idea:** In the edit context, let hours go up to the larger of 23 and the seeded value. Or don't commit text the user didn't change.
- **Confidence:** confirmed
- **Worse in GTK:** its SpinRow max is 23 (`log/imp.rs:815`), so even a note-only edit of a session of 24 h or more saves 23 h.

### #6 Deleting or renaming a label from the edit chooser leaves the Log stale, and a later edit is then lost
- **Where:**
  - `lib.rs:9284` `on_rename_label_confirm`: no feed reload.
  - `lib.rs:9332` `on_delete_label_confirm`: reloads only if the filter changed.
  - `on_card_tap` (~`:9864-9876`) treats an unknown label name as "on, id L".
- **What happens:**
  - Cards keep the old name.
  - Opening another card with the deleted label and saving fails on the foreign key. That failure is only logged (item 4), so the note edit is lost.
- **Repro:**
  1. Open card A (label "Work") → label row → delete "Work" → Cancel.
  2. Open card B (also "Work"), change its note, Save. The note reverts.
- **Fix idea:** Reload the feed after a label delete or rename. In `card_tap`, treat a label that can't be resolved as None.
- **Confidence:** confirmed

### #34 The Log filter dropdown keeps a renamed label's old name
- **Where:** `lib.rs:3299-3302`. Material `drop_down_menu.slint:122` updates only on an index change.
- **Fix idea:** Bounce the index, or refresh the dropdown when its items change.

## C. CSV

Export and import, core. Check: host tests, then one export and re-import.

### #7 CSV export and re-import corrupt notes and labels that start with `- = + @` or TAB, and drop the guided-file link
- **Where:**
  - `meditate-core/src/data_io.rs:100-108` (the export guard adds `'`).
  - `data_io.rs:194-195` (import trims and never strips the guard).
  - The header has no `guided_file_uuid` column (`:125`).
- **What happens:**
  - Note "- calm" comes back as "'- calm".
  - Label "-Work" becomes a separate new label "'-Work".
  - Leading and trailing whitespace in notes is lost.
  - Guided sessions lose their file attribution.
  - It shows up when restoring into an empty DB or onto a new device.
- **Fix idea:** On import, strip one `'` when the next character is a guard character. Don't trim notes. Add an optional `guided_file_uuid` column.
- **Confidence:** confirmed

### #36 CSV export isn't in chronological order
- **Where:** `meditate-core/src/data_io.rs:127-130`: rows are by id, reversed, despite the comment.
- **Fix idea:** Sort by `start_iso` ascending.

## D. Locale

Month and weekday names, the 12-hour clock, the first day of the week, the picker text. Check: phone in German, and in English with a 12-hour clock.

### #29 Month and weekday names are English and in US order
- **Where:** `lib.rs:2002,2009` (heatmap range), `:2057-2064` (chart axis), `:2303` (Longest-session insight). chrono has no locale support.
- **Fix idea:** Build them from numbers through Tr arrays (like `Tr.day-month`).

### #26 Edit Session ignores the 12-hour clock
- **Where:** `main.slint:5233-5235` (hand-built "HH:MM") and `:5320` (`use_24_hour_format: true`). The Log uses the locale's AM/PM.
- **Confidence:** likely (only matters on 12-hour locales)

### #25 The week always starts on Monday on Android
- **Where:** `meditate-core/src/date_math.rs:51-61`: the locale lookup is `cfg(target_os = "linux")` only.
- **What happens:** en-US phones get Monday-first heatmap rows and week-over-week. There is no effect for a German locale.
- **Fix idea:** Have the shell pass the Android locale's first weekday.
- **Confidence:** confirmed

### #30 Date and time picker chrome is English
- **Where:** the Material `date_picker.slint` / `time_picker.slint` strings ("Ok", "Hour", "Minute", "Enter date", the weekday letters with msgctxt) aren't in any .po. `on_format_date` (`lib.rs:3610`) uses chrono's English names.
- **Fix idea:** Add those msgids to every .po, and translate the names in `format_date`.

## E. Stats

Stats page, the Log day caption and the streak line. Move the duplicated Stats inputs into core with it (`insights::input_from_db`, `goal::from_db`, about -40 lines). Check: phone and desktop.

### #18 A session dated in the future zeroes the streak and counts toward today's goal
- **Where:** `meditate-core/src/db/sessions.rs:905-936` (`streak_filtered`), `:330` and `:155` (no upper bound). Edit/Add allows any date.
- **Repro:** Add a 10-minute session tomorrow. The streak drops to 0 and the goal ring includes the 10 minutes.
- **Fix idea:** In core, ignore days after today for the streak, and bound the totals and the average with `< tomorrow`.
- **Confidence:** confirmed

### #24 The newest bar in the 3-month chart covers only 6 days
- **Where:** `meditate-core/src/date_math.rs:190` (90 days) and `:252-260` (`chunks(7)`).
- **Fix idea:** Use 91 days, or chunk from the newest end.
- **Confidence:** confirmed

### #35 The Log day caption total disagrees with the card minutes
- **Where:** `meditate-core/src/format.rs:389` rounds; the caption (`hm_compact_key`) floors.
- **What happens:** Two 10m40s cards show "11 min" each, but the caption says "21m".
- **Fix idea:** A core `log_day_caption` that sums the cards' `log_card_minutes`. One rounding rule alone does not fix it: 11 + 11 = 22, while the rounded sum of 21m20s is 21. GTK has the same bug (log/imp.rs:641-650 floors, cards round at 404), so fix both apps.
- **Do with it:** the other Stats fixes (#18, #24) are a good moment to also move the duplicated Stats inputs into core (`insights::input_from_db`, `goal::from_db`), as this batch's intro says.

### #22 The streak line on the Timer screen is always empty
- **Where:** `main.slint:1775,2977`. `set_streak_text` is never called (GTK shows the streak there).
- **Fix idea:** Set it whenever stats refresh.
- **Confidence:** confirmed

## F. File import (Kotlin)

The picker and the transcoder. Check: phone, with an HE-AAC file.

### #21 Importing an HE-AAC file plays at half speed
- **Where:**
  - `kotlin/MeditateGuidedImport.kt:149-150,189,298-299`: the rate and channel count come from the extractor, and `INFO_OUTPUT_FORMAT_CHANGED` is ignored.
  - Sources with more than 2 channels are fed to the encoder as stereo.
- **What happens:** HE-AAC (SBR or parametric stereo) is about 2x slow and an octave low. 5.1 sources come out garbled. The broken file is synced.
- **Fix idea:** On a format change, rebuild the resampler from `decoder.outputFormat`, and downmix to stereo.
- **Confidence:** likely

### #37 The app looks frozen while a picked file is copied
- **Where:** `MeditateFilePickerActivity.kt:112-124`: the translucent activity eats every touch until the copy and probe finish.
- **Fix idea:** Finish the activity at once and let the worker write the drop-file.

### #38 Codec and extractor leak when import setup fails
- **Where:** `MeditateGuidedImport.kt:131-184` (created before the `try/finally` at `:269`), and `probe()` in `MeditateFilePickerActivity.kt:221-249`.
- **Fix idea:** Bring setup into the `try/finally`.

## G. Bell file cleanup

Both apps. Check: desktop and phone.

### Deleting a custom bell leaves its audio file on disk
- **Where:** both apps: GTK `sounds.rs`, Android `ui.rs`, core `bell_sounds.rs`. Guided delete does remove its file; peer tombstones leak the same way.
- **Fix idea:** Remove the file where the sounds dir is known.

## H. Layout and wording

Slint layout and translation content. Check: phone in German and Russian, and the desktop's sync Test.

### #23 Layouts overflow at 360dp or with longer translations
- **Recovery dialog** (`main.slint:8672-8693`): three text buttons, about 360px in English and 480px in German, in a card about 292px wide. "Push My Data" spills off the card.
- **`CompactToggle`** (`:626-674`, used at `:6177`): segments can't shrink, so the third "Kind" option is clipped in fr/es/nl/it/pt_BR.
- **Stats period `SegmentedButton`** (`:4818-4829`): about 385px against about 332px.
- **Fix idea:** Stack the recovery buttons. Use equal-width segments, as `ModeToggle` does. (The editor headers were fixed by the shared `PageHeader`.)
- **Confidence:** likely (estimated from font metrics)

### #11 Russian: the Discard dialog's buttons read almost the same
- **Where:** `lang/ru/.../meditate-android.po`: "Discard" = "Отменить", "Cancel" = "Отмена", "Discard Session?" = "Отменить сессию?". Used at `main.slint:8435-8438` and `:4061`.
- **What happens:** Two buttons that both read as "cancel", and one of them deletes the session and its note.
- **Fix idea:** Use "Не сохранять" or "Удалить" for Discard, and "Удалить сессию?" for the title.
- **Confidence:** confirmed

### #32 German mixes "Label" and "Kategorie"
- **Where:** `lang/de/.../meditate-android.po`, e.g. lines 176, 320, 323 vs 83, 110, 833.
- **Fix idea:** Pick one term.

### GTK: the sync Test toast is English
- **Where:** GTK `preferences.rs`, `credentials.rs`: the toast shows core's English `Display`.

## Deferred refactors

Only together with other work in the same area; none has a bug behind it.

- **R3 step 1, `settings_from_db(db, shape)`:** one core builder for the session settings instead of Android's `build_session_settings` and GTK's three copies (about -70 GTK, -30 Android). It changes how every session type starts, so it needs a full round of all modes on phone and desktop.
- **R7(b), a session record:** `Active { session, PendingSession }` and `Finished(PendingSession)` instead of `session_start_unix`, `pending_done` and the `SESSION_GUIDED_FILE` static; leaving Finished emits `StopActiveSignals` in one place. About -30 lines, but it rewrites about 54 test chains.
- **R12, sync runner into core** (with batch A if that grows into more sync work): core `run_attempt` with a password closure, `SyncError::is_transient_network()`, a typed error, one blob routine for sounds and guided files (remote paths byte-identical). About -150 in the apps, +100 in core, -90 in the orchestrator. Includes the narrow sync-account error enums that remove four `unreachable!` arms. Do it when sync needs work.
- **R15(c), `SlidePage`** (with batch H if it touches the pages anyway): one frame (slide, scroll area, tap catcher) for 13 pages. Several pages reach into their own Flickable by id (note release, Diagnostics scroll), so each needs rework and a retest.
- **R15(d), merge `VerticalSpinBox` and `StepperRow`** (with batch B, whose duration dialog uses `VerticalSpinBox`): they share about 20 lines of commit logic; merging touches every number field's focus and keyboard handling for about -40 lines.
- **R13 small duplicates:** fold `hm_mins_key` into `hm_secs_key` (they differ at 0); one `session_payload()` helper for the three session `json!` copies, next time the session payload changes (batch C touches sessions but not the payload).
- **R2b, all database writes off the UI thread:** one background writer per app (about 175 call sites, L). Only if the 1 s lock wait is noticeable in practice, or a feature needs long writes.

## Considered and rejected

So these aren't proposed again:
- Typed serde structs for every sync event payload: the silent defaults are deliberate compatibility tolerance, and there is wire risk.
- A change-counter-driven view refresh: `LocalChanges` is per connection, so sync pulls never bump it.
- A run token on every drop file: only imports needed one (done).
- Number fields that commit on every keystroke: intermediate values go stale or clamp mid-typing.
- Typed `BellSlot` through Slint: large, with no host safety net.
- Moving the whole sync account Save/Test into core: the keychains are per app.
- Moving the Log edit build into core: low value; reconsider when fixing #5/#6.
- Splitting `ui.rs` per screen or the big core files: cosmetic.
- A `Divider` component, a page stack model, caching JNI classes, moving the CAN_DUCK policy to Rust, a typed `SettingKey` enum, a core pending-Undo queue.
- Left out of the bug audit on purpose: configChanges / activity recreate and the import-labels transaction (both in TODO.md); Back on the Done screen discarding the session (decided behaviour).
