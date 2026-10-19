# Store screenshots

Base: 05d5876 (beta). Base tests: `cargo test -p meditate` 151 passed.

## Checklist
- [x] 0 ponytail ultra (+ slint:slint) loaded
- [x] 1 Understand
- [x] 2 Proposal (short form; captions and FlatPark draft approved 2026-10-19 in chat)
- [x] 3 Build, test first
- [x] 4 Review, then /simplify
- [x] 5 Specs, docs (RELEASE.md screenshot step; check-specs ok)
- [x] 6 Verify and hand over (store check only possible after the next release)
- [x] 7 Ship (2026-10-19, old July screenshots deleted on Janek's go)

## Review Focus
- Another language: captions in all nine .po files, `msgfmt --check`.
- The FlatPark draft keeps everything except the screenshot block.

## Tasks
- [x] T1 `metainfo_screenshots_exist_in_the_repo`, then point the metainfo at the composites
- [x] T2 captions translated (update-translations.sh, nine .po)
- [x] T3 FlatPark draft screenshot block from the translated metainfo

## Progress
- T1 test passed at once: beta already had the right folder (15554f6). Mutation: one
  link back to the old `data/screenshots/` path -> fails "is missing". Restored.
  Metainfo now links the three composites; `appstreamcli validate --no-net` ok.
- T2 update-translations.sh; the old captions dropped (two kept as obsolete `#~`),
  nine translations written with each language's existing terms; `msgfmt --check` all ok.
- T3 msgfmt --xml on the base reproduces the draft's screenshot block byte for byte;
  the block from the new files replaced it; validate ok. Nothing else in the draft changed.
- `cargo test -p meditate`: 152 passed. check-specs ok.

## Review 1 (fresh agent): PASS, three MINOR
1. Test checks the file on disk, not that git tracks it: rejected; the crate's tests
   also run outside a git checkout (source tarball), and the composites are tracked.
2. RELEASE.md screenshot step named only fastlane: fixed, Linux composites added.
3. Test doesn't check the FlatPark draft: rejected; the draft lives in the archive
   (history), and a test must not read archived files.

## /simplify (4 agents)
- Reuse, efficiency: clean.
- Fixed: the test no longer pins the screenshot count (`!images.is_empty()`).
- Skipped, for Janek: pin the links to the release tag instead of `main`, so a later
  move can't break shipped releases again (a new approach, beyond the approved scope).
- Skipped, for Janek: delete the unused July `screenshot-*.png` (deleting needs a go).

## Hand-over
- After /simplify: `cargo test -p meditate` 152 passed, clippy (meditate, meditate-core,
  all targets) no warnings, check-specs ok.
- Manual check, after the next release: open Meditate in GNOME Software; it shows the
  three light/dark screenshots with captions in the system language.

Proposed commit message:
    flatpak: Show the current light/dark screenshots in the store

    26.10.2 links the old screenshot folder, so the store shows none, and the
    images on beta predate Guided and Box Breath. Link the README composites
    with new captions in all nine languages, also in the FlatPark draft.
