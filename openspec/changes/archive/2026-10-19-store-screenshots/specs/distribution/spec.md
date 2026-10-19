## ADDED Requirements

### Requirement: The store listing shows current screenshots
The Linux app's metainfo SHALL link screenshots that exist in the repo at the linked path.

#### Scenario: A screenshot link after files moved
- **WHEN** the metainfo is read
- **THEN** every screenshot it links names a file in the repo, so the store finds it once `main` has it
- **TEST** `meditate-gtk/src/application.rs::metainfo_screenshots_exist_in_the_repo`

#### Scenario: The store after the next release
- **WHEN** the released Flatpak is opened in GNOME Software
- **THEN** it shows the light/dark Timer, Guided and Box Breath screenshots with their captions in the system language
- **MANUAL** Janek after the next release; the images load from `main`, which only moves at a release
