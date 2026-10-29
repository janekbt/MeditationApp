# Bells as media (GitHub #10)

## Why
GitHub issue #10: the bells play as an alarm (`MeditateAudio.kt`,
`USAGE_ALARM`), and Android plays alarms on the speaker and the
headphones at once, so on a train everyone hears the bell. The reporter
wants the bells kept to the headphones. A first attempt (per-mode
switch plus a Bluetooth lead-in noise) was discarded on 2026-10-29; it
lies in `git stash` and parts of its media path may be reused. Commit:
`Fixes #10`.

## What Changes
- Android, Preferences: a new group "Glocken" / "Bells" right after
  Stats, with an explanation under its heading (as the Stats group has)
  and one switch "Glocken als Alarm abspielen" / "Play Bells as Alarm",
  off by default (Janek after the device test, 2026-10-29: media is the
  default, the alarm the fallback for devices where bells fail).
- On: every bell rings as before this change (alarm, fixed alarm step).
- Off: every bell of every mode, and every volume preview, plays as
  media. Android sends media where music would play: only to connected
  headphones (wired, USB, Bluetooth; a Bluetooth speaker counts too),
  else to the speaker.
- The volume stays fixed, as with the alarm: the app sets the media
  step the bell's volume needs and puts the user's step back afterwards
  (also after a crash). On the speaker it uses the speaker's media
  curve; in headphones the slider spans quiet up to Android's
  hearing-safety step (assumed index 10, AOSP's default and the FP5's
  value, whatever the step count; no public API reads it), so Android never shows its warning dialog.
- The output changes while a media bell rings (headphones pulled or
  plugged in): the bell stops at once. Android would move it to the
  other output at a step meant for the first one, possibly too loud in
  headphones.
- Vibration does not change.
- Linux: nothing (it has no channels; the bell plays on the chosen
  output).
- Left out: the Bluetooth lead-in for sleeping devices (Janek: later);
  per-mode switches and presets carrying it (one global choice is
  enough for one pair of headphones).

## Decisions
1. One global switch in Preferences, with an explanation (Janek,
   2026-10-29).
2. Media volume fixed like the alarm, not following the media volume
   (Janek, 2026-10-29); in headphones up to the hearing-safety step
   (Janek, 2026-10-23, kept).
3. Wording (Janek, after the device test, 2026-10-29): group
   "Glocken"; explanation "Wenn du Probleme mit Glocken hast, die nicht
   abgespielt werden, kannst du diese Option aktivieren. Damit werden
   die Glocken über den Alarmkanal abgespielt, was die sicherste Option
   ist."; switch "Glocken als Alarm abspielen". English msgids:
   "Bells"; "If you have trouble with bells not playing, you can turn
   this option on. Bells then play on the alarm channel, which is the
   most reliable option."; "Play Bells as Alarm". The other seven
   languages by me.
4. Linux shows no row (Janek, 2026-10-29).
5. My calls: synced like the other preferences; headphones counted at
   the moment each bell starts; a bell that is playing when the switch
   changes keeps its channel; the bell stops when its output changes
   (extends Janek's 2026-10-23 decision for headphones going away); the
   media step is put back on the output it was set on, once that output
   is current again; no audio focus is requested, so other apps' music
   neither pauses nor ducks.

## Capabilities
### Modified Capabilities
- `bells`: a bell plays as media, at a fixed media step, unless set to
  play as an alarm.

## Impact
- Core: `settings_keys` (key and reader), `bell_volume` (the headphone
  curve capped at the safe step).
- GTK: none.
- Android: `kotlin/MeditateAudio.kt` (media path, a media hold beside
  the alarm hold, stop on headphones gone), `src/audio.rs` (JNI: the
  flag, the media curves), `src/alarm_volume.rs` (media levels),
  `src/ui.rs` (read and write the switch, pass it to every play site),
  `src/app.rs` (JNI table), `ui/main.slint` (the group), nine
  `lang/*/meditate-android.po`.
- Translations: three new strings, Android only.
- Data: a new synced setting key, additive; no migration.
- Android build environment: untouched.
- Docs: README features (Bells line).
