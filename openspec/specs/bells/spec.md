# Bells

## Purpose
Bells mark a session: a starting bell, interval bells during it and
an end bell. Each can play a sound, a vibration or both, at the time
the user set.

## Requirements

### Requirement: An interval bell rings at most once per tick
After a time jump, an interval bell SHALL ring once and continue from its next due time after now, not ring for every interval it missed.

#### Scenario: Suspend across several intervals
- **WHEN** a session with a bell every 5 minutes is suspended at 4:59 and woken at 25:00
- **THEN** the bell rings once, and next at 30:00
- **TEST** `meditate-core/src/bells.rs::an_interval_bell_rings_once_after_a_time_jump`

### Requirement: The end bell is not followed by an interval bell
An interval bell due at or before the moment the countdown ends SHALL NOT ring after the end bell.

#### Scenario: Interval bell due exactly at the end
- **WHEN** a 10-minute countdown has a bell every 5 minutes
- **THEN** at 10:00 only the end bell rings, and no interval bell rings at 10:01
- **TEST** `meditate-core/src/session/mod.rs::an_interval_bell_due_at_the_end_gives_way_to_the_end_bell`

### Requirement: A volume change is saved for the bell and mode it was made in
On Linux, a bell volume set on a slider SHALL be saved for the bell and the mode that were showing when the slider moved, even when the page closes or the mode changes before the preview plays.

#### Scenario: The interval bell page closes right after the change
- **WHEN** the user drags an interval bell's volume to 20% and goes back within 0.4 s
- **THEN** the bell keeps 20%; only the preview doesn't play
- **TEST** `meditate-gtk/src/volume_row.rs::a_volume_is_saved_even_when_the_page_has_closed`

#### Scenario: The mode changes right after an End Bell change
- **WHEN** the user drags Timer's End Bell volume to 20% and switches to Guided within 0.4 s
- **THEN** Timer's End Bell is at 20% and Guided's End Bell keeps its own level
- **TEST** `meditate-gtk/src/timer/mod.rs::a_volume_change_keeps_the_mode_it_was_made_in`

#### Scenario: The mode changes right after a Starting Bell change
- **WHEN** the user drags the Starting Bell volume to 20% and switches mode within 0.4 s
- **THEN** the slider in the new mode shows 20%, the level that was saved
- **TEST** `meditate-gtk/src/timer/mod.rs::a_pending_volume_is_saved_before_the_rows_reload`

#### Scenario: Start right after a Starting Bell change
- **WHEN** the user drags the Starting Bell volume to 20% and presses Start within 0.4 s
- **THEN** the starting bell rings at 20%
- **TEST** `meditate-gtk/src/timer/mod.rs::start_saves_a_pending_volume_first`

#### Scenario: The window closes right after a change
- **WHEN** the user drags a bell's volume to 20% and closes the window within 0.4 s
- **THEN** after a restart the bell is at 20%
- **MANUAL** drag the End Bell volume, close the window at once, start the app again: the new level shows (whether closing the window unmaps the slider needs a display; the test only proves the slider saves on unmap)

### Requirement: A volume preview plays the bell at each resting level
When the user lets go of a bell's volume slider, or a keyboard or scroll change rests for a moment, the app SHALL play that bell at the new level, restarting it if it is still ringing.

#### Scenario: The slider rests on a bell's Volume row
- **WHEN** the user changes a bell's volume with the arrow keys and stops
- **THEN** after a short pause the bell plays at the new level, with a Stop button while it rings
- **TEST** `meditate-gtk/src/volume_row.rs::a_volume_is_saved_even_when_the_page_has_closed`

#### Scenario: The user lets go of the slider
- **WHEN** the user taps a level or drags the slider and lets go, while the bell still rings
- **THEN** the ringing bell stops at once and plays again at the new level, without waiting
- **TEST** `meditate-gtk/src/volume_row.rs::letting_go_of_the_slider_plays_at_once`

