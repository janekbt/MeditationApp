# Tasks: Linux app on FlatPark

Base: 3bae0d5f56af7a2dcd01b811e783a40c0f1afe61; suites on it: core 1386
(+4+11+3) passed, gtk 151 passed, android --lib 236 passed

## Loop
- [x] 0 Skills loaded, again after every /compact: `/ponytail ultra`
      (no Android UI touched, no slint)
- [x] 1 Understand
- [x] 2 Propose (approved: 2026-10-18)
- [x] 3 Build
- [x] 4 Review
- [x] 5 Specs, docs, list
- [x] 6 Verify and hand over (Janek, 2026-10-18: all test steps passed)
- [x] 7 Ship

## Review Focus
- A user who has the GitHub bundle installed and adds FlatPark -> same
  app id, Flatpak refuses a second install from another origin; they
  uninstall the bundle (data in ~/.var/app stays). Goes into the README
  lines of the follow-up, no scenario.
- System-wide install (`apply_extra` runs as root, capabilities
  dropped) -> `tar --no-same-owner`; FlatPark's `check-apply-extra.sh`
  needs the published URL, so it runs before the follow-up PR.
- Another language -> wrapper sets `LOCALEDIR`; scenario 1 checks the
  system language.
- GSettings schema -> wrapper sets `GSETTINGS_SCHEMA_DIR`; scenario 1
  checks the window size survives a restart.
- Notifications and D-Bus activation -> the service file's
  `Exec=/app/bin/meditate` hits the wrapper, which has that name.
- GNOME 51 deprecations -> the GTK build is not run with `-D warnings`;
  the local Flatpak build (task 4) shows whether it compiles.
- A CI run from before this change -> scenario 3. Run against the live
  26.10.2 release through a `gh` shim (the live run was blocked).
  Tarballs are never replaced: FlatPark pins them (scenario 4).
- Locales: with separate-locales, `share/locale/<lang>` is a symlink
  into `share/runtime/locale`; check in task 4 that the tarball keeps
  it relative and it resolves under `/app/extra/meditate`.
- Notifications without the bus names -> scenario 1 checks the
  end-of-session notification.

## Tasks
1. Manifest `runtime-version` "51"; both CI jobs on
   `flatpak-github-actions:gnome-51`.
2. CI: x86_64 job sets `build-dir: flatpak_app` explicitly; both jobs
   then `tar -C flatpak_app --transform 's,^files,meditate,' -czf
   meditate-<arch>.tar.gz files` and
   upload the artifact `meditate-<arch>.tar.gz`.
3. `attach-flatpaks.sh`: download all four artifacts (bundle and
   tarball per arch) first, check each with the existing `[ -s ]`,
   then upload; the tarball as `meditate-<version>-linux-<arch>.tar.gz`.
   Today it uploads x86_64 before it downloads aarch64, so a missing
   file leaves a half-attached release (scenario 3).
4. Local x86_64 Flatpak build with GNOME 51 (one heavy step, JOBS=4),
   then the same `tar` as CI.
5. `flatpark-package/` in this folder, modelled on FlatPark's Grab
   package; local `flatpak-builder` install pointing at the tarball
   served from localhost (scenario 1), plus FlatPark's
   `audit-descriptor.mjs` and `check-apply-extra.sh` from a clone of
   their repo in ~/.cache. Their `flatpark-local` remote is removed
   afterwards. Needs GNOME 51 runtime + SDK and the rust-stable
   extension for its base (user install).
6. Docs: RELEASE.md 5b, BUILDING.md and README "GNOME 50" -> 51.

## Build notes
- No Rust code changes, so no failing-test-first step: every scenario
  is MANUAL (shell, CI and Flatpak only).
- Scenario 3: run against 26.10.2 with a `gh` shim that passes reads
  to the real `gh` and only logs `release` calls (a run against the
  live release was blocked by the permission check): stopped at
  "no artifact matches", exit 1, zero uploads.
- Task 4: GNOME 51 build ok, no Rust warnings (only cargo's old
  `config` name notice, as before). Tarball 6.2 MB; locale symlinks
  relative (`../../share/runtime/locale/...`), schema compiled.
