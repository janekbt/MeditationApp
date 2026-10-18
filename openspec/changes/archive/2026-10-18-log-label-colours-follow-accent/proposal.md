# Log label colours follow the accent

## Why
The Log colours each labelled session (the stripe and the label name; on Linux also the chip background and its dot) from a fixed palette of eight colours, whatever accent the system (Linux) or the user (Android) chose. With a green or rose accent the Log stays blue, purple and orange. And because a label's colour comes from a checksum of its name, two labels often share one: with five labels, about four times in five. OPEN-ISSUES #186.

## What Changes
- Sixteen label colours, taken from the accent. Colours 1 to 8: the accent itself, then the colour wheel in 45 degree steps at the accent's brightness. Colours 9 to 16: the same ring turned a further 22.5 degrees and moved toward the text colour (darker in light mode, lighter in dark mode), so the chip text stays readable. From the 17th label on, colours repeat.
- Labels get their colour in the order they were created, the same on every device once synced: the first 16 labels always differ. A new label never changes another's colour on the same device; renaming keeps it; deleting a label moves the labels created after it up by one.
- Changing the accent recolours the Log right away (Linux: system accent; Android: Preferences accent picker). Sessions without a label stay grey.
- A device that creates labels (or restores a backup) before its first sync can move the other devices' colours once; after that every device agrees (Janek, 2026-10-18, after the review).
- Existing labels get new colours once, on the first start after the update; no data changes, so no migration and no notice (my call, flagged to Janek).
- Left out: a colour the user picks per label (needs a synced field; nobody asked).

## Decisions
- Colour wheel starting at the accent, not tones of one hue or near neighbours (Janek, 2026-10-18).
- Order of creation, not alphabetical and not the name checksum (Janek, 2026-10-18).
- 16 colours, 8 hues in two brightnesses (Janek, 2026-10-18). Changed from the offered option: the second ring moves toward the text colour (darker in light mode) instead of "lighter in light mode", because a lighter chip text on the white card would be hard to read (my call, flagged to Janek).
- "Created" is the earliest label-created event in the synced event log, ordered by its sync clock and then device id, so every device agrees once synced. Labels with no such event (none expected) come last, by local id. No schema or sync change (my call).
- Linux takes brightness and saturation from libadwaita's `@accent_color`; the second ring mixes 35 % of the window text colour into it, which works in light and dark without a second stylesheet (my call).
- Android: colour 1 is exactly the scheme's primary in each mode; the others turn its HSL hue and keep saturation and lightness; the second ring takes lightness L x 0.7 in light mode and L + 0.45 x (1 - L) in dark mode (my call).
- GTK turns the hue in oklch, Android in HSL: a label need not look the same on both apps, their accents already differ (my call).

## Capabilities
### Modified Capabilities
- `log`: label colours follow the accent and the order labels were created.

## Impact
- Core: `db/labels.rs` (colour place per label from the event log), `format.rs` (`label_color_class_index` and its tests removed; no other caller).
- GTK: `data/style.css` (16 `@define-color log_cN` relative to `@accent_color`, and the stripe, chip and dot rules for 16 classes), `src/log/imp.rs` (card colour from the place map loaded with the labels), one stylesheet test in `src/log/mod.rs`.
- Android: `src/theme.rs` (16-colour palette from a mode's primary), `src/ui.rs` (`apply_accent` sets light and dark palettes; the Log cards take the place from core), `ui/main.slint` (two palette properties; the card colour reads them by mode).
- Translations: none.
- Data: none (reads existing events only).
- Android build environment: untouched.
- Docs: README accent line names the Log; OPEN-ISSUES drops #186.
