## ADDED Requirements

### Requirement: A save that changes nothing sends nothing
Saving a preset, interval bell, Box Breath phase, vibration pattern, guided file, bell sound or session with the values it already has SHALL write nothing and send nothing to other devices, so it cannot undo an edit another device made.

#### Scenario: The laptop overrides a preset with the same settings after the phone renamed it
- **WHEN** the phone renames a preset and, before syncing, the laptop saves that preset's unchanged settings over it, then both sync
- **THEN** both show the phone's new name
- **TEST** `meditate-core/src/db/presets.rs::an_unchanged_override_keeps_a_peers_rename`

#### Scenario: Starring a preset that is already starred
- **WHEN** a starred preset is starred again
- **THEN** nothing is sent
- **TEST** `meditate-core/src/db/presets.rs::starring_a_starred_preset_sends_nothing`

#### Scenario: A Box Breath phase written with its own values
- **WHEN** the phone changes the inhale sound and, before syncing, the laptop writes the inhale phase with its old values, then both sync
- **THEN** both keep the phone's sound
- **TEST** `meditate-core/src/db/box_breath_phases.rs::an_unchanged_phase_write_keeps_a_peers_edit`

#### Scenario: An interval bell saved or switched without a change
- **WHEN** an interval bell is saved with the values it has, or switched on while on
- **THEN** nothing is sent
- **TEST** `meditate-core/src/db/interval_bells.rs::an_unchanged_bell_write_sends_nothing`

#### Scenario: A vibration pattern saved without a change
- **WHEN** the phone edits a pattern and, before syncing, the laptop saves its old version without changing it, then both sync
- **THEN** both show the phone's version
- **TEST** `meditate-core/src/db/vibration_patterns.rs::an_unchanged_pattern_save_keeps_a_peers_edit`

#### Scenario: A guided file starred again
- **WHEN** a starred guided file is starred again
- **THEN** nothing is sent
- **TEST** `meditate-core/src/db/guided_files.rs::starring_a_starred_guided_file_sends_nothing`

#### Scenario: A session edited without a change
- **WHEN** a Log entry is saved with the values it has
- **THEN** nothing is sent
- **TEST** `meditate-core/src/db/sessions.rs::an_unchanged_session_edit_sends_nothing`

### Requirement: What one device saves, every device stores the same
Every item saved on one device SHALL be stored on another device, after a sync, with exactly the same values.

#### Scenario: Each kind of item read on a fresh device
- **WHEN** one device creates, renames, stars and edits a preset, an interval bell, a Box Breath phase, a vibration pattern, a guided file, a bell sound and a session, and a fresh device pulls the events
- **THEN** every row on the fresh device equals the row on the first one
- **TEST** `meditate-core/src/db/events.rs::every_local_write_is_stored_as_a_peer_stores_it`

### Requirement: A name conflict is settled once
A "(conflict-...)" name whose holder still holds the name and wins the pair SHALL stay as it is on later syncs, without being written again.

#### Scenario: Two labels named "Work" sync again
- **WHEN** two devices once created "Work", synced, and the conflict is checked again on the next sync
- **THEN** the suffixed label is not written again
- **TEST** `meditate-core/src/db/events.rs::a_permanent_name_conflict_is_not_settled_again`

#### Scenario: The holder is renamed later
- **WHEN** the label holding "Work" is renamed to "Office" and the devices sync
- **THEN** the suffixed label becomes "Work"
- **TEST** `meditate-core/src/db/labels.rs::a_name_freed_in_the_same_pull_goes_to_the_new_row`

#### Scenario: A suffixed preset is starred, then its rival is renamed
- **WHEN** a preset shows as "Calm (conflict-...)" on this device, is starred here, and then the device holding "Calm" renames its preset to "Quiet" and both sync
- **THEN** the starred preset is "Calm" on both devices
- **TEST** `meditate-core/src/db/presets.rs::a_suffixed_preset_starred_here_still_heals`

#### Scenario: A preset renamed to a name another preset holds
- **WHEN** a preset is renamed to a name another one holds
- **THEN** the rename is refused with the duplicate-name message and the name stays
- **TEST** `meditate-core/src/db/presets.rs::update_preset_name_duplicate_returns_error_and_rolls_back`

#### Scenario: A guided file renamed to a name another file holds
- **WHEN** a guided file is renamed to another file's name in other letter case
- **THEN** the rename is refused with the duplicate-name message and nothing is sent
- **TEST** `meditate-core/src/db/guided_files.rs::renaming_a_guided_file_to_a_taken_name_sends_nothing`

#### Scenario: A pattern renamed to a name another pattern holds
- **WHEN** a vibration pattern is renamed to another pattern's name in other letter case
- **THEN** the rename is refused with the duplicate-name message and nothing is sent
- **TEST** `meditate-core/src/db/vibration_patterns.rs::renaming_a_pattern_to_a_taken_name_sends_nothing`

### Requirement: A peer's lengths are clamped
A length or bell time from another device that is negative, too large or not a whole number SHALL be stored as the nearest value a local save would store, for sessions, guided files, vibration patterns and interval bells.

#### Scenario: A damaged session length arrives
- **WHEN** a peer sends a session of 4294967896 seconds, one of 600.0 seconds and one of -5 seconds
- **THEN** they are stored as 4294967295, 600 and 0 seconds
- **TEST** `meditate-core/src/db/sessions.rs::a_peers_damaged_length_is_clamped`

#### Scenario: A damaged guided file length arrives
- **WHEN** a peer sends guided files of 4294967896 and 600.0 seconds
- **THEN** they are stored as 4294967295 and 600 seconds
- **TEST** `meditate-core/src/db/guided_files.rs::a_peers_damaged_file_length_is_clamped`

#### Scenario: A damaged pattern length arrives
- **WHEN** a peer sends a vibration pattern of 1200.0 ms
- **THEN** it is stored as 1200 ms
- **TEST** `meditate-core/src/db/vibration_patterns.rs::a_peers_damaged_pattern_length_is_clamped`

#### Scenario: Damaged bell times arrive
- **WHEN** a peer sends an interval bell of 90.0 seconds with a step of 4294967896 seconds
- **THEN** it rings every 90 seconds, its step stored as 4294967295
- **TEST** `meditate-core/src/db/interval_bells.rs::a_peers_damaged_bell_times_are_clamped`
