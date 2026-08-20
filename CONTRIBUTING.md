# Contributing

This is a small, owner-operated project — patches are welcome but
expect feedback rounds before merge. Read these short docs before
opening a substantial PR; they save back-and-forth:

- [`README.md`](README.md) — what the app is, how to install/build.
- [`BUILDING.md`](BUILDING.md) — cross-compile + Librem 5 deploy +
  Android xbuild quirks.
- [`DECISIONS.md`](DECISIONS.md) — six standing design rules
  (decisions-in-core, typed i18n keys, no schema back-compat,
  CLOCK_BOOTTIME, etc.).
- [`ENTITIES.md`](ENTITIES.md) — adding a sync-able entity (9
  surfaces, easy to miss `wipe_local_event_log`).
- [`meditate-core/README.md`](meditate-core/README.md) — entry
  point for the portable core crate.

## Commit-message style

Imperative, short summary, `<area>: <change>` prefix when one
applies. Body explains the *why* when non-obvious; the diff
already shows the *what*. Recent commits are the canonical
examples (`git log --oneline -20`):

```
core+gtk: propagate BellSoundUuid + VibrationPatternUuid to cross-references
docs: ENTITIES.md walkthrough for adding a sync-able entity
core: import_sessions_csv rejects unparseable start_iso
```

Common area prefixes: `core`, `gtk`, `core+gtk`, `docs`,
`backlog`, `sync`. No prefix is fine when none fits.

One logical change per commit. Bundling unrelated work makes
review harder and roll-back messier.

## Tests

- `cargo test --workspace` must pass before any commit lands on
  `beta`.
- New behaviour in `meditate-core` gets a unit test in the same
  module. The bar is "the test would catch a future refactor
  silently breaking this," not "every line is covered."
- For shell-side UI changes that affect haptic, vibration, or
  playback behaviour: build, deploy to the Librem 5, and verify
  on-device before committing. See [`DECISIONS.md`](DECISIONS.md)'s
  "on-device test before commit" rule.

## Branches

- `main` is the always-shippable trunk.
- `beta` is the working branch — all in-progress work lands
  here, then ff-merges to `main` at release boundaries.
- Feature branches off `beta` are fine for substantial work;
  short-lived changes can land directly on `beta`.

Push to `main` only when explicitly cutting a release. If you're
not sure, push to `beta`.

## What gets refused

- **Schema or wire-format changes without a migration.** The app is
  published, so users have data that must survive the update. See
  [`DECISIONS.md`](DECISIONS.md) rule 3. Additive changes still need
  no migration; breaking ones ship the upgrade path in the same
  commit, tested against a DB from the previous release.
- **Logic decisions added to the gtk shell** when they could live
  in `meditate-core`. See [`DECISIONS.md`](DECISIONS.md) rule 1.
  The Android shell will need the same decision; doing it twice
  is the smell.
- **Rendered strings returned from `meditate-core`.** Translatable
  copy must travel as a typed `*Key` enum; the shell maps each
  variant to `gettext`. See [`DECISIONS.md`](DECISIONS.md) rule 2.
- **Warning suppression.** No `#[allow(dead_code)]`,
  `#[allow(unused)]`, etc. Fix the cause: `#[cfg(test)]` for
  test-only items, delete if truly dead, restructure otherwise.
- **`Instant::now()` for timer-affecting code.** Use
  `crate::time::boot_time_now()` so timer math survives system
  suspend.

## What lands easily

- Bug fixes with a regression test.
- Tightenings of the type surface (newtype wrappers, enum
  variants instead of bool/string params).
- i18n improvements that move rendered strings out of core into
  typed `*Key`s.
- Performance fixes with a `prepare_cached` flip or an unbounded-
  alloc reduction.
- Doc additions that capture tribal knowledge as repo files.

## Reporting issues

Open an issue with:
- What you tried (exact command / UI sequence).
- What you expected.
- What happened.
- Diagnostics log if applicable — `~/.var/app/.../diagnostics.log`
  on Flatpak, `~/.local/share/meditate/diagnostics.log`
  otherwise.
