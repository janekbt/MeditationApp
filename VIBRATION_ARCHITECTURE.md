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

No "bell" concept here. End chime exists (single shared end bell, configured
in the Session group like the others). Phase vibrations are net-new; they're
the only signal a phase has, so the Sound/Vibration/Both selector doesn't
apply. Just per-phase enable + pattern.

```
Phase Vibrations  [PreferencesGroup]
  └── Vibrate during phases  [show-enable-switch]   ← outer ExpanderRow / master
        ├── Inhale       [show-enable-switch]
        │     └── Pattern: <name>  ›
        ├── Hold (in)    [show-enable-switch]
        │     └── Pattern: <name>  ›
        ├── Exhale       [show-enable-switch]
        │     └── Pattern: <name>  ›
        └── Hold (out)   [show-enable-switch]
              └── Pattern: <name>  ›
```

Master is the outer expander itself — flipping it reveals/hides the four
phase rows with the native expander animation (no extra GtkRevealer
wrapping). Three-level nesting; same chevron CSS override the existing
Starting Bell already relies on covers it.

Defaults: master off, all four phase switches off.

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

User confirmed: the toggle is per-mode (Timer / Guided / Box Breath). Each
mode's setup view gets its own toggle that overrides per-bell intent for
that mode's session.

For Box Breath specifically, the master "Vibrate during phases" already
serves the gating role for phase vibrations; the per-mode toggle would only
apply to the End Bell (and any other shared bells visible in that mode).

Open: where exactly the toggle sits in each mode's setup view. Probably top
of the bells subsection. Defer detail until we get to that plan item.

---

## Vibration pattern editor

Mockup at `/home/janek/Downloads/vibration_pattern_editor_mockup.html`.

Confirmed deviations from the mockup:
- **Drop the presets row** at top (bundled patterns live in the chooser
  page, not the editor).
- **Drop the "tap and hold to record" pad** at the bottom.
- **Drop the "loop every 5 min" checkbox.**
- **Replace the bar-chart pulse-train** with an **envelope** rendered as a
  line chart.

New / changed knobs:
- **Window length** — `Adw.SpinRow` "Duration" in seconds (range e.g.
  0.5–10.0 s, step 0.1). Replaces the hardcoded 2.0 s.
- **Resolution** — `Adw.SpinRow` "Points" (range e.g. 3–24, step 1). Spacing
  is `duration / (points - 1)`. When resolution changes, **resample** the
  existing curve onto the new grid (linear interpolate) — don't reset.
- **Line chart canvas** — custom `Gtk.DrawingArea` (Cairo). Renders the
  polyline through the N equally-spaced control points. Each point is a
  draggable handle; drag y maps to intensity (clamped 0–100%).
- **Tap-to-select** — selecting a point highlights it; sliders below edit
  intensity precisely.
- **Snap** — default to 5% intensity steps when dragging. Flag for review.
- **Preview button** — bottom-right; plays the envelope through feedbackd
  at the device's max sample rate (laptop: visual-only, no haptic).

Header bar: Cancel / Save (own NavigationPage with `Adw.HeaderBar`).

Keyboard:
- `Tab` cycles points
- `Up/Down` nudges intensity by 5% (1% with `Shift`)
- `Enter` confirms

---

## Data model

```sql
CREATE TABLE vibration_patterns (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    uuid TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL UNIQUE COLLATE NOCASE,
    duration_ms INTEGER NOT NULL,
    intensities_json TEXT NOT NULL,   -- e.g. "[0.0, 0.4, 0.9, 0.4, 0.0]"
    is_bundled INTEGER NOT NULL DEFAULT 0,
    created_iso TEXT NOT NULL,
    updated_iso TEXT NOT NULL
);
```

`intensities_json` is a JSON array of floats in [0, 1]. N is implicit from
the array length. Sampling at playback time uses linear interpolation
between consecutive points.

Bundled seeds (4 patterns under stable UUIDs, like `bell_sounds`):
- **Pulse** — single short bump.
- **Heartbeat** — double-thud.
- **Wave** — slow rise-and-fall.
- **Ripple** — decaying succession.

Foreign references:
- `bell_sounds` gains `vibration_pattern_uuid TEXT NULL`.
- Box Breath per-phase: either four new settings keys
  (`boxbreath_vibration_inhale_uuid` etc.) or a tiny `box_breath_phases`
  table. Lean toward settings keys to avoid a new table for four rows.

Sync via the existing event log:
`vibration_pattern_insert / _update / _delete`. Bell-row updates are already
covered by `bell_sound_update`. Box-breath phase choices ride the
settings-event channel.

Drop on schema land:
- `vibrate_on_end` setting — replaced by per-bell vibration on End Bell + the
  per-mode "what plays" toggle.
- `src/vibration.rs` — the existing 60-line one-shot feedbackd trigger gets
  rewritten as a pattern-driven playback driver during the playback phase.

---

## Capability detection

Probe at app startup, cache the result globally.

Approach: try to connect to `org.sigxcpu.Feedback` on the session bus and
fetch its supported events list. Available on Phosh-based devices (Librem
5, Pinephone). On a laptop, the service isn't present — connection fails
→ no haptic.

When `has_haptic = false`:
- Hide every "Vibrate" / Pattern row entirely (cleaner than greying — they
  don't apply at all on this device).
- The per-mode "what plays" toggle collapses to no-op or reads as
  "Sound only" with no segmented choice.
- Pattern editor is still reachable (you might be authoring patterns to
  sync to a phone) — top-level banner: "This device doesn't support
  vibration. Patterns sync to phones."

The existing `src/vibration.rs` is already a no-op-on-failure shape; the
new playback path inherits that.

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
5. **Box Breath per-phase** — outer "Vibrate during phases" expander with
   four nested phase expanders; settings keys; playback hook.
6. **Per-mode "what plays" toggle** — placed at the top of each mode's
   bells subsection.
7. **feedbackd playback driver** — pattern → tick stream → DBus calls.
   Replaces the existing one-shot vibration.rs. Phone-side; laptop is the
   no-op stub.
8. **On-device test pass + tuning** — Janek's day, not mine.

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

The Start / End / Box-Breath prototypes live at the bottom of the timer
setup page. To remove them when the real implementation lands:

- `data/ui/timer_view.blp`: search for "Vibration UI prototype" — three
  contiguous `Adw.Clamp` blocks.
- `src/timer/imp.rs`: search for `vibration_proto_` (template children) and
  `setup_vibration_proto` (the wiring function and its `wire_signal_toggle`
  helper). Helper logic is reusable as-is for the real per-bell wiring.
