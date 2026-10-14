## ADDED Requirements

### Requirement: A label delete that fails says so
A failed label delete on the laptop SHALL show "Delete failed" and leave the label in place.

#### Scenario: Delete during a sync
- **WHEN** the user confirms deleting a label and the write fails
- **THEN** "Delete failed" shows and the label is still listed
- **TEST** `meditate-gtk/src/db/mod.rs::no_write_result_is_dropped`

### Requirement: A renamed label shows its new name on the phone's chips and widget
After a label is renamed, merged or deleted on the phone, the preset chips and the home-screen widget SHALL show the current label names.

#### Scenario: "Yoga" renamed to "Zen"
- **WHEN** a preset uses label "Yoga" and the user renames the label to "Zen"
- **THEN** the preset chip and the widget subtitle read "Zen"
- **TEST** `meditate-android/src/app.rs::label_and_sound_renames_reach_widget_and_bell_list`
