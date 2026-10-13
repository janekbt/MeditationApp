## ADDED Requirements

### Requirement: The Duration row shows the length of the mode on screen
On Linux, the Setup page's Duration row SHALL show the session length of the mode that is selected, whenever the page is shown or reloaded.

#### Scenario: Back from Stats in Box Breath with a different Timer length
- **WHEN** Box Breath is set to 5 minutes and Timer to 10 minutes, Box Breath is selected, and the user opens Stats and comes back
- **THEN** the Duration row reads 0:05
- **TEST** `meditate-gtk/src/timer/mod.rs::the_duration_row_always_shows_the_selected_modes_length`

#### Scenario: Timer selected when the settings load
- **WHEN** Timer is selected with 10 minutes and Box Breath is set to 5 minutes, and the Setup page reloads its settings
- **THEN** the Duration row reads 0:10
- **TEST** `meditate-gtk/src/timer/mod.rs::the_duration_row_always_shows_the_selected_modes_length`
