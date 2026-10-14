## ADDED Requirements

### Requirement: The counter shows the running session's length
On the phone, the Box Breath counter SHALL show the length of the running session, also after Pause, Resume or a sync that changes Setup.

#### Scenario: Setup changes during a 10-minute session
- **WHEN** a 10-minute session runs, Setup changes to 20 minutes through a sync, and the user pauses and resumes
- **THEN** the counter reads ".. / 10:00" and the session ends at 10:00
- **TEST** `meditate-android/src/app.rs::the_box_breath_counter_reads_the_running_session`
