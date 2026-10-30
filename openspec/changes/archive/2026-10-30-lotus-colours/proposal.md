# Lotus in the system colours

## Why
The widget's small app image stays blue whatever the wallpaper, while
the rest of the widget follows the system colours. The app symbol has a
themed (one-colour) version for Android 13+, but Android reads only its
shape, so the petals melt into one white blob. The lotus parts its
petals with light-blue lines, which look wrong on any colour but blue.

## What Changes
- The lotus gets real gaps between its petals instead of blue lines,
  one drawing for the app symbol, the themed symbol, the widget, the
  media control picture and the small status-bar symbol.
- On Android 12+ the widget's app image sits on a gradient of the
  system accent colour; below 12 it stays blue.
- The media control picture draws the new lotus on the accent; the
  pixel pass that turned the blue lines into gaps goes away.
- The media control's buttons use the app's own symbols: pause, play,
  a check for Save, the plus for Add, an X for Discard, instead of
  Android's symbols with the floppy disk and the bin (added after the
  device test).
- Left out: an app symbol that follows the in-app accent (Blue, Rose…).
  Android only allows switching between built-in symbols, which closes
  the app and can drop home-screen shortcuts.
- Left out: the laptop's icon. It keeps its blue lines; Linux has no
  colour-changing app icons and nothing there shows the lotus on colour.

## Decisions
- Widget image background: a strong gradient of the system colour, mid
  tone at the top to darker at the bottom, the same in light and dark
  (Janek).
- The media control uses the same new lotus (Janek).
- Check and X for Save and Discard, and the app's own pause, play and
  plus for the rest (Janek, after the device test).
- Gradient tones system_accent1_400 to system_accent1_600, matching
  the brightness of today's blue (my call).
- Gap width 1.5 units of the icon's 128-unit drawing, about the width
  of today's lines (my call; adjusted after the device look).
- The gapped petal outlines are computed once with a script and stored
  as plain path data; the script is not kept in the repo (my call: the
  drawing is the source from now on, the old one stays in git history).

## Capabilities
### Modified Capabilities
- `appearance`: the widget's app image follows the system colours; the
  lotus shows its petals apart on every colour.

## Impact
- Core: none
- GTK: none
- Android: `res/drawable/ic_launcher_foreground.xml` (new petal paths),
  `res/drawable/widget_app_icon.xml`, `res/values/colors.xml`,
  `res/values-v31/colors.xml`, `kotlin/MeditateSessionService.kt`
  (lotusArt), tests in `src/app.rs`
- Translations: none
- Data: none
- Android build environment: untouched (res files are app resources;
  Janek allowed touching them on 2026-10-30)
- Docs: none expected; grep README and docs for the icon description