#### Scenario: The slider is held still mid-drag
- **WHEN** the user drags the slider, holds it still for a second without letting go, then lets go
- **THEN** nothing restarts while it is held, and the bell plays once at the level let go on
- **AND** held still for 2 s or more, the bell plays once then, in case the release went to a swipe gesture
- **TEST** `meditate-gtk/src/volume_row.rs::letting_go_of_the_slider_plays_at_once`

#### Scenario: Several levels clicked in quick succession
- **WHEN** the user clicks one level after another on the same slider while the bell still rings
- **THEN** the bell restarts at each new level and the app keeps running
- **TEST** `meditate-gtk/src/volume_row.rs::a_volume_is_saved_even_when_the_page_has_closed`

#### Scenario: The last preview failed
- **WHEN** a bell's preview could not play (the audio output went away) and the slider rests again
- **THEN** the bell gets a new player and plays
- **TEST** `meditate-gtk/src/sound.rs::only_a_healthy_player_still_in_the_preview_slot_restarts`

### Requirement: A bell or sound write that fails says so
A failed write of an interval bell or a bell sound SHALL show "Save failed" or "Delete failed" and leave any dialog usable.

#### Scenario: Toggling, editing or deleting an interval bell fails (laptop)
- **WHEN** an interval bell's switch, edit or delete fails to save
- **THEN** "Save failed" or "Delete failed" shows
- **TEST** `meditate-gtk/src/db/mod.rs::no_write_result_is_dropped`

#### Scenario: Renaming or deleting a sound fails (both apps)
- **WHEN** a sound rename or delete fails
- **THEN** the message shows, the sound keeps its name and file, and on the phone the dialog closes and opens again normally
- **TEST** `meditate-android/src/app.rs::failed_dialog_writes_say_so_and_close`

### Requirement: Bell rows show the current sound and pattern names
After a sound or pattern is renamed or deleted, the Interval Bells list, the bell edit page and Setup's bell rows SHALL show its current name, or "Missing" when it is gone.

#### Scenario: A sound is renamed while the Interval Bells list is open (laptop)
- **WHEN** the user opens the sound chooser from a bell, renames the sound and goes back
- **THEN** the bell edit page and the Interval Bells list show the new name
- **TEST** `meditate-gtk/src/bells.rs::bell_pages_reread_names_when_shown`

#### Scenario: A pattern used by Setup is deleted (laptop)
- **WHEN** the user deletes the end bell's pattern in the chooser and goes back to Setup
- **THEN** Setup's end bell pattern row shows "Missing"
- **TEST** `meditate-gtk/src/timer/imp.rs::setup_rereads_bell_rows_when_shown_again`

#### Scenario: A sound is renamed or deleted on the phone
- **WHEN** the user renames or deletes a sound used by an interval bell
- **THEN** the Interval Bells list shows the new name or "Missing"
- **TEST** `meditate-android/src/app.rs::label_and_sound_renames_reach_widget_and_bell_list`

#### Scenario: A chooser stays open across a sync on the phone
- **WHEN** a bell or pattern chooser is open, the app resumes, and the sync pulls another device's delete
- **THEN** the chooser no longer lists the deleted item
- **TEST** `meditate-android/src/app.rs::a_pull_follows_the_guided_file_and_refills_choosers`

### Requirement: A bell preview stops when its editor closes and never shows a silent Stop
On the phone, a bell preview SHALL play only while its editor is open and a sound actually plays, with "Playback failed" when it cannot.

#### Scenario: Back pressed during a volume preview
- **WHEN** the user drags Volume in the interval-bell editor and presses the phone's Back
- **THEN** the bell stops and the editor closes as with Cancel
- **TEST** `meditate-android/src/app.rs::back_from_the_interval_editor_cancels_it`

#### Scenario: A sound whose file hasn't arrived yet
- **WHEN** the user taps Play on a sound or pattern that cannot play
- **THEN** "Playback failed" shows and the row keeps its Play button
- **TEST** `meditate-android/src/app.rs::a_preview_that_cannot_play_says_so`

#### Scenario: A preview cut short shows Play again
- **WHEN** a volume or sound-chooser preview stops before its end because headphones were plugged in or pulled
- **THEN** its Stop turns back into Play within a quarter second, not only after the sound's length; a preview that ends on its own reverts the same way, at the latest after its length
- **TEST** `meditate-android/src/app.rs::a_preview_cut_short_shows_play_again`

