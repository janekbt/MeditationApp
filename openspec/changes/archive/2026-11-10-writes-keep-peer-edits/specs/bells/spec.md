## ADDED Requirements

### Requirement: The bundled Kansho bell has one name everywhere
The bundled Kansho bell SHALL be called "Kansho" in both apps and stay so after a sync, without sending anything; a name the user gave it, other than "Kanshō", is kept.

#### Scenario: A laptop that stored "Kanshō" updates
- **WHEN** a database holds the bundled bell as "Kanshō" and opens with this version
- **THEN** it is "Kansho", nothing is sent, and a synced phone shows "Kansho" too
- **TEST** `meditate-core/src/db/seeds.rs::the_bundled_kansho_bell_drops_its_macron_once`

#### Scenario: A laptop that stored "Kanshō" and ran Wipe Local updates
- **WHEN** the bundled bell is "Kanshō" with no sync history left for it, and the database opens with this version
- **THEN** it is "Kansho" and nothing is sent
- **TEST** `meditate-core/src/db/seeds.rs::a_kansho_bell_without_events_drops_its_macron_too`

#### Scenario: The user types the old spelling
- **WHEN** the user renames the bundled bell to "Kanshō"
- **THEN** it stays "Kansho" and nothing is sent
- **TEST** `meditate-core/src/db/seeds.rs::typing_kansho_with_its_macron_sends_nothing`

#### Scenario: The user renamed the bell
- **WHEN** the bundled bell is named "Temple" by the user and the database opens with this version
- **THEN** it stays "Temple"
- **TEST** `meditate-core/src/db/seeds.rs::a_user_name_on_the_kansho_bell_stays`
