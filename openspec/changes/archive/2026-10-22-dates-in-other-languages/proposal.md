# Dates in other languages

## Why
Dates come out in English word order or with broken short forms in other languages (OPEN-ISSUES batch O: #151, #152, #153, #154, #116). German Linux Log headers read "Okt 9, 2025", an en-GB phone shows 9 October as "10/9", Chinese charts label October to December "1", Portuguese weekday labels repeat ("se te qu qu se"), and the lifetime longest session gives no year.

## What Changes
- Linux: the short date ("Oct 9"), the short date with year ("Oct 9, 2025") and the heatmap's spoken date ("Thursday, October 9") become translatable patterns, all nine languages. They are used by the Log headers, the edit dialog's date, the chart's day labels, the longest-session insight and the heatmap cells. The edit dialog's date and the heatmap's spoken date lose their padded day ("Oct 09", "October  9").
- Android: Log day headers use the phone's own date format with the month name ("9. Okt.", earlier years "9. Okt. 2025"), like Linux; the two number patterns and their translations go.
- Both apps: a month label in the year chart is the month's leading number when its name starts with a digit (Chinese "10"), else its first letter.
- Linux: weekday chart labels are the locale's whole short weekday, not two characters (as Android already does).
- Both apps: the longest-session date carries its year when the session is from an earlier year.
- Left out: the heatmap's month-range caption ("Oct - Oct") is a single %b each, already locale-correct.

## Decisions
- Android Log headers show the month name, like Linux (maintainer, 2026-10-22).
- On Linux the date follows the Formats setting, not the UI language, since both are set separately (maintainer, 2026-10-22, after review finding 1). The patterns are read from the catalog of the LC_TIME language directly (glibc's own lookup lets LANGUAGE win, which Ubuntu sets).
- The year/no-year rule for the longest session is core's Log-header rule: with year only for an earlier calendar year (my call, as the issue says).
- Android's month label also goes through core's month-initial rule on the narrow month name, instead of only dropping `take(1)`, so both apps share one rule (my call).
- The Linux patterns live in one GTK helper, `format::short_date(dt, with_year)`, plus the heatmap's own pattern (my call).

## Capabilities
### Modified Capabilities
- `translations`: dates follow each language's order; chart labels stay distinct
- `stats`: the longest-session date names its year

## Impact
- Core: `format.rs` (year rule), `date_math.rs` (month initial)
- GTK: `src/format.rs`, `src/log/imp.rs`, `src/stats/imp.rs`, `src/i18n.rs` (tests), `po/` (template + nine .po), `po/meson.build` (xgettext keyword `date_pattern`)
- Android: `src/ui.rs`, `src/app.rs` (DayHeader), `ui/main.slint` (two Tr functions removed), `lang/*/meditate-android.po` (two entries removed)
- Translations: GTK three new msgids x nine languages; Android two removed
- Data: none
- Android build environment: untouched
- Docs: OPEN-ISSUES.md (batch O)
