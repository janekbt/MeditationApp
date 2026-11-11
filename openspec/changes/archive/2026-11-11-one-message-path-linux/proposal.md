# One message path on Linux

## Why
The Linux app builds its toasts about ten different ways. Some are one-line titles that a narrow window cuts with "…" (timer save failures, the Log edit dialog), some land on the main window's overlay while a pushed page covers it, so the user sees nothing (sound chooser import errors), and some glue a name onto a word so translators can't reorder it, or cut the name with "…" (guided file and pattern delete). Dialogs open through a guard that silently skips when it finds no window. P3 step 1: REFACTORING #320, #357, #373, with OPEN-ISSUES #362, #326, #349, #392, #311 riding along.

## What Changes
- Every Linux toast goes through one path in `db/mod.rs`:
  - `plain_toast(text)` and `undo_toast(text, on_undo, on_untaken)` build a toast whose title wraps; the caller may set a timeout.
  - `post_toast(anchor, slot, toast)` puts it on the anchor's page: its own overlay, or Preferences' when the anchor is in it. Such a page shows the toast also when the user only comes back to it, as when an editor pops. An anchor that is the window, or one already gone from it, means "no page of its own" (a bell error, a background save or scan): the toast goes to the page in view, picked once the current handler is done (Start pushes the running page after its own messages): an open dialog's overlay (Preferences, the Log editor), else the visible page's; inside a nested navigation view only its shown page counts. The running session page and the Local Folder page get an overlay of their own for this.
  - A new toast replaces the last one in its slot. Each area's Undo has its own slot (presets, guided files, the applied preset, the Log, a saved or recovered session), as before; pattern offers queue, as before. A plain message replaces only the same message on the same page (a slider step that fails again), so one failure never hides another. The bell error shows once per session or preview instead of once per run. The new toast is recorded and added first, and only then is the old one dismissed, so a dismiss handler that posts a toast cannot remove the new one.
  - `show_toast(anchor, text)` posts a plain toast.
- The three undo-toast builders (presets, guided, vibrations), the presets/guided `ToastSlot`s, `window_from`, `add_toast_to_window`, `present_toast`, `present_import_error_toast`, `data_toast` and the timer's `toast` go.
- The Log's delete toast and the timer's apply toast keep their own "which toast is current" state, because their dismiss handlers act on it. The Log sets that state before it calls `post_toast`, as it does today before its dismiss.
- Dialogs open with `dialog.present(Some(anchor))` instead of the root-downcast guard (about 22 places). File dialogs keep their `gtk::Window` parent.
- "Delete Bell?" is built once and used by the list row and the edit page.
- `Announcement` goes: core's `announcement.rs` and GTK's `announcement.rs`. Each call site writes its `gettext`/`ngettext` text. `format::ellipsize` goes, since it has no caller left.
- Wording, decided by the maintainer: a deleted guided file or pattern says "'{name}' deleted", the preset sentence that is already translated. An updated pattern says "'{name}' updated", which is new. An import error in the sound chooser uses the existing "Import failed: {error}".
- Left out:
  - The failures that only reach the log (EE, FF). They are step 3, and they will use this path.
  - The shell's `DbError` and its wrappers (#328). That is step 2.
  - Android. Its snackbar path is already one.

## Decisions
- Wording as above (maintainer, 2026-11-11: "Wie Presets").
- Delete `Announcement` rather than make it the only path. Only GTK used it, and the texts read better at the call site. (my call; REFACTORING #373 offered both)
- Undo offers replace only offers of their own area, as before, so a preset tap cannot take away "Session saved / Undo". A plain message never removes an undo offer. (my call, after review: an app-wide single offer would have)
- Undo toasts keep their timeouts: 8 s for "Session saved" and recovery, 5 s for the Log and patterns, the adw default elsewhere. Plain toasts take the libadwaita default of 5 s (show_toast had 4 s, the plain toasts 5 s), and the audio error keeps 6 s. (my call)
- "'{name}' updated" quotes the pattern's new name, the one it now has. Today's "Updated" toast quotes the old name. (my call)
- The source test that bans building or posting toasts allows only `db/mod.rs`, the path itself. (my call)
- The tests are source tests. GTK widgets only run on the thread that started GTK, and the one widget test already started it. (my call)

## Capabilities
### New Capabilities
- `messages`: how each app tells the user what happened. This change adds the Linux half; EE and FF will add to it.

## Impact
- Core: delete `announcement.rs` and `format::ellipsize` (and its test), and drop the `pub mod announcement` line.
- GTK: `db/mod.rs`, `timer/imp.rs`, `presets.rs`, `guided.rs`, `vibrations.rs`, `sounds.rs`, `sound.rs`, `local_folder.rs`, `labels.rs`, `preferences.rs`, `log/imp.rs`, `application.rs`, `bells.rs`, `stats/imp.rs`, `window/`, `main.rs`; delete `announcement.rs`.
- Android: none.
- Translations: GTK gains "'{name}' updated" and "Importing: {name}" (nine languages; the second was found while building, see tasks.md Notes) and loses "Deleted", "Updated", "Import failed:" and "Importing:". Android: none.
- Data: none.
- Android build environment: untouched.
- Docs: `meditate-core/README.md` (the module table), REFACTORING.md (#320, #357, #373 out, plus the `announcement.rs` note in item #426's slice), OPEN-ISSUES.md (#362, #326, #349, #392, #311 out).
