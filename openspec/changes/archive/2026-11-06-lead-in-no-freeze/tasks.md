# Tasks: lead-in-no-freeze

Base: 2f5e73356c16967b51c83395919b53669c89e128; suites on it: green (core 1463, GTK 171)

## Loop
- [x] 0 Skills loaded: `/ponytail ultra`; no Android UI
- [x] 1 Understand (measured in the Flatpak, see proposal)
- [x] 2 Propose (approved: 2026-11-06)
- [x] 3 Build
- [x] 4 Review, then `/simplify` after the last review
- [x] 5 Specs, docs, list (check-specs green; OPEN-ISSUES K4 ticked, #199 removed; no other doc names the probe)
- [x] 6 Verify and hand over (maintainer 2026-11-06: no hang, hiss with headphones, all passed)
- [x] 7 Ship

## Review Focus
- PulseAudio provider missing -> fallback to the monitor, logged; no scenario (a missing plugin cannot be set up in a test)
- Sound server down -> provider start fails: logged, falls back to the device monitor (R1); in the Flatpak that brings back the 2 s card scan in this one case, harmless since nothing can play without the server
- Real PulseAudio (Librem 5) vs PipeWire's pulse server -> `device.api` "bluez" / "bluez5" both covered by `a_bluez_sink_counts_as_bluetooth`
- A Bluetooth headset's microphone as default input -> `only_the_default_sink_decides_the_lead_in`

## 1. Default sink in core
- [x] 1.1 `lead_in::default_sink_is_bluetooth` over (class, is-default, api, node) entries, None without a default sink. Files: meditate-core/src/lead_in.rs. Proof: `only_the_default_sink_decides_the_lead_in`, Expected: FAIL then PASS

## 2. GTK asks the sound server only
- [x] 2.1 `default_output_is_bluetooth` uses the `pulsedeviceprovider` (start, devices, stop) and core's pick; the monitor only when the provider is missing. Files: meditate-gtk/src/sound.rs. Proof: MANUAL scenario "A bell in the Linux sandbox with the HDMI audio asleep", reproduced on the host with `PIPEWIRE_REMOTE` pointing nowhere, before and after (GST_DEBUG timings)

## Notes
- Pre-build questions (fresh agent), answered (my call):
  core takes one plain tuple (class, is_default, api, node) per device, first default sink wins;
  class from `device_class()`, exact "Audio/Sink"; a missing `is-default` counts as false;
  plain PulseAudio sets `device.api` "bluez" on its sinks, so no `internal-name` fallback;
  the monitor fallback goes through the same core pick;
  `is-default` IS in the pulse devices' properties (seen in the 2026-11-06 sandbox probe);
  the source-text assertion in `the_lead_in_plays_before_a_bluetooth_bell` that quotes the old
  `.find(... "is-default" ...)` line is updated with 2.1; log line when the provider is missing:
  "pulse device provider missing, using the device monitor" (scope sound.lead_in).
- 1.1 failed first (stub None vs Some(false)), passes after the pick.
- 2.1: the updated assertions in `the_lead_in_plays_before_a_bluetooth_bell` failed on the old code, pass after.
  The host has no ALSA device provider installed (no gstreamer1.0-alsa), so the stall cannot be reproduced
  there with the app. Reproduced inside the Flatpak sandbox with a Python probe instead (HDMI audio suspended):
  full monitor 1.993 s and the HDMI audio woke; pulse provider 0.006 s, HDMI stayed asleep. The app-level
  check of the fix needs a sandboxed build (see step 6).
- 2.1 stays unticked until that check.

## Review fixes
- R1 (review, minor, holds): a pulse plugin without an answering server returned "not Bluetooth" with no fallback,
  where the old monitor's PipeWire provider still answered (host install on PipeWire without pipewire-pulse).
  Fixed: a failed server start also falls back to the device monitor, logged. Proof: assertion in
  `the_lead_in_plays_before_a_bluetooth_bell` (both fallbacks log "using the device monitor"), failed first, passes.
- R2 (fix review, minor, holds): the R1 assertion only counted the log texts; now each fallback arm must end in None. Checked by putting `return false` after the server-failure log: the test failed; restored.

## /simplify
- Applied: the device list in one `match` with a `monitor_devices()` helper for both fallbacks; both
  `devices()` stay `glib::List` (no collects); the fallback assertions check the log lines and that both
  arms call the helper (checked by replacing one call with `None`: failed; restored). GTK 171 green, clippy clean.
- Skipped: a long-lived monitor or a worker thread (altitude, efficiency): the dropped old plan, not needed for
  a 0.006 s query; the slow path is left only where the server can't answer, at the old cost.
- Skipped: filtering the class in the shell instead of core (altitude): which device decides is a decision, approved in core with its test.
- Skipped: keeping one started provider (efficiency): a few ms per bell, more code.

## Hand-over
Artifacts: meditate-gtk/builddir/src/meditate (11:03:37), copied to
~/.var/app/io.github.janekbt.Meditate/cache/probe/meditate-dev; started inside the Flatpak sandbox through
`CAPPED_COMMAND="sh .../probe/run-dev.sh"` (own test data in .../probe/data, never the real data).

Test steps (maintainer):
1. In the test version: Preferences, Bluetooth, switch the lead-in on.
2. Without Bluetooth headphones: wait a minute, press Test; three times. The window must not hang; no hiss.
3. With Bluetooth headphones as output: Test again after a minute of silence: hiss, then the bell, no hang.
4. A short Timer session with an interval bell, headphones on: no hang before start or interval bell.

Commit message (proposed):
    sound: Ask only the sound server whether the output is Bluetooth

    With the Bluetooth lead-in on, the Flatpak hung about 2 s before
    every bell: GStreamer's device monitor cannot reach PipeWire in the
    sandbox and opens every ALSA card instead, waking the graphics
    card's sleeping HDMI audio. The PulseAudio provider answers in a few
    milliseconds; the monitor stays as the fallback.
