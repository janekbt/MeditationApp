# Core migration backlog

All audit-surfaced items have been implemented across waves 1–6 on
the `beta` branch. The two sections below remain as forward-looking
notes — items intentionally not (yet) lifted to core.

## Defer (re-evaluate when Android editor lands)

These were flagged during the vibration-editor audit but only pay
off once the Android pattern editor is actually being written. Until
then they sit here as a heads-up for the eventual port.

- **Duration bounds constants** in `src/vibration_editor.rs:33-34`
  (`DURATION_MIN_S = 0.5`, `DURATION_MAX_S = 10.0`) → consts in
  `meditate_core::vibration` next to the existing editor consts.
- **`intensity_from_drag(start_intensity, drag_dy_px, chart_height_px)
  → f32`** — the drag-y → intensity snap-and-clamp math at
  `src/vibration_editor.rs:381-385`.
- **`pick_handle_index(intensities, chart_rect, click, hit_radius_px)
  → Option<usize>`** — the closest-handle hit-test loop at
  `src/vibration_editor.rs:333-366`, plus the `chart_rect` inset math
  at `:600-606` (and the `Y_LABEL_W`/`PAD`/`X_LABEL_H` consts).
- **`format_seconds(f64) → "{:.1}s"`** — x-axis label formatter at
  `src/vibration_editor.rs:750-752`. One-liner to
  `meditate_core::vibration` (or `format`).

## Skipped (intentionally not migrating)

### `lookup_bell` walker in `src/bells.rs:763-769`
- 6-line `iter → find(|b| b.uuid == uuid)` over a `Vec`. No
  decision logic encoded; Android shell will write its own trivial
  walker against the same `list_interval_bells`. Not worth a helper.

### `lookup_bell_sound_by_uuid` in `src/sound.rs:105-114`
- Same shape as `lookup_bell` above — `if uuid.is_empty() { None }
  else { list_bell_sounds().iter().find(...) }`. Skipped on the
  same rationale.

### Notification-target `is_focused` predicate in `src/timer/imp.rs`
- `!app.active_window().map(|w| w.is_active()).unwrap_or(false)` —
  single-line GTK-bound expression; the decision content is just
  `!is_focused`. Not worth a helper.

### `has_filter` two-field disjunction in `src/log/imp.rs`
- `self.filter_notes_only.get() || self.filter_label_id.get().is_some()`
  — two-Cell boolean OR. No decision content beyond the OR.

### Bundled-vs-custom suffix row affordances in `src/vibrations.rs` (and `src/sounds.rs`)
- `pattern.is_bundled ? [rename] : [edit, delete]`. One-line
  boolean fold; not worth a helper unless multiple shells render the
  same affordance set.

### `accent_color_rgba` unpack
- `src/window/imp.rs` + `src/stats/imp.rs` —
  `adw::StyleManager::default().accent_color_rgba()` + unpack to
  `(f64, f64, f64)`. The `StyleManager` half is gtk-bound; the
  unpack is three field accesses. Not worth a helper.

### Save-as-you-go clamp in bell editor
- `src/bells.rs` — `row.value().round().clamp(MIN as f64, MAX as f64)
  as u32`. The bounds constants live in core already; the round/
  clamp idiom is shell-language-specific.

### Import-form tri-state in `src/sounds.rs`
- `(import_btn_sensitive, collision_label_visible)` two-state
  visibility decision. Deferred — only worth migrating if the
  Android shell renders the same dual-state.

### `format::format_date` fold into `date_group_kind`
- The dialog's `format_date` uses `%b %d, %Y` (zero-padded day);
  `date_group_kind`'s `EarlierYearOther` arm renders `%b %-d, %Y`
  (no padding). Folding would change the visible day format. Not
  worth the visible regression.

### `vibration::PointsSubtitleKey` — duplicated subtitle template
- The "(up to N for this duration)" template is rendered identically
  in two places in `vibration_editor.rs`. The duplication is i18n
  string-template repetition, not decision logic — a typed key
  wouldn't actually carry decisions (always show "up to N for this
  duration"). Pure shell-side dedup if anything; not core work.
