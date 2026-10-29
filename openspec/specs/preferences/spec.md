# Preferences

## Purpose
Preferences hold the choices that shape the whole app rather than one
session, such as its accent colour. This covers how those choices are
offered, what they start on and how they travel between devices.

## Requirements

### Requirement: The Android accent can follow the system colours
On Android 14 and later, the accent picker SHALL offer System as its first choice, which colours the app from the system's Material 3 scheme in light and dark mode; below Android 14 it SHALL offer only the six fixed colours.

#### Scenario: A new install on Android 14 or later
- **WHEN** the app starts on Android 14+ and no accent was ever picked on this device or synced to it
- **THEN** the accent is System, and nothing is written, so no pick is synced to the other devices
- **TEST** `meditate-android/src/theme.rs::a_missing_pick_is_system_where_the_system_has_a_scheme`

#### Scenario: A new install below Android 14
- **WHEN** the app starts on Android 13 or older and no accent was ever picked
- **THEN** the accent is Blue, as before
- **TEST** `meditate-android/src/theme.rs::a_missing_pick_is_blue_without_a_system_scheme`

#### Scenario: The order of the circles
- **WHEN** the user opens Preferences on Android 14+, and then on Android 13
- **THEN** on 14+ the row is System, Blue, Green, Teal, Purple, Rose, Amber; on 13 it is the six colours without System
- **TEST** `meditate-android/src/theme.rs::the_system_circle_comes_first_only_with_a_system_scheme`

#### Scenario: Picking System
- **WHEN** the user picks the System circle on the FP5 (Android 15)
- **THEN** buttons, switches, the navigation bar and the label colours in the Log take the wallpaper's colours, in light and in dark mode; picking Green afterwards brings Green back
- **MANUAL** on the FP5: the scheme comes from the phone's wallpaper, which the host can't provide.

#### Scenario: The wallpaper changes while System is picked
- **WHEN** System is picked and the user sets a wallpaper with other colours, then returns to Meditate
- **THEN** Meditate shows the new colours without a restart by hand
- **MANUAL** on the API 35 emulator: Android recreates the activity, which needs a device.

#### Scenario: The row at phone width
- **WHEN** Preferences is open at 360 px width on Android 14+
- **THEN** all seven circles show in full on one row; the System circle shows the system primary colour with a wand symbol, and the picked circle has the ring
- **MANUAL** screenshot on the API 35 emulator at 360 px: layout needs a render.

### Requirement: An accent pick means the same colour on every version
Every stored accent SHALL be readable by the previous release as the same colour, and System SHALL be stored as its own value that the previous release reads as Blue.

#### Scenario: Picking System stores its own value
- **WHEN** the user picks System, and later Green
- **THEN** the stored pick is `system`, then a value the previous release reads as Green and this version tells apart from an old Green
- **TEST** `meditate-android/src/theme.rs::picking_stores_values_the_previous_release_reads`

#### Scenario: A pick synced from a device on the previous release
- **WHEN** a device still on the previous release picks Teal and the pick syncs
- **THEN** a device with this change shows Teal below Android 14 and System on 14+, the same as an old pick of its own
- **TEST** `meditate-android/src/theme.rs::a_synced_old_pick_reads_like_an_old_pick`

#### Scenario: A System pick synced to a phone below Android 14
- **WHEN** a pick of System syncs to a phone on Android 13
- **THEN** that phone shows Blue with the Blue circle ringed, and the stored pick stays System for the devices that have it
- **TEST** `meditate-android/src/theme.rs::a_system_pick_shows_blue_without_a_system_scheme`

### Requirement: Picks from before this change count as System
On Android 14 and later, an accent stored by an earlier version SHALL show as System without anything being written, while a pick made with this change SHALL be kept on every device and version.

#### Scenario: An existing install with any colour
- **WHEN** the app starts on Android 14+ with Blue, or with Rose, stored by an earlier version
- **THEN** the accent is System, the System circle is ringed, and nothing is written or synced
- **TEST** `meditate-android/src/theme.rs::an_old_pick_is_system_where_the_system_has_a_scheme`

