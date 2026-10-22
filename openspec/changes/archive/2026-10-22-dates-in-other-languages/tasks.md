# Tasks: Dates in other languages

Base: 1955a96 (beta); suites on it: green (core 254, android 1386, gtk 153)

## Loop
- [x] 0 Skills loaded, again after every /compact: `/ponytail ultra`, `slint:slint` (Android UI touched)
- [x] 1 Understand
- [x] 2 Propose (approved: 2026-10-22)
- [x] 3 Build
- [x] 4 Review, then `/simplify` after the last review
- [x] 5 Specs, docs, list (check-specs green; AGENTS.md translation rule; OPEN-ISSUES batch O ticked)
- [x] 6 Verify and hand over (Linux and FP5 tested and approved 2026-10-22)
- [x] 7 Ship (specs merged, archived)

## Review Focus
- A language without a translation (Swedish on Linux): the English pattern is used, as today. No scenario.
- Russian month forms: host probe under ru_RU.utf8 (2026-10-22): %b gives "окт", %B the genitive "октября", so "9 окт" and "Четверг, 9 октября". Kept (my call).
- 29 February in an earlier year: Android renders the header from the real date (with its year), never a stand-in year. Covered by the changed DayHeader carrying the date.
- Today / Yesterday headers: unchanged, still covered by `day_header` tests.
- The Material date picker on Android: unchanged, it already uses skeletons.
- Weekday labels getting wider on Linux (English "Mon", Portuguese "seg."): a 7-bar chart has room; Android shows them so already. MANUAL "Seen in Chinese and Portuguese".

## 1. Core rules
- [x] 1.1 `date_math::month_initial(name)`: leading digits if the name starts with one, else the first character. Proof: `a_month_label_keeps_a_leading_number`, FAIL (stub) then PASS.
- [x] 1.2 `format::date_shows_year(unix, now)`: true only for `EarlierYearOther`. Proof: `the_longest_session_names_its_year_only_from_an_earlier_year`, FAIL (stub) then PASS.

## 2. Linux (#151, #153, #154, #116)
- [x] 2.1 `src/format.rs`: `short_date(dt, with_year)` with msgids "%b %-d" / "%b %-d, %Y" and `heatmap_date(dt)` with "%A, %B %-d", each with a comment for translators above it. Proof: `a_short_date_has_no_padded_day`, `the_spoken_heatmap_date_has_no_padded_day` (C locale, untranslated), FAIL then PASS.
- [x] 2.2 Use them: Log headers (`date_group_display`), edit dialog (`format_date`), chart `MonthShortDay`, heatmap name, longest session (with `date_shows_year`); month letter through `month_initial`; `weekday_for` without `take(2)`. Proof: `a_weekday_label_is_the_whole_short_name` FAIL then PASS; full gtk suite.
- [x] 2.3 `build-aux/update-translations.sh`, then the three msgstrs in all nine .po; `msgfmt --check` each. Proof: `every_language_writes_dates_in_its_own_order` FAIL (empty msgstr) then PASS; `cargo test -p meditate i18n::`.

## 3. Android (#152, #153, #116)
- [x] 3.1 Test first: `log_sections_render_through_core` requires `locale_date(` with "MMMd"/"yMMMd" in `day_header_text` and no `day-month` in `ui/main.slint`; `day_header` tests carry the date. FAIL then PASS.
- [x] 3.2 `DayHeader::Date { date, with_year }`; `day_header_text` via `locale_date`; drop the two Tr functions and their entries in the nine .po; month letter via `month_initial` on "MMMMM"; longest session "yMMMd" when `date_shows_year`. Proof: android suite, clippy.

## Notes
- Pre-build questions (fresh agent, 2026-10-22), all my call:
  - The year rule in the po test is presence, not position (Chinese puts it first).
  - Valid pattern: literal text plus %b %B %-m %-d %Y %A %a; %d and %e are rejected in every language. Month is %b, %B or %-m.
  - Chinese uses "%b%-d日", "%Y年%b%-d日", "%-m月%-d日%A" (probe: zh %b is "10月", %B "十月").
  - `DayHeader::Date` falls back to the UTC date when the local lookup fails, instead of a zero date.
