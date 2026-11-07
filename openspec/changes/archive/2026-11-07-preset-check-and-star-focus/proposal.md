# Preset check and star focus

## Why
Two small faults found while testing setup-lists-order (OPEN-ISSUES batch BB). #211: a preset saved with the label switched off never gets its check or highlight once a label was chosen at some point, although it applies correctly. #212: on Linux, starring a file on the Local Folder page throws the list back to the top. #213 (the green sync check opens no status) is already fixed on beta (66b188d, after v26.11.0); it leaves the list.

## What Changes
- A switched-off label counts as "off" for the check, whatever label sits behind it (core, so both apps).
- Linux Local Folder page: after a star (or any rebuild of the page), the focus goes back to the same file's star and the list stays where it was.
- #213 leaves OPEN-ISSUES; no code.
- Left out: a preset with the label on but no label stored (it keeps whatever label is set when applied); not reported, no change.

## Decisions
- #211 fixed in core only; both apps read the same check (my call).
- #212 Linux only: the Android list is not rebuilt on a star (my call).
- #212 keeps both the focus (on the row or its star, whichever had it) and the scroll value, so a background length read does not pull the list either (my call, from the pre-build questions).

## Capabilities
### Modified Capabilities
- `presets`: the check ignores the label behind a switched-off label switch
- `guided`: starring on the Local Folder page keeps the place

## Impact
- Core: `preset_config.rs` (`active_presets`)
- GTK: `local_folder.rs` (`fill_page`, rows named by file id)
- Android: none
- Translations: none
- Data: none
- Android build environment: untouched
- Docs: OPEN-ISSUES.md (#211, #212, #213 out, batch BB ticked)
