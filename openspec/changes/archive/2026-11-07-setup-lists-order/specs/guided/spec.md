## ADDED Requirements

### Requirement: Starred Files stand in the order the user gives them
Moving a row in Starred Files SHALL put it at the new place, with library and local files in any mix, and only the library files' order among themselves SHALL reach other devices.

#### Scenario: A local file moved between two library files
- **WHEN** Starred Files reads library A, library B, local x and x is moved to the second place
- **THEN** it reads A, x, B, and no sync event is written
- **TEST** `meditate-core/src/local_folder.rs::a_local_file_moves_between_library_files_without_sync`

#### Scenario: A library file moved only past local files
- **WHEN** it reads library A, local x, local y, library B and A is moved to the third place
- **THEN** it reads x, y, A, B, and no sync event is written
- **TEST** `meditate-core/src/local_folder.rs::a_library_file_moved_past_only_local_files_writes_no_sync`

#### Scenario: A library file moved past another reaches the other device
- **WHEN** it reads library A, local x, library B and B is moved to the top, then both devices sync
- **THEN** this device reads B, A, x; the other device's Starred Files read B, A; one sync event was written
- **TEST** `meditate-core/src/local_folder.rs::a_library_reorder_reaches_the_other_device`

#### Scenario: The other device reorders the library files
- **WHEN** this device reads library A, local x, library B, C and the other device moves C to the top, then they sync
- **THEN** this device reads C, A, x, B
- **TEST** `meditate-core/src/local_folder.rs::a_reorder_from_another_device_keeps_local_files_in_place`

#### Scenario: A file moved on Manage Imports
- **WHEN** the library holds A, B (not starred), C, Starred Files reads A, local x, C, and C is moved to the top of Manage Imports
- **THEN** Manage Imports reads C, A, B, Starred Files reads C, A, x, and one sync event is written
- **TEST** `meditate-core/src/local_folder.rs::a_move_on_manage_imports_shows_in_starred_files`

#### Scenario: A local file moved between two files imported at the same moment
- **WHEN** two starred library files carry the same import time and a local file is moved between them
- **THEN** Starred Files reads in the new order, and Manage Imports keeps its order, an unstarred file included
- **TEST** `meditate-core/src/db/starred_order.rs::a_move_with_no_room_keeps_the_manage_imports_order`

#### Scenario: A Manage Imports move between two files imported at the same moment
- **WHEN** an unstarred file is moved between two starred library files with the same import time, a local file standing after them
- **THEN** Manage Imports reads in the new order and Starred Files keeps its order
- **TEST** `meditate-core/src/db/starred_order.rs::a_manage_imports_move_with_no_room_keeps_local_places`

#### Scenario: A newly starred local file comes last
- **WHEN** a local file is starred while A, x, B are starred, or unstarred and starred again
- **THEN** it stands last
- **TEST** `meditate-core/src/local_folder.rs::a_newly_starred_local_file_comes_last`

#### Scenario: A drop on its own place
- **WHEN** a row is dropped where it stands
- **THEN** nothing is written
- **TEST** `meditate-core/src/local_folder.rs::a_starred_file_dropped_on_its_place_writes_nothing`

#### Scenario: Starred files from an older version keep their order
- **WHEN** a database from v26.11.0 with two starred library and two starred local files is opened
- **THEN** Starred Files reads the library files by import, then the local files by name, as before
- **TEST** `meditate-core/src/local_folder.rs::starred_files_from_an_older_database_keep_their_order`

#### Scenario: Dragging on Linux and on the phone
- **WHEN** on the Guided Setup a Starred Files row is dragged by its handle and dropped on another row
- **THEN** it stands at that row's place; a swipe elsewhere on a row, or a quick swipe on the handle on the phone, scrolls instead
- **MANUAL** drag needs a pointer or a finger; on Linux and the FP5

#### Scenario: Dragging on Manage Imports
- **WHEN** a Manage Imports row is dragged by its handle and dropped on another row, on Linux or the phone
- **THEN** it stands at that row's place there and in Starred Files
- **MANUAL** drag needs a pointer or a finger; on Linux and the FP5

#### Scenario: Without dragging on Linux
- **WHEN** on Linux a Starred Files or Manage Imports row has the keyboard focus and Alt+Up or Alt+Down is pressed
- **THEN** the file moves one place up or down and keeps the focus; at the top or bottom nothing happens
- **MANUAL** keyboard input on the GTK page; on Linux
