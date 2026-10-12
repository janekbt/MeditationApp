# Contributing

Meditate is a meditation timer for Linux (GTK and libadwaita) and
Android (Slint), both built on one shared Rust core. It is published
on F-Droid and as Flatpak bundles, and people's practice history is in
it. It's a small project run by one maintainer: patches are welcome,
expect a round of feedback.

## Before you start

- Read [`AGENTS.md`](AGENTS.md): the rules, the commands and which doc
  to update for what. Where another doc disagrees, it wins.
- Every change follows the `/spec` loop
  ([`.claude/skills/spec/SKILL.md`](.claude/skills/spec/SKILL.md)),
  with or without an AI tool: a change folder in `openspec/changes/`
  with WHEN/THEN scenarios, the maintainer's go on the proposal, tests
  first, then the code.
- A bigger idea, or unsure whether it fits? Open an issue first.
- Setting up and building: [`BUILDING.md`](BUILDING.md). Why the rules
  exist: [`DECISIONS.md`](DECISIONS.md). A new synced kind of item:
  [`ENTITIES.md`](ENTITIES.md). The core crate:
  [`meditate-core/README.md`](meditate-core/README.md).

## Sending a change

- Open a pull request against `beta`, never `main`, with the change
  folder in it. One logical change per pull request.
- Before opening it, the commands under "Commands" in `AGENTS.md`
  pass, clippy is warning-free and `build-aux/check-specs.sh` passes.
- Green tests alone don't merge: the maintainer tries every change on
  a phone or laptop first.
- New or changed text the user sees needs all nine translations, in
  both apps where it appears.
- Don't touch the Android build environment (the list in `AGENTS.md`)
  without asking: F-Droid only ships the app while its own rebuild
  matches byte for byte.

## Commit messages

`<area>: <change>`, short; the body says why when it isn't obvious.
Common areas: `android`, `gtk`, `core`, `core+gtk`, `sync`, `docs`,
`backlog`. `Fixes #N` only for real GitHub issues, never for the
numbers in `OPEN-ISSUES.md`. Examples from `git log`:

```
sync: say what stops a sync, keep accounts apart (batch F)
backups: restore without doubles, ask before partial imports (batch D)
```

## What lands easily

Bug fixes with a regression test, tighter types (newtypes, enums
instead of bools or strings), moving rendered text out of core into
typed `*Key` enums, and doc fixes.

## Reporting a bug

Open an issue with what you did, what you expected, what happened,
the app and its version, and the diagnostics log:

- Flatpak: `~/.var/app/io.github.janekbt.Meditate/data/meditate/diagnostics.log`
- other Linux installs: `~/.local/share/meditate/diagnostics.log`
- Android: About, then Diagnostics, then Share.
