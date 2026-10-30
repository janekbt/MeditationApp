## RENAMED Requirements
- FROM: `### Requirement: Repeating bells are counted under the time`
- TO: `### Requirement: The bells are counted under the time`

## MODIFIED Requirements
### Requirement: The running page ends every session
The running page SHALL offer the end of a session itself, with no second screen, with Discard always on its own line under the main buttons: Pause and Save while running; Pause during preparation; Resume and Save with the note when paused; Resume when paused during preparation; Save and Add with the note when the time ran out; Save with the note when a session ended unsaved.

#### Scenario: The buttons follow the session
- **WHEN** a session is preparing, running, paused, paused during preparation, in overtime, or has ended (Box Breath countdown, or a save that failed)
- **THEN** the row shows [Pause]; [Pause][Save]; [Resume][Save] + note; [Resume]; [Save][Add] + note; [Save] + note, in that order, and Discard sits under the row in every one of them
- **TEST** `meditate-core/src/session/mod.rs::the_end_buttons_follow_the_session`

#### Scenario: Nothing moves when the session pauses, on the laptop
- **WHEN** a running session is paused on the laptop and the note appears, in a tall or a short window
- **THEN** the time, the row and Discard stay where they were, the note comes in below them, and a short window scrolls to it
- **TEST** `meditate-gtk/src/window/imp.rs::the_session_buttons_keep_their_place`

#### Scenario: Nothing moves when the session pauses, on the phone
- **WHEN** a running session is paused on the phone and the note appears
- **THEN** the time, the row and Discard stay where they were, and the note comes in below them
- **TEST** `meditate-android/src/app.rs::the_session_buttons_keep_their_place`

#### Scenario: Save while paused keeps the time so far
- **WHEN** a 20-minute countdown is paused after 7 minutes and the user taps Save
- **THEN** a 7-minute session is saved with the note and the Setup label, and Setup shows again
- **TEST** `meditate-core/src/session/mod.rs::save_while_paused_keeps_the_time_so_far`

#### Scenario: Save joins Pause when preparation ends, on the laptop
- **WHEN** a Timer with 10 seconds of preparation runs past its preparation on the laptop
- **THEN** the row changes from [Pause] to [Pause][Save] without a pause in between
- **TEST** `meditate-gtk/src/timer/mod.rs::save_joins_pause_when_preparation_ends`

#### Scenario: Save while running keeps the time so far
- **WHEN** a 20-minute countdown has run 8 minutes with a 1-minute pause and the user taps Save without pausing
- **THEN** a 7-minute session is saved with the note written in an earlier pause, if any, and Setup shows again
- **TEST** `meditate-android/src/app.rs::save_while_running_keeps_the_time_so_far`

#### Scenario: Save after the time ran out keeps the planned length
- **WHEN** a 20-minute countdown has run 3 minutes into overtime and the user taps Save
- **THEN** a 20-minute session is saved with the note
- **TEST** `meditate-core/src/session/mod.rs::finish_overtime_returns_target_secs`

#### Scenario: Add after the time ran out keeps the overtime
- **WHEN** a 20-minute countdown has run 3 minutes into overtime and the user taps Add
- **THEN** a 23-minute session is saved with the note
- **TEST** `meditate-core/src/session/mod.rs::add_overtime_and_finish_returns_running_plus_overtime`

#### Scenario: The Add button says that it saves, on the laptop
- **WHEN** a countdown is 3 minutes into overtime on the laptop
- **THEN** the button reads "+ 03:00 and Save", on two lines where one doesn't fit
- **TEST** `meditate-gtk/src/timer/imp.rs::the_add_button_says_that_it_saves`

#### Scenario: The Add button says that it saves, on the phone
- **WHEN** a countdown is 3 minutes into overtime on the phone
- **THEN** the button reads "+ 03:00 and Save", on two lines where one doesn't fit, never cut off
- **TEST** `meditate-android/src/app.rs::the_add_button_says_that_it_saves`

#### Scenario: Discard ends without saving
- **WHEN** a running or paused session with no note is discarded
- **THEN** nothing is saved, the recovery snapshot is cleared and Setup shows again
- **TEST** `meditate-android/src/app.rs::discard_returns_to_idle_without_saving`

#### Scenario: Discard with a note asks first
- **WHEN** a note was typed during a pause and the user taps Discard, while paused or after resuming
- **THEN** "Discard Session?" asks first; Cancel leaves the session as it was, still running if it was running and a ringing bell still ringing
- **TEST** `meditate-android/src/app.rs::discarding_a_note_and_deleting_a_bell_ask_first`

