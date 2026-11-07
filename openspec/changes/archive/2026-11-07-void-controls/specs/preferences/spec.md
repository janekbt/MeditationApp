## ADDED Requirements
### Requirement: Preferences hold the Void-Controls on Android
Android Preferences SHALL have an "Experimental" group right before About, with the explanation under its heading and the switch "Void-Controls", off until set; the value syncs.

#### Scenario: The value until set and after
- **WHEN** the key was never written; then the switch is set on
- **THEN** off; on
- **TEST** `meditate-core/src/settings_keys.rs::void_controls_are_off_until_set`

#### Scenario: The group on Android
- **WHEN** Preferences is opened
- **THEN** the Experimental group comes right before About and holds the explanation and the switch; the switch saves through the synced settings and shows the stored value again after a failed save
- **TEST** `meditate-android/src/app.rs::the_experimental_group_holds_the_void_controls`; **MANUAL** at 360 px in German the explanation shows in full
