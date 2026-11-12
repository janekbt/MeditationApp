# Number field tap

## Why
On Linux, clicking a number field puts the cursor before or after the number, so it must be deleted before a new one can be typed. On Android a tap empties the field, so typing replaces the number, but the old number is gone from view and the user may no longer know what it was.

## What Changes
- Linux: focusing any number field (click, tap or Tab) selects its whole number; typing replaces it, leaving without typing keeps it.
- Android: a tapped number field still empties (a selection there pops the cut/copy/paste bar over the number), but shows the old number pale until the first digit is typed. Leaving it empty keeps the number, as today.
- Left out: Linux reading an emptied field as the old number. With the number selected, an untouched field keeps it anyway; a field the user deletes and leaves empty commits as today (GTK clamps it to the lowest value).

## Decisions
- Linux selects instead of emptying, so the number stays visible (Janek, 2026-11-12).
- Android empties and shows the old number pale (Janek, 2026-11-12, over "select too" and "unchanged").
- One handler on the main window selects the number of whatever number field gets the focus, today's and future ones, instead of a call per field; every Linux number field lives in the main window or in a dialog shown inside it (my call).
- The pale number uses the field's font and size in the Material "on surface variant" colour, the shade of the field labels (my call).

## Capabilities
### Modified Capabilities
- `text-input`: a tapped number field readies for a new number while the old one stays visible, both apps.

## Impact
- Core: none (focus and selection are widget mechanisms).
- GTK: `window/mod.rs` (the handler and its test), `window/imp.rs` (wired in `constructed`).
- Android: `ui/main.slint` (VerticalSpinBox and StepperRow: the pale old number).
- Translations: none.
- Data: none.
- Android build environment: untouched.
- Docs: none beyond the spec delta.
