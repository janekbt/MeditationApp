# Binding design decisions

Six standing rules that shape how change lands in this codebase.
They aren't preferences — every one of them was set in response to
an incident (a refactor that drifted, an i18n regression that was
narrowly avoided, a suspend bug that lost timer time). Anything
that contradicts a rule here gets pushed back.

Rules are listed in roughly increasing scope: from "where does
code live" (1–2) to "what does the wire format look like" (3) to
"what's the testing/process discipline" (4–6).

## 1. Decisions live in core, mechanisms in the shell

> When adding NEW behavior, default the *decision* logic to
> `meditate-core`, not `src/timer/imp.rs` or any other gtk shell
> file. Even when the *mechanics* must stay in the shell because
> they're platform-specific, the *trigger / rule / state-machine
> decision* belongs in core.

**Why:** The whole rewrite is about portability for the planned
Slint / Android shell. Every decision that lands in the gtk shell
is one that has to be migrated (or re-implemented) later.

**How to apply:** Before adding a method on `TimerView::imp` or a
branch in `tick_*` / `on_*`, ask: *is this a decision (when /
whether / which) or a mechanism (how)?* If decision, design as a
`Session` method or `Effect` first, gtk dispatch second. If
mechanism, put it next to the native API it wraps and have core
trigger it via an Effect. Concrete heuristic: if the same logic
would need to exist on the Android shell, it belongs in core.

When in doubt, ask before adding to the gtk shell. The four
"Push decision predicates to core" passes shipped recently — each
one came from a Setup-view callback that drifted decision logic
into glue.

## 2. Translatable copy: core returns typed keys, shell renders

> When a helper produces user-visible text, the core function
> returns a typed `*Key` enum or struct of enums; the shell maps
> each variant to its native i18n stack
> (`gettext("None enabled")`, etc.).

**Why:** The translator-callback alternative (`fn(s: &str) -> String`
parameter on every core fn) desyncs `.po` files when copy renames.
Typed keys make the *decision* the refactor anchor; rendering
moves freely.

**How to apply:**

- Trivial 2–3-arm decisions: `pub enum IntervalsCountKey { None,
  One, Many(usize) }`. Variants carry runtime data the shell
  splices into a localized template.
- Complex helpers: a `pub struct` with multiple typed-enum fields
  (one per sub-decision). `PresetSubtitleParts` is the canonical
  example.
- Tests in core assert on the typed value, never on rendered
  strings: `assert_eq!(intervals_count_key(0),
  IntervalsCountKey::None)`.
- `xgettext` and `.po` scanning stay shell-side. There are no
  translatable strings in `meditate-core/`.
- For shells with different i18n stacks (Android `strings.xml`,
  iOS `.strings`, web `Intl`), each defines its own variant-to-
  string mapping. Core doesn't change.

**One-liner heuristic:** if the function previously called
`gettext(...)`, its core counterpart returns a typed key/struct.

## 3. Migrate schema / wire-format changes (REVERSED 2026-08-21)

> When changing schema, wire format, or sync layout, **write the
> migration**. A user's sessions, labels, presets and settings must
> survive every update. Losing them is data loss, not an
> inconvenience.

**Why:** this rule used to say the opposite, and that was correct
while Janek was the only user (laptop + Librem 5): the recovery
path was "wipe remote + wipe local DB + re-import", and compat code
was dead weight. The app is now published — F-Droid inclusion plus
Flatpak bundles on GitHub releases — so there are users we cannot
ask to wipe anything, who have no export, and who will not read a
release note before updating.

**How to apply:** a change that cannot read the previous version's
data needs a migration path shipped in the same commit as the
change. Bump `PRAGMA user_version` and add the upgrade step; for
sync, either read both layouts during a transition or migrate the
remote on first write. Test the upgrade against a DB written by the
previous release, not only against a fresh one — a fresh DB proves
nothing about upgrades.

**Still true:** the schema stays additive where it can. Adding a
column or a new event kind needs no migration and remains the
preferred shape. This rule is about the cases that genuinely break
readability, which now need work rather than a wipe.

## 4. Keep i18n infrastructure

> Don't drop gettext / .po files / localized error messages
> during refactors. Written when Janek was the only user, on the
> bet that it wouldn't stay that way — the app now ships in ten
> languages, so this is no longer a bet.

**Why:** The translation infrastructure is already wired up
(.po files for de/es/fr/it/nl/pl/pt-BR/ru/zh-CN, `gettext-rs`,
build-time PO compilation, `gettext()` calls throughout the gtk
shell). Adding it back later would be a much bigger task than
keeping it in place now. Janek wants translations ready for a
future publish.

