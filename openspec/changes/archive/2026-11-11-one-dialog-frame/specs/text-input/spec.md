## ADDED Requirements

### Requirement: Custom Time cannot be set to 0:00
In both apps, the Custom Time dialog SHALL keep Set off while both hours and minutes are zero.

#### Scenario: Custom Time at 0:00 on the phone
- **WHEN** Custom Time is set to 0 hours and 0 minutes, then 5 is typed into the minutes without leaving the field
- **THEN** Set is off at 0:00, also for a screen reader's tap, and on as soon as the 5 is typed; Set then sets 0:05
- **TEST** `meditate-android/src/app.rs::the_time_dialogs_are_number_fields`

#### Scenario: Custom Time at 0:00 on the laptop
- **WHEN** Custom Time opens at 0:10 and the minutes are set to 0, then the hours to 1
- **THEN** Set is off at 0:00 and on again at 1:00; a dialog opened at 0:00 starts with Set off
- **TEST** `meditate-gtk/src/timer/imp.rs::custom_time_cannot_be_set_to_zero`

### Requirement: The time dialogs take a number like Linux
On Android, Custom Time, Preparation Time and Daily Goal SHALL each be a − / + number field with Set.

#### Scenario: Preparation Time steps by five
- **WHEN** Preparation Time shows 15 seconds and the user taps + or −, or types 13 and taps + or −
- **THEN** + gives 20 and − gives 10; a typed 13 stays 13 until + gives 15 or − gives 10; − stops at 5, + at 300
- **TEST** `meditate-android/src/app.rs::the_time_dialogs_are_number_fields`

#### Scenario: Daily Goal is a number field
- **WHEN** Daily Goal opens on a 20 minute goal
- **THEN** it shows 20 with − and +, steps by 1 between 1 and 1440, and Set stores the shown number
- **TEST** `meditate-android/src/app.rs::the_time_dialogs_are_number_fields`

#### Scenario: On the FP5
- **WHEN** Custom Time is set to 0:00, then a 5 typed, on the phone and on the laptop; Preparation Time stepped with + and −; Daily Goal stepped, 1440 shown, and a number typed
- **THEN** Set is grey at 0:00 and lights up with the 5 (on the laptop: with any non-zero time); Preparation steps by 5 and snaps to fives; Daily Goal shows 1440 in full and stores the shown number
- **MANUAL** typing with the phone keyboard exists only on a device

## MODIFIED Requirements

### Requirement: A dialog stays above the keyboard and Enter confirms it
On Android, every modal card SHALL center in the part of the window above the on-screen keyboard while one is shown, and Enter in a dialog's single text field SHALL do what its confirm button does while that button is enabled.

#### Scenario: Where the cards sit
- **WHEN** a modal card is shown with and without the keyboard open
- **THEN** its `y` centers it between the window's top and the keyboard's top (the window's bottom without a keyboard), never closer than 16 px below the status bar; a card too tall for that keeps its bottom (the buttons) above the keyboard and goes up to the window's top; one taller than the whole room above the keyboard (landscape) starts at the window's top
- **TEST** `meditate-android/src/app.rs::dialogs_center_above_the_keyboard`

#### Scenario: Enter confirms
- **WHEN** Enter is pressed in the field of Create or Rename Preset, Create or Rename Label, Rename Sound, Rename Track, or Track name on import
- **THEN** the dialog confirms as its button would, only while that button is enabled; whenever a dialog closes, the keyboard closes with it
- **TEST** `meditate-android/src/app.rs::dialogs_center_above_the_keyboard`

#### Scenario: The field is ready to type
- **WHEN** one of those seven dialogs opens
- **THEN** its field has the focus and the keyboard comes up, as on Linux; a pre-filled field (a rename, Track name) has its text selected, so typing replaces it
- **TEST** `meditate-android/src/app.rs::dialogs_center_above_the_keyboard`

#### Scenario: Enter in a number field
- **WHEN** Enter is pressed in a Custom Time, Preparation Time, Daily Goal or countdown field or in a − / + row's number
- **THEN** the typed number is taken (clamped as before) and the keyboard closes; the dialog or page stays open
- **TEST** `meditate-android/src/app.rs::dialogs_center_above_the_keyboard`

#### Scenario: On the FP5
- **WHEN** Create Preset is opened and a name typed with Gboard; then Enter; then Daily Goal and Duration; then the bell time in the interval editor
- **THEN** the title, field and both buttons stay visible above the keyboard; Enter creates the preset and the keyboard is gone; the other dialogs sit above the keyboard too; Enter in Duration, Daily Goal and the bell time takes the number and closes the keyboard
- **MANUAL** keyboard placement exists only on a device
