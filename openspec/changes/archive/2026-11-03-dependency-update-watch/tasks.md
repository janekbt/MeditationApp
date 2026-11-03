# Tasks: dependency-update-watch

Base: ecd6e44b4b7b3793baf162db25435ccb4a2a0394; suites on it: not run, the change adds only a workflow, a config file and docs (no Rust, no build script)

## Loop
- [x] 0 Skills loaded, again after every /compact: `/ponytail ultra`; `slint:slint` (loaded, no Android UI touched)
- [x] 1 Understand
- [x] 2 Propose (approved: 2026-11-03)
- [x] 3 Build
- [x] 4 Review, then `/simplify` after the last review
- [x] 5 Specs, docs, list (no OPEN-ISSUES item)
- [x] 6 Verify and hand over (maintainer approved 2026-11-03)
- [x] 7 Ship

## Review Focus
- A pin whose line format changes later -> not a scenario; RELEASE.md sweep checks "Detected dependencies"
- Slint's patched crates in `third_party/` (path patches) -> "The test run lists every pin": slint shows from meditate-android's Cargo.toml; the path crates must not appear as crates.io updates or error
- The repro-frozen pins (Rust 1.95.0, NDK r27c, Gradle 8.5, AGP 7.3, JDK 17) -> listed like any other; the dashboard header says Android pins need the repro probe and a go
- A ticked box -> no permission to push; the header says the boxes do nothing
- `main` lagging behind `beta` -> the config names `beta` as base branch
- Run fails -> red scheduled run, no scenario (GitHub's own notice)

## 1. The workflow and config, checked on a side branch
- [x] 1.1 `.github/workflows/renovate.yml`: schedule Monday 05:00 UTC, workflow_dispatch, push to `renovate-test`; permissions contents read, issues write, pull-requests read; renovatebot/github-action pinned to a commit, `renovate-version` exact, `configurationFile` set. Proof: run on `renovate-test` is green
- [x] 1.2 `.github/renovate-config.json5` as design.md lists it. Proof: scenario "The test run lists every pin", each missing pin fixed and rerun
- [x] 1.3 Proof: scenario "Newer versions appear, nothing changes"
- [x] 1.4 Delete `renovate-test` after the last green run; the dashboard issue stays

## 2. Docs
- [x] 2.1 BUILDING.md CI section; RELEASE.md sweep line. Proof: grep "renovate" in both

## Notes
- Pre-build agent questions, my call: ignore `third_party/**`; one line per update, no groups; JDK 17.x only; NDK from the pin's comment tag, Gradle/rust-build.sh copies not watched; Gradle wrapper and PINNED_GRADLE both listed; Kotlin plugin and compiler watched separately; `gnome-51` container disabled, RELEASE.md line instead; exact Renovate version; `pull-requests: read`; whole config in the workflow's file since repo config is only read from `main`; lock-file maintenance "at any time".
- Test runs on `renovate-test` (commits made with a temporary index, beta's working tree untouched):
  - 3cf0f0f: run green but Renovate aborted, "external host error": gitlab.gnome.org's tag API answered 503 (also to curl with per_page=100). Fix: github-tags on GNOME's GitHub mirror for runtime and blueprint-compiler.
  - 83d58be: green, issue #21 "Dependency Dashboard" created; every pin detected with today's version (crates of all three manifests, lock-file line, rust 1.95.0, AGP 7.3.0, Kotlin plugin 1.7.20, wrapper and PINNED_GRADLE 8.5, kotlinc 1.9.25, java 17.0.21+9 (no update: newest 17 on Adoptium), GNOME runtime 51 (newest), blueprint-compiler 0.20.4, the actions). NDK showed "27c -> 27", wrong.
  - 2d469d7: NDK major only, shows "27 -> 30". No `renovate/` branch, no PR (`git ls-remote`, `gh pr list`).
  - Also listed: `java-jdk 21` from flatpak.yml's setup-java (CI's non-repro APK); kept, it's real.
- Review (fresh agent) findings, all fixed:
  1. Renovate exits 0 when it aborts (run 41608396476 was green with "external-host-error"): the workflow now writes Renovate's log to a file (`LOG_FILE`) and a step greps for "Repository result: done". Success path seen green on 1cd8d57's run; the failure path could not be replayed (gitlab.gnome.org answered 200 again by then), the abort line it guards against is in the debug log of run 2 (fc57591).
  2. "A ticked box turns the next run red" was untested: header and docs now say the boxes do nothing.
  3. setup-java's JDK 21 is watched on all majors: said in BUILDING.md and the proposal.
  4. proposal named `.github/renovate.json`: fixed.
  5. NDK is read from the comment: config comment and RELEASE.md say it must match PINNED_NDK.
  6. Wrapper and PINNED_GRADLE shared one line: PINNED_GRADLE is now "gradle (setup-android)".
- Fix review (fresh agent): the six fixes hold, the check step's path, env passthrough and message text confirmed from the logs. Two minor findings fixed: the config comment still said a ticked box turns runs red (now "creates nothing"); a vanished source (404) is only a warning, so RELEASE.md's sweep also checks that the issue has no "Repository problems" section and BUILDING.md says so. Wording only, no further review.
- Not tested: what a ticked box does with a read-only token (per-branch error or abort); the header says not to rely on the boxes either way.
- /simplify (4 agents), applied: NDK read from PINNED_NDK's major instead of the comment (the RELEASE.md "comment must match" line goes); the gnome-NN container compared with regex versioning instead of disabled (shows `gnome-51`, no update yet); `platform` and `dependencyDashboardTitle` removed (Renovate defaults); NDK's packageName = depName removed. Run da8a0d4 green, both lines in the issue, no branch, no PR.
- /simplify skipped: `# renovate:` comments in setup-android.sh to merge three managers (touches the repro script for ~20 lines of config); Renovate's JSON report to fail on zero matches or problems (option not verified for 44.171.1, more code; RELEASE.md's eye check stays); sparse checkout and a shorter timeout (seconds a week); deduplicating the "only reads" sentence across workflow header, BUILDING.md and the issue header (each reader sees only one).

