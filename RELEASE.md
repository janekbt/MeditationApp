# Release runbook

How to cut a release of both shells. Written for an AI operator
(Claude or similar) working with Janek; a human can follow it too.
Steps marked **[judgment]** need actual reading/writing/verifying,
not just command execution — do them properly, they are the point
of this document. Everything happens on `beta`; `main` only moves
at the very end.

One version number covers the whole repo (date scheme `yy.m.p`,
Android `versionCode` = `yymmpp`). Both shells bump together even
if only one changed — same commit, same version, no drift.

## 0. Preconditions

- Working tree clean, `beta` up to date with `origin/beta`.
- Ask Janek what this release is (or read it from the work just
  landed) — you need that for the release notes.

## 1. [judgment] Pre-release sweep

Do these BEFORE bumping, so fixes land in the release:

- **Untranslated strings:** every user-visible string added since
  the last release must be in the Tr catalogue / @tr() and have
  entries in ALL TEN Android po files (`meditate-android/lang/`)
  and the GTK po files (`meditate-gtk/po/`) where applicable.
  Check coverage — every msgid in `lang/de/...po` must exist with
  a non-empty msgstr in the other nine; write the missing
  translations yourself (match each language's established
  terminology; correct plural forms — fr/pt_BR `n>1`, pl/ru three
  forms, zh_CN one). Validate every file:
  `msgfmt --check -o /dev/null <file>`.
- **README.md:** read it end to end; new features present, stale
  claims gone, links resolve.
- **AUDIT.md open items:** anything that should block a release?
- **cargo-sources.json:** if `Cargo.lock` changed since the last
  regeneration (`git log -1 -- build-aux/cargo-sources.json` vs
  `git log -1 -- Cargo.lock`), regenerate — offline Flatpak CI
  fails on missing crates otherwise:
  `flatpak-cargo-generator.py Cargo.lock -o build-aux/cargo-sources.json`
  (script: flatpak/flatpak-builder-tools; on machines without the
  `toml` module, shim it over stdlib `tomllib` — read-only use).
- **Screenshots:** if the UI changed visibly, recapture the
  fastlane screenshots for all 10 locales. Set the app language
  per locale with
  `adb shell cmd locale set-app-locales io.github.janekbt.Meditate --locales <tag>`,
  `am force-stop` + relaunch between locales (the recreate races
  you otherwise), NEVER send taps without first confirming
  Meditate is the foreground app (`dumpsys window`, current
  focus), and verify no locale leaked the wrong language
  (pixel-diff the nav strip across locales — identical nav strips
  mean a leak). Reset with the same command minus `--locales`.

## 2. Bump

    build-aux/bump-version.sh          # or: bump-version.sh yy.m.p

Stamps gradle, both Cargo.tomls, meson, the Flatpak manifest,
and inserts skeletons for the metainfo release entry and the
en-US fastlane changelog. Run `cargo check --workspace` once so
Cargo.lock picks up the crate version bumps.

## 3. [judgment] Release notes

- Write the real notes into the metainfo `<release>` entry
  (`meditate-gtk/data/io.github.janekbt.Meditate.metainfo.xml.in`
  — replace the TODO; user-facing tone, no commit-speak) and into
  `fastlane/metadata/android/en-US/changelogs/<versionCode>.txt`
  (≤ 500 chars, bullet style — look at 260701.txt).
- Translate the fastlane changelog into ALL nine other locale
  dirs (`de-DE`, `es-ES`, `fr-FR`, `it-IT`, `nl-NL`, `pl-PL`,
  `pt-BR`, `ru-RU`, `zh-CN` — same `<versionCode>.txt` name).
- GTK metainfo release notes are translated through the gettext
  pipeline: regenerate the pot/po (`ninja meditate-pot` +
  `msgmerge` or meson's update-po target) and translate the new
  entries in each `meditate-gtk/po/*.po`.

## 4. Verify

All must pass before anything is tagged:

    cargo test -p meditate-core -p meditate-android --lib
    cargo test --workspace          # GTK shell included
    cd meditate-android/android && . ~/.config/meditate-android/env.sh \
        && ./gradlew :app:assembleRelease   # exit-code gate, never grep

- appstream sanity: `appstreamcli validate --no-net
  meditate-gtk/data/io.github.janekbt.Meditate.metainfo.xml.in`
  (CI runs this too — fail here, not there).
- Commit everything on beta (terse message, e.g.
  `release: 26.8.1`), push, then dispatch Flatpak CI on beta:
  `gh workflow run flatpak.yml --ref beta` and WAIT for green
  (~35 min; poll `gh run view`). Do not proceed on red.
- **[judgment] On-device check:** install the release APK on the
  FP5 (`adb install -r .../app-release.apk`) and let Janek
  confirm the release works before tagging — his standing rule is
  release builds go on only at release time, debug builds while
  iterating.

## 5. Tag and publish (needs Janek's explicit go)

    git checkout main && git merge --ff-only beta
    git tag v<version>
    git push origin main --tags
    git checkout beta

- F-Droid picks the new tag up automatically once the app is in
  fdroiddata (`UpdateCheckMode: Tags`). Until first inclusion,
  submission is manual — see `build-aux/fdroid-metadata-draft.yml`.
  fdroiddata build entries must pin the FULL commit hash, never
  the tag name (maintainer rule).
- Flathub: release notes travel in the metainfo; the Flathub repo
  update is its own flow (out of scope here).

## 6. Afterwards

- Verify `git describe` on main says the new tag.
- Update any project memory / plan docs that track "latest
  shipped version".
- Back on `beta` for further work.

## Known traps (learned the hard way)

- `versionCode` must strictly increase; the script enforces it.
- Slint bundled-translation selection needs the FULL locale tag
  (pt-BR → pt_BR bundle, then base language) and must run before
  any Rust-composed string is built — both already handled in
  `build_ui`, don't reorder.
- The mode-chip row clips long labels — keep chip translations
  compact ("Resp. cuadrada", "Кв. дыхание"), full names belong to
  the running view.
- A stale `cargo-sources.json` or a moved meson path fails CI ~10
  minutes in; the pre-release sweep exists because of both.
- gradle builds: gate on exit code, never on grepping the log.
