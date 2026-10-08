# Android bug audit (third pass)

A fresh audit of beta HEAD, run independently of the two earlier ones. Six
reviewers each took one area: session lifecycle, JNI/Kotlin, data paths,
Slint UI, i18n/resources and edge cases. Every item below was checked
against the code. Items that only *likely* bite in practice say so under
Confidence.

Left out on purpose:
- configChanges / activity recreate, and the import-labels transaction:
  both are already in TODO.md.
- Back on the Done screen discarding the session: that is the decided
  behaviour.

---

## A. Data loss

### 1. A guided preview that is still playing can end a widget-started Timer or Box Breath session, which then saves nothing
- **Fixed (2026-10-08):** R4 (`stop_all_previews` on every start) and the overtime guard in core (5ee5ad7).
- **Where:**
  - `meditate-android/src/lib.rs:1036` (`close_transient_overlays` clears the preview id but never calls `guided::stop`).
  - `lib.rs:5594-5617` (EOS and focus-loss are applied to any running session).
  - `meditate-core/src/session/mod.rs:412` (`enter_overtime` ignores the session shape).
- **What happens:**
  - A Manage Files preview keeps playing under a session started from the widget. `StopActiveSignals` stops only bells, not the guided player.
  - When the preview ends, the Timer session is forced into Overtime. A countdown rings early and saves its full planned length.
  - A stopwatch or Box Breath session ends up at a 00:00 Finish. Save then writes nothing, and the recovery snapshot is cleared too.
  - The preview's focus loss can also pause a Timer session.
- **Repro:**
  1. Guided → Manage Files → play a preview.
  2. Go Home and tap a stopwatch preset on the widget.
  3. Wait for the preview to end, then Finish → Save. No session is saved.
- **Fix idea:**
  - Call `guided::stop` in `close_transient_overlays`.
  - In core, make `enter_overtime` a no-op for non-Guided shapes, and have `finish_overtime` stay Active when core emits no `EndSession`.
- **Confidence:** confirmed

### 2. Edits, deletes and renames fail at once while sync is writing
- **Where:**
  - `meditate-core/src/db/sessions.rs:610,675`, `labels.rs:107,129`, `presets.rs:198ff`. All use `unchecked_transaction()`: a DEFERRED transaction that reads first, then writes.
  - The Android callers only log the error: `lib.rs:10039` (edit save) and `lib.rs:3012` (pending-delete commit).
- **What happens:**
  - The sync worker writes on its own connection. When the UI connection tries to upgrade its read to a write, SQLite returns SQLITE_BUSY or BUSY_SNAPSHOT instead of waiting out the 8 s busy timeout. A test with Python sqlite3 showed it failing after 0.00 s.
  - A note edit or delete made during sync replay is silently dropped, and the deleted card comes back on the next reload.
- **Repro:** Resume the app while a large pull is replaying, then save an edited note. The old note comes back.
- **Fix idea:** In core, open write transactions with `BEGIN IMMEDIATE` so the busy timeout applies. Item 4 covers surfacing the error.
- **Confidence:** the mechanism is confirmed; how often it hits is likely.

### 3. Sync compaction can delete another device's batch before anyone pulled it
- **Where:** `meditate-core/src/sync/orchestrator.rs:521-617` (`maybe_compact_events`).
- **What happens:**
  - Compaction takes a fresh listing after push and swallows every batch in it.
  - But the consolidated file contains only this device's `all_events()`.
  - A batch that a peer uploaded between our pull and our compaction is deleted without ever having been pulled. Its events are lost for every other device.
  - This is core code, so GTK is affected too.
- **Repro:** More than 50 remote batches. Device B pushes while device A is between its pull listing and its compaction.
- **Fix idea:** Only swallow and delete batches whose uuid is in `known_remote_file_uuids()`.
- **Confidence:** likely (the race window is confirmed in code).

### 4. Edit/Add Session closes as if saved when the DB write fails
- **Where:**
  - `lib.rs:10039-10043` and `:10065-10071` only log the error; `:10079-10081` then closes the page.
  - The same pattern appears in create label (`:9199-9230`), interval bell save (`:9077-9086`), and preset/guided-delete Undo (`let _ =` at `:9715`, `:9738`).
