# Dark colours on the session screen

## Why
GitHub #15, part 2: the session screen is black in both themes, but its
buttons take the light theme's colours there. In light mode "Discard" is dark
olive text on black and hard to read (Janek's FP5 screenshot); "Pause" is a
dark fill on black.

## What Changes
- The black session screen always uses the dark scheme's colours: Pause,
  Resume, Save and Add get the dark primary fill with its text colour,
  Discard the dark primary text and dark outline; the Box Breath square,
  its ring and dot take the dark primary too. Dark mode looks as before.
- Left out: the note field on the paused or finished screen. It is its own
  filled box with its own text colour and is readable in both themes (my call).

## Decisions
- The whole session screen takes the dark scheme, not only the Discard text (Janek).
- WrapButton gets three colour inputs that default to today's roles; only
  EndButtonView passes the dark ones, so the other WrapButton uses stay (my call).

## Capabilities
### Modified Capabilities
- `appearance`: the session screen uses the dark colour roles in both themes

## Impact
- Core: none
- GTK: none. The GTK running page uses libadwaita's own colours over its fade to black.
- Android: `ui/main.slint` (WrapButton, EndButtonView, Box Breath square), a guard test in `src/app.rs`
- Translations: none
- Data: none
- Android build environment: untouched
- Docs: none
