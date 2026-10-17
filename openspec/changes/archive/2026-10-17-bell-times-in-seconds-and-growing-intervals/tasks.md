# Tasks: bell times in seconds, jitter as a time, growing intervals

Base: 8fd6ac3d2980279542cb4af9ef75de2fd94e7252; suites on it: green (`cargo test --workspace`, 1745 passed)

## Loop
- [x] 1 Understand (GitHub #5 and #7; decisions in proposal.md)
- [x] 2 Propose (approved: 2026-10-16)
- [x] 3 Build
- [x] 4 Review (two reviewers, then two rounds on the fixes; log at the end)
- [x] 5 Specs, docs, list (check-specs green; README, EVENTS.md, design; GitHub #5 and #7 are not in OPEN-ISSUES)
- [x] 6 Verify and hand over (device test passed 2026-10-17)
- [x] 7 Ship (2026-10-17)

## Review Focus
- A second device on the release build edits a bell -> "A synced bell from an older version"; its step and seconds are lost, accepted.
- A release build reads a new bell or preset -> "A bell written now, read by an older version", "A preset saved before this change".
- Downgrade on one device, edit, upgrade again -> "A bell changed by an older version on this device".
- An older version switches a bell on or off, or applies a preset -> same as an edit (full-row update); README note says so.
- Unit flipped past the unit's limit -> clamped like a typed number (proposal, my call).
- Kind switched from repeating to fixed with a jitter or step set -> both saved as 0 in both apps ("A fixed bell has no jitter or step").
- Time lowered below twice the jitter -> jitter capped when ringing, stored value kept ("A jitter longer than half the time").
- A preset applied that matches a library bell -> `same_settings` compares the seconds fields; covered by the preset test.
- Stopwatch with a step: gaps keep growing, no end; no cap (proposal, Left out).
- Another language: titles and rows in 9 languages each app; manual check in de and ru at 360 dp.

## 1. Core: schedule
- [x] 1.1 Time in seconds for all kinds. Files: `meditate-core/src/bells.rs`. Proof: `a_bell_set_in_seconds_rings_at_that_second`, Expected: FAIL then PASS
- [x] 1.2 Jitter in seconds, cap at half. Proof: `a_jitter_in_seconds_bounds_each_gap`, `a_jitter_is_capped_at_half_the_time`
- [x] 1.3 Step, incl. the time-jump loop. Proof: `a_step_grows_each_gap`, `a_step_survives_a_time_jump`
- [x] 1.4 Display unit and title key. Proof: `the_title_key_carries_each_time_with_its_unit`

## 2. Core: data
- [x] 2.1 Added columns, read fallback. Files: `db/schema.rs`, `db/interval_bells.rs`, fixtures. Proof: `a_bell_from_the_last_release_keeps_its_times`
- [x] 2.2 Events write both, apply prefers new keys. Proof: `a_time_keeps_the_unit_it_was_set_in`, `a_synced_bell_from_an_older_version_reads_in_seconds`, `a_bell_in_seconds_also_writes_minutes_and_percent`, `a_row_an_older_version_changed_reads_from_its_minutes`
- [x] 2.3 Preset bells, `same_settings`, apply; every `insert_interval_bell` caller. Proof: `a_preset_bell_without_seconds_reads_from_minutes`

## 3. GTK
- [x] 3.1 Editor: kind "Repeating", Time + s/min toggle, Jitter + toggle, Step + toggle (repeating only); title from the key. Files: `meditate-gtk/src/bells.rs`, `src/db/mod.rs`. Proof: title tests in `meditate-gtk/src/bells.rs` (unit-tested formatting), editor MANUAL
- [x] 3.2 Translations via `build-aux/update-translations.sh`, 9 languages, `cargo test -p meditate i18n::`

## 4. Android
- [x] 4.1 Interval editor: Time, Jitter, Step steppers each with a CompactToggle s/min; load, clamp and save in seconds. Files: `ui/main.slint`, `src/ui.rs`, `src/app.rs`. Proof: clamp tests in `app.rs`, editor MANUAL
- [x] 4.2 Title strings and 9 `.po` files; `msgfmt --check`

## 5. Docs
- [x] 5.1 README (feature line, "update every device together"), fastlane full description, EVENTS.md payload and version-skew note, ARCHITECTURE.md if it lists migrations, bells/timer living specs via deltas

## Notes
- Fastlane store descriptions left as they are: "interval bells (fixed, periodic, or randomised)" is still true in all ten languages. (my call)
- Android: the editor's "Time" uses the context "Bell time", because the Log's "Time" (time of day) already has its own translation.
- Core order slip: the schedule and data code (1.x, 2.x) was written before its tests. Each of the 12 new tests was then run against a one-line break of the behaviour it proves and failed, and passed with the code restored (mutation check, 2026-10-16). Recorded here instead of deleting and retyping identical code.
- Spec fix while building: the title example "Every 60 s ±1 min" breaks the jitter cap (half of 60 s is 30 s); the scenario now uses 120 s, plus a capped case.
- Fresh read before showing (2026-10-16), 10 questions: 1 (stored vs derived unit) asked Janek: stored. 2-10 decided as my call, now in proposal Decisions and design.md (older version also resets on enable toggle and preset apply; jitter cap at use not at save; clamp on unit flip; Option preset fields; drift kept; half-up rounding from seconds; read-time detection of older-build rows; title templates per kind and unit; step 0-60 min, repeating only).

- Device test 2026-10-17: all steps passed. Janek asked for equal-width s/min halves: phone uses `ModeToggle` (96 px), laptop `homogeneous(true)`. Layout only, no review.
- German units changed to "Sek." and "Min." in the bell strings of both apps (Janek, 2026-10-17); preset subtitles and stats durations keep "Min".

## Review log
Two reviewers (data and sync; completeness, both apps and rules), 2026-10-16:
- R1 (CRITICAL, held): a device on 26.10.1 that applied a newer device's bell event kept only minutes and percent; after updating, its first toggle synced the rounded values back. Fixed: bell rows are rebuilt from their events on every open. Tests `a_bell_a_newer_device_sent_before_the_update_keeps_its_seconds`, `a_change_received_on_the_older_version_is_kept_after_updating_again`. These tests also found that events wrote `jitter_secs_unit`/`step_secs_unit` instead of the documented `jitter_unit`/`step_unit`; fixed with one key table.
- R2 (IMPORTANT, held): downgrade, peer change keeping minutes and percent, upgrade read a stale step. Fixed by R1.
- R3 (held): GTK kept jitter and step on a switch to a fixed kind, the phone zeroed them. Fixed: core `bell_kind_offsets` decides, both apps call it, GTK resets the rows. Test `a_fixed_bell_keeps_no_jitter_or_step`.
- R4 (MINOR, accepted): 1 min ±1 % becomes ±1 s, written back as 2 %. No exact value exists; noted in design.md Risks.
- R5 (held): the unit test of "A time keeps the unit" did not check titles. Added.
- R6 (held): the phone title was claimed with "±" and has no test. Scenario added: MANUAL with "+/-".
- R7 (held by R1): a release-build edit of only the sound left devices disagreeing. With the rebuild on open both read it from the event.
- R8 (held): proposal still had the 60 s example and listed fastlane/timer spec; doc comments of `write_back_bell` and `render_bell_title` landed on new items. Fixed.
- The completeness reviewer saw a red suite: it read the tree while R1 and R3 were being fixed.

Fresh review of R1 to R8 (2026-10-16): PASS, three MINOR:
- R9 (held): the rebuild walked every bell uuid in the event log (311 on the laptop's database, for 5 rows) with a full scan of events, on every open and sync. Fixed: it walks the existing rows that have events. (First tried all rows: three release-fixture tests failed because a row without events was deleted; rows without events are now left alone.)
- R10 (held): `a_row_an_older_version_changed_reads_from_its_minutes` never reopened the database and wrote no release event. Now it writes the release's update event too and checks the bell before and after reopening.
- R11 (no change needed): events written with the pre-fix unit key names would fall back to minutes. Only this unreleased working tree wrote them; no build of it reached a device or a synced database (the APK on the FP5 is batch M).

Review of R9 and R10 (2026-10-16): R9 sound. R10 (MINOR, held): the test passed by the read check alone; it now also checks the stored seconds after reopening, which only the rebuild sets. Wording, test-only: no further review.

## Hand-over (2026-10-16)
Verified: `cargo test --workspace` 1759 passed; clippy (GTK + core all targets, CI form, Android target) clean; check-specs green; msgfmt --check on all 18 .po files.
Artifacts: Flatpak installed 2026-10-16 17:18 from the working tree; debug APK `meditate-android/android/app/build/outputs/apk/debug/app-debug.apk` 17:21 (not yet installed: phone disconnected).

Test steps (German UI):
1. Laptop, Intervallglocken, new bell: Art "Wiederholt", Zeit 120 with s, Streuung 1 with min, Schritt 5 with s. Row reads "Alle 120 s ±1 Min, +5 s"; reopened, the editor shows the same numbers and switches.
2. Laptop: Zeit 200 with s, flip to min: the number becomes 120.
3. Laptop: switch the bell to "Zeitpunkt ab Beginn": Streuung and Schritt disappear, the row reads "Bei 120 s"; back to "Wiederholt": both are 0.
4. Laptop: existing bells keep their times ("Alle 5 Min" etc.); a percent jitter now shows as s or min.
5. Laptop or phone: a 2-minute countdown with one bell "Wiederholt", Zeit 20 s, Schritt 5 s: it rings at 0:20, 0:45, 1:15, 1:50.
6. Phone, Intervallglocke editor in German and Russian, portrait: Zeit, Streuung, Schritt each with an s/min switch under the number; nothing cut off; the list row reads "Alle 120 s +/-1 Min, +5 s".
7. Phone: type a number into Zeit and tap the switch without leaving the field: the typed number stays (clamped).
8. Both updated and synced: a bell set in seconds on one device shows the same time, unit, jitter and step on the other.

Proposed commit message:

    core, gtk, android: Bell times in seconds, jitter as a time, growing gaps

    Interval bells took only whole minutes and a jitter in percent.
    Every bell time is now set in seconds or minutes and keeps the
    unit it was set in, the jitter is a time, and a repeating bell
    can grow its gap by a step on each ring. Older versions still
    read these bells in whole minutes and percent, and bell rows are
    rebuilt from their events on open, so a device that updates
    second keeps the seconds.

    Fixes #5
    Fixes #7
