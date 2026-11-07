## ADDED Requirements

### Requirement: The chosen guided file comes back after a restart
Each device SHALL remember the Guided file chosen last, from the library or the local folder, and choose it again when the app starts, unless the file is gone or not found by then; this memory SHALL stay on the device.

#### Scenario: The app is started again
- **WHEN** a library file is chosen in Guided and the app is closed and started again
- **THEN** the same file is chosen in Guided
- **TEST** `meditate-core/src/guided.rs::the_chosen_file_comes_back_after_a_restart`

#### Scenario: A local folder file is started again
- **WHEN** a local folder file is chosen and the app is started again with the same folder
- **THEN** that file is chosen
- **TEST** `meditate-core/src/guided.rs::a_chosen_local_file_comes_back_after_a_restart`

#### Scenario: The file was deleted before the restart
- **WHEN** the chosen library file is deleted, here or on another device, before the next start
- **THEN** no file is chosen at the next start
- **TEST** `meditate-core/src/guided.rs::a_remembered_file_that_is_gone_stays_unchosen`

#### Scenario: The local file is missing or the folder changed
- **WHEN** the chosen local file is missing, or another local folder is chosen, before the next start
- **THEN** no file is chosen at the next start
- **TEST** `meditate-core/src/guided.rs::a_remembered_local_file_that_is_not_found_stays_unchosen`

#### Scenario: The local file's length was never read
- **WHEN** the chosen local file's length was still unread when the app ended
- **THEN** no file is chosen at the next start, and no notice shows
- **TEST** `meditate-core/src/guided.rs::a_remembered_local_file_without_a_length_stays_unchosen`

#### Scenario: A one-off Open File pick
- **WHEN** a library file is chosen, then a file is opened once with Open File, and the app is started again
- **THEN** no file is chosen
- **TEST** `meditate-core/src/guided.rs::a_one_off_pick_forgets_the_chosen_file`

#### Scenario: Another device chooses a file
- **WHEN** the laptop chooses a file and the devices sync
- **THEN** the phone's chosen file is unchanged
- **TEST** `meditate-core/src/guided.rs::the_chosen_file_stays_on_its_device`

#### Scenario: Android ends the app in the background
- **WHEN** a file is chosen in Guided on the phone, Android ends the app in the background, and the user opens it again
- **THEN** Guided is open with that file chosen
- **MANUAL** on the FP5: choose a file, end the app with `adb shell am kill` while it is in the background, open it; process death needs the phone

### Requirement: A starred guided file starts from the widget
The phone's home-screen widget SHALL list the starred guided files after the Timer presets and before the Box Breath presets, in the Starred Files order with the second line Starred Files shows, and a tap on one SHALL open Guided and start that file.

#### Scenario: Files between the Timer and Box Breath presets
- **WHEN** Timer and Box Breath presets and two files are starred
- **THEN** the widget shows the Timer presets, then the two files in their Starred Files order, then the Box Breath presets
- **TEST** `meditate-android/src/widget.rs::the_widget_lists_files_between_timer_and_box_breath`

#### Scenario: Tapping a starred file in the widget
- **WHEN** the user taps a starred file in the widget while no session runs
- **THEN** the app opens in Guided with that file chosen and its session starts
- **MANUAL** tap a widget file row on the phone; the widget host can't run on the laptop

#### Scenario: A tap while a session runs
- **WHEN** a session runs or waits to be saved and the user taps a file in the widget
- **THEN** the app shows that session and starts nothing
- **MANUAL** on the phone, during a Timer session tap a widget file row

#### Scenario: The tapped file is gone
- **WHEN** the tapped file was deleted, or the local file is not found
- **THEN** the app opens in Guided, says "File not found" and starts nothing
- **MANUAL** on the phone: star a local file, rename it on the phone's storage, refresh the folder, tap its widget row

#### Scenario: A local file whose length is still unread
- **WHEN** the tapped local file's length has not been read yet
- **THEN** the file is chosen, its length is read as for a tap in the app, and nothing starts
- **MANUAL** on the phone: star a new local file before its length is read, tap its widget row

#### Scenario: A file row and a preset row are told apart
- **WHEN** the widget hands the app a tapped row
- **THEN** a preset row starts that preset and a file row that file, and a row drawn by the previous version still starts its preset
- **TEST** `meditate-android/src/widget.rs::a_widget_tap_tells_a_file_from_a_preset`
