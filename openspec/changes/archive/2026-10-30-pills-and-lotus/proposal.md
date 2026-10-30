# Pills and the lotus

## Why
On the laptop the streak above the timer and the label on each Log
card sit in tinted pills; the phone shows plain text. The "Meditation
complete" notification shows Android's play triangle in the status
bar, while the running session already shows the lotus there.

## What Changes
- Phone, Timer page: the streak sits in a pill like the laptop's: the
  accent at 15 % behind, accent text, bold, capitals, a little letter
  spacing. While the streak text is still empty the pill is hidden.
- Phone, Log: a labelled card shows its label in a pill like the
  laptop's: the label's own colour at 14 % behind, a small dot and the
  name in that colour. Long names still wrap.
- Phone: the "Meditation complete" notification shows the lotus.
- Left out: the laptop, which already has all three.

## Decisions
- Streak and label pills mirror the laptop's styling (Janek: go on the say-back, 2026-10-30).
- The label pill takes the label's colour, not the accent, as on the laptop (Janek: go).
- Lotus as for the running session (Janek).
- Pill sizes from GTK's CSS: streak padding 4/12 px, 14 px text; label padding 2/10 px (8 px before the dot), 6 px dot, 5 px gap, 12 px semibold text (my call).

## Impact
- Android: `ui/main.slint` (streak slot, Log card label), `kotlin/MeditateSessionService.kt` (notifyComplete), test in `src/app.rs`.
- Core, GTK, translations (no new strings), data, build environment: none.
- Docs: none.