- **What happens:** On a full disk or SQLITE_BUSY (see item 2), the page closes, the edit is gone and nothing tells the user. Done-screen Save already handles this properly.
- **Fix idea:** On error, keep the page open, restore `editing_session`, and raise the existing save-failed snackbar.
- **Confidence:** confirmed

### 5. Opening the duration dialog on a session of 24 h or more silently cuts it to 23 h
- **Where:**
  - `lib.rs:9881` seeds `hours = total/3600`.
  - The dialog has `max-value: 23` (`main.slint:5667`).
  - `commit()` clamps on Set.
  - `log_edit_duration_secs` sees a change and saves it.
- **What happens:** A 25h 10m session, for example from a long stopwatch run or a GTK sync, becomes 23h 10m just by tapping Duration → Set → Save. Manual add is capped at 23:59 too.
- **Fix idea:** In the edit context, let hours go up to the larger of 23 and the seeded value. Or don't commit text the user didn't change.
- **Confidence:** confirmed

### 6. Deleting or renaming a label from the edit chooser leaves the Log stale, and a later edit is then lost
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

### 7. CSV export and re-import corrupt notes and labels that start with `- = + @` or TAB, and drop the guided-file link
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

### 8. Deleted Log cards come back if the app is killed within the undo window
- **Fixed (2026-10-08):** R5: a pending Undo is made final when the app goes to the background.
- **Where:** `lib.rs:9608-9637` (in-memory timer only). The `MainEvent::Pause` handler (`:11510`) doesn't flush `pending_deletes`.
- **What happens:** Delete a card, then swipe the app away within 5 s. The session is back on the next launch.
- **Fix idea:** Call `commit_pending_deletes` on Pause/Stop.
- **Confidence:** likely

### 9. Cancelling an import and quickly re-importing can adopt the old worker's result
- **Where:** `kotlin/MeditateGuidedImport.kt:70-72,92-96,111-114`; `lib.rs:6188`.
- **What happens:**
  - The new run clears the cancel flag while the old WAV/OGG passthrough copy is still running.
  - The old worker then writes "ok". The tick inserts the new import's DB row before its file exists, so the row may point to a missing or partial file.
  - Both runs also share `guided/transient.<ext>`.
- **Repro:** Import a large WAV, Cancel during the copy, then immediately pick and confirm another file.
- **Fix idea:** Put a per-run token in the cancel and result file names, and ignore results whose token doesn't match.
- **Confidence:** likely (narrow window)

### 10. Save isn't atomic with clearing the recovery snapshot
- **Where:** the `lib.rs` save tap (~`:5851-5860`): the insert, then `clear_session_in_progress_snapshot()` as a separate statement.
- **What happens:** A kill between the two steps makes the next launch recover and save the same session again, leaving a duplicate row.
- **Fix idea:** A core method that does the insert and the clear in one transaction.
- **Confidence:** confirmed (a window of a few milliseconds)

### 11. Russian: the Discard dialog's buttons read almost the same
- **Where:** `lang/ru/.../meditate-android.po`: "Discard" = "Отменить", "Cancel" = "Отмена", "Discard Session?" = "Отменить сессию?". Used at `main.slint:8435-8438` and `:4061`.
- **What happens:** Two buttons that both read as "cancel", and one of them deletes the session and its note.
- **Fix idea:** Use "Не сохранять" or "Удалить" for Discard, and "Удалить сессию?" for the title.
- **Confidence:** confirmed

---

## B. Wrong behaviour

### 12. Back sends the app to the background after a text field loses focus
- **Fixed (2026-10-08):** dd704f2: focus returns to `root-focus` whenever no text field has it. Still open: a field that is only hidden (not closed) while focused loses one Back.
- **Where:**
  - `main.slint:2919-2931`: Back is handled by `root-focus`.
  - Focus goes to None through `clear-focus()` (`:478,1253,1313,1334,1351`) or when a focused field is hidden or destroyed (the `if` dialogs, the Edit-Session note).
  - Slint 1.16 `window.rs:839-882` only delivers keys up from the focused item. With no focus, the key is unhandled and Android backgrounds the app.
