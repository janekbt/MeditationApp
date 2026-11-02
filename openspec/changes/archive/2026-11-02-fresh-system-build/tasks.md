# Tasks: fresh-system-build

Base: 605c4740988b34a9c531cd0f38ac0781baa4c741; suites on it: not run, the change touches only shell scripts, a workflow and docs (no Rust)

## Loop
- [x] 0 Skills loaded, again after every /compact: `/ponytail ultra`; `slint:slint` (loaded, no Android UI touched)
- [x] 1 Understand
- [x] 2 Propose (approved: 2026-11-02)
- [x] 3 Build
- [x] 4 Review, then `/simplify` after the last review
- [x] 5 Specs, docs, list
- [x] 6 Verify and hand over
- [x] 7 Ship

## Review Focus
- A user whose shell exports `JAVA_HOME` to another JDK -> not covered; env.sh keeps an exported JAVA_HOME on purpose (BUILDING.md says so); the workflow user has none
- A second run on the same machine (everything present) -> covered by the existing presence checks; the workflow does not test it, the maintainer's laptop does every day
- An interrupted JDK download -> "A downloaded JDK that doesn't match its checksum" (unpack only after the check, into a temp dir, then move)
- Repo path with a space, non-x86_64 host -> not covered; the NDK path is linux-x86_64 already, #168 owns the space problem
- Disk and memory on the GitHub runner -> android jobs run separately from gtk jobs; RAM guard caps jobs at 4

