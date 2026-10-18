Title: android: Place the text selection menu next to the selection

Description (identical to the commit message, since a squash merge uses it):

The cut, copy and paste menu covered the selected text instead of
appearing above it. ActionMode.Callback2.onGetContentRect() expects a
rectangle relative to the view, but got the selection in window
coordinates. The input view sits below the status bar unless the window
is edge-to-edge, so the menu ended up lower by the height of the status
bar.

Convert the selection to the view's coordinates. The selection handles
keep using window coordinates, which is what PopupWindow.showAtLocation()
expects.

== Implementation Notes ==

The top < 0 workaround after the conversion now sees view coordinates.
It may have compensated for this offset, but that isn't proven, so it
stays as it is.

ChangeLog: Android: The text selection menu appears next to the selected text instead of covering it

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

---

First comment (review only, not part of the commit):

## How this shows up

Long-press a word in a text field: the menu sits on top of the text, or
below the field over the next one, instead of above the selection.

Logged on a Fairphone 5 (Android 15), where the status bar is 110 px:

```
onGetContentRect window=Rect(99, 1483 - 829, 1611), view at y=110
```

Android adds the view's position to the rectangle again, so the menu is
placed 110 px too low.
The emulators show the same with their status bar height, 63 px on API 26
and 30 and 128 px on API 35.

## Testing

- `material-gallery` from this branch on the API 35 emulator: the menu
  appears above the field, also for the field right below the app bar.
  On master it appears below the field, over the next one.
- The same patch on 1.18.1 in an app: Fairphone 5 (Android 15) and
  emulators on API 26, 30 and 35.
- The Java compiles through the crate's `build.rs`.
