# Tasks: Slim Flatpak tree

Base: 63fa7f94ad910f9adcfef5a79919932e1c6053cf; suites not run: the
change touches no Rust (only the Flatpak manifest and the CI workflow)

## Loop
- [x] 0 Skills loaded, again after every /compact: `/ponytail ultra`,
      `slint:slint` (hook; no Android UI touched)
- [x] 1 Understand
- [x] 2 Propose (approved: 2026-11-02)
- [x] 3 Build (no Rust: config only, proofs are the MANUAL scenarios)
- [x] 4 Review (one fresh agent, 2 findings; /simplify, nothing applied; see below)
- [x] 5 Specs, docs, list (check-specs passes; no doc names the removed files; no OPEN-ISSUES entry)
- [x] 6 Verify and hand over (Janek, 2026-11-02: laptop test passed)
- [x] 7 Ship

## Review Focus
- Something at runtime that came from blueprint-compiler's module ->
  none: the `.blp` files compile to the GResource at build time, and
  the app is Rust with no Python. Scenario 2 is the proof.
- The aarch64 job builds with its own `flatpak-builder` call, not the
  action -> it reads the same manifest; its `tar` line gets the same
  exclude. Scenario 1 lists both arches.
- `cleanup` runs after all modules are built -> blueprint-compiler is
  still there when the meson build of `meditate` needs it.
- A tarball already pinned by FlatPark -> untouched; the change only
  affects the next release's files.

## Pre-build questions (fresh agent), answered
- Spec said bundle too, proposal says bundle keeps `manifest.json` ->
  requirement now names the tarball only (my call).
- `__pycache__` written while the meditate module builds escapes the
  module cleanup -> top-level `/lib/python*` as well (my call).
- `--exclude` position and name -> before the operand, `files/...`.
- Does FlatPark's package read `manifest.json`, `app-info` or Python? ->
  no: grep of `registry/io.github.janekbt.Meditate/` finds none of them.
- Reviewer and `share/app-info` -> the notes are non-blocking; no PR
  depends on it.

## Review findings
- 1 (aarch64 tarball not listed yet) -> holds; it is task 4, before ship.
- 2 (tar `--exclude` unanchored, would also drop a deeper
  `files/manifest.json`) -> rejected: no such path in the tree, and
  `--anchored` would mean another CI run for a hypothetical case.

## /simplify (4 agents): nothing applied
- Top-level `/lib/python*` overlaps the module `["*"]` (2 agents) ->
  skipped: it catches `__pycache__` written while the meditate module
  runs the compiler, which belong to that module; proving it redundant
  needs another CI run.
- One shared pack step for both `tar` lines, or remove `manifest.json`
  at the source -> skipped: the duplication predates this change, and
  flatpak-builder writes `manifest.json`, which the bundle keeps.
- Reuse, efficiency: none.

## Tasks
- [x] 1 Manifest: `"cleanup": ["*"]` on the blueprint-compiler module, and
      `/lib/python*` added to the top-level `cleanup` (the meditate module
      runs the compiler, so `__pycache__` files written then belong to it)
- [x] 2 Workflow: `--exclude=files/manifest.json` on both tarball `tar` lines,
      before the `files` operand (tar matches the name before `--transform`)
- [x] 3 Push the side branch `slim-flatpak-tree`, dispatch Flatpak CI on it (scope all)
- [x] 4 Scenario 1: list both tarballs (run 41448970328: neither has blueprint-compiler, python or manifest.json; same 101 entries; bin/meditate is ARM aarch64; x86_64 6.27 -> 5.86 MB)
- [x] 5 Scenario 2: Janek installs the x86_64 bundle and runs a session (passed 2026-11-02)

## Commit message
flatpak: Drop build tools and the manifest from the app tree

FlatPark's review (flatpark#920, note 1): blueprint-compiler, its Python
files and manifest.json were shipped but never run.
