# GTK translations

## Why
In a translated Linux app some texts stay English: the "Meditation Complete" notification, the
"Meditating" page title, half a sentence in the vibration editor, the CSV import filter. Units show
"4s" and "5d" in every language (Russian Android already shows "4 с"), and the By-label count picks
between two forms by hand, so Russian and Polish get a wrong form. Twelve texts use the long dash,
which the project rule forbids. OPEN-ISSUES #56, #81, #64, #155, #156.

## What Changes
- Linux: the completion notification (title and "Session: …"), the "Meditating" title, the CSV import
  filter name and the editor's whole sentence "Min 100 ms between points (up to {max} for this
  duration)" go through gettext, in all nine languages.
- Linux: Box Breath phase seconds and the editor's axis seconds use Android's "{}s" msgid.
- Both apps: the best streak uses a plural pair "{n}d", a short abbreviation per language.
- Linux: the By-label row uses `ngettext("{duration} · 1 session", "{duration} · {n} sessions", n)`;
  the two hand-picked msgids go.
- Linux: twelve msgids lose the long dash (list under Decisions); the Russian translations that add one
  (three time-of-day insights, "{duration} on {date}", the store description) are rephrased.
- Left out: the published release notes in metainfo (history; the English originals have the dash
  too); the keyring item label "Meditate sync — user on url" (not translated, only seen in Seahorse);
  decimal commas.

## Decisions
- Streak: short abbreviation per language (Janek, 2026-10-22): en "{n}d", de "{n} T.", es "{n} d",
  fr "{n} j", it "{n} g", nl "{n} d", pl "{n} dn.", pt_BR "{n} d", ru "{n} дн.", zh_CN "{n}天".
- Release notes stay as published (Janek, say-back 2026-10-22).
- The new English wording (my call):
  - "Unstar: remove from home list", "Star: show in home list"
  - "(none, pick one)"
  - "Couldn't save session: storage error", "Couldn't save session: storage unavailable"
  - "Minutes per day, shown as the ring on the Stats tab"
  - "Daily goal: {pct}%, {done} of {goal}"
  - "{date}: goal exceeded, {mins} minutes", "{date}: {mins} minutes", "{date}: no sessions"
  - "1 day, new record" / "{n} days, new record"
  - "Last sync failed, click to retry"
- Running-page title: "Meditating" (de "Meditation läuft") (my call).
- The translations of the new msgids are mine; existing translations of a rephrased msgid are kept
  with the dash replaced the same way (my call).

## Capabilities
### Modified Capabilities
- `translations`: no Linux text is left English or uses the long dash; units and counts translate.

## Impact
- Core: none
- GTK: `timer/imp.rs`, `window/imp.rs`, `vibration_editor.rs`, `preferences.rs`, `stats/imp.rs`,
  `log/imp.rs`, `guided.rs`, `application.rs` (test strings), `i18n.rs` (tests), `po/*.po`, `po/meditate.pot`
- Android: `src/ui.rs` (streak), `ui/main.slint` (one Tr function), `src/app.rs` (test), the nine
  `lang/*/LC_MESSAGES/meditate-android.po`
- Translations: the strings above, both apps where they appear
- Data: none
- Android build environment: untouched
- Docs: OPEN-ISSUES.md (batch P)
