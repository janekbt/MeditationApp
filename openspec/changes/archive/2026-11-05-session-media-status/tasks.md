# Tasks: session-media-status

Base: 66b188dd6c5d0f3cd18942da92a4f14a30608d9f; suites on it: green (core 1457, android 287, GTK 168; run 2026-11-05).

## Loop
- [x] 0 Skills loaded, again after every /compact: `/ponytail ultra`; `slint:slint` (Android UI touched)
- [x] 1 Understand
- [x] 2 Propose (approved: 2026-11-05)
- [x] 3 Build (MANUAL checks of 2.2 and 3.2 wait for step 6)
- [x] 4 Review, then `/simplify` after the last review (review R1-R7, fix review F1-F6, /simplify applied)
- [x] 5 Specs, docs, list (check-specs ok; README features; ARCHITECTURE, DECISIONS, core README unaffected; no OPEN-ISSUES entry; the delta merges at ship)
- [x] 6 Verify and hand over (maintainer tested on the FP5 and the laptop 2026-11-05: pass; the 11 minute waits left out on purpose)
- [x] 7 Ship (delta merged, archived)

## Review Focus
- The line changes every second: Android resends metadata each second (art bitmap cost, binder size) -> art drawn once per accent and reused; manual check that the control doesn't flicker
- Guided ends early (EOS before the probed length) -> overtime line, full bar ("Overtime")
- Box Breath's target is rounded up to whole cycles before the start (android ui.rs:4407, gtk imp.rs:2183), so the total can read 20:16; the scenarios use a 4-4-4-4 pattern that divides 20 minutes
- A session restored after a crash (snapshot) -> it starts as a new session, same path, no extra case
- Linux without a session bus, or the name already owned (a second instance) -> registering fails quietly with a log line; the app works as before
- A stopped session on the Done screen -> no status (None), the player is gone, as Android's service
- GNOME keeps a removed player's card for a moment -> manual check in step 6
- A long guided file name or a long translation at 360 px -> the system cuts the title; manual check, German and Russian
- Locked phone: the line updates only while the Rust tick runs (session wake lock) -> paused phone sleeps, line stands, which is right

