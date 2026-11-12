# Bells that ring

## Why
P5-c of REFACTORING's P5 (session engine): #269, #279, #280, #293. The running page counts the starting and end bell as heard when they never rang (muted by the mode's signal setting, or the starting bell skipped by a suspend through preparation), so it says "1 of N" before any bell. After a suspend every missed fixed bell rings at once. The Bells subtitle counts bells that can't ring at the set length. Inside core, the four bell effects repeat the same fields and builder, so a new cue field costs about ten edits.

## What Changes
- The counter counts the starting and end bell as heard only when they rang (#280). A bell the mode's signal setting mutes (starting, end, fixed or repeating) is left out everywhere: no bead, not in the counter, not in the Bells count.
- After a suspend, of the fixed bells that came due, only the latest rings; the earlier ones stay unheard (hollow beads), as repeating bells already do (#279).
- The Timer's Bells subtitle and the main Bells row count only the bells that can ring at the set length, and follow a change of the length (#293).
- Manage Bells shows a switched-on bell that can't ring at the set length, or that is muted, as off and greyed, as Stopwatch already does for bells timed from the end; the saved switch stays (Janek, review round 1).
- The main Bells row leaves out a muted starting bell, end bell or phase cues; a preset's subtitle counts its bells by the same rule (Janek, review round 1).
- Inside: one bell effect carrying the cue, its channel and the Box Breath phase; `FireRoute` and `fire_route` go (#269). No visible change.
- Left out: #274 (Librem jank, its own item).

## Decisions
- A muted bell, of any kind, leaves the ring, the counter and the Bells count: one rule, what rings is shown (Janek, 2026-11-12).
- A skipped earlier fixed bell keeps its hollow bead, as a skipped repeating ring does (my call).
- Two fixed bells due at the same latest second both ring, as today (my call).
- One core rule decides whether a Timer bell can ring (length and signal setting): the Bells count, the Manage Bells switches and the preset subtitle use it (my call).

## Capabilities
### Modified Capabilities
- `timer`: the bell counter.
- `bells`: fixed bells after a suspend; the Bells count.

## Impact
- Core: `session/effect.rs`, `session/mod.rs` (one fire builder, muted bells dropped at start, the skipped starting bell, fixed bells after a jump), `session/settings.rs` (test), `bells.rs` (`RingLimits`, `interval_bells_count`, `bell_row_switch_state`), `preset_config.rs` (`bells_summary`), `format.rs` (preset subtitle).
- GTK: `timer/imp.rs` (dispatch reads the new effect; a saved length or signal setting refreshes the bell counts), `bells.rs` (Manage Bells switches), `sound.rs` tests that name the dispatch.
- Android: `ui.rs` (dispatch; a saved length or signal setting refreshes the counts; Manage Bells switches), `app.rs` and `alarm_volume.rs` (tests).
- Translations: none. Data: none. Android build environment: untouched.
- Docs: ARCHITECTURE.md if it names the effects, REFACTORING.md (P5 row; #269, #279, #280, #293 done), OPEN-ISSUES.md if listed.
