# Tasks: alarm-bells-headphones

Base: fbd6447; suites on it: core 1392, android 265 green (run on this
tree before the fbd6447 commit; Linux not run, untouched).

## Loop
- [x] 0 Skills loaded, again after every /compact: `/ponytail ultra`; `slint:slint`
- [x] 1 Understand (said back and confirmed; spike approved and run on the FP5, Bluetooth only; decisions in proposal.md)
- [x] 2 Propose (fresh-agent check done, answered in Notes; approved: Janek 2026-10-29 "der rest passt", the cap he asked for then dropped)
- [x] 3 Build (core 1392, android 266 green; the new test failed first on the missing aim)
- [x] 4 Review, then `/simplify` after the last review (review, fix review, /simplify; see Notes)
- [x] 5 Specs, docs, list (check-specs green; README and explanation still true; no OPEN-ISSUES entry, review point 3 built as 2.1)
- [x] 6 Verify and hand over (core 1392, android 266 green; CI clippy clean; check-specs green; APK 20:12:45 installed; device test 1 passed after its fix, Janek 2026-10-29 "tests work"; FP5: alarm only in the box, Janek "tests work"; button, Janek "works")
- [x] 7 Ship (deltas merged into openspec/specs/bells/spec.md, archived 2026-10-29, check-specs green)

## Review Focus
- Wired headphones: not tried in the spike; the FP5 test covers them.
- A preview started on the speaker, then headphones plugged in: it stops (decision 2), the next preview goes to the headphones.
- Android refuses `setPreferredDevice` (returns false) or the device vanishes between lookup and start: the bell plays on the alarm route; the output callback then sees a route different from where it started only if the route changed, so it is not stopped needlessly.
- A media and an alarm bell never ring together (one switch), but a switch flip during a long bell: the old bell keeps its own output and stream.
- Crash recovery: the alarm restore keys stay the bare old ones.

## Notes
Builder check (fresh agent), three questions, all answered by me:
1. API 26-27 have no `setPreferredDevice`: guarded with `SDK_INT >= P`; on 8.x the alarm keeps ringing on both (spec and proposal say so).
2. If the FP5 shows wired headphones still ringing on the speaker too: a device-test finding, brought to Janek before anything ships (not decided ahead).
3. Only the alarm player is aimed; the media check's `!contains("setPreferredDevice")` becomes a check that the call sits behind `hold === alarm`.

Review (fresh agent): PASS, three MINOR.
1. Fixed: for media the output was read twice (before and after `prepare()`); a device arriving in between could raise the new output's media step to a speaker-curve step. `hold.output` takes the one lookup again (`out` for media, `""` for the alarm).
2. Fixed with 1: the "read once per bell" comment is true again; a comment rewrapped.
3. Question for Janek: a preview stopped by plugging in or pulling headphones leaves its pill showing "playing" until its length runs out (already so for media; now also for the alarm). Fixing it is beyond the approved proposal.
Fix review (fresh agent, fixes 1 and 2): PASS; one MINOR, fixed: the comment above `watching` now says every bell stops on an output change, whatever its stream (decision 2).
/simplify (four agents), applied: one `outOf(phones)` helper behind `route()` and `startLocked`; `Level.head` dropped (a media bell off the speaker takes the headphone curve, derived in `levelFor`); `outputChanged` returns after the put-back when nothing rings (one lookup per idle device event); two duplicate asserts out of the new test. Skipped: `hold.output = hold.route(app)` instead of the `hold === media` pick (brings back the double lookup the review removed); a general `Hold` field for "aimed" or "keyed per output" (two fixed streams, more code); settling once after stopping several bells (at most four, small).
Device-only, left to 6.1: wired headphones may play an aimed alarm at the headset's own alarm step (Android keeps alarm steps per output); before Android 13 with two Bluetooth outputs the aim may pick the inactive one.

## Tasks
- [x] 1.1 `MeditateAudio.kt`: an alarm player gets `setPreferredDevice(headphones())` before `prepare()` when there are headphones; every player's `Level` records the output it started on (`route()`), the output callback is registered for every bell, and `outputChanged` stops each player whose output is no longer current. Proof: `an_alarm_bell_plays_only_in_the_headphones` (source check, Kotlin can't run on the host), FAIL then PASS; the media source check `a_media_bell_holds_the_media_step_and_puts_it_back` adjusted where its quoted lines move.
- [x] 2.1 Preview pills (volume row, sound chooser) revert as soon as the preview stops: `MeditateAudio.previewPlaying()`, `audio::preview_playing`, one `when_preview_ends` poll in ui.rs replacing both length timers. Proof: `a_preview_cut_short_shows_play_again` and the JNI table entry, FAIL then PASS.
- [x] 6.1 FP5: Bluetooth box, the switch on and off (wired headphones not reported as tried) (spec scenario "Bells on the FP5, as media and as an alarm").

## Device test 1 (2026-10-29)
Janek, Bluetooth box, switch on: the bell rang on the box and the speaker. Debug log: the box was found (`8/ws-1`), but `preferredDevice` stayed null. Cause: the aim was set before `setDataSource`, and `MediaPlayer.setPreferredDevice` fails on a player without its file (the spike set it just before `prepare()` and got true). Fix, test first: the source check pins the aim between `setDataSource` and `prepare()` (failed, then passed) and a refused aim logs a warning. Debug logging removed. Fix review (fresh agent): PASS, nothing holds (libmedia attaches the player in `setDataSource`; the aim set before `prepare()` is kept for the track it opens). /simplify after it (four agents): reuse, efficiency, altitude clean; applied: two overlapping asserts merged; skipped: folding the aim into one long `if` condition (a side effect at the end of a condition reads worse).

## Preview pill (2.1)
Janek after device test 1: "fix the button". Built test first (the JNI table entry and `a_preview_cut_short_shows_play_again` failed on the missing method, then passed; the pattern preview keeps its length timer, it has no player to ask). Android-target and CI clippy clean. Review (fresh agent): PASS, nothing holds. /simplify (four agents): reuse, efficiency, altitude clean (polling is how the app already hears from Kotlin, cf. `take_eos`; a push would need native registration); applied: the chooser's `if dur_ms > 0` wrapper (it already returned on 0); skipped: the volume row's same guard (it also guards nothing worth a wrapper-less form), caching the JNI class (microseconds at 4 calls a second).

## Hand-over (step 6)
APK: `meditate-android/android/app/build/outputs/apk/debug/app-debug.apk` (20:12:45, with the preview pill fix), debug, `adb install -r`.

FP5 steps, "Glocken als Alarm abspielen" on:
1. Wired headphones: a volume preview and a short session (starting, interval, end bell) ring only in the headphones; the speaker stays silent. Slider 0 % vs 100 %: clearly quieter and louder.
2. Bluetooth headphones: the same.
3. No headphones: the bells ring on the speaker.
4. During a long bell in the headphones, pull them (or switch Bluetooth off): the bell stops. During a bell on the speaker, plug them in: it stops. The next bell rings on the new output.
5. Afterwards the alarm volume (system slider) is where it was.
6. Switch off: media as before (headphones only, speaker without).

Commit message:
```
bells: Ring alarm bells only in the headphones on Android

With "Play Bells as Alarm" on, Android played each bell on the
speaker and the headphones at once. An alarm bell is now aimed at the
headphones media would play on, so it rings only there, as a media
bell does, and it stops when headphones are pulled or plugged in.

A preview stopped that way showed Stop until its length ran out; both
preview buttons now ask the player and show Play again at once.
```
