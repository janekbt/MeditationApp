# Text input

## Purpose
Text fields on Android take their text, cursor and selection from the
system keyboard and show the system's cut/copy/paste bar. This covers
how they follow the keyboard and where that bar appears.

## Requirements

### Requirement: A cursor move from the keyboard moves the cursor
On Android, a text field SHALL move its cursor or selection to where the keyboard sets it, without changing the text.

#### Scenario: Sliding over the space bar
- **WHEN** the user slides over Gboard's space bar in a field with text, left and then right
- **THEN** the cursor follows the slide in both directions, and typing a letter inserts it at the new place
- **MANUAL** on the FP5 (Android 15) and the API 30 and 35 emulators: Java and the Android IME can't run on the host.

#### Scenario: The cursor stays visible while it slides
- **WHEN** the user slides over Gboard's space bar
- **THEN** the cursor stays lit while it moves and starts blinking again only once it stops, as with the arrow keys
- **MANUAL** on the FP5: found in the device test (maintainer); blinking needs a person watching.

#### Scenario: Sliding over the backspace key selects, then deletes
- **WHEN** the user slides left from Gboard's backspace key over two words and lets go
- **THEN** the words are highlighted while sliding and deleted on release
- **MANUAL** same devices; it uses a selection range, not a collapsed cursor.

#### Scenario: The move is not an edit
- **WHEN** the user slides the cursor in the rename dialog of a label without typing
- **THEN** the text is unchanged and the dialog behaves as if nothing was typed
- **MANUAL** on the FP5 with the spike's logging: no text update reaches Rust for the move (log shows only the selection), so `edited` can't fire.

#### Scenario: Sliding right after a word with suggestions
- **WHEN** the user types a word with Gboard's suggestions on and slides over the space bar at once
- **THEN** the word stays where it was typed, written once, and the cursor moves
- **MANUAL** on the FP5: Gboard ends the word without telling Slint first.

#### Scenario: Sliding in a number field
- **WHEN** the user slides the cursor in a field that only takes digits
- **THEN** the cursor moves and the digits stay as they were
- **MANUAL** on the FP5.

#### Scenario: Moving the cursor while a word is still being composed
- **WHEN** the user types part of a word (Gboard underlines it) and slides over the space bar before finishing it
- **THEN** the suggestions keep matching the underlined word, and the word is written once, where it was typed
- **MANUAL** on the FP5: the composing range lives in Gboard; upstream's #14205 as merged reports it with each selection change.

### Requirement: The copy bar sits next to the selection
On Android, the cut/copy/paste bar SHALL appear above the selected text, or below it when there's no room above, never on it.

#### Scenario: A selection in the middle of the screen
- **WHEN** the user long-presses a word in a field in the middle of the screen
- **THEN** the bar sits directly above the highlighted word and the word stays readable
- **MANUAL** on the FP5 and the API 26, 30 and 35 emulators.

#### Scenario: A selection near the top of the screen
- **WHEN** the user selects a word in a field just below the app bar
- **THEN** the bar sits above the word if it fits, otherwise below it, never on it
- **MANUAL** same devices; this is where Slint's `top < 0` workaround applies.

### Requirement: A long-press selection extends by word
On Android, after a long press selects a word, moving the held finger SHALL extend the selection by whole words and never shrink it below that word.

#### Scenario: The finger trembles inside the word
- **WHEN** the user long-presses a word and moves the finger a little within it before lifting
- **THEN** the whole word stays selected
- **MANUAL** on the FP5 and the API 35 emulator: the long press is Android-only. Upstream the core part has an automated test (task 1.1).

#### Scenario: The finger drifts before the long press fires
- **WHEN** the user presses a word and the finger drifts a few pixels before the long press fires
- **THEN** the selection starts at the word where the press started and reaches the word under the finger: within the same word only that word, into the next word both words, as upstream merged it
- **MANUAL** same devices; upstream's `text_input_long_press` test covers the core part.

#### Scenario: The finger moves on to the next word
- **WHEN** the user long-presses a word and drags right into the next word
- **THEN** the selection covers both whole words; dragging back left over the first word keeps the first word selected
- **MANUAL** same devices.

#### Scenario: A long press on an empty line or after the last word
- **WHEN** the user long-presses an empty line in a multi-line field, or behind the end of the text
- **THEN** no word is selected and the press goes on like a plain press: the cursor sits where the finger is
- **MANUAL** on the FP5 and the API 35 emulator; upstream's `text_input_long_press` test covers the core part.

#### Scenario: A double tap that is then held
- **WHEN** the user double-taps a word and keeps the finger down until the long press fires
- **THEN** the word from the double tap stays selected, and it does not grow
- **MANUAL** same devices.
