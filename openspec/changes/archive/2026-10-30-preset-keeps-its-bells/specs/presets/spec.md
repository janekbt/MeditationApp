## ADDED Requirements
### Requirement: Editing a bell a preset uses leaves the preset's bell alone
Editing an interval bell that a saved preset uses SHALL put the edit into a new bell at the same place in the list, on or off as the old one was, and switch the old bell off unchanged; a bell no preset uses SHALL be changed in place.

#### Scenario: The bell was edited after the preset was saved
- **WHEN** a preset ringing a 5 min bell is saved, the bell is changed in Setup to 7 min with another sound and volume, and the preset is applied
- **THEN** the list holds the 5 min bell with its saved sound and volume, switched on, and the 7 min bell, switched off, and no other bell
- **TEST** `meditate-core/src/preset_config.rs::editing_a_preset_bell_makes_a_new_bell`

#### Scenario: A save that changes nothing
- **WHEN** a bell a preset rings is saved with every setting as it was
- **THEN** no bell is added
- **TEST** `meditate-core/src/preset_config.rs::a_save_without_changes_copies_nothing`

#### Scenario: Bells a preset keeps off, and Box Breath presets
- **WHEN** a bell is edited that the saved Timer presets keep switched off, or that only a Box Breath preset carries
- **THEN** it changes in place
- **TEST** `meditate-core/src/preset_config.rs::only_bells_a_preset_rings_are_copied`

#### Scenario: Further edits on the same page
- **WHEN** the new 7 min bell is edited again to 9 min
- **THEN** it changes in place: the list holds the 5 min bell and the 9 min bell
- **TEST** `meditate-core/src/preset_config.rs::the_new_bell_takes_further_edits_in_place`

#### Scenario: A bell no preset uses
- **WHEN** a bell that no saved preset uses is edited
- **THEN** it changes in place and no bell is added
- **TEST** `meditate-core/src/preset_config.rs::a_bell_no_preset_uses_is_edited_in_place`

#### Scenario: The new bell's place in the list
- **WHEN** of three bells the middle one, used by a preset, is edited
- **THEN** the new bell sits right next to the old one, before the third
- **TEST** `meditate-core/src/preset_config.rs::the_new_bell_sits_next_to_the_old_one`

#### Scenario: A preset saved before bells carried their id
- **WHEN** such a preset rings a 5 min bell and that bell is edited
- **THEN** the 5 min bell stays for it, as for a newer preset
- **TEST** `meditate-core/src/preset_config.rs::an_old_preset_keeps_its_bell_too`

#### Scenario: Another device gets the copy
- **WHEN** a bell a preset rings is edited and the other device syncs
- **THEN** it shows the same bells, on and off alike, the copy at the same place
- **TEST** `meditate-core/src/preset_config.rs::another_device_gets_the_copy_as_it_is`

#### Scenario: Two devices edit the same bell before they sync
- **WHEN** the phone changes a preset's 5 min bell to 7 min and the laptop, offline, changes it to 9 min, then they sync
- **THEN** both show the 5 min bell off and one copy, switched on, with one of the two edits, chosen by the rule that settles any edit made on two devices
- **TEST** `meditate-core/src/preset_config.rs::two_devices_editing_the_same_bell_keep_one_copy`

#### Scenario: The same bell copied a second time
- **WHEN** the preset's old bell is switched on again and edited once more
- **THEN** that edit becomes a further bell, the first copy stays
- **TEST** `meditate-core/src/preset_config.rs::a_second_copy_of_the_same_bell_is_a_bell_of_its_own`

#### Scenario: A preset whose bell was deleted and added back
- **WHEN** a preset's bell was deleted, applying the preset added it back, and that bell is edited
- **THEN** the edit becomes a new bell, as for the bell the preset was saved with
- **TEST** `meditate-core/src/preset_config.rs::a_preset_whose_bell_was_re_made_keeps_it_too`

#### Scenario: A deleted copy and a later edit
- **WHEN** a copy is saved in preset B and then deleted, and the old bell is edited again
- **THEN** that edit becomes a new bell, and applying B adds B's bell back as it was saved
- **TEST** `meditate-core/src/preset_config.rs::a_deleted_copy_is_not_brought_back_by_the_next_edit`

#### Scenario: Editing in both apps
- **WHEN** on Linux (fields save as they change) and on Android (Save button) a bell of a saved preset is edited, several fields one after another, and the preset is applied afterwards
- **THEN** once, at the first change, "Saved as a new bell, your presets keep the old one" shows in full, on Linux wrapped onto a second line in a narrow window; the list shows the old bell off and one new bell with every change; on Linux Delete on that page removes the new bell; the preset rings its old bell again
- **MANUAL** the edit pages are UI glue; checked on the laptop and the FP5
