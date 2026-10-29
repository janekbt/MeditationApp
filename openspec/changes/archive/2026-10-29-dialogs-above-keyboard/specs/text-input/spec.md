## ADDED Requirements
### Requirement: A dialog stays above the keyboard and Enter confirms it
On Android, every modal card SHALL center in the part of the window above the on-screen keyboard while one is shown, and Enter in a dialog's single text field SHALL do what its confirm button does while that button is enabled.

#### Scenario: Where the cards sit
- **WHEN** a modal card is shown with and without the keyboard open
- **THEN** its `y` centers it between the window's top and the keyboard's top (the window's bottom without a keyboard), never closer than 16 px below the status bar; a card too tall for that keeps its bottom (the buttons) above the keyboard and goes up to the window's top; one taller than the whole room above the keyboard (landscape) starts at the window's top
- **TEST** `meditate-android/src/app.rs::dialogs_center_above_the_keyboard`

#### Scenario: Enter confirms
- **WHEN** Enter is pressed in the field of Create or Rename Preset, Create or Rename Label, Rename Sound, Rename Track, Track name on import, or Daily Goal
- **THEN** the dialog confirms as its button would, only while that button is enabled; whenever a dialog closes, the keyboard closes with it
- **TEST** `meditate-android/src/app.rs::dialogs_center_above_the_keyboard`

#### Scenario: The field is ready to type
- **WHEN** one of those eight dialogs opens
- **THEN** its field has the focus and the keyboard comes up, as on Linux; a pre-filled field (a rename, Track name, Daily Goal) has its text selected, so typing replaces it
- **TEST** `meditate-android/src/app.rs::dialogs_center_above_the_keyboard`

#### Scenario: Enter in a number field
- **WHEN** Enter is pressed in a Duration or countdown field or in a − / + row's number
- **THEN** the typed number is taken (clamped as before) and the keyboard closes; the dialog or page stays open
- **TEST** `meditate-android/src/app.rs::dialogs_center_above_the_keyboard`

#### Scenario: On the FP5
- **WHEN** Create Preset is opened and a name typed with Gboard; then Enter; then Daily Goal and Duration; then the bell time in the interval editor
- **THEN** the title, field and both buttons stay visible above the keyboard; Enter creates the preset and the keyboard is gone; the other dialogs sit above the keyboard too; Enter in Duration and in the bell time takes the number and closes the keyboard
- **MANUAL** keyboard placement exists only on a device
