# Dependency refresh

## Why
The update list (#21) shows every crate behind its newest compatible
version, a two-minor-old blueprint-compiler and GitHub Actions two majors
behind. The maintainer wants everything that isn't a fixed reproducibility
pin brought up to date, without touching F-Droid's build recipe.

## What Changes
- `Cargo.lock`: every crate to its newest version the existing Cargo.toml
  ranges allow (262 changes in a dry run). kstring stays at 2.0.2: 2.0.5
  needs Rust 1.96, the pin is 1.95. cc stays at 1.2.60: it compiles the
  APK's C code, and only F-Droid's own rebuild can prove a new one keeps
  the bytes. Both keep the lock-file line in the update list.
- `build-aux/cargo-sources.json` regenerated for the offline Flatpak build.
- Flatpak manifest: blueprint-compiler 0.20.4 -> 0.22.2 (URL and sha256).
- Workflows: actions/checkout v7 (also the commit-pinned copies in
  fresh-build.yml and renovate.yml), actions/upload-artifact v7,
  actions/setup-java v6.
- Update check config: the CI APK's `setup-java` JDK only shows 21.x,
  since Gradle 8.5 runs on JDK 21 at most.
- OPEN-ISSUES: one entry per crate with a new major (rusqlite 0.41, ureq 3,
  jni 0.22, oo7 0.6, gettext-rs 0.8, roxmltree 0.21, material-colors 0.5),
  and one entry for the pinned toolchain (Rust, NDK r27d, Gradle, AGP,
  Kotlin, kstring, cc), which needs a fdroiddata recipe MR or a release
  to prove.
- Left out: the seven majors and the pins (own issues, above); SHA-pinning
  the actions (#169).

## Decisions
- Lock refresh and the outside-the-app updates now, the majors as single
  OPEN-ISSUES entries (maintainer, 2026-11-03).
- Nothing in F-Droid's recipe may need to change; anything that would goes
  to its own issue (maintainer).
- cc held at 1.2.60, into the pins entry (maintainer).
- A crate update that needs a newer Rust, or a new build-time tool in the
  Android build (cmake, bindgen, a new `-sys` crate needing system
  libraries), is held back with `cargo update --precise` and noted in the
  pins issue (my call).
- Actions stay on tags as today; SHA pins are #169 (my call).
- The repro probe builds a commit object made with a temporary index and
  hung on no branch (as for the update-check test runs), so nothing is
  committed before the device test (my call).
- `deps-test` and `renovate-test` are pushed for the CI proofs and
  deleted afterwards (my call). The update-list proof (2.2) runs after the
  commit lands on `beta`, since Renovate scans `beta` (my call).
- The JDK rule matches `matchDepNames: ["java-jdk"]`; matching the package
  name would also hit the JDK 17 pin (my call).

## Capabilities
None: no behaviour changes (`skip_specs: true`).

## Impact
- Core, GTK, Android code: none planned; only if a patch release breaks a
  build, then the smallest fix.
- Translations, Data: none.
- Android build environment: `Cargo.lock` (asked by the maintainer);
  recipe, toolchain, Gradle untouched; the repro probe runs.
- Docs: OPEN-ISSUES.md; BUILDING.md's update-check paragraph (CI JDK 21.x).
