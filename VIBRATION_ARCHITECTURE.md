# Vibration architecture

Design notes for the vibration-pattern feature. Working document — decisions
recorded as we make them, open questions surfaced inline. Implementation has
not started yet; throwaway prototypes for the bell-row UI live at the bottom
of `data/ui/timer_view.blp` (search for "Vibration UI prototype") and
`src/timer/imp.rs::setup_vibration_proto`.

## Goals

- Per-bell vibration patterns alongside per-bell sounds.
- Per-phase vibration in Box Breath (accessibility — visually-impaired users
  can feel where they are in the cycle by distinct patterns per phase).
- A custom-pattern editor so users can author their own envelopes.
- Per-mode "what plays" toggle (Sound / Vibration / Both) — scoped to the
  mode it sits in, **not** app-wide.
- Capability-gated: greyed/hidden where the device has no haptic motor.

## Non-goals

- Backwards compatibility with the existing single-boolean
  `vibrate_on_end` setting. Per project rule (`feedback_meditate_no_compat`),
  delete it cleanly when the new system lands.
- Haptic feedback on laptop. Pattern *design* works on laptop; *playback* is
  a Librem-only path.

---

## Bell-row UI

Three different bell types, three slightly different shells, but the same
**Sound / Vibration / Both** selector pattern slots into all of them. The
selector itself is an `Adw.ToggleGroup` with `.round`, three `Adw.Toggle`
children. Conditional rows (Bell Sound, Pattern) below it are wrapped in
`Gtk.Revealer`s with `slide_down` transition for animated reveal.

### End Bell — confirmed

```
End Bell  [enable-switch on outer ExpanderRow]
  ├── Type: [ Sound | Vibration | Both ]    ← AdwToggleGroup .round
  ├── Bell Sound: <name>  ›                  ← revealed when Sound or Both
  └── Pattern: <name>     ›                  ← revealed when Vibration or Both
```

Default toggle state: **Sound** (matches today's behavior — Bell Sound is
the only thing the End Bell does).

### Starting Bell — confirmed

Same shape as End Bell, plus the existing Preparation Time peer expander
sits below the Bell Sound / Pattern rows. Preparation Time is orthogonal to
*what fires* — it controls *when* — so it's a sibling, not part of the
Type-gated reveal block.

```
Starting Bell  [enable-switch]
  ├── Type: [ Sound | Vibration | Both ]
  ├── Bell Sound: <name>  ›                  (revealed conditionally)
  ├── Pattern: <name>     ›                  (revealed conditionally)
  └── Preparation Time  [enable-switch]      (existing, untouched)
        └── Duration: <secs>
```

### Interval Bell edit page — confirmed (no prototype needed; same pattern)

The interval-bell edit page in `src/bells.rs::push_edit_page` is a separate
sub-page reachable via Manage Bells → tap a row. The same toggle goes inline
in the existing form group:

```
Edit Bell  [PreferencesGroup]
  ├── Kind: <Every N min | At time | Before end>  ⌄
  ├── Minutes: <n>
  ├── Jitter: <%>                              (visibility-gated on Kind)
  ├── Type: [ Sound | Vibration | Both ]       ← new
  ├── Bell Sound: <name>  ›                     ← revealed conditionally
  └── Pattern: <name>     ›                     ← revealed conditionally
```

The list page (Manage Bells) keeps its current per-row enable-switch + trash
button. The Type toggle and Pattern row only live on the edit page.

The list page row's subtitle shows the sound name today; once vibration is
configured per bell, it could read e.g. "Tibetan Bowl · Heartbeat" — minor
follow-up, not required.

### Box Breath — confirmed

No "bell" concept here. End chime exists (single shared end bell,
configured in the Session group like the others). Phase cues are net-new
and now carry **both sound and vibration** (per the data-model decision
to add `sound_uuid` to phases for the voice-cue use case — see
"Box Breath per-phase storage" further down). Each phase row uses the
**same Sound / Vibration / Both ToggleGroup** the bells use, with the
same conditional Bell-Sound and Pattern rows beneath it.

```
Phase Cues  [PreferencesGroup]
  └── Cues during phases  [show-enable-switch]      ← outer ExpanderRow / master
        ├── Inhale       [show-enable-switch]
        │     ├── Type: [Sound | Vibration | Both]
        │     ├── Bell Sound: <name>  ›             (revealed conditionally)
        │     └── Pattern: <name>     ›             (revealed conditionally)
        ├── Hold (in)    [show-enable-switch]
        │     └── …same shape…
        ├── Exhale       [show-enable-switch]
        │     └── …same shape…
        └── Hold (out)   [show-enable-switch]
              └── …same shape…
```

Master is the outer expander itself — flipping it reveals/hides the four
phase rows with the native expander animation (no extra GtkRevealer
wrapping). Three-level nesting; same chevron CSS override the existing
Starting Bell already relies on covers it.

The Bell Sound chooser opened from a phase row is **filtered to
`category = 'box_breath'`** (see "Bell-sound categories" below) so the
user sees voice cues / phase markers, not bells.

Defaults: master off, all four phase switches off, each phase
`signal_mode='sound'`, Pulse pattern, Bowl sound.

Phase order: Inhale → Hold (in) → Exhale → Hold (out). Match whatever
labels the existing `phase_tiles_grid` already uses.

#### Why not the header-suffix master switch

