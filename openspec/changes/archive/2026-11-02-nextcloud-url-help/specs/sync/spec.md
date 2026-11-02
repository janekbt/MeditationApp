## ADDED Requirements

### Requirement: The sync settings say which address and password to enter
Both apps SHALL label the address field "WebDAV URL" and say where Nextcloud shows that address and where to create an app password.

#### Scenario: A Linux user opens the Nextcloud settings
- **WHEN** Preferences is opened on Linux in German
- **THEN** the Nextcloud Sync group names the WebDAV address under Files settings and the app password under Personal settings, Security, in German, and the first field reads "WebDAV-URL"
- **MANUAL** Janek on the laptop: the preferences dialog only renders in a running app

#### Scenario: An Android user opens the Nextcloud settings on a narrow phone
- **WHEN** Preferences is opened on the phone in German
- **THEN** the same sentence and label show in German, wrapped in full with nothing cut off and no box characters
- **MANUAL** Janek on the FP5: layout at phone width only shows on the device

#### Scenario: The new texts in every Linux language
- **WHEN** a Linux catalog lacks the label or the sentence
- **THEN** the check names the language and the text
- **TEST** `meditate-gtk/src/i18n.rs::every_language_translates_every_text`

#### Scenario: The new texts in every Android language
- **WHEN** an Android catalog lacks the label or the sentence, or leaves one empty
- **THEN** the check names the language and the text
- **TEST** `meditate-android/src/app.rs::nextcloud_help_is_translated`
