# Preset keeps its bells

## Why
A preset finds its interval bells in the bell list and switches them
on. When a bell is edited in Setup (another sound, time, volume), the
row itself changes, so every preset ringing it plays the edited bell
from then on instead of the bell it was saved with.

## What Changes
- Editing an interval bell that a saved preset uses puts the edit into
  a new bell at the same place in the list, switched on as the old one
  was; the old bell stays as it was, switched off, for the preset.
- Further edits on the same edit page change the new bell in place
  (no preset uses it yet). A bell no preset uses is changed in place,
  as today.
- Applying a preset is unchanged: its bell was never altered.
- Left out: repairing at apply time (re-made bells, derived uuids,
  copies after a sync; dropped for this simpler fix, Janek). Edits made
  in place by an older app version on another device still change a
  preset's bell until that device updates.

## Decisions
- Copy at edit time, not repair at apply time (Janek, 2026-10-30 "viel besser, so machen wir es").
- The edit not saved to any preset stays as its own bell (Janek, same).
- "A preset uses the bell": a saved Timer preset with interval bells on
  rings a bell (on) with that row's id, or, saved before bells carried
  their id, one with the row's settings before the edit (my call; off
  bells and Box Breath presets don't count, every preset stores the
  whole list).
- A save that changes no setting copies nothing (my call).
- GTK edit page: Delete after a copy deletes the new bell (my call).
- Two devices editing the same bell offline make the same copy (its id
  is derived from the old bell's), so the later edit wins, as for any
  edit (Janek, 2026-10-30, instead of two copies ringing).
  Accepted limits (Janek): a preset saved with the copy on one device
  rings the other device's edit if that one wins; an edit made offline
  is lost when the same copy was deleted elsewhere meanwhile.
- An old bell no preset rings any more after an override stays in the
  list, off; no automatic clean-up (Janek, 2026-10-30 "Liegen lassen").
- Linux shows the hint in full in a toast whose title wraps (a plain
  toast title is one line, cut with "..." at 360 px; tried both on
  libadwaita 1.7; Janek, 2026-10-30). The GTK save/delete failure
  toasts share that toast and wrap too (my call: same look when short).
- The copy sits right next to the old bell, above or below by uuid,
  the same on every device (my call).
- The first copy tells the user once: "Saved as a new bell, your
  presets keep the old one" (toast on Linux, snackbar on Android;
  Janek, 2026-10-30).
- Switching a bell on or off and deleting it are not edits: unchanged (my call).
- Copy and switch-off are one transaction (hard rule: all or nothing).

## Capabilities
### Modified Capabilities
- `presets`: a preset's interval bells survive edits in Setup

## Impact
- Core: `preset_config.rs` (new `edit_interval_bell`: decides copy or
  in place, returns whether it copied and the bell now holding the edit), `db/interval_bells.rs` (new copy-and-switch-off in one
  transaction)
- GTK: `bells.rs` `write_back_bell` calls the core edit, follows the
  returned bell (Delete too) and shows the toast on a copy
- Android: `ui.rs` interval editor Save calls the core edit and shows the snackbar on a copy
- Translations: one new string in both apps, nine languages each
- Data: none; the copy is an ordinary bell insert event, the switch-off
  an ordinary update event
- Android build environment: untouched
- Docs: none
