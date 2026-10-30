# Smooth ring

## Why
On the phone the session ring jumps five times a second while Box
Breath's dot glides: the dot animates each 200 ms step, the ring is
set straight to the new value, so the phone redraws it only on ticks.

## What Changes
- Android: the big ring and the small preparation ring glide over each 200 ms tick (linear, 200 ms), like Box Breath's dot. Nothing else changes: what the ring shows, when, and the beads.
- Linux: the ring was redrawn every 250 ms; it now follows the frame clock (a tick callback on the drawing area reads the session's ring each frame while it moves; a still ring stops it, and the quarter-second refresh starts it again when the ring moves, so a paused ring costs nothing). The bell counter keeps its 250 ms refresh.
- Left out: beads (they only switch colour).

## Decisions
- Linux too (Janek: "linux mit reinnehmen", taken as the go).
- The glide hides a negative start (preparation's -1 "none") by clamping at 0 (my call).
