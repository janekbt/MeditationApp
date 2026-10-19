# Screenshot links pinned to the release tag

Base: 1322734 (beta). Base tests: `cargo test -p meditate` 152 passed (run on 1322734's tree).

## Checklist
- [x] 0 ponytail ultra (+ slint:slint) loaded
- [x] 1 Understand
- [x] 2 Proposal shown, go (2026-10-19, plus Stats/Log screenshots)
- [x] 3 Build, test first
- [x] 4 Review, then /simplify
- [x] 5 Specs, docs (RELEASE.md bump + beta notes; FlatPark memory note)
- [x] 6 Verify and hand over (store check after the next release)
- [x] 7 Ship (2026-10-19)

## Review Focus
- The About dialog reads the raw `.xml.in`: links must stay plain text there.
- Bump twice in one month: the links move from the previous tag, not only from `main`.
- The tag only exists after the release is pushed; the store reads released metainfo only.

## Tasks
- [x] T0 Stats/Log composites replace Guided/Box Breath (README, metainfo, nine .po, FlatPark draft)
- [x] T1 test: links name `v<newest release>`; fails on `main` links; then the metainfo
- [x] T2 bump-version.sh moves the links; manual run on a scratch branch

## Progress
- T0: deleting the wrong composites turned the existing test red ("composite-guided.png
  is missing"); links and captions moved, green. Log caption revived from its old
  translations; Stats caption fuzzy in all nine, rewritten with each language's
  "Daily goal" term; msgfmt --check ok. Draft block regenerated (still `main`), validate ok.
- T1: test rewritten for the tag; red on the `main` links ("is not at v26.10.2"); links
  to v26.10.2, green. Mutation: newest release marked development -> fails
  "is not at v26.10.2-beta". Restored.
- T2: bump-version.sh rewrites the ref. Manual run in a throwaway worktree with
  `bump-version.sh 26.10.9`: all three links at v26.10.9, worktree removed.
  RELEASE.md beta notes: add `-beta` to the links.

## Review 1 (fresh agent): PASS, four MINOR
1. New PNGs untracked, `commit -a` would miss them: at ship they're added by name.
2. RELEASE.md step 2 didn't mention the links: fixed.
3. FlatPark note missing: added to the FlatPark project memory (PR-time checklist).
4. Beta rule only tested by a one-off mutation: `screenshot_tag` pulled out, new test
   `a_beta_links_its_beta_tag` on a fixture; mutation (no `-beta`) fails it. 153 passed.

## /simplify (4 agents)
- Efficiency: clean.
- Skipped: reuse core's release parser (`attr` in release_notes.rs) for `screenshot_tag`;
  it would make a private parser public API only for a test (two agents, same point).
- Skipped: one `format!` with a suffix instead of if/else; the branch reads clearer.
- Skipped, for Janek: `bump-version.sh --beta` doing the three beta hand edits at once
  (beyond this change).

## Hand-over
- `cargo test -p meditate` 153 passed, clippy clean, check-specs ok.
- Manual after the next release: GNOME Software shows Timer, Stats, Log (light/dark).
- Ship: add composite-stats.png and composite-log.png by name.

Proposed commit message:
    flatpak: Pin store screenshots to the release tag, show Stats and Log

    Screenshots linked from main vanish from a shipped release's store page
    when files move, as 26.10.2's did. Each release now links its own tag;
    bump-version.sh moves the links, a beta adds -beta by hand.
    The Linux shots now match Android: Timer, Stats and Log.
