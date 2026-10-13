# GTK timer and bells (batch I)

## Why
Six bugs in the Linux app's timer screen and bell settings. The Duration
row shows the wrong mode's length after Stats (#73). Esc in overtime
pauses a session that can't be paused, after which "Pause" ends it and
drops the overtime (#126). A sound-only bell cuts a running vibration
(#127), and a replaced long vibration keeps sending its old parts
(#77). A volume change is lost when the bell page closes within 0.4 s
(#53), or lands in the wrong mode when the mode is switched within
0.4 s (#132). #130 (suspend during a session) was already fixed in
batch H, with its test; it only leaves the list.

## What Changes
- The Duration row always shows the length of the mode on screen.
- Esc pauses only a running or preparing session; in overtime, while
  paused and on the Done page it does nothing.
- A bell without vibration leaves a running vibration alone; Pause,
  Stop and Discard still cut it.
- A replaced vibration (bell, pattern preview, pattern editor preview)
  stops sending its later parts.
- A volume change is saved for the bell and mode the slider was moved
  in, even if the page closes, the window closes, the mode changes or
  a session starts within 0.4 s; only the preview is skipped then.
- Left out: Android: not affected (no Esc, vibration replaced only by
  a new one, volume saved on release), so no Android change.

## Decisions
- Volume preview, decided during the device test: as on Android, letting go of the slider saves and plays at once; the keyboard waits 0.4 s. Clicking levels quickly no longer crashes the app (it restarts the row's own player).
- Esc in overtime does nothing (maintainer, 2026-10-13). Ending the
  session there would save without asking.
- Long form of the /spec loop, plus a cold read of the plan by a fresh
  agent before it is shown (maintainer, 2026-10-13).
- Esc reuses core's existing toggle decision: it pauses only where the
  play/pause button would pause (my call; no new core logic).
- The volume row remembers the bell and mode when the slider moves and
  saves to that; one shared helper for the Setup rows and the interval
  bell page (my call).
- A reload of the rows (mode switch, sync) first saves a level that
  is still waiting for its 0.4 s; its preview is skipped then (my call).
- No preview plays once the volume row is no longer on screen, even
  if the closing animation still runs (my call, from the cold read).
- Start and closing the window also save a level still waiting
  (maintainer, 2026-10-13, after the review found both cases). The
  slider saves as soon as it leaves the screen, which covers a closed
  page and a closed window in one place (my call).
- GTK-only behaviour is proved by source-reading tests where the code
  needs a display (the existing pattern in `timer/mod.rs`); the
  vibration slot and its cancel flag get real unit tests (my call).

## Capabilities
### New Capabilities
- `vibration`: when a running vibration is kept, replaced or stopped.
### Modified Capabilities
- `timer`: Esc pauses only a session that can pause.
- `box-breath`: the Duration row shows the Box Breath length.
- `bells`: a volume change is saved for the bell and mode it was made in.

## Impact
- Core: none.
- GTK: `src/timer/imp.rs` (duration row writers, `on_pause` guard,
  vibration slot, volume rows), `src/vibration.rs` (`disarm`, slot
  helper), `src/volume_row.rs` (capture target, save first, flush),
  `src/bells.rs` (new `install` signature), tests in `src/timer/mod.rs`,
  `src/vibration.rs`, `src/volume_row.rs`.
- Android: none.
- Translations: none.
- Data: none.
- Android build environment: untouched.
- Docs: OPEN-ISSUES.md (remove #73, #126, #127, #77, #130, #53, #132;
  tick batch I); living specs through the deltas.
