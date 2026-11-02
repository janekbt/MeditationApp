## ADDED Requirements

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
