# Timer

## Purpose
The timer runs a meditation session: an optional preparation time,
then a countdown or a stopwatch, then overtime or, with Auto-Finish,
an automatic save. The running page itself saves or discards the
session. It keeps true time across a blank screen, a late tick and
system suspend.

## Requirements

### Requirement: The countdown after preparation keeps its real start
When preparation time ends, the countdown SHALL start at the moment preparation ended, even if the tick that notices it comes late.

#### Scenario: Suspend during preparation
- **WHEN** a 10-minute countdown with 30 s preparation is suspended 5 s in and woken 20 minutes later
- **THEN** the next tick ends the countdown and enters overtime, as it would without preparation
- **TEST** `meditate-core/src/session/mod.rs::a_late_tick_at_the_end_of_prep_keeps_the_countdown_on_time`

#### Scenario: A tick slightly late at the end of preparation
- **WHEN** the tick that ends a 30 s preparation arrives at 30.8 s
- **THEN** the countdown has already run 0.8 s
- **TEST** `meditate-core/src/session/mod.rs::a_slightly_late_prep_tick_loses_no_countdown_time`

### Requirement: A countdown skipped by a suspend rings only the end bell
When the device wakes from a suspend during preparation after the countdown would have ended, the session SHALL ring only the end bell, not the starting bell as well.

#### Scenario: Lid closed during preparation, opened after the end
- **WHEN** a 10-minute countdown with 30 s preparation is suspended 5 s in and woken 20 minutes later
- **THEN** no starting bell rings, and the end bell rings once
- **TEST** `meditate-core/src/session/mod.rs::a_suspend_past_the_end_during_prep_rings_only_the_end_bell`

#### Scenario: An interval bell due by then
- **WHEN** a 1-minute countdown with 30 s preparation and a 1-minute interval bell is suspended 5 s in and woken 3 minutes later
- **THEN** the end bell rings once and no interval bell rings with it
- **TEST** `meditate-core/src/session/mod.rs::a_suspend_past_the_end_during_prep_rings_no_interval_bell`

### Requirement: A running session keeps the device awake, a paused one doesn't
While a session runs on Linux, the app SHALL block automatic suspend, whether or not "Keep screen awake" is on; the screen itself stays on only with that setting. While a session is paused, on Linux and Android, the app SHALL NOT keep the device or its screen awake, and SHALL take that back on resume. If the system refuses the block, the app SHALL say so.

#### Scenario: Automatic suspend during a session
- **WHEN** a session runs and the computer's automatic-suspend time passes
- **THEN** the computer stays awake and the bells ring on time
- **TEST** `meditate-gtk/src/timer/mod.rs::a_running_session_blocks_suspend`

#### Scenario: A forgotten pause on Linux
- **WHEN** a session is paused and left
- **THEN** the computer can suspend and blank its screen as usual, and resuming blocks suspend again
- **TEST** `meditate-gtk/src/timer/mod.rs::a_paused_session_lets_the_computer_suspend`

#### Scenario: A forgotten pause on Android
- **WHEN** a session is paused and left
- **THEN** the phone releases its wake lock and the screen-on hold, and resuming takes them back
- **TEST** `meditate-android/src/app.rs::a_paused_session_lets_the_phone_sleep`

#### Scenario: The system refuses the block
- **WHEN** a session starts and GNOME or the portal refuses to block suspend
- **THEN** a message says "Couldn't keep the computer awake for the bells"
- **TEST** `meditate-gtk/src/timer/mod.rs::a_refused_suspend_block_tells_the_user`

### Requirement: Esc pauses only a session that can pause
On Linux, Esc on the running page SHALL pause the session only during preparation or the running countdown or stopwatch, and SHALL do nothing in overtime, while paused or after the session ended.

#### Scenario: Esc in overtime
- **WHEN** a countdown has passed its end and the user presses Esc
- **THEN** nothing changes: the overtime keeps counting, and Save, Add and Discard still work
- **TEST** `meditate-gtk/src/timer/mod.rs::escape_pauses_only_where_the_pause_button_would`

#### Scenario: Esc while paused
- **WHEN** a paused session's page is showing and the user presses Esc
- **THEN** the session stays paused and the button still reads "Resume"
- **TEST** `meditate-gtk/src/timer/mod.rs::escape_pauses_only_where_the_pause_button_would`

#### Scenario: Esc during a running countdown
- **WHEN** a countdown is running and the user presses Esc
- **THEN** the session pauses and the button reads "Resume", as before
- **MANUAL** press Esc during a running Timer session on the laptop; a keyboard event on a live window can't run in the test suite

