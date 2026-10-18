# Android platform (batch L)

## Why
OPEN-ISSUES batch L: #50, #51, #57, #79, #149, #176 (#63 is already
fixed by batch H and only leaves the list). On the phone, any installed
app can start a meditation through the widget; a phone-to-phone
transfer copies the device id, so two phones sync under one id; a
guided track plays through a call, moves to the loudspeaker when
earbuds disconnect, and a preview of a file that isn't there does
nothing; Simplified Chinese is never picked; vibration bells are
dropped under Battery Saver on Android 8 to 12; a font-size or language
change mid-session ends the session but leaves its notification, wake
lock and track running.

## What Changes
- #50: only the widget's own tap starts a preset; the launch command
  moves to a new receiver other apps can't reach.
- #51: a phone-to-phone transfer takes no app data along.
- #57: a refused audio focus means the track does not start ("Couldn't
  start playback", as for any failed start); Play in Manage Files on a
  file that won't play shows the same notice; disconnecting earbuds
  pauses the session and its track like a focus loss.
- #79: a system language with a script ("zh-Hans-CN") picks the
  regional bundle ("zh_CN").
- #149: below Android 13 a vibration is sent with the alarm usage, so
  Battery Saver lets it through on Android 9 to 12.
- #176: font size (applied at once), bold text, split screen and
  keyboard changes no longer restart the screen; a language change
  still does, so the app switches language at once; at startup, a leftover service, guided
  track or bell from an earlier screen is stopped.
- Left out: Traditional Chinese keeps falling back to English (no
  bundle); Android 8.x, whose Battery Saver lets only ringtone
  vibrations through (from memory, not checked against AOSP); a Manage
  Files preview when earbuds disconnect keeps playing on the
  loudspeaker (it isn't a session, and the pause path is the
  session's).

## Decisions
- A language change keeps recreating the screen (and ends a running
  session as a recovered one, now without leftovers), so the app shows
  the new language at once (Janek, after review 1: "Sprache wie
  bisher").
- Manifest and a new `res/xml` file may change; the repro probe runs
  after (Janek, 2026-10-18: "passt, go").
- The new receiver is a separate small Kotlin class, not a check inside
  the exported widget provider: a broadcast carries no trustworthy
  sender (my call).
- Startup stops leftovers unconditionally: a session from an earlier
  screen only comes back as a crash-recovery Undo, which never restarts
  its service or track, so nothing that still plays belongs to a live
  session (my call).

## Capabilities
### New Capabilities
- `sync`: a transferred phone keeps its own device id.
- `translations`: the system language picks its bundle.
### Modified Capabilities
- `presets`: only the widget starts a preset from outside the app.
- `guided`: track start, earbuds and preview failures.
- `vibration`: vibration bells under Battery Saver.
- `timer`: a session survives configuration changes.

## Impact
- Core: none
- GTK: none (no widget, no transfer, no audio focus there)
- Android: `kotlin/MeditateWidgetProvider.kt`, new
  `kotlin/MeditateWidgetLaunchReceiver.kt`, `kotlin/MeditateGuided.kt`,
  `kotlin/MeditateHaptics.kt`, `src/app.rs`, `src/ui.rs`
- Translations: none (the notice exists)
- Data: none
- Android build environment: `AndroidManifest.xml` (new receiver,
  `dataExtractionRules`, `configChanges`) and new
  `res/xml/data_extraction_rules.xml`; repro probe after the build
- Docs: OPEN-ISSUES.md
