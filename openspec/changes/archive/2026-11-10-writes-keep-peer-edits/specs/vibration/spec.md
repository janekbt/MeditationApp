## ADDED Requirements

### Requirement: Undo after a pattern Save appears only for a change
After Save in the Linux vibration pattern editor, the app SHALL offer Undo only when the pattern changed.

#### Scenario: Saving a pattern without a change
- **WHEN** a pattern is saved with its own values, its points off by less than a millionth
- **THEN** the save reports no change and nothing is sent
- **TEST** `meditate-core/src/db/vibration_patterns.rs::saving_an_unchanged_pattern_reports_no_change`

#### Scenario: On the laptop, with and without a change
- **WHEN** the maintainer opens a pattern on the laptop, saves it once unchanged and once with one point moved
- **THEN** the Undo toast shows only after the moved point
- **MANUAL** the toast only shows in the running app
