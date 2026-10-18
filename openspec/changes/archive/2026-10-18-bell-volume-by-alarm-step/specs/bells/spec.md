## ADDED Requirements

### Requirement: On Android a bell's volume sets the alarm step
On Android, a ringing bell SHALL set the alarm stream to the step its volume needs, trimmed by the player's gain, so the volume slider works even on a device that reports a wrong volume range or ignores the gain.

#### Scenario: A device that reports no usable range
- **WHEN** a device with alarm steps 1 to 7 reports only 1.2 dB between its quietest and loudest step, almost all of it below step 2 (-1.2, -0.1, -0.08 ... 0 dB), and bells are at 0 %, 50 % and 100 %
- **THEN** they set the alarm to step 1, 4 and 7
- **TEST** `meditate-core/src/bell_volume.rs::the_slider_picks_the_alarm_step_by_its_position`

#### Scenario: A volume between two steps
- **WHEN** a device's steps 1 to 5 play at -40, -30, -20, -10 and 0 dB, and a bell is at 40 %
- **THEN** it sets step 3 and plays 4 dB below it, at -24 dB like before this change
- **TEST** `meditate-core/src/bell_volume.rs::a_volume_between_two_steps_is_trimmed_by_the_gain`

#### Scenario: A steep curve at the bottom
- **WHEN** a device's steps 1 to 5 play at -60, -55, -50, -20 and 0 dB, and a bell is at 25 %
- **THEN** it sets step 4 (step 2 by position plays at -55 dB, quieter than the -45 dB the bell needs) and plays 25 dB below it
- **TEST** `meditate-core/src/bell_volume.rs::the_curve_can_only_raise_the_step`

#### Scenario: A quiet bell while a louder one rings
- **WHEN** a bell at 40 % starts while a bell at 100 % holds the stream at step 5 of that curve (-40 to 0 dB)
- **THEN** the quiet bell's gain at step 5 is 24 dB down, so it still plays at -24 dB
- **TEST** `meditate-core/src/bell_volume.rs::a_quieter_bell_keeps_its_level_under_a_louder_one`

#### Scenario: A report that isn't a curve
- **WHEN** the report is missing (before Android 9), has the wrong length, holds NaN, or falls from step to step
- **THEN** the steps are spread evenly over 40 dB
- **TEST** `meditate-core/src/bell_volume.rs::a_report_that_is_not_a_curve_falls_back_to_even_steps`

#### Scenario: The alarm volume comes back
- **WHEN** a bell rings and ends, and the user did not touch the alarm volume meanwhile
- **THEN** the alarm volume is what it was before
- **MANUAL** on the phone, set the alarm volume to 3 of 7, ring a bell at 100 %: the system alarm slider is back at 3 afterwards

#### Scenario: The slider changes the loudness on the device
- **WHEN** a bell's volume slider is dragged from 0 % to 100 % with the preview ringing
- **THEN** the bell gets clearly louder, on the FP5 as before
- **MANUAL** on the FP5, and on the tablet or by the reporter of #8
