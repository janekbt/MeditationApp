## ADDED Requirements
### Requirement: A Bluetooth bell can wake its output first
With "Wake-up Lead-in for Bluetooth" on, every bell and preview whose output is a Bluetooth device SHALL be preceded by a soft noise for the set length and a fixed 0.05 s of silence counted from where the noise really finished playing; on Android on the bell's own stream and output (media, or the alarm aimed at the headphones), on Linux when the default output is a Bluetooth sink. Other outputs get no lead-in.

#### Scenario: The lead-in sound
- **WHEN** the lead-in is built for 100 ms at 48000 Hz
- **THEN** it holds 4800 stereo frames of noise, never above 1000 of 32768 and starting and ending at 0 (20 ms fades), then 2400 frames of silence; the same length always gives the same samples, and its WAV holds exactly them
- **TEST** `meditate-core/src/lead_in.rs::the_lead_in_is_noise_then_a_short_silence`

#### Scenario: Which Linux output is Bluetooth
- **WHEN** a sink's properties have `device.api` "bluez5"; "bluez" (PulseAudio); a `node.name` "bluez_output.XX.1"; "bluez_sink.XX.a2dp_sink"; `device.api` "alsa" with `node.name` "alsa_output.pci"; none
- **THEN** Bluetooth, Bluetooth, Bluetooth, Bluetooth; not, not
- **TEST** `meditate-core/src/lead_in.rs::a_bluez_sink_counts_as_bluetooth`

#### Scenario: The player on Android
- **WHEN** a bell or preview starts with the lead-in on and its output is a Bluetooth device
- **THEN** the noise plays on a track with the bell's audio attributes, aimed at that device, at the bell's step; the bell starts when the track's playback reaches its end (noise and silence played), a bell due while a lead-in on the same output runs waits for that end and gets no noise; the track lives while any bell waits on it (a stopped preview's player leaves it; previews never play during a session on Android, so a preview stop stops all audio as before); stop and the output changing end it; a refused aim is logged; the bell starts at the planned time plus 5 s if the marker never comes (a cold link stalls the track); wired, USB and the speaker start at once
- **TEST** `meditate-android/src/alarm_volume.rs::a_bluetooth_bell_waits_for_its_lead_in`

#### Scenario: The vibration waits for the sound
- **WHEN** a session bell with sound and vibration rings with a lead-in
- **THEN** the vibration waits as long as the sound (the planned wait, or what is left of a running lead-in), on Android and on Linux; a Stop meanwhile drops it
- **TEST** `meditate-android/src/app.rs::a_bell_vibrates_when_its_sound_starts`; `meditate-gtk/src/sound.rs::the_lead_in_plays_before_a_bluetooth_bell`

#### Scenario: The player on Linux
- **WHEN** a bell or preview starts with the lead-in on and the default sink is Bluetooth
- **THEN** core's noise plays first and the bell starts at the noise's end of stream (its silence included), or 5 s after the planned end if that never comes; a bell due meanwhile waits for it; Stop cancels both; closing a chooser drops only its preview; another sink starts the bell at once; not finding the default sink is logged
- **TEST** `meditate-gtk/src/sound.rs::the_lead_in_plays_before_a_bluetooth_bell`

#### Scenario: On the devices
- **WHEN** the FP5 with the Bluetooth box, media and then "Play Bells as Alarm", lead-in off then on at 0,10 s and 1,00 s, Test tapped after 60 s of silence, a Timer session with interval bells, a Box Breath session with sound cues; the laptop with the box
- **THEN** off: the asleep box swallows the woodblock; on and set right: only the bell is heard, the phone speaker silent on both channels; the bell rings late by the set time; vibration with the sound; Stop during the hiss ends both
- **MANUAL** routing, waking and timing exist only on a device
