## ADDED Requirements
### Requirement: Vibration bells get through Battery Saver
On the phone, a vibration bell SHALL be sent as an alarm vibration on every Android version whose Battery Saver lets alarm vibrations through (9 and newer).

#### Scenario: A Vibration end bell on Android 9 to 12 under Battery Saver
- **WHEN** a session with a Vibration end bell ends on Android 9 to 12 with Battery Saver on
- **THEN** the phone vibrates
- **TEST** `meditate-android/src/app.rs::vibrations_are_alarms_on_every_android`
