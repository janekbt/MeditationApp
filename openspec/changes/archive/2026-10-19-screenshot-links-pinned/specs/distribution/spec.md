## MODIFIED Requirements

### Requirement: The store listing shows current screenshots
The Linux app's metainfo SHALL link screenshots at the newest release's tag, each naming a file in the repo.

#### Scenario: A screenshot link after files moved
- **WHEN** the metainfo is read
- **THEN** every screenshot it links names a file in the repo, at the tag of the newest release it lists
- **TEST** `meditate-gtk/src/application.rs::metainfo_screenshots_exist_in_the_repo`

#### Scenario: A beta links its beta tag
- **WHEN** the newest release is marked as a development release
- **THEN** every screenshot link names the `-beta` tag of that version
- **TEST** `meditate-gtk/src/application.rs::a_beta_links_its_beta_tag`

#### Scenario: A release bump moves the links
- **WHEN** `bump-version.sh` stamps a new version
- **THEN** every screenshot link names the new version's tag
- **MANUAL** run it on a scratch branch (`bump-version.sh 26.10.9`), check the links, discard; a script that edits five release files has no host test

#### Scenario: The store after the next release
- **WHEN** the released Flatpak is opened in GNOME Software
- **THEN** it shows the light/dark Timer, Stats and Log screenshots with their captions in the system language
- **MANUAL** Janek after the next release; the images load from that release's tag
