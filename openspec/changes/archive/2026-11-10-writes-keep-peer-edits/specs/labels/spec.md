## MODIFIED Requirements

### Requirement: A merge leaves nothing on the merged-away label
Merging a label into another SHALL move its sessions, every mode's default label, every preset that uses it and any open selection to the kept label.

#### Scenario: A mode's default label is merged away
- **WHEN** the Timer's default label is merged into another
- **THEN** the Timer's default becomes the kept label, and new sessions save with it
- **TEST** `meditate-core/src/db/labels.rs::a_merge_moves_a_modes_default_label_to_the_kept_one`

#### Scenario: The phone merges while the session editor has it selected
- **WHEN** the phone merges a label that Setup, the session editor or a Log filter is using
- **THEN** they switch to the kept label, and Save works
- **TEST** `meditate-android/src/app.rs::a_merge_cleans_up_like_a_delete`

#### Scenario: A preset uses the merged-away label
- **WHEN** a preset pinned to "Walk (conflict-...)" exists and that label is merged into "Walk", then the preset is applied and a session saved
- **THEN** the session is saved with "Walk", on this device and on a device that syncs after the merge
- **TEST** `meditate-core/src/db/labels.rs::a_merge_moves_presets_to_the_kept_label`
