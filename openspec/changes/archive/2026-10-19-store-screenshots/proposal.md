# Store screenshots: current light/dark composites

## Why
GNOME Software shows no screenshots for 26.10.2: its metainfo links the
old `data/screenshots/` folder (404 since the crate move). Beta has the
right folder, but those images are from July and predate Guided and Box
Breath.

## What changes
- The metainfo's three screenshots become the README composites (light
  and dark, split diagonally): Timer, Guided and Box Breath, with new
  captions, translated into all nine languages.
- The FlatPark package draft (in the archived flatpark-tarballs change)
  gets the same translated screenshot block.
- A host test checks that every metainfo screenshot exists in the repo.

## Decisions
- Janek: captions "Timer mode with presets and daily streak", "Guided
  mode with your own audio files", "Box Breath mode with adjustable
  breath phases".
- Janek: update the FlatPark draft too.
- My call: the old July images stay in the repo (deleting needs a go);
  URLs keep pointing at `main`, so the store shows them from the next
  release.