- `DayHeader::Date` falls back to the UTC date, then to 1970-01-01 only for a timestamp chrono cannot represent.
- Android's chrono fallback (no JNI) follows the same year rule as the skeleton.
- The heatmap msgid's translator comment also carries the function's doc comment ("#. / ..."); harmless.
- Review 1 (2026-10-22):
  - 1 Linux UI language and format language differ (LANG=en_US, LC_TIME=de_DE, as on the maintainer's laptop): order from the UI language, names from LC_TIME, "Okt 9". Maintainer: follow Formats. First fix (dcgettext with LC_TIME plus LC_TIME links at install) replaced after fix review 1.
  - 2 German THEN said "9. Okt." for Linux; glibc's %b is "Okt". Fixed in the delta.
  - 3 Living stats scenario said "one letter per month"; MODIFIED delta added.
  - 4 One JNI call per Android Log header (cost on a long Log). Not fixed up front: added to the device test (long Log, Load more, then edit a session); cache only if it stutters.
  - 5 The po test counted fuzzy entries; po_entries now skips them. Proven by marking de "%b %-d" fuzzy: FAIL, restored: PASS.
- Fix review 1 (2026-10-22):
  - 1 glibc lets LANGUAGE (Ubuntu: "en_US:en") override LC_TIME, so dcgettext followed the UI language there. Replaced: `format::date_pattern` takes the locale from LC_ALL / LC_TIME / LANG (what setlocale("") uses) and reads the msgstr from that language's .mo in LOCALEDIR, cached per pattern. Tests `a_locale_names_its_catalogs_most_specific_first`, `a_date_pattern_is_read_from_a_compiled_catalog` (FAIL with stubs, then PASS). The meson LC_TIME links are gone; xgettext keyword is `date_pattern`; the i18n scanner reads `date_pattern("...")`.
  - 2 Dev runs without LC_TIME links: gone with the links; dates use the same catalog dir as the UI text.
  - 3 MODIFIED stats scenario wrongly promised no repeated label for letters (J, M, A repeat): reworded to no repeated month.
  - 4 AGENTS.md translation rule named only gettext: now also names `date_pattern`.
- Fix review 2 (2026-10-22): PASS. One MINOR taken: the Formats locale now comes from `setlocale(LC_TIME, NULL)` (glibc's own result, "C" when a named locale isn't generated) instead of re-reading LC_ALL / LC_TIME / LANG. GTK suite 159 green.
  - Known limit: the two C-locale format tests would read a catalog if `meditate-gtk/build/po` existed and another test's `adw::init` had set the locale first; that dir doesn't exist in a normal checkout (dev catalogs live in the repo-root build/po).
- /simplify (2026-10-22), applied:
  - The longest session's year is decided in core: `InsightKey::LongestSession` carries `with_year` (`insights::compute` with its `now_unix`); both apps only render it. `longest_session_appears_only_when_some` asserts both values.
  - Android: one `short_date(date, with_year)` beside `locale_date`, used by the Log header and the insight; one `app::local_date` (chrono `earliest`) for both.
  - GTK: `config::locale_dir()` shared by `setup_gettext` and the date patterns; the week chart's weekday comes from the already parsed date, `weekday_for` removed; the weekday test goes through `x_label_text`.
  - Small: test `match` without the unused pattern, a double blank line, a `ponytail:` note on the .mo scan's limit.
  - Skipped: reading the catalog once for all three patterns (sub-millisecond, once per process); merging `formats_languages` with Android's `translation_candidates` (different input syntax); the O(n^2) `months` rebuild in `x_label_text` (old code, not this change).
  - After: core 1388, android 254, gtk 159 green; clippy (gtk+core all targets, CI lib -D warnings, Android target) clean.

## Hand-over (2026-10-22)
Suites: core 1388, android 254, gtk 159 green; clippy clean (three runs); check-specs green.
Artifacts:
- APK (debug, 10:22): `meditate-android/android/app/build/outputs/apk/debug/app-debug.apk`; the old `{1}/{0}/{2}` pattern is gone from its .so.
- Linux (debug, 10:22): `meditate-gtk/builddir/src/meditate`, catalogs in `meditate-gtk/builddir/po`.

Test steps:
1. Linux, your desktop as it is (English UI, German Formats): `LOCALEDIR=meditate-gtk/builddir/po ./meditate-gtk/builddir/src/meditate`. Log headers "9. Okt" / "9. Okt 2025"; the edit dialog's date "9. Okt 2025"; the rest English.
2. Same with `LANGUAGE=en_US:en` in front (Ubuntu's default): the same dates.
3. Same with `LANGUAGE=de` in front: German UI, the same dates.
4. Linux Stats: week chart weekdays "Mo Di Mi ..."; 28-day chart "19. Okt" style labels; year chart one letter per month; longest session with year if it is from an earlier year; Orca (optional) reads a heatmap cell as "Donnerstag, 9. Oktober".
5. Linux with `LC_ALL=zh_CN.UTF-8` and then `LC_ALL=pt_BR.UTF-8`: year chart labels "10", "11", "12" with no repeated "1"; week chart seven different weekdays.
6. Phone (German): Log headers "9. Okt." and earlier years "9. Okt. 2025"; Stats longest session with year when from an earlier year.
7. Phone in English (UK): headers "9 Oct". Optional: Chinese year chart "10", "11", "12"; Portuguese week chart seven different weekdays.
8. Phone: a long Log, tap "Load more" a few times, then edit a session: the list must not stutter (one date lookup per header now).

Proposed commit message:
```
dates: Write dates in each language's own order

Linux wrote dates in English order ("Okt 9, 2025" in German), Android's
Log headers used a number pattern ("10/9" for 9 October in en-GB),
Chinese charts labelled October to December "1", Portuguese weekday
labels repeated, and the longest session gave no year.

Linux takes its date patterns from the language of the Formats
setting, read from that catalog directly because LANGUAGE would
override LC_TIME. Android uses the phone's own short date. A month
label keeps a leading number, and the longest session shows its year
when it is from an earlier year.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_012WGZUPYHL7TurMz6YSFVJs
```
- Device test, Linux (2026-10-22): steps 1 to 5 passed (maintainer). Phone steps 6 to 8 open.
