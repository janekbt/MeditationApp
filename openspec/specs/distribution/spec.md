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

### Requirement: The shipped Linux app carries no build tools
The FlatPark tarball SHALL hold only what the app needs at runtime, without the UI compiler or the build manifest.

#### Scenario: The tarball of either architecture after a CI run
- **WHEN** the x86_64 and the aarch64 tarball of a CI run are listed
- **THEN** neither holds `bin/blueprint-compiler`, a `lib/python*` folder or `manifest.json`, and both still hold `bin/meditate`
- **MANUAL** `tar -tzf` on both artifacts of the side-branch CI run; the Flatpak builds only run in CI, no host test can make them

#### Scenario: The slimmed bundle still runs
- **WHEN** the x86_64 bundle of that run is installed and Meditate started
- **THEN** the app opens in the system language with its normal UI, and a one-minute session rings its start and end bells
- **MANUAL** Janek on the laptop: only an installed Flatpak shows whether something the app needs was cleaned out

### Requirement: Both apps build from source on a fresh machine with one command
On a fresh Ubuntu 26.04, Debian 13, Fedora or Arch, `./build.sh gtk` and `./build.sh android` SHALL each install what is missing and finish with the built app, with no step done by hand.

#### Scenario: The Linux app on a fresh machine
- **WHEN** a normal user with sudo runs `./build.sh gtk --debug` in a fresh clone on each of the four systems
- **THEN** it ends with `meditate-gtk/builddir/src/meditate` built and no question asked
- **MANUAL** the "Fresh system build" workflow, jobs `gtk` on all four systems, green; a whole-system bootstrap can't run as a host test

#### Scenario: The Android app on a machine without rustup or JDK 17
- **WHEN** the same user runs `./build.sh android --debug` on each of the four systems, none of which has rustup or a JDK 17
- **THEN** it ends with the debug APK built and no question asked
- **MANUAL** the "Fresh system build" workflow, jobs `android` on all four systems, green; same reason

#### Scenario: A machine that already has JDK 17 keeps it
- **WHEN** `build-aux/setup-android.sh` runs where a JDK 17 is installed
- **THEN** it downloads no JDK and the Android build uses the installed one
- **MANUAL** run it on the maintainer's laptop: no JDK download in the log, `env.sh` still points `JAVA_HOME` at `/usr/lib/jvm/java-17-openjdk-amd64`; the release builds depend on this machine's JDK

#### Scenario: A downloaded JDK that doesn't match its checksum
- **WHEN** the JDK download's checksum differs from the pinned one
- **THEN** the setup stops with a message naming the file, and leaves no JDK behind that a later run would trust
- **MANUAL** run `setup-android.sh` with `HOME` set to a scratch folder and one digit of the pinned checksum changed; then run it again with the right checksum and it downloads again

### Requirement: A weekly list shows what the apps could update
Once a week, one GitHub issue SHALL list every newer version of the libraries and tools the two apps build with, without changing the repository.

#### Scenario: The test run lists every pin
- **WHEN** the update check runs on the `renovate-test` branch
- **THEN** the "Dependency Dashboard" issue exists, and its detected list holds the direct crates of all three apps' manifests, the Cargo.lock refresh, the Rust toolchain, the Android Gradle and Kotlin plugins, the Gradle wrapper, NDK (with its patch releases), Gradle, Kotlin compiler and JDK 17 from the Android setup script, the GNOME runtime and blueprint-compiler, and the version-pinned GitHub Actions, each with the version the repo pins today
- **MANUAL** read the issue after the side-branch run against `grep PINNED_ build-aux/setup-android.sh`, the manifests and the workflows; only a real run against GitHub's API can show it

#### Scenario: Newer versions appear, nothing changes
- **WHEN** that run finds newer versions
- **THEN** each is listed in the issue on its own line (the indirect crates as one lock-file line), and no branch, pull request or commit appears on the repository
- **MANUAL** after the run, `git ls-remote origin` shows no `renovate/` branch and `gh pr list` shows no new pull request; same reason
