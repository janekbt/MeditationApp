# Distribution

## Purpose
How people get the app: the Android app on F-Droid, the Linux app as
Flatpak bundles on GitHub releases, and from the next release on as
the app tree the FlatPark remote installs.

## Requirements

### Requirement: The Linux app installs through FlatPark on PC and phone
Every release SHALL carry, for x86_64 and aarch64, a tarball of the
installed app that runs unmodified from FlatPark's package on the
current GNOME runtime.

#### Scenario: Installed from the FlatPark package on a PC
- **WHEN** the FlatPark package, pointing at the x86_64 tarball, is installed and Meditate started
- **THEN** the app opens in the system language, a timer session rings its bells and shows its notification at the end, and the window size is kept after a restart
- **MANUAL** Janek on the laptop: the package runs only inside Flatpak, no host test can start it

#### Scenario: The aarch64 tarball is a phone build of the same tree
- **WHEN** the aarch64 tarball is unpacked
- **THEN** it holds the same files as the x86_64 one, and `bin/meditate` is an aarch64 executable
- **MANUAL** on the release's CI run before the tag (an aarch64 build only runs in CI): compare both file lists and run `file bin/meditate`; a Librem 5 install waits for the listing

#### Scenario: A release without the tarball is refused
- **WHEN** `attach-flatpaks.sh` runs for a release whose CI run has no tarball artifact
- **THEN** it stops with an error before uploading anything
- **MANUAL** run it for 26.10.2 with `gh` replaced by a shim that passes reads through and only logs uploads (that run has no tarball): it fails with zero uploads; a script driving `gh` against a real release has no host test

#### Scenario: A tarball FlatPark may have pinned is never replaced
- **WHEN** `attach-flatpaks.sh` runs again for a release that already has a tarball
- **THEN** the same file is kept as it is, and a different file stops the script before any upload
- **MANUAL** with a fake `gh` (no asset, same digest, other digest): uploads all four, uploads only the bundles, uploads nothing; same reason as above

### Requirement: A new GNOME runtime runs on the phone before it is released
A release that moves the Linux app to a new runtime major SHALL have its
aarch64 build started on a Librem 5 before it is tagged.

#### Scenario: The GNOME 51 bundle on the Librem 5
- **WHEN** the release's CI run is green and its aarch64 bundle is installed on the Librem 5
- **THEN** a timer session rings its bells, vibrates, and shows its notification at the end
- **MANUAL** Janek on the Librem 5 at the next release (RELEASE.md step 4); no aarch64 GNOME 51 build exists before CI builds the committed change

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
