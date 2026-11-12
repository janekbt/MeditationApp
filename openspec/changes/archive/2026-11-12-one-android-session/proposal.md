# One Android session

## Why
P5-b of REFACTORING's P5 (session engine): #429, #394, #395, #272, #437 and the rest of #430. Android keeps its own state machine beside core's session: `AppState` with a consuming `self -> Transition` API, seven "swap the state out, run, write back, react" blocks in `ui.rs`, its own pause-or-resume rule (which pauses in overtime, #437), three statics for the running session's file, and its own copy of the session shape and the label and guided gates that Linux also writes by hand.

## What Changes
- The phone's ended page shows the ring and the bell count again, at the saved length, as Linux does (lost in P5-a, noted under #429). The page only stays up after its end when a save fails, so this is seen rarely.
- Pause or resume on the phone (the button, the headphones' key, a call taking the audio) follows core's `toggle_action`: in overtime it does nothing (#437). Latent today: overtime offers no Pause, so only the host test shows it.
- On the phone, the saved session records the guided file, title and Local Folder play of the file the session started with, also when the Guided selection changes before Save (#395). Linux keeps reading the selection at Save until #312 (P14). This also fixes OPEN-ISSUES #407 (Save could store one file's uuid with another's name, after an import changed the selection mid-session).
- Everything else looks and works as before. Inside:
  - Android: `AppState` keeps the session after its end (as Linux does) together with what Start fixed (start time, guided file, title, local file); transitions are `&mut self -> Vec<Effect>`. `Transition`, `prepend`, `remaining` and the `Finished(u64)` variant go. One `apply` in `ui.rs` runs a transition and owns its edges (start, pause/resume, end, Auto-Finish, notice, refresh) instead of seven blocks (#429, #394). The three statics, `session_start_unix` and `pending_done` go (#395).
  - Core: `SessionShape::new(mode, stopwatch_on, target_secs, pattern)` builds the shape for both apps, `None` for a 0 s Timer or Box Breath countdown and a 0 s Guided file either way (#272). `labels::session_label(db, mode)` is "the mode's label if its switch is on" for both apps and core's own summary (#395). `SessionMode::guided_file(uuid)` keeps the uuid only for Guided, for both apps (#430).
  - Android's Setup time uses core's `idle_hero_label`; the headphones' key uses `toggle_action` (#430).
- Left out: P5-c (#269, #279, #280, #293); #274 (Librem jank); #312's Linux half (the saved session and snapshot built once at Start, `Session::press`) stays in P14. The label is still read at Save, as on Linux: the running page can't change it, and a label deleted by a sync in between must not be saved.

## Decisions
- Ended page ring on the phone: follows "Android mirrors Linux"; P5-a's note under #429 (my call).
- Idle hero via `idle_hero_label` (Android already has the mode's target) instead of `idle_hero_label_from_modes`, which needs all three modes' values (my call).
- `SessionShape::new` returns `None` for a 0 s Timer or Box Breath countdown and for a 0 s Guided file counting either way; today Linux returns early for Timer and Guided and both apps' Start is disabled at 0 (my call).
- A guided file deleted by a sync between Start and Save: the session still saves its uuid and title, as the recovery snapshot already does; a session row naming a gone file is already possible through sync, and nothing joins on it (my call).
- The facts fixed at Start live inside `AppState` next to the session, so the heartbeat, the media control and Save read them from the one state (my call).

## Capabilities
### Modified Capabilities
- `timer`: the phone's ended page, pause in overtime, the saved file.

## Impact
- Core: `session/settings.rs` (`SessionShape::new`), `labels.rs` (`session_label`), `db/sessions.rs` (`SessionMode::guided_file`), `preset_config.rs` (uses `session_label`).
- GTK: `timer/imp.rs` (`on_start` shape, `setup_selected_label_id`, the two guided gates); source-grep tests that name them.
- Android: `app.rs` (`AppState`), `ui.rs` (the seven blocks, the statics, Start, Save, heartbeat, media control, label sites).
- Translations: none. Data: none (no schema, no sync; the snapshot keeps its form). Android build environment: untouched.
- Docs: ARCHITECTURE.md (Android session state), REFACTORING.md (P5 row; #429, #394, #395, #272, #430 done), OPEN-ISSUES.md (#437).
