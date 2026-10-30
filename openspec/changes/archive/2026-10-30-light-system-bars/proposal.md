# Light system bars

## Why
GitHub #15: in the light theme on Android the navigation bar buttons stay
white and are almost invisible on the light page. Janek's FP5 screenshots
(Android 15) show the same for the status bar: clock, battery and icons are
white on the light top bar. The app never tells Android which symbol colour
its bars need.

## What Changes
- In the light theme, Android draws the navigation and status bar symbols
  dark; in the dark theme light, as now.
- On the black session screen the symbols are light in both themes.
- When the system switches between light and dark while Meditate is open,
  the symbols switch with the page at once.
- Left out: the black Discard button in dark mode from the same issue; it
  waits for the screenshot asked for in #15.
- Left out: a light-bar entry in the window theme. The runtime call covers
  start and switch; the theme would only shorten the white-symbol moment
  before the first frame (my call).

## Decisions
- Fix part 1 now, part 2 after the screenshot (Janek).
- `res/` may be touched (Janek); with the runtime call it is not needed.
- The bars switch at once when the theme changes while the app is open (Janek).
- Slint decides from what lies behind the bars (light theme and not the
  black session screen); Rust passes that on at start and on every change,
  Kotlin only applies it (my call, after the review found the session screen).

## Capabilities
### Modified Capabilities
- `appearance`: system bar symbols follow the light or dark theme

## Impact
- Core: none
- GTK: none
- Android: `kotlin/MeditateScreen.kt` (new function), `src/screen.rs`
  (bridge), `ui/main.slint` (one property and a callback when it changes),
  `src/ui.rs` (call at start and on that callback)
- Translations: none
- Data: none
- Android build environment: untouched
- Docs: none
