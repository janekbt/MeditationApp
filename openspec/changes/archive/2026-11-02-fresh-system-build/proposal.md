# Fresh system build

## Why
README promises that `./build.sh gtk|android` installs whatever is
missing, but nobody has run it on a fresh machine. Reading the scripts
shows at least three places where it stops and asks for handwork:
Android without rustup (#174), JDK 17 on Debian 13 and Fedora 42+ (both
no longer ship it), and the rustup installer needing curl, which a fresh
Ubuntu lacks. A first-time builder should get both apps with one command
and nothing to fix by hand.

## What Changes
- A GitHub workflow builds both apps with `./build.sh <app> --debug` on
  fresh Ubuntu 26.04, Debian 13, Fedora and Arch, as a normal user with
  sudo, started by hand (and by a push to the throwaway branch
  `fresh-build` while this change is tested).
- `build.sh android` installs Rust before the Android bootstrap (#174).
- When no JDK 17 is installed, `setup-android.sh` downloads a pinned
  JDK 17 (Eclipse Temurin 17.0.21+9, checksum verified) into
  `~/Android`, like the SDK and NDK. An installed JDK 17 is still used
  first. The distro JDK packages and the Debian 13 bookworm workaround
  go away.
- `build.sh` installs curl (for rustup), and for Android unzip and a C
  compiler, on every distro it knows; setup-android downloads with curl
  only and no longer pauses for Enter when build.sh runs it.
- Whatever else the first workflow run shows is fixed the same way.
- Left out: Flatpak builds (CI already builds them), optimised builds
  (same bootstrap as debug, only the last Gradle/meson step differs),
  aarch64 hosts, and moving off JDK 17 (needs newer build-tools and
  F-Droid's recipe; belongs with #201).

## Decisions
- Systems: Ubuntu 26.04, Debian 13, Fedora, Arch (maintainer, Debian
  added on request).
- A throwaway branch `fresh-build` with commits on it is fine for
  running the workflow before approval; it is deleted afterwards
  (maintainer).
- JDK: download Temurin 17 when none is installed, instead of per-distro
  packages (maintainer agreed to "fetch a pinned JDK 17").
- "Fresh" = the distro's official container image, plus `sudo` and a
  normal user with passwordless sudo. Arch also gets `pacman -Syu`
  first, which every real Arch install has done (my call).
- Image tags: `ubuntu:26.04`, `debian:13`, `fedora:latest`,
  `archlinux:latest`, so Fedora and Arch test what a new user installs
  today (my call). `actions/checkout` pinned to a commit SHA, as #169
  asks for the other workflow (my call).
- Only on demand, never on push to beta: GitHub minutes are free for a
  public repo, but a run takes up to an hour (my call). RELEASE.md gets
  one line: run it before a release that changed `build.sh` or
  `setup-android.sh` (my call).

## Capabilities
### Modified Capabilities
- `distribution`: building from source works on a fresh machine

## Impact
- Core: none
- GTK: none
- Android: none (app code untouched)
- Translations: none
- Data: none
- Android build environment: untouched. `setup-android.sh` is not on the
  list; the maintainer's machine keeps its own JDK 17, and F-Droid builds
  with its own recipe, so the reproducible build does not change.
- Files: `.github/workflows/fresh-build.yml` (new), `build.sh`,
  `build-aux/setup-android.sh`
- Docs: README (build section), BUILDING.md (JDK 17 and rustup
  paragraphs, CI section), RELEASE.md (one line), OPEN-ISSUES.md (#174
  out)
