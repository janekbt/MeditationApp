# Tasks: Refresh says nothing false when length reading can't start

Base: f13c5360311891a1f9f0cab0b6cd381084e0b31b

## Loop
- [x] 1 Understand (second review of f4c6b81, finding 2; confirmed in ui.rs read_local_lengths)
- [x] 2 Propose (approved 2026-10-12 in chat: reuse "Couldn't read audio file")
- [x] 3 Build (core 1367, GTK 115, Android 211 tests; three clippy runs clean)
- [x] 4 Review (fresh agent 2026-10-12: FAIL / NEEDS FIXES)
  - 1 a pick whose read can't start stayed selected, Start blocked: fixed, not selected (as the async failure path); read_local_lengths returns whether it started.
  - 2 any failure during a tapped refresh suppresses its success: kept, nothing false is shown; the failure itself is.
  - 3 #188 belongs to batch W: moved.
  - Re-review of fix 1 (2026-10-12): PASS; one minor: unstarted ids stayed queued ("0%" while walking) -> marked finished.
  - Extra: OPEN-ISSUES #183 heading restored (cut by f4c6b81).
- [x] 5 Specs, docs, list (OPEN-ISSUES #188 in batch W, #183 heading restored)
- [x] 6 Verify and hand over (maintainer tested on the FP5, 2026-10-12: both steps pass)
- [x] 7 Ship

## Review Focus
- A pick (no tap) whose read can't start -> forgotten (`a_failure_with_no_tapped_refresh_is_forgotten`).
- A tapped refresh with a save failure -> still no success (`a_failed_write_reports_no_refresh`).
- Both callers of read_local_lengths (scan result, pick) show the notice.

## Tasks
- [x] 1 Core: write_failed counts only with a tap pending; test first
- [x] 2 Android: read_local_lengths failure marks the run and shows the notice; drop the busy gate in local_save
- [x] 3 GTK: drop the busy gate in save()
- [x] 4 OPEN-ISSUES #188

## Hand-over
- APK: `meditate-android/android/app/build/outputs/apk/debug/app-debug.apk` (21:43, on the FP5 21:43).
- Test steps (the failure itself can't be caused on the phone):
  1. Local Folder: tap refresh. Spinner, then "Folder refreshed" once, as before.
  2. Pick a file in Folders view: it becomes the current file and plays.
- Proposed commit message:
  guided: no false "Folder refreshed" when length reading can't start

  On Android a length reading that couldn't start was dropped
  silently; a tapped refresh still said "Folder refreshed" and a
  picked file waited forever for its length. Now it says "Couldn't
  read audio file", the refresh shows no success and the file isn't
  selected. A failure counts only for a refresh that waits for its
  message, so a failed pick can't silence a later one. Also files
  #188 (found by the same review) and restores the #183 heading.