### Requirement: The streak line follows saved sessions on the phone
After a session is saved, edited or brought back with Undo on the phone, the Timer page's streak line and Stats SHALL include it.

#### Scenario: First session of the day
- **WHEN** the streak line says "No streak" and the user saves today's first session
- **THEN** the streak line shows the new streak without opening Stats
- **TEST** `meditate-android/src/app.rs::saving_or_undoing_a_session_refreshes_the_streak`

#### Scenario: Recovery Undo
- **WHEN** a recovered session is undone
- **THEN** the streak line and Stats no longer count it
- **TEST** `meditate-android/src/app.rs::saving_or_undoing_a_session_refreshes_the_streak`

### Requirement: A setting that could not be saved springs back
When a Timer page setting fails to save, the app SHALL show "Save failed" and show the saved value again on that page.

#### Scenario: Laptop, a switch flipped while the database is locked
- **WHEN** on the laptop a Timer page switch, pick, length or volume is changed and saving it fails
- **THEN** "Save failed" shows and the page shows the value that is saved
- **TEST** `meditate-gtk/src/timer/imp.rs::a_failed_setting_says_so_and_springs_back`

#### Scenario: Laptop, no Timer page write drops its result
- **WHEN** any Timer page setting is written on the laptop
- **THEN** its result reaches the failure message, none is thrown away or only logged
- **TEST** `meditate-gtk/src/db/mod.rs::no_write_result_is_dropped`

#### Scenario: Phone, a setting changed while the database is locked
- **WHEN** on the phone a Timer page switch, pick, length, volume, a preset star or the accent colour is changed and saving it fails
- **THEN** "Save failed" shows and the page shows the value that is saved
- **TEST** `meditate-android/src/app.rs::a_failed_setting_says_so_and_springs_back`

#### Scenario: Laptop, a preset that fails halfway is put back
- **WHEN** applying a preset, or its Undo, fails after some of its values were saved
- **THEN** "Save failed" shows, no Undo, and the setup from before is saved again
- **TEST** `meditate-gtk/src/timer/imp.rs::a_preset_that_fails_halfway_is_put_back`

#### Scenario: Phone, a preset that fails halfway is put back
- **WHEN** on the phone applying a preset, or its Undo, fails after some of its values were saved
- **THEN** "Save failed" shows, no Undo, and the setup from before is saved again
- **TEST** `meditate-android/src/app.rs::a_preset_that_fails_halfway_is_put_back`

#### Scenario: A real failure on each device
- **WHEN** a sync lock is held on the database (BEGIN EXCLUSIVE from sqlite3) and a bell switch is flipped
- **THEN** after about 8 s "Save failed" shows and the switch flips back
- **MANUAL** needs a locked database on a real install; neither app's UI runs on the test host

### Requirement: Row texts never push a control off screen
On the phone, a row's title and subtitle SHALL wrap so the switch or button beside them stays on screen at 360 dp in every language.

#### Scenario: Keep Screen Awake in German at 360 dp
- **WHEN** Setup is shown in German, French or Russian on a 360 dp wide screen
- **THEN** the Keep Screen Awake and Cues during phases switches are fully visible and their texts show in full
- **MANUAL** layout on a phone

#### Scenario: A preset with a long subtitle
- **WHEN** a preset whose subtitle is longer than one line is listed in the preset chooser
- **THEN** its delete button is fully visible and the subtitle shows in full
- **MANUAL** layout on a phone

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

### Requirement: Auto-Finish saves a countdown when the time is up
When Auto-Finish is on for a mode, a countdown in that mode SHALL, at its end, ring the end bell, save the session with its planned length and the Setup label, return to Setup and show "Session saved" with Undo.

#### Scenario: A Timer countdown ends by itself
- **WHEN** Auto-Finish is on and a 10-minute Timer countdown reaches 0
- **THEN** the end bell rings and a 10-minute session is saved with any note typed during a pause, with no overtime
- **TEST** `meditate-core/src/session/mod.rs::a_countdown_with_auto_finish_ends_at_its_length`

#### Scenario: A guided file ends by itself
- **WHEN** Auto-Finish is on for Guided and the file plays to its end, or its length is reached first
- **THEN** a session of the file's stored length is saved, with no overtime
- **TEST** `meditate-core/src/session/mod.rs::a_guided_file_with_auto_finish_ends_at_its_length`

#### Scenario: A Box Breath countdown ends by itself
- **WHEN** Auto-Finish is on for Box Breath and its countdown ends
- **THEN** the session is saved at once, without the Save and Discard buttons
- **TEST** `meditate-core/src/session/mod.rs::a_box_breath_countdown_with_auto_finish_saves_by_itself`

