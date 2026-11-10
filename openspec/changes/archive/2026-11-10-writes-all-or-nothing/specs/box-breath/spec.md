## ADDED Requirements

### Requirement: A Box Breath pattern is saved whole or not at all
Saving a Box Breath pattern SHALL store its four phase lengths and the session length together or none of them.

#### Scenario: The fourth value fails to save
- **WHEN** a new pattern is saved and storing the hold-out length fails
- **THEN** the old pattern and length are all still stored and no change goes to other devices
- **TEST** `meditate-core/src/settings_keys.rs::a_breath_pattern_that_fails_halfway_stores_nothing`

### Requirement: Changing one thing on a phase keeps the rest of it
Changing a Box Breath phase's switch, Type, sound or pattern SHALL change only that, keeping the phase's other values as stored at that moment.

#### Scenario: The volume was dragged just before the sound is picked
- **WHEN** Inhale's volume is changed and then Inhale's sound is picked from a chooser opened before
- **THEN** Inhale has the new sound and the new volume
- **TEST** `meditate-core/src/db/box_breath_phases.rs::editing_one_field_keeps_a_newer_volume`

#### Scenario: Picking a sound on the laptop right after a volume drag
- **WHEN** on the laptop the user drags Inhale's volume and opens Inhale's sound within half a second and picks one
- **THEN** reopening the phase shows the dragged volume and the picked sound
- **MANUAL** the 0.4 s save delay and the chooser need the running app
