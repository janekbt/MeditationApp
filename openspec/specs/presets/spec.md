# Presets

## Purpose
Presets store a mode's whole setup under a name, so the user can apply
it with one tap; they can be starred, renamed, overridden, deleted and
restored with Undo, and they sync between devices.

## Requirements

### Requirement: A preset write that fails says so
A failed preset write SHALL show "Save failed" or "Delete failed" in place of any success message or Undo.

#### Scenario: Override while a sync holds the database (laptop)
- **WHEN** the user overrides a preset and the write fails
- **THEN** "Save failed" shows, no "overridden" message, and the preset keeps its old setup
- **TEST** `meditate-gtk/src/db/mod.rs::no_write_result_is_dropped`

#### Scenario: Delete fails on the phone
- **WHEN** the user deletes a preset and the write fails
- **THEN** "Delete failed" shows without Undo, the dialog closes and the preset stays listed
- **TEST** `meditate-android/src/app.rs::failed_dialog_writes_say_so_and_close`

#### Scenario: Override or rename fails on the phone, then the user tries again
- **WHEN** an override or a rename fails and the user opens the dialog again
- **THEN** "Save failed" showed, and the dialog works on the second try
- **TEST** `meditate-android/src/app.rs::failed_dialog_writes_say_so_and_close`

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

### Requirement: Only the widget starts a preset from outside the app
On the phone, a preset SHALL start from outside the app only through a tap on the home-screen widget, never through a command another app sends.

#### Scenario: Another app sends the widget's start command
- **WHEN** another app sends the widget's start command with a preset's id
- **THEN** no session starts, now or at the next launch
- **TEST** `meditate-android/src/app.rs::only_the_widget_can_start_a_preset`

#### Scenario: Tapping a starred preset in the widget
- **WHEN** the user taps a preset in the home-screen widget
- **THEN** the app opens and that preset's session starts
- **MANUAL** tap a widget row on the phone; the widget host can't run on the laptop
