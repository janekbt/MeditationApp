# Meditate — Implementation Plan

**App ID:** `io.github.janekbt.Meditate`
**GitHub:** https://github.com/janekbt/MeditationApp
**Stack:** Rust + GTK4 + libadwaita + Blueprint + Meson + Flatpak
**Storage:** SQLite via `rusqlite`

---

## App Overview

A simple, adaptive meditation app for GNOME (Linux phone + desktop). Three views:
- **Timer** — countdown or stopwatch, labels, note, streak display
- **Stats** — monthly calendar, bar chart, text stats
- **Log** — browsable session history, add/edit/delete, filters

Navigation: `AdwViewSwitcher` in header bar (desktop) + `AdwViewSwitcherBar` at bottom (mobile/narrow). Navigation is locked while the timer is running — only available again when paused or stopped.

---

## HIG Notes (key constraints to keep in mind)

- Timer display: `large-title` CSS class. Stat numbers: `title-1`/`title-2`. No hardcoded font sizes.
- Tooltips required on every header bar button (HIG hard requirement).
- Custom-drawn widgets (calendar dots, bar chart bars) must use `@accent_color` — never hardcoded hex — for dark/high-contrast mode compatibility.
- Empty Log state needs an `AdwStatusPage` placeholder with heading, description, and suggested "Add Session" button.
- Adaptive minimum: 360×294px (phone). Every view must work at this width.
- App icon: 128×128px full-color SVG + symbolic SVG variant.
- Primary menu (⋮) must include: Preferences, Keyboard Shortcuts, About.
- Buttons outside header bars: either icon OR label, not both.
- Only one suggested/destructive button per view.
- Prefer `AdwToast` + undo over confirmation dialogs for deletions.
- Cancel always left in dialogs; Esc dismisses.
- Capitalization: Header caps for buttons/menus/tooltips; sentence caps for labels/checkboxes/descriptions.

---

## Phase 1 — Project Scaffold

- [x] **1.1** Create Meson project skeleton (`meson.build`, `src/`, `data/`, `build-aux/`)
- [x] **1.2** Set up `Cargo.toml` with all dependencies (`gtk4`, `libadwaita`, `rusqlite`, `glib`, `gio`)
- [x] **1.3** Minimal `main.rs` + `AdwApplicationWindow` that compiles and opens a blank window
- [x] **1.4** `AdwViewStack` + `AdwViewSwitcher`/`AdwViewSwitcherBar` with three empty placeholder views + tooltips on all header buttons
- [x] **1.5** Flatpak manifest (`io.github.janekbt.Meditate.json`) + `.desktop` file + AppStream metainfo

## Phase 2 — Data Layer

- [x] **2.1** SQLite schema: `sessions` (id, start_time, duration_secs, mode, label_id, note) + `labels` (id, name)
- [x] **2.2** `db` module: connection init, schema migrations, full CRUD for sessions and labels
- [x] **2.3** Thread-safe DB wrapper accessible across the app (via `gio::Application` data or a `RefCell` singleton)

## Phase 3 — Timer View

- [x] **3.1** Blueprint: mode toggle (Countdown/Stopwatch segmented button), `AdwSpinRow` H:M, quick-preset pill buttons (5/10/15/20/30 min)
- [x] **3.2** Streak display: `title-4` label with streak count shown above the time display
- [x] **3.3** Timer state machine (`Idle → Running → Paused → Stopped`) driven by `glib::timeout_add_local`
- [x] **3.4** Large time display using `large-title` CSS class; hide/disable duration inputs while running
- [x] **3.5** Navigation lock: push a full-screen `AdwNavigationView` page on Start; pop on Pause (tab bar becomes visible again)
- [x] **3.6** Post-stop panel: `AdwEntryRow` for note + `AdwComboRow` for label + Save (suggested) / Discard (destructive)
- [x] **3.7** Wire Save → DB insert; Discard → `AdwAlertDialog` confirmation if a note was typed

## Phase 4 — Log View

