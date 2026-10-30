## MODIFIED Requirements
### Requirement: The running page ends every session
The running page SHALL offer the end of a session itself, with no second screen, with Discard always on its own line under the main buttons: Pause while running; Resume and Save with the note when paused; Resume when paused during preparation; Save and Add with the note when the time ran out; Save with the note when a session ended unsaved.

#### Scenario: The buttons follow the session
- **WHEN** a session is preparing or running, paused, paused during preparation, in overtime, or has ended (Box Breath countdown, or a save that failed)
- **THEN** the row shows [Pause]; [Resume][Save] + note; [Resume]; [Save][Add] + note; [Save] + note, in that order, and Discard sits under the row in every one of them
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
