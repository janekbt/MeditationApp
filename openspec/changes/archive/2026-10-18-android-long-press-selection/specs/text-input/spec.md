## ADDED Requirements

### Requirement: A long-press selection extends by word
On Android, after a long press selects a word, moving the held finger SHALL extend the selection by whole words and never shrink it below that word.

#### Scenario: The finger trembles inside the word
- **WHEN** the user long-presses a word and moves the finger a little within it before lifting
- **THEN** the whole word stays selected
- **MANUAL** on the FP5 and the API 35 emulator: the long press is Android-only. Upstream the core part has an automated test (task 1.1).

#### Scenario: The finger drifts before the long press fires
- **WHEN** the user presses a word and the finger drifts a few pixels, even into the next word, before the long press fires
- **THEN** the word where the press started is selected, not a part of it and not the next one
- **MANUAL** same devices; found in the review. Upstream it's a step of the same automated test.

#### Scenario: The finger moves on to the next word
- **WHEN** the user long-presses a word and drags right into the next word
- **THEN** the selection covers both whole words; dragging back left over the first word keeps the first word selected
- **MANUAL** same devices.
