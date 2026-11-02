# Tasks: Nextcloud URL help

Base: ecda2bbe0a24485069205746bada108a37cd688a; suites on it: core 1438
passed, android --lib 285 passed, gtk 167 passed

## Loop
- [x] 0 Skills loaded, again after every /compact: `/ponytail ultra`,
      `slint:slint`
- [x] 1 Understand
- [x] 2 Propose (approved: 2026-11-02, with: look up Nextcloud's menu names)
- [x] 3 Build
- [x] 4 Review (one fresh agent, 1 finding fixed; /simplify below)
- [x] 5 Specs, docs, list (check-specs passes; no doc names the label; #18 is a GitHub issue, not in OPEN-ISSUES)
- [x] 6 Verify and hand over (Janek: laptop and FP5 passed, 2026-11-02)
- [x] 7 Ship

## Review Focus
- An account saved before the change -> its address shows in the
  renamed field and syncs as before; nothing about stored data changes.
- Another language -> the sentence is longest in German and Russian;
  the Android scenario checks German at phone width, GTK wraps the
  group description itself.
- The arrow on Android -> ">" instead, the phone's font lacks "→"
  (AGENTS.md rule).
- A WebDAV server that isn't Nextcloud -> the group is already titled
  Nextcloud Sync; the sentence only adds where Nextcloud shows things.

## Pre-build questions (fresh agent), answered (my calls)
- GTK joins two catalog texts -> each is a whole sentence, joined by a
  space; the house rule bans sentences glued from pieces, not two
  sentences side by side.
- Android test shape -> its own test; the sentence stays on one line in
  every catalog so the `msgid "…"\nmsgstr "` lookup finds it.
- Old "Server URL" -> Android catalogs rename the entry in place;
  Linux marks it obsolete through `update-translations.sh`.
- Other texts saying "URL" ("Enter URL and username", "URL must start
  with https://", ...) -> stay: plain "URL" still names that field.
- Android form -> exactly "Files > Files settings" and
  "Personal settings > Security"; no long dash in any language.

## Menu names, from Nextcloud's own catalogs (Janek: look them up)
Source: github.com/nextcloud/server master, `apps/files/l10n/<lang>.json`
("Files settings", "WebDAV URL"), `lib/l10n` ("Personal settings"),
`apps/settings/l10n` ("Security"), fetched 2026-11-02.

| lang | Files settings | Personal settings | Security | WebDAV URL |
|---|---|---|---|---|
| de | Dateien-Einstellungen | Persönliche Einstellungen | Sicherheit | WebDAV-URL |
| es | Configuración de archivos | Ajustes personales | Seguridad | URL de WebDAV |
| fr | Paramètres de Fichiers | Paramètres personnels | Sécurité | URL WebDAV |
| it | Impostazioni File | Impostazioni personali | Sicurezza | URL WebDAV |
| nl | Instellingen voor bestanden | Persoonlijke instellingen | Beveiliging | WebDAV URL |
| pl | Ustawienia Plików | Ustawienia osobiste | Bezpieczeństwo | URL WebDAV |
| pt_BR | Configurações de arquivos | Configurações pessoais | Segurança | URL WebDAV |
| ru | Параметры «Файлы» | Личные параметры | Безопасность | Ссылка WebDAV |
| zh_CN | 文件设置 | 个人设置 | 安全 | WebDAV URL |

"Files" (the app): Dateien, Archivos, Fichiers, File, Bestanden, Pliki,
Arquivos, Файлы, 文件.

## Review findings
- 1 (Android test only read the catalogs, a changed `@tr` text in
  main.slint would show English and stay green) -> fixed: the test also
  asserts both `@tr("...")` texts in main.slint; seen failing with the
  label changed, restored; android 286, core 1438.

## /simplify (4 agents): nothing applied
- Merge the new test into `ui_text_is_translated_whole` (reuse) ->
  skipped: that test is about glued sentences (#27/#28/#31); the house
  pattern is one test per issue, and the spec names this one.
- Two Slint `Text` blocks with the same four properties -> skipped: one
  `Text` would need two `@tr` glued together, which the house rule bans.
- Shared constant for the nine languages -> skipped: every neighbouring
  test lists them inline; a refactor outside this change.
- One general Android test over every `@tr` msgid (altitude) ->
  skipped here: a new check over all existing texts is its own change;
  offered to Janek as an OPEN-ISSUES item.
- GTK has no msgid check (altitude) -> false: `every_language_translates_every_text`
  reads every template msgid.
- Efficiency: none.

## Tasks
- [x] 1 Android test `nextcloud_help_is_translated` (the two msgids
      exist and are translated in all nine catalogs), watch it fail (failed: de had no entry)
- [x] 2 Android: rename the label, add the help `Text` under the
      description, nine translations; test passes (android 286, core 1438)
- [x] 3 GTK: rename the label, append the sentence to the group
      description, `update-translations.sh`, watch
      `every_language_translates_every_text` fail, nine translations,
      it passes; `msgfmt --check` on every touched catalog (failed on
      the empty entries, then gtk 167 passed; the script also rewrapped
      long release-note entries, content unchanged; clippy clean both)
- [x] 4 Builds for the two MANUAL scenarios (GTK debug 15:55, debug APK 15:56)

## Test steps (step 6)
Artifacts: `meditate-gtk/builddir/src/meditate`,
`meditate-android/android/app/build/outputs/apk/debug/app-debug.apk`.
1. Laptop, German: Preferences, Nextcloud Sync. The description ends
   with "Nimm die WebDAV-Adresse aus Dateien → Dateien-Einstellungen und
   ein App-Passwort aus Persönliche Einstellungen → Sicherheit.", the
   first field reads "WebDAV-URL", the saved address is still in it.
2. FP5, German: Preferences. The same sentence with ">" shows in full
   under the description, nothing cut off, no boxes; the field reads
   "WebDAV-URL"; a sync still works.

## Commit message
sync: Say which address and password Nextcloud sync needs

The field asked for a "Server URL", but the app needs the WebDAV
address and an app password. Fixes #18.
