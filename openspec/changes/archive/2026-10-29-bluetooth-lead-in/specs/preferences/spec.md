## ADDED Requirements
### Requirement: Preferences hold the Bluetooth lead-in
Preferences SHALL have a "Bluetooth" group, on Android right after "Bells" and on Linux right after "Statistics", with the explanation under its heading, the switch "Wake-up Lead-in for Bluetooth" (off until set), "Length (Seconds)" from 0.10 to 5.00 in 0.01 steps (0.10 until set, shown only while the switch is on) and a "Test" row with a "Play" button; both values sync.

#### Scenario: The values until set and after
- **WHEN** neither key was written; the switch is set on; the length is set to 1.234 s, 1.235 s, 0.05 s, 7 s; the stored length is text; only the first attempt's `bluetooth_lead_in` is "true"
- **THEN** off and 100 ms; on; 1230 ms, 1240 ms, 100 ms, 5000 ms; 100 ms; still off
- **TEST** `meditate-core/src/settings_keys.rs::the_lead_in_is_off_and_a_tenth_of_a_second_until_set`

#### Scenario: The group on Android
- **WHEN** Preferences is opened
- **THEN** the Bluetooth group follows Bells and holds the explanation, the switch, the length stepper with two decimals revealed by the switch, and Test; each control saves through the synced settings and shows the stored value again after a failed save
- **TEST** `meditate-android/src/app.rs::the_bluetooth_group_holds_the_lead_in`; **MANUAL** at 360 px in German, "5,00" fits

#### Scenario: The group on Linux
- **WHEN** Preferences is opened on Linux
- **THEN** a Bluetooth group follows Statistics with the explanation as its description, a switch row, a spin row 0.10 to 5.00 with two digits, insensitive while the switch is off, and a Test row with a Play button
- **TEST** `meditate-gtk/src/preferences.rs::the_bluetooth_group_holds_the_lead_in`