#### Scenario: Seen on the phone
- **WHEN** the maintainer drags Volume and presses Back; plays a long preview and plugs in or pulls headphones
- **THEN** the sound stops at once; the preview stops and its button shows Play again
- **MANUAL** sound needs a speaker

### Requirement: A volume drag the page takes over changes nothing
When the page takes over a touch that started on a volume slider (a quick swipe up or down in its first 0.1 s, before the slider holds the page), the slider SHALL keep the value it had before the touch, which is the saved one.

#### Scenario: A vertical swipe that starts on the slider
- **WHEN** a bell is at 80%, a finger lands on its slider at 30% and the page then takes the touch
- **THEN** the slider shows 80% again and the bell rings at 80%
- **TEST** `meditate-android/src/app.rs::a_cancelled_volume_drag_puts_the_slider_back`

#### Scenario: The same swipe on the FP5
- **WHEN** on the FP5 a quick swipe up starts on a bell's volume slider
- **THEN** the page scrolls and the slider shows the value it had before
- **MANUAL** touch handover between slider and page only happens on the phone

### Requirement: The interval-bell editor reaches every row
The phone's interval-bell editor SHALL scroll its body, so every row is reachable in landscape and split screen.

#### Scenario: The editor in landscape
- **WHEN** the interval-bell editor is opened with the phone in landscape
- **THEN** every row can be scrolled to and used, and the volume slider still sets its value
- **MANUAL** layout on a phone

### Requirement: Sound chooser rows leave room for the name
Each row of the phone's sound chooser SHALL show its controls as icon buttons in the laptop's order (check, play, edit, delete), so a long name in any language shows in full.

#### Scenario: A long imported sound name at 360 dp
- **WHEN** an imported sound with a long name is listed in Russian at 360 dp width
- **THEN** the full name shows, wrapped if needed, next to the play, edit and delete icons
- **MANUAL** layout on a phone

### Requirement: Interval bell times are set in seconds or minutes
Every interval bell's time SHALL be settable in seconds or minutes, from 5 s to 120 min, and SHALL ring at exactly that time.

#### Scenario: A bell 90 seconds from start
- **WHEN** a countdown has a bell "At 90 s"
- **THEN** it rings at 1:30
- **TEST** `meditate-core/src/bells.rs::a_bell_set_in_seconds_rings_at_that_second`

#### Scenario: A bell 30 seconds before end
- **WHEN** a 10-minute countdown has a bell "30 s before end"
- **THEN** it rings at 9:30
- **TEST** `meditate-core/src/bells.rs::a_bell_set_in_seconds_rings_at_that_second`

#### Scenario: A time keeps the unit it was set in
- **WHEN** a bell is set to 60 s and another to 1 min, and both are saved and read back
- **THEN** the first is 60 s and the second 1 min, and their titles say so
- **TEST** `meditate-core/src/db/interval_bells.rs::a_time_keeps_the_unit_it_was_set_in`

#### Scenario: Setting a time on the laptop and the phone
- **WHEN** a bell's time is set to 45 with the s switch, saved and reopened, in each app
- **THEN** the row reads "Every 45 s" and the editor shows 45 with s
- **MANUAL** editor widgets and the phone's UI don't run on the test host

### Requirement: Jitter is a time
A repeating bell's jitter SHALL be a time from 0 to half the bell's time, and each ring SHALL land anywhere from the planned gap minus the jitter to the gap plus the jitter.

#### Scenario: Every 7 minutes give or take one minute
- **WHEN** a bell rings every 7 min ±1 min
- **THEN** each gap is between 6:00 and 8:00, and the lowest and highest draws give 6:00 and 8:00
- **TEST** `meditate-core/src/bells.rs::a_jitter_in_seconds_bounds_each_gap`

#### Scenario: A jitter longer than half the time
- **WHEN** a bell every 60 s has a jitter of 45 s
- **THEN** each gap is between 30 s and 90 s, and the stored jitter stays 45 s
- **TEST** `meditate-core/src/bells.rs::a_jitter_is_capped_at_half_the_time`