- Task 5: package audit passes except the placeholder extra-data
  (filled at PR time). `resolve-update.sh` refuses 26.10.2 (no
  tarballs). First smoke test with FLATPAK_USER_DIR + HOME under
  ~/.cache/flatpark-test: apply_extra unpacked it, 6 s start without
  errors. That proves little (FlatPark: such an installation is unknown
  to the portal), so it was removed; scenario 1 runs in a registered
  system installation (Janek's choice, polkit instead of sudo; his
  user-installed 26.10.1 bundle stays), HOME still isolated. A system
  install of extra-data needs a signed repo: signed with a throwaway
  key in ~/.cache/flatpark-test/gpg. apply_extra ran as root with
  capabilities dropped and unpacked fine (the system-wide path
  `--no-same-owner` is for), and removed the archive.
  Scenario 1 itself is Janek's test in step 6, not done yet.
- Found on the way: the metainfo screenshot URLs pointed at
  `main/data/screenshots` (404 since the crate move); now
  `main/meditate-gtk/data/screenshots` (200). FlatPark and software
  centres show them.
- Every task 1 to 6 done.

## Review (one reviewer, findings checked against the code)
1. aarch64 GNOME 51 never started on the phone -> fixed: requirement
   and MANUAL scenario, RELEASE.md step 4 Linux check.
2. `--clobber` could replace a tarball FlatPark pinned -> fixed: digest
   compared before any upload, same kept, other stops (scenario 4,
   three fake-`gh` runs); resolver emits `size`.
3. Task 5 claimed scenario 1; FLATPAK_USER_DIR isolation unreliable ->
   fixed in the notes; registered installation for the test.
4. apply_extra unpacked over the old tree -> fixed: `rm -rf meditate`.
5. Light run picked by attach-flatpaks -> partly: the download failure
   now names the artifact and the likely cause. Picking the right run
   stays in OPEN-ISSUES #169 (not this change's scope).
6. GNOME 50 left in dev-xbuild.sh and a window.blp comment -> fixed
   (51 icons checked in the runtime).
7. OPEN-ISSUES #167 lists the screenshot fix -> fixed, item keeps only
   the Rust floor.
8. Two summaries -> fixed: the metainfo one in flatpark.yml.
9. Package metainfo release 26.10.2 -> rejected for now: filled at PR
   time with the pinned version, sha256 and size (follow-up list).
10. Scenario 3 proof says a live run -> fixed wording; a committed
    `--self-test` rejected: the script runs once per release.
Fixes 2 and 4 change behaviour -> fix review: one finding, a half
uploaded asset (state not "uploaded") was reported as "pinned" ->
fixed: own message naming `gh release delete-asset`; four fake-`gh`
cases (none 4 uploads, same 2, other 0, half 0).

## Follow-up (the FlatPark PR, after the next release, its own go)
- Fill the managed extra-data block (VERSION, sha256, size) and the
  metainfo `<release>` with the released version.
- Run `audit-descriptor.mjs`, `check-apply-extra.sh` and
  `publish.sh --verify` from a clone; remove `flatpark-local` after.
- Upstream-approval row; README install lines and the hint to
  uninstall the bundle first (same app id, data stays).

## Hand-over
Start the test copy (system install, own HOME, your data untouched):

    HOME=~/.cache/flatpark-test/home flatpak --system run io.github.janekbt.Meditate

1. The app opens in German (your system language).
2. A short timer session (1 min): the start and end bells ring, at the
   end the "session complete" notification appears.
3. Resize the window, close, start again with the same command: the
   size is kept.
After the test: `flatpak --system uninstall -y io.github.janekbt.Meditate`,
`flatpak --system remote-delete fptest`, `rm -rf ~/.cache/flatpark-test`,
stop the localhost server. (Done; ~/.cache/flatpark clone kept for the PR.)
Found in the test: GNOME 51 libadwaita warns about the bell rows in
revealers (18 lines per start, not in GNOME 50) -> OPEN-ISSUES #197.

Proposed commit:

    flatpak: Ship the app tree for FlatPark, move to GNOME 51

    FlatPark lists apps from their own release files, and it takes an
    installed app tree, not a .flatpak bundle, on the current runtime.
    CI now packs that tree per architecture and attach-flatpaks.sh adds
    it to the release, downloading and checking everything before the
    first upload and never replacing a tarball FlatPark may have pinned.
    The FlatPark package itself is drafted in the archived change folder
    for the pull request after the next release.

    Also fixes the metainfo screenshot links (404 since the crate move).
