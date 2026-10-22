## ADDED Requirements

### Requirement: Android's breath dot follows the rounded square without jumps
On Android the Box Breath dot SHALL travel along the rounded square's outline, each phase ending where the next one starts.

#### Scenario: Phase boundaries and corners
- **WHEN** the dot reaches the end of In, Hold, Out and Hold
- **THEN** it stands where the next phase starts, on the outline, and halfway through a corner it lies on the corner's arc
- **TEST** `meditate-android/src/app.rs::breath_dot_path_has_no_jumps`
