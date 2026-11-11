# Messages

## Purpose
How each app tells the user what happened: where a message appears, that it shows in full, and how it names the item it is about.

## Requirements

### Requirement: Linux messages show in full on the page in view
Every message of the Linux app SHALL wrap instead of being cut, and SHALL appear on the page or dialog the user is looking at.

#### Scenario: A long failure message on a narrow window
- **WHEN** saving a session fails while the window is narrow
- **THEN** "Couldn't save session: storage error" shows in full, on as many lines as it needs
- **TEST** `meditate-gtk/src/db/mod.rs::every_toast_goes_through_the_shared_path`

#### Scenario: A long name with no spaces, next to Undo
- **WHEN** the user deletes a pattern named "Testmusteraaaaaaaaaaaaaaaaaaaaaaaaaa" on a phone-wide window
- **THEN** the name breaks onto the next lines inside the message, the page keeps its width, and the Undo button shows in full
- **TEST** `meditate-gtk/src/db/mod.rs::every_toast_goes_through_the_shared_path`

#### Scenario: An import error while the sound chooser covers the main page
- **WHEN** the user picks a file larger than 10 MB in the bell sound chooser
- **THEN** "File is larger than 10 MB" shows on the chooser page
- **MANUAL** On the laptop, open Bells, then a sound, then Import, and pick a large file: the message appears on the chooser. The overlay a toast lands on is only visible in a running window.

#### Scenario: A message with no page of its own while a pushed page is open
- **WHEN** the user taps Refresh on the Local Folder page
- **THEN** "Folder refreshed" shows on the Local Folder page
- **MANUAL** On the laptop, open Guided, then Local Folder, and tap Refresh: the message appears on that page. Only a running window has a page in view.

#### Scenario: A message from Preferences
- **WHEN** saving the sync settings fails while Preferences is open
- **THEN** the message shows inside Preferences
- **MANUAL** On the laptop, open Preferences, then Sync, and save with an empty password: "Enter a password" appears inside the dialog.

### Requirement: Linux confirm dialogs always open
A Linux confirm or name dialog SHALL open whenever its button is pressed.

#### Scenario: Deleting a bell from its edit page
- **WHEN** the user taps Delete Bell on a bell's edit page
- **THEN** the same "Delete Bell?" dialog opens as from the list row
- **TEST** `meditate-gtk/src/db/mod.rs::dialogs_open_on_their_anchor`

### Requirement: Messages name an item in full, in one translatable sentence
A Linux message about a named item SHALL quote the whole name inside one sentence that translators can reorder.

#### Scenario: Deleting a guided file with a long name
- **WHEN** the user deletes "Body scan with long pauses, morning"
- **THEN** the message reads "'Body scan with long pauses, morning' deleted", with no "…"
- **TEST** `meditate-gtk/src/db/mod.rs::no_message_cuts_or_glues_a_name`

#### Scenario: Updating a vibration pattern
- **WHEN** the user renames the pattern "Pulse" to "Wave" and saves
- **THEN** the message reads "'Wave' updated", in the user's language
- **MANUAL** On the laptop in German, open Vibration, rename a pattern and save: the message quotes the new name inside the German sentence. Which name the toast gets is only visible in a running window.
