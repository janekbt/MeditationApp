## ADDED Requirements
### Requirement: On Android bells play as media unless set to play as an alarm
On Android, every bell and volume preview SHALL play as media, where music would play: only in connected headphones, else on the speaker, at a media step the app sets for the bell's volume and puts back afterwards. With "Play Bells as Alarm" on in Preferences (default off) they SHALL play as an alarm as before, which rings on every device.

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

#### Scenario: The Bells group in Preferences
- **WHEN** Preferences is opened on Android; the switch is turned on; the app is restarted
- **THEN** a Bells group right after Stats shows the explanation under its heading and the switch off; on after the tap; still on after the restart; on Linux there is no such row
- **TEST** `meditate-android/src/app.rs::the_bells_group_holds_the_alarm_switch`; Android **MANUAL** on the FP5 for the layout at 360 px in German

#### Scenario: Bells as media on the FP5
- **WHEN** the switch is off and a session with a starting, interval and end bell runs with wired headphones, with Bluetooth headphones, with none; headphones are pulled while a bell rings, and plugged in while one rings on the speaker; the switch is on with headphones connected
- **THEN** with headphones the bells ring only there and the speaker stays silent; without headphones they ring on the speaker; the same preset rings equally loud each time; the media volume is back where it was after each bell; pulling or plugging in headphones stops the bell, and both outputs' media volume is back afterwards; switched on, the bell rings on the speaker and in the headphones as before
- **MANUAL** on the FP5: routing and loudness only exist on a device

## MODIFIED Requirements
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
