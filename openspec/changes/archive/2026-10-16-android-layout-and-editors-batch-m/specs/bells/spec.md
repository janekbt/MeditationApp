## ADDED Requirements

### Requirement: A volume drag the page takes over changes nothing
When the page takes over a drag that started on a volume slider, the slider SHALL go back to the value it had before the touch, which is the saved one.

#### Scenario: A vertical swipe that starts on the slider
- **WHEN** a bell is at 80%, a finger lands on its slider at 30% and the page then scrolls
- **THEN** the slider shows 80% again and the bell rings at 80%
- **TEST** `meditate-android/src/app.rs::a_cancelled_volume_drag_puts_the_slider_back`

#### Scenario: The same swipe on the FP5
- **WHEN** on the FP5 a vertical swipe starts on a bell's volume slider
- **THEN** after the scroll the slider shows the value it had before
- **MANUAL** touch handover between slider and page only happens on the phone

### Requirement: The interval-bell editor reaches every row
The phone's interval-bell editor SHALL scroll its body, so every row is reachable in landscape and split screen.

#### Scenario: The editor in landscape
- **WHEN** the interval-bell editor is opened with the phone in landscape
- **THEN** every row can be scrolled to and used, and the volume slider still sets its value
- **MANUAL** layout on a phone

### Requirement: Sound chooser rows leave room for the name
Each row of the phone's sound chooser SHALL show its controls as icon buttons in the laptop's order (check, play, edit, delete), so a long name in any language shows in full.

#### Scenario: A long imported sound name at 360 dp
- **WHEN** an imported sound with a long name is listed in Russian at 360 dp width
- **THEN** the full name shows, wrapped if needed, next to the play, edit and delete icons
- **MANUAL** layout on a phone
