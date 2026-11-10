# Log (delta)

## MODIFIED Requirements

### Requirement: A filtered Log shows only matching sessions
The Log SHALL show only sessions that match its filters, also right after a session is saved.

#### Scenario: A session with another label is saved while the Log is filtered
- **WHEN** the Log is filtered to "Yoga" and a "Zen" session is saved
- **THEN** the Log reloads from the database when shown, so it shows no Zen card, and Load more continues at the next Yoga session
- **MANUAL** on the laptop: filter the Log to one label, save a session with another, open the Log (GTK views can't run in host tests; every save now reloads the Log, the hand-added card that skipped the filter is gone)
