## ADDED Requirements

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
