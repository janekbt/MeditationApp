## ADDED Requirements
### Requirement: A touched volume slider holds the page still
While a finger rests on a bell's volume slider, the page SHALL not scroll, so a drag that drifts up or down keeps moving the slider.

#### Scenario: A drag that drifts up and down
- **WHEN** on the setup page or in the interval bell editor a finger lands on a volume slider and moves sideways and up or down, then lifts, and a swipe then starts beside the slider
- **THEN** the slider follows the finger sideways, the page does not move while the finger is down nor when it lifts, the value where it lifts is saved, and the swipe beside the slider scrolls the page
- **TEST** `meditate-android/src/app.rs::a_touched_volume_slider_holds_the_page_still`

#### Scenario: The same drag on the FP5
- **WHEN** on the FP5 a finger lands on a bell's volume slider in the Timer setup and in the interval bell editor, drags left and right while drifting up and down, then lifts; then swipes up beside the slider
- **THEN** the slider follows sideways and never snaps back, the page stays still while the finger is down and does not jump or glide when it lifts, and the later swipe scrolls
- **MANUAL** touch handover between slider and page only happens on the phone

## MODIFIED Requirements
### Requirement: A volume drag the page takes over changes nothing
When the page takes over a touch that started on a volume slider (a quick swipe up or down in its first 0.1 s, before the slider holds the page), the slider SHALL keep the value it had before the touch, which is the saved one.

#### Scenario: A vertical swipe that starts on the slider
- **WHEN** a bell is at 80%, a finger lands on its slider at 30% and the page then takes the touch
- **THEN** the slider shows 80% again and the bell rings at 80%
- **TEST** `meditate-android/src/app.rs::a_cancelled_volume_drag_puts_the_slider_back`

#### Scenario: The same swipe on the FP5
- **WHEN** on the FP5 a quick swipe up starts on a bell's volume slider
- **THEN** the page scrolls and the slider shows the value it had before
- **MANUAL** touch handover between slider and page only happens on the phone
