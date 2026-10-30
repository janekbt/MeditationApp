# Notification controls

## Why
A user on GitHub asked to control a session without opening the app or
lighting the screen. On the phone, the running session's media control
in the notification shade and on the lock screen shows only a play
symbol and "Session running": no buttons, and a dark blue background
that matches neither the app nor the system.

## What Changes
- The media control shows the running page's buttons plus Discard:
  Pause and Discard while running or preparing; Resume, Save and
  Discard while paused; Resume and Discard while paused in
  preparation; Save, Add and Discard in overtime.
- A tap does what the same button in the app does, including the
  guided track, the wake lock and the note.
- Discard discards at once. While a note is written, Discard is left
  out of the media control; a tap on the notification opens the app,
  where Discard asks as usual.
- The headphones' play/pause button pauses and resumes the session;
  in overtime it does nothing (no Pause there in the app either).
- The media control's background takes the app's accent colour and
  shows the white lotus of the app icon, if the buttons leave room
  for it (decided at the device test).
- Left out: shake or flip gestures (accidental triggers);
  the Linux app (no such notification).

## Decisions
- Discard: no note, at once; with a note, the button is left out,
  because Android 13+ lets no media button open the app (Janek).
- Colour: the app's accent, so System, Blue, Rose etc. (Janek).
- Lotus on the coloured background, sharp, its petals parted by gaps
  the accent shows through; the small symbol is the lotus too: the play
  triangle beside a paused session's Resume read as two play buttons
  (Janek, device test).
- Save keeps the system's save symbol (Janek).
- Buttons are symbols only: Android 13+ media controls show no text;
  the labels are read by screen readers (Janek asked).
- The accent tone is the light scheme's primary, dark enough for the
  white lotus (my call).
- Labels (read by screen readers; the buttons show system icons)
  reuse the app's translated "Pause", "Resume", "Save", "Discard" and
  "+ {} and Save"; Rust hands them to Kotlin and resends only when one
  changes, so Add's label follows the overtime each second (my call).
- Headphone button in overtime: ignored (my call).

## Capabilities
### Modified Capabilities
- `timer`: the phone's session notification carries the session buttons.

## Impact
- Core: none (the row of buttons is already `Session::end_buttons`).
- GTK: none.
- Android: `kotlin/MeditateSessionService.kt` (session actions,
  playback state, artwork), `src/service.rs` (one update call),
  `src/ui.rs` (send the buttons on change, take taps on the tick),
  `src/app.rs` (which buttons, what a tap or headphone press does).
- Translations: none new; existing strings reused.
- Data: none.
- Android build environment: untouched (system icons and the existing
  `ic_launcher_foreground` drawable; no manifest, Gradle or resource
  file changes).
- Docs: README features list (notification controls), `openspec/specs/timer`.
