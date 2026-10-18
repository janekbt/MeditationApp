# Tasks: Stats numbers agree (batch N)

Base: 6e1d58c613de7457031f360fefedfdf6e38f2b70; suites on it: green (core
1378, android --lib 226, GTK 151). Working tree: only the #159 rule in
OPEN-ISSUES.md from earlier today; Janek agreed it rides along in this commit.

## Loop
- [x] 0 Skills loaded, again after every /compact: `/ponytail ultra`, `slint:slint`
- [x] 1 Understand
- [x] 2 Propose (approved: 2026-10-18)
- [x] 3 Build
- [x] 4 Review (one reviewer, PASS; two MINOR findings fixed, see Notes)
- [x] 5 Specs, docs, list (check-specs green; no doc names the changed functions; OPEN-ISSUES: the five removed, batch N ticked)
- [x] 6 Verify and hand over (Janek tested on the FP5 2026-10-18: all fine)
- [x] 7 Ship (approved 2026-10-18)

## Review Focus
- A future-dated session in the heatmap, week card and chart -> heatmap and insights already drop days after today; best streak gets its scenario.
- A month with no session inside the year window -> zero bar, in the year scenario.
- Today's date at a month or year boundary (Jan 1, Jan 31) -> the year test also runs on 2026-01-31 (window 2025-02-01 to 2026-01).
- Week start other than Monday (Sunday locales) -> the midweek tests run with week start 7 as well.
- Several short sessions on one day (Log floors each card at 1 min, the ring rounds the sum) -> a known, accepted difference, in the proposal.
- A long translation of "Best streak" -> the caption wraps (MANUAL scenario), never "...".
- An old database -> nothing stored changes.

## 1. Rounding (#55)
- [x] 1.1 Test `a_session_of_19_min_30_reaches_a_20_min_goal` in `meditate-core/src/goal.rs`: `compute(1170, 20)` gives `today_mins` 20, `remaining_mins` 0, status Reached. Then `(secs + 30) / 60` in `compute`. Expected: FAIL then PASS.
- [x] 1.2 Test `a_40_second_day_colours_its_heatmap_cell` in `meditate-core/src/contrib.rs`: yesterday 40 s, goal 20 -> that cell's level 1. Then round in `build_grid`. Expected: FAIL then PASS.
- [x] 1.3 Test `stats_totals_round_to_the_nearest_minute` in `meditate-core/src/format.rs`: all three keys of 40 s -> MinsOnly(1); 29 s -> Empty from `hm_compact_key`, MinsOnly(0) from `hm_secs_key` and `hm_mins_key` (each key keeps its own zero rule, only the rounding moves), 3570 s -> HoursOnly(1) for all three. Round once at the top of each key. Update `hm_secs_key_drops_sub_minute_and_emits_empty_for_zero` (30 s now reads 1m) and any other test the rounding moves. Expected: FAIL then PASS.

## 2. Week card (#59)
- [x] 2.1 Tests in `meditate-core/src/date_math.rs`: `week_over_week_leaves_out_today_until_it_has_practice` (Wednesday, week start Monday and Sunday, data Mon/Tue of both weeks plus last Wednesday -> last Wednesday not counted) and `week_over_week_counts_today_once_it_has_practice`. Then: if today's total rounds to 0 min (under 30 s, as #55 counts it), sum one day fewer on both sides; the first test also asserts a 20 s session today is left out. Expected: FAIL then PASS for the first; the second passes today (proves the existing path).
- [x] 2.2 Test `the_week_card_waits_on_the_first_morning_of_the_week` in `meditate-core/src/insights.rs`: on the first day of the week, nothing today, last week's same day 30 min -> no WeekOverWeek in the output. Follows from 2.1 (both sums 0, the existing `last_week > 0` guard). Expected: FAIL then PASS.