We prototyped putting the master switch in the `Adw.PreferencesGroup`'s
`header-suffix` slot. Visually possible, but:
- The slot is conventionally for inline action links, not toggles — users
  don't look there for on/off.
- It also greyed out the four rows below rather than hiding them, which
  clutters the page.

The outer-expander-as-master approach beats it on both counts: HIG-aligned
and cleanly hides content the user isn't using.

### Animation: row reveals

For the Sound / Vibration / Both toggle on Start and End bells, the Sound
row and Pattern row are each wrapped in a `Gtk.Revealer` with
`transition-type: slide_down` and `transition-duration: 220` ms. Toggling
between modes slides rows in/out instead of popping.

Caveat noted: a `Gtk.Revealer` sits in the listbox where an `Adw.ActionRow`
would, so the row's hover/separator chrome renders slightly differently
than a plain ActionRow would. In the current prototype it looks fine; flag
for a closer look during real implementation.

### Reusable wiring helper

The Sound/Vibration/Both toggle plus revealer wiring lives in one helper —
copy this verbatim when implementing the real thing:

```rust
fn wire_signal_toggle(
    host: &gtk::Box,
    sound_revealer: &gtk::Revealer,
    pattern_revealer: &gtk::Revealer,
) {
    let toggle_group = adw::ToggleGroup::builder()
        .css_classes(["round"])
        .valign(gtk::Align::Center)
        .build();

    let sound = adw::Toggle::builder()
        .name("sound")
        .label(crate::i18n::gettext("Sound"))
        .build();
    let vibration = adw::Toggle::builder()
        .name("vibration")
        .label(crate::i18n::gettext("Vibration"))
        .build();
    let both = adw::Toggle::builder()
        .name("both")
        .label(crate::i18n::gettext("Both"))
        .build();
    toggle_group.add(sound);
    toggle_group.add(vibration);
    toggle_group.add(both);
    toggle_group.set_active_name(Some("sound"));

    host.append(&toggle_group);

    sound_revealer.set_reveal_child(true);
    pattern_revealer.set_reveal_child(false);

    let sound_revealer = sound_revealer.clone();
    let pattern_revealer = pattern_revealer.clone();
    toggle_group.connect_active_name_notify(move |tg| {
        let active = tg.active_name();
        let show_sound = matches!(
            active.as_deref(),
            Some("sound") | Some("both")
        );
        let show_pattern = matches!(
            active.as_deref(),
            Some("vibration") | Some("both")
        );
        sound_revealer.set_reveal_child(show_sound);
        pattern_revealer.set_reveal_child(show_pattern);
    });
}
```

Notes on `Adw.ToggleGroup` (libadwaita 1.7+):
- `.add()` takes `Toggle` by value, not by reference.
- `.set_active_name(Some("..."))` to programmatically select.
- `.connect_active_name_notify` fires on change; read the active name from
  the closure's `tg.active_name()`.
- Active-toggle background defaults to `@active_toggle_bg_color` (neutral
  white-ish in light, dark-grey in dark). HIG-correct — accent is reserved
  for `.suggested-action` and `AdwViewSwitcher`'s selected tab. Don't
  override unless we want to dilute the "this is THE action" signal.
- Practical segment count: comfortable up to **3** on a Librem 5 portrait
  (~280 px after margins), tight at **4**, breaks at **5+**. Same as HIG
  guidance for `AdwViewSwitcher`.

### Reusable BLP snippet — Sound/Vibration/Both block

