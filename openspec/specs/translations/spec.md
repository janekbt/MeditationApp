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
