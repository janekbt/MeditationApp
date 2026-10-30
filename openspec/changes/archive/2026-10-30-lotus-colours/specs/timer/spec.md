## ADDED Requirements
### Requirement: The media control's buttons use the app's own symbols
On the phone, every button of the session's media control SHALL show the symbol the app itself uses: Pause and Resume as the app's pause and play, Save as a check mark, Add as the app's plus, Discard as an X.

#### Scenario: A paused session and overtime in the media control
- **WHEN** a session is paused, and later another one runs into overtime, with the media control opened on the lock screen
- **THEN** paused shows the app's play symbol, a check mark and an X; overtime shows a check mark, the app's plus and an X; while running it shows the app's pause symbol
- **MANUAL** on the FP5: the system draws the media control, so only a phone shows it.

#### Scenario: A later edit brings back a system symbol or another drawing
- **WHEN** any media control button's symbol is changed to a system symbol or a drawing other than the app's
- **THEN** the build's tests fail
- **TEST** `meditate-android/src/app.rs::the_media_buttons_show_the_apps_own_symbols`
