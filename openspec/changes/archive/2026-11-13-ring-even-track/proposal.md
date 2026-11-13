# Proposal: ring-even-track

Why: Janek on the Librem 5: the faint, unfilled ring is not one even colour; beside every hollow bead it is brighter. The track and the hollow beads' outlines are the accent at 25 % alpha; where an outline crosses the track the two layers add up (about 44 %). The Android app draws the same way.

What changes: the track and the outlines of the hollow beads and the start point use one opaque colour, the accent at 25 % mixed onto the black running page. It looks the same as now, but a second layer of it cannot change anything.

Janek's screenshot from the FP5 (2026-11-13) also shows the outline of a hollow bead a shade brighter than the track: there the see-through outline lies on the bead's dark grey inside, not on black. The Android beads' and start point's inside becomes black like the page (Janek: "das Innere der Perlen auch bei Android schwarz machen"), and the opaque faint colour gives outline and track one shade.

Reaches: `meditate-gtk/src/window/imp.rs` `draw_bell_ring` (and its accent becomes a parameter so a test can draw it); `meditate-android/ui/main.slint` `BellRingView`'s `track` and the inside of its hollow beads and start point. No core, text, translation, data or build-environment change.

Decisions: Janek: both apps; Android bead insides black (2026-11-13). Mine: mixing onto black, since both running pages are black (GTK `.running-view-dark`, Android `#000000`).

After the device test (Janek, FP5): a riding bead moved jerkily while the arc glided; each bead's place now glides 200 ms like the arc (Janek: in this change).
