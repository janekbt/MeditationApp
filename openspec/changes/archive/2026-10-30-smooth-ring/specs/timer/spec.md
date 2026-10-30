## ADDED Requirements
### Requirement: The ring moves smoothly
The session ring and the preparation ring SHALL move in a steady motion instead of jumping with each tick: on Android they glide between the 200 ms ticks like Box Breath's dot, on Linux they are redrawn every frame.

#### Scenario: Both rings glide between ticks
- **WHEN** a Timer countdown with preparation runs on the phone
- **THEN** the small preparation ring and then the big ring fill in a steady motion, not in steps five times a second
- **TEST** `meditate-android/src/app.rs::the_ring_glides_between_ticks`

#### Scenario: The ring follows every frame on the laptop
- **WHEN** a Timer countdown runs on the laptop
- **THEN** the ring is redrawn from the session on every frame, not every quarter second
- **TEST** `meditate-gtk/src/window/imp.rs::the_ring_follows_every_frame`

#### Scenario: How it looks
- **WHEN** a 2 minute Timer with 10 seconds of preparation runs on the FP5 and on the laptop
- **THEN** both rings move as smoothly as the Box Breath dot, and pause, resume and the end still look right
- **MANUAL** motion on a real screen; a person checks the FP5 (release-type build) and the laptop
