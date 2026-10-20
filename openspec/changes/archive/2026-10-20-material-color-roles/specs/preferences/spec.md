## ADDED Requirements

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
