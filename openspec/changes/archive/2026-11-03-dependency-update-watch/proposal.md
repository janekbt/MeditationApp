# Dependency update watch

## Why
New versions of the libraries and tools the apps build with (Rust crates,
the Rust toolchain, Slint, the GNOME runtime, the Android NDK, Gradle,
Kotlin, the JDK, GitHub Actions) only get noticed by chance today. The
maintainer wants one place that shows, every week, what could be updated,
so the apps stay current. Nothing may update itself: F-Droid's
reproducible build needs every pin to change only on purpose.

## What Changes
- A weekly GitHub Actions workflow runs Renovate (the open-source CLI, no
  Mend app, no Mend servers) with the workflow's own token.
- Renovate keeps one issue, the "Dependency Dashboard", listing every
  available update one line each; it opens no pull request and pushes no
  branch. Its "Detected dependencies" part lists every pin, by file.
- Covered: direct crates of the three Cargo.toml (one line each), the rest
  of Cargo.lock as one "refresh the lock file" line, `rust-toolchain.toml`,
  the Gradle plugins (AGP, Kotlin) and the Gradle wrapper, the pins in
  `build-aux/setup-android.sh` (NDK with its patch releases, Gradle, Kotlin compiler,
  Temurin JDK 17.x), the Flatpak manifest (GNOME runtime, blueprint-compiler), the
  GitHub Actions pinned to a version (`dtolnay/rust-toolchain@stable` follows
  a branch and is skipped).
- Left out: Android SDK platform, build-tools and cmdline-tools (Renovate
  has no source for Google's SDK index, and the reproducible pins
  platform 34 / build-tools 31 must not move anyway; #201 owns the
  targetSdk step); JDK majors above 17 (the repro dexer needs 17, #201
  owns ending that; the CI APK's `setup-java` JDK is watched on every
  major); the patched Slint copies in `third_party/` (they follow
  the slint line); the second `gnome-51` copy inside flatpak.yml's run
  script (RELEASE.md: move both); the second NDK copy in Gradle and rust-build.sh (moves with the pin); pull requests on a ticked box (updates go through
  `/spec`, so the workflow keeps write access to issues only); e-mail
  (not needed, the issue is enough).

## Decisions
- Renovate over Dependabot: Dependabot can't watch the shell-script and
  Flatpak pins and only opens one PR per update (maintainer, 2026-11-03).
- Self-run as a GitHub Action, not the Mend app (maintainer).
- Crates: direct ones one line each, indirect ones as one lock-file line
  (maintainer).
- A ticked box does nothing: issues-only write access, the header says
  so (maintainer).
- Weekly, Monday 05:00 UTC (my call).
- Renovate reads `beta`, where work lands; the schedule itself only runs
  from `main`, so it starts after the next release, and a push to the
  branch `renovate-test` runs it before that (my call, same pattern as
  fresh-build.yml).
- All Renovate settings live in one file the workflow hands to Renovate,
  and repo config files are ignored: Renovate reads a repo's own config
  only from the default branch (`main`), so the side-branch test would
  otherwise run on defaults and open PRs (my call).
- NDK patch releases count: compared with the whole `PINNED_NDK`, using
  the sdkmanager version each release's notes carry (r27d =
  27.3.13750724) (maintainer, 2026-11-03).
- The Renovate version is pinned exactly, like the action (my call).
- Info only: the dashboard's header says not to tick and that updates
  go through `/spec`; the token gets `issues: write`, `contents: read` (my
  call).
- GNOME runtime and blueprint-compiler: tags of GNOME's GitHub mirror
  (gnome-build-meta, major only); GNOME's GitLab API answered 503 on the
  test run (my call).

## Capabilities
### Modified Capabilities
- `distribution`: a weekly list of available updates for everything the apps build with

## Impact
- Core, GTK, Android code: none
- Translations: none
- Data: none
- Android build environment: untouched (Renovate only reads the pins)
- New files: `.github/workflows/renovate.yml`, `.github/renovate-config.json5`
- Docs: BUILDING.md (CI section), RELEASE.md (sweep: look at the dashboard,
  and check its "Detected dependencies" still lists every pin; the
  `gnome-NN` container moves with the runtime)
