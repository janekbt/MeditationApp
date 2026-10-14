## ADDED Requirements

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

#### Scenario: Only the label for next time fails at Save
- **WHEN** a session is saved and only remembering its label as next time's default fails
- **THEN** the session is saved and "Label not kept for next time" shows, not "Save failed"
- **TEST** `meditate-gtk/src/timer/imp.rs::a_label_default_that_was_not_kept_says_so`

#### Scenario: Only the label for next time fails at Save on the phone
- **WHEN** on the phone a session is saved and only remembering its label as next time's default fails
- **THEN** the session is saved and "Label not kept for next time" shows
- **TEST** `meditate-android/src/app.rs::a_label_default_that_was_not_kept_says_so`

#### Scenario: Laptop, a preset that fails halfway is put back
- **WHEN** applying a preset, or its Undo, fails after some of its values were saved
- **THEN** "Save failed" shows, no Undo, and the setup from before is saved again
- **TEST** `meditate-gtk/src/timer/imp.rs::a_preset_that_fails_halfway_is_put_back`

#### Scenario: Phone, a preset that fails halfway is put back
- **WHEN** on the phone applying a preset, or its Undo, fails after some of its values were saved
- **THEN** "Save failed" shows, no Undo, and the setup from before is saved again
- **TEST** `meditate-android/src/app.rs::a_preset_that_fails_halfway_is_put_back`

#### Scenario: A real failure on each device
- **WHEN** a sync lock is held on the database (BEGIN EXCLUSIVE from sqlite3) and a bell switch is flipped
- **THEN** after about 8 s "Save failed" shows and the switch flips back
- **MANUAL** needs a locked database on a real install; neither app's UI runs on the test host
