# Design decisions and why

The standing design rules and why each exists. The rules themselves
are listed in `AGENTS.md` (Hard rules); this file keeps the reasons
and the incidents behind them. Where the two disagree, `AGENTS.md`
wins. A new standing rule goes into both: the rule in `AGENTS.md`, its
why here.

## 1. Decisions live in core, mechanisms in the apps

The *decision* (when, whether, which) goes in `meditate-core`; only
the *mechanism* (how, with a platform API) stays in an app.

**Why:** two apps, Linux (GTK) and Android (Slint), share one core. A
decision made in one app's glue has to be written a second time in
the other, and the two copies drift apart.

**How to apply:** before adding a method on `TimerView` in
`meditate-gtk/src/timer/imp.rs`, a branch in a `tick_*` / `on_*`
handler, or the same in `meditate-android/src/ui.rs`, ask: is this a
decision or a mechanism? A decision becomes a `Session` method or an
`Effect` (`meditate-core/src/session/`) or a core function, and the
apps dispatch on it. A mechanism sits next to the native API it wraps
and is triggered by core. Several "push the predicate to core"
commits (e.g. 5a75aec) came from Setup-view callbacks that had drifted
decision logic into glue.

## 2. Translatable text: core returns typed keys, apps render

When a helper produces user-visible text, the core function returns a
typed `*Key` enum, or a struct of such enums; each app maps every
variant to its own translations.

**Why:** the alternative, a translator callback passed into core,
lets `.po` files go out of sync when wording changes. Typed keys make
the decision the thing refactors hold on to; the wording moves freely.

**How to apply:**
- Small decisions: an enum like `IntervalsCountKey { None, One,
  Many(usize) }`; variants carry the data the app puts into its
  translated template.
- Larger helpers: a struct of typed-enum fields, one per
  sub-decision. `PresetSubtitleParts` is the example.
- Core tests assert on the typed value, never on rendered text:
  `assert_eq!(intervals_count_key(0), IntervalsCountKey::None)`.
- There are no translatable strings in `meditate-core/`. GTK renders
  through `gettext()` and `meditate-gtk/po/`, Android through Slint's
  `@tr` and `meditate-android/lang/`.

## 3. Schema and wire-format changes ship a migration

**Why:** the rule used to be the opposite while the maintainer was the
only user: the recovery path was "wipe the server, wipe the local
database, re-import", and compatibility code was dead weight. Since
the app was published (F-Droid, Flatpak bundles on GitHub releases,
2026-08-21) there are users nobody can ask to wipe anything, who have
no export, and who don't read release notes before updating.

**How to apply:** a change that can't read the previous version's
data ships its migration in the same commit. Bump `SCHEMA_VERSION`
(`meditate-core/src/db/schema.rs`, stored as `PRAGMA user_version`)
only then, because older builds refuse a database with a higher
version, and write the upgrade in `Database::open` (there is no step
framework yet). For sync, either read both layouts during a
transition or migrate the server on first write. Test the upgrade
against a database written by the previous release; a fresh
database proves nothing about upgrades.

Additive changes stay the preferred shape and need no migration, but
they have their own steps: a new column goes into `ADDED_COLUMNS`,
and a new event kind needs a `CACHE_SCHEMA_VERSION` bump so events
older builds recorded get applied (see ARCHITECTURE.md,
"Migrations").

## 4. Keep the translation infrastructure

Don't drop gettext, `.po` files or translated error messages during
refactors. The app ships in English plus nine translations (de, es,
fr, it, nl, pl, pt_BR, ru, zh_CN) in both apps.

**Why:** the translators' work lives in those files; dropping the
wiring loses it, and putting it back is far more work than keeping
it.

**How to apply:**
- When logic moves from the GTK app to core, the translated `Display`
  stays in a GTK-side wrapper; core may have a plain English
  `Display` for logs, bridged with `From`.
- Refactors leave `.po` files alone unless user-visible text really
  changes.
- Wording is not stored data (rule 3 doesn't apply), but changing a
  string invalidates it in nine `.po` files per app, and every new or
  changed string needs all nine translations in the same change.

## 5. Guided has no presets

`preset_config::mode_supports_presets` returns false for Guided, and
both apps branch on it.

**Why:** a Guided session is driven by the picked audio file (its
length, its own starred-files library). A preset stores duration,
labels and bells, not a file, so it doesn't fit.

**How to apply:** keep the decision in `mode_supports_presets`. To add
Guided presets later, add a `PresetTiming::Guided` variant; that is a
data-model change, so rule 3 applies.

## 6. Timer time comes from CLOCK_BOOTTIME

`std::time::Instant` uses `CLOCK_MONOTONIC` on Linux and on Android,
and that clock stops during system suspend. All timer code takes the
time from `crate::time::boot_time_now()`, which uses
`CLOCK_BOOTTIME`.

**Why:** a 5-minute suspend during a 10-minute countdown silently
lost 5 minutes (13 s left before suspend, still 13 s after waking;
Librem 5, 2026-04-28). Later the Android app trusted `Instant` on the
strength of a wrong comment, and a ~34-minute screen-off stopwatch
was recorded as ~15 minutes.

**How to apply:** never `Instant::now()` in code that affects session
time, in core or in either app. Diagnostic log lines use the wall
clock plus a `[+Ns since prev log]` marker when two lines are 5 s or
more apart in boot time, which shows both suspend and long idle. A
locked screen is not suspend: the kernel keeps running and both
clocks tick; only deep suspend (lid close, `systemctl suspend`, a
phone's deep sleep) stops `CLOCK_MONOTONIC`.

## Process: every change is tried on a device before it is committed

The rule is `/spec` step 6 (`.claude/skills/spec/SKILL.md`): no commit
before the maintainer has tried the change on the phone or the laptop
and said it works.

**Why:** host tests prove the math, not the feel. Several vibration
changes were committed as "tests pass, deployed" and needed rework
once tried on the phone (cancel racing the pattern, the 10-segment
cap, gaps between chunks). Layout, sound and background behaviour
have the same blind spot, so the rule covers every change, not only
haptics and playback.

## Process: native libadwaita patterns first

On Linux, lead with the libadwaita / GTK4 pattern (`AdwActionRow`,
`AdwExpanderRow`, `AdwPreferencesGroup`, `AdwNavigationView`, ...)
before anything hand-made, even when the hand-made version would be a
smaller diff.

**Why:** the maintainer's stated preference ("please always suggest
native adwaita ways first and other options after that"), and the
app should feel like a GNOME app. See the GNOME HIG
(developer.gnome.org/hig). Hand-made alternatives (a raw
`Gtk.Revealer`, custom CSS animation, manual visibility flips) come
second, and only where they are really better.
