# Labels (delta)

## MODIFIED Requirements

### Requirement: Label changes show everywhere at once
Creating, renaming or deleting a label SHALL update the Log, its filters and Stats right away, without waiting for a sync.

#### Scenario: Rename on the desktop without sync set up
- **WHEN** the user renames "Yoga" to "Zen" on the desktop with no sync configured
- **THEN** the Log chips, the Log filter and Stats show "Zen" the next time they are shown
- **MANUAL** on the laptop with sync off: rename a label in the chooser, open the Log and Stats (GTK views can't run in host tests; the text test that stood here only counted calls in the source)
