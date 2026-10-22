# Tasks: GTK translations

Base: 51ba476 (beta); suites on it: green (core 1406, gtk 159, android 254)

## Loop
- [x] 0 Skills loaded, again after every /compact: `/ponytail ultra`, `slint:slint` (one Tr function in main.slint)
- [x] 1 Understand
- [x] 2 Propose (approved: 2026-10-22)
- [x] 3 Build
- [x] 4 Review, then `/simplify` after the last review
- [x] 5 Specs, docs, list (check-specs green; labels delta; OPEN-ISSUES batch P ticked)
- [x] 6 Verify and hand over (Linux de+ru and FP5 tested and approved 2026-10-22)
- [x] 7 Ship (specs merged, archived)

## Review Focus
- A language whose plural rule has three forms (ru, pl) for the streak and By-label row: both are ngettext, covered by `plural_forms_for_several_counts_show_the_count` (every form with {n}).
- Box Breath tiles at the Russian "4 с" width: the tile already fits "4 с"-width text on Android; MANUAL "Seen in German and Russian".
- The streak at 360 px with 3 digits and "dn.": Android MANUAL "Seen on the phone in German".
- Release notes keep their dash: the dash test skips entries referenced only from the metainfo `<releases>` block (line found from the file, not hard-coded).
- Fuzzy or obsolete entries: `po_entries` already skips them.

## 1. Tests first (GTK)
- [x] 1.1 `every_language_translates_every_text`: every template msgid has a non-empty msgstr (every form) in all nine .po. Expected: PASS on base (mutation: blank one msgstr, FAIL, restore); FAIL after 3.1 adds new msgids, PASS after translations.
- [x] 1.2 `no_text_uses_the_long_dash`: template msgids and live msgstrs of all nine .po, release-note entries skipped. Expected: FAIL then PASS.
- [x] 1.3 `visible_text_goes_through_translation` (GTK): the literals `Notification::new("Meditation Complete"`, `"Session: {}"`, `("Meditating"`, `Some("CSV files")`, `(up to {} for this duration)`, `"{val}s"`, `"{secs:.1}s"`, `"{streak}d"`, `"{duration} · 1 session"` are gone from src. Expected: FAIL then PASS.

## 2. Android test first
- [x] 2.1 `visible_text_goes_through_translation`: `format!("{streak}d")` gone from src/ui.rs. Expected: FAIL then PASS.

## 3. Code and catalogs
- [x] 3.1 GTK: wrap the texts (#56, #81), "{}s" (#155), ngettext streak and By-label row (#155, #156, fix the comment naming `pluralize_sessions`), the twelve rephrased msgids (#64) incl. `application.rs` test strings; `build-aux/update-translations.sh`; fill all nine .po; fix the five Russian msgstrs; `msgfmt --check`. Proof: 1.1 to 1.3, `cargo test -p meditate i18n::`, gtk suite.
- [x] 3.2 Android: Tr `streak(n)` = `@tr("{n}d" | "{n}d" % n)`, used in `refresh_stats`; nine .po entries. Proof: 2.1, android suite, `msgfmt --check`.

## Notes
- Pre-build questions (fresh agent, 2026-10-22): none. Its notes, my call: the dash test reads msgid/msgstr fields only (the translator comment at bells.rs:591 carries a dash); in the By-label row `.replace("{n}", …)` comes first; the editor's axis keeps `format!("{secs:.1}")` inside "{}s".
- 1.1 passed on base as planned; its failure came naturally after update-translations (new and fuzzy entries), then PASS once filled.
- 1.2 failed on base with 13 template entries (12 msgids, one plural) plus the translations; release notes skipped as intended.
- 1.3 literals reworded while building: `with_content("Meditating"` and `gettext("{duration} · 1 session")` (the quoted literals alone also matched the new gettext/ngettext calls). Both matched the base code.
- Android streak count goes through `i32::try_from(streak)` (my call; the Tr function takes an int).
- German translations of the rephrased msgids used an en dash "–"; they now follow the English colon or comma too (my call).
- Suites after build: gtk 162 green, android 254 green; clippy gtk+core clean; Android-target clippy clean.
- Review (fresh agent, 2026-10-22), Spec PASS, Quality PASS:
  - Finding 1 (labels spec still quotes "(none — pick one)"): fixed with a MODIFIED delta for `labels` (wording only).
  - Finding 2 (literal-ban tests miss a re-written `format!("{x}d")`): rejected. A ban on any `}s"`/`}d"` format string also hits legit text (`gettext("{}s")`, the sync log line "retrying in {backoff_secs}s" in ui.rs); the literal list matches the repo's existing pattern.
- /simplify (4 agents, 2026-10-22): applied `format::secs_text` and `format::days_text` beside the h/m unit helpers (one home for "{}s" and "{n}d"); i18n tests read the template once, build file paths directly, collect refs once. Skipped: per-frame `gettext("{}s")` in the editor chart (a catalog hash lookup per point, not measurable); the mini streak's zero check through core's `mini_stat_value` (two agents): the `streak == 0` branch predates this change in both apps; left for batch S cleanup.

## Hand-over (2026-10-22)
- Suites: gtk 162 green, android 254 green, core untouched (1406 on base); clippy gtk+core clean, Android-target clippy clean; check-specs green.
- Artifacts: `meditate-gtk/builddir/src/meditate` (11:37), `meditate-android/android/app/build/outputs/apk/debug/app-debug.apk` (11:39).
- Start Linux: `XDG_DATA_HOME=/home/janek/.cache/meditate-datetest LOCALEDIR=meditate-gtk/builddir/po LANGUAGE=de ./meditate-gtk/builddir/src/meditate` (LANGUAGE=ru for test 6).
- Test steps:
  1. German, Timer: start a 1-minute countdown. Title "Meditation läuft". Click another window and let it end: notification "Meditation beendet", "Sitzung: 01:00".
  2. German, Box Breath: phase tiles show "4 s".
  3. German, a vibration pattern's editor: points subtitle wholly German "Mind. 100 ms zwischen Punkten (bis zu N bei dieser Dauer)"; axis labels "1.0 s".
  4. German, Stats: best streak "1 T." (or N T.); Preferences, Import: file filter "CSV-Dateien".
  5. German, texts without the long dash: daily-goal subtitle in Preferences "Minuten pro Tag, als Ring im Statistik-Tab".
  6. Russian, Stats with a label of 3 sessions: By-label row "… · 3 сессии"; Box Breath tiles "4 с".
  7. Phone in German, Stats: best streak "N T.", fits.
- Proposed commit message:
  gtk: translate the last English texts and units (batch P)

  The completion notification, the running page title, the CSV import
  filter and the vibration editor's points sentence now go through
  gettext. Seconds reuse Android's "{}s", the best streak is a "{n}d"
  plural in both apps, and the By-label count uses ngettext, so Russian
  and Polish get their own forms. Twelve texts lose the long dash; three
  i18n tests keep every catalog complete and dash-free.
