# Alarm bells in the headphones only

## Why
With "Play Bells as Alarm" on, Android plays a bell on the speaker and
the headphones at once, so everyone nearby hears it. Janek wants the
alarm path to behave for the user exactly like the media path: with
headphones, only in the headphones. A throwaway spike on the FP5
(2026-10-29, Bluetooth only) showed Android honours a preferred output
device for an alarm player: every bell went to the headphones only, at
the alarm step the slider set.

## What Changes
- Android, switch on: every bell and preview still plays as an alarm,
  aimed (`MediaPlayer.setPreferredDevice`) at the headphones media
  would play on (the existing `headphones()` lookup). No headphones:
  Android's own alarm route, the speaker, as now.
- The output changes while an alarm bell rings (headphones pulled or
  plugged in): the bell stops, as a media bell does.
- Volume: the alarm step and curve as now (Janek found the loudness in
  Bluetooth headphones fine in the spike).
- A preview that stops early (headphones plugged in or pulled, either
  stream) shows Play again at once, not only after its length (Janek,
  2026-10-29, from the review): the app checks every 250 ms whether the
  preview still plays.
- Switch off (media): unchanged otherwise. Linux: nothing.
- Left out: a hearing-safety cap for the alarm in headphones (Janek,
  2026-10-29: the user sets the volume and can keep headphone
  presets); a new explanation text (the current one is still true).

## Decisions
1. Alarm bells only in the headphones, like media (Janek, 2026-10-29).
2. Headphones pulled or plugged in under an alarm bell: it stops, like
   media (Janek, 2026-10-29).
3. No hearing-safety cap for the alarm in headphones: proposed, then
   dropped by Janek (2026-10-29).
4. A preview stopped early shows Play again at once (Janek, 2026-10-29).
5. My calls: only from Android 9 (`setPreferredDevice` is API 28; minSdk is 26, so 8.x keeps speaker and headphones together); only the alarm player is aimed, media keeps Android's own route; the alarm keeps one restore step, not one per output (the
   spike showed the alarm step follows the slider in headphones); the
   alarm is aimed at the device media plays on, so both paths pick the
   same headphones; if Android refuses the preferred device, the bell
   plays on Android's alarm route rather than not at all.

## Capabilities
### Modified Capabilities
- `bells`: an alarm bell rings only in connected headphones.

## Impact
- Android: `kotlin/MeditateAudio.kt` (aim the alarm player; watch the
  output for every bell; each player remembers the output it started
  on; `previewPlaying`), `src/alarm_volume.rs` (source check),
  `src/audio.rs` and `src/ui.rs` (poll the preview for both pills),
  `src/app.rs` (JNI table, source check).
- Core, GTK, translations, data, docs: none (README already says
  "with headphones they ring only there"; the explanation stays).
- Android build environment: untouched.
