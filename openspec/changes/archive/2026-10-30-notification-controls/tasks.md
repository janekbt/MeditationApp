# Tasks: notification-controls

Base: 91f5d0ac9ff6a08e0b0d8a47437a68ee0f2d29b2; suites on it: green (core 1438, android 275; gtk not run, untouched)

## Loop
- [x] 0 Skills loaded, again after every /compact: `/ponytail ultra`; `slint:slint`
- [x] 1 Understand
- [x] 2 Propose (approved: 2026-10-30)
- [x] 3 Build (tests 1-4 failed first, then green; android 279)
- [x] 4 Review, then `/simplify` after the last review
- [x] 5 Specs, docs, list (README; no OPEN-ISSUES entry)
- [x] 6 Verify and hand over
- [x] 7 Ship (device test passed 2026-10-30: "tests alle gruen")

## Review Focus
- Android 13+ (targetSdk 34) builds media-control buttons from the media
  session's playback state, not from notification actions: all buttons,
  Pause/Resume too, are custom actions with system icons; the state stays
  PLAYING (a PAUSED player is dropped after about 10 minutes), so the
  play/pause symbol is the Pause or Resume button's icon.
- A tap while the app is in the background with the screen off and the
  phone paused (no wake lock): the tap wakes the process; the drop-file
  is taken on the next tick. Device test.
- A tap that arrives after the session already ended (stale button):
  the command is taken against an idle state and does nothing; covered
  in `a_notification_tap_goes_through_the_session_buttons`.
- Two taps before one tick: the drop-file keeps the last; acceptable,
  the second tap sees the new buttons only after the tick anyway.
- The note changes while paused: the Discard behaviour is resent when
  the note turns empty or non-empty.
- Another language: labels are existing translated strings.
- A guided track's own audio holds no media session, so the headphone
  button reaches ours. Device test.

## Pre-build answers (my calls)
- Android 8 to 12 (minSdk 26) build buttons from notification actions,
  13+ from the playback state: both from one button list.
- Every tap takes a short timed wake lock (10 s) so a paused phone with
  the screen off still reaches the next tick; Resume then takes the
  session lock as usual.
- The tap drop-file is deleted when a session starts, and a code the
  current row does not offer is ignored: an old Discard can never hit a
  new session.
- Buttons work on the lock screen without unlocking (the GitHub request asks for it).
- Artwork is redrawn only when the accent changes.

## Tasks
- [x] 1 `the_notification_offers_the_session_buttons`: `app.rs` maps
  `end_buttons()` plus Discard to the media control's buttons.
- [x] 2 `a_notification_tap_goes_through_the_session_buttons`: the
  service writes the tapped button's code to a drop-file; the tick takes
  it and calls the same handler as the app's button
  (`invoke_end_button_tap` / `invoke_discard_tap`); a code that the
  current row does not offer is ignored.
- [x] 3 `discard_from_the_notification_spares_a_note`: with a note the
  button list has no Discard; a Discard code arriving anyway (sent
  before the note) is ignored.
- [x] 4 `the_headphone_button_toggles_the_pause`: play/pause maps to
  the row's Pause or Resume, none in overtime.
- [ ] 5 Colour and lotus: artwork bitmap of the accent (light primary)
  with `ic_launcher_foreground` drawn on it; playback state PLAYING (not
  PAUSED). Device test 1: small lotus dropped, big one blurry (256 px)
  with blue petal lines -> 1024 px, strokes turned into gaps. Device
  test 2: the play triangle beside Resume reads as two play buttons ->
  small icon back to the lotus; Save keeps ic_menu_save.
- [x] 6 README features line (living spec merge at ship).

## Review 1
- Fixed: the note field lets go before a notification tap (2); the tap
  wake lock is kept in a field (4); public visibility on the lock screen
  (5); labels by button, not raw code (6); a stopped service forgets its
  buttons (8).
- Rejected (1), state always PLAYING: deliberate, see Review Focus; the
  Pause/Resume button carries the play or pause symbol. Device test
  checks a long pause keeps the control.
- Rejected (3), a lock-screen Save that fails goes unseen: the failed
  save keeps the running page with Save and Discard and the snapshot, so
  the user finds it on return; the same holds for Auto-Finish in the
  background today. Shown to Janek: no extra notification.
- Rejected (7), tests per mode: the row is core's `end_buttons`, tested
  per mode in core; the wiring check is what runs on the host.
- Fix review: a tap reaching a stopped service stops it instead of
  leaving a ghost media control (fixed).

## /simplify
- Applied: one `Tr.end-button` label function for the page and the
  media control (the separate pause/resume/save/discard ones are gone);
  named button codes in Kotlin and Rust's headphone toggle; one
  stopped-service guard for SHOW_BUTTONS and TAP; taps read only during
  a session; the lotus art at 256 px.
- Skipped: building the payload only on state changes (microseconds a
  tick, and Add's label changes every second anyway); one shared button
  list for page and notification and a typed tap enum (larger than the
  change; the codes already cross JNI as numbers); dropping the
  source-text wiring checks (the only host proof of the tick wiring).

## Hand-over
APK: meditate-android/android/app/build/outputs/apk/debug/app-debug.apk
(19:36, installed on the FP5). android 279 green, clippy (CI and Android
target) clean, check-specs green; core and GTK untouched.

Test steps (FP5):
1. Start a Timer session, lock the phone: the media control shows the
   lotus on the accent colour, the lotus in the status bar, and Pause +
   Discard.
2. Pause from the lock screen: Resume, Save, Discard; the app is paused.
   Resume from there.
3. Pause, wait 10+ minutes with the screen off: the control is still
   there and Resume works at once.
4. Headphones: play/pause pauses and resumes; in overtime nothing.
5. Write a note while paused: Discard leaves the control. Save from the
   lock screen saves with the note.
6. Without a note, Discard from the lock screen: the session is gone.
7. Overtime: Save, Add, Discard; Add saves with the overtime.
8. Guided session: Pause from the control pauses the track.
9. Accent: switch to Rose and start again: the background follows.

Proposed commit message:
android: Control the session from its notification

The session's media control shows the running page's buttons and
Discard, follows the headphones' play/pause button, and takes the
app's accent with the lotus.
