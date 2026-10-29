# Bluetooth wake-up lead-in

## Why
Bluetooth headphones and speakers fall asleep after 30 to 60 s of
silence; waking, they swallow the start of a bell, short bells (the
284 ms one) entirely. No Bluetooth command wakes an amplifier, only
sound does. The first attempt (2026-10-22 to 10-29, in `git stash`)
built a lead-in noise; it was dropped with the per-mode switch. Now
bells play as media by default and as an aimed alarm with the switch,
so the lead-in must work on both channels. Part of GitHub #10.

## What Changes
- Preferences, a new group "Bluetooth" right after "Glocken" on Android
  and after "Statistics" on Linux: the explanation under its heading,
  a switch "Aufweckvorlauf für Bluetooth-Geräte" (off by default), the
  length "Dauer (Sekunden)" 0,10 to 5,00 s in 0,01 s steps, default
  0,10 s, sliding away while the switch is off, and a row "Test" with a
  button "Abspielen". Synced like every preference.
- On, and a bell or preview goes to a Bluetooth output: a soft noise
  plays for the set length, then 0,05 s of silence, then the bell. The
  silence is counted from where the noise really finished playing, not
  from when the app started it. Wired and USB headphones, the speaker:
  no lead-in.
- Android: the noise plays on the bell's own stream and output, media
  or the aimed alarm, at the bell's step. Linux: when the default output
  is a Bluetooth sink.
- A bell due during another's lead-in on the same output waits for it
  and gets no noise of its own. A bell's vibration starts with its
  sound. Stop ends the noise and every waiting bell; a closed editor
  ends only its preview on Linux (a session bell waiting on the same
  noise keeps it; on Android previews never play during a session); on Android the output changing ends the noise and its bells.
- Test plays the woodblock at volume 50 as a bell would: on the current
  output, with the lead-in when it is on and the output is Bluetooth.

## Decisions
1. Architecture of the first attempt (decision 11 there): the set time
   is noise alone at one level (white, stereo, about -30 dBFS, 20 ms
   fades), then a fixed 0,05 s of silence, then the bell; global, not
   per mode; the bell rings later by the lead-in (Janek, 2026-10-29).
2. Both channels on Android (Janek).
3. The silence counted from the noise's real end (Janek's "not good
   enough" on 2026-10-29 for counting from its start).
4. Linux too, as in the first attempt (Janek).
5. Its own group "Bluetooth" (Janek); default 0,10 s (Janek).
8. A lead-in that fails to play is only logged; the bell plays as
   without one (Janek, 2026-10-29: a broken audio system already fails
   the bell itself, which has its own failure path).
6. Wording: the first attempt's strings, kept: "Aufweckvorlauf für
   Bluetooth-Geräte" / "Wake-up Lead-in for Bluetooth", "Dauer
   (Sekunden)" / "Length (Seconds)", "Test", "Abspielen" / "Play", and
   the explanation (Janek, 2026-10-25); new: the group title
   "Bluetooth" (the same in every language).
7. My calls: stored as `bluetooth_wake_lead_in` (bool) and
   `bluetooth_wake_lead_in_ms` (100 to 5000, rounded half up to 10 ms),
   new names because the first attempt's `bluetooth_lead_in` may sit in
   synced data; the noise at a fixed level, not trimmed by the bell's
   gain (it must open the amplifier), on the bell's step, and its track
   aimed at the device on both channels; vibration delayed by the planned
   wait in both apps (noise plus 0,05 s, or what is left of a running
   lead-in; it can lead the bell by the noise's start delay, tens of
   ms), and dropped by a Stop meanwhile; on Linux the bell itself starts
   at the noise's end of stream; a bell whose marker never comes starts at the planned
   time plus 5 s (a cold Bluetooth link stalls the track before it
   plays); a bell that never rings still vibrates; Test toggles
   Play/Stop over noise and bell like a volume preview (the preview
   slot, stoppable during the hiss); the Linux Bluetooth check accepts
   `device.api` starting with "bluez" or `node.name` starting with
   "bluez_" (PipeWire and PulseAudio) on the sink marked `is-default`
   (checked on the laptop, 2026-10-29); Linux watches no output change
   during a lead-in; the noise and
   the Bluetooth check of a Linux sink in core (`lead_in`), both apps
   play the same samples; the join rule stays in each shell (it keys on
   the output, which core doesn't know); no skipping the noise when a
   sound played shortly before.

## Cost (accepted with decision 1)
With the lead-in on, each Bluetooth bell rings late by the set time
plus 0,05 s and is preceded by a soft hiss. Box Breath sound cues get
their own hiss each; a lead-in longer than a phase makes two cues ring
together.

## Capabilities
### Modified Capabilities
- `bells`: a Bluetooth bell can get a lead-in.
- `preferences`: the Bluetooth group.

## Impact
- Core: `lead_in.rs` (new: samples, WAV bytes, Bluetooth sink check),
  `settings_keys.rs` (two keys and readers).
- Android: `kotlin/MeditateAudio.kt` (noise track, marker start, join,
  stop), `src/audio.rs` and `src/app.rs` (JNI: the samples, the wait),
  `src/ui.rs` (read the setting at each play site, vibration waits, the
  group's callbacks, Test), `ui/main.slint` (the group), nine `.po`.
- GTK: `src/sound.rs` (Bluetooth default sink via GStreamer's device
  monitor, noise then bell, join, Stop), `src/timer/imp.rs` (vibration
  waits), `src/preferences.rs` (the group), nine `po/*.po` via
  `build-aux/update-translations.sh`.
- README Bells line. No schema change (two new setting keys), no new
  dependency, Android build environment untouched.
