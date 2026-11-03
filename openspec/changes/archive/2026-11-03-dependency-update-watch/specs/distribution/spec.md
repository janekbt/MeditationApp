## ADDED Requirements

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
