# Tasks: meditation-assistant-import

Base: 5106ca935c4279b13a17e0a5f956d814711f2a3f; suites on it: green (core 1465 re-run 2026-11-07; Android lib 295, GTK 171 on the same commit)

## Loop
- [x] 0 Skills loaded: `/ponytail ultra`; `slint:slint` (Android UI)
- [x] 1 Understand
- [x] 2 Propose (approved: 2026-11-07)
- [x] 3 Build
- [x] 4 Review (R1; fixes are a wrap and a test assert, no fix review), then `/simplify`
- [x] 5 Specs, docs, list (check-specs green; README features line; OPEN-ISSUES has no #16 entry)
- [x] 6 Verify and hand over (maintainer 2026-11-07: Linux tested, all fine; phone test waived by the maintainer: "das reicht mir so")
- [x] 7 Ship

## Review Focus
- A Meditate backup or an Insight Timer file picked in the new row -> every line unreadable, the existing "Nothing in this file can be imported." question
- Length over 24 h or minutes 60+ -> unreadable duration (Meditation Assistant itself refuses those)
- A session under one minute (00:00) -> "A session under a minute" (imports as 1 minute)
- Digits other than 0-9 (Meditation Assistant formats the length in the phone's locale) -> unreadable duration, reported, not guessed
- A note spanning lines (quoted) -> the csv reader keeps it; line numbers via the record's position
- An empty file or header only -> zero rows, "Imported 0 sessions" path as for the other imports
- Another device -> imported sessions sync like any import
- Another language -> the row label at 360 px, MANUAL in German

## Tasks
- [x] 1 Core: `parse_meditation_assistant_csv` + the four core tests (all failed on a stub first; "imported twice" proves the existing duplicate skip of `insert_imported` for this reader)
- [x] 2 Android: `app::read_picked_csv(path, kind)` + its test; `ui.rs` calls it; Kotlin target `import-meditation-assistant` -> kind `meditation-assistant`; Slint row + callback
- [x] 3 GTK: row + `data_io::parse_meditation_assistant`
- [x] 4 Translations: nine `.po` per app
- [x] 5 README features line
- Note: `local_times_are_converted_by_core` lost its ui.rs assert on `insighttimer_started_at`: the call moved to `app::read_picked_csv`, which `each_picked_csv_kind_reads_with_its_own_format` runs on the host

## Pre-build questions (fresh agent), answered
- Started UTC? Yes: `exportSessions` sets UTC; its import dialog says "Exports saved since version 1.5.6 have UTC dates and times" (1.5.6 = 2019-09-11). An export from before that is local time and lands shifted (maintainer: go by the source, 2026-11-07)
- Note kept verbatim: no `csv_unguard` (that undoes only our own export guard) (my call)
- Translations drafted as in earlier changes, nine per app (my call)
- Header not checked: columns by position, as Meditation Assistant's own import reads them; a foreign file fails line by line into "Nothing in this file can be imported." (my call)
- Sessions under a minute (00:00): import as 1 minute (maintainer, 2026-11-07)

## Review R1 (fresh agent): spec PASS, quality NEEDS FIXES
- 1 IMPORTANT, Android label cut off at 360 px (de/fr measured 264-265 px of ~265): fixed, the row label (`DurationRow`) wraps; the row stays 56px, two lines fit. MANUAL scenario now names it
- 2 MINOR, lost proof that Insight Timer picks use core's DST-safe conversion: fixed, `each_picked_csv_kind_reads_with_its_own_format` compares the start with `insighttimer_started_at`
- 3 MINOR, pre-2019 exports (local time) land shifted without warning: rejected, the maintainer chose to go by the source; a current export is UTC
- No fix review: the fixes are a text wrap and a test assert, no behaviour, data or sync change

## /simplify
- Fixed: the four copied Data-page file rows in GTK `preferences.rs` are one `file_row` helper
- Skipped: a core `ImportFormat` enum for the kind strings (three formats, would reach Kotlin and both shells; beyond this change)
- Skipped: a shared record loop / Session literal for the three parsers (each reads different columns and errors; a helper would hide that)
- Skipped: dropping the Insight Timer start assert in the Android test (added on purpose by R1: core tests can't see the app's dispatch)
- Efficiency: nothing

## Hand-over
Artifacts: `meditate-android/android/app/build/outputs/apk/debug/app-debug.apk` (2026-11-07 20:02), `meditate-gtk/builddir/src/meditate` (20:02, started with `XDG_DATA_HOME=~/.cache/meditate-ma16`, German). Test file: `~/Downloads/meditation-assistant-sessions.csv` (the issue's export).
Suites: core 1469, Android lib 296, GTK 171; clippy clean (CI, GTK, Android target); check-specs green.

Test steps:
1. Linux: Preferences, Data, Migrate: "Aus Meditation Assistant importieren" under Insight Timer. Pick the test file: toast "3 Sitzungen importiert"; Log shows 2026-10-30 at 22:11 (4 min), 21:09 (7 min), 19:36 (5 min, note "That's a column for notes"), no label.
2. Linux: import the same file again: nothing added.
3. Phone (German): Einstellungen, Daten: the new row's label shows in full (two lines are fine). Import the same file (copy it to the phone): same three sessions, same times.

Commit message:
```
data: import from Meditation Assistant

Meditation Assistant's CSV export now imports in both apps, from a
row in the Migrate group. Its start times are UTC, its lengths whole
minutes (a session under a minute comes in as one minute), its message
becomes the note. Long Android row labels wrap instead of being cut.

Fixes #16

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_012WGZUPYHL7TurMz6YSFVJs
```