Place inside an `Adw.ExpanderRow` (the bell's outer enable-switch row):

```blueprint
Adw.ActionRow {
  title: _("Type");

  [suffix]
  Gtk.Box <toggle_host> {
    orientation: horizontal;
    valign: center;
  }
}

Gtk.Revealer <sound_revealer> {
  transition-type: slide_down;
  transition-duration: 220;
  reveal-child: true;

  Adw.ActionRow <sound_row> {
    title: _("Bell Sound");
    subtitle: _("<name>");
    activatable: true;

    [suffix]
    Gtk.Image {
      icon-name: "go-next-symbolic";
      styles ["dim-label"]
    }
  }
}

Gtk.Revealer <pattern_revealer> {
  transition-type: slide_down;
  transition-duration: 220;
  reveal-child: false;

  Adw.ActionRow <pattern_row> {
    title: _("Pattern");
    subtitle: _("<name>");
    activatable: true;

    [suffix]
    Gtk.Image {
      icon-name: "go-next-symbolic";
      styles ["dim-label"]
    }
  }
}
```

---

## Per-mode "what plays" toggle — scoped, not app-wide

The toggle is per-mode (Timer / Guided / Box Breath); each mode's setup
view gets its own. Acts as a runtime override on top of per-bell and
per-phase intent — see "Per-mode 'what plays' toggle storage" further
down for the storage shape, semantics, defaults, capability gating, and
UI placement.

For Box Breath specifically, the master `boxbreath_cues_active` already
gates per-phase cues entirely; the per-mode toggle layers on top of that
to silence one channel (sound or vibration) across both the phase cues
*and* the End Bell.

---

## Vibration pattern editor

Mockup at `/home/janek/Downloads/vibration_pattern_editor_mockup.html`.
Working prototype at `src/vibration_proto.rs` (functional enough to feel
the page; no DB write, no playback).

### Page layout — confirmed

Own `Adw.NavigationPage` pushed from the chooser, with `Adw.HeaderBar`
carrying Cancel / Save. Body is a vertical `Gtk.Box` of `Adw.Clamp`s (NOT
a single `Adw.PreferencesPage` — we needed flexibility to drop a
`Gtk.DrawingArea` and a non-row banner alongside `PreferencesGroup`s):

```
[ HeaderBar: Cancel    Pattern editor    Save ]

  ┌─[card] Name ─────────────────────┐         Adw.EntryRow (no popup)
  │  <name>                          │
  └──────────────────────────────────┘

  ┌─[card] Shape ────────────────────┐
  │  Duration   <2.0 s>           ⌃⌄ │         Adw.SpinRow, 0.5–10.0 s
  │  Points     <7>               ⌃⌄ │         Adw.SpinRow, 3–24
  └──────────────────────────────────┘

  ┌─[card] ──────────────────────────┐
  │  Pattern                          │        bold heading
  │  Line: Continuous transitions     │        static 2-line subtitle
  │  Bar: Abrupt transitions  [Line|Bar]
  │                                   │
  │  [chart canvas]                   │
  └──────────────────────────────────┘

  [ Preview ]                                  pill, .suggested-action

  prototype banner (placeholder for the laptop / no-haptic banner)
```

Decisions baked in:
- **Name as `Adw.EntryRow`** — inline edit, no popup; matches how guided
  files / labels rename.
- **No per-point Duration field** — points are equally-spaced; spacing is
  `Duration / (Points - 1)`, implicit from the two SpinRows.
- **No tap-to-add on canvas** — point count is changed via the Points
  SpinRow, which resamples the curve linearly. Trade-off: can't place a
  point at an arbitrary time; gain: clean equal spacing.
- **Selected-point Intensity slider — DROPPED.** Drag handles directly on
  the chart is the only intensity input. Saves a row, page reads shorter.
- **Preview button at the bottom**, not floating in the chart card.

### Chart canvas — confirmed

- **`Gtk.DrawingArea`** with Cairo.
- **Y axis**: `0%` / `50%` / `100%` labels, right-aligned in a 38 px
  gutter. Faint horizontal gridlines at each level. **No rotated
  "Intensity" title** — implicit from the heading and labels.
- **X axis**: actual seconds at each control point, formatted with one
  decimal place: `0.0s`, `0.3s`, `0.7s`, ... `2.0s`. Updates live as
  Duration / Points changes.
- **Line / Bar `Adw.ToggleGroup` `.round`** at the top-right of the
  chart card with two segments. Default Line.
  - **Line**: filled area under the polyline (accent at 22% opacity) +
    polyline stroke (accent solid, 2 px, round joins).
  - **Bar**: filled rectangles, one per control point, centered on each
    handle's x-position. Adjacent bars touch (each is `step / 2` wide on
    either side; first/last bar clamps to the chart edge). Reads as a
    sample-and-hold envelope. Accent at 55% opacity (denser than line
    fill since bars are discrete).
  - The two modes share the same N intensity values; switching is purely
    a render flip.
- **Handles**: dot per control point, sized 6 px (8 px when selected).
  White outer ring (1.5 px) for separation from the filled background.
  Selected handle gets a halo (accent at 30% opacity, 4 px wider).
- **Drag**: `Gtk.GestureDrag` on the canvas. Drag-begin finds the closest
  handle within a 28 px hit radius and selects it. Drag-update maps drag
  Y to an intensity delta (`-oy / chart_height`), snaps to 5%, clamps to
  [0, 1]. Drag-only — no precise slider beneath.
- **Resampling on Points change**: linear interpolation between adjacent
  old samples onto the new equally-spaced grid. Selected index is
  cleared if it falls out of bounds.

### Code reference — chart drawing

```rust
match editor.chart_kind.get() {
    ChartKind::Line => {
        // Filled area under the polyline.
        cr.set_source_rgba(ar, ag, ab, 0.22);
        cr.move_to(xs[0], cy + ch);
        for i in 0..n { cr.line_to(xs[i], ys[i]); }
        cr.line_to(xs[n - 1], cy + ch);
        cr.close_path();
        let _ = cr.fill();

        // Polyline stroke.
        cr.set_source_rgba(ar, ag, ab, 1.0);
        cr.set_line_width(2.0);
        cr.set_line_join(gtk::cairo::LineJoin::Round);
        cr.move_to(xs[0], ys[0]);
        for i in 1..n { cr.line_to(xs[i], ys[i]); }
        let _ = cr.stroke();
    }
    ChartKind::Bar => {
        // Filled bars centered on each control point. Adjacent bars
        // touch; first/last clamps to the chart edge.
        let step = if n > 1 { cw / (n - 1) as f64 } else { cw };
        cr.set_source_rgba(ar, ag, ab, 0.55);
        for i in 0..n {
            let center = xs[i];
            let left  = if i == 0     { cx        } else { center - step / 2.0 };
            let right = if i == n - 1 { cx + cw   } else { center + step / 2.0 };
            let h = intensities[i] * ch;
            cr.rectangle(left, cy + ch - h, (right - left).max(0.0), h);
        }
        let _ = cr.fill();
    }
}
```

### Code reference — resampling

When Points (`new_n`) changes, project old intensities onto the new grid
by linearly interpolating each new sample's position in the old curve:

