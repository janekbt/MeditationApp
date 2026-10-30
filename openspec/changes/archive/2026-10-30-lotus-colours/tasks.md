# Tasks: lotus in the system colours

Base: 571ae0c; suites on it: android 279 green, clippy clean (verified at ship of 571ae0c, same commit, 2026-10-30)

## Loop
- [x] 0 Skills loaded, again after every /compact: `/ponytail ultra`; `slint:slint`
- [x] 1 Understand
- [x] 2 Propose (approved: 2026-10-30)
- [x] 3 Build (281 green)
- [x] 4 Review, then `/simplify` after the last review
- [x] 5 Specs, docs, list (README line; check-specs green)
- [x] 6 Verify and hand over (Janek: "passt alles", then "works", 2026-10-30)
- [x] 7 Ship (specs merged, archived)

## Review Focus
- Dark mode on 12+ -> values-night-v31 has no icon colours, so the v31 ones apply in both modes (Janek: same in light and dark); covered by the widget 12+ scenario.
- Android 8 to 11 -> "The widget below Android 12".
- Android 12 (no themed icons, which start at 13) -> plain symbol only, nothing to check.
- Status-bar small icon (alpha only) -> gaps help it too; checked in the media-control scenario on the FP5.
- Launcher icon cache after a wallpaper change -> the themed symbol is the system's job; not ours to test.
- GTK icon -> unchanged, see proposal.

## 1. Gapped lotus
- [x] 1.1 Test: no petal path in ic_launcher_foreground.xml has a strokeColor (the base arc may). Files: `src/app.rs`. Proof: `the_lotus_parts_its_petals_with_gaps`, Expected: FAIL then PASS
- [x] 1.2 Compute the visible part of each petal (back petals minus the front ones grown by 1.5 units) with a scratch script (shapely in a venv under ~/.cache), write them as fill-only paths in the same source space and group transform. Files: `res/drawable/ic_launcher_foreground.xml`. Proof: 1.1 passes; render the drawable to PNG on the host and look at it
- [x] 1.3 lotusArt: fill the accent, draw the drawable, drop the pixel pass. Files: `kotlin/MeditateSessionService.kt`. Proof: device scenario (media control on Rose)

## 2. Widget app image
- [x] 2.1 Test: widget_app_icon.xml gradient uses @color/widget_icon_top and @color/widget_icon_bottom; values/colors.xml has #5BA4E8 / #1A60BB; values-v31 maps them to system_accent1_400 / system_accent1_600. Files: `src/app.rs`. Proof: `the_widget_app_image_takes_the_system_accent`, Expected: FAIL then PASS
- [x] 2.2 Add the two colours and point the gradient at them. Files: `res/drawable/widget_app_icon.xml`, `res/values/colors.xml`, `res/values-v31/colors.xml`. Proof: 2.1 passes; device scenario

## 3. The app's own symbols in the media control (check and X added 2026-10-30 after the device test; pause, play and plus after Janek saw them)
- [x] 3.1 Test (first version): Save and Discard map to ic_check / ic_close with the material-1.0 path data. Proof: was `save_and_discard_show_a_check_and_an_x`, FAIL then PASS
- [x] 3.2 Add res/drawable/ic_check.xml and ic_close.xml and use them in buttonIcon
- [x] 3.3 Test widened to every code: one table code -> drawable -> source SVG (Pause ui/assets/pause.svg, Resume play.svg, Save check, Add ui/assets/add.svg, Discard close); no android.R in buttonIcon. Files: `src/app.rs`. Proof: `the_media_buttons_show_the_apps_own_symbols`, Expected: FAIL then PASS
- [x] 3.4 Add ic_pause.xml, ic_play.xml, ic_add.xml (add.svg uses viewBox 0 -960 960 960: viewport 960 with a group translateY 960) and use them. Files: `res/drawable/`, `kotlin/MeditateSessionService.kt`. Proof: 3.3 passes; device scenario

## Notes
- The themed-symbol layer already exists (ic_launcher.xml monochrome = ic_launcher_foreground); it only needs the gaps, so no new file.
- Tests read resource source: resources cannot run on the host.
- Pre-build answers (my call): petals cut by draw order (outer by inner+centre, inner by centre); if the two inner petals overlap outside the centre and the cut makes the flower lopsided, leave that pair uncut. Curves flattened densely (within ~0.05 units), 2 decimals, evenOdd only if a piece has holes. Fill-only petals lose the stroke's outer half-unit; scale 0.62 stays. Test 1.1: no `<path>` with android:fillColor also has android:strokeColor (the file comment names #64A0E6, so no string search). Rewrite the stale comments in ic_launcher_foreground.xml, widget_app_icon.xml and lotusArt.
- Tooling: shapely + cairosvg via pip into a venv under ~/.cache (scratch tool, not a project dependency); needs Janek's go with the proposal.
- Review 1 (fixed): status bar added to the media-control scenario (MANUAL; reviewer measured the gap at ~0.5 px there, so the FP5 decides); requirement starts "On the phone,"; test also counts exactly one stroke (the base arc); stale "blue margin" comment.
- /simplify: reuse, efficiency, altitude found nothing. Applied: the lotus test is two counts (5 fills, 1 stroke). Skipped: test checks names only instead of literal colours (the spec asks for blue below 12 and the accent tones); shorter lotus header comment (the GTK note explains why the two icons differ).

## Hand-over
- Verified: android 282 green, clippy clean, check-specs green; debug APK installed on the FP5 (see below).
- Test steps (FP5): 1. Widget: app image on a gradient of the system colour; change wallpaper colours / dark mode, it follows. 2. Wallpaper & style: switch "Themed icons" on; Meditate's symbol shows the five petals apart. 3. Plain symbol (themed off): looks as before, blue between the petals. 4. Running: the app's pause symbol; paused: the app's play, a check, an X; overtime: a check, a plus, an X. 5. Session with accent Rose: media control rose between the petals, sharp; small status-bar lotus shows petals apart (reviewer expects the gaps may be too thin there).
- Commit message:
  android: Lotus with real gaps, widget image in the system colour

  The lotus parted its petals with light-blue lines, which looked wrong
  on any colour but blue and vanished in the themed icon, where Android
  reads only the shape. The petals now have gaps that show what is
  behind them, in one drawing for the app symbol, the themed symbol,
  the widget, the media control and the status bar, so the media
  control no longer needs its pixel pass. On Android 12 and later the
  widget's app image sits on a gradient of the system accent, and the
  media control's buttons use the app's own symbols: pause, play, a
  check for Save, the plus for Add and an X for Discard, instead of
  Android's, with its floppy disk and bin.
- Review of group 3 (fixed): Discard has its own branch in buttonIcon, the test pins that line (an added bin above `else` slipped through before).
- /simplify 2 (check/X): reuse, simplification, efficiency found nothing. Skipped (altitude): app's own SVGs for Pause, Play and Add too, with one table test over every code: changes icons Janek did not ask for; offered to him as an option.
- Group 3 widened after Janek saw check and X: pause, play and plus from the app's own SVGs too; one table test over every code (renamed to the_media_buttons_show_the_apps_own_symbols). Review fix: the test also checks the single path, the viewport and the y shift (mutation: removing ic_add's translateY fails it). Stale "system play triangle" comment above buildNotification removed.
- /simplify 3: nothing applied. Skipped: an x-offset check (no source has one). Not touched: the "session complete" notification still uses Android's play triangle as its small icon (outside this change; offered to Janek).
