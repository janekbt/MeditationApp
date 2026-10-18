## ADDED Requirements

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
- **THEN** the stripes (and on Linux the chips and dots) take the new colours at once, each label keeps its place, and unlabelled sessions stay grey
- **MANUAL** on the laptop and the FP5: colour on screen can't be checked on the host

### Requirement: Labels take colours in the order they were created
Each label SHALL take the next of the sixteen colours in the order labels were created, the same on every device once synced, so the first sixteen labels all differ.

#### Scenario: Three labels, then a fourth
- **WHEN** "Morning", "Walking" and "Evening" are created in that order, then "Body Scan"
- **THEN** they take colours 1, 2, 3 and 4, and creating "Body Scan" changes none of the first three
- **TEST** `meditate-core/src/db/labels.rs::labels_take_colours_in_the_order_they_were_created`

#### Scenario: A label is renamed or deleted
- **WHEN** "Walking" (colour 2 of three) is renamed to "Walk", and later "Morning" (colour 1) is deleted
- **THEN** the rename keeps colour 2; after the delete "Walk" has colour 1 and "Evening" colour 2
- **TEST** `meditate-core/src/db/labels.rs::a_rename_keeps_a_label_colour_and_a_delete_moves_later_ones_up`

#### Scenario: Two devices created labels before they synced
- **WHEN** device A created "Morning" and device B created "Walking", each offline, and both have synced
- **THEN** both devices give each label the same colour
- **TEST** `meditate-core/src/db/labels.rs::synced_devices_agree_on_label_colours`

#### Scenario: A device made a label before its first sync
- **WHEN** a laptop has "Morning" and "Evening" after some sessions, and a new phone that never synced creates "Walking" (or restores a backup), then both sync
- **THEN** both devices agree on every label's colour, and "Walking" may take an early place, moving the laptop's labels on by one, once
- **TEST** `meditate-core/src/db/labels.rs::a_label_made_before_a_device_first_synced_moves_the_others_once`

#### Scenario: The seventeenth label
- **WHEN** seventeen labels exist
- **THEN** the seventeenth has colour 1 again
- **TEST** `meditate-core/src/db/labels.rs::labels_take_colours_in_the_order_they_were_created`
