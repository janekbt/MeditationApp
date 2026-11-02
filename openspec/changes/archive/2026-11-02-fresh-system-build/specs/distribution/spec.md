## ADDED Requirements

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
