# Bell ring

## Why
GitHub #14: a glance at the running page should tell how far the session
is and which repeat of a repeating bell just rang, without a second
ticking number. A ring of beads around the time does both: it fills as
the session runs, each bell is a bead on it, and a small counter shows
the repeat number.

## What Changes
- Timer countdown and Guided: a ring around the running time fills
  clockwise from the top, live, and stops while paused.
- Each bell of the session is a bead on the ring at its time: repeating
  bells small, one-off bells larger, the start point largest. A bead that
  has rung is filled, one still to come is hollow.
- Random bells ring on a fixed grid: each ring at its planned time ±
  the jitter (5 ±1, 10 ±1, 15 ±1), whenever the ring before it came. A
  ring that can come before the end rings before the end. Today each gap
  is drawn from the last ring, so the errors add up.
- Each bead of a random bell sits at the earliest time its ring can
  come. Once the fill reaches it, it rides along at the front of the
  fill, hollow, until it rings, and stays where it rang.
- Under the time, one counter per repeating bell, "2 of 5", ordered by
  first ring. In the Timer stopwatch only the number, "2", and no ring.
- During preparation the ring is empty and a small ring around the
  start point fills; at the starting bell it goes and the start point
  fills.
- In overtime the ring is full and counters show only the number.
- Box Breath: unchanged, no ring and no counter.
- Left out: the accent-coloured time in overtime from mockup H. The time
  already stays frozen at the planned length and the full ring says the
  end was reached; a colour change adds nothing.
- Left out: labels on the beads, and shapes per bell kind (decided
  against).

## Decisions
- Ring, not a line (Janek; the line is too short for dense bells).
- Timer and Guided, both apps (Janek).
- Bead shapes: all circles; repeating small, one-off larger (Janek).
- Legend: only repeating bells, counted; none for one-off bells (Janek).
- Random bells on a grid, in this change; the last window is cut at the
  end so it always rings (Janek). Two rings can come seconds apart with
  a jitter of half the time (accepted).
- Random beads: each at its earliest time, rides along with the fill,
  hollow, until it rings, then stays (Janek).
- Two repeating bells: one counter each, order fixed at the start by
  the earliest first ring, ties by the bell list (Janek).
- Stopwatch: counter as a plain number, no ring (Janek).
- Preparation: small ring around the start point (Janek's idea); it
  goes at the starting bell (my call).
- Overtime: full ring, counters without "of", still counting (my call:
  bells keep ringing in overtime, the total no longer fits).
- "of" total: the beads of that bell on the ring, fixed at the start (my
  call). A bead skipped by a suspend stays hollow (my call).
- The ring follows the session: on the phone with the existing 200 ms
  refresh, on the laptop with its own quarter-second refresh of the
  running page, so a counter moves within a quarter second of its bell
  (my call, after review: the existing tick has no hook for the page).
- Ring size: laptop a fixed 300 px, a short window scrolls as the
  running page already does (Janek's earlier "wie am Handy: scrollen");
  phone min(320 px, the column), 220 px on short screens. The time
  shrinks when it shows hours, to fit inside the ring (my call).

## Capabilities
### Modified Capabilities
- `timer`: the running page shows the session progress and the bells.
- `bells`: random bells ring on a grid; the last one before the end
  always rings.

## Impact
- Core: `bells.rs` (grid schedule, ring history), `session/mod.rs` (a
  read of the ring and counters for the shells).
- GTK: `src/window/imp.rs` running page (a DrawingArea ring around the
  time label, the counter row), `src/timer/imp.rs` (redraw on tick).
- Android: `ui/main.slint` running block (Material circular progress
  plus bead positions computed in Rust, as Box Breath's dot already is),
  `src/ui.rs`, `src/app.rs`.
- Translations: new "{rung} of {total}" (GTK) / "{} of {}" (Android),
  nine languages each.
- Data: none. Nothing is stored; the ring is computed from the running
  session.
- Android build environment: untouched.
- Docs: README features list; living timer spec.
