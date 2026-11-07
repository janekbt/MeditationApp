## ADDED Requirements
### Requirement: Starring on the Local Folder page keeps the place
Starring or unstarring a file on the Linux Local Folder page SHALL keep the focus on that file's star and the list where it was.

#### Scenario: Starring a file far down the list
- **WHEN** the user scrolls down the Local Folder page (Folders or List view) and stars a file, by mouse or keyboard
- **THEN** the list does not move and the star of that file keeps the focus
- **AND** a length read finishing later does not move the list either
- **MANUAL** On the laptop: a folder with 30 files, scroll to the bottom, click a star, then Tab to a star and press Space; the view stays put. Focus and scrolling need a real window, and the GTK tests run no widgets.
