# Tasks: Android long press, selection extends by word (#195, batch Y2)

Base: 799f3c8 (beta); suites on it: green at 3dacff2/799f3c8 (android --lib 226, core 1378+18, GTK 151; part 2 changed no app code).

## Loop
- [x] 1 Understand
- [x] 2 Propose (approved: 2026-10-18)
- [x] 3 Build
- [x] 4 Review (fixes need a second look: R1 changes behaviour)
- [x] 5 Specs, docs, list
- [x] 6 Verify and hand over (maintainer tested the 16:40 build on the FP5, installed 16:44: all fine)
- [x] 7 Ship

## Review Focus
- Long press, then release without moving -> word stays selected, copy bar shows (unchanged path).
- Long press after the press already ended (timer racing the release) -> word selected, no mode change (pressed is 0).
- Long press with an existing selection (cursor != anchor) -> backend skips select_word today; keep that, so the new function is only called where select_word was.
- Drag left of the word -> select_word's anchor > cursor branch keeps the original word (as desktop double-click drag).
- Callbacks: word-mode moves use SkipCallbacks for the intermediate cursor set and select_word triggers them, as for the desktop double click.
- A press on another item after the long press -> pressed is reset on Released/Exit as today.

## 1. Upstream (Slint master, ~/.cache/slint-pr/slint, branch android-long-press-word)
- [x] 1.1 Test first: `tests/cases/text/select_long_press.slint` modelled on `select_double_click.slint`: press inside "Ipsum", do in the Rust block what the Android long-press timer does (`WindowInner::from_pub(..).focus_item`, downcast to `TextInput`, `select_word_on_long_press`), with the field focused first, assert anchor 6 / cursor 11, move a few px inside the word -> still 6/11, move into "Dolor" -> 6/17, move left of the word -> 11/0 (or the boundaries select_word gives). Expected: compile stub, FAIL (moves shrink to the finger), then PASS. Run with SLINT_TEST_FILTER in test-driver-rust, build under ~/.cache.
- [x] 1.2 `TextInput::select_word_on_long_press` in `internal/core/items/text.rs` (doc comment: contract only), backend `long_press_timeout` calls it. Proof: the test, clippy for the backend (aarch64) and core, cargo fmt.
- [x] 1.3 Material gallery on the API 35 emulator: before (master) and after, long press + small move. Commit with Slint's template; PR text = commit message; evidence as first comment.

## 2. Meditate
- [x] 2.1 Vendor `i-slint-core` 1.18.1 into `third_party/` with the same function (backport), patch entry, backend call. Proof: `diff -r` shows only this; android --lib, CI clippy, Android target clippy.
- [x] 2.2 APK, FP5 + API 35 emulator: both MANUAL scenarios; maintainer confirms.
- [x] 2.3 Repro probe on the final tree: 22/22.
- [x] 2.4 README, cargo-sources.json, OPEN-ISSUES (#195 removed, Y2 ticked).

## Notes
- Pre-build agent: touch-down reaches TextInput as Pressed with click_count 0, so pressed == 1 when the timer fires (a tap then tap-and-hold already gives word mode, harmless). Core 1.18.1 and master match. Out of scope, in the proposal: unfocused field, Flickable taking over an 8 px vertical move in the first 500 ms. Device test: tap the field first, then long-press; hold the finger still vertically.
- Builds of the Slint tree stay under ~/.cache (no /tmp: it's RAM).
- 1.1: `select_long_press.slint` FAILED on the stub (select_word only) at the tremble: cursor 7, expected 11; PASS with the fix. Mutation: setting word mode unconditionally fails the press-less part (17 vs 12). text/ in test-driver-rust: 38 passed; interpreter: the new case passes.
- 1.2: core clippy (--features std, -D warnings) clean; Android backend clippy 26 warnings = master baseline; fmt clean. Commit 0e013d7 on branch android-long-press-word (local, ~/.cache/slint-pr/slint).
- 1.3: gallery on API 35: master long press + tremble -> "ips"; fixed -> "ipsum"; drag on -> "ipsum dolor". Screenshots in ~/.cache/slint-pr/evidence/. PR text: pr-long-press.md.
- 2.1: third_party/i-slint-core = 1.18.1 + the function (diff 13 lines); backend call; patch entry. android --lib 226 passed; CI clippy and Android target clippy clean. APK 16:18 (lib hash changed: 82cc5828... vs abbb3b4f...).
- 2.3: probe running on 34fe2c1.

## Review (2026-10-18)
- R1 held: a drift during the hold (before the timer) moved the cursor by character; the backend then skipped selecting the word (cursor != anchor) and showed the menu over 1-2 letters. Fixed: core moves the cursor back to the anchor (press point) while the press is held, the backend checks the character at the anchor. Test step added (press at 10, drift to 12, long press -> 6/11): FAIL before (6/17), PASS after. text/ 38 passed; clippy core clean, Android backend 26 = baseline; fmt clean. Upstream commit amended (c9addee).
- R2 held: PR text claimed an FP5 test not yet done, and "interpreter passes" proves only that the file compiles. Reworded.
- R3 held: drag back onto the first word now asserted (6/11).
- R4 held: root cause moved under `== Implementation Notes ==`.
- R5 held: task reference fixed.
- R6 held: unfocused-field long press added as #196 (batch Y2).
- R7 held: #195 stays in OPEN-ISSUES until the maintainer's device test; Y2 unticked.
- Known corner, set aside: Shift held on a hardware keyboard during a long press now selects the word at the old anchor instead of skipping; rare on a phone.
- Probe on 34fe2c1 stopped (obsolete after R1); rerun on the final tree.
- Second look (R1 fixes): IMPORTANT held: with the old guard gone, a triple press then hold ran select_word on the paragraph and grew it ("Hello world.\nNext": 0/12 -> 0/17). Fixed: the function acts only on pressed 0 (select word, as before) and 1 (reset cursor, word mode, select word); a double or triple press keeps its selection. Test step added: FAIL (0,17) vs (0,12) before, PASS after; text/ 38 passed; core clippy clean. Upstream commit amended (51c7716). MINOR set aside: the 20 px drift in the test exceeds the backend's 10 px cancel radius with the 10 px test font, but the radius is logical px and real narrow glyphs ("i", "l") are ~4 px, so a drift across a word edge happens on devices. MINOR fixed: backend comment wording.
- Third look (pressed match): PASS. Two wording notes applied: doc comment (pressed 0 behaves like select_word), test comment ("calls" instead of "does"). Upstream commit ecca0da; vendored function and backend caller byte-identical to it.
- APK from the final state 2026-10-18 16:40 (~/.cache/slint-pr/meditate-longpress.apk). Probe running on cab0094.
- 2.3: probe on cab0094 (final code; later edits are OPEN-ISSUES, tasks and PR text only): 22/22 identical.
