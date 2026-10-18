## ADDED Requirements
### Requirement: A guided track plays only with the audio to itself
On the phone, a guided track SHALL start only when Android grants it the audio, and SHALL pause with its session when the headphones disconnect.

#### Scenario: Starting a guided session during a call
- **WHEN** the user starts a guided session while Android refuses the audio, as during a call
- **THEN** "Couldn't start playback" shows and no session starts
- **TEST** `meditate-android/src/app.rs::a_refused_audio_focus_starts_no_track`

#### Scenario: Bluetooth earbuds disconnect during a guided session
- **WHEN** the earbuds disconnect while a guided session plays
- **THEN** the session and its track pause instead of playing on the loudspeaker
- **TEST** `meditate-android/src/app.rs::disconnected_headphones_pause_the_track`

### Requirement: A guided preview that can't play says so
On the phone, Play in Manage Files SHALL show "Couldn't start playback" when the file does not play.

#### Scenario: Play on a file whose audio hasn't arrived yet
- **WHEN** the user taps Play on a synced file whose audio is not on the phone yet
- **THEN** "Couldn't start playback" shows and no row's button shows Stop, also when another row was playing
- **TEST** `meditate-android/src/app.rs::a_guided_preview_that_fails_says_so`