```rust
fn resample_to(&self, new_n: usize) {
    let old = self.intensities.borrow().clone();
    let old_n = old.len();
    if new_n == old_n || new_n == 0 { return; }
    let mut out = Vec::with_capacity(new_n);
    if old_n <= 1 {
        out.resize(new_n, old.first().copied().unwrap_or(0.5));
    } else {
        for i in 0..new_n {
            let t  = i as f64 / (new_n - 1).max(1) as f64;
            let xf = t * (old_n - 1) as f64;
            let lo = xf.floor() as usize;
            let hi = (lo + 1).min(old_n - 1);
            let frac = xf - lo as f64;
            out.push(old[lo] * (1.0 - frac) + old[hi] * frac);
        }
    }
    *self.intensities.borrow_mut() = out;
}
```

### Code reference — drag interaction

```rust
let drag = gtk::GestureDrag::new();
drawing_area.add_controller(drag.clone());
let drag_start_intensity = Rc::new(Cell::new(0.0_f64));

// Drag-begin: select the closest handle within the hit radius.
drag.connect_drag_begin(move |_, x, y| {
    let (cx, cy, cw, ch) = chart_rect(area.width() as f64, area.height() as f64);
    let intensities = editor.intensities.borrow();
    let n = intensities.len();
    let denom = (n - 1).max(1) as f64;
    let mut best_i = 0usize;
    let mut best_dist = f64::MAX;
    for i in 0..n {
        let px = cx + (i as f64 / denom) * cw;
        let py = cy + (1.0 - intensities[i]) * ch;
        let d  = ((px - x).powi(2) + (py - y).powi(2)).sqrt();
        if d < best_dist { best_dist = d; best_i = i; }
    }
    if best_dist < HIT_RADIUS_PX {
        editor.selected.set(Some(best_i));
        drag_start_intensity.set(intensities[best_i]);
        area.queue_draw();
    }
});

// Drag-update: snap to 5%, clamp 0..=1, update + redraw.
drag.connect_drag_update(move |_, _ox, oy| {
    let Some(i) = editor.selected.get() else { return; };
    let (_, _, _, ch) = chart_rect(area.width() as f64, area.height() as f64);
    let raw     = drag_start_intensity.get() + (-oy / ch);
    let snapped = (raw / 0.05).round() * 0.05;
    let clamped = snapped.clamp(0.0, 1.0);
    editor.intensities.borrow_mut()[i] = clamped;
    area.queue_draw();
});
```

### Tunables

```rust
const DEFAULT_POINTS: usize       = 7;
const DEFAULT_DURATION_S: f64     = 2.0;
const POINTS_MIN: u32             = 3;
const POINTS_MAX: u32             = 24;
const DURATION_MIN_S: f64         = 0.5;
const DURATION_MAX_S: f64         = 10.0;

const HANDLE_R: f64               = 6.0;
const HANDLE_R_SELECTED: f64      = 8.0;
const HIT_RADIUS_PX: f64          = 28.0;
const INTENSITY_STEP: f64         = 0.05;   // 5% snap

const CHART_HEIGHT: i32           = 220;
const Y_LABEL_W: f64              = 38.0;
const X_LABEL_H: f64              = 18.0;
const PAD: f64                    = 10.0;
```

### Keyboard accessibility (deferred to real implementation)

- `Tab` cycles handles
- `Up` / `Down` nudge intensity by 5% (1% with `Shift`)
- `Enter` commits

Not in the prototype — drag is the only input there.

### Laptop / no-haptic device handling — confirmed

- **Show the chooser and editor on laptop** — pattern *authoring* benefits
  from a precise pointer and a big screen, both of which the laptop has and
  the phone doesn't.
- **Per-bell vibration toggles greyed out** when no haptic is detected.
  They'd be no-ops on this device anyway.
- **Entry point: a "Manage vibration patterns" row in Preferences** that
  always opens the chooser, regardless of capability detection. Reachable
  on laptop where per-bell toggles are unavailable.
- **Editor banner**: "This device doesn't support vibration. Patterns sync
  to phones." Stays visible while editing on laptop.
- **Preview on laptop**: visual-only — sweep a playhead across the chart,
  pulse a coloured dot at the bottom, intensity-modulated by the envelope.
  No actual haptic call.

---

## Data model

### `vibration_patterns` library table — confirmed

```sql
CREATE TABLE IF NOT EXISTS vibration_patterns (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    uuid             TEXT NOT NULL UNIQUE,
    name             TEXT NOT NULL UNIQUE COLLATE NOCASE,
    duration_ms      INTEGER NOT NULL,
    intensities_json TEXT NOT NULL,           -- "[0.0, 0.4, 0.9, 0.4, 0.0]"
    chart_kind       TEXT NOT NULL DEFAULT 'line'
                     CHECK (chart_kind IN ('line', 'bar')),
    is_bundled       INTEGER NOT NULL DEFAULT 0,
    created_iso      TEXT NOT NULL,
    updated_iso      TEXT NOT NULL
);
```

Per-column reasoning:
- **`name UNIQUE NOCASE`** — pattern picker shows names; "heartbeat" and
  "Heartbeat" should collide. (Bell sounds aren't unique because their
  filenames disambiguate; patterns have no filename.)
- **`intensities_json`** — JSON array of floats in `[0, 1]`. N is implicit
  from array length. Avoids a `pattern_points` child table.
