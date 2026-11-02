# Slim Flatpak tree

## Why
FlatPark's review of the package (flatpark#920, note 1) found build-time
leftovers in the app tree every Linux user downloads: blueprint-compiler
with its Python files (about 1.5 MB) and flatpak-builder's
`manifest.json` (425 KB). The app never runs them.

## What Changes
- The blueprint-compiler module is cleaned out of the installed tree, so
  neither the FlatPark tarball nor the `.flatpak` bundle carries it.
- The FlatPark tarball leaves out `manifest.json` (the bundle keeps it,
  as every Flatpak does).
- Left out: review note 2 (the Secret Service grant stays: without it a
  desktop with no Secret portal can't store the Nextcloud password) and
  note 3 (the resolver regex lives in FlatPark's repo, a separate
  one-line PR there).

## Decisions
- Do notes 1 and 3, keep the grant from note 2 (Janek, 2026-11-02).
- `share/app-info`, also named by the reviewer, stays: flatpak-builder
  writes it for the bundle, it is small, and the reviewer only asked
  for the two items above (my call).
- The CI run that proves it is dispatched on a side branch
  `slim-flatpak-tree`, since the Flatpak builds only run in CI; `beta`
  gets the commit only after Janek's test.

## Capabilities
### Modified Capabilities
- `distribution`: the shipped tree carries no build tools

## Impact
- Core: none
- GTK: none (only `build-aux/io.github.janekbt.Meditate.json`)
- Android: none
- Translations: none
- Data: none
- Android build environment: untouched
- CI: `.github/workflows/flatpak.yml`, both tarball `tar` lines
- Docs: none (RELEASE.md 5b describes the tarball's layout, which stays)
