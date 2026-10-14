## ADDED Requirements

### Requirement: The streak line follows saved sessions on the phone
After a session is saved, edited or brought back with Undo on the phone, the Timer page's streak line and Stats SHALL include it.

#### Scenario: First session of the day
- **WHEN** the streak line says "No streak" and the user saves today's first session
- **THEN** the streak line shows the new streak without opening Stats
- **TEST** `meditate-android/src/app.rs::saving_or_undoing_a_session_refreshes_the_streak`

#### Scenario: Recovery Undo
- **WHEN** a recovered session is undone
- **THEN** the streak line and Stats no longer count it
- **TEST** `meditate-android/src/app.rs::saving_or_undoing_a_session_refreshes_the_streak`