#### Scenario: Linux has no Done page
- **WHEN** a session ends on the laptop by any button
- **THEN** it is saved or discarded from the running page; no Done page, no label pick
- **TEST** `meditate-gtk/src/timer/mod.rs::the_running_page_is_the_only_end_screen`

#### Scenario: The phone has no Done page
- **WHEN** a session ends on the phone by any button
- **THEN** it is saved or discarded from the running page; no Done page, no label pick
- **TEST** `meditate-android/src/app.rs::the_running_page_is_the_only_end_screen`

#### Scenario: A failed save keeps the running page
- **WHEN** Save fails, on either device
- **THEN** the failure is shown and the running page stays with its note, so Save can be tapped again
- **TEST** `meditate-android/src/app.rs::a_failed_save_keeps_the_session_and_says_so`

#### Scenario: Space never saves on Linux
- **WHEN** Space is pressed in overtime or after a session ended
- **THEN** nothing happens; Space only pauses and resumes
- **TEST** `meditate-gtk/src/timer/mod.rs::space_never_ends_a_session`

#### Scenario: A failed save keeps the running page on Linux
- **WHEN** Save fails on the laptop
- **THEN** the failure toast shows and the running page stays with its note
- **TEST** `meditate-gtk/src/timer/mod.rs::only_a_successful_save_leaves_the_running_page`

#### Scenario: How it looks
- **WHEN** a session is paused, then resumed, then runs into overtime, on the phone at 360 dp and on the laptop, in German, in Timer and Box Breath
- **THEN** Discard never changes its place, the time and the buttons don't jump when the note appears or goes, "+ 03:00 und speichern" fits, nothing is cut off, and the note keeps its text across Resume and is hidden while running
- **MANUAL** layout and the soft keyboard over the note need a real screen

### Requirement: The phone's session notification controls the session
On the phone, the running session's media control SHALL offer the session page's buttons plus Discard, act like them, follow the headphones' play/pause button, and show the app's accent colour with the white lotus.

#### Scenario: The buttons follow the session page in every state
- **WHEN** a session is running, preparing, paused, paused in preparation, or in overtime
- **THEN** the media control offers Pause, Save and Discard; Pause and Discard; Resume, Save and Discard; Resume and Discard; Save, Add and Discard
- **TEST** `meditate-android/src/app.rs::the_notification_offers_the_session_buttons`

#### Scenario: A button in the notification does what the app's button does
- **WHEN** the user taps Pause, Resume, Save or Add in the media control
- **THEN** the session behaves as after the same tap in the app: a guided track pauses and resumes with it, a paused phone may sleep, Save and Add save with the note
- **TEST** `meditate-android/src/app.rs::a_notification_tap_goes_through_the_session_buttons`

#### Scenario: Discard from the notification with and without a note
- **WHEN** the user taps Discard in the media control, once with an empty note and once after writing a note
- **THEN** without a note the session is discarded at once; once a note is written, Discard is no longer offered there
- **TEST** `meditate-android/src/app.rs::discard_from_the_notification_spares_a_note`

#### Scenario: The headphones' button pauses and resumes, but not in overtime
- **WHEN** the user presses play/pause on the headphones while running, while paused, and in overtime
- **THEN** the session pauses, resumes, and in overtime nothing changes
- **TEST** `meditate-android/src/app.rs::the_headphone_button_toggles_the_pause`

#### Scenario: The media control shows the accent colour and the lotus
- **WHEN** a session runs with the accent set to System, then to Rose, on the lock screen and in the notification shade
- **THEN** the media control's background follows the accent, the white lotus shows on it sharply with the accent showing between its petals and as the small symbol, and the play or pause symbol matches the session
- **MANUAL** on the FP5: the system draws the media control, so only a phone shows it.

### Requirement: Each bell is a bead on the ring
The ring SHALL show each bell of the session as a bead at its time, small for a repeating bell and larger for a one-off bell, filled in the accent once it rang and hollow before, outlined in the faint colour of the ring's track.

#### Scenario: A one-off bell before and after it rings
- **WHEN** a 30 minute Timer has a bell 3 minutes from the start and 2 minutes before the end, and 10 minutes have passed
- **THEN** there is a large filled bead at 3 minutes and a large hollow bead at 28 minutes
- **TEST** `meditate-core/src/session/mod.rs::one_off_bells_are_large_beads`

