# Text input

## Purpose
Text fields on Android take their text, cursor and selection from the
system keyboard and show the system's cut/copy/paste bar. This covers
how they follow the keyboard and where that bar appears, and, in both
apps, what a click or tap into a number field does.

## Requirements

### Requirement: A cursor move from the keyboard moves the cursor
On Android, a text field SHALL move its cursor or selection to where the keyboard sets it, without changing the text.

#### Scenario: Sliding over the space bar
- **WHEN** the user slides over Gboard's space bar in a field with text, left and then right
- **THEN** the cursor follows the slide in both directions, and typing a letter inserts it at the new place
- **MANUAL** on the FP5 (Android 15) and the API 30 and 35 emulators: Java and the Android IME can't run on the host.

#### Scenario: The cursor stays visible while it slides
- **WHEN** the user slides over Gboard's space bar
- **THEN** the cursor stays lit while it moves and starts blinking again only once it stops, as with the arrow keys
- **MANUAL** on the FP5: found in the device test (maintainer); blinking needs a person watching.

#### Scenario: Sliding over the backspace key selects, then deletes
- **WHEN** the user slides left from Gboard's backspace key over two words and lets go
- **THEN** the words are highlighted while sliding and deleted on release
- **MANUAL** same devices; it uses a selection range, not a collapsed cursor.

#### Scenario: The move is not an edit
- **WHEN** the user slides the cursor in the rename dialog of a label without typing
- **THEN** the text is unchanged and the dialog behaves as if nothing was typed
- **MANUAL** on the FP5 with the spike's logging: no text update reaches Rust for the move (log shows only the selection), so `edited` can't fire.

#### Scenario: Sliding right after a word with suggestions
- **WHEN** the user types a word with Gboard's suggestions on and slides over the space bar at once
- **THEN** the word stays where it was typed, written once, and the cursor moves
- **MANUAL** on the FP5: Gboard ends the word without telling Slint first.

#### Scenario: Sliding in a number field
- **WHEN** the user slides the cursor in a field that only takes digits
- **THEN** the cursor moves and the digits stay as they were
- **MANUAL** on the FP5.

#### Scenario: Moving the cursor while a word is still being composed
- **WHEN** the user types part of a word (Gboard underlines it) and slides over the space bar before finishing it
- **THEN** the suggestions keep matching the underlined word, and the word is written once, where it was typed
- **MANUAL** on the FP5: the composing range lives in Gboard; upstream's #14205 as merged reports it with each selection change.

### Requirement: The copy bar sits next to the selection
On Android, the cut/copy/paste bar SHALL appear above the selected text, or below it when there's no room above, never on it.

#### Scenario: A selection in the middle of the screen
- **WHEN** the user long-presses a word in a field in the middle of the screen
- **THEN** the bar sits directly above the highlighted word and the word stays readable
- **MANUAL** on the FP5 and the API 26, 30 and 35 emulators.

#### Scenario: A selection near the top of the screen
- **WHEN** the user selects a word in a field just below the app bar
- **THEN** the bar sits above the word if it fits, otherwise below it, never on it
- **MANUAL** same devices; this is where Slint's `top < 0` workaround applies.

### Requirement: A long-press selection extends by word
On Android, after a long press selects a word, moving the held finger SHALL extend the selection by whole words and never shrink it below that word.

#### Scenario: The finger trembles inside the word
- **WHEN** the user long-presses a word and moves the finger a little within it before lifting
- **THEN** the whole word stays selected
- **MANUAL** on the FP5 and the API 35 emulator: the long press is Android-only. Upstream the core part has an automated test (task 1.1).

#### Scenario: The finger drifts before the long press fires
- **WHEN** the user presses a word and the finger drifts a few pixels before the long press fires
- **THEN** the selection starts at the word where the press started and reaches the word under the finger: within the same word only that word, into the next word both words, as upstream merged it
- **MANUAL** same devices; upstream's `text_input_long_press` test covers the core part.

#### Scenario: The finger moves on to the next word
- **WHEN** the user long-presses a word and drags right into the next word
- **THEN** the selection covers both whole words; dragging back left over the first word keeps the first word selected
- **MANUAL** same devices.

#### Scenario: A long press on an empty line or after the last word
- **WHEN** the user long-presses an empty line in a multi-line field, or behind the end of the text
- **THEN** no word is selected and the press goes on like a plain press: the cursor sits where the finger is
- **MANUAL** on the FP5 and the API 35 emulator; upstream's `text_input_long_press` test covers the core part.

#### Scenario: A double tap that is then held
- **WHEN** the user double-taps a word and keeps the finger down until the long press fires
- **THEN** the word from the double tap stays selected, and it does not grow
- **MANUAL** same devices.

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

### Requirement: A tapped number field readies for a new number and still shows the old one
In both apps, a number field that gets the focus SHALL let the first typed digit start a new number while the old number stays visible until then, and SHALL keep the old number when left without typing.

#### Scenario: Clicking into a number field on Linux
- **WHEN** the user clicks a number field showing 10 that does not have the focus
- **THEN** the whole number is selected
- **TEST** `meditate-gtk/src/window/mod.rs::a_clicked_number_field_selects_its_number`

#### Scenario: Clicking a Linux number field that already has its number selected
- **WHEN** the focus is already in a field showing 10 with the number selected (after Tab, or as a dialog opens) and the user clicks it
- **THEN** the number stays selected
- **TEST** `meditate-gtk/src/window/mod.rs::a_clicked_number_field_selects_its_number`

#### Scenario: A number field in a dialog
- **WHEN** the user opens the Custom Time dialog, or a dialog over another one, and clicks into a number
- **THEN** the number is selected, as in the main window
- **TEST** `meditate-gtk/src/window/mod.rs::every_dialog_selects_numbers_on_click`

#### Scenario: Clicking inside a number while typing it on Linux
- **WHEN** the user has typed 0.8 into a number field and clicks between the 0 and the point
- **THEN** the cursor moves there and nothing is selected, so a digit can be inserted
- **TEST** `meditate-gtk/src/window/mod.rs::a_clicked_number_field_selects_its_number`

#### Scenario: Selecting part of a number with the pointer on Linux
- **WHEN** the user drags over one digit of a number field
- **THEN** that digit stays selected
- **TEST** `meditate-gtk/src/window/mod.rs::a_clicked_number_field_selects_its_number`

#### Scenario: Clicking or tapping into a number field on the devices
- **WHEN** the user clicks a number field showing 10 (laptop) or taps it (Librem 5) and types 12
- **THEN** the 10 is highlighted after the click or tap with no copy bubble over it, and the field then shows 12
- **MANUAL** on the laptop and the Librem 5: real pointer and touch events run through GTK's own handling, which the host test only models.

#### Scenario: Leaving a Linux number field without typing
- **WHEN** the user clicks into a field showing 10 and clicks elsewhere without typing
- **THEN** it still shows 10 and the setting is unchanged
- **MANUAL** on the laptop: GTK's own commit on focus loss; nothing of ours runs.

#### Scenario: Tapping a number field on Android
- **WHEN** the user taps any number field: Custom Time, Preparation Time, Daily Goal, an interval bell's Time, Jitter or Step, or a vibration editor stepper
- **THEN** the field shows its old number pale; the first digit typed replaces it, and leaving without typing shows the number as before
- **MANUAL** on the FP5: the Slint fields run only on the device; `meditate-android/src/app.rs::interval_editor_numbers_let_go_of_focus` checks both components carry the pale number.