### Requirement: A repeating bell's gap can grow by a step
A repeating bell SHALL have a step, 0 to 60 min; each gap SHALL be the previous gap plus the step, and a jitter SHALL apply around the grown gap.

#### Scenario: Every 60 seconds, growing by 5
- **WHEN** a bell rings every 60 s with a step of 5 s and no jitter
- **THEN** it rings at 1:00, 2:05, 3:15 and 4:30
- **TEST** `meditate-core/src/bells.rs::a_step_grows_each_gap`

#### Scenario: A time jump with a step
- **WHEN** that bell's session is suspended at 0:59 and woken at 3:20
- **THEN** the bell rings once, and next at 4:30
- **TEST** `meditate-core/src/bells.rs::a_step_survives_a_time_jump`

#### Scenario: A fixed bell has no jitter or step
- **WHEN** a bell every 60 s ±30 s, +5 s is switched to "At time from start", in either app
- **THEN** it is saved with no jitter and no step
- **TEST** `meditate-core/src/bells.rs::a_fixed_bell_keeps_no_jitter_or_step`

#### Scenario: The row title names all three
- **WHEN** a bell is every 120 s ±1 min with a step of 5 s, and another is the same with ±3 min
- **THEN** both rows read "Every 120 s ±1 min, +5 s" in both apps: a jitter shows as it rings
- **TEST** `meditate-core/src/bells.rs::the_title_key_carries_each_time_with_its_unit`

#### Scenario: The same titles on the laptop and the phone
- **WHEN** those two bells are listed in each app
- **THEN** the laptop reads "Every 120 s ±1 min, +5 s" and the phone "Every 120 s +/-1 min, +5 s" (its font has no plus-minus sign), for both rows
- **MANUAL** the GTK title is unit-tested (`bell_title_names_time_jitter_and_step_with_their_units`); the phone's title goes through Slint's catalogue, which only runs on Android

### Requirement: Bells from older versions keep their times
A bell written before this change, or by an older version over sync or in a preset, SHALL read with the same time and jitter it had and no step; a bell written now SHALL still read sensibly on the older version.

#### Scenario: A database from the last release
- **WHEN** a database from 26.10.1 with a bell every 7 min ±14 % is opened
- **THEN** the bell reads every 7 min, ±59 s, step 0
- **TEST** `meditate-core/src/db/mod.rs::a_bell_from_the_last_release_keeps_its_times`

#### Scenario: A synced bell from an older version
- **WHEN** an older version's edit of a bell (every 5 min ±20 %) arrives
- **THEN** the bell reads every 5 min, ±1 min, step 0
- **TEST** `meditate-core/src/db/interval_bells.rs::a_synced_bell_from_an_older_version_reads_in_seconds`

#### Scenario: A bell changed by an older version on this device
- **WHEN** a bell every 90 s, step 5 s is changed to 3 min by an older version on the same device, and this version opens the database again
- **THEN** the bell reads every 3 min, step 0
- **TEST** `meditate-core/src/db/interval_bells.rs::a_row_an_older_version_changed_reads_from_its_minutes`

#### Scenario: A bell written now, read by an older version
- **WHEN** a bell every 90 s ±30 s, step 5 s is saved
- **THEN** its event also carries 2 minutes and 33 %, which the older version reads
- **TEST** `meditate-core/src/db/interval_bells.rs::a_bell_in_seconds_also_writes_minutes_and_percent`

#### Scenario: A bell a newer device sent before this one updated
- **WHEN** a device on 26.10.1 receives a bell every 90 s ±30 s, step 5 s from an updated device, and then updates itself
- **THEN** after the update it reads every 90 s, ±30 s, step 5 s, like the other device
- **TEST** `meditate-core/src/db/mod.rs::a_bell_a_newer_device_sent_before_the_update_keeps_its_seconds`

#### Scenario: A newer device's change applied while this one was back on the older version
- **WHEN** a device goes back to 26.10.1, receives a change of a bell's step from 5 s to 10 s there, and updates again
- **THEN** the bell reads step 10 s
- **TEST** `meditate-core/src/db/mod.rs::a_change_received_on_the_older_version_is_kept_after_updating_again`

