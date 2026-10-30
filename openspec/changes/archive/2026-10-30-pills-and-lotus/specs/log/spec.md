## MODIFIED Requirements
### Requirement: Label colours follow the accent
The Log SHALL colour each label from the current accent: sixteen colours, the first being the accent, the next seven stepping round the colour wheel from it, and the last eight the same hues turned half a step and set apart in brightness; a session without a label stays grey.

#### Scenario: A green accent on Android
- **WHEN** the accent is Green and the phone is in light mode
- **THEN** the first label colour is the green the app uses for its buttons, colours 1 to 8 have hues about 140, 185, 230, 275, 320, 5, 50 and 95 degrees, and colours 9 to 16 the same hues plus 22.5 degrees, darker than colours 1 to 8
- **AND** in dark mode colours 1 to 8 are lighter than in light mode, and colours 9 to 16 lighter than colours 1 to 8
- **TEST** `meditate-android/src/theme.rs::label_colours_start_at_the_accent_and_step_round_the_wheel`

#### Scenario: The Linux stylesheet's accent colours load
- **WHEN** the Linux app loads its stylesheet under libadwaita
- **THEN** the sixteen label colours, defined relative to the system accent, load without an error
- **TEST** `meditate-gtk/src/log/mod.rs::the_label_colours_load_relative_to_the_accent`

#### Scenario: The accent is changed while the Log is open
- **WHEN** the Log shows labelled sessions and the accent is changed (Linux: GNOME Settings, Appearance; Android: Preferences)
- **THEN** the stripes and the chips and dots take the new colours at once, each label keeps its place, and unlabelled sessions stay grey
- **MANUAL** on the laptop and the FP5: colour on screen can't be checked on the host

## ADDED Requirements
### Requirement: A labelled card shows its label in a pill
On the phone, a Log card with a label SHALL show the label as a pill like the laptop's: a light tint of the label's colour behind a small dot and the name in that colour.

#### Scenario: Two labels in the Log
- **WHEN** the Log shows sessions with two different labels and one without
- **THEN** each labelled card shows a pill tinted in its label's colour, with the dot and the name in that colour, matching its stripe; the unlabelled card shows no pill
- **MANUAL** on the FP5: colours and the pill's shape need a render.

#### Scenario: A long label name
- **WHEN** a label's name is longer than the card is wide, in any language
- **THEN** the name wraps inside the pill and shows in full
- **MANUAL** on the FP5 at 360 px width: wrapping needs a render.
