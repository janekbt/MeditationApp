# Tasks: Log file name and Local Folder refresh

Base: 4417a9b12cec9b630d44018051aac2715db49caa

## Loop
- [x] 1 Understand (request from the maintainer 2026-10-12; no OPEN-ISSUES entry)
- [x] 2 Propose (approved 2026-10-12, with percentage; snapshot name in mode_payload if it fits)
- [x] 3 Build (an_unreachable_folder_reports_no_refresh passed on the stub; shown to fail without the unreachable check)
- [x] 4 Review (fresh agent 2026-10-12: FAIL / NEEDS FIXES; all six handled below)
  - 1 missing MODIFIED delta: added (it was written while the review ran).
  - 2 failed write still toasted: fixed, `a_failed_write_reports_no_refresh`; Android now clears `scanning` after the save.
  - 3 Android scan that never starts kept the tap: the tap counts only when busy after `local_scan` (GTK the same).
  - 4 stale design/tasks/proposal text: fixed.
  - 5 Android button not tappable while busy: now a TouchArea, like GTK.
  - 6 downgrade then re-update skips the replay: noted in ARCHITECTURE.md, not fixed (needs a downgrade).
- Device: the first start after install was slow (FP5 debug: replay of 21,900 events from the CACHE_SCHEMA_VERSION bump). Replaced by a one-time fill when the column is added; bump reverted. Same test.
- [x] 5 Specs, docs, list (EVENTS, ARCHITECTURE, README, OPEN-ISSUES #187)
- [x] 6 Verify and hand over (maintainer tested on the FP5, 2026-10-12: all passed)
- [x] 7 Ship

## Review Focus
- A second device -> payload field (`a_guided_session_carries_its_file_name_to_a_peer`).
- A replayed event from an older peer -> `an_edit_without_a_file_name_keeps_the_stored_one`.
- An old database -> `a_database_from_the_last_release_gains_the_file_name_columns`.
- A deleted library file -> the name comes from the library lookup as today (deleted names kept from the last insert event), else the stored name.
- A session from before this change -> no stored name, card as today.
- Another language -> one new string in nine languages, both apps; short enough for 360 px.
- A tap while a refresh runs -> one toast at its end.
- A tap on an unreachable folder -> banner, no toast.
- A refresh the page started itself -> spins, no toast.
- A folder with hundreds of new files -> spins until every length is read; the folder line shows the count meanwhile.
- Old CSV without the tenth column -> the reader is flexible; field reads empty.

## 1. Log name (core first, then apps)
- [x] 1.1 Old database opens with the column added
- [x] 1.2 Payload carries the name; peer stores it
- [x] 1.3 Apply keeps the stored name when the payload has none
- [x] 1.4 Recovery snapshot keeps the name
- [x] 1.5 CSV round trip keeps the name
- [x] 1.6 Log name rule; both Log cards use it
- [x] 1.7 GTK and Android pass the pick's title on save and snapshot

## 2. Local Folder refresh
- [x] 2.1 Core `RefreshProgress` (tap, settle)
- [x] 2.1b `RefreshProgress::percent`; drop the folder line's reading count
- [x] 2.2 GTK: spinner while busy, flag on tap, toast
- [x] 2.3 Android: spinner while busy, flag on tap, snackbar
- [x] 2.4 "Folder refreshed" and "{n}%" in nine languages, both apps; "reading" string removed

## 5. Hand-over
- [x] 5.1 check-specs: only the living-spec line that the MODIFIED delta replaces at archive.
- [x] 5.2 Docs: EVENTS (payload field, no-name rule, local folder line), ARCHITECTURE (snapshot, fixtures, downgrade note), README (Log line).
- Artifacts: `meditate-android/android/app/build/outputs/apk/debug/app-debug.apk` (21:17, on the FP5 21:17). No Flatpak: the maintainer tests on the phone only this time (2026-10-12); laptop steps 2 and 4 skipped.
- FP5 2026-10-12: the folder showed 0 files because Android's ExternalStorageProvider hid it (restart fixed it); recorded as OPEN-ISSUES #187. Phone data checked after the slow first start: integrity ok, 2991 sessions = live session events, matches the laptop.
- Test steps:
  1. Phone: play a Local Folder file for over a minute, save. The Log card names the file.
  2. Laptop: play an Open File pick, save. Sync both. Both Logs name both files.
  3. Phone: add two audio files to /sdcard/Music/Meditate-Test, open Local Folder, tap refresh. Spinner, rising percent, then "Ordner aktualisiert" once.
  4. Laptop: the same with a folder of your own.
  5. Tap refresh with nothing new: short spin, then the message.
- Proposed commit message:
  guided: file name in the Log, Local Folder refresh shows progress

  Sessions from the Local Folder or Open File showed no name in the
  Log. A guided session now stores and syncs the title it played (an
  added column and payload field; an older peer's edit keeps it, a
  one-time fill adds names an older release kept only in events). The Local
  Folder refresh button spins with a percentage while it reads and,
  after a tap, says when it is done.
