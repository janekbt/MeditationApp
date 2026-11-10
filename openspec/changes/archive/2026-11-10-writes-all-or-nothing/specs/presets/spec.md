## ADDED Requirements

### Requirement: Applying a preset lands whole or not at all
Applying a preset SHALL write all of it, its length included, or nothing, and a failure SHALL say "Save failed" with the setup from before still in place.

#### Scenario: A write fails halfway through a Timer preset
- **WHEN** a preset with two interval bells is applied and adding the second bell fails
- **THEN** no setting, bell or length of the preset is stored and no change goes to other devices
- **TEST** `meditate-core/src/preset_config.rs::a_preset_that_fails_halfway_stores_nothing`

#### Scenario: The preset's length comes with it
- **WHEN** a 25-minute Timer preset or a 4-7-8-0 Box Breath preset of 10 minutes is applied
- **THEN** the stored length is 25 minutes, or the pattern is 4-7-8-0 with 10 minutes
- **TEST** `meditate-core/src/preset_config.rs::applying_a_preset_stores_its_length`

#### Scenario: A failed apply on the phone is not written again
- **WHEN** applying a preset fails on the phone
- **THEN** "Save failed" shows and the app does not write the old setup again
- **TEST** `meditate-android/src/app.rs::a_failed_preset_apply_is_not_written_twice`

#### Scenario: A failed apply on the laptop is not written again
- **WHEN** applying a preset fails on the laptop
- **THEN** "Save failed" shows and the app does not write the old setup again
- **TEST** `meditate-gtk/src/timer/imp.rs::a_failed_preset_apply_is_not_written_twice`

### Requirement: Saving the current setup never drops what it can't read
Saving the current setup as a preset SHALL fail with "Save failed" when the setup can't be read, instead of storing a preset with parts missing.

#### Scenario: The bell list can't be read during "Save current setup"
- **WHEN** the interval bells or a setting can't be read while the setup is saved
- **THEN** the save fails with an error and no preset is stored
- **TEST** `meditate-core/src/preset_config.rs::a_setup_whose_bells_cannot_be_read_is_not_saved`

### Requirement: A preset with the label switched on but none picked applies the mode's default label
Applying a preset saved with the label switch on and no label picked SHALL set the mode's label back to its default, and a merge of that default into another label SHALL make the preset use the kept label; a preset with the switch off SHALL only switch it off.

#### Scenario: "Walking" is picked when such a preset is applied
- **WHEN** a preset saved with the label switch on and no label picked is applied while "Walking" is picked
- **THEN** the mode's default label is picked and the preset shows as active
- **TEST** `meditate-core/src/preset_config.rs::a_preset_without_a_label_picks_the_mode_default`

#### Scenario: A preset with the label switched off keeps the label picked behind the switch
- **WHEN** a preset saved with the label switch off is applied while "Walking" is picked
- **THEN** the switch is off and "Walking" is still the label behind it
- **TEST** `meditate-core/src/preset_config.rs::a_preset_with_the_label_off_keeps_the_label_behind_it`

#### Scenario: The mode's default label was merged into "Sitting"
- **WHEN** the default label is merged into "Sitting", "Walking" is picked, and a preset saved with the label on and none picked is applied
- **THEN** "Sitting" is picked and the preset shows as active
- **TEST** `meditate-core/src/preset_config.rs::a_preset_without_a_label_keeps_the_label_its_merged_default_went_into`

#### Scenario: A preset from before such a merge
- **WHEN** a preset with the label on and none picked, from before the merge, is applied while the mode is on "Sitting"
- **THEN** "Sitting" stays picked, not an empty label
- **TEST** `meditate-core/src/preset_config.rs::a_preset_from_before_a_merge_keeps_the_kept_label`
