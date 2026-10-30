## ADDED Requirements
### Requirement: The "Meditation complete" notification shows the lotus
On the phone, the notification that a session reached its end in the background SHALL show the lotus as its symbol in the status bar, like the running session.

#### Scenario: A session ends while the app is in the background
- **WHEN** a timed session reaches its end while another app is open
- **THEN** the status bar and the notification show the lotus, not a play triangle
- **MANUAL** on the FP5: the system draws the status bar.

#### Scenario: A later edit brings back a system symbol
- **WHEN** the notification's symbol is changed to one of Android's
- **THEN** the build's tests fail
- **TEST** `meditate-android/src/app.rs::the_complete_notification_shows_the_lotus`

### Requirement: The phone shows the streak in an accent pill
On the phone, the streak above the timer SHALL sit in a pill like the laptop's: a light tint of the accent behind bold capitals in the accent colour.

#### Scenario: A streak with Rose and with System
- **WHEN** the Timer page shows a streak, once with the accent Rose and once with System, in light and dark mode
- **THEN** the streak reads in capitals, in the accent colour, on a light accent-tinted pill that hugs the text
- **MANUAL** on the FP5: colours and the pill's shape need a render.

#### Scenario: Before the streak is known
- **WHEN** the app starts and the streak has not been read yet
- **THEN** no empty pill shows above the timer
- **MANUAL** on the FP5: the first frames after start need a device.
