# One session end

## Why
P5-a of REFACTORING's P5 (session engine): #271, #270, #273, #275, #313, and the end half of #430. Core tells the apps about a session through effects that partly repeat its own state (display, end of preparation, overtime label, entering overtime) and through three ways of saying "ended". Each app folds them back by hand, in five places on Linux and three on Android, and they already disagree. Three bugs live in that gap: #277 (a session stopped while paused still reports Paused), #278 (a bell due before a guided track's early end rings just after the end bell), and a stop during preparation records the preparation seconds (OPEN-ISSUES HH, "plausible" line).

## What Changes
- A session stopped while paused counts as ended: the media control and the play key treat it as ended, not resumable (#277).
- When a guided track ends early, every bell due by its planned length is skipped, as at a timed end (#278).
- A stop during preparation records 0 seconds, never the preparation (HH).
- An ended session shows the length it saves, on both apps (Android showed 00:00).
- "Meditation complete" is decided once in core: only when a session reaches its planned end by itself (overtime, a Box Breath end, Auto-Finish), never when ended by hand. Same behaviour as today; it moves from the two apps into core.
- Everything else looks and works as before. Inside: core reports only what an app must do (bells, stop signals, stop the track, "reached its end", "ended"); the apps redraw the time, the buttons and the overtime label from core after every call. Pause lives only in the session's clock. Linux reacts to every tick in one place.
- Left out (P5-b): #429, #394, #395, #272, #437, and #430's guided gate, idle hero label and headphone button: they are Android's own state machine and setup. (P5-c): #269 bell effect, #279, #280, #293. #274 waits for a Librem 5 jank, #309 goes with #303.

## Decisions
- Split P5 into a, b, c: Janek, 2026-11-11 ("go").
- Two end effects instead of #270's one with three kinds: "reached its planned end" (drives the notice and nothing else) and "ended" (with whether to save at once, for Auto-Finish). Overtime reaches the end without ending, so one effect cannot carry both (my call).
- Linux refreshes the end buttons on every tick instead of on the prep-to-running effect; it already does in overtime (my call).
- Once a session has ended, both apps show its saved length (45:00 after a 45 minute session): Janek, 2026-11-11. Android showed 00:00, Linux the last tick's value.
- `toggle_action` takes the session (`&self`); no session means Start at the call sites (#275, my call).

## Capabilities
### Modified Capabilities
- `timer`: the three end bugs and the "Meditation complete" rule as requirements.

## Impact
- Core: `session/mod.rs`, `session/effect.rs`, `timer.rs` (Stopwatch `Copy`).
- GTK: `timer/imp.rs` (one tick, one reaction), `mpris.rs` (toggle_action), tests in `timer/mod.rs`, `application.rs`, `sound.rs` that grep the effect names.
- Android: `app.rs` (ended length and notice from core; tests), `ui.rs` (tick, guided end, end taps read core; overtime label from core).
- Translations: none.
- Data: none (no schema, no sync, the in-progress snapshot keeps its form).
- Android build environment: untouched.
- Docs: ARCHITECTURE.md (session engine, effects list), REFACTORING.md (P5 row; #270, #271, #273, #275, #313 done; #430 split), OPEN-ISSUES.md (#277, #278, the HH plausible line).
