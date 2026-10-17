## ADDED Requirements

### Requirement: Interval bell times are set in seconds or minutes
Every interval bell's time SHALL be settable in seconds or minutes, from 5 s to 120 min, and SHALL ring at exactly that time.

#### Scenario: A bell 90 seconds from start
- **WHEN** a countdown has a bell "At 90 s"
- **THEN** it rings at 1:30
- **TEST** `meditate-core/src/bells.rs::a_bell_set_in_seconds_rings_at_that_second`

#### Scenario: A bell 30 seconds before end
- **WHEN** a 10-minute countdown has a bell "30 s before end"
- **THEN** it rings at 9:30
- **TEST** `meditate-core/src/bells.rs::a_bell_set_in_seconds_rings_at_that_second`

#### Scenario: A time keeps the unit it was set in
- **WHEN** a bell is set to 60 s and another to 1 min, and both are saved and read back
- **THEN** the first is 60 s and the second 1 min, and their titles say so
- **TEST** `meditate-core/src/db/interval_bells.rs::a_time_keeps_the_unit_it_was_set_in`

#### Scenario: Setting a time on the laptop and the phone
- **WHEN** a bell's time is set to 45 with the s switch, saved and reopened, in each app
- **THEN** the row reads "Every 45 s" and the editor shows 45 with s
- **MANUAL** editor widgets and the phone's UI don't run on the test host

### Requirement: Jitter is a time
A repeating bell's jitter SHALL be a time from 0 to half the bell's time, and each ring SHALL land anywhere from the planned gap minus the jitter to the gap plus the jitter.

#### Scenario: Every 7 minutes give or take one minute
- **WHEN** a bell rings every 7 min ±1 min
- **THEN** each gap is between 6:00 and 8:00, and the lowest and highest draws give 6:00 and 8:00
- **TEST** `meditate-core/src/bells.rs::a_jitter_in_seconds_bounds_each_gap`

#### Scenario: A jitter longer than half the time
- **WHEN** a bell every 60 s has a jitter of 45 s
- **THEN** each gap is between 30 s and 90 s, and the stored jitter stays 45 s
- **TEST** `meditate-core/src/bells.rs::a_jitter_is_capped_at_half_the_time`

### Requirement: A repeating bell's gap can grow by a step
A repeating bell SHALL have a step, 0 to 60 min; each gap SHALL be the previous gap plus the step, and a jitter SHALL apply around the grown gap.

#### Scenario: Every 60 seconds, growing by 5
- **WHEN** a bell rings every 60 s with a step of 5 s and no jitter
- **THEN** it rings at 1:00, 2:05, 3:15 and 4:30
- **TEST** `meditate-core/src/bells.rs::a_step_grows_each_gap`

#### Scenario: A time jump with a step
- **WHEN** that bell's session is suspended at 0:59 and woken at 3:20
- **THEN** the bell rings once, and next at 4:30
- **TEST** `meditate-core/src/bells.rs::a_step_survives_a_time_jump`

#### Scenario: A fixed bell has no jitter or step
- **WHEN** a bell every 60 s ±30 s, +5 s is switched to "At time from start", in either app
- **THEN** it is saved with no jitter and no step
- **TEST** `meditate-core/src/bells.rs::a_fixed_bell_keeps_no_jitter_or_step`

#### Scenario: The row title names all three
- **WHEN** a bell is every 120 s ±1 min with a step of 5 s, and another is the same with ±3 min
- **THEN** both rows read "Every 120 s ±1 min, +5 s" in both apps: a jitter shows as it rings
- **TEST** `meditate-core/src/bells.rs::the_title_key_carries_each_time_with_its_unit`

#### Scenario: The same titles on the laptop and the phone
- **WHEN** those two bells are listed in each app
- **THEN** the laptop reads "Every 120 s ±1 min, +5 s" and the phone "Every 120 s +/-1 min, +5 s" (its font has no plus-minus sign), for both rows
- **MANUAL** the GTK title is unit-tested (`bell_title_names_time_jitter_and_step_with_their_units`); the phone's title goes through Slint's catalogue, which only runs on Android

### Requirement: Bells from older versions keep their times
A bell written before this change, or by an older version over sync or in a preset, SHALL read with the same time and jitter it had and no step; a bell written now SHALL still read sensibly on the older version.

#### Scenario: A database from the last release
- **WHEN** a database from 26.10.1 with a bell every 7 min ±14 % is opened
- **THEN** the bell reads every 7 min, ±59 s, step 0
- **TEST** `meditate-core/src/db/mod.rs::a_bell_from_the_last_release_keeps_its_times`

#### Scenario: A synced bell from an older version
- **WHEN** an older version's edit of a bell (every 5 min ±20 %) arrives
- **THEN** the bell reads every 5 min, ±1 min, step 0
- **TEST** `meditate-core/src/db/interval_bells.rs::a_synced_bell_from_an_older_version_reads_in_seconds`

#### Scenario: A bell changed by an older version on this device
- **WHEN** a bell every 90 s, step 5 s is changed to 3 min by an older version on the same device, and this version opens the database again
- **THEN** the bell reads every 3 min, step 0
- **TEST** `meditate-core/src/db/interval_bells.rs::a_row_an_older_version_changed_reads_from_its_minutes`

#### Scenario: A bell written now, read by an older version
- **WHEN** a bell every 90 s ±30 s, step 5 s is saved
- **THEN** its event also carries 2 minutes and 33 %, which the older version reads
- **TEST** `meditate-core/src/db/interval_bells.rs::a_bell_in_seconds_also_writes_minutes_and_percent`

#### Scenario: A bell a newer device sent before this one updated
- **WHEN** a device on 26.10.1 receives a bell every 90 s ±30 s, step 5 s from an updated device, and then updates itself
- **THEN** after the update it reads every 90 s, ±30 s, step 5 s, like the other device
- **TEST** `meditate-core/src/db/mod.rs::a_bell_a_newer_device_sent_before_the_update_keeps_its_seconds`

#### Scenario: A newer device's change applied while this one was back on the older version
- **WHEN** a device goes back to 26.10.1, receives a change of a bell's step from 5 s to 10 s there, and updates again
- **THEN** the bell reads step 10 s
- **TEST** `meditate-core/src/db/mod.rs::a_change_received_on_the_older_version_is_kept_after_updating_again`

#### Scenario: A preset saved before this change
- **WHEN** a preset whose bell has only minutes 5 and jitter 20 % is loaded
- **THEN** its bell is every 5 min, ±1 min, step 0 and matches the same library bell, and a preset saved now still carries minutes and percent
- **TEST** `meditate-core/src/preset_config.rs::a_preset_bell_without_seconds_reads_from_minutes`
