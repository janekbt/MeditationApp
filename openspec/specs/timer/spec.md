# Timer

## Purpose
The timer runs a meditation session: an optional preparation time,
then a countdown or a stopwatch, then overtime. It keeps true time
across a blank screen, a late tick and system suspend.

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
- **THEN** nothing changes: the overtime keeps counting, Add and Finish still work, and Finish saves the overtime
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

#### Scenario: Only the label for next time fails at Save
- **WHEN** a session is saved and only remembering its label as next time's default fails
- **THEN** the session is saved and "Label not kept for next time" shows, not "Save failed"
- **TEST** `meditate-gtk/src/timer/imp.rs::a_label_default_that_was_not_kept_says_so`

#### Scenario: Only the label for next time fails at Save on the phone
- **WHEN** on the phone a session is saved and only remembering its label as next time's default fails
- **THEN** the session is saved and "Label not kept for next time" shows
- **TEST** `meditate-android/src/app.rs::a_label_default_that_was_not_kept_says_so`

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