#### Scenario: Off, nothing changes
- **WHEN** Auto-Finish is off and a countdown reaches 0
- **THEN** it goes into overtime as before
- **TEST** `meditate-core/src/session/mod.rs::tick_running_countdown_at_target_enters_overtime`

#### Scenario: Remembered per mode, only for countdowns
- **WHEN** Auto-Finish is turned on for Timer
- **THEN** Guided and Box Breath keep their own value, the switch survives a restart, and it is greyed out (not hidden) while the mode is set to stopwatch
- **TEST** `meditate-core/src/settings_keys.rs::auto_finish_is_remembered_per_mode`

#### Scenario: Undo on the laptop
- **WHEN** a session was auto-saved on the laptop and the user taps Undo on "Session saved", with the Log already open
- **THEN** the session is gone from the open Log at once, and from Stats, like the recovery Undo
- **TEST** `meditate-gtk/src/timer/imp.rs::an_auto_finished_session_is_saved_with_undo`

#### Scenario: Undo on the phone
- **WHEN** a session was auto-saved on the phone and the user taps Undo on "Session saved"
- **THEN** the session is deleted from the Log, Stats and the streak line
- **TEST** `meditate-android/src/app.rs::an_auto_finished_session_is_saved_with_undo`

#### Scenario: The automatic save lets the end bell ring on the laptop
- **WHEN** Auto-Finish saves a session on the laptop
- **THEN** the end bell and its vibration play to their end; only a Save the user taps silences what rings
- **TEST** `meditate-gtk/src/timer/imp.rs::an_auto_save_lets_the_end_bell_ring`

#### Scenario: The automatic save lets the end bell ring on the phone
- **WHEN** Auto-Finish saves a session on the phone
- **THEN** the end bell and its vibration play to their end
- **TEST** `meditate-android/src/app.rs::an_auto_save_lets_the_end_bell_ring`

#### Scenario: The time runs out while Discard asks, on the laptop
- **WHEN** "Discard Session?" is open and Auto-Finish ends the session
- **THEN** the question closes and "Session saved" with Undo shows, so Undo removes it
- **TEST** `meditate-gtk/src/timer/imp.rs::a_discard_question_closes_when_the_session_saves_itself`

#### Scenario: The time runs out while Discard asks, on the phone
- **WHEN** the discard question is open and Auto-Finish ends the session
- **THEN** the question closes and "Session saved" with Undo shows
- **TEST** `meditate-android/src/app.rs::a_discard_question_closes_when_the_session_saves_itself`

#### Scenario: Saved while the laptop window is not focused
- **WHEN** Auto-Finish saves while the window is not focused
- **THEN** no "Session saved" notice is left waiting; the session is in the Log, where it can be deleted
- **TEST** `meditate-gtk/src/timer/imp.rs::an_auto_finished_session_is_saved_with_undo`

#### Scenario: Saved while the phone app is in the background
- **WHEN** Auto-Finish saves while the app is in the background
- **THEN** no "Session saved" notice is left waiting; the session is in the Log, where it can be deleted
- **TEST** `meditate-android/src/app.rs::an_auto_finished_session_is_saved_with_undo`

#### Scenario: An auto-save that fails
- **WHEN** the automatic save fails
- **THEN** the failure is shown and the running page stays with [Save][Discard] and the note, so nothing is lost
- **TEST** `meditate-android/src/app.rs::an_auto_finished_session_is_saved_with_undo`

#### Scenario: Auto-Finish with the phone in the pocket
- **WHEN** Auto-Finish is on, the phone screen is off and a countdown ends
- **THEN** the end bell rings, and on unlocking, Setup shows and the session is in the Log
- **MANUAL** the background service and screen-off timing need the phone

### Requirement: A running session survives a display change
On the phone, a change of font size, bold text, screen size or keyboard during a session SHALL NOT end the session, and an app start SHALL stop any notification, track or bell left over from a session that is gone.

#### Scenario: Changing the font size during a session
- **WHEN** the user changes the system font size while a session runs
- **THEN** the session keeps running with its time
- **TEST** `meditate-android/src/app.rs::configuration_changes_keep_the_session`

#### Scenario: The screen restarts with a session's service still running
- **WHEN** the app's screen starts while a foreground service, guided track or bell from an earlier screen is still running, as after a language change mid-session
- **THEN** they are stopped, and no notification stays
- **TEST** `meditate-android/src/app.rs::startup_stops_what_an_old_screen_left_running`
