# Names

## Purpose
How both apps take a name for a new, renamed or imported preset, label, guided file, bell sound or vibration pattern: what makes a name acceptable, what the dialog says, and what happens when saving it fails.

## Requirements

### Requirement: A name is trimmed and must be free
Every created, renamed or imported name SHALL lose its leading and trailing spaces and SHALL be refused when it is empty or used by another item of the same kind, ignoring case.

#### Scenario: Spaces around a name
- **WHEN** a preset, label, guided file, bell sound or pattern is created or renamed as "  Walk  "
- **THEN** it is stored as "Walk"
- **TEST** `meditate-core/src/naming.rs::every_kind_stores_a_name_without_outer_spaces`

#### Scenario: An import's picked file name on Linux
- **WHEN** the user imports "Pulse.ogg" while a sound "Pulse" exists
- **THEN** the import dialog opens with Import off and "This name is already in use", before any conversion
- **TEST** `meditate-gtk/src/name_dialog.rs::an_import_checks_the_picked_files_name`

#### Scenario: An empty name
- **WHEN** any of the five kinds is created or renamed as "   "
- **THEN** the write is refused and nothing changes
- **TEST** `meditate-core/src/naming.rs::every_kind_refuses_an_empty_name`

#### Scenario: A bell sound takes a used name in other case
- **WHEN** a bell sound "Kansho" exists and another sound is imported or renamed as "kansho"
- **THEN** the write is refused as a taken name
- **TEST** `meditate-core/src/db/bell_sounds.rs::a_bell_sound_cannot_take_a_used_name`

#### Scenario: Bell sounds that already share a name
- **WHEN** two bell sounds already have the same name (from an older version or a sync)
- **THEN** both still load and play, and either can be renamed to a free name; a rename that only changes the case of the shared name is refused, since the other sound holds it
- **TEST** `meditate-core/src/db/bell_sounds.rs::bell_sounds_that_already_share_a_name_keep_working`

#### Scenario: A new label is known at once
- **WHEN** a label is created
- **THEN** the app gets the new label from the write itself, so a save that worked never reports "Save failed"
- **TEST** `meditate-core/src/db/labels.rs::creating_a_label_returns_it`

#### Scenario: A new preset is starred
- **WHEN** a preset is created from either app
- **THEN** it is starred, so it shows on Setup's chips at once
- **TEST** `meditate-core/src/db/presets.rs::a_new_preset_is_starred`

### Requirement: A name dialog says why Save is off
In every name dialog of both apps, Save SHALL be off while the trimmed name is empty or taken, and the dialog SHALL show "This name is already in use" while it is taken.

#### Scenario: A taken name on Android
- **WHEN** the user types "pulse" into Rename Sound while a sound "Pulse" exists
- **THEN** Save is off and "This name is already in use" shows under the field; typing a free name hides it and turns Save on
- **TEST** `meditate-core/src/naming.rs::a_taken_name_turns_save_off_and_says_why`

#### Scenario: A taken name on Linux
- **WHEN** the user types a used name into any Linux name dialog
- **THEN** Save is off and "This name is already in use" shows under the field
- **TEST** `meditate-gtk/src/name_dialog.rs::a_taken_name_turns_save_off_and_says_why`

#### Scenario: An empty name
- **WHEN** the field is empty or only spaces
- **THEN** Save is off and no hint shows
- **TEST** `meditate-core/src/naming.rs::a_taken_name_turns_save_off_and_says_why`

#### Scenario: The pattern editor's Name field
- **WHEN** the user types a used pattern name into the pattern editor of either app
- **THEN** Save is off and the same hint shows under the Name field
- **TEST** `meditate-core/src/naming.rs::the_pattern_editor_says_a_taken_name`

#### Scenario: The pattern editor's Name field on Linux
- **WHEN** the user types a used pattern name into the Linux pattern editor
- **THEN** its Save button is off and the hint shows under the Name field
- **TEST** `meditate-gtk/src/vibration_editor.rs::the_pattern_editor_says_a_taken_name`

#### Scenario: Renaming one of two same-named items to its own name
- **WHEN** two bell sounds share the name "Gong" and the user opens Rename on one of them
- **THEN** Save is on and no hint shows; changing only the case ("gong") shows the hint, since the other holds the name
- **TEST** `meditate-core/src/naming.rs::a_taken_name_turns_save_off_and_says_why`

#### Scenario: Every naming behaves alike
- **WHEN** the user creates and renames a preset and a label, renames a guided file and a bell sound, and imports a guided file and a bell sound, in both apps
- **THEN** each dialog trims, greys out Save and shows the hint the same way
- **MANUAL** a person walks the eight namings on Linux and on the phone; the dialog widgets themselves can't be driven from the host on Android

### Requirement: A failed save keeps the name dialog open
When saving a name fails, the dialog SHALL stay open with the typed name, say why, and retry on the next Save.

#### Scenario: The database can't be written (Android)
- **WHEN** Save in a name dialog fails because the database is locked or full
- **THEN** the dialog stays open with the typed name, "Save failed" shows, and Save tries again
- **TEST** `meditate-core/src/naming.rs::a_failed_save_keeps_the_name_dialog_open`

#### Scenario: The name was taken meanwhile (Android)
- **WHEN** a sync adds an item with the typed name while the dialog is open, and the user taps Save
- **THEN** the dialog stays open, Save is off and "This name is already in use" shows
- **TEST** `meditate-core/src/naming.rs::a_failed_save_keeps_the_name_dialog_open`

#### Scenario: Linux
- **WHEN** Save in a Linux name dialog fails, once for a locked database and once for a name taken meanwhile
- **THEN** the dialog stays open with the typed name and shows "Save failed", or the hint
- **TEST** `meditate-gtk/src/name_dialog.rs::a_failed_save_keeps_the_name_dialog_open`
