# Setup

## Purpose
Setup is the screen a session starts from: the time, the mode, the
presets, and the pages for session and bell settings.

## Requirements

### Requirement: Session and Bells settings live on their own pages
The main screen SHALL show a Session row and a Bells row whose subtitles sum up the settings, each opening its own page.

#### Scenario: Session summary with a label, Stopwatch and Auto-Finish on
- **WHEN** Stopwatch, the label "Walking" and Auto-Finish are on in Timer mode, and then Stopwatch goes off
- **THEN** the Session row reads "Stopwatch · Walking" (Auto-Finish does nothing with Stopwatch), then "Walking · Auto-Finish"
- **TEST** `meditate-core/src/preset_config.rs::session_summary_names_stopwatch_and_label`

#### Scenario: Session summary with nothing set
- **WHEN** Stopwatch and Auto-Finish are off and no label is on
- **THEN** the Session row has no subtitle
- **TEST** `meditate-core/src/preset_config.rs::session_summary_is_empty_when_plain`

#### Scenario: Bells summary per mode
- **WHEN** starting bell, two of three interval bells and the end bell are on, and Box Breath cues are on
- **THEN** Timer reads "Starting Bell · 2 interval bells · End Bell", Box Breath "End Bell · Cues during phases", Guided "End Bell"
- **TEST** `meditate-core/src/preset_config.rs::bells_summary_lists_what_rings_per_mode`

#### Scenario: Bells summary with Stopwatch on
- **WHEN** Stopwatch is on in Timer mode with starting, interval and end bell switched on
- **THEN** the Bells row names only what will ring: no end bell, and no interval bells timed from the end
- **TEST** `meditate-core/src/preset_config.rs::bells_summary_follows_what_rings_with_stopwatch`

#### Scenario: No bells at all
- **WHEN** every bell of the mode is off
- **THEN** the Bells row reads "No bells"
- **TEST** `meditate-core/src/preset_config.rs::bells_summary_says_no_bells`

#### Scenario: The summary follows a change on the page
- **WHEN** the end bell is switched off on the Bells page and the user goes back
- **THEN** the Bells row no longer names it
- **MANUAL** Janek on laptop and FP5: needs the running pages

#### Scenario: The new texts in every language
- **WHEN** a catalog lacks "No bells" or the interval bell count
- **THEN** the check names the language and the text
- **TEST** `meditate-android/src/app.rs::setup_summaries_are_translated`

### Requirement: A session starts from every settings page
Session, Bells and Manage Bells SHALL show a start bar with the session's time and Start Session.

#### Scenario: Starting from the Bells page
- **WHEN** Start Session is tapped in the Bells page's start bar
- **THEN** the running screen slides in over the page, and Stop returns to the main screen, not to Bells
- **MANUAL** Janek on laptop and FP5: page transitions only show in the running app

#### Scenario: The start bar at phone width
- **WHEN** the Session page is open on the FP5 in German and Russian
- **THEN** the time, "Stunden · Minuten" and "Sitzung starten" show in full, nothing cut off
- **MANUAL** Janek on the FP5: layout at 360 px

### Requirement: Tapping the time sets the duration
Tapping the time on the main screen or in the start bar SHALL open the Duration dialog in Timer and Box Breath unless Stopwatch is on.

#### Scenario: Tap the time in Timer mode
- **WHEN** the time is tapped on the main screen, and again in a start bar
- **THEN** the Duration dialog opens each time; in Guided or with Stopwatch on, nothing opens
- **MANUAL** Janek on laptop and FP5: a tap on a running page
