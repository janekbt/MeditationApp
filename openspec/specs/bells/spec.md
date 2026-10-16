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

#### Scenario: Seen on the phone
- **WHEN** the maintainer drags Volume and presses Back
- **THEN** the sound stops at once
- **MANUAL** sound needs a speaker

### Requirement: A volume drag the page takes over changes nothing
When the page takes over a drag that started on a volume slider, the slider SHALL go back to the value it had before the touch, which is the saved one.

#### Scenario: A vertical swipe that starts on the slider
- **WHEN** a bell is at 80%, a finger lands on its slider at 30% and the page then scrolls
- **THEN** the slider shows 80% again and the bell rings at 80%
- **TEST** `meditate-android/src/app.rs::a_cancelled_volume_drag_puts_the_slider_back`

#### Scenario: The same swipe on the FP5
- **WHEN** on the FP5 a vertical swipe starts on a bell's volume slider
- **THEN** after the scroll the slider shows the value it had before
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
