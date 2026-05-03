# Bundled bell sounds — sources and licenses

All files in this directory are bundled into the `meditate` gresource at build time and installed under `/io/github/janekbt/Meditate/sounds/`. The `bell_sounds` table in the app references them by stable UUID (see `BUNDLED_BELL_SOUNDS` in `src/db/mod.rs`); appending new entries is a one-tuple addition with a fresh UUID — never reorder or rename existing rows, peer DBs depend on those UUIDs.

The post-import processing pipeline (run once when adding) is:

```
ffmpeg -i <source> [-ss <trim>] \
  -af "loudnorm=I=-16:LRA=11:TP=-1.5,aresample=48000" \
  -ac 1 -c:a libvorbis -q:a 4 <slug>.ogg
```

Mono 48 kHz / OGG Vorbis q4 keeps the bundle small (a 30 s gong tail is ~250 KB) without audible loss for percussive bell-shaped audio. EBU R128 normalisation to −16 LUFS lands a 0.3 s woodblock click and a 30 s bonshō tail at comparable perceived volume.

## bowl.wav · bell.wav · gong.wav

The original three placeholder bundles. CC0 — no attribution required.

## Expanded set (CC0 except where noted)

| File | Source | License |
|---|---|---|
| `tibetan-bowl-medium.ogg` | [Tibetan singing bowl for meditation](https://freesound.org/people/franciscoguerrero/sounds/436935/) by *franciscoguerrero* (Freesound) — trimmed to start at 20 s (the strongest of three takes in the original). | CC0 |
| `inkin.ogg` | [Bell Meditation](https://freesound.org/people/fauxpress/sounds/42095/) by *fauxpress* (Freesound). | CC0 |
| `tingsha.ogg` | [Tingsha Cymbal](https://freesound.org/people/steffcaffrey/sounds/435074/) by *steffcaffrey* (Freesound). | CC0 |
| `bonsho.ogg` | [Montbell (Bonshō) von Japan](https://freesound.org/people/Vurca/sounds/397352/) by *Vurca* (Freesound). | CC0 |
| `burmese-brass.ogg` | [Buddhist Prayer Bell](https://freesound.org/people/surly/sounds/91196/) by *surly* (Freesound) — trimmed to start at 4 s. | CC0 |
| `chau-gong.ogg` | [Gong](https://freesound.org/people/juskiddink/sounds/86773/) by *juskiddink* (Freesound). | **CC-BY 4.0** — attribution required, satisfied by this file. |
| `crystal-bowl.ogg` | [Crystal bowl F#3](https://freesound.org/people/caiogracco/sounds/150454/) by *caiogracco* (Freesound). | CC0 |
| `woodblock.ogg` | [Wood block hit](https://freesound.org/people/thomasjaunism/sounds/218460/) by *thomasjaunism* (Freesound). | CC0 |
