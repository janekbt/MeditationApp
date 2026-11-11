## ADDED Requirements

### Requirement: A bundled pattern is changed through a copy, on both apps
Each bundled vibration pattern row SHALL offer one Edit action in both apps, which opens the pattern editor on a new pattern copied from it (its curve, length and kind, named "<name> copy"); a bundled pattern itself SHALL NOT be renamed or edited.

#### Scenario: Editing "Pulse" on Linux
- **WHEN** the user taps the pencil on the bundled "Pulse" row in the Linux pattern chooser
- **THEN** the editor opens as "New Pattern" with Pulse's curve and the name "Pulse copy"; Save adds a new pattern and "Pulse" stays as it was; the row has no Rename
- **MANUAL** the GTK chooser row and editor page are walked by a person; core's insert is tested

#### Scenario: Editing "Pulse" on the phone
- **WHEN** the user taps the pencil on the bundled "Pulse" row on the phone
- **THEN** the same happens as on Linux, as it does today
- **MANUAL** the Slint chooser can't be driven from the host
