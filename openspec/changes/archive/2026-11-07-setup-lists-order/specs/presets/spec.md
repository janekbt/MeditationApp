## ADDED Requirements

### Requirement: The starred presets can be moved on the Setup page
Moving a row in the Setup page's Presets list SHALL be the same move as on the Manage Presets page, so the new order shows in every list of that mode and on every synced device.

#### Scenario: Moving among the starred presets past an unstarred one
- **WHEN** the mode's presets are A (starred), u (not starred), B (starred) and B is moved to the top of the Setup list
- **THEN** the Setup list reads B, A and the Manage list B, A, u
- **TEST** `meditate-core/src/db/presets.rs::a_starred_preset_moved_on_setup_takes_its_place`

#### Scenario: Moving down on the Setup page
- **WHEN** the starred presets are A, B, C, with u unstarred between A and B, and A is moved to the last place
- **THEN** the Setup list reads B, C, A
- **TEST** `meditate-core/src/db/presets.rs::a_starred_preset_moved_down_on_setup_lands_last`

#### Scenario: Past the end of the Setup list
- **WHEN** the starred presets are A, B, with u unstarred between them, and B is moved one place further down
- **THEN** nothing changes and nothing is written
- **TEST** `meditate-core/src/db/presets.rs::a_starred_preset_moved_on_setup_takes_its_place`

#### Scenario: Dragging on the Setup page, Linux and the phone
- **WHEN** a Presets row on the Timer or Box Breath Setup is dragged by its handle and dropped on another row
- **THEN** it stands at that row's place; the Manage page and the Android widget follow; a swipe elsewhere on a row scrolls
- **MANUAL** drag needs a pointer or a finger; on Linux and the FP5

#### Scenario: Without dragging on the Setup page, Linux
- **WHEN** on Linux a Setup Presets row has the keyboard focus and Alt+Up or Alt+Down is pressed
- **THEN** the preset moves one place up or down and keeps the focus
- **MANUAL** keyboard input on the GTK page; on Linux