#### Scenario: A preset saved before this change
- **WHEN** a preset whose bell has only minutes 5 and jitter 20 % is loaded
- **THEN** its bell is every 5 min, ±1 min, step 0 and matches the same library bell, and a preset saved now still carries minutes and percent
- **TEST** `meditate-core/src/preset_config.rs::a_preset_bell_without_seconds_reads_from_minutes`

### Requirement: On Android a bell's volume sets the alarm step
On Android, a ringing bell SHALL set its stream (the alarm, or as media the media stream) to the step its volume needs, trimmed by the player's gain, so the volume slider works even on a device that reports a wrong volume range or ignores the gain.

#### Scenario: A device that reports no usable range
- **WHEN** a device with alarm steps 1 to 7 reports only 1.2 dB between its quietest and loudest step, almost all of it below step 2 (-1.2, -0.1, -0.08 ... 0 dB), and bells are at 0 %, 50 % and 100 %
- **THEN** they set the alarm to step 1, 4 and 7
- **TEST** `meditate-core/src/bell_volume.rs::the_slider_picks_the_alarm_step_by_its_position`

#### Scenario: A volume between two steps
- **WHEN** a device's steps 1 to 5 play at -40, -30, -20, -10 and 0 dB, and a bell is at 40 %
- **THEN** it sets step 3 and plays 4 dB below it, at -24 dB like before this change
- **TEST** `meditate-core/src/bell_volume.rs::a_volume_between_two_steps_is_trimmed_by_the_gain`

#### Scenario: A steep curve at the bottom
- **WHEN** a device's steps 1 to 5 play at -60, -55, -50, -20 and 0 dB, and a bell is at 25 %
- **THEN** it sets step 4 (step 2 by position plays at -55 dB, quieter than the -45 dB the bell needs) and plays 25 dB below it
- **TEST** `meditate-core/src/bell_volume.rs::the_curve_can_only_raise_the_step`

#### Scenario: A quiet bell while a louder one rings
- **WHEN** a bell at 40 % starts while a bell at 100 % holds the stream at step 5 of that curve (-40 to 0 dB)
- **THEN** the quiet bell's gain at step 5 is 24 dB down, so it still plays at -24 dB
- **TEST** `meditate-core/src/bell_volume.rs::a_quieter_bell_keeps_its_level_under_a_louder_one`

#### Scenario: A report that isn't a curve
- **WHEN** the report is missing (before Android 9), has the wrong length, holds NaN, or falls from step to step
- **THEN** the steps are spread evenly over 40 dB
- **TEST** `meditate-core/src/bell_volume.rs::a_report_that_is_not_a_curve_falls_back_to_even_steps`

#### Scenario: The alarm volume comes back
- **WHEN** a bell rings and ends, and the user did not touch the alarm volume meanwhile
- **THEN** the alarm volume is what it was before
- **MANUAL** on the phone, set the alarm volume to 3 of 7, ring a bell at 100 %: the system alarm slider is back at 3 afterwards

#### Scenario: The slider changes the loudness on the device
- **WHEN** a bell's volume slider is dragged from 0 % to 100 % with the preview ringing
- **THEN** the bell gets clearly louder, on the FP5 as before
- **MANUAL** on the FP5, and on the tablet or by the reporter of #8

### Requirement: A touched volume slider holds the page still
While a finger rests on a bell's volume slider, the page SHALL not scroll, so a drag that drifts up or down keeps moving the slider.

#### Scenario: A drag that drifts up and down
- **WHEN** on the setup page or in the interval bell editor a finger lands on a volume slider and moves sideways and up or down, then lifts, and a swipe then starts beside the slider
- **THEN** the slider follows the finger sideways, the page does not move while the finger is down nor when it lifts, the value where it lifts is saved, and the swipe beside the slider scrolls the page
- **TEST** `meditate-android/src/app.rs::a_touched_volume_slider_holds_the_page_still`

