## ADDED Requirements

### Requirement: A session ends the same way however it is ended
A session SHALL count as ended once it is stopped, whether it was running, paused or in preparation, and SHALL record only meditation time as its length.

#### Scenario: Stopped while paused
- **WHEN** a running session is paused and then stopped (Save, and the save fails so the page stays)
- **THEN** it shows as ended with Save and the note, the play key does nothing, and the media control no longer offers Resume
- **TEST** `meditate-core/src/session/mod.rs::a_session_stopped_while_paused_is_ended`

#### Scenario: Stopped during preparation
- **WHEN** a Timer with 30 seconds of preparation is stopped 20 seconds in
- **THEN** it ends with a length of 0 seconds; the preparation is never recorded as meditation
- **TEST** `meditate-core/src/session/mod.rs::a_stop_during_preparation_records_no_time`

#### Scenario: The app is killed during preparation
- **WHEN** a Timer with 5 minutes of preparation is killed 2.5 minutes in, and the app starts again
- **THEN** no session with the preparation's minutes is recovered: the crash-recovery heartbeat records 0 during preparation, on both apps
- **TEST** `meditate-core/src/session/mod.rs::a_stop_during_preparation_records_no_time`

#### Scenario: An ended session shows its length, on the laptop
- **WHEN** a 45 minute countdown ends (by itself, in overtime by Save or Add, or by Save after 7 minutes) and the running page stays up
- **THEN** the time reads the length that is saved: 45:00, 45:00 or the overtime-added length, and 07:00
- **TEST** `meditate-core/src/session/mod.rs::an_ended_session_shows_its_length`

#### Scenario: An ended session shows its length, on the phone
- **WHEN** a 45 minute countdown ends on the phone and the running page stays up
- **THEN** the time reads 45:00, not 00:00
- **TEST** `meditate-android/src/app.rs::an_ended_session_shows_its_length`

### Requirement: No bell rings after the end bell
When a countdown reaches its end, whether its time ran out or a guided track ended early, the session SHALL skip every bell due by its planned length, so none rings just after the end bell.

#### Scenario: A guided track ends a second early
- **WHEN** a guided file of 900 seconds has bells due at 899 and at 900 seconds and the track ends at 899.4 seconds
- **THEN** the end bell rings and neither bell rings afterwards; a bell due at 960 seconds still rings in overtime
- **TEST** `meditate-core/src/session/mod.rs::a_guided_track_ending_early_rings_no_bell_after_the_end_bell`

### Requirement: "Meditation complete" comes only at an end the session reached by itself
Both apps SHALL post "Meditation complete" with the planned length, while the app is not in front, once when a session reaches its planned end by itself, and never when the user ends it.

#### Scenario: Each way a session can end
- **WHEN** a 60 second Timer runs into overtime; a Box Breath countdown reaches its end; a countdown with Auto-Finish ends; and, separately, sessions end by Save, by Save in overtime and by Add
- **THEN** the first three each say "complete" once with the planned length (60 s for the Timer), at the crossing and not on the ticks after; the hand ends say nothing
- **TEST** `meditate-core/src/session/mod.rs::only_an_end_reached_by_itself_is_complete`

#### Scenario: On the laptop and the phone
- **WHEN** a 1 minute Timer runs out while another app is in front, on the laptop and on the phone
- **THEN** "Meditation complete" with 01:00 shows once; ending a session by Save shows nothing
- **MANUAL** notifications only show on a real desktop or phone
