# Appearance

## Purpose
How the Android app and its home-screen widget look: which Material 3
colour role each kind of element takes, so every accent and the
system colours reach every element the same way.

## Requirements

### Requirement: Android elements take their colours from Material 3 roles
Every element of the Android app SHALL take its colour from a Material 3 colour role or a state layer over one; only the black running screen, the white wand on the System circle (which shows the system primary) and the transparent chart and heatmap fills are exempt.

#### Scenario: Rows inside an expanded group
- **WHEN** the user expands a group such as Label, Bells or Preparation Time, in light and in dark mode
- **THEN** the rows inside sit on the container-low colour, a step apart from the card around them, and a pressed row shifts toward the text colour by the on-surface state layer
- **MANUAL** screenshots on the API 35 emulator, light and dark, with Blue and Rose: colours need a render.

#### Scenario: A selected segment or preset
- **WHEN** a segment of a toggle (such as s / min) or a preset in the list is selected
- **THEN** its text has the on-secondary-container colour on the secondary-container fill
- **MANUAL** screenshot on the API 35 emulator with Rose: colours need a render.

#### Scenario: Pressing a button and the Play/Stop pill
- **WHEN** the user presses a filled or outlined wrap button, or the Play/Stop pill of a bell sound
- **THEN** the button shows a Material state layer, the pill's text is in the accent colour and it fills lightly with the accent while pressed
- **MANUAL** press and hold on the API 35 emulator: a pressed state needs a device.

#### Scenario: A dialog, a sheet or the snackbar opens
- **WHEN** a dialog or the Log filter sheet opens, or a snackbar shows
- **THEN** the screen behind dims by the scrim colour at 32 %, and the snackbar's shadow uses the shadow colour
- **MANUAL** screenshot on the API 35 emulator: colours need a render.

#### Scenario: The page behind the cards
- **WHEN** the Timer page shows with Rose, or with System on Android 14+
- **THEN** the space between the cards is the surface colour of that scheme, not a fixed light grey
- **TEST** `meditate-android/src/app.rs::the_window_background_is_the_surface_role`

#### Scenario: A later edit brings back a colour outside the roles
- **WHEN** the layout file contains a darkened, brightened or faded colour, or any fixed colour outside the running screen
- **THEN** the build's tests fail
- **TEST** `meditate-android/src/app.rs::the_layout_uses_only_material_roles`

### Requirement: The widget follows the system colours
On Android 12 and later, the home-screen widget SHALL take its background, text and row colours from the system colours, light and dark; below Android 12 it SHALL keep its slate colours.

#### Scenario: The widget on Android 12 or later
- **WHEN** the widget is on the home screen of a phone with Android 12+ and the user switches dark mode or sets a wallpaper with other colours
- **THEN** the widget's background, text and preset rows take the new system colours
- **MANUAL** on the FP5: the widget host and wallpaper colours need a phone.

#### Scenario: The widget below Android 12
- **WHEN** the widget is on a phone with Android 8 to 11
- **THEN** it looks as before, in slate
- **MANUAL** on the API 30 emulator: the widget host needs a device.

### Requirement: System bar symbols stay readable in both themes
The navigation bar and status bar symbols SHALL be dark over a light page and light over a dark or black page, also after the theme changes while the app is open.

#### Scenario: Three-button navigation in the light theme
- **WHEN** the phone is in light mode with three-button navigation and Meditate is opened
- **THEN** the back, home and recent buttons and the status bar clock and battery are dark on the light page
- **MANUAL** on the FP5: Android draws the system bars, so only a phone shows them.

#### Scenario: Three-button navigation in the dark theme
- **WHEN** the phone is in dark mode with three-button navigation and Meditate is opened
- **THEN** the buttons and the status bar symbols are light, as before
- **MANUAL** on the FP5: Android draws the system bars, so only a phone shows them.

#### Scenario: The theme changes while Meditate is open
- **WHEN** Meditate is open and the user switches the phone from dark to light mode in the quick settings, and back
- **THEN** each time the bar symbols switch with the page, without restarting the app
- **MANUAL** on the FP5: Android draws the system bars, so only a phone shows them.

#### Scenario: A session runs in the light theme
- **WHEN** the phone is in light mode and a session is started, then stopped and the session saved
- **THEN** on the black session screen the bar symbols are light, and back on the timer page they are dark again
- **MANUAL** on the FP5: Android draws the system bars, so only a phone shows them.