**How to apply:**

- gettext calls in the gtk shell stay. When migrating logic out
  of gtk to core, keep the gettext-localized `Display` in the
  gtk-side wrapper. Core can have a simpler English-only
  `Display` for internal use; the gtk wrapper bridges via `From`.
- `.po` files and `po/` are sacred. Don't touch them in refactor
  commits unless the migration genuinely changes user-visible
  strings.
- Dividing line vs rule 3: rule 3 is about stored data, which must
  migrate. Display-string wording is not stored data, but changing
  it now invalidates that string in ten `.po` files — cheap before
  publication, a translation chore since. Permanent infrastructure
  (gettext, a11y) = preserve.

## 5. Guided presets exist in core, hidden from UI

> Guided is a first-class mode in `meditate_core::preset_config`
> the same way Timer and Box Breath are — even though the gtk
> Setup view does NOT surface a preset chooser for Guided.

**Why:** Guided has only ~3 settings, so a preset selector feels
awkward there UX-wise. But future requirements may flip; keeping
core capable means we can surface the UI later without a data-
model refactor or preset migration.

**How to apply:** When changing `PresetConfig` or its walkers,
treat Guided as a first-class mode in the data model. When
changing the gtk Setup view, do NOT add a preset row to the
Guided section.

## 6. CLOCK_BOOTTIME for suspend-resilient timers

> `std::time::Instant` on Linux wraps `clock_gettime(CLOCK_MONOTONIC)`,
> which **does not advance during system suspend**. Use
> `clock_gettime(CLOCK_BOOTTIME)` via `libc` for any timer that
> must survive suspend.

**Why:** A 5-minute suspend during a 10-minute countdown silently
loses 5 minutes. Symptom: timer shows 13s remaining before
suspend, still 13s after wake. Verified on Librem 5 (2026-04-28).

**How to apply:** Every monotonic clock read in core goes through
`crate::time::boot_time_now()`. Don't call `Instant::now()`
directly for session-duration-affecting code. Diag-log timestamps
use wall-clock + a `[+Ns since prev log]` marker when consecutive
calls span a boot-time gap of 5 s+, which catches both suspend
and long idle.

Lock-screen ≠ suspend on Linux: lock keeps the kernel running, so
`CLOCK_MONOTONIC` ticks fine there. Only deep suspend
(`systemctl suspend`, lid close, the Librem 5's suspend mode)
freezes it.

**Android note:** Rust's `Instant` uses `CLOCK_BOOTTIME` natively
since 1.79. When porting to Android, the `libc` shim can be
replaced with plain `Instant::now()` under
`#[cfg(target_os = "android")]`.

## Process: on-device test before commit (haptics / vibration / playback)

> When iterating on haptic, vibration, or playback behaviour:
> build + cross-build + scp + install on the Librem 5, then
> **stop and ask Janek to test**. Don't commit until he reports
> back that the change works on-device.

**Why:** The dev laptop has no haptic motor; laptop tests verify
math but not feel. Several iterations on the vibration pipeline
ended with "tests pass, deployed" commits that needed rework
once Janek tried them on the phone (cancel-races-the-pattern,
10-segment cap, chunk gaps).

**How to apply:**

- After build + deploy on the Librem 5, post the test plan and
  stop. A passing `cargo test` is necessary but not sufficient.
- Defer the commit until Janek confirms the change feels right
  on-device.
- Laptop-only changes (UI math, DB schema, settings persistence)
  are still fine to commit on the same iteration if he's
  confirmed the laptop-side behaviour — but err on the side of
  asking.

## Process: native libadwaita / GTK4 patterns first

> When proposing UI options, lead with the canonical libadwaita /
> GTK4 idiom (`AdwExpanderRow`, `AdwActionRow`,
> `AdwPreferencesGroup`, `AdwNavigationView`, etc.) — only list
> non-canonical alternatives below.

**Why:** Stated as a rule: "please always suggest native adwaita
ways first and other options after that." Cross-reference the
GNOME HIG cheatsheet before listing options.

**How to apply:** Whenever proposing UI / layout / interaction
options, the first option should be the GNOME-HIG / libadwaita-
canonical approach, even if a more surgical hand-rolled
alternative would be a smaller diff. Only mention non-adwaita
escape hatches (raw `Gtk.Revealer`, `Gtk.Stack`, custom CSS
animations, manual visibility flips) if they're actually better
in the specific case — and after the native option.