#### Scenario: The ring after an old pick
- **WHEN** Preferences opens on Android 14+ with Rose stored by an earlier version, and on Android 13 with the same value
- **THEN** on 14+ the System circle is ringed; on 13 the Rose circle is
- **TEST** `meditate-android/src/theme.rs::the_ring_marks_the_accent_that_shows`

#### Scenario: Starting the app writes no accent
- **WHEN** the app starts with any stored accent, or none
- **THEN** it stores nothing for the accent; only a pick in Preferences does
- **TEST** `meditate-android/src/app.rs::only_a_pick_writes_the_accent`

#### Scenario: A colour picked with this change
- **WHEN** the user picks Teal (or Blue) and restarts the app, and the pick syncs to a device on the previous release
- **THEN** the accent stays Teal here, and the other device shows Teal too
- **TEST** `meditate-android/src/theme.rs::a_pick_made_now_is_kept`

#### Scenario: An old pick on a phone below Android 14
- **WHEN** the app starts on Android 13 with Rose stored by an earlier version
- **THEN** the accent is Rose, as before
- **TEST** `meditate-android/src/theme.rs::an_old_pick_below_android_14_keeps_its_colour`

### Requirement: The System colours are the system's own roles
With System picked, each Material 3 colour role SHALL take the colour the system reports for it, light and dark separately; when the system lacks any of them, System SHALL not be offered.

#### Scenario: The roles are read from the system
- **WHEN** the system reports its colour roles
- **THEN** the app's primary, surface and the other roles show those exact colours, the surface tint is the system primary, the inverse roles come from the system's tonal palettes, and shadow and scrim stay as shipped
- **TEST** `meditate-android/src/theme.rs::the_system_scheme_takes_every_role_from_the_system`

#### Scenario: The app takes the system roles
- **WHEN** System is picked and the system reports its roles
- **THEN** each colour role of the app's light and dark scheme is the system's, and shadow and scrim are the shipped ones
- **TEST** `meditate-android/src/theme.rs::system_puts_every_role_on_the_material_scheme`

#### Scenario: The system reports a role as missing
- **WHEN** the system lacks one of the roles (a modified Android)
- **THEN** System is not offered and the fixed colours work as below Android 14
- **TEST** `meditate-android/src/theme.rs::a_missing_role_means_no_system_scheme`

### Requirement: A fixed accent is a full Material 3 scheme
Green, Teal, Purple, Rose and Amber SHALL each colour every Material 3 role, surfaces included, with Material's tonal-spot scheme from that accent's seed colour, while Blue SHALL stay the shipped scheme.

#### Scenario: The scheme from a seed matches Material's own
- **WHEN** a scheme is made from Material's high-chroma reference seed, in light and in dark mode
- **THEN** each role Material's reference test lists has the value it gives
- **TEST** `meditate-android/src/theme.rs::a_seed_gives_materials_tonal_spot_scheme`

#### Scenario: Picking Rose
- **WHEN** the user picks Rose
- **THEN** every role, the surfaces too, comes from Rose's seed, in light and in dark mode
- **TEST** `meditate-android/src/theme.rs::a_fixed_accent_takes_every_role_from_its_seed`

#### Scenario: Picking Blue
- **WHEN** the user picks Blue
- **THEN** the app shows the shipped scheme, unchanged
- **TEST** `meditate-android/src/theme.rs::blue_stays_the_shipped_scheme`

#### Scenario: The circles in the picker
- **WHEN** the accent picker shows the fixed colours
- **THEN** each circle shows its accent's light primary colour
- **TEST** `meditate-android/src/theme.rs::a_circle_shows_its_accents_light_primary`

#### Scenario: Rose on screen
- **WHEN** the user picks Rose and looks at Timer, Log and Preferences in light and dark mode
- **THEN** the backgrounds and cards carry a faint rose tint, and text stays readable
- **MANUAL** screenshots on the API 35 emulator: a tint needs a render.

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
