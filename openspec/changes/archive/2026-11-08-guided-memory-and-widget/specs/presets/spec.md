## RENAMED Requirements
- FROM: `### Requirement: Only the widget starts a preset from outside the app`
- TO: `### Requirement: Only the widget starts a session from outside the app`

## MODIFIED Requirements

### Requirement: Only the widget starts a session from outside the app
On the phone, a preset or a guided file SHALL start from outside the app only through a tap on the home-screen widget, never through a command another app sends.

#### Scenario: Another app sends the widget's start command
- **WHEN** another app sends the widget's start command with a preset's or a file's id
- **THEN** no session starts, now or at the next launch
- **TEST** `meditate-android/src/app.rs::only_the_widget_can_start_a_preset`

#### Scenario: Tapping a starred preset in the widget
- **WHEN** the user taps a preset in the home-screen widget
- **THEN** the app opens and that preset's session starts
- **MANUAL** tap a widget row on the phone; the widget host can't run on the laptop
