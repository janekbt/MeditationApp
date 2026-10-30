## MODIFIED Requirements
### Requirement: The widget follows the system colours
On Android 12 and later, the home-screen widget SHALL take its background, text, row colours and the background of its app image from the system colours, light and dark; below Android 12 it SHALL keep its slate colours and the blue app image.

#### Scenario: The widget on Android 12 or later
- **WHEN** the widget is on the home screen of a phone with Android 12+ and the user switches dark mode or sets a wallpaper with other colours
- **THEN** the widget's background, text and preset rows take the new system colours, and the app image's background turns into a gradient of the new system colour with the white lotus on it
- **MANUAL** on the FP5: the widget host and wallpaper colours need a phone.

#### Scenario: The widget below Android 12
- **WHEN** the widget is on a phone with Android 8 to 11
- **THEN** it looks as before, in slate with the blue app image
- **MANUAL** on the API 30 emulator: the widget host needs a device.

#### Scenario: The app image's colours come from the system on 12+ and stay blue below
- **WHEN** the app is built
- **THEN** the app image's gradient reads its two colours from the colour list, blue in the base list and the system accent in the Android 12 list
- **TEST** `meditate-android/src/app.rs::the_widget_app_image_takes_the_system_accent`

## ADDED Requirements
### Requirement: The lotus shows its petals apart on every colour
On the phone, the lotus in the app symbol, the themed app symbol, the widget, the media control and the status bar SHALL part its petals with gaps that show the colour behind it, not with lines of a fixed colour.

#### Scenario: The themed app symbol
- **WHEN** "Themed icons" is switched on in the phone's wallpaper and style settings (Android 13+)
- **THEN** the Meditate symbol shows the lotus in the system colour with its five petals visibly apart, not as one blob
- **MANUAL** on the FP5: the launcher draws the themed symbol, so only a phone shows it.

#### Scenario: The app symbol and the media control on a non-blue colour
- **WHEN** the plain app symbol is on the home screen, and a session runs with the accent set to Rose
- **THEN** the app symbol looks as before with the blue between the petals, and the media control shows rose, not blue, between the petals, as sharp as before, and the small lotus in the status bar shows its petals apart
- **MANUAL** on the FP5: the launcher, the media control and the status bar need a phone.

#### Scenario: A later edit brings back fixed-colour lines
- **WHEN** the lotus drawing contains a coloured stroke on a petal, or a line drawn over the petals
- **THEN** the build's tests fail
- **TEST** `meditate-android/src/app.rs::the_lotus_parts_its_petals_with_gaps`
