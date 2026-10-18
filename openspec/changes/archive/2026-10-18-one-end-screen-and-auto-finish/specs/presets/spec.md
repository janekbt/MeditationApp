## ADDED Requirements

### Requirement: A preset carries Auto-Finish
A preset SHALL save the mode's Auto-Finish switch and set it again when applied; a preset saved before Auto-Finish existed SHALL apply it as off.

#### Scenario: Save and apply
- **WHEN** a Timer preset is saved with Auto-Finish on, the switch is turned off, and the preset is applied
- **THEN** Auto-Finish is on again
- **TEST** `meditate-core/src/preset_config.rs::a_preset_keeps_auto_finish`

#### Scenario: A preset from an older version
- **WHEN** a preset whose stored setup has no Auto-Finish value (written by 26.10.1, or synced from it) is applied
- **THEN** it applies normally and Auto-Finish is off
- **TEST** `meditate-core/src/preset_config.rs::a_preset_from_before_auto_finish_applies_it_off`

#### Scenario: Hidden for a stopwatch, ignored for the active preset
- **WHEN** the Timer is set to stopwatch and a preset differs from Setup only in Auto-Finish
- **THEN** that preset still shows as active
- **TEST** `meditate-core/src/preset_config.rs::a_hidden_auto_finish_does_not_change_the_active_preset`
