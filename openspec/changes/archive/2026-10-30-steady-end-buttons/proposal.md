# Steady end buttons

## Why
On the session screen Discard jumps from the top right (running) to its
own line below (paused), so a destructive button seems to appear while
the one that was destructive turns into Save. Pausing also adds Discard
and the note to the centred column, so the time and every button jump
up. In overtime "Add 03:00 and Save" is so long that it wraps in its
half of the row.

## What Changes
- Discard has its own line under the row in every state; the row holds only the main actions: [Pause]; paused [Resume][Save]; paused in preparation [Resume]; overtime [Save][+ 03:00 and Save]; ended [Save].
- The time and the buttons stay where they are when the note appears; the note comes in below them (the page scrolls when the screen is too short).
- The Add button reads "+ 03:00 and Save" ("+ 03:00 und speichern").
- Left out: a corner or header Discard, full-width stacking (not chosen); any change to what each button does.

## Decisions
- Layout: Discard fixed under the row from the start (Janek, option 1 of 3).
- Wording: "+ 03:00 und speichern", written out, no "u." (Janek).
- Core's button set drops `discard_below` and Discard from the row; the row is a first button and an optional second (my call).
- Both apps hold the time and buttons centred in the visible page and hang the note below; a short window or screen scrolls instead of lifting them (Janek, after review: "Wie am Handy: scrollen"). GTK: a spacer whose height follows the visible page height; Android: padding (my call).

## Capabilities
### Modified Capabilities
- `timer`: the running page's button set, the Add label and a steady layout.

## Impact
- Core: `meditate-core/src/session/mod.rs` (`EndButtons`, `end_buttons`)
- GTK: `src/window/imp.rs` (end controls, running pages), `src/timer/imp.rs` (refresh and clicks, Add label)
- Android: `ui/main.slint` (running stack, EndButtonView row), `src/ui.rs` (`refresh`), `src/app.rs` (tests, button codes)
- Translations: Add label changes in both apps, nine languages each (GTK `Add {time} and Save` -> `+ {time} and Save`, Android `Add {} and Save` -> `+ {} and Save`)
- Data: none
- Android build environment: untouched
- Docs: timer spec only (no other doc names these buttons)
