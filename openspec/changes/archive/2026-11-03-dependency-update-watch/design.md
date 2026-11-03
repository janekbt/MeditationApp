# Design: dependency update watch

## Context
A CI tool, not an app or build dependency: nothing it does ships or
changes a built file. It still runs third-party code with a token on the
repo, so it gets the design note a new dependency gets.

## Goals / Non-Goals
- Goal: one place, weekly, that lists what could be updated.
- Non-goal: automatic updates, pull requests, e-mail.

## Decisions
- `renovatebot/github-action`, pinned to a commit like fresh-build.yml's
  checkout, running the Renovate image the action names. Rejected: the
  Mend-hosted app (gives a company write access), Dependabot (no custom
  pins, PR per update), an own script (re-builds Renovate).
- Token: the workflow's `GITHUB_TOKEN`, permissions `contents: read`,
  `issues: write`, `pull-requests: read` (Renovate lists PRs each run). Rejected: a personal token (expires, acts as the
  maintainer), an own GitHub App (setup for PRs nobody needs yet).
- One config file, `.github/renovate-config.json5`, handed to the action as
  its `configurationFile`, with `requireConfig: "ignored"` and
  `onboarding: false`, so the branch the workflow runs on carries the
  config (a repo config is only read from `main`). In it: repo
  `janekbt/MeditationApp`, base branch `beta`,
  `dependencyDashboard: true`, `dependencyDashboardApproval: true` (so
  nothing becomes a branch), `lockFileMaintenance` on with schedule "at any
  time" (its default, before 4am Monday, would hide it), `ignorePaths`
  `third_party/**`, JDK limited to 17.x, the `gnome-51` container compared by `regex:^gnome-(?<major>\d+)$`,
  no `groupName` (it would merge crates into one line), the built-in
  `rust-toolchain` and `gradle-wrapper` managers, and `customManagers`
  (regex) for the whole `PINNED_NDK`, against a custom datasource that
  reads android/ndk's GitHub releases and takes the sdkmanager version
  from each release's notes, `PINNED_GRADLE`, `PINNED_KOTLIN`, `PINNED_JDK`, the
  Flatpak `runtime-version` (github-tags of GNOME's mirror
  GNOME/gnome-build-meta, major only, no alpha/beta/rc) and the
  blueprint-compiler URL (github-tags GNOME/blueprint-compiler, `v`
  stripped); gitlab.gnome.org's API answered 503. Option
  names as the pinned Renovate version spells them.
- NDK tags carry a letter (`r27c`) that no versioning compares (`loose`
  ranks `27c` below `27`); every stable release's notes carry the
  sdkmanager version (`27.2.12479018` for r27c), so the datasource reads
  that, and the dashboard shows the exact value to pin. Rejected: majors
  only (misses LTS patches such as r27d); letter-to-minor mapping (no
  build number); Google's SDK XML (no XML format in custom datasources).
  A release whose notes lack the number is dropped, not misread.

## Risks / Trade-offs
- A pin line changes its format and the regex stops matching: Renovate
  says nothing. Mitigation: RELEASE.md's sweep checks the dashboard's
  "Detected dependencies" list.
- The run fails (network, a Renovate bug): Renovate itself exits 0 on a
  host error, so the workflow greps Renovate's log file for "Repository
  result: done" and turns red without it; GitHub shows it in Actions and
  mails whoever last changed the schedule.
- Mend changes Renovate's licence or the image: the pinned commit keeps
  working until we bump it.