## 3. Chart series (#60)
- [x] 3.1 Tests `the_year_chart_has_twelve_calendar_months` (today 2026-10-09 and 2026-01-31) and `the_week_chart_has_seven_days_ending_today` in `meditate-core/src/date_math.rs`. Add `ChartPeriod::window_start(today) -> NaiveDate` (OneYear: the 1st of the month 11 months back; others: today - days + 1) and `chart_series(totals: &[(NaiveDate, i64)], today, period) -> Vec<(String, i64)>` (dense zero-filled days from the window start through today, then `aggregate_for_chart_period`). Expected: FAIL then PASS.
- [x] 3.2 GTK `stats/imp.rs::reload_chart` and Android `ui.rs::refresh_chart` read totals since `window_start` and call `chart_series`; their own zero-fill loops go. Axis and x-label code stay as they are (they key on `period.days()`). Proof: both suites green; MANUAL year chart on both devices.

## 4. Best streak (#114)
- [x] 4.1 Test `best_streak_ignores_days_after_today` in `meditate-core/src/db/sessions.rs`. `get_best_streak_from_db(db, today)` keeps days `<= today`. Callers: `insights.rs`, GTK `db/mod.rs::get_best_streak` and `stats/imp.rs`, Android `ui.rs::refresh_stats`; existing best-streak tests pass a today. Expected: FAIL then PASS.

## 5. Tile caption (#115)
- [x] 5.1 Caption "Best streak" in `meditate-gtk/data/ui/stats_view.blp` (label wraps, centred) and `meditate-android/ui/main.slint` (caption Text `wrap: word-wrap`), nine translations in each app's .po (via `build-aux/update-translations.sh` for GTK), "Streak" dropped where unused. Proof: `msgfmt --check` on each touched file, `cargo test -p meditate i18n::`, MANUAL scenario.

## Notes
- Pre-build reader (2026-10-18), my calls: a session under 30 s today does not count as practice for the week card, matching #55; `hm_secs_key` keeps "0m" for 1 to 29 s and the dash only for zero, as today.
- 2.1: `week_over_week_counts_today_once_it_has_practice` is the renamed `week_over_week_sums_only_within_current_week_window`; it passed before and after (the existing path), and fails if today never counts.
- 3.2: GTK's `Database::get_daily_totals` lost its only caller and was deleted; GTK reads core's `get_daily_totals_since_from_db` through `db.core()` like the insights do.
- 5.1: `update-translations.sh` re-wrapped the long release-note msgstrs in every GTK .po (msgmerge's line width; the text is unchanged) and offered a fuzzy "Aktuelle Serie" for "Best streak", replaced by hand.
- Suites after build: core 1386, android --lib 226, GTK 151; CI clippy, GTK/core clippy --all-targets and Android target clippy: zero warnings.
- Review 1 (PASS), both MINOR findings fixed, tests only, so no second look: the Android translation guard now checks "Best streak" instead of the dead "Streak" (`app.rs::visible_text_goes_through_translation`); `the_week_card_waits_on_the_first_morning_of_the_week` also runs on a Sunday with week start 7.

## Hand-over (step 6)
Artifacts (2026-10-18 21:48/21:49): `meditate-android/android/app/build/outputs/apk/debug/app-debug.apk` (debug), `meditate-gtk/builddir/src/meditate` (debug). After the test: the FP5 ends on the optimised build, debug key.

Test steps, both apps:
1. Stats, bottom tiles: the first reads "Best streak" in the language set (German: "Beste Serie"), in full; the number is the longest run. If possible, switch to French and Portuguese: the caption shows in full, on one or two lines.
2. Stats, year chart: 12 bars, one letter per month, November to October, no letter twice.
3. Stats, Daily goal: a day whose total ends at :30 s or more counts the next minute (e.g. 19:30 against a 20 min goal reads as reached).
4. Stats, Insights: "This week's practice" compares the same days of both weeks; on the first day of the week it appears only after the first session of that day.

Proposed commit message:
```
stats: Make the Stats numbers agree with the Log and the Timer

Minutes round to the nearest minute everywhere on Stats, as the Log
does. The week card leaves out today until today has practice, so it no
longer shows -100% every morning. The year chart shows twelve calendar
months from one core function both apps call. The best streak ignores
sessions dated after today, and its tile is captioned "Best streak".
Batch N: #55, #59, #60, #114, #115.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_012WGZUPYHL7TurMz6YSFVJs
```
