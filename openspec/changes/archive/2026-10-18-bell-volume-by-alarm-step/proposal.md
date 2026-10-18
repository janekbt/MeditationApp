# Bell volume through the alarm step

## Why
GitHub #8: on Android 13 (the reporter) and on an older tablet (Janek)
the bell volume slider has no effect; every bell rings at full alarm
volume. On the FP5 it works. The app raises the alarm stream to its
top step and makes each bell quieter with the player's own gain,
sized from the dB range the device reports. A device that reports a
bogus range, or ignores the player gain, rings everything at the top.
Janek chose to fix it without measuring the tablet.

## What Changes
- While a bell rings, the alarm stream is set to the step that fits
  the bell's volume instead of its top step: 0 % is the quietest
  step, 100 % the loudest, in between the step at the same position,
  or a louder one when the device's reported curve says that step is
  too quiet. The player gain only trims the rest. So the slider works
  even when the reported range is wrong or the gain is ignored.
- Bells that ring at once: the stream takes the loudest bell's step,
  and each quieter bell is trimmed by its gain. (my call: one alarm
  stream; a device that ignores the gain plays the quieter bell at the
  louder one's step)
- The volume slider's live preview moves the stream step too.
- The user's alarm volume is still put back after the bell, and after
  a crash on the next start, unless they moved it while it rang.
- A reported curve that isn't one (wrong length, not finite, falling,
  no span) is replaced by an even 40 dB curve over the device's steps,
  as before Android 9. (my call)
- Left out: measuring on the tablet (Janek); any change to Linux.

## Decisions
- Robust fix without measurement (Janek).
- Step choice by slider position with the reported curve only able to
  make it louder, never quieter. (my call)

## Capabilities
### Modified Capabilities
- `bells`: Android bell volume uses the alarm step.

## Impact
- Core: `meditate-core/src/bell_volume.rs` (the curve, step and gains
  replace `DeviceRange` and `absolute_gain`).
- Android: `kotlin/MeditateAudio.kt` (report all steps, set the stream
  to the loudest active bell's step, gains per step), `src/audio.rs`
  (JNI signatures), `src/alarm_volume.rs` (keeps the curve), the four
  play sites in `src/ui.rs`.
- No UI text, no translations, no data, no sync. GTK untouched.
- Android build environment: untouched.
