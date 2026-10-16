## ADDED Requirements

### Requirement: Row texts never push a control off screen
On the phone, a row's title and subtitle SHALL wrap so the switch or button beside them stays on screen at 360 dp in every language.

#### Scenario: Keep Screen Awake in German at 360 dp
- **WHEN** Setup is shown in German, French or Russian on a 360 dp wide screen
- **THEN** the Keep Screen Awake and Cues during phases switches are fully visible and their texts show in full
- **MANUAL** layout on a phone

#### Scenario: A preset with a long subtitle
- **WHEN** a preset whose subtitle is longer than one line is listed in the preset chooser
- **THEN** its delete button is fully visible and the subtitle shows in full
- **MANUAL** layout on a phone
