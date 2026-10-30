# Destructive actions in red

## Why
Discard on the session screen and Delete in the confirm dialogs throw data away,
but on Android they wear the accent colour like every safe button. The Linux app
already shows them red (`destructive-action`, `ResponseAppearance::Destructive`).

## What Changes
- Session screen: Discard stays an outlined pill; its text and outline take the
  dark scheme's error colour (light red on black).
- The eight confirm dialogs that destroy data (Discard Session, Delete Label,
  Preset, Sound, Pattern, Bell, Delete all sessions, Wipe Local Data) show their
  confirm button in the error colour, and so does "Wipe Local" in the sync
  recovery dialog (GTK marks both wipe answers destructive). Cancel and all
  other dialogs stay as they are.
- Left out: GTK, already red. Trash icons in lists, already error-tinted. The
  "Delete all sessions" row in the preferences: it is a list row on Android, not a
  button, and its dialog turns red.

## Decisions
- Outlined Discard with red text and outline, not a red fill (Janek, option 1).
- All destructive dialogs, not only Discard (Janek); the two wipe answers count as
  destructive because GTK marks them so (my call, from the pre-build check).
- Discard's pressed tint is red too (my call).
- ConfirmDialog takes `destructive: bool`; its confirm button is a small local
  button on the library's BaseButton, since the library's TextButton has no
  colour input and `material-1.0/` is off limits (my call).
- WrapButton takes `destructive: bool` next to `dark`; EndButtonView sets it for
  Discard (my call).

## Capabilities
### Modified Capabilities
- `appearance`: destructive buttons in the error colour

## Impact
- Android: `ui/main.slint` (WrapButton, EndButtonView, ConfirmDialog, eight dialog uses, the recovery dialog's Wipe Local), guard test in `src/app.rs`
- Core, GTK, translations, data, docs: none. Android build environment: untouched (BaseButton is imported, not changed)
