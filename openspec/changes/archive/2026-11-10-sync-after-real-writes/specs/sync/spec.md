# Sync (delta)

## ADDED Requirements

### Requirement: A sync starts only after a write that changed something
Each app SHALL start a sync after a local write that recorded a change for other devices, and SHALL NOT start one after a write that changed nothing.

#### Scenario: A setting saved with the value it already has
- **WHEN** a setting is written with its current value
- **THEN** the write reports no change, so no sync starts and no screen is marked
- **TEST** `meditate-core/src/db/events.rs::a_write_that_changes_nothing_reports_nothing`

#### Scenario: A label renamed
- **WHEN** a label is renamed
- **THEN** the write reports the Log, Stats, Setup and the label lists, so a sync starts and those screens re-read when shown
- **TEST** `meditate-core/src/db/events.rs::a_committed_write_reports_the_screens_it_touches`

#### Scenario: A write that fails
- **WHEN** a write fails and is undone
- **THEN** it reports no change
- **TEST** `meditate-core/src/db/events.rs::an_undone_write_reports_nothing`

#### Scenario: Linux, idle
- **WHEN** the Linux app is open and nothing is written
- **THEN** no timer wakes it to check for changes, and a slider or switch change in Setup updates the active preset mark once
- **MANUAL** on the laptop: change the Timer length to a starred preset's length, its row shows as active; change it back, the mark goes (GTK views can't run in host tests)

### Requirement: Linux, Sync Now says when it can't prepare
When the user taps Sync Now and clearing the refused uploads fails, the Linux app SHALL show "Save failed" and still start the sync.

#### Scenario: The database is locked
- **WHEN** Sync Now is tapped and the write fails
- **THEN** "Save failed" shows
- **MANUAL** on the laptop: hard to force; checked in review that the result goes through `write_ok` (GTK views can't run in host tests)
