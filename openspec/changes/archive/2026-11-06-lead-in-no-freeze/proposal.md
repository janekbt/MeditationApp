# Lead-in without freeze

## Why
With the Bluetooth lead-in on, the Linux Flatpak hangs about 2 s before every bell (#199). Measured 2026-11-06 in the sandbox: the output check runs GStreamer's device monitor, its PipeWire provider cannot connect there, so it falls back to opening every ALSA card; waking the graphics card's sleeping HDMI audio takes 2.0 s, on the main thread. On the host PipeWire answers and the cards are never opened.

## What Changes
- Linux asks only the sound server (GStreamer's PulseAudio device provider, which already gave the right answer in the sandbox) which sink is default; the full device monitor stays only as the fallback when that provider is missing.
- The default sink is picked among the server's sinks only (the provider also lists inputs).
- Left out: a long-lived background monitor with a cached answer (the old #199 plan); a 0.02 s server query does not need it.

## Decisions
- Approach confirmed by the maintainer 2026-11-06 ("go").
- Fallback to the old monitor when the PulseAudio provider is missing (my call): keeps the lead-in working on a system without that plugin, at the old cost.

## Capabilities
### Modified Capabilities
- `bells`: the Linux output check never waits for a sleeping card

## Impact
- Core: `lead_in.rs` (pick the default sink)
- GTK: `src/sound.rs` (`default_output_is_bluetooth`)
- Android: none
- Translations: none
- Data: none
- Android build environment: untouched
- Docs: OPEN-ISSUES #199 / K4 removed