- After hand-over, maintainer asked for NDK patch releases (2026-11-03, go): custom datasource `android-ndk` over the GitHub releases API, JSONata maps r27 -> 27.0, b -> .1, c -> .2, d -> .3; PINNED_NDK read as major.minor. Run d329193 green: "android/ndk 27.2 -> 27.3, 30.0"; no branch. Local JSONata check not possible (no npm; fetching jsonata from a CDN was blocked), so the run was the test.
- Fix review (fresh agent) of the NDK addition: mapping checked against all 13 GitHub releases' x.y.build (all match), API calls carry the token. Minor findings fixed: a letter past the table would have given "27."; the letter's position in the alphabet now replaces the table. BUILDING.md says where the full build number is.
- /simplify of the NDK addition (4 agents): applied the altitude finding, the version comes from each release's notes (`\d+.\d+.\d{7,}`), PINNED_NDK matched whole; the letter mapping, its comment and the double `$match` went with it. Run 950abbb green: "android/ndk 27.2.12479018 -> 27.3.13750724, 30.0.16248370". Skipped: tags endpoint instead of releases (smaller payload, loses the prerelease flag and dates, a few hundred KB a week); dropping `versioningTemplate: loose` (default not verified for this format).

## Hand-over
- Nothing to install: the check runs on GitHub. Look at issue #21 (https://github.com/janekbt/MeditationApp/issues/21): the header text, the "Pending Approval" list (one line per update) and "Detected Dependencies".
- Commit message (proposed):
  ci: weekly list of available updates

  New versions of the crates, the Rust toolchain, the Android pins, the
  GNOME runtime and the GitHub Actions were only noticed by chance.
  Renovate now runs as a GitHub Action every Monday and keeps one issue
  listing them. It only reads: no branch, no pull request, so every pin
  still moves only on purpose. The schedule starts once the workflow is
  on main.
- After the commit: delete the `renovate-test` branch (task 1.4).
