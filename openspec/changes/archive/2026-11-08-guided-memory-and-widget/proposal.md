# Guided memory and Guided in the widget

## Why
GitHub #22 asks for three things; the first (rearranging Starred Files)
shipped in 4e4d041. Left: when Android ends the app in the background,
the Guided file the user had chosen is gone at the next start, and the
home-screen widget can only start presets, so a guided file always
needs the app opened by hand. The maintainer added: a setting for the
order of the modes in the widget.

## What Changes
- Each device remembers the mode that was open last and the chosen
  Guided file (library or local folder file). At the next start the app
  opens in that mode with that file chosen, on the phone and on Linux.
  A file that is gone or not found by then stays unchosen.
- A one-off Open File pick is not remembered (the phone loses access to
  it at restart) and makes the app forget the file chosen before it.
- The widget lists the starred Guided files too, in the Starred Files
  order, with the same second line as there. A tap opens Guided and
  starts the file, like a preset tap; while a session runs or waits to
  be saved the tap only shows it; a file that is gone shows "File not
  found" and starts nothing.
- The widget lists Timer presets, then Guided files, then Box Breath
  presets.
- The phone's Preferences show the Danger Zone last, as on Linux
  (maintainer, during the device test).
- The widget's empty text names files too.
- Left out: section headings in the widget (maintainer: no headings).
  Dropped after the device test (maintainer): a Preferences drag list
  for the order of the modes in the widget; its rows would not drag on
  the phone, cause not found (see tasks.md).

## Decisions
- Widget: one flat list, no headings (maintainer).
- The last open mode is remembered too, both apps (maintainer).
- Last mode and last file are per device, never synced: a local folder
  file only exists on its device, and the laptop's choice must not move
  the phone's (my call).
- A widget tap on a local file whose length is still unread chooses it
  and does not start; the hero shows the read as for a tap in the app
  (my call; the start would have no length to run against).
- Not-found local files show in the widget with "Not found" as in
  Starred Files, so the lists agree (my call).
- The tap's launch value keeps the old extra name, so widget rows drawn
  by the previous version still start their preset after the update;
  a file's value is `guided:` plus its Starred Files key (my call).
- A widget tap wins over the remembered mode (my call). A session
  rescued after the process died is saved to the Log at start, as
  today; it does not change the mode (my call; no session resumes).
- The memory always mirrors the file shown as chosen: a delete while
  open forgets, its Undo remembers again, a failed length read forgets,
  the Create-from-Manage pick forgets like Open File (my call).
- A remembered local file whose length is still unread is not chosen
  at start, so no read and no notice runs before the user does
  anything (my call).

## Capabilities
### Modified Capabilities
- `guided`: the chosen file survives a restart; files start from the widget
- `setup`: the app opens in the mode used last
- `presets`: the widget's start rule covers files too
- `preferences`: the Danger Zone comes last on the phone

## Impact
- Core: `settings_keys.rs` (last mode), `guided.rs` (remember
  and restore the mode and file, device-local in `sync_state`)
- GTK: `timer/imp.rs` (save on mode switch and choice, restore at start)
- Android: `ui.rs` (save, restore, widget rows, deep link for files,
  `widget.rs` (launch value, ordering), `ui/main.slint` (Danger Zone
  last)
- Translations: the widget's empty text "No starred presets or files"
  in the ten `res/values*/strings.xml`. GTK none.
- Data: no schema change; two new device-local `sync_state` keys
- Android build environment: untouched (`res/values*/strings.xml` are
  resources, edited before without asking, e.g. ac5e3ab)
- Docs: README features (widget line), ARCHITECTURE.md if it lists
  `sync_state` keys
