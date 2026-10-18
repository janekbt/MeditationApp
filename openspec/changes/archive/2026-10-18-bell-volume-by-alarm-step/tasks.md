# Tasks: bell volume through the alarm step

Base: dcc493058475df2cdeb5abb033ee015a3275a354; suites on it: green (`cargo test --workspace`, 1759 passed, run on this tree before the commit)

## Loop
- [x] 1 Understand (GitHub #8; Janek: build the robust fix without measuring)
- [x] 2 Propose (approved: 2026-10-17)
- [x] 3 Build
- [x] 4 Review (one reviewer, then a fresh look at the fixes; log at the end)
- [x] 5 Specs, docs, list (check-specs green; no doc names the old scheme; GitHub #8 is not in OPEN-ISSUES)
- [ ] 6 Verify and hand over (built and handed over 2026-10-17; waiting for the device test)
- [ ] 7 Ship

## Review Focus
- Two bells at once with different volumes -> "A quiet bell while a louder one rings"; the stream follows the loudest active bell and drops when it ends.
- Preview while a session bell rings: same rule, all players count.
- App dies mid-bell -> recovery compares with the step the app set, not the top step; MANUAL "The alarm volume comes back".
- User moves the alarm volume while a bell rings -> theirs stays (unchanged rule).
- Do Not Disturb / setStreamVolume throws -> logged, the bell still plays (unchanged).
- Android 8 (API 26/27): no dB report -> even fallback curve.
- Alarm min step 0 on some devices -> the curve starts at step max(min, 1), so 0 % is never mute.

## 1. Core
- [x] 1.1 Curve from the report, fallback. Files: `meditate-core/src/bell_volume.rs`. Proof: `a_report_that_is_not_a_curve_falls_back_to_even_steps`
- [x] 1.2 Step by position, raised by the curve. Proof: `the_slider_picks_the_alarm_step_by_its_position`, `the_curve_can_only_raise_the_step`
- [x] 1.3 Gains per stream step. Proof: `a_volume_between_two_steps_is_trimmed_by_the_gain`, `a_quieter_bell_keeps_its_level_under_a_louder_one`

## 2. Android
- [x] 2.1 Kotlin: report min, max and every step's dB; each player keeps its step and gains; the stream follows the loudest active player; restore compares with the step set. Rust JNI and the four play sites pass the step and gains. Proof: MANUAL scenarios; existing source tests updated.

## Notes
Fresh read before showing (2026-10-17), 8 questions, all my call:
- Position step in integers (`ceil(v * (max - min) / 100)`), dB compared with 1e-3 dB tolerance (reports are f32).
- The curve starts at max(min, 1): step 0 mutes and reports -inf.
- "Falling" means a step quieter than the one below it; flat neighbours pass if the whole span is above 0.
- The user's step is saved once, on the first raise; the step the app last set is written (commit) before every setStreamVolume, and recovery and restore compare with it.
- If the current step differs from the one the app last set, the user took over: the app stops moving the stream until idle and does not restore.
- Each player keeps its gains per stream step; whenever the stream changes (start, end, live drag), every live player gets gains[step], with the step read back from getStreamVolume, so a refused change leaves bells quieter, never louder.
- Live drag sets the stream with flags 0: no panel, no click.
- Build (2026-10-17): the fallback test passed against the stub (the stub's flat curve equals any other flat one). Mutation check after building: dropping the "not falling" check, the step-0 skip, the span check, or raising instead of lowering-capping the step each fail one test, and pass restored.
- Kotlin keeps recovery for a crash left by 26.10.1 (KEY_RESTORE without KEY_SET): it compares with the top step as before.

## Review log
One reviewer, 2026-10-17:
- R1 (IMPORTANT, held): the 1.2 dB test report was even, so the curve alone gave 1/4/7 and the position rule had no guard. Now an uneven report (-1.2, -0.1 ... 0) where the curve alone gives step 2 at 50 %; the scenario says so. Mutation (drop the position rule) fails the test.
- R2 (held): gains were set after raising the stream, so a quiet bell could peak for one buffer when a loud one starts. Raising now trims for the new step first (`trimLocked(want)`), then sets, then trims for the step read back.
- R3 (held): an unreadable stream step gave every player gain 1.0. Now each player falls back to its own step's gain.
- R4 (accepted): if even getStreamMaxVolume throws, the curve falls back to 1..=7; on a 15-step device a 100 % bell would ring at step 7. Only that one call sits in the outer catch; noted, no change.
- R5, R6 (held): stale doc line in audio.rs, a doubled blank line in bell_volume.rs. Removed.
- Live drag had no test: the source test now checks `setVolume` re-applies the stream.

Review of the fixes (2026-10-17): PASS, three MINOR, all held: an unreadable stream step now trims for the step the app last read back (own step only before any read); the drag test also checks the preview's new level is stored; the scenario says "almost all" of the 1.2 dB. Test and wording, plus one fallback value: no further review.

## Hand-over (2026-10-17)
Verified: `cargo test --workspace` 1761 passed; clippy clean (GTK + core all targets, CI form, Android target before the review fixes; the fixes touch Kotlin and a test only); check-specs green.
Artifact: debug APK `meditate-android/android/app/build/outputs/apk/debug/app-debug.apk` 22:49, on the FP5 22:49.

Test steps (FP5, German UI):
1. Set the system alarm volume to about half (e.g. 3 of 7). In Meditate, open a bell's Lautstärke row and drag from left to right with the preview ringing: the bell gets clearly louder, no system volume panel appears.
2. Afterwards the system alarm slider is back where it was in step 1.
3. Same bell at 50 %: about as loud as before this change.
4. A short countdown with an interval bell at 20 % and the end bell at 100 % ringing close together: the quiet bell stays quiet while the loud one rings, no burst.
5. While a bell rings, move the system alarm volume yourself: your setting stays after the bell.
6. Tablet or the #8 reporter: the slider at 0 % is clearly quieter than at 100 %.

Proposed commit message:

    android, core: Bell volume sets the alarm step

    Bells raised the alarm stream to its top step and were made
    quieter by the player's gain, sized from the dB range the device
    reports. A device that reports a wrong range or ignores the gain
    rang every bell at full alarm volume. Each bell now sets the alarm
    step its volume needs, by the slider's position and raised by the
    reported curve, and the gain only trims the rest. Bells that ring
    together share the loudest one's step.

    Refs #8
