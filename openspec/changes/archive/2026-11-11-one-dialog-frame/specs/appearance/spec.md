## ADDED Requirements

### Requirement: Every Android dialog has the same frame
On Android, every dialog SHALL show one card of one width over a dimmed window, with a title that wraps instead of being cut.

#### Scenario: A long title in a narrow window
- **WHEN** any dialog opens (filter, Custom Time, Preparation Time, a name dialog, Daily Goal, a confirm dialog, Recovery, Sync status, Label conflict) and its title is longer than one line
- **THEN** the title wraps onto a second line; every card is as wide as the others, and a tap outside the card closes it as before (Cancel or Close; Label conflict: decide later; never while a file is converting)
- **TEST** `meditate-android/src/app.rs::every_dialog_has_one_frame`

#### Scenario: On the FP5 in German
- **WHEN** Filter, Custom Time, Preparation Time, Daily Goal and Delete Preset are opened with the phone in German
- **THEN** all five cards have the same width, every title shows in full, and the buttons look as before
- **MANUAL** layout and fonts exist only on a device
