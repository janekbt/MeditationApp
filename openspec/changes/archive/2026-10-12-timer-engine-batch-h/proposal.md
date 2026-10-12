# Timer engine: right minutes, bells at the right time (batch H)

## Why
After a suspend, a blank screen or a late tick, the session engine
saves the wrong length or rings at the wrong time. A Box Breath
session can be saved as 40 minutes instead of 5 (#39, worse on Linux
where Box Breath only ticks while its window is drawn, #166). A
countdown with preparation time restarts from zero after a suspend
(#45). Interval bells ring in a burst after a time jump (#46), or one
second after the end bell (#47). Box Breath ends with a stray
"Breathe in" cue (#72) and gives no cue for the very first inhale
(#113). On Linux, a session doesn't stop the system from suspending,
which is how it over-sleeps in the first place (#185).

## What Changes
- Box Breath with a set length saves that length, however late the
  ending tick arrives (#39).
- The countdown after preparation time keeps its real start time,
  so a late tick or a suspend no longer restarts it (#45).
- After a time jump, each interval bell rings at most once and
  continues from the next due time (#46).
- An interval bell due exactly at the end gives way to the end bell
  (#47).
- Box Breath ends without a cue for the next cycle (#72) and cues the
  first inhale at the start, when cues are on (#113).
- On Linux, Box Breath runs on a 200 ms timer like the Android app,
  not on screen redraws; redraws only draw (#166).
- On Linux, a running session blocks automatic suspend, every
  time, not only with "Keep screen awake" (#185). Android already
  holds a wake lock.
- A paused session no longer keeps the device awake, on Linux or
  Android: the suspend block (wake lock on Android) and the
  screen-on hold go with the pause and come back on resume.
- If GNOME or the portal refuses the suspend block, a message says
  so (new string, all nine translations).
- If the system suspends anyway (lid closed, Suspend chosen) during
  preparation and wakes after the countdown would have ended, only
  the end bell rings, not a minutes-late starting bell first.
- Kept (maintainer's call): after a wake-up in overtime, interval
  bells stay on their schedule from the session start, so one can
  ring seconds after the end bell.
- Left out: waking a suspended laptop for a bell (needs administrator
  rights); catching up skipped Box Breath cues after a suspend (the
  session simply ends, or continues from the current phase); a cue
  timing finer than 200 ms.

## Capabilities
### New Capabilities
- `timer`: session length and the countdown after preparation time.
- `bells`: when interval bells ring.
- `box-breath`: phase cues and the end of a Box Breath session.

## Impact
- Core: `meditate-core/src/session/mod.rs`, `meditate-core/src/bells.rs`.
- GTK: `meditate-gtk/src/timer/imp.rs` (tick, suspend inhibit), `meditate-gtk/src/window/imp.rs` (frame callback draws only).
- Android: `kotlin/MeditateSessionService.kt` (pause/resume actions), `src/service.rs`, `src/ui.rs` (wake lock follows pause); otherwise it already ticks every 200 ms and gets the core fixes.
- Translations: one new GTK string, all nine languages.
- Data: none (no schema or event change).
- Android build environment: untouched.
- Docs: ARCHITECTURE.md where it describes the GTK tick; README if it mentions keeping the screen awake.
