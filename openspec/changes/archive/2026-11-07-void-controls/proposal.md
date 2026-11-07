# Void-Controls

## Why
GitHub issue #17 (voidplayer): run a session without opening the app or
lighting the screen, as alarm apps do: turn the phone over to pause,
shake it to finish. On Android this becomes an opt-in experimental
setting, named after the person who suggested it.

## What Changes
- Android Preferences get an "Experimental" group with the switch
  "Void-Controls" (off until set) and an explanation under the heading.
- With the switch on, during a session: turning the phone face down
  pauses, turning it face up again resumes, shaking it saves the
  session. Only the turn counts, not how the phone lies when the
  session starts. Each gesture the session acts on vibrates briefly.
- Shaking in overtime saves with the overtime (as Add) once the
  overtime is longer than one minute; before that it saves the planned
  length (as Save).
- With the switch on, a paused session keeps the phone's CPU awake
  (not the screen), so turning it face up still resumes. Today a pause
  lets the phone sleep; that stays so with the switch off.
- Left out: Linux (a laptop has no orientation or shake sensor); a
  sensitivity setting (thresholds are tuned on the FP5 instead);
  gestures outside a session (nothing to pause or save there).

## Decisions
- Face down pauses, face up resumes; only the change counts (maintainer).
- Shake in overtime: with the overtime after one minute of overtime,
  without it before (maintainer).
- A short vibration confirms every gesture the session acts on; a
  gesture the session can't act on now (shake in preparation, face down
  in overtime) does nothing and does not vibrate (maintainer: vibrate).
- Text (maintainer): "Turn the phone face down to pause, face up to
  continue. Shaking it finishes and saves the session. Each recognised
  gesture vibrates briefly. Only for meditation sitting or lying down:
  movement, as in walking meditation, sets it off."
- The group sits right before About, the last group (my call).
- "Void-Controls" is a name and stays untranslated in every language
  (my call).
- The setting syncs like every other preference, as "Play Bells as
  Alarm" (Android-only too) does (my call).
- A gesture acts like the matching button of the media control: it
  goes through the same check of what the session offers right now
  (my call).
- Turning the switch on or off during a session takes effect at once
  (my call).

## Capabilities
### Modified Capabilities
- `preferences`: the Experimental group on Android.
- `timer`: gestures control a session on Android; a paused session
  keeps the CPU awake while Void-Controls is on.

## Impact
- Core: `settings_keys.rs` (the key, off until set).
- GTK: none (written reason above: no sensors).
- Android: `ui/main.slint` (group), `src/ui.rs` (load, save, tell the
  service, act on a gesture, vibrate), `src/app.rs` (gesture codes and
  which button each one stands for), `src/service.rs` (switch to the
  service), `kotlin/MeditateSessionService.kt` (accelerometer listener,
  gesture detection, wake lock in a pause).
- Translations: Android "Experimental" and the explanation, nine
  languages; "Void-Controls" untranslated. Linux none.
- Data: one new synced setting key; no schema or event change.
- Android build environment: untouched (the accelerometer needs no
  permission, no manifest change).
- Docs: README features list (Android).
