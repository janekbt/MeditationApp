## ADDED Requirements

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
