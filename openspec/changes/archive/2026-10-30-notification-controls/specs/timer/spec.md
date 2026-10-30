## ADDED Requirements

### Requirement: The phone's session notification controls the session
On the phone, the running session's media control SHALL offer the session page's buttons plus Discard, act like them, follow the headphones' play/pause button, and show the app's accent colour with the white lotus.

#### Scenario: The buttons follow the session page in every state
- **WHEN** a session is running, preparing, paused, paused in preparation, or in overtime
- **THEN** the media control offers Pause and Discard; Pause and Discard; Resume, Save and Discard; Resume and Discard; Save, Add and Discard
- **TEST** `meditate-android/src/app.rs::the_notification_offers_the_session_buttons`

#### Scenario: A button in the notification does what the app's button does
- **WHEN** the user taps Pause, Resume, Save or Add in the media control
- **THEN** the session behaves as after the same tap in the app: a guided track pauses and resumes with it, a paused phone may sleep, Save and Add save with the note
- **TEST** `meditate-android/src/app.rs::a_notification_tap_goes_through_the_session_buttons`

#### Scenario: Discard from the notification with and without a note
- **WHEN** the user taps Discard in the media control, once with an empty note and once after writing a note
- **THEN** without a note the session is discarded at once; once a note is written, Discard is no longer offered there
- **TEST** `meditate-android/src/app.rs::discard_from_the_notification_spares_a_note`

#### Scenario: The headphones' button pauses and resumes, but not in overtime
- **WHEN** the user presses play/pause on the headphones while running, while paused, and in overtime
- **THEN** the session pauses, resumes, and in overtime nothing changes
- **TEST** `meditate-android/src/app.rs::the_headphone_button_toggles_the_pause`

#### Scenario: The media control shows the accent colour and the lotus
- **WHEN** a session runs with the accent set to System, then to Rose, on the lock screen and in the notification shade
- **THEN** the media control's background follows the accent, the white lotus shows on it sharply with the accent showing between its petals and as the small symbol, and the play or pause symbol matches the session
- **MANUAL** on the FP5: the system draws the media control, so only a phone shows it.
