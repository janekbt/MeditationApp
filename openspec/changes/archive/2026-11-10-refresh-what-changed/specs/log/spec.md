# Log (delta)

## ADDED Requirements

### Requirement: Linux, undoing a recovered session says when it fails
When the user undoes a recovered session on Linux, the app SHALL remove it from the Log, Stats and the streak at once, or show "Delete failed" and keep it.

#### Scenario: Undo of a recovered session with sync off
- **WHEN** the app starts after a crash, recovers a running session, and the user taps Undo with sync not set up
- **THEN** the session leaves the Log, Stats and the streak at once
- **MANUAL** on the laptop: kill the app during a session, restart, Undo, look at the Log (GTK views can't run in host tests)

#### Scenario: Undo of a recovered session when the write fails
- **WHEN** the user taps Undo on a recovered session and the delete fails
- **THEN** "Delete failed" shows and the session stays
- **MANUAL** on the laptop: as above with the database made read-only before Undo (GTK views can't run in host tests)
