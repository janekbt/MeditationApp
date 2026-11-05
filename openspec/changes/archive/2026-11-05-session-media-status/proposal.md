# Session time in the media control

## Why
The phone's session notification says only "Meditation running". You
have to open the app to see how long is left. Linux has no media
control during a session at all, so on GNOME and on the Librem 5
(Phosh) the shade and the lock screen show nothing.

## What Changes
- Android: the media control shows a second line with the time and,
  for a session with an end, Android's own progress bar with elapsed
  and total time under it. The system keeps the bar moving. For Guided
  the first line is the file's name instead of "Meditate".
- Linux: during a session the app shows up as a media player (MPRIS)
  in the GNOME shade and on the Phosh lock screen, with the same title
  and second line. Play/pause pauses and resumes the session; that is
  also how the headphones' and keyboard's play/pause keys reach it.
  It goes away when the session ends.
- Core decides what the line says and how far the bar is; each app
  translates and draws it.

## Decisions
- Box Breath: time and bar like the Timer, no breath phases
  (maintainer).
- Second line: "{time} left" plus the bar; the line is set every
  second (maintainer).
- Counting up: with an end, always the time left (the bar already
  shows elapsed); without an end, the time so far, no bar
  (maintainer).
- Guided: the file's name is the title (maintainer).
- States (maintainer):

  | State | Second line | Bar |
  |---|---|---|
  | Running, has an end | `{t} left` | moves |
  | Running, no end | `{t}` (time so far) | none |
  | Preparation | `Preparation, {t} left` | none yet |
  | Paused, has an end | `Paused, {t} left` | stands |
  | Paused, no end | `Paused, {t}` | none |
  | Overtime | `Overtime +{t}` | full |

- A pause during preparation shows the preparation line, standing
  (my call).
- After the first phone test (maintainer, 2026-11-05):
  - Android animates every change of the line (out to the left, the
    new one in), so the phone's line counts whole minutes: "15 min
    left" (rounded up, as the countdown), "12 min" and "Overtime +2
    min" (rounded down), "Paused, ..." the same; preparation says only
    "Preparation". Linux keeps seconds (no animation known there,
    checked in the Linux test) (maintainer, Linux part my call).
  - Preparation has its own bar, from 0 to the preparation's length;
    the session's bar starts after it (maintainer).
  - The lotus art is a little smaller and paler, still in the middle,
    so title and line read over it (maintainer).
- Linux: the full MPRIS (title, line, length and position, so players
  that draw a bar, such as KDE's, show one). Only play/pause; previous
  and next are greyed out; Save and Discard stay in the app
  (maintainer). In overtime play/pause does nothing, as on Android.
- Linux: the player is registered when a session starts and removed
  when it stops (the Done screen, a save, a discard), as on Android
  (my call). Raise brings the window to the front (my call).
- Linux in overtime: reported as paused, so players that move a bar
  themselves don't run past the end. It still says it can play,
  because GNOME drops a player that can't (seen in the first Linux
  test); its play/pause does nothing there, and GNOME has no way to
  grey that button out (my call).
- Linux shows the app icon as the player's cover, written to the
  app's cache at session start (maintainer, after the first Linux
  test).
- The symbolic app icon (GNOME's small icon on the media card and on
  notifications) is redrawn as filled areas: GTK fills every path of a
  symbolic icon, which turned the old outline lotus into thick rims.
  The new one is the app icon's petals with thin gaps, as plain filled
  paths (maintainer approved the preview).
- Linux: GNOME's media keys and the Phosh lock screen follow one
  player at a time; with a music app also open, Meditate may not be
  the one shown (known limit, my call).
- A guided track that ends early shows "Overtime +00:00" until its
  planned length is passed, like the app's own Add button (my call).
- Android 13+ draws its own bar: a wave that moves while playing. It
  shows elapsed and total only while the bar is dragged, and dragging
  is off, so the time is read from the line (the bar's numbers in the
  mock-up don't show). The state stays "playing" throughout; a standing
  bar (paused, overtime) is speed 0, because SystemUI drops a paused
  session's control after 10 minutes and a long pause or overtime
  still needs its buttons (my call, after the review; the wave may
  keep moving on a standing bar, checked on the FP5).
- Android: the bar is sent as its start on the boot clock (the same
  clock as Android's `elapsedRealtime`), so it only changes on pause,
  resume or overtime and never jumps back; only the line changes every
  second (my call).
- Android 8 to 12 (API 26-32) show the notification's own text, not
  the media metadata: the line goes there too, without a bar (my call).
- Android: the lotus art is drawn once per accent and reused, since
  the metadata is now resent every second; it stays 1024 px so the
  lotus stays sharp (my call).
- Left out: breath phases, a Linux notification other than MPRIS,
  seeking.

## Capabilities
### Modified Capabilities
- `timer`: the session's media control shows time and progress, on
  the phone and now on Linux.

## Impact
- Core: `session/mod.rs`, a `media_status(now)` on `Session`: a typed
  line key and the bar (elapsed, total, moving), from the phase and
  shape.
- Android: `src/app.rs` (pass the status through `AppState`),
  `src/ui.rs` (translate the line, send title, line and bar with the
  buttons, resend only on change), `ui/main.slint` (Tr functions),
  `kotlin/MeditateSessionService.kt` (metadata title and artist,
  duration, playback position and speed; art cached).
- GTK: a new `src/mpris.rs` with `gio::DBusConnection`
  (`org.mpris.MediaPlayer2` and `.Player`, no new crate),
  `src/timer/imp.rs` (register at session start, update from
  all four ticks (prep, breathing, running, overtime), pause and
  resume, unregister when the session stops; PlayPause
  goes through the same path as the Pause/Resume button).
- Flatpak: `build-aux/io.github.janekbt.Meditate.json` gets
  `--own-name=org.mpris.MediaPlayer2.io.github.janekbt.Meditate`. The
  FlatPark package needs the same line at the next release (a pull
  request there, after the maintainer's go).
- Translations, all nine languages: Linux "{time} left", "Preparation,
  {time} left", "Paused, {time} left", "Paused, {time}", "Overtime
  +{time}"; Android the minute forms "{n} min left", "Paused, {n} min
  left", "Paused, {n} min", "Overtime +{n} min", "Preparation" (the
  plain "{n} min" exists).
- Docs: `openspec/specs/timer/spec.md` (delta), README features
  (media control on Linux).
- Icon: `meditate-gtk/data/icons/hicolor/symbolic/apps/io.github.janekbt.Meditate-symbolic.svg`.
- Data, schema, sync: untouched. Android build environment: untouched.