#### Scenario: The same drag on the FP5
- **WHEN** on the FP5 a finger lands on a bell's volume slider in the Timer setup and in the interval bell editor, drags left and right while drifting up and down, then lifts; then swipes up beside the slider
- **THEN** the slider follows sideways and never snaps back, the page stays still while the finger is down and does not jump or glide when it lifts, and the later swipe scrolls
- **MANUAL** touch handover between slider and page only happens on the phone

### Requirement: On Android bells play as media unless set to play as an alarm
On Android, every bell and volume preview SHALL play as media, where music would play: only in connected headphones, else on the speaker, at a media step the app sets for the bell's volume and puts back afterwards. With "Play Bells as Alarm" on in Preferences (default off) they SHALL play as an alarm, which rings on every device, aimed at the same headphones media would play on, so with headphones they ring only there too (from Android 9; before it, on the speaker and the headphones at once); without headphones on the speaker.

#### Scenario: The switch until set and after it
- **WHEN** the setting was never written; it was turned on; it holds text that isn't true or false; it was turned off
- **THEN** bells play as media, as an alarm, as media, as media
- **TEST** `meditate-core/src/settings_keys.rs::bells_play_as_an_alarm_once_turned_on`

#### Scenario: The headphone slider stops at the hearing-safety step
- **WHEN** a device has media steps 0 to 15, its headphone curve plays at -60 dB to 0 dB evenly, and bells are at 0 %, 50 % and 100 %
- **THEN** they set media steps 1, 6 and 10 (index 10 is Android's default hearing-safety step), never above 10; a device with 30 media steps stops at 10 too
- **TEST** `meditate-core/src/bell_volume.rs::the_headphone_slider_stops_at_the_hearing_safety_step`

#### Scenario: Which stream and curve a bell uses
- **WHEN** the levels are worked out for a bell at 50 % from an alarm curve, a speaker media curve and a headphone media curve
- **THEN** each level comes from its own curve: the alarm step from the alarm curve, the speaker media step from the speaker's media curve, the headphone step from the headphone curve capped at the safe step (which stream is used is checked in the player's source check below)
- **TEST** `meditate-android/src/alarm_volume.rs::a_bell_takes_the_level_of_where_it_plays`

#### Scenario: The media path in the player
- **WHEN** a bell or preview plays with the switch off
- **THEN** it plays with media audio attributes, the media stream is held at its step like the alarm stream and put back when nothing rings (also after a crash), and it stops when its output changes (headphones pulled or plugged in); the step is put back on the output it was set on, and only while that output is current
- **TEST** `meditate-android/src/alarm_volume.rs::a_media_bell_holds_the_media_step_and_puts_it_back`

#### Scenario: The alarm path in the player
- **WHEN** a bell or preview plays with the switch on
- **THEN** it plays with alarm audio attributes at the alarm step as before, is aimed at the headphones media plays on when there are any (else Android's own alarm route; only alarm players are aimed, media keeps Android's own route), and stops when its output changes (headphones pulled or plugged in), like a media bell
- **TEST** `meditate-android/src/alarm_volume.rs::an_alarm_bell_plays_only_in_the_headphones`

#### Scenario: The Bells group in Preferences
- **WHEN** Preferences is opened on Android; the switch is turned on; the app is restarted
- **THEN** a Bells group right after Stats shows the explanation under its heading and the switch off; on after the tap; still on after the restart; on Linux there is no such row
- **TEST** `meditate-android/src/app.rs::the_bells_group_holds_the_alarm_switch`; Android **MANUAL** on the FP5 for the layout at 360 px in German

#### Scenario: Bells on the FP5, as media and as an alarm
- **WHEN** the switch is off and a session with a starting, interval and end bell runs with wired headphones, with Bluetooth headphones, with none; headphones are pulled while a bell rings, and plugged in while one rings on the speaker; the switch is on and the same is done again
- **THEN** with headphones the bells ring only there and the speaker stays silent; without headphones they ring on the speaker; the same preset rings equally loud each time; the media volume is back where it was after each bell; pulling or plugging in headphones stops the bell, and both outputs' media volume is back afterwards; switched on, the same holds as an alarm, at the alarm volume, which is back where it was after each bell
- **MANUAL** on the FP5: routing and loudness only exist on a device