- [x] **4.1** Blueprint: `AdwListBox` in `GtkScrolledWindow`; + button in header bar with tooltip
- [x] **4.2** Log row widget: duration · date · label chip · note preview (truncated)
- [x] **4.3** Empty-state `AdwStatusPage`: illustration icon, "No Sessions Yet" heading, description, suggested "Add Session" button
- [x] **4.4** Swipe-to-delete + `AdwToast` with Undo (5-second window before actual DB delete)
- [x] **4.5** Edit session dialog (`AdwDialog`): rows for date, duration, label, note — all editable
- [x] **4.6** Add-manually dialog: same form as edit, blank
- [x] **4.7** Filter popover (filter button in header): `AdwSwitchRow` "Only with notes" + `AdwComboRow` "Label"

## Phase 5 — Stats View

- [x] **5.1** Monthly calendar widget (`GtkGrid`, 7×6): colored dot per day with at least one session; uses `@accent_color`; month nav < Month Year >
- [x] **5.2** Month browsing: previous/next navigation with DB query per month
- [x] **5.3** Bar chart widget (`GtkDrawingArea` + Cairo): bars use `@accent_color`; Y-axis = total duration; dark mode safe
- [x] **5.4** Daily/Weekly/Monthly/Yearly toggle above chart; chart reloads on change
- [x] **5.5** Text stats row: running average · longest streak · total time formatted as `Xh Ym`

## Phase 6 — Preferences

- [x] **6.1** `AdwPreferencesDialog` skeleton: two pages — General and Labels
- [x] **6.2** General page: `AdwComboRow` for bundled sounds (Singing Bowl, Bell, Gong) + `AdwActionRow` "Choose custom file…"
- [x] **6.3** Sound preview: play button next to picker; `gtk::MediaFile` / GStreamer for all sounds
- [x] **6.4** Running average period: `AdwComboRow` with 7 / 14 / 30 days options; persisted in `settings` DB table
- [x] **6.5** Labels page: `AdwEntryRow` per label with inline rename (manual apply/discard buttons); `AdwToast` undo on delete; new labels appear at top

## Phase 7 — Polish & Accessibility

- [x] **7.1** System notification (`GNotification`) when timer ends — only fires when app is in background
- [x] **7.2** Keyboard shortcuts: Space = start/pause, Ctrl+, = Preferences, Ctrl+? = Shortcuts, Ctrl+W = close, Ctrl+Q = quit
- [x] **7.3** `GtkShortcutsWindow` dialog (triggered by Ctrl+? and menu item)
- [x] **7.4** Adaptive layout audit: Clamp + Breakpoint + ScrolledWindow in all views; default width 360px
- [x] **7.5** Dark mode + high-contrast audit: calendar and chart use `@accent_bg_color`/`@accent_fg_color`; no hardcoded colors
- [x] **7.6** Accessibility pass: tooltip-text on all icon/number-only buttons; chart and calendar use standard GTK widgets (screen-reader readable); keyboard nav via Space/Ctrl shortcuts

## Phase 8 — Flatpak & App Identity

- [ ] **8.1** Design app icon: 128×128px SVG (full-color, GNOME geometric style) + symbolic SVG variant
- [x] **8.2** Generate Flatpak Cargo sources JSON via `flatpak-cargo-generator.py`
- [x] **8.3** GStreamer: GNOME Platform runtime includes gst-plugins-base/good (covers WAV/OGG/FLAC/Opus); no extra module needed. MP3 not supported (gst-plugins-ugly absent from runtime).
- [ ] **8.4** Full `flatpak-builder` test build; verify file-chooser portal works in sandbox
- [x] **8.5** Icon SVG placeholders + hicolor install in meson.build; `<icon type="stock">` in metainfo; manifest finish-args cleaned up

---

## Task Summary

| Phase | Tasks | Description |
|-------|-------|-------------|
| 1 | 1.1–1.5 | Project scaffold |
| 2 | 2.1–2.3 | Data layer (SQLite) |
| 3 | 3.1–3.7 | Timer view |
| 4 | 4.1–4.7 | Log view |
| 5 | 5.1–5.5 | Stats view |
| 6 | 6.1–6.5 | Preferences |
| 7 | 7.1–7.6 | Polish & accessibility |
| 8 | 8.1–8.5 | Flatpak & app identity |
| **Total** | **36** | |

Phases 3, 4, and 5 can be developed in parallel once Phase 2 is complete.
