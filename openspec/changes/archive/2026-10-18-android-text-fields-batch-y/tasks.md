# Tasks: Android text fields, keyboard cursor and copy bar (batch Y, part 2)

Base: 3dacff2d5baef672351696460ef060fdbeee33c0; suites on it: green at 3dacff2
(android --lib 226, core 1378+18, GTK 151, all clippy clean; run for part 1,
nothing changed since).

## Loop
- [x] 1 Understand
- [x] 2 Propose (approved: 2026-10-18; MIT-0 already accepted with slint-ui/slint#12932)
- [x] 3 Build
- [x] 4 Review
- [x] 5 Specs, docs, list
- [x] 6 Verify and hand over (maintainer tested the final 15:29 build on the FP5: all fine)
- [x] 7 Ship

## Review Focus
- The IME calls `setSelection` inside a batch with a text change -> the pending full update already carries the selection; endBatchEdit takes that path first (1.2).
- `setSelection` inside a batch without a text change -> endBatchEdit reports the selection (1.2).
- Rust echoes the cursor back through `set_imm_data` -> Java's stored selection already matches, so no loop and no `restartInput` (1.2, checked in the log).
- A composing span during the move, or one Gboard just finished without telling Rust -> full update (proposal decision); scenario: type a word with suggestions on, then slide at once.
- An echo from Rust for an older step arriving after a newer `setSelection` -> last write wins, the same as for text today; echoes arrived within 2 ms in the spike log, steps are 50-70 ms apart. Checked in the fixed build's log.
- Read-only field with a connected keyboard -> selection may move, text can't change.
- Offsets: Java sends UTF-16 offsets; Rust converts with the TextInput's own text, so emoji and umlauts land right -> backspace scenario with a "ä" and an emoji in the field.
- Out-of-range offsets -> `BaseInputConnection.setSelection` ignores them before our code; Rust clamps anyway.
- Read-only or unfocused field -> Rust does nothing unless a `TextInput` has focus, like the handle drag.
- Content rect with the view at y = 0 (an app drawing edge-to-edge, or landscape without status bar) -> offset is 0, behaviour unchanged.
- The `top < 0` workaround now sees view coordinates -> near-top scenario.
- Other keyboards (AOSP on the API 26 image) -> typing and copy bar regression only.

## 1. Spike result and vendored crate
- [x] 1.1 Logging spike (approved): Gboard calls `setSelection(14,14)`, `(12,12)` ... with batch 0 and nothing reaches Rust; `onGetContentRect` gets window coordinates, the view is at (0,110) = status bar height. Log kept in scratchpad, spike not committed.
- [x] 1.2 Upstream fix A (selection), against Slint master in a scratch clone: Java `setSelection` override + `updateSelection()` in `SlintEditable` (no-op when unchanged; full `update()` while a composing span exists or the last reported state had one; otherwise report to Rust and call `InputMethodManager.updateSelection`), endBatchEdit reports a pending selection; Rust `update_selection` native method + `callback_update_selection` (focus TextInput, UTF-16 -> byte offsets with its own text, set anchor + cursor with `TriggerCallbacks`, hide handles like `update_text`). One commit, Slint's commit template, `ChangeLog:` trailer. Proof: `cargo check` of the backend for aarch64-linux-android (its build.rs compiles the Java), rustfmt, cspell on the changed files.
- [x] 1.3 Upstream fix B (copy bar): in `onGetContentRect`, subtract the view's location in the window from the selection rect, before the existing workaround. One commit. Same proofs.
- [x] 1.4 Vendor `i-slint-backend-android-activity` 1.18.1 from the registry into `third_party/`, apply A and B backported (1.18.1 has no package or lambda refactor), `[patch.crates-io]` entry, README. Proof: `cargo tree -i i-slint-backend-android-activity --target aarch64-linux-android` shows the path; diff against the registry copy shows only A and B.
- [x] 1.5 Build and lint: android --lib tests, CI clippy, Android target clippy, APK arm64 (FP5) and x86_64 (emulators). Proof: zero warnings, APK timestamps.
- [x] 1.6 Emulators: API 30 and 35 Play images (Gboard), API 26 Google APIs x86_64. Run every MANUAL scenario; the test matrix goes into the PR texts.
- [x] 1.7 Master check: build Slint master's material gallery (or the smallest master example with a text field) for Android with A and B and run the space-bar and copy-bar scenarios once on the FP5, so the PRs are tested on the branch they target. If that build can't be made on this laptop, the PRs say they were tested as a 1.18.1 backport.
- [x] 1.8 Repro probe on the final tree. Proof: 22/22 identical.

## 2. Docs and list
- [x] 2.1 `third_party/README.md`, cargo-sources.json regenerated, OPEN-ISSUES #160 removed and batch Y ticked, PR texts in this folder (`pr-selection.md`, `pr-copy-bar.md`). Proof: check-specs green, grep for #160.

## Notes
- Pre-build agent: 3 questions, all answered from the code (proposal Decisions, my calls).
- Upstream rules read: CONTRIBUTING (small focused PRs, MIT-0 checkbox once, by the account that opens it), AGENTS.md (no CHANGELOG edits, `ChangeLog:` trailer), writing style guide (comments only for what code can't show, US English, sentences under 25 words), commit template (subject, details, `== Implementation Notes ==`, trailers). Java isn't auto-formatted; follow the file's style (4 spaces, ~120 columns). cspell runs on changed files.
- Opening the PRs is an outward action: only after the maintainer reads the texts and says go.
- 1.2/1.3: on Slint master 3bf5f6d, branches android-ime-selection (141a29d, amended with the blink fix) and android-action-mode-rect (see `git log` there; amended with the FIXME note) in ~/.cache/slint-pr/slint (moved from the scratchpad). Android-target clippy of the backend: 26 warnings before and after, none in the new code; the Java compiles (build.rs); cargo fmt clean. cspell not run (no npx on the laptop); every new word is ordinary English, "preedit" is in Slint's word list.
- FP5 with fixes + temporary logging (14:36): space bar three times: every step is `setSelection n,n batch=0` -> `updateSelection n,n` -> Rust echo `n,n` within 1-9 ms; a repeated same position is a no-op. Backspace slide goes through key events Slint already handled (Rust-side selection 7 -> 0, no setSelection), so it worked before too. Copy bar: view at y=110, rect shifted by -110, bar above the word, also near the top (maintainer). Logs: scratchpad/spike/evidence-*.log.
- Found on the FP5 (maintainer), not caused by this change: after a long press selects a word, the selection end follows the finger at once (log: Rust-side 16,48 -> 16,44 130 ms later, no Java involved). Slint core's TextInput; Android's own fields extend by word. Separate issue.
- Device test finding (maintainer): the cursor kept blinking while sliding, so it was often invisible. TextInput restarts the blink on every arrow-key move (`show_cursor`, private); `callback_update_selection` now does the same through the public `WindowInner::set_cursor_blink_binding`. Added to fix A and the scenario list.
- 1.5: android --lib 226 passed; CI clippy clean; Android target clippy clean (clippy doesn't lint the path crate, it was linted on master, see 1.2). Final clean APK 2026-10-18 15:29 (arm64 only, no test logging: no "SlintFix" string; "updateSelection" present), installed on the FP5 15:29.
- 1.6 emulators (headless, driven with adb input, x86_64 APK with logging; jniLibs/x86_64 removed afterwards): API 35 and 30 Play images with Gboard: slide = setSelection -> updateSelection -> echo, letter lands at the new place, copy bar above the word (view at y=128 / 63). API 26 Google APIs image (Gboard preinstalled, composing on): first slide step takes the full-update fallback after Gboard ended the word, then the direct path; the word stays intact; copy bar above the text (view at 63).
- 1.7 master: material-gallery built from master+A+B and from plain master (xbuild, x64) on the API 35 emulator. Master: letter after a slide lands at the old place, copy bar below the field over the next one. Patched: letter at the new place, copy bar above, also for the field right below the app bar. Screenshots in ~/.cache/slint-pr/evidence/.
- /tmp near-miss: /tmp is a 7.8 GiB tmpfs; the Slint gallery build there filled it and MemAvailable hit 0. Cleaned up at once (my target dirs only); Slint clone moved to ~/.cache/slint-pr. Saved to memory.
- Upstream branches now in ~/.cache/slint-pr/slint: android-ime-selection (fix A incl. blink), android-action-mode-rect (fix B), test-both.
- 2.1: OPEN-ISSUES batch Y ticked (#148 in part 1, #160 here); #195 (long-press selection end) added as batch Y2. PR texts: pr-selection.md, pr-copy-bar.md.

## Review (2026-10-18)
- R1 held: repro probe still open at review time -> running on fbc132e (final tree).
- R2 held: PR texts didn't follow Slint's commit template (squash merge uses the description). Rebuilt: description = commit message incl. `== Implementation Notes ==` and trailers; logs and test matrix moved to a first comment.
- R3 held: "as above" in the copy-bar PR; now standalone.
- R4 held: Co-Authored-By was in the commits, not in the PR texts; now in both (same as slint-ui/slint#12932's commit).
- R5 held: stale branch hash in the notes; fixed. Commit B also got the FIXME note as `== Implementation Notes ==` (it was only in the PR text).
- All fixes are text only; no second review needed. Blink fix: confirmed by the maintainer on the 14:58 logging build; the 15:29 clean build has the same code without the Log lines.
- 1.8: probe on fbc132e (final code tree; later edits are tasks.md/PR texts only): 22/22 identical.