## 1. The workflow, red first
- [x] 1.1 Write `.github/workflows/fresh-build.yml`: matrix {ubuntu:26.04, debian:13, fedora:latest, archlinux:latest} x {gtk, android}, fail-fast off; checkout on the runner, then `docker run` of the image with the checkout mounted: install sudo, add user `builder` (the runner's uid, passwordless sudo), then `sudo -u builder -H` runs `./build.sh <app> --debug`; checks the built file exists. Triggers: workflow_dispatch, push to `fresh-build`. Proof: pushed to `fresh-build` at the base, Expected: android jobs FAIL (rustup), Fedora/Debian android FAIL on JDK, Ubuntu FAIL on curl; record each failure in Notes

## 2. build.sh
- [x] 2.1 `ensure_rust` before the Android bootstrap (#174). Proof: android jobs get past the rustup error
- [x] 2.2 curl and ca-certificates installed by `ensure_rust` when curl is missing; a C compiler, unzip and curl installed for android on every package manager, apt included; the JDK package lines go. Proof: Ubuntu jobs get past rustup's download

## 3. setup-android.sh
- [x] 3.1 No JDK 17 found -> download Temurin 17.0.21+9 (sha256 8b9b611d917e3dfd8419d3566b5949226f69b052fed7e109c1368add1dfe8711) into `~/Android/jdk-17.0.21+9` (verify, unpack to temp, move); the apt/dpkg branch and the bookworm message go; missing unzip/curl -> one-line message; downloads by curl. Proof: Debian and Fedora android jobs green; scenarios "keeps it" and "checksum" by hand
- [x] 3.2 Every other failure the runs show: one task each, added here (none besides the Arch JAVA_HOME export, covered by 3.1)

## 4. Docs
- [x] 4.1 README build section, BUILDING.md (JDK 17, rustup, CI), RELEASE.md line, OPEN-ISSUES #174 out. Proof: grep for "bookworm", "rustup installed first", "#174"

## 5. Cleanup
- [x] 5.1 Delete the `fresh-build` branch on GitHub after the last green run

## Notes
- Throwaway-branch commits before approval: allowed by the maintainer (2026-11-02).
- Pre-build agent questions, all my call:
  - Android path also installs a host C compiler (build scripts link with host `cc`): build-essential / gcc / base-devel.
  - setup-android exports `JAVA_HOME` and puts its `bin` on PATH as soon as the JDK is chosen (sdkmanager needs java; a downloaded JDK is not on PATH).
  - `find_jdk17` order: `/usr/lib/jvm`, then `javac` on PATH, then `~/Android/jdk-17.0.21+9` last, so the laptop keeps its system JDK and a second run doesn't re-download.
  - Pin: GitHub release asset `OpenJDK17U-jdk_x64_linux_hotspot_17.0.21_9.tar.gz` (JDK, not JRE), sha256 from api.adoptium.net; checked against Adoptium's `.sha256.txt` once while building.
  - "Press Enter" pauses: build.sh runs setup-android with stdin from /dev/null, so its `-t 0` checks stay quiet; run alone it still pauses.
  - Downloads use curl only; wget is dropped (Fedora's wget2 flags differ, curl is installed anyway).
  - Fedora line adds `tar`; apt runs with `DEBIAN_FRONTEND=noninteractive` set inside sudo.
  - rustup installs with `--default-toolchain none`; rust-toolchain.toml brings 1.95.0 (saves a second toolchain on disk).
  - Workflow: checkout on the runner with git, then `docker run` of the image with the checkout mounted, instead of `container:`; so `.git` is there and a host step can free disk if a run needs it.
  - The JDK pre-check in build.sh becomes a check for the tools build.sh itself installs (curl, unzip, cc).
  - fontconfig headers, skia prebuilt download: left to the first run (task 3.2).
- Base run 41535720668 (d0801fe, 2026-11-02), red as expected:
  - gtk ubuntu, gtk debian: `curl: command not found` at the rustup install.
  - android ubuntu: setup-android stops, `rustup not found` (#174).
  - android debian: JDK 17 not in apt, prints the bookworm commands.
  - android arch: jdk17 installed by pacman but not on PATH until a re-login, JAVA_HOME not exported, so `sdkmanager --licenses` fails (pre-build question 2).
  - gtk/android fedora (45): the image has no `su`; a workflow bug, fixed with `sudo -u` (task 1.1).
  - gtk arch: green already.
- Run 2 (14edd94): all eight jobs green.
- Scenario "checksum" by hand: setup-android cut after Step 1, `/usr/lib/jvm` hidden by a tmpfs (`unshare -rm`), HOME in ~/.cache: wrong checksum -> message, rc 1, nothing in ~/Android; right one -> installed; again -> no download, same JDK.
- Review (all held, fixed in 32a91ee):
  1. JDK unpacked in /tmp (tmpfs) then mv'd: an interrupted copy left a JDK that looks whole, or a nested `jdk-*/jdk-*`. Temp dir now inside ~/Android, `rm -rf` of the target before the mv. Checked by hand: a leftover folder without javac is replaced, no temp dir left.
  2. Release machine could silently get Temurin instead of Debian 12's JDK: RELEASE.md checks the JAVA_HOME path; BUILDING.md keeps the bookworm lines for the release machine.
  3. rustup with --no-modify-path: a second build.sh run reinstalled it. ensure_rust now adds ~/.cargo/bin first.
  4. Stale docs (troubleshooting JDK path, AGENTS.md command comment): fixed.
  5. No default toolchain outside the repo: one line in BUILDING.md (kept the flag, it saves a toolchain on disk).
- Fix review (32a91ee): PASS; its one MINOR (a temp dir left by a power cut) folded into the /simplify rewrite below.
- /simplify (9a7cfa0), applied: JDK unpacked in a fixed `jdk-*.part` beside its place (cleared by the next run, no trap/glob sweep); unused `JAVA_HOME=""` gone; `sha256sum --status`; `ensure_rust` puts ~/.cargo/bin on PATH once at the top; "(#174)" dropped from a comment; workflow and BUILDING.md wording on run time and triggers; troubleshooting path `~/Android/jdk-17*`. Re-checked by hand: bad checksum, good, again, leftover `.part` plus half JDK.
- /simplify, skipped:
  - One `fetch_pinned` helper for all four downloads (cmdline-tools, Gradle, kotlinc unchecked): rewrites three downloads outside this change.
  - curl only in `ensure_rust`: setup-android needs curl even when rustup is already installed, so the Android list keeps it.
  - curl in the apt GTK list to save a second apt run: ~20 s on a fresh machine, not worth a second list.
  - gcc instead of build-essential/base-devel: unverified whether a host build script needs C++ or make; the GTK path installs the same.
  - Android on one image only: the maintainer asked for all four.
  - build.sh refusing a release build on the downloaded JDK: behaviour beyond the proposal; maintainer: keep only the RELEASE.md check (2026-11-02).
- Scenario "keeps it" by hand: full setup-android on the laptop with JAVA_HOME unset -> JDK home /usr/lib/jvm/java-17-openjdk-amd64, env.sh byte-identical.
- Run 3 (32a91ee) and run 4 (9a7cfa0, final code): all eight jobs green.

## Hand-over
- Test steps for the maintainer:
  1. GitHub, Actions, "Fresh system build", run on 9a7cfa0: eight green jobs.
  2. Laptop (done by Claude, repeat if wanted): `build-aux/setup-android.sh` with JAVA_HOME unset logs `JDK home: /usr/lib/jvm/java-17-openjdk-amd64`, no download.
- Artifacts: none to install (scripts only). The optimised APK on the phone (debug key) was built from this tree on request; the app code is unchanged.
- Ship: one commit on beta with the files from the `fresh-build` branch plus the archived change folder; then delete the `fresh-build` branch (task 5.1).
- Proposed commit message:

      build: build.sh works on a fresh machine

      A first-time builder hit handwork on every distro: Android stopped
      without rustup, Debian 13 and Fedora 42+ have no JDK 17, and a fresh
      Ubuntu has no curl for the rustup installer. build.sh now installs
      Rust first and the tools it needs everywhere; setup-android downloads
      a pinned, checksum-checked Temurin 17 when no JDK 17 is installed. An
      installed one still wins, so release builds keep Debian 12's JDK.

      A manual workflow builds both apps on fresh Ubuntu 26.04, Debian 13,
      Fedora and Arch images.
