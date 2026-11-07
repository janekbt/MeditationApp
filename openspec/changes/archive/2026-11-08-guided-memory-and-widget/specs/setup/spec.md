## ADDED Requirements

### Requirement: The app opens in the mode used last
Each device SHALL remember the mode that was open last and open in it at the next start, unless a widget tap sets the mode; this memory SHALL stay on the device.

#### Scenario: The app is started again
- **WHEN** Box Breath is open and the app is closed and started again
- **THEN** Box Breath is open
- **TEST** `meditate-core/src/settings_keys.rs::the_last_mode_comes_back_after_a_restart`

#### Scenario: Another device switches mode
- **WHEN** the laptop switches to Guided and the devices sync
- **THEN** the mode the phone opens in is unchanged
- **TEST** `meditate-core/src/settings_keys.rs::the_last_mode_stays_on_its_device`

#### Scenario: Nothing remembered yet
- **WHEN** the app starts for the first time after the update
- **THEN** Timer is open, as before
- **TEST** `meditate-core/src/settings_keys.rs::the_app_opens_in_timer_with_nothing_remembered`

#### Scenario: A widget tap at the start
- **WHEN** Guided was open last and the app starts from a widget tap on a Timer preset
- **THEN** Timer is open and the preset's session runs
- **MANUAL** on the phone: leave Guided open, end the app, tap a Timer preset in the widget; the deep link and restore run only on the device
