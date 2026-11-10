# Timer (delta)

## ADDED Requirements

### Requirement: The Linux streak follows a new day on the Timer page
When the date changes while the Linux app runs, the Timer page SHALL show the streak for the new day without a restart or a tab switch.

#### Scenario: The app stays open past midnight on the Timer page
- **WHEN** the app is open on the Timer page with a streak that yesterday's session kept, and midnight passes without a session today
- **THEN** after midnight the streak line shows the new day's value
- **MANUAL** on the laptop: set the clock to 23:59 with the app on the Timer page, wait past midnight (GTK views can't run in host tests)