## 1. Core
- [x] 1.1 `Session::media_status(now) -> Option<MediaStatus>` (None once stopped) { line: MediaLine, bar: Option<MediaBar> }`; `MediaLine { Left(u64), SoFar(u64), Preparing(u64), PausedLeft(u64), PausedSoFar(u64), Overtime(u64) }`; `MediaBar { elapsed_ms, total_secs, moving }`. The end is `shape.target_secs()` (Box Breath countdown included). Overtime secs = elapsed past the target. File: `meditate-core/src/session/mod.rs`. Proof: the six core tests in the delta. Expected: FAIL then PASS

## 2. Android
- [x] 2.1 `AppState::media_status(now)` passes through (None when not active); the title: the guided file's name or "Meditate". Proof: `the_media_control_title_names_the_guided_file`. Expected: FAIL then PASS
- [x] 2.2 Tr functions for the five lines in `ui/main.slint`; `send_media_buttons` adds title, line and bar to the payload (a header line `title\tline\ttotal_secs\tbar`, where `bar` is the moving bar's start on the boot clock in ms, or `p<elapsed_ms>` when it stands), still resent only on change. `MeditateSessionService.showButtons` parses it: metadata title, artist = line, duration (none without a bar), art cached per accent (1024 px kept for sharpness); the playback state only when `bar` changed: PLAYING at speed 1 with position `elapsedRealtime - start`, or PAUSED at the standing position; no ACTION_SEEK_TO; `setContentText(line)` for API 26-32. Files: `ui/main.slint`, `src/ui.rs`, `kotlin/MeditateSessionService.kt`; source checks in `src/app.rs` as needed. Proof: `cargo test -p meditate-android --lib`, Android clippy, then MANUAL
- [x] 2.3 All nine `meditate-android.po`; `msgfmt --check`

## 3. GTK
- [x] 3.1 `src/mpris.rs`: owns `org.mpris.MediaPlayer2.io.github.janekbt.Meditate` on the session bus while a session runs; root (Identity "Meditate", DesktopEntry, CanRaise, Raise) and Player (PlaybackStatus, Metadata with trackid, title, artist = line, length; Position; CanPlay/CanPause/CanControl true; CanGoNext/CanGoPrevious/CanSeek false; PlayPause/Play/Pause through the session's Pause/Resume button when it is offered; overtime reports Paused, CanPlay stays true (GNOME drops a player that can't play; see L1), CanPause false); PropertiesChanged on change. The property values come from a pure function, tested without a bus. Proof: `play_pause_follows_the_session_buttons`, `the_player_reports_the_session`. Expected: FAIL then PASS
- [x] 3.2 `src/timer/imp.rs`: register at session start, update from `tick_prep`, `tick_breathing`, `tick_running`, `tick_overtime`, pause, resume and the overtime switch; unregister when the session stops (Done screen, save, discard). Proof: `cargo test -p meditate`, then MANUAL
- [x] 3.3 Strings via `gettext`, `build-aux/update-translations.sh`, nine `.po`; `cargo test -p meditate i18n::`
- [x] 3.4 `build-aux/io.github.janekbt.Meditate.json`: `--own-name=org.mpris.MediaPlayer2.io.github.janekbt.Meditate`

## Review fixes (first review, all seven findings held)
- R1 Android: a paused media session drops out of SystemUI after 10 minutes; the state stays PLAYING, a standing bar is speed 0 (the proposal's fallback). The device test now covers 11 minutes paused and in overtime.
- R2 GTK: a pause during preparation reported Playing; paused now comes from the session (`is_paused`) or overtime. Test: `the_player_reports_the_session` (paused prep), failed before the fix.
- R3 Android art: kept at 1024 px for sharpness; flicker and SystemUI load are a step 6 check.
- R4 GTK: Position no longer signals (MPRIS: clients follow Rate); an empty change sends nothing. Box Breath's 200 ms tick no longer sends 5 signals a second.
- R5 Android 8-10: the notification's title is the guided file's name too.
- R6 The title test now checks ui.rs passes the session's name and sets it only for Guided (source check: ui.rs can't run on the host). It passed at once: the behaviour existed; removing the Guided filter fails it.
- R7 GTK: mpris errors go to the app's log (`meditate_core::log`), not stderr.

## Fix review (second reviewer, R1-R7 only)
- F1 held: the Metadata map was a HashMap, so equal contents compared unequal and signalled on every tick; now a BTreeMap. Test `only_a_real_change_reaches_the_shell` failed first.
- F2 held: after preparation (and on pause, resume, overtime) the position jumps; `Seeked` now goes out when the bar starts, stops or appears. Same test (expected 302 s and 390 s: running starts at the prep's end).
- F3 held: the moving bar's start may round a millisecond apart between ticks, so the phone payload changed 5 times a second; the service now skips metadata and notification when accent, title, line, length and buttons are the same and the bar didn't move.
- F4 open for the maintainer: a failed MPRIS registration only reaches the log (AGENTS.md: a failure only in the log is a bug). Only happens without the Flatpak `--own-name` permission (FlatPark until its PR); asked at hand-over.
- F5 held: a failed registration is not retried every tick until the next session; both interfaces are looked up before registering.
- F6 rejected: the source check pins spelling, but replacing it means moving the session-start selection code out of ui.rs, beyond this change; it does fail on the real regression (checked).

## /simplify
- Applied: the bar carries its exact elapsed `Duration`, so the phone's bar start no longer wobbles by a millisecond; the Kotlin 50 ms tolerance and the second dedupe (`lastShown`) are gone, `MEDIA_SENT` does it. Paused or overtime is one `standing` flag from core (the two Paused line variants folded into it), used by both apps. The title rule (`media_title`) moved to core. GTK play/pause goes through the existing `toggle_playback` / `Session::toggle_action`, not its own mapping. The countdown rounding is one `ceil_secs` in core (four copies before). GTK refreshes the player from the tick and from pause, resume and the end, no longer twice per overtime tick; a failed registration returns before any work. `mpris.rs` items no one else uses are private; the `seeked` comment matches the code.
- Skipped: a smaller lotus (kept 1024 px for sharpness, a device check); building only the changed D-Bus values per tick (1 to 5 times a second, cheap); `Player.owner` as an Option (needed for Drop).
- After /simplify: core 1463, android 289, GTK 171 green; clippy (GTK+core all targets, core+android -D warnings, android aarch64) clean.

## Notes
- Pre-build questions (fresh agent, 9): answered as my calls in the proposal Decisions (Android bar wave and paused state, boot-clock bar start, API 26-32 text, 512 px art, Linux overtime paused + greyed, removal at stop, four GTK ticks, Box Breath rounding, one-player limit, early guided end).
- 1.1: the six core tests failed on a stub returning None, green after; core 1463.
- 2.1: title and header tests failed on stubs, green after; android 289. Android clippy (aarch64) clean.
- 3.1: both mpris tests failed on stubs (no toggle, no status with a bar), green after. GTK refreshes the player from the one tick dispatcher in `start_tick` (covers prep, breathing, running, overtime), from `refresh_end_buttons` (pause, resume, end) and `on_start`; `reset_mode` drops it. GTK 170.
- 3.3: `src/mpris.rs` added to `po/POTFILES.in`.
- Clippy: GTK+core all targets, core+android -D warnings, android aarch64: clean.

## Device test fixes (first phone test, 2026-11-05)
- D1 Android animates every change of the media line (slides out, the new one in) each second: the phone's line now counts whole minutes (`media_minutes`: left rounded up, so far and overtime rounded down; preparation says "Preparation"). Test `the_phone_line_counts_whole_minutes` failed on a stub. Linux keeps seconds. Android strings: the five second lines replaced by four plural minute forms and "Preparation", nine languages.
- D2 Preparation has its own bar (0 to the prep length); core test `the_media_control_shows_the_preparation` updated first (failed), GTK sends `Seeked` when the bar turns from the prep's into the session's (`only_a_real_change_reaches_the_shell` covers it).
- D3 The lotus art is drawn at 3/4 size, alpha 150, still centred, so title and line read over it (MANUAL).
- Fix review of D1-D3: two held. (1) GTK: a preparation as long as the session (60 s prep, 1 min Timer) sent no `Seeked` at the handover; the bar's kind now includes "preparing"; the equal-length case in `only_a_real_change_reaches_the_shell` failed first. (2) es, it, pt_BR: the singular form of "{n} min left" and "Paused, {n} min left" ("Queda", "Manca", "Falta"). Rejected: "pl {n} min has 2 forms" (it has 3; pre-existing anyway).
- After D1-D3: core 1463, android 290, GTK 171 green; clippy (all three runs) clean.

## /simplify after the device-test fixes
- Applied: `MediaBar.moving` dropped (always `!standing`); the Prep branch no longer reads its target twice; GTK spots a bar jump by the bar's start (or standing position), as Android does, so `Shown.preparing` went; `media_minutes` returns a plain number; Kotlin sets the playback state on every payload (Rust already sends only changes) and lost its `bar` field; the art comment matches the dedupe.
- Skipped: paused line variants back in core (the earlier /simplify folded them into `standing`; flip-flopping costs more than the two match guards); building the payload before the Rust dedupe (microseconds per tick); per-tick variant builds in GTK (negligible).
- After: core 1463, android 290, GTK 171 green; clippy (all three runs) clean.

## Linux test fixes (first laptop test, 2026-11-05)
- Phone tests passed (maintainer, 2026-11-05); the 11 minute pause and overtime waits were left out on purpose.
- L1 The player vanished in overtime: GNOME 48 (`mpris.js`) drops a player whose CanPlay is false. CanPlay is now always true; play/pause is NoOp in overtime and GNOME can't grey it (its button only follows PlaybackStatus), so it shows a play symbol that does nothing: chosen over the player vanishing. Test `the_player_reports_the_session` failed first.
- L2 Cover: `mpris:artUrl` points to the app icon, written to `<cache>/meditate/mpris-cover.svg` at player start (GNOME reads it via `Gio.File`); sent only when the file exists. Same test, failed first (also on a stub `write_cover`).
- L3 Symbolic icon: GTK's symbolic loader adds `rect,circle,path {fill: ... !important}` (seen in libgtk-4), so strokes and masks don't survive; the approved filled draft was traced into six plain filled paths (no stroke, no mask) and checked rendered under that stylesheet at 16, 32 and 64 px. Visible in the shell once a Flatpak with it is installed (the shell reads the installed icon theme).
- Cover and overtime retested on the laptop: pass (maintainer). The space bar in the shell's media card: not wanted (maintainer).
- Fix review of L1-L3: four held: stale overtime wording here (fixed below), artUrl sent for a missing file (fixed), no test for CanPause off in overtime (added), CanPlay in the change list (removed).
- /simplify of L1-L3: the cover is written once per run and kept as its URI (`cover_uri`, a OnceLock), so Metadata no longer checks the file on every tick; the write path is plain; `CanPlay` joined the always-true arm. Skipped: comparing fields instead of built variants per tick (optional, a second rule set to keep in step); the traced icon's tiny steps (render clean at 16 px). After: GTK 171 green, clippy clean.
- F4 (a failed MPRIS registration only in the log): maintainer didn't object to the recommendation; no in-app notice. It only happens without the Flatpak `--own-name` line, which FlatPark gets with the release.

## Hand-over (step 6)
- Suites: see the last block; clippy clean (GTK+core all targets, core+android -D warnings, android aarch64); check-specs ok.
- Linux build: `meditate-gtk/builddir/src/meditate`. Start: `XDG_DATA_HOME=/home/janek/.cache/meditate-statustest LOCALEDIR=meditate-gtk/builddir/po ./meditate-gtk/builddir/src/meditate`.
- APK: `meditate-android/android/app/build/outputs/apk/debug/app-debug.apk`; `adb install -r`.
- Test steps (all passed 2026-11-05 except the 11 minute waits):
  1. Phone, 20 min Timer: lock screen and shade show "Meditate", "Noch 15 Min", changing once a minute, a bar that moves with the app in the background, not draggable; title and line readable over the paler lotus.
  2. Phone: preparation says "Vorbereitung" with its own bar; the session's bar starts after it.
  3. Phone: pause "Pausiert, noch ... Min", bar stands; overtime "Nachspielzeit +... Min", bar full; Save/Add work.
  4. Phone: stopwatch "... Min", no bar; Box Breath: time left and bar, no phases; Guided: the file's name as title.
  5. Laptop (GNOME): the player shows the lotus cover, title and line each second; play/pause in the shade and on the keyboard pauses and resumes; previous/next greyed; in overtime it stays, shows play, and that button does nothing; after Save it is gone.
  6. After installing a Flatpak with the change: the small symbol on the media card is the filled lotus.
- Proposed commit message:

  timer: the media control shows the session's time

  The phone's notification only said the session was running, and
  Linux showed nothing at all. The media control now shows the time
  left (or so far), preparation, pause and overtime, a progress bar,
  and a guided file's name as its title; the phone counts whole
  minutes, since Android animates every change of the line. On Linux
  the session shows as an MPRIS player with the app icon as its cover,
  and play/pause there or on the keyboard pauses and resumes it. The
  symbolic icon is redrawn as filled areas, which GNOME's symbolic
  colouring keeps. Core decides the line and the bar; each app words
  and draws them.
