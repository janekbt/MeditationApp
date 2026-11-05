## ADDED Requirements

### Requirement: The session's media control shows the time and progress
During a session, the media control on the phone and on Linux SHALL show the time left, or the time so far when the session has no end, the state (preparation, paused, overtime), a progress bar for a session with an end and for its preparation, and for Guided the file's name as its title, but no breath phases.

#### Scenario: A timed session runs
- **WHEN** a 20 minute Timer has run 5 minutes 12 seconds
- **THEN** the line says "14:48 left" and the bar stands at 5:12 of 20:00 and moves on by itself
- **TEST** `meditate-core/src/session/mod.rs::the_media_control_counts_down_a_timed_session`

#### Scenario: A session that counts up but has an end
- **WHEN** a 20 minute Box Breath with 4 second phases, and a Guided file of 20 minutes set to count up, have run 5 minutes 12 seconds
- **THEN** both lines say "14:48 left", the bar stands at 5:12 of 20:00, and no breath phase shows
- **TEST** `meditate-core/src/session/mod.rs::the_media_control_shows_the_time_left_when_the_app_counts_up`

#### Scenario: A stopwatch without an end
- **WHEN** a Timer stopwatch, and a Box Breath without an end, have run 12 minutes 34 seconds
- **THEN** the line says "12:34" and there is no bar
- **TEST** `meditate-core/src/session/mod.rs::the_media_control_counts_up_without_an_end`

#### Scenario: Preparation before the session
- **WHEN** a session with 10 seconds of preparation is 5 seconds in, running and then paused
- **THEN** on Linux the line says "Preparation, 00:05 left", on the phone "Preparation", and the bar stands at 5 of 10 seconds, moving, then standing
- **TEST** `meditate-core/src/session/mod.rs::the_media_control_shows_the_preparation`

#### Scenario: A paused session
- **WHEN** a 20 minute Timer is paused at 5:12, and a stopwatch is paused at 12:34
- **THEN** the lines say "Paused, 14:48 left" with the bar standing at 5:12 of 20:00, and "Paused, 12:34" with no bar
- **TEST** `meditate-core/src/session/mod.rs::the_media_control_stands_still_while_paused`

#### Scenario: Overtime
- **WHEN** a 20 minute Timer is 2 minutes 13 seconds past its end
- **THEN** the line says "Overtime +02:13" and the bar stands full
- **TEST** `meditate-core/src/session/mod.rs::the_media_control_shows_overtime_with_a_full_bar`

#### Scenario: The phone's line counts whole minutes
- **WHEN** the phone shows 14:48 left, 12:34 so far, and 2:13 of overtime
- **THEN** the lines say "15 min left", "12 min" and "Overtime +2 min", so the line changes once a minute, and preparation says only "Preparation"
- **TEST** `meditate-android/src/app.rs::the_phone_line_counts_whole_minutes`

#### Scenario: The title names the guided file
- **WHEN** a Guided session plays "Morning breath", and later a Timer runs
- **THEN** the title is "Morning breath", then "Meditate"
- **TEST** `meditate-android/src/app.rs::the_media_control_title_names_the_guided_file`

#### Scenario: The phone shows it on the lock screen and in the shade
- **WHEN** the maintainer runs each state on the Fairphone in German, and in Russian at phone width
- **THEN** the line and the bar show as in the scenarios above, the line slides in anew at most once a minute, title and line are readable over a smaller, paler lotus in the middle, the bar moves without the app in front and stands while paused and in overtime, can't be dragged, and the buttons, accent and pinned place stay as before, also after 11 minutes paused and 11 minutes in overtime
- **MANUAL** on the FP5: the system draws the media control.

### Requirement: On Linux a session shows as a media player
On Linux, while a session runs, the app SHALL show as a media player in the system's media controls with the title and line above, pause and resume from its play/pause button and from the play/pause keys, except in overtime, offer no previous or next, and disappear when the session stops.

#### Scenario: Play/pause in each state
- **WHEN** the player's play/pause is used while running, while paused, while preparing, and in overtime
- **THEN** the session pauses, resumes, pauses, and in overtime nothing changes
- **TEST** `meditate-gtk/src/mpris.rs::play_pause_follows_the_session_buttons`

#### Scenario: What the player reports
- **WHEN** a 20 minute Timer has run 5:12, then is paused
- **THEN** the player reports the title "Meditate", the line "14:48 left", a length of 20:00 at 5:12, playing, then paused with "Paused, 14:48 left"; previous, next and seeking are off
- **AND** it shows the app icon as its cover; in overtime it reports paused, "Overtime +02:13", stays among the media players, and its play/pause does nothing; once stopped it reports nothing
- **TEST** `meditate-gtk/src/mpris.rs::the_player_reports_the_session`

#### Scenario: On GNOME and on the Librem 5
- **WHEN** the maintainer runs a session on the laptop (GNOME) and on the Librem 5 (Phosh), opens the shade and locks the screen, presses play/pause there and on the keyboard, then saves the session
- **THEN** the player shows the app icon as cover and a filled lotus as the small app symbol, title and line, updating every second, play/pause pauses and resumes, it stays through overtime, and it is gone once the session stops
- **MANUAL** the shell draws it; only a desktop and the phone show it.