- **`chart_kind`** — *persisted* because Line and Bar describe two
  different playback semantics: linear interpolation vs. sample-and-hold
  step. Same data shape, different output curve.
- **`updated_iso`** — patterns are user-editable, so timestamp-of-last-edit
  is useful for chooser sorting. (`bell_sounds` doesn't need this since
  sounds are immutable files.)

Rust mirror:

```rust
pub struct VibrationPattern {
    pub id: i64,
    pub uuid: String,
    pub name: String,
    pub duration_ms: u32,
    pub intensities: Vec<f32>,
    pub chart_kind: ChartKind,
    pub is_bundled: bool,
    pub created_iso: String,
    pub updated_iso: String,
}
```

### Bundled seeds — confirmed

Fresh UUID family `7e9c4d2f-5a8b-4f1d-9e3c-2d6f7a8b00XX`, separate from
the bell-sounds family for visual disambiguation in DB inspection. Five
patterns; `Pyramid` ships in `bar` mode to demo that variant out of the
box.

| Const | UUID suffix | Kind | Duration | Intensities |
|---|---|---|---|---|
| `BUNDLED_PATTERN_PULSE_UUID` | `…0001` | line | 0.4 s | `[0.0, 1.0, 0.0]` |
| `BUNDLED_PATTERN_HEARTBEAT_UUID` | `…0002` | line | 1.5 s | `[0.0, 0.6, 0.0, 0.0, 1.0, 0.0]` |
| `BUNDLED_PATTERN_WAVE_UUID` | `…0003` | line | 2.0 s | `[0.0, 0.4, 0.7, 1.0, 0.7, 0.4, 0.0]` |
| `BUNDLED_PATTERN_RIPPLE_UUID` | `…0004` | line | 2.5 s | `[1.0, 0.7, 0.5, 0.3, 0.15, 0.0]` |
| `BUNDLED_PATTERN_PYRAMID_UUID` | `…0005` | **bar** | 3.0 s | `[0.2, 0.5, 1.0, 0.5, 0.2]` |

Bundled rows are deletable at the DB level (mirroring `bell_sounds`); the
chooser UI hides their delete buttons. Seeded via
`INSERT OR IGNORE INTO vibration_patterns (...)` so re-seeds are idempotent.

### Per-bell config storage — confirmed

Same logical shape (`sound_uuid + pattern_uuid + signal_mode`) at three
storage locations because the existing bells live in different places.

#### Starting Bell — `settings` keys

```
starting_bell_active        existing  bool, default false
starting_bell_sound         existing  UUID, default BUNDLED_BOWL_UUID
starting_bell_pattern       NEW       UUID, default BUNDLED_PATTERN_PULSE_UUID
starting_bell_signal_mode   NEW       text, default 'sound'
```

#### End Bell — `settings` keys

```
end_bell_active        existing  bool, default true
end_bell_sound         existing  UUID, default BUNDLED_BOWL_UUID
end_bell_pattern       NEW       UUID, default BUNDLED_PATTERN_PULSE_UUID
end_bell_signal_mode   NEW       text, default 'sound'
```

(Settings keys aren't constrained at the DB level — settings is a generic
k/v store. `signal_mode` validation lives in Rust at parse time.)

#### Interval Bells — `interval_bells` table columns

```sql
-- Final shape after the bump (no ALTER — wipe-and-reimport is the
-- migration path):
CREATE TABLE IF NOT EXISTS interval_bells (
    id                     INTEGER PRIMARY KEY AUTOINCREMENT,
    uuid                   TEXT NOT NULL UNIQUE,
    kind                   TEXT NOT NULL,    -- existing, with its own CHECK
    minutes                INTEGER NOT NULL,
    jitter_pct             INTEGER NOT NULL DEFAULT 0,
    sound                  TEXT NOT NULL,
    vibration_pattern_uuid TEXT NOT NULL DEFAULT '<PULSE_UUID>',  -- NEW
    signal_mode            TEXT NOT NULL DEFAULT 'sound'           -- NEW
                           CHECK (signal_mode IN ('sound', 'vibration', 'both')),
    enabled                INTEGER NOT NULL DEFAULT 1,
    created_iso            TEXT NOT NULL
);
```

CHECK constraint **kept and extended** when a new `signal_mode` value is
ever needed — same precedent as `sessions.mode`'s
`CHECK (mode IN ('timer', 'box_breath', 'guided'))`.

### Per-bell config decisions

- **`signal_mode` as TEXT (not int enum)** — readable in DB inspection,
  easy to extend if a fourth mode appears.
- **Pattern UUID always populated**, default to bundled `Pulse`. Toggling
  to vibration mode "just works" without a chooser detour. User's
  previously-picked pattern is preserved when flipping back to `'sound'`.
- **Default `'sound'` signal mode** for all bells out of the box —
  matches today's behaviour where bells just ring.
- **Default Pulse pattern everywhere** — neutral, user customizes.

### Box Breath per-phase storage — confirmed

Per-phase cues now mirror the per-bell shape: each phase carries
`signal_mode + sound_uuid + pattern_uuid + enabled`. Reasons for adding
sound to phases:
1. **Symmetry** with the bell rows.
2. **Voice-cue use case**: an "Inhale" recording for the inhale phase,
   "Hold" for the hold phases, "Exhale" for the exhale phase. Useful for
   visually-impaired users (vibration alone) *and* for users who'd
   benefit from spoken phase markers.

