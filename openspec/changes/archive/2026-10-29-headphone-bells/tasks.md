# Tasks: headphone-bells (bells as media, the alarm as a switch)

Base: df88c33; suites on it: core 1390, android 262 green (Linux not
run: the change does not touch it).

## Loop
- [x] 0 Skills loaded, again after every /compact: `/ponytail ultra`; `slint:slint`
- [x] 1 Understand (said back and confirmed; decisions in proposal.md)
- [x] 2 Propose (fresh-agent check done, answered in Notes; approved: 2026-10-29, Janek "go"; the explanation text to be reworded at the end, Janek)
- [x] 3 Build (core 1392, android 265 green; host CI clippy with tests and Android-target clippy clean; check-specs green; every new test failed first for the right reason)
- [x] 4 Review, then `/simplify` after the last review (review, fix review, /simplify; see Notes)
- [x] 5 Specs, docs, list (check-specs green; README Bells line; MODIFIED the alarm-step requirement; no OPEN-ISSUES entry for #10)
- [x] 6 Verify and hand over (second FP5 test passed, Janek 2026-10-29 "Passt alles"; first FP5 test passed, Janek 2026-10-29 "passt, alles funktioniert"; then flipped: media by default, the switch plays as an alarm, new explanation; see Notes, device test 2)
- [x] 7 Ship (deltas merged into openspec/specs/bells/spec.md, archived 2026-10-29, check-specs green)

## Review Focus
- The switch synced from another device while a bell rings -> the bell keeps its channel; the next bell uses the new value (decision 5).
- A crash while the media step is raised -> put back at the next start, like the alarm (scenario "The media path in the player").
- The user moves the media volume while a bell rings -> theirs stays, like the alarm's takenOver rule.
- Music playing when a media bell rings -> it gets louder or quieter for the bell's length (accepted cost of decision 2).
- A Bluetooth speaker counts as headphones (Android can't tell them apart) -> stated in the proposal, no scenario.
- Bluetooth devices that sleep and swallow a bell's start -> left out (Janek: later).
- Linux and an old database -> no row; a missing key reads as off.

## 1. The setting (core)
- [x] 1.1 `BELLS_AS_ALARM_KEY = "bells_as_alarm"` and `bells_as_alarm_from_db` (was `bells_as_media` until device test 2) in `meditate-core/src/settings_keys.rs`. Proof: `bells_play_as_an_alarm_once_turned_on`, FAIL then PASS.

## 2. The headphone curve (core)
- [x] 2.1 `AlarmCurve::headphones(min, max, db)` in `meditate-core/src/bell_volume.rs`: the steps above the safe step (10/15 of max, rounded) are dropped, so `alarm_level` never picks them. Proof: `the_headphone_slider_stops_at_the_hearing_safety_step`, FAIL then PASS.

## 3. Levels per stream (Android Rust)
- [x] 3.1 `alarm_volume.rs`: keep three curves (alarm on the speaker, media on the speaker, media on headphones), read once at startup (`audio::*_steps`), and `level(volume)` returns all three; the Kotlin side picks by stream and route at play time. Proof: `a_bell_takes_the_level_of_where_it_plays`, FAIL then PASS.

## 4. The media path (Kotlin, JNI)
- [x] 4.1 `MeditateAudio.kt`: `play`/`playBell` take `asMedia` and the three levels; as media: `USAGE_MEDIA`, a second hold for `STREAM_MUSIC` with its own restore keys, the route read at start (headphones or speaker) choosing the level; an `AudioDeviceCallback` stops a media bell whose headphones go away. `src/audio.rs` and the `src/app.rs` JNI table follow. Parts may come from the stashed first attempt. Proof: `a_media_bell_holds_the_media_step_and_puts_it_back` (source check, Kotlin can't run on the host), FAIL then PASS.
- [x] 4.2 `src/ui.rs`: every play site (starting, interval, end, Box Breath cues, previews) passes the switch (`bells_as_alarm_from_db` since device test 2, negated once in `bell_levels`). Covered by 4.1's test listing the play sites.

## 5. Preferences (Android UI)
- [x] 5.1 `ui/main.slint`: a Bells group after Stats: header, explanation text in the Stats group's style, a SwitchRow; `src/ui.rs` reads it on open and writes it with `write_global_setting`. Nine `.po` files. Proof: `the_bells_group_holds_the_alarm_switch`, FAIL then PASS; layout MANUAL.

## 6. Docs
- [x] 6.1 README Bells line: on Android bells can play as media, in the headphones only.

## Notes
- Builder check (fresh agent), answered as my calls:
  1. Media steps are kept per output: save the output (type and address, as the stash's `media_device`) with the restore keys; put the step back only while that output is current, else at the next start or bell there; an output change during a bell marks the hold taken over and stops the bell (also headphones plugged in during a speaker bell; proposal updated).
  2. Android routes the bell itself (no `setPreferredDevice`); the stash's `headphones()` lookup (`getAudioDevicesForAttributes`, falling back to the output list: wired, USB, Bluetooth) only picks the curve.
  3. The safe step rounds down: `max * 10 / 15`. USB headsets may have a lower limit of their own: unverified.
  4. One headphone curve, read for wired headphones at startup; on Bluetooth absolute volume the trim may be off, the step still follows the slider.
  5. One media hold, owned by the output of its first bell; a bell on another output marks it taken over. Alarm and media holds run side by side.
  6. Headphones = the stash's list (wired headset and headphones, USB headset and device, Bluetooth A2DP, BLE headset and speaker).
  7. The switch is read at every play site, previews and the live slider drag included.
  8. `a_bell_takes_the_level_of_where_it_plays` tests a pure function taking three curves (no global, which would race); the stream choice is in the Kotlin source check.
  9. No audio focus requested.

## Review (fresh agent)
- 1 CRITICAL, fixed (tests first): one shared pair of media keys let a bell on the new output overwrite the step kept for headphones pulled mid-bell, so they stayed at the app's step. Each output now keeps its own pair (`media_restore_index/<output>`, `media_set_index/<output>`); the `stale` shortcut is gone.
- 2 IMPORTANT, fixed: a kept step came back only at the next start or bell there; now as soon as that output is current again (`outputChanged` restores when nothing is raised), and the watch starts with the recovery at app start.
- 3 MINOR, fixed: a failed save of the switch now shows the stored value again (`reload_setup`).
- 4 MINOR, fixed: MODIFIED delta for "On Android a bell's volume sets the alarm step": its stream, alarm or media.
- 5 MINOR, fixed: `bell_levels` moved out of `play_volume_preview`'s doc comment.
- 6 MINOR, accepted: a media preview stopped by an output change leaves the Stop pill until the clip's length is over; rare, and the existing pill timer reverts it.
- 7 MINOR, fixed: a slider drag carries the switch; a preview started on the other stream ignores it.
- 8 MINOR, fixed: the Russian explanation read "but also"; now "even ... also".
- Also from the review (cannot verify from code): the safety cap scaled with the step count (`max*10/15`), but AudioService keeps index 10 whatever the count; now `min(10, max)` (my call: never the warning dialog, a 30-step device is quieter at the top). Test first.
- Device-only, open: whether Android re-routes before or after the device callback on Bluetooth (a bell may not stop when Bluetooth headphones connect mid-bell); USB and Bluetooth curves; Android 14+ sound dose.
- Fix review of those fixes (fresh agent): 1 IMPORTANT, fixed (test first): `restoreSaved` removed an output's keys even when the stream wasn't at the app's step, so with Android still routing media elsewhere for a moment (Bluetooth reconnecting, or wired and Bluetooth both connected) a kept step was wiped without being put back. Now the keys go only once the step is back; otherwise they stay and the next raise overwrites them (alarm too: a user who moved the alarm volume keeps it, the keys wait). Left open for the device test: the device callback may come before Android re-routes media; then a returning output's step comes back only at the next start or bell there, and a media bell may keep playing when Bluetooth connects mid-bell (a per-player routing listener would catch that, if the FP5 shows it). 2 MINOR, partly fixed: the Kotlin checks pin the text; `restoreSaved` now pins the order (set back, then remove) and that no `edit().clear()` exists; logic tests would need a JVM harness, so the FP5 test carries the rest.
- /simplify (four agents), applied: `Hold` carries its attributes, its route (`{ "" }` for the alarm, `::route` for media) and builds its own keys, so the hold code has no media special cases left and the alarm keys stay the bare old ones; the route is read once per bell; `Level` remembers whether it took the headphone curve, so a slider drag needs no device lookup; one `levelFor`; `holds` list; `alarm_steps` uses `report_steps` (`invoke_alarm_steps` gone, log tag now `audio.steps`); the three curves are `Slot`s that each hold their reader and fallback once; one `bells_as_media()` in ui.rs. Skipped: moving the alarm/media curve choice into core (alarm and media are Android streams, GTK has none; a one-line pick); a shared JNI argument helper for the three play calls (three sites, the signature test pins them); reading the switch once per preview instead of per drag (a mismatched drag is already ignored, cost not noticeable); building only the needed levels (keeps one JNI shape).

## Device test 2 (2026-10-29)
First FP5 test: all seven steps passed (Janek). He then asked for media
as the default and the alarm when the switch is on, with his
explanation (proposal decision 3). Built test first: the key is now
`bells_as_alarm` (`BELLS_AS_ALARM_KEY`, `bells_as_alarm_from_db`, off
until set; the old key never shipped, so no migration),
`bells_play_as_an_alarm_once_turned_on` failed on "media until set"
against a stub defaulting to true, then passed; ui.rs passes
`!bells_as_alarm()`; the Slint property, callback, texts, nine `.po`
files and `the_bells_group_holds_the_alarm_switch` follow. The Kotlin
player is unchanged (it still takes `asMedia`). Existing users get
media after the update.
Fix review (fresh agent): five MINOR, all fixed: the FP5 scenario still said "switch off" for the alarm check; the MeditateAudio.kt header and a MeditateGuided.kt comment still called the alarm the default; old proof names in 1.1 and 5.1; es/fr/it/pt_BR said "safest" where the English says "most reliable" (German and Dutch kept, Janek's own wording and close enough).
/simplify after the fix review (four agents): clean; the one `!` in `bell_levels` stays (renaming the `as_media` chain through JNI and Kotlin only moves it). Applied: task 4.2 named the old reader. Skipped: dropping the "yes please" check in the core test (it is a named case of the spec scenario).

## Hand-over (step 6)
APK: `meditate-android/android/app/build/outputs/apk/debug/app-debug.apk` (19:12:44, after device test 2), debug, `adb install -r`.

FP5 steps:
1. Preferences: a "Glocken" group right after "Statistik", the new explanation in full at phone width, the switch "Glocken als Alarm abspielen" off.
2. Switch on, wired or Bluetooth headphones on: a volume preview rings in the headphones and on the speaker, as before.
3. Switch off, headphones on: a preview and a short Timer session with starting, interval and end bell ring only in the headphones; the speaker stays silent. Same preset twice: equally loud.
4. Switch off, no headphones: the bells ring on the speaker.
5. Note the media volume (system slider) before: after each bell it is back where it was, on the speaker and in the headphones.
6. Switch off, a long bell ringing in the headphones, pull them: the bell stops; plug them back in: their media volume is what it was before the bell. The same with plugging in during a bell on the speaker.
7. A volume slider dragged while its preview rings in the headphones: the loudness follows the slider.

Commit message:
```
bells: Play bells as media on Android, in the headphones only

Android plays alarms on the speaker and the headphones at once, so on
a train everyone heard the bell. Bells now play as media, where music
would play: only in connected headphones, else on the speaker. The
volume stays fixed as with the alarm, in headphones up to Android's
hearing-safety step, and each output gets its own media volume back.
A switch in Preferences plays them as an alarm again, for devices
where media bells fail.

Fixes #10
```
