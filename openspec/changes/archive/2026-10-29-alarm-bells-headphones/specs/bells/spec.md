## MODIFIED Requirements
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
