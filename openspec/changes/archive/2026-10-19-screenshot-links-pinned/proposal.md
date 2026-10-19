# Screenshot links pinned to the release tag

## Why
The metainfo links screenshots on `main`, so every shipped release shows
whatever `main` holds today. 26.10.2 lost its screenshots when a folder
moved; any later move or rename would break all past releases again.

## What changes
- Janek (2026-10-19): the Linux Guided and Box Breath composites were the
  wrong screens; Stats and Log replace them, as on Android, in the README,
  the metainfo and the FlatPark draft. Captions: the Log one from before
  (translations kept), the Stats one says "Daily goal" (my call: the
  screen shows the daily goal, not a weekly one), nine translations.
- The links use the release tag (`.../MeditationApp/v26.10.2/...`)
  instead of `main`. `bump-version.sh` moves them to the new tag when it
  adds the release skeleton, so each release links its own files forever.
- The test checks that every link names the tag of the newest release in
  the metainfo, and a file in the repo.
- A beta (`type="development"`, tag `v<version>-beta`) links its `-beta`
  tag; the test fails until the links match, and RELEASE.md's beta notes
  say to add `-beta` to them.

## Decisions
- My call: "newest release" is the first `<release>` entry, the order
  the bump script writes and the About dialog reads.
- My call: the bump script replaces any ref (`main` or an older tag) in
  the screenshot URLs with one pattern.
- My call: between the bump and the tag push, beta builds show no store
  images; only released metainfo matters.
- My call: the bump script rewrites the links (no meson substitution:
  the About dialog and its test read the raw `.xml.in`).
- My call: the FlatPark draft keeps `main` for now; its links get the
  tag at PR time together with its `<release>` entry (already a PR-time
  step), noted in the project's FlatPark notes.
- Not done: fixing 26.10.2 itself; a shipped metainfo can't change.