#### Scenario: A one-off bell slept through stays hollow
- **WHEN** a 30 minute Timer has a bell 2 minutes before the end and the device sleeps from 27:00 to 31:00
- **THEN** in overtime its bead is still hollow, because it never rang
- **TEST** `meditate-core/src/session/mod.rs::a_one_off_bell_slept_through_stays_hollow`

#### Scenario: A repeating bell lists every ring that fits
- **WHEN** a 30 minute Timer has a bell every 5 minutes without spread, and 12 minutes have passed
- **THEN** small beads sit at 5, 10, 15, 20 and 25 minutes, the first two filled
- **AND** the counter reads "2 of 5" and, once the last has rung, "5 of 5": no bead and no count for 30:00, where it would not ring
- **TEST** `meditate-core/src/session/mod.rs::repeating_bells_are_small_beads`

#### Scenario: A bell with growing gaps
- **WHEN** a 30 minute Timer has a bell every 5 minutes growing by 1 minute
- **THEN** beads sit at 5, 11, 18 and 26 minutes
- **TEST** `meditate-core/src/session/mod.rs::repeating_bells_are_small_beads`

#### Scenario: A bell skipped by a suspend
- **WHEN** a 30 minute Timer has a bell every 5 minutes without spread and the device sleeps from 4:00 to 12:00
- **THEN** the bell rings once at 12:00: its 10 minute bead rides to 12:00 and is filled, its 5 minute bead stays hollow because it never rang, and the counter reads "1 of 5"
- **TEST** `meditate-core/src/session/mod.rs::repeating_bells_are_small_beads`

#### Scenario: Beads before they ring
- **WHEN** a Timer with a bell every 5 minutes runs, with the accent Rose on the phone and the laptop's accent set to purple
- **THEN** the beads that have not rung yet and the start point before the session starts are outlined in the faint accent of the ring's track, not grey or white
- **MANUAL** on the FP5 and the laptop: colours on the running page need a person to see them.

### Requirement: The bells are counted under the time
The running page SHALL show one counter under the time for all the session's bells, the starting and end bells included, as "rung of total" in a countdown and as the plain number in the Timer stopwatch, and none when no bell rings between the starting and the end bell.

#### Scenario: Every bell of a countdown, starting and end bell included
- **WHEN** a 30 minute Timer has a starting bell, a bell 3 minutes in, a bell every 5 minutes and an end bell
- **THEN** the counter reads "1 of 8" once the starting bell rang, "0 of 8" before it during preparation, "4 of 8" at 12 minutes, and "8" once the end bell rang
- **TEST** `meditate-core/src/session/mod.rs::the_counter_counts_every_bell`

#### Scenario: Two repeating bells share the counter
- **WHEN** a 30 minute Timer has a bell every 7 minutes and one every 5 minutes, and 12 minutes have passed
- **THEN** the counter reads "3 of 9"
- **TEST** `meditate-core/src/session/mod.rs::the_counter_counts_every_bell`

#### Scenario: Only a starting and an end bell
- **WHEN** a Timer has a starting and an end bell and no bell between them
- **THEN** no counter shows
- **TEST** `meditate-core/src/session/mod.rs::the_counter_counts_every_bell`

#### Scenario: One-off bells count too
- **WHEN** a 30 minute Timer has bells 3 minutes in and 2 minutes before the end, and 10 minutes have passed
- **THEN** the counter reads "1 of 2"
- **TEST** `meditate-core/src/session/mod.rs::one_off_bells_are_large_beads`

#### Scenario: Stopwatch counts without a total
- **WHEN** a Timer stopwatch with a starting bell and a bell every 5 minutes has run 12 minutes
- **THEN** the counter reads "3" and there is no ring
- **TEST** `meditate-core/src/session/mod.rs::the_stopwatch_counts_without_a_total`

#### Scenario: The counter text in every language
- **WHEN** the app runs in any of its nine languages
- **THEN** the counter reads "2 of 5" in that language, in both apps
- **TEST** `meditate-android/src/app.rs::ui_text_is_translated_whole`

### Requirement: Overtime shows a full ring
Once a countdown passes its end, the ring SHALL be full with every rung bead filled, and the counter SHALL show only its number, still counting the bells that ring in overtime.

#### Scenario: A bell rings on in overtime
- **WHEN** a 30 minute Timer with a bell every 5 minutes give or take 1 has rung six times and passes the end
- **THEN** the ring is full and the counter reads "6"
- **AND** when it rings again in overtime the counter reads "7"
- **TEST** `meditate-core/src/session/mod.rs::overtime_shows_a_full_ring`
