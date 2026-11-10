## MODIFIED Requirements
### Requirement: A setting that could not be saved springs back
When a Timer page setting fails to save, the app SHALL show "Save failed" and show the saved value again on that page.

#### Scenario: Laptop, a switch flipped while the database is locked
- **WHEN** on the laptop a Timer page switch, pick, length or volume is changed and saving it fails
- **THEN** "Save failed" shows and the page shows the value that is saved
- **TEST** `meditate-gtk/src/timer/imp.rs::a_failed_setting_says_so_and_springs_back`

#### Scenario: Laptop, no Timer page write drops its result
- **WHEN** any Timer page setting is written on the laptop
- **THEN** its result reaches the failure message, none is thrown away or only logged
- **TEST** `meditate-gtk/src/db/mod.rs::no_write_result_is_dropped`

#### Scenario: Phone, a setting changed while the database is locked
- **WHEN** on the phone a Timer page switch, pick, length, volume, a preset star or the accent colour is changed and saving it fails
- **THEN** "Save failed" shows and the page shows the value that is saved
- **TEST** `meditate-android/src/app.rs::a_failed_setting_says_so_and_springs_back`

#### Scenario: Laptop, a preset that fails halfway saves nothing of it
- **WHEN** applying a preset, or its Undo, fails partway
- **THEN** "Save failed" shows, no Undo, nothing of the preset is saved, and nothing is written a second time
- **TEST** `meditate-gtk/src/timer/imp.rs::a_failed_preset_apply_is_not_written_twice`

#### Scenario: Phone, a preset that fails halfway saves nothing of it
- **WHEN** on the phone applying a preset, or its Undo, fails partway
- **THEN** "Save failed" shows, no Undo, nothing of the preset is saved, and nothing is written a second time
- **TEST** `meditate-android/src/app.rs::a_failed_preset_apply_is_not_written_twice`

#### Scenario: A real failure on each device
- **WHEN** a sync lock is held on the database (BEGIN EXCLUSIVE from sqlite3) and a bell switch is flipped
- **THEN** after about 8 s "Save failed" shows and the switch flips back
- **MANUAL** needs a locked database on a real install; neither app's UI runs on the test host