With four fields per phase plus the master, a flat settings-keys layout
would be 17 keys. Promoting to a small table keeps the column count
manageable and lets us reuse the CHECK-constraint pattern.

```sql
CREATE TABLE IF NOT EXISTS box_breath_phases (
    phase        TEXT PRIMARY KEY
                 CHECK (phase IN ('in', 'holdin', 'out', 'holdout')),
    enabled      INTEGER NOT NULL DEFAULT 0,
    signal_mode  TEXT NOT NULL DEFAULT 'sound'
                 CHECK (signal_mode IN ('sound', 'vibration', 'both')),
    sound_uuid   TEXT NOT NULL DEFAULT '<BUNDLED_BOWL_UUID>',
    pattern_uuid TEXT NOT NULL DEFAULT '<BUNDLED_PATTERN_PULSE_UUID>'
);
```

Phase names mirror the existing `Phase` enum (`In / HoldIn / Out /
HoldOut`). Four rows seeded at DB init via `INSERT OR IGNORE` keyed by
`phase`, like the bundled bell-sounds rows.

Plus one master setting key (renamed — was "vibration", now covers both
sound and vibration since phases get both):

```
boxbreath_cues_active   bool, default false   (master toggle)
```

UI rename to match: outer expander reads **"Cues during phases"** (was
"Vibrate during phases" in the prototype). Each phase row inside expands
to the same Sound/Vibration/Both ToggleGroup we use on bells, with
conditional Bell Sound + Pattern rows beneath.

Why a table over 17 settings keys:
- CHECK constraints on `phase` and `signal_mode` enforced at DB level —
  same precedent as `interval_bells.signal_mode` and `sessions.mode`.
- Symmetry with `interval_bells` — they're conceptually parallel
  ("a tiny library of cues, one per fixed key") and now have nearly the
  same column shape.
- Adding a new per-phase property later (e.g., a per-phase volume) is
  one column, not four new keys.

Trade-off: one new event kind (`box_breath_phase_update`) and a
`recompute_box_breath_phase` function. Settings-keys path would ride the
existing `setting_update` events with no new code — but the column
count tips the balance.

### Bell-sound categories — confirmed

`bell_sounds` gains a `category` column so the chooser can filter by
context. Bells (Starting / Interval / End) want general bell / gong /
chime sounds. Box Breath phases want voice cues or other sounds tailored
to the in / hold / out / hold cycle. No one wants a temple bell to ring
mid-inhalation, and a soft-spoken "Hold" doesn't fit a session-end cue.

```sql
-- Final shape of bell_sounds after the bump:
CREATE TABLE IF NOT EXISTS bell_sounds (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    uuid        TEXT NOT NULL UNIQUE,
    name        TEXT NOT NULL,
    file_path   TEXT NOT NULL,
    is_bundled  INTEGER NOT NULL DEFAULT 0,
    mime_type   TEXT NOT NULL,
    category    TEXT NOT NULL DEFAULT 'general'                -- NEW
                CHECK (category IN ('general', 'box_breath')),
    created_iso TEXT NOT NULL
);
```

Rust mirror:

```rust
pub enum BellSoundCategory {
    General,    // Bells / gongs / chimes — Start / Interval / End bells
    BoxBreath,  // Voice cues / soft phase markers — Box Breath phases
}
```

Categories are **mutually exclusive** (no `'both'` catch-all). Per
Janek: "no one will use a bell inside a box breath." If a real
"applies-to-both" use case shows up, extend the CHECK list later.

### Bell-sound category — chooser API

Sound chooser gains a category argument:

```rust
pub fn push_sound_chooser(
    &self,
    app: &MeditateApplication,
    current_uuid: Option<String>,
    category: BellSoundCategory,            // NEW
    on_selected: impl Fn(String) + 'static,
);
```

DB-layer filter:

```rust
fn list_bell_sounds_for_category(
    &self,
    category: BellSoundCategory,
) -> Result<Vec<BellSound>>;
```

Chooser callers pass the category implied by their context — bell rows
pass `General`, Box Breath phase rows pass `BoxBreath`. Empty-state in
the Box Breath chooser (when no `category = 'box_breath'` rows exist
yet) reads:

> No sounds for this category yet.
> Tap "Import file" to add one.

We **don't** fall back to `'general'` sounds in the Box Breath chooser
— that defeats the filter's purpose.

### Bell-sound import auto-categorization

The existing import flow takes a category argument from the calling
chooser context — no UI for picking. So importing a file from the Box
Breath phase chooser auto-tags it `category = 'box_breath'`; importing
from a bell row auto-tags `'general'`. Per-row category-edit UI (e.g.,
moving a sound from one category to the other after the fact) is a
deferrable extension.

### Bundled bell-sound categories

All currently-bundled rows (Tibetan Bowl, Bell, Gong, Inkin, Kanshō)
seed with `category = 'general'`. No `'box_breath'` bundled rows exist
yet — sourcing voice cues ("Inhale" / "Hold" / "Exhale" / "Hold") is a
follow-up TODO entry; until those land, the phase chooser ships empty
for new users and they import their own audio.

### Per-mode "what plays" toggle storage — confirmed

Three settings keys, mirroring the per-bell `signal_mode` enum:

