# Preset order

## Why
Presets always stand in the order they were created, oldest first, and
nothing changes that. A preset used every day can end up at the bottom
of the starred list on the home page and in the widget (GitHub #12).

## What Changes
- On the Manage Presets page each row gets a drag handle at its left;
  dragging a row to another place moves the preset there, in both apps.
- The new order holds for that mode everywhere presets are listed: the
  Manage and Save pages, the starred presets on the home page and the
  Android widget, and on every synced device.
- On Linux the keyboard can move a preset too, without dragging.
- Left out: reordering on the Save page (a tap there overrides the
  preset; one place to sort is enough), moving presets between modes
  (a Timer preset can't run as Box Breath).

## Decisions
- Drag at a handle in both apps (Janek, 2026-10-30).
- A hidden second way on Linux: Alt+Up / Alt+Down on the focused row
  (Janek, 2026-10-30 "Unsichtbar mitgeben"). Android: drag only; the
  app has no screen-reader bridge (the vendored backend has none), so
  a hidden way would reach no one (Janek, 2026-10-30).
- The order lives in the preset's existing creation time text, not a
  new field: a moved preset gets a text that sorts between its new
  neighbours' (to the end: the current time; to the top: the first
  preset's text with its first digit above 0 lowered and a 9 after it; else the
  upper neighbour's text, shortened and with a digit raised or digits
  added, so there is always room and it stays short, plus 16 digits
  that stand for the end of its id one to one, so two devices' moves
  into one gap never meet). No schema or event change, no migration, and a device on an
  older version shows the same order at once (my call; a new field
  would be dropped again by any older device that edits the preset;
  nothing reads a preset's creation time as a time).
- Only the moved preset is written, one update (Janek, 2026-10-30,
  after the review: rewriting every preset it passed let a sync drop an
  edit made elsewhere to one of them, or bring back one deleted there).
- Only two neighbours with equal text leave no room (data from
  outside; the app's own moves never make it); then the move gives the
  mode's presets fresh times 1 ms apart from now (my call).
- Accepted limit: the moved preset is written whole, so an edit to that
  same preset on another device before it synced loses to the later of
  the two, as any two edits of one preset do today; the presets it
  passed are never touched. Two devices moving presets before they sync
  end in one order, the same on every device.
- The handle's tooltip on Linux: "Drag to reorder" (my call; Android
  shows no tooltips, so no new Android text).
- Android drag: Slint's DragArea on the handle, a DropArea per row; the
  uuid travels as plain text built on the Rust side (my call). In a
  list long enough to scroll, a quick swipe on the handle scrolls; a
  short press, then dragging, moves the preset. Nothing follows the
  finger: the row being dragged dims and a line shows where it lands.
  The list does not scroll during a drag; a preset goes far in two
  moves (my call, Slint's drag has no auto-scroll).
- A failed move shows the existing "Save failed" message and the list
  as stored (my call: the presets' existing failure path).

## Capabilities
### Modified Capabilities
- `presets`: presets can be put in an order of the user's own

## Impact
- Core: `db/presets.rs` (new `move_preset(uuid, to_index)`)
- GTK: `presets.rs` (handle, drag source and drop target per Manage
  row, Alt+Up/Down), a bundled handle icon in the gresource
- Android: `ui/main.slint` (handle, drag and drop on the Manage rows), `src/ui.rs` (callback: move, reload the chooser,
  home list and widget), a bundled handle SVG in `ui/assets/`
- Translations: "Drag to reorder", nine languages, Linux only
- Data: none (existing preset update events)
- Android build environment: untouched
- Docs: README presets line, `EVENTS.md` note that `created_iso` of a
  preset is its place in the list
