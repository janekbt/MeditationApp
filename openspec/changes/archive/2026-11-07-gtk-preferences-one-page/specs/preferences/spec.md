## ADDED Requirements

### Requirement: Linux Preferences are one page
On Linux, Preferences SHALL show every group on one page without tabs, in this order: Statistics, Bluetooth, Backup, Migrate, Nextcloud Sync, Danger Zone. Opened from the sync status in the headerbar, it SHALL open scrolled to Nextcloud Sync (to the WebDAV URL field, or to the App password field for Enter Password), with no cursor in a field and nothing selected.

#### Scenario: Preferences opened from the menu
- **WHEN** Preferences is opened from the main menu on Linux
- **THEN** there are no tabs; the page starts with Statistics, and scrolling down shows Bluetooth, Backup, Migrate, Nextcloud Sync and Danger Zone, each working as before
- **MANUAL** Janek on the laptop: the preferences dialog only renders in a running app

#### Scenario: Preferences opened from the sync status
- **WHEN** the sync status in the headerbar is clicked and its Preferences button pressed; then again with the password missing and its Enter Password button pressed
- **THEN** the dialog opens scrolled to Nextcloud Sync, with no cursor in any field and nothing selected
- **MANUAL** Janek on the laptop: scrolling only happens in a running app