```
timer_signal_mode         text 'sound'|'vibration'|'both', default 'both'
guided_signal_mode        text 'sound'|'vibration'|'both', default 'both'
boxbreath_signal_mode     text 'sound'|'vibration'|'both', default 'both'
```

Settings keys, no DB-level CHECK (settings is a generic k/v store);
validation in Rust at parse time — same as the per-bell `_signal_mode`
keys.

**Playback semantics** — the per-mode toggle is a runtime override on
top of per-bell intent:

| Mode toggle    | Bell `signal_mode='sound'` | `'vibration'` | `'both'` | Box Breath phase cues |
|---|---|---|---|---|
| `'both'`       | sound only                  | vibration only | both     | honoured per-phase |
| `'sound'`      | sound only                  | suppressed     | sound only | sounds honoured; vibrations suppressed |
| `'vibration'`  | suppressed                  | vibration only | vibration only | vibrations honoured; sounds suppressed |

For Box Breath specifically: phase **vibrations** are silenced under
mode `'sound'`, phase **sounds** are silenced under mode `'vibration'` —
phases obey the per-mode toggle just like bells. Per-phase
`signal_mode` plus per-phase `enabled` plus the master
`boxbreath_cues_active` are finer-grained gates *underneath* the mode
toggle.

**Defaults**: `'both'` everywhere. New installs start with all bells at
`signal_mode='sound'` (today's behaviour), so `'both'` at the mode level
just respects whatever the bell-level config says. Defaulting to
`'sound'` at the mode level would force the user to flip *two* toggles
to enable vibration anywhere — friction.

**Capability gating** (when `has_haptic = false`): all three keys are
*forced* to `'sound'` at read-time and the UI presents the toggle as a
static dimmed label "Sound only — no haptic device". The persisted
setting is left untouched so syncing to a phone restores the user's
intended mode.

**UI placement**: top of each mode's Bells subsection — but **defer**
prototyping until the Session-group split TODO lands (placement reads
weird inside the current monolithic Session group, prototyping it
twice is wasted work).

### Stale-reference handling — confirmed

If a bell's `sound` / `vibration_pattern_uuid`, or a Box Breath phase's
`sound_uuid` / `pattern_uuid`, references a row that no longer resolves
(deleted, never seeded, sync removed it):
- **Refuse to play** that bell or phase cue at session start, AND
- **Surface in setup**: red `.warning` chip on the row's subtitle
  ("Sound missing — pick another"), error banner on the setup page
  summarising affected bells / phases, **block Start Session** if any
  active row has a stale reference.

A separate TODO entry tracks landing this for the *existing*
bell-sound case first — establishes the pattern before vibration and
Box Breath inherit it. (`TODO.md`: "Surface stale / missing bell-sound
references at setup time…")

### Sync events — confirmed

All vibration / cue data rides event-log channels:
- New library table → `vibration_pattern_insert / _update / _delete`.
- Per-bell config (settings keys + `interval_bells` columns) rides the
  existing `setting_update` and `interval_bell_insert / _update` events
  — extend their payload JSON with the new fields, no new event kinds.
- Box Breath phase rows → new event kind
  `box_breath_phase_update` (PK is `phase`; payload carries the four
  mutable columns). Master toggle rides the existing `setting_update`
  channel.
- Per-mode "what plays" toggle (three settings keys) rides
  `setting_update`.
- Bell-sound category extension to `bell_sound_update` payload — no new
  event kind.

### Drop on schema land — confirmed

Per the no-compat rule (Janek is the single user; wipe-and-reimport is
the accepted migration path):
- `vibrate_on_end` setting — replaced by per-bell vibration on End Bell.
- `src/vibration.rs::trigger_if_enabled` body — replaced by the
  pattern-driven feedbackd playback driver during the playback phase.
  The 60-line file shape (no-op-on-failure) is reused by the new driver.

---

## Capability detection — confirmed

### Probe mechanism

Synchronous DBus call at app startup, before UI assembly. The probe
calls `org.sigxcpu.Feedback.GetEventsTheme` on the session bus — any
cheap real method works; this one returns the active feedback theme
name and is universally implemented by feedbackd. Auto-activation is
allowed (DBus launches feedbackd if it's installed but not running).

```rust
pub fn probe_haptic() -> bool {
    let Ok(conn) = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE) else {
        return false;
    };
    conn.call_sync(
        Some("org.sigxcpu.Feedback"),
        "/org/sigxcpu/Feedback",
        "org.sigxcpu.Feedback",
        "GetEventsTheme",
        None,
        None,
        gio::DBusCallFlags::NONE,    // allow auto-start
        500,                          // 500 ms ceiling
        gio::Cancellable::NONE,
    )
    .is_ok()
}
```

Why `GetEventsTheme` over alternatives:
- `NameHasOwner` would skip auto-start, so a phone with lazily-started
  feedbackd would falsely report `false` on first launch after boot.
- `GetCapabilities` isn't a feedbackd method.
- `Introspect` works but is heavier than necessary.

### Performance

Typical perceived freeze: **< 50 ms** (imperceptible).
- Phone: feedbackd is local on the session bus, call returns in tens of ms.
- Laptop: no service file matches, DBus returns `ServiceUnknown` near-
  instantly (a few ms).

The 500 ms timeout is the worst-case ceiling for a hung DBus daemon —
not the expected wait. Synchronous is fine: the probe runs once at
startup, well below any "slow startup" threshold.

### Caching

Probe runs once at startup. Result cached on `MeditateApplication`:

```rust
impl MeditateApplication {
    pub fn has_haptic(&self) -> bool {
        self.imp().has_haptic.get()
    }
}
```

UI consumers read `app.has_haptic()` when constructing rows. No
re-probing — devices don't grow vibration motors at runtime.

### UI gating — confirmed

When `has_haptic = false`:
- **Bell + Box-Breath-phase Sound/Vibration/Both ToggleGroups**: the
  `Vibration` and `Both` `Adw.Toggle` segments go insensitive
  (`set_sensitive(false)`); the `Sound` segment stays interactive.
  Affordance stays visible so the user understands what's available
  on a different device.
- **Pattern Adw.ActionRow** (revealed when Vibration / Both is
  selected): irrelevant since Vibration / Both can't be selected; the
  Revealer simply never reveals.
- **Per-mode "what plays" toggle**: read-time forced to `'sound'` and
  shown as a dimmed static label "Sound only — no haptic device". The
  persisted setting key is left untouched so syncing to a phone
  restores the user's intended mode.
- **Pattern chooser and editor**: fully interactive (laptop authoring
  path). Reachable via a "Manage vibration patterns" row in
  Preferences that is always present.
- **Editor banner** at top of the page: "This device doesn't support
  vibration. Patterns sync to phones."
- **Preview button** in the editor: visual-only — playhead sweep +
  accent dot pulse modulated by intensity. No feedbackd call.

The existing `src/vibration.rs` is already a no-op-on-failure shape;
the new pattern-driven playback driver inherits that, and never even
gets called when `has_haptic = false`.

---

## Phasing

1. **`vibration_patterns` CRUD + bundled seeds** — `meditate-core` + db
   wrapper, full TDD coverage. No UI yet.
2. **Pattern chooser NavigationPage** — list of patterns, synthetic
   "Create custom pattern…" top row, rename / delete / star toasts.
   Mirrors the guided-files chooser.
3. **Pattern editor NavigationPage** — line-chart canvas, sliders, header
   Cancel / Save. Save returns the new pattern's UUID to the chooser.
4. **Per-bell wiring** — Sound/Vibration/Both toggle on Starting / Interval
   (edit page) / End Bell rows; schema column on `bell_sounds`; events.
   Reuses the prototype's `wire_signal_toggle` helper.
5. **Bell-sound categories** — `category` column on `bell_sounds` with
   CHECK constraint, chooser filter argument, import auto-categorization.
   Bundled rows seed as `'general'`. Phase chooser starts empty for new
   users until voice-cue bundled sounds land (separate TODO).
6. **Box Breath per-phase** — outer "Cues during phases" expander with
   four nested phase expanders, each carrying the same
   Sound/Vibration/Both ToggleGroup as bells; new `box_breath_phases`
   table; `boxbreath_cues_active` master setting; phase chooser filtered
   to `category = 'box_breath'`; playback hook.
7. **Per-mode "what plays" toggle** — three settings keys
   (`timer_signal_mode` / `guided_signal_mode` / `boxbreath_signal_mode`,
   default `'both'`); UI placement deferred until the Session-group
   split TODO lands.
8. **feedbackd playback driver** — pattern → tick stream → DBus calls.
   Replaces the existing one-shot vibration.rs. Phone-side; laptop is the
   no-op stub.
9. **On-device test pass + tuning** — Janek's day, not mine.

---

## Open questions (parking lot)

- **Per-bell mute switch** — original plan included a "Mute Sound" parallel
  switch on each bell so a bell could vibrate without ringing. Decided to
  skip in favor of the Sound/Vibration/Both toggle which expresses the
  same intent. Revisit if the toggle proves insufficient for interval bells
  (where some of N might want to be silent).
- **Subtitle on interval-bell list rows** — extend to show vibration too,
  or keep showing just the sound? Defer.
- **Pattern editor snap behavior** — always 5%, or free with Shift-snap?
  Always-5% on touch.
- **Three-level nesting on Box Breath** — relies on the existing chevron
  CSS override. Visual-check during real implementation; if it breaks,
  fall back to the SwitchRow-master + Box-of-expanders-in-Revealer shape.

---

## Throwaway prototype location

Two layers of prototype, both at the bottom of the timer setup page:

**Bell-row UI prototypes** (Start / End / Box Breath) — inline expander
shells with the Sound / Vibration / Both ToggleGroup + Revealer-wrapped
rows.

**Pattern editor prototype** — full NavigationPage with chart canvas,
drag interaction, Line/Bar toggle, header subtitle, Duration / Points
SpinRows. Launched from the "Open pattern editor (prototype)" button.

To remove all of it when the real implementation lands:

- `data/ui/timer_view.blp`: search for "Vibration UI prototype" — three
  contiguous `Adw.Clamp` blocks plus the launcher-button clamp.
- `src/timer/imp.rs`: search for `vibration_proto_` (template children) and
  `setup_vibration_proto` (the wiring function and its `wire_signal_toggle`
  helper). Helper logic is reusable as-is for the real per-bell wiring.
- `src/vibration_proto.rs`: entire file. The chart-drawing,
  resampling, and drag-handler patterns are reusable verbatim in the real
  module — see the code-reference blocks earlier in this doc.
- `src/main.rs`: drop the `pub mod vibration_proto;` line.
