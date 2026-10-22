# Tasks: headphone-bells

Base: f26dced (tests run on 9d3ecf6, which differs only in OPEN-ISSUES.md:
core 1390, android 261, gtk 162, all green). Re-run on the base before
building.

## Loop
- [ ] 0. ponytail ultra + slint:slint loaded (untick after /compact)
- [ ] 1. Understand (said back and confirmed; decisions 1 done, 2-3 open)
- [ ] 2. Propose
- [ ] 3. Build, test first
- [ ] 4. Review + /simplify
- [ ] 5. Specs, docs, list
- [ ] 6. Verify and hand over (FP5 test by Janek)
- [ ] 7. Ship

## Research (2026-10-22, AOSP source and app code; nothing tried on the FP5)
- Alarms (STRATEGY_SONIFICATION) go to speaker + headset by design
  (AOSP Engine.cpp, EngineDefaultConfig.h).
- `MediaPlayer.setPreferredDevice(headset)` is honoured for USAGE_ALARM
  too (AudioPolicyManager getOutputForAttrInt -> Engine returns only the
  preferred device). Caveat: another alarm client active at the same
  time (clock app) can bring the speaker back.
- But the alarm on a headset has its own per-device index that apps can
  neither read nor set (`setDeviceVolume` is SystemApi);
  `setStreamVolume(STREAM_ALARM)` with dual routing lands on the speaker
  index. Android also subtracts 6 dB for sonification on headsets and
  caps it at the music volume if music played in the last 5 s. On
  Bluetooth with absolute volume it tracks the headset's media volume.
  So alarm-to-headset is not repeatable in loudness.
- `getStreamVolumeDb(stream, index, TYPE_WIRED_HEADPHONES)` works with
  no headset connected (static device category), so the headphone curve
  can be read while the user sets the slider. It includes the -6 dB /
  music cap for sonification streams, not for MUSIC.
- Safe media volume: over headphones `setStreamVolume(STREAM_MUSIC)`
  above the safe index (AOSP default 10 of 15) is not applied; a warning
  dialog waits for the user. No public API reads the index; assume
  10/15 of max. CSD (Android 14+) can force the volume down at high dose.
- dB values are dBFS: "as loud as the speaker" can't be computed for
  headphones; only "as loud as last time with the same headphones".
- Other apps: Medito plays bells as USAGE_MEDIA and follows the media
  volume; Sleep as Android's headphones-only alarm uses the media stream
  and raises it to max. None found that does calibrated loudness.

## Proposal under discussion (not approved)
- With headphones and the switch on: play as USAGE_MEDIA with
  `setPreferredDevice(headset)`.
- The bell slider maps to a fixed headphone range, quiet up to the safe
  index (never above, so no warning dialog), read from the headphone
  curve. Same preset = same media step every time.
- Set STREAM_MUSIC to that step for the bell and restore it afterwards,
  like the alarm today (same KEY_RESTORE / takenOver logic). Cost: music
  or a Guided track playing at that moment gets louder/quieter for the
  bell's length.
- Later, only if the fixed range feels wrong: a separate headphone
  volume slider with a headphone preview.

## Next
1. Ask Janek: approve a throwaway spike on the FP5 (debug build, no
   commit) that logs: whether preferred-device keeps the speaker silent
   (alarm and media), the FP5's MUSIC/ALARM headphone curves, the safe
   index behaviour, Bluetooth behaviour. Ask whether he has wired or
   Bluetooth headphones.
2. Then settle decisions 2 and 3 and the switch's wording (subtitle, nine
   languages), and write the deltas (area: bells).
