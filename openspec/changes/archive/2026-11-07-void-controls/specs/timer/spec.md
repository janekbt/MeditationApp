## MODIFIED Requirements
### Requirement: A running session keeps the device awake, a paused one doesn't
While a session runs on Linux, the app SHALL block automatic suspend, whether or not "Keep screen awake" is on; the screen itself stays on only with that setting. While a session is paused, on Linux and Android, the app SHALL NOT keep the device or its screen awake, except the phone's processor while Void-Controls is on, and SHALL take that back on resume. If the system refuses the block, the app SHALL say so.

#### Scenario: Automatic suspend during a session
- **WHEN** a session runs and the computer's automatic-suspend time passes
- **THEN** the computer stays awake and the bells ring on time
- **TEST** `meditate-gtk/src/timer/mod.rs::a_running_session_blocks_suspend`

#### Scenario: A forgotten pause on Linux
- **WHEN** a session is paused and left
- **THEN** the computer can suspend and blank its screen as usual, and resuming blocks suspend again
- **TEST** `meditate-gtk/src/timer/mod.rs::a_paused_session_lets_the_computer_suspend`

#### Scenario: A forgotten pause on Android
- **WHEN** a session is paused and left with Void-Controls off
- **THEN** the phone releases its wake lock and the screen-on hold, and resuming takes them back
- **TEST** `meditate-android/src/app.rs::a_paused_session_lets_the_phone_sleep`

#### Scenario: A pause on Android with Void-Controls on
- **WHEN** a session is paused with Void-Controls on, and the switch is then turned off during the pause
- **THEN** the screen-on hold is released but the processor stays awake; once the switch is off it may sleep
- **TEST** `meditate-android/src/app.rs::a_void_controls_pause_keeps_the_processor_awake`

#### Scenario: The system refuses the block
- **WHEN** a session starts and GNOME or the portal refuses to block suspend
- **THEN** a message says "Couldn't keep the computer awake for the bells"
- **TEST** `meditate-gtk/src/timer/mod.rs::a_refused_suspend_block_tells_the_user`

## ADDED Requirements
### Requirement: Void-Controls turn and shake the phone to control a session
On the phone with Void-Controls on, turning the phone face down SHALL pause a session, turning it face up SHALL resume it, and shaking it SHALL save it, each only where the session page offers that button, with a short vibration when the session acts on it.

#### Scenario: Each gesture in every state
- **WHEN** face down, face up and shake arrive while running, preparing, paused, paused in preparation, and in overtime of 30 s and of 90 s
- **THEN** running: Pause, nothing, Save; preparing: Pause, nothing, nothing; paused: nothing, Resume, Save; paused in preparation: nothing, Resume, nothing; overtime 30 s: nothing, nothing, Save; overtime 90 s: nothing, nothing, Add
- **TEST** `meditate-android/src/app.rs::each_void_gesture_stands_for_one_session_button`

#### Scenario: A gesture acts like the button and vibrates
- **WHEN** a gesture the session offers arrives, and one it doesn't offer
- **THEN** the first goes through the same path as the media control's button and vibrates once briefly; the second changes nothing and doesn't vibrate
- **TEST** `meditate-android/src/app.rs::a_void_gesture_goes_through_the_session_buttons`

#### Scenario: The sensor listens only with the switch on and a session live
- **WHEN** a session starts with the switch off, the switch is turned on and off during it, and a session starts with it on and ends
- **THEN** the phone listens for gestures only while the switch is on during a session, and stops when the session ends
- **TEST** `meditate-android/src/app.rs::void_controls_listen_only_during_a_session`

#### Scenario: Turning, shaking and the vibration on the phone
- **WHEN** on the FP5 with the screen off, a Timer session runs; the phone lies face up, is turned face down, waits, is turned face up, is shaken; the start happens with the phone already face down; the phone is knocked once on the floor and set down hard; the phone is carried around in a pocket for a minute
- **THEN** face down pauses with one short vibration, face up resumes with one, shaking saves with one; starting face down does not pause; a single knock or a hard set-down does not save; a turn during a shake does not also pause; carrying it in a pocket may set it off (what the explanation warns about)
- **MANUAL** on the FP5: sensors and vibration exist only on a phone.
