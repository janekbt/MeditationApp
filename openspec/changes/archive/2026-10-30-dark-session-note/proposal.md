# Dark note field on the session screen

## Why
After 10b37e0 the note field on the paused or finished session screen is the
only light element on black in light mode. Janek wants it dark too (#15 follow-up).

## What Changes
- The note field on the session screen takes the dark scheme's colours in both
  themes, as it already looks in dark mode. The note field on the Edit Session
  page keeps the current theme.

## Decisions
- Dark note field on the session screen (Janek).
- NoteField and WrapButton both take one `in property <bool> dark` that switches
  their roles to `MaterialPalette.schemes.dark`; only the session screen sets it.
  WrapButton's three colour inputs from 10b37e0 became this switch (my call, /simplify).

## Impact
- Android: `ui/main.slint` (NoteField, its session-screen use), guard test in `src/app.rs`
- Core, GTK, translations, data, docs: none. Android build environment: untouched
