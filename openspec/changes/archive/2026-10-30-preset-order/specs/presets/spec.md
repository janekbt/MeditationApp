## ADDED Requirements

### Requirement: Presets stand in the order the user gives them
Moving a preset on the Manage Presets page SHALL put it at the new place in every list of that mode's presets, on every synced device.

#### Scenario: Dragging a preset to another place
- **WHEN** three Timer presets stand as A, B, C and C is moved to the top
- **THEN** the Manage list and the starred list read C, A, B
- **TEST** `meditate-core/src/db/presets.rs::a_moved_preset_takes_its_new_place`

#### Scenario: Moving down
- **WHEN** A, B, C and A is moved to the last place
- **THEN** the list reads B, C, A
- **TEST** `meditate-core/src/db/presets.rs::a_preset_moved_down_lands_below_the_others`

#### Scenario: A drop on its own place
- **WHEN** a preset is dropped where it already stands
- **THEN** nothing is written and nothing is sent to other devices
- **TEST** `meditate-core/src/db/presets.rs::a_preset_dropped_on_its_place_writes_nothing`

#### Scenario: Another device, also one with an older version
- **WHEN** a device moves a preset and another device syncs
- **THEN** the other device lists the presets in the new order
- **TEST** `meditate-core/src/db/presets.rs::another_device_gets_the_new_order`

#### Scenario: An edit elsewhere to a preset the move did not pass
- **WHEN** A, B, C, D; device 1 swaps A and B, device 2 renames D before it synced, then both sync
- **THEN** both devices read B, A, C, D with D renamed
- **TEST** `meditate-core/src/db/presets.rs::an_edit_elsewhere_to_a_preset_not_moved_keeps_the_order`

#### Scenario: An edit or delete elsewhere to a preset the move passes
- **WHEN** A, B, C; device 2 renames B and deletes A before it synced, device 1 moves C to the top later, then both sync
- **THEN** both devices read C, B with B renamed; A stays deleted
- **TEST** `meditate-core/src/db/presets.rs::an_edit_elsewhere_to_a_preset_the_move_passes_is_kept`

#### Scenario: A move past the end of the list
- **WHEN** A, B, C and A is moved to a place past the end
- **THEN** the list reads B, C, A
- **TEST** `meditate-core/src/db/presets.rs::a_move_past_the_end_lands_last`

#### Scenario: Two devices move presets before they sync
- **WHEN** A, B, C, D; device 1 moves D and device 2 moves C to the second place, then both sync
- **THEN** both devices show the same order, A first and B last
- **TEST** `meditate-core/src/db/presets.rs::two_devices_moving_before_they_sync_end_in_one_order`

#### Scenario: Many moves into the same place
- **WHEN** a preset is moved between the same two presets sixty times in a row, then twice to the top
- **THEN** each move lands where it was dropped and changes only the moved preset
- **TEST** `meditate-core/src/db/presets.rs::many_moves_into_one_gap_write_only_the_moved_preset`

#### Scenario: Two devices move into the same place, then a third move goes between them
- **WHEN** A, B, C, D; device 1 moves D and device 2 moves C to the second place, both sync, device 2 deletes A, device 1 moves B between C and D later, then both sync
- **THEN** both devices show the same three presets with B in the middle; A stays deleted
- **TEST** `meditate-core/src/db/presets.rs::moves_into_one_gap_on_two_devices_leave_room_between_them`

#### Scenario: Two devices move two presets into the same place
- **WHEN** A, B, C, D, where C's and D's ids start alike; device 1 moves D and device 2 moves C to the second place, both sync, then B is moved between them
- **THEN** B lands between them and only B is changed
- **TEST** `meditate-core/src/db/presets.rs::moves_of_two_presets_into_one_gap_never_share_a_place`

#### Scenario: The top two swapped again and again
- **WHEN** the first preset is moved to the second place two hundred times, then the last one to the top two hundred times
- **THEN** every preset's stored place stays short, so the data sent on every later change stays small
- **TEST** `meditate-core/src/db/presets.rs::swapping_the_top_two_again_and_again_keeps_the_place_short`

#### Scenario: A move to the top after moves in the middle
- **WHEN** A, B, C; C is moved to the second place, A to the end, then B to the top
- **THEN** the list reads B, C, A and each move changed only the moved preset
- **TEST** `meditate-core/src/db/presets.rs::a_move_to_the_top_after_moves_in_the_middle_writes_only_the_moved_preset`

#### Scenario: Many random moves on two devices that sync now and then
- **WHEN** six hundred random moves are made on two devices that sync every fifty moves
- **THEN** each move lands where it was dropped and changes only the moved preset, both devices show one order after every sync, and the stored places stay short
- **TEST** `meditate-core/src/db/presets.rs::random_moves_write_only_the_moved_preset`

#### Scenario: The two bundled Box Breath presets moved into one place on two devices
- **WHEN** device 1 moves 4-4-4-4 and device 2 moves 4-7-8-0 between the same two presets, both sync, then a preset is moved between them
- **THEN** it lands between them and only it is changed
- **TEST** `meditate-core/src/db/presets.rs::the_bundled_box_breath_presets_moved_into_one_gap_on_two_devices_leave_room`

#### Scenario: Moves to the end while this device's clock is behind
- **WHEN** the last preset's place was written by a device whose clock runs far ahead, and the last two are swapped two hundred times
- **THEN** each move lands where it was dropped, changes only the moved preset, and the stored places stay short
- **TEST** `meditate-core/src/db/presets.rs::swapping_the_last_two_while_the_clock_is_behind_keeps_the_place_short`

#### Scenario: A preset saved after a move to the end
- **WHEN** A, B; A is moved to the end, then C is saved
- **THEN** the list reads B, A, C
- **TEST** `meditate-core/src/db/presets.rs::a_preset_moved_to_the_end_stays_above_one_saved_later`

#### Scenario: Two presets created at the same moment
- **WHEN** two presets carry the same creation time and one of them is moved
- **THEN** the list reads in the new order all the same
- **TEST** `meditate-core/src/db/presets.rs::presets_with_the_same_time_can_still_be_moved`

#### Scenario: The other mode's presets
- **WHEN** a Timer preset is moved
- **THEN** the Box Breath presets keep their order
- **TEST** `meditate-core/src/db/presets.rs::a_moved_preset_takes_its_new_place`

#### Scenario: Dragging on Linux and on the phone
- **WHEN** on the Manage page a row is dragged by its handle and dropped on another row
- **THEN** it stands at that row's place; the home page's starred list and the Android widget follow
- **MANUAL** drag needs a pointer or a finger; on Linux and the FP5

#### Scenario: Without dragging on Linux
- **WHEN** on Linux a row has the keyboard focus and Alt+Up or Alt+Down is pressed
- **THEN** the preset moves one place up or down and keeps the focus; at the top or bottom nothing happens
- **MANUAL** keyboard input on the GTK page; on Linux
