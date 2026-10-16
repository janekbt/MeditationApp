## ADDED Requirements

### Requirement: Starred Files marks the selected file
The Starred Files list on the Guided page SHALL show a check on the row of the selected file, in both apps, and on no other row.

#### Scenario: A starred file is picked
- **WHEN** a starred library file is tapped, then a starred local file
- **THEN** the check moves to the local file's row; the library file's row has none
- **MANUAL** row layout on the laptop and the phone

#### Scenario: The selected file is not starred
- **WHEN** the selected file is not in Starred Files
- **THEN** no row in Starred Files shows a check
- **MANUAL** row layout on the laptop and the phone
