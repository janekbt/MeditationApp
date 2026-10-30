# Session buttons

## Why
Ending a session early needs Pause first, then Save. On the phone,
Discard on the session screen is a thin red outline, which doesn't
look like the laptop's red pill Janek likes. The ring's beads that
have not rung yet are grey or white outlines instead of the accent.

## What Changes
- While a session runs, Save sits next to Pause, in both apps (the
  rule is in core) and in the phone's media control (Pause, Save,
  Discard). Saving keeps the time so far, like Pause then Save. During
  preparation it stays Pause alone.
- Phone: Discard on the session screen becomes the laptop's flat red
  pill: red text on a faint (15 %) tint of the same red. The confirm dialogs stay as they are.
- Both apps: beads not rung yet, and the start point before it fills,
  are outlined in the ring track's faint accent (Janek, after the device
  test: the full accent was too bright).

- Both apps: one counter under the time for all the session's bells,
  starting and end bell included ("4 of 8"), instead of one per
  repeating bell; none when no bell rings between start and end.

## Decisions
- Discard looks like the laptop's (Janek, chose "Wie am Laptop").
- Save while running saves without a note field; whoever wants a note
  pauses first. A note typed in an earlier pause is saved with it
  (Janek: "Wenn jemand eine Notiz schreiben will, kann er immer noch
  auf Pause drücken").
- No Save during preparation: those seconds never count (my call, as
  the paused-preparation row already does). When preparation ends, Pause
  slides aside as Save joins it (Janek: "Verschieben ist okay").
- Laptop: Save stays the accent-filled button next to the plain Pause,
  as it is when paused (my call).
- Discard takes the light scheme's error red for text and tint, the
  mid red libadwaita uses; the dark scheme's error is pale pink. Measured
  from Janek's laptop screenshot: tint #1D0000, text about #C30000 (my call).
- The headphone button keeps toggling Pause only (my call).

- One counter for all bells (Janek: option 1 of three, 2026-10-30), with
  the starting and end bells counted when on (Janek: "jo"). In overtime
  the total goes as before, since bells ring on (my call: unchanged).
- No counter for a session with only a starting and an end bell, so a
  plain session stays as it was (my call, told at hand-over).

## Capabilities
### Modified Capabilities
- `timer`: the running row, the media control's row, bead outlines,
  one counter for all bells.
- `appearance`: Discard on the phone's session screen.

## Impact
- Core: `session/mod.rs` (end_buttons) and its tests.
- GTK: `window/imp.rs` (bead outline colour); `timer/imp.rs` refreshes
  the row when preparation ends (it only did on pause, resume, overtime).
- Android: `ui/main.slint` (WrapButton/EndButtonView, BellRingView),
  tests in `src/app.rs` (media row, red Discard).
- Translations: none (Save, Pause exist).
- Data: none. Android build environment: untouched.
- Docs: README if it lists the session buttons.
