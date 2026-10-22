# Translations

## Purpose
Both apps show their text in the system language when a translation
for it is bundled, and in English otherwise.

## Requirements

### Requirement: A language with a script picks its regional translation
On the phone, a system language that names a script and a region SHALL pick the translation for that language and region.

#### Scenario: The phone is set to Simplified Chinese
- **WHEN** Android reports the language as "zh-Hans-CN"
- **THEN** the app shows the Simplified Chinese (zh_CN) translation
- **TEST** `meditate-android/src/app.rs::a_language_with_a_script_picks_its_region`

#### Scenario: A language without a script
- **WHEN** Android reports "pt-BR" or "de"
- **THEN** the app picks pt_BR then pt, or de
- **TEST** `meditate-android/src/app.rs::a_language_without_a_script_picks_as_before`

### Requirement: Dates follow each language's own order
Every date both apps show SHALL follow the word order and names of the system's date format setting (Formats on Linux, the phone's language on Android), with the day unpadded.

#### Scenario: Every Linux translation writes its dates in its own order
- **WHEN** the short date, the short date with year and the spoken heatmap date are translated into each of the nine languages
- **THEN** each translation is a valid date pattern, has a year exactly when the English one has one, uses no padded day, and puts the day before the month in every language but Chinese, which writes year, month, day
- **TEST** `meditate-gtk/src/i18n.rs::every_language_writes_dates_in_its_own_order`

#### Scenario: The edit dialog's date has no padded day
- **WHEN** the Linux edit dialog shows a session from 9 October 2025 in English
- **THEN** its date reads "Oct 9, 2025", not "Oct 09, 2025"
- **TEST** `meditate-gtk/src/format.rs::a_short_date_has_no_padded_day`

#### Scenario: The heatmap speaks a date without a double space
- **WHEN** a screen reader reads the heatmap cell for 9 October 2025 in English
- **THEN** the date part reads "Thursday, October 9"
- **TEST** `meditate-gtk/src/format.rs::the_spoken_heatmap_date_has_no_padded_day`

#### Scenario: Android Log headers use the phone's date format
- **WHEN** the Android Log shows a day that is neither today nor yesterday
- **THEN** its header is the phone's own short date with the month name, with the year only for an earlier year, and no number pattern is left to translate
- **TEST** `meditate-android/src/app.rs::log_sections_render_through_core`

#### Scenario: English desktop with German formats
- **WHEN** Linux runs with English as its language and German as its Formats, and the Log shows 9 October 2025
- **THEN** the header reads "9. Okt 2025" while the rest of the app stays English
- **MANUAL** needs a desktop with mixed locale settings and the installed catalogs; also with LANGUAGE set to "en_US:en", as Ubuntu does

#### Scenario: Seen in German and en-GB
- **WHEN** the maintainer opens the Log in German on Linux and on the phone, and in English (UK) on the phone
- **THEN** Linux shows "9. Okt" (earlier years "9. Okt 2025"), the phone "9. Okt." (its own abbreviation), en-GB "9 Oct"
- **MANUAL** the phone's date format and the desktop locale are only there on a device

### Requirement: Chart labels stay distinct in every language
Each month and weekday label of the Stats charts SHALL name its month or weekday unambiguously in every language.

#### Scenario: Chinese months October to December
- **WHEN** the month label is taken from "10月", "11月", "12月", "Янв" and "Jan"
- **THEN** it is "10", "11", "12", "Я" and "J"
- **TEST** `meditate-core/src/date_math.rs::a_month_label_keeps_a_leading_number`

#### Scenario: Weekdays keep their whole short name
- **WHEN** the Linux week chart labels Monday 19 October 2026 in English
- **THEN** the label is "Mon", the locale's whole short weekday, so Portuguese "seg", "ter", "qua", "qui" stay distinct
- **TEST** `meditate-gtk/src/stats/imp.rs::a_weekday_label_is_the_whole_short_name`

#### Scenario: Seen in Chinese and Portuguese
- **WHEN** the maintainer opens the year chart in Chinese and the week chart in Portuguese, in both apps
- **THEN** the year chart has no repeated label and the week chart has seven different weekdays
- **MANUAL** the labels come from each platform's locale data
