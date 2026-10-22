## ADDED Requirements

### Requirement: Every Linux text is translated in every language
The Linux app SHALL pass every user-visible text through its catalog, and each of the nine catalogs SHALL translate every text.

#### Scenario: A text is added without its nine translations
- **WHEN** a translatable text is in the template and one language has no translation for it
- **THEN** the check names the language and the text
- **TEST** `meditate-gtk/src/i18n.rs::every_language_translates_every_text`

#### Scenario: Texts that showed English in every language
- **WHEN** the code builds the completion notification, the "Meditating" title, the CSV import filter, the vibration editor's points sentence, a seconds value or the best streak
- **THEN** it builds them from catalog texts, not from English literals
- **TEST** `meditate-gtk/src/i18n.rs::visible_text_goes_through_translation`

#### Scenario: Seen in German and Russian
- **WHEN** the Linux app runs in German, a timer session ends while the window is in the background, then in Russian the Stats tab is opened with a label of 3 sessions
- **THEN** German shows "Meditation läuft" as the running title, a German notification, "4 s" on the Box Breath tiles, the editor sentence wholly German and "5 T." as streak; Russian shows "3 сессии" in the By-label row and "4 с" on the tiles
- **MANUAL** the notification, titles and rows only render in a running session

### Requirement: No text uses the long dash
The app's texts and their translations SHALL use a comma or colon instead of the long dash, except the published release notes.

#### Scenario: A Linux text or translation with the long dash
- **WHEN** a template text, or any of the nine translations of a text outside the release notes, contains "—"
- **THEN** the check names it
- **TEST** `meditate-gtk/src/i18n.rs::no_text_uses_the_long_dash`

### Requirement: The best streak is written in each language's short form
Both apps SHALL show the best streak as a number with that language's day abbreviation, chosen by its plural rule.

#### Scenario: Android shows the streak through its catalog
- **WHEN** Android fills the best streak tile
- **THEN** it uses the translated plural "{n}d", not a fixed "d"
- **TEST** `meditate-android/src/app.rs::visible_text_goes_through_translation`

#### Scenario: Seen on the phone in German
- **WHEN** the phone is in German and Stats is opened with a best streak of 5
- **THEN** the tile shows "5 T." and fits at phone width
- **MANUAL** the tile only renders on a device
