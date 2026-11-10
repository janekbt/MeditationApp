## MODIFIED Requirements
### Requirement: Bell rows show the current sound and pattern names
After a sound or pattern is renamed or deleted, the Interval Bells list, the bell edit page and Setup's bell rows SHALL show its current name, or "Missing" when it is gone.

#### Scenario: A sound is renamed while the Interval Bells list is open (laptop)
- **WHEN** the user opens the sound chooser from a bell, renames the sound and goes back
- **THEN** the bell edit page and the Interval Bells list show the new name
- **TEST** `meditate-gtk/src/bells.rs::bell_pages_reread_names_when_shown`

#### Scenario: A pattern used by Setup is deleted (laptop)
- **WHEN** the user deletes the end bell's pattern in the chooser and goes back to Setup
- **THEN** Setup's end bell pattern row shows "Missing"
- **TEST** `meditate-gtk/src/timer/imp.rs::setup_rereads_bell_rows_when_shown_again`

#### Scenario: A sound is renamed or deleted on the phone
- **WHEN** the user renames or deletes a sound used by an interval bell
- **THEN** the Interval Bells list shows the new name or "Missing"
- **TEST** `meditate-android/src/app.rs::label_and_sound_renames_reach_widget_and_bell_list`

#### Scenario: A chooser stays open across a sync on the phone
- **WHEN** a bell or pattern chooser is open, the app resumes, and the sync pulls another device's delete
- **THEN** the chooser no longer lists the deleted item
- **TEST** `meditate-android/src/app.rs::a_pull_follows_the_guided_file_and_refills_choosers`

#### Scenario: An interval bell's sound is gone on the phone
- **WHEN** the sound an interval bell uses was deleted (here or on another device) and the Interval Bells list is shown
- **THEN** that bell's row shows "Missing" as its sound, not an empty line
- **TEST** `meditate-android/src/app.rs::interval_bell_rows_say_when_a_sound_is_missing`