- **Repro:**
  1. Duration row → type an hour value → Set.
  2. Start, then press Back. The app leaves.
- **Fix idea:** Call `root-focus.focus()` whenever a dialog or page closes and after every `clear-focus()`.
- **Confidence:** confirmed in Slint's code path (not yet tried on the phone).

### 13. The vibration-pattern editor saves a stale typed number
- **Where:** `main.slint:6652-6656` (Save doesn't commit the steppers at `:6697-6712`). The interval editor does commit them (`:6155-6160`).
- **Repro:** Create pattern → type 50 into Duration → tap Save. It saves the old value. The keyboard or popup can also stay up after closing.
- **Fix idea:** Name the steppers, `commit()` them before save, and release them on every close path.
- **Confidence:** confirmed

### 14. Guided audio focus: Resume plays without focus, and a ducking notification pauses the session
- **Where:** `kotlin/MeditateGuided.kt`: `resumeAudio` (`:388-393`) never re-requests focus; `:416-422` treats `LOSS_TRANSIENT_CAN_DUCK` as a full loss.
- **What happens:**
  - After another media app takes focus, Resume plays over that app, and later calls no longer pause the session.
  - Any notification sound or navigation prompt pauses the whole guided session, and it stays paused.
- **Fix idea:**
  - Re-request focus in `resumeAudio`, and stay paused if it isn't granted.
  - On CAN_DUCK, lower the volume instead of pausing.
  - Abandon focus when the track completes.
- **Confidence:** likely (Android focus semantics; confirmed in code)

### 15. A preview that plays to its end leaks its focus request, and the next guided session pauses itself
- **Where:**
  - `MeditateGuided.kt:347-359`: `startAudio` never abandons the previous request; the completion and error listeners never abandon theirs.
  - The EOS tick clears the preview id (`lib.rs:5594`), so Back no longer calls `guided::stop`.
- **Repro:** Preview a file to its end → Back → start a guided session. It pauses within one tick.
- **Fix idea:** Abandon focus in `releaseLocked` and in the completion and error listeners.
- **Confidence:** likely

### 16. Starting from the widget leaves bell dialogs open and bell or pattern previews playing
- **Fixed (2026-10-08):** R4 (previews) and R6 (one `modal` property, closed by a session start).
- **Where:** `lib.rs:1036-1073`.
  - `bell_rename_dialog_open`, `bell_delete_dialog_open` and `interval_bell_delete_dialog_open` are not closed. They are declared last, so they sit above Running.
  - Bell and pattern previews only have their ids cleared. The volume preview and the vibration-editor preview are not stopped.
- **Repro:** Bell Sound → pencil (Rename dialog open) → Home → tap a widget preset. The session runs under the dialog.
- **Fix idea:** Close those three dialogs, and stop previews through the same helpers the Back chain uses.
- **Confidence:** confirmed (dialogs), likely (audio)

### 17. Preset-apply Undo still works after Start and changes the running session's label
- **Fixed (2026-10-08):** R11 step 1: a session start drops the preset-apply Undo.
- **Where:** `on_action_tap` (`lib.rs:4654-4846`) doesn't clear `pending_preset_undo` or hide the snackbar. The snackbar draws above Running (`main.slint:8155` vs `:4086`). The Undo branch (`:9680-9706`) only checks the mode.
- **Repro:** Tap a preset chip with a different label → Start within 5 s → Undo → Stop. The Done screen shows the old label.
- **Fix idea:** On the Idle→Active edge, clear the pending Undo slots and hide the snackbar.
- **Confidence:** confirmed

### 18. A session dated in the future zeroes the streak and counts toward today's goal
- **Where:** `meditate-core/src/db/sessions.rs:905-936` (`streak_filtered`), `:330` and `:155` (no upper bound). Edit/Add allows any date.
- **Repro:** Add a 10-minute session tomorrow. The streak drops to 0 and the goal ring includes the 10 minutes.
- **Fix idea:** In core, ignore days after today for the streak, and bound the totals and the average with `< tomorrow`.
- **Confidence:** confirmed

### 19. A guided or bell "Open File" whose copy fails gives no feedback
- **Where:** `kotlin/MeditateFilePickerActivity.kt:111-146`: a null stream or an exception writes no drop-file and leaves a partial `transient.*`. The CSV path already writes `err:`.
- **Repro:** Pick a Drive audio file while offline, or with storage full. Nothing happens.
- **Fix idea:** Delete `dest`, write an `err:` marker, and map it to the existing `import_failed` toast.
- **Confidence:** confirmed

### 20. A guided pick with a failed duration probe (0 s) ends at once and saves nothing
- **Where:** `MeditateFilePickerActivity.kt:208-245` returns 0 on failure. Start is enabled with just a name (`main.slint:3024`). Core crosses the 0 s target on the first tick.
- **Fix idea:** Treat 0 s as a pick error, or require a duration above 0 to enable Start.
- **Confidence:** likely (needs such a file)

### 21. Importing an HE-AAC file plays at half speed
- **Where:**
  - `kotlin/MeditateGuidedImport.kt:149-150,189,298-299`: the rate and channel count come from the extractor, and `INFO_OUTPUT_FORMAT_CHANGED` is ignored.
  - Sources with more than 2 channels are fed to the encoder as stereo.
- **What happens:** HE-AAC (SBR or parametric stereo) is about 2x slow and an octave low. 5.1 sources come out garbled. The broken file is synced.
- **Fix idea:** On a format change, rebuild the resampler from `decoder.outputFormat`, and downmix to stereo.
- **Confidence:** likely

### 22. The streak line on the Timer screen is always empty
- **Where:** `main.slint:1775,2977`. `set_streak_text` is never called (GTK shows the streak there).
- **Fix idea:** Set it whenever stats refresh.
- **Confidence:** confirmed

### 23. Layouts overflow at 360dp or with longer translations
- **Recovery dialog** (`main.slint:8672-8693`): three text buttons, about 360px in English and 480px in German, in a card about 292px wide. "Push My Data" spills off the card.
- **Interval-bell and vibration editor headers** (`:6143-6150`, `:6642-6649`): the title has no `min-width: 0` / elide, so Save is pushed off-screen in ru/de/es/fr.
- **`CompactToggle`** (`:626-674`, used at `:6177`): segments can't shrink, so the third "Kind" option is clipped in fr/es/nl/it/pt_BR.
- **Stats period `SegmentedButton`** (`:4818-4829`): about 385px against about 332px.
- **Fix idea:** Stack the recovery buttons. Add elide to the titles. Use equal-width segments, as `ModeToggle` does.
- **Confidence:** likely (estimated from font metrics)

### 24. The newest bar in the 3-month chart covers only 6 days
- **Where:** `meditate-core/src/date_math.rs:190` (90 days) and `:252-260` (`chunks(7)`).
- **Fix idea:** Use 91 days, or chunk from the newest end.
- **Confidence:** confirmed

### 25. The week always starts on Monday on Android
- **Where:** `meditate-core/src/date_math.rs:51-61`: the locale lookup is `cfg(target_os = "linux")` only.
- **What happens:** en-US phones get Monday-first heatmap rows and week-over-week. There is no effect for a German locale.
- **Fix idea:** Have the shell pass the Android locale's first weekday.
- **Confidence:** confirmed

### 26. Edit Session ignores the 12-hour clock
- **Where:** `main.slint:5233-5235` (hand-built "HH:MM") and `:5320` (`use_24_hour_format: true`). The Log uses the locale's AM/PM.
- **Confidence:** likely (only matters on 12-hour locales)

---

## C. Cosmetic

### 27. Untranslated units: "h", "m", "s"
- **Where:**
  - `lib.rs:1782-1790` (`render_hm`), which feeds the Total tile, goal ring, chart axis, insights and Log day caption.
  - `main.slint:5202-5203` ("1 ч 30m"), `:1712` and `:3294` (`seconds + "s"`).
- **Fix idea:** Plural Tr functions per unit. The GTK .po already has the translations.

### 28. The "By label" subtitle is English
- **Where:** `lib.rs:1949-1953` (`"{dur} · {n} sessions"`).
- **Fix idea:** Reuse `Tr.day-caption`.

### 29. Month and weekday names are English and in US order
- **Where:** `lib.rs:2002,2009` (heatmap range), `:2057-2064` (chart axis), `:2303` (Longest-session insight). chrono has no locale support.
- **Fix idea:** Build them from numbers through Tr arrays (like `Tr.day-month`).

### 30. Date and time picker chrome is English
- **Where:** the Material `date_picker.slint` / `time_picker.slint` strings ("Ok", "Hour", "Minute", "Enter date", the weekday letters with msgctxt) aren't in any .po. `on_format_date` (`lib.rs:3610`) uses chrono's English names.
- **Fix idea:** Add those msgids to every .po, and translate the names in `format_date`.

### 31. Split sentences around preset names, and mismatched quotes in Chinese
- **Where:** `main.slint:7494-7495` (Delete Preset) and `:7382-7383` (Override). zh shows `'名称”`.
- **Fix idea:** Use single msgids with `{}`.

### 32. German mixes "Label" and "Kategorie"
- **Where:** `lang/de/.../meditate-android.po`, e.g. lines 176, 320, 323 vs 83, 110, 833.
- **Fix idea:** Pick one term.

### 33. Leftover running-screen content for one tick
- **Fixed (2026-10-08):** R7(a): every session end goes through `end_session`, which resets both.
- **Where:** `bb_running_active` is reset only on a natural Box Breath end (`lib.rs:5693`). `overtime-add-label` is never reset.
- **What happens:** The old breathing square, or the old "Add MM:SS", flashes for up to 200 ms on the next session.
- **Fix idea:** Reset both on start.

### 34. The Log filter dropdown keeps a renamed label's old name
- **Where:** `lib.rs:3299-3302`. Material `drop_down_menu.slint:122` updates only on an index change.
- **Fix idea:** Bounce the index, or refresh the dropdown when its items change.

### 35. The Log day caption total disagrees with the card minutes
- **Where:** `meditate-core/src/format.rs:389` rounds; the caption (`hm_compact_key`) floors.
- **What happens:** Two 10m40s cards show "11 min" each, but the caption says "21m".
- **Fix idea:** A core `log_day_caption` that sums the cards' `log_card_minutes`. One rounding rule alone does not fix it: 11 + 11 = 22, while the rounded sum of 21m20s is 21. GTK has the same bug (log/imp.rs:641-650 floors, cards round at 404), so fix both apps.
- **Do with it:** the other Stats fixes (#18, #24) are a good moment to also move the duplicated Stats inputs into core (`insights::input_from_db`, `goal::from_db`), see REFACTOR-AUDIT.md R13.

### 36. CSV export isn't in chronological order
- **Where:** `meditate-core/src/data_io.rs:127-130`: rows are by id, reversed, despite the comment.
- **Fix idea:** Sort by `start_iso` ascending.

### 37. The app looks frozen while a picked file is copied
- **Where:** `MeditateFilePickerActivity.kt:112-124`: the translucent activity eats every touch until the copy and probe finish.
- **Fix idea:** Finish the activity at once and let the worker write the drop-file.

### 38. Codec and extractor leak when import setup fails
- **Where:** `MeditateGuidedImport.kt:131-184` (created before the `try/finally` at `:269`), and `probe()` in `MeditateFilePickerActivity.kt:221-249`.
- **Fix idea:** Bring setup into the `try/finally`.

### 39. The sync status icon has no accessible label
- **Where:** `main.slint:5393-5399`. `sync-indicator-tooltip` is set but never read.
- **Fix idea:** `accessible-role: button; accessible-label: root.sync-indicator-tooltip;`
