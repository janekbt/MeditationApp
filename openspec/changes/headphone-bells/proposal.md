# Bells only in the headphones (GitHub #10)

DRAFT, step 1 (Understand) not finished; nothing approved, nothing built.

## Why
GitHub issue #10 (marius-shark): the bells play as an alarm
(`MeditateAudio.kt`, `USAGE_ALARM`). Android plays alarms on the speaker
and the headphones at once, so on a train everyone hears the bell. The
reporter wants the bells kept to the headphones. Commit may say
`Fixes #10` (real GitHub issue).

## What changes (so far)
- Android only. GTK has no dual routing; the bell follows the chosen
  output there.
- A per-mode switch next to the Signal choice (Sound / Vibration / Both)
  in each mode's setup, like Keep screen awake. On: when headphones are
  connected, the bells play only there; without headphones they ring as
  today (alarm, speaker). Off: as today.
- Vibration stays as it is (my call: it makes no sound).

## Decisions
1. Where: per mode, next to Signal (Janek, 2026-10-22). Janek doubts a
   bare label explains it: the switch needs a subtitle; wording still to
   settle.
2. Volume with headphones: NOT "follow the media volume" (Janek: the
   same preset would ring at different loudness when the media volume
   happened to change). The same preset must ring equally loud every
   time. Open: how; see tasks.md, Research.
3. The bell volume is usually set and previewed without headphones, so
   the slider value must translate to headphones (Janek). Open.
