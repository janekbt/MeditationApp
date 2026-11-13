## ADDED Requirements

### Requirement: The ring's unfilled track is one even colour
The ring's faint track SHALL look the same all the way round, also where a hollow bead's outline crosses it.

#### Scenario: A hollow bead does not brighten the track beside it
- **WHEN** the Linux app draws a ring with a hollow bead at a quarter of the circle on the black page
- **THEN** the track just beside the bead, where the bead's outline crosses it, has the same colour as the track far from any bead
- **TEST** `meditate-gtk/src/window/imp.rs::the_track_beside_a_hollow_bead_is_not_brighter`

#### Scenario: The phone's track is even
- **WHEN** a session with a bell every 5 minutes runs on the Fairphone 5
- **THEN** the unfilled ring shows no brighter spots beside the hollow beads, the beads' outlines have the track's shade, and their inside is black like the page
- **TEST** MANUAL on the Fairphone 5 (the Slint ring cannot be drawn on the host)

### Requirement: On the phone a riding bead glides with the fill
On Android a random bell's hollow bead riding the front of the fill SHALL move as smoothly as the fill's arc, not jump on each tick.

#### Scenario: The riding bead keeps pace with the arc
- **WHEN** a session with a bell every 5 seconds, spread on, runs on the Fairphone 5 and a bead rides the fill
- **THEN** the bead moves as smoothly as the arc's front and stays on it
- **TEST** MANUAL on the Fairphone 5 (the Slint ring cannot be drawn on the host)
