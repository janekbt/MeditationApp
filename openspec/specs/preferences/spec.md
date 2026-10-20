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
