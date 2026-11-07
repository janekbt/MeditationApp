## ADDED Requirements
### Requirement: A switched-off label decides the check only as "off"
The check and highlight of a starred preset SHALL treat a switched-off label as off, whichever label was chosen behind the switch.

#### Scenario: A preset saved with the label off after a label was chosen elsewhere
- **WHEN** a preset is saved with the label off and no label chosen, another preset with a label is applied, and then the first preset is applied
- **THEN** the first preset shows its check and highlight
- **TEST** `meditate-core/src/preset_config.rs::a_preset_saved_with_the_label_off_keeps_its_check`
