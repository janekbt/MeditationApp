//! Throwaway prototype of the vibration-pattern editor — a NavigationPage
//! pushed from a launcher button in the timer setup. No DB persistence,
//! no preview playback, just the layout + chart-drag interaction so Janek
//! can feel whether the editor reads right before we wire it for real.
//!
//! Gets ripped out alongside `setup_vibration_proto` once the real
//! pattern-editor module lands.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;

use crate::i18n::gettext;

// ── Tunables ──────────────────────────────────────────────────────────────
const DEFAULT_POINTS: usize       = 7;
const DEFAULT_DURATION_S: f64     = 2.0;
const POINTS_MIN: u32             = 3;
const POINTS_MAX: u32             = 24;
const DURATION_MIN_S: f64         = 0.5;
const DURATION_MAX_S: f64         = 10.0;

const HANDLE_R: f64               = 6.0;
const HANDLE_R_SELECTED: f64      = 8.0;
const HIT_RADIUS_PX: f64          = 28.0;

const CHART_HEIGHT: i32           = 220;
const Y_LABEL_W: f64              = 38.0;
const X_LABEL_H: f64              = 18.0;
const PAD: f64                    = 10.0;

// Snap to 5% increments.
const INTENSITY_STEP: f64         = 0.05;

// ── State ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChartKind {
    Line,
    Bar,
}

struct Editor {
    name: RefCell<String>,
    duration_s: Cell<f64>,
    intensities: RefCell<Vec<f64>>,
    selected: Cell<Option<usize>>,
    chart_kind: Cell<ChartKind>,
}

impl Editor {
    fn new() -> Rc<Self> {
        Rc::new(Self {
            name: RefCell::new(String::new()),
            duration_s: Cell::new(DEFAULT_DURATION_S),
            intensities: RefCell::new(vec![0.5; DEFAULT_POINTS]),
            selected: Cell::new(None),
            chart_kind: Cell::new(ChartKind::Line),
        })
    }

    /// Resample intensities onto a new equally-spaced grid of `new_n`
    /// points, linearly interpolating between adjacent old samples.
    /// Preserves the user's curve shape across Points-spinner changes.
    fn resample_to(&self, new_n: usize) {
        let old = self.intensities.borrow().clone();
        let old_n = old.len();
        if new_n == old_n || new_n == 0 {
            return;
        }
        let mut out = Vec::with_capacity(new_n);
        if old_n == 0 {
            out.resize(new_n, 0.5);
        } else if old_n == 1 {
            out.resize(new_n, old[0]);
        } else {
            for i in 0..new_n {
                let t = i as f64 / (new_n - 1).max(1) as f64;
                let xf = t * (old_n - 1) as f64;
                let lo = xf.floor() as usize;
                let hi = (lo + 1).min(old_n - 1);
                let frac = xf - lo as f64;
                out.push(old[lo] * (1.0 - frac) + old[hi] * frac);
            }
        }
        *self.intensities.borrow_mut() = out;
    }
}

// ── Public entry point ───────────────────────────────────────────────────

pub fn push_pattern_editor(nav_view: &adw::NavigationView) {
    let editor = Editor::new();

    // ── Header ────────────────────────────────────────────────────────
    let header = adw::HeaderBar::builder()
        .show_back_button(false)
        .show_start_title_buttons(false)
        .show_end_title_buttons(false)
        .build();

    let cancel_btn = gtk::Button::with_label(&gettext("Cancel"));
    let save_btn = gtk::Button::with_label(&gettext("Save"));
    save_btn.add_css_class("suggested-action");

    header.pack_start(&cancel_btn);
    header.pack_end(&save_btn);

    // ── Body: vertical Box of Adw.Clamps ─────────────────────────────
    let body = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(14)
        .margin_top(18)
        .margin_bottom(24)
        .margin_start(12)
        .margin_end(12)
        .build();

    // Name field.
    let name_clamp = adw::Clamp::builder()
        .maximum_size(360)
        .tightening_threshold(300)
        .build();
    let name_group = adw::PreferencesGroup::new();
    let name_row = adw::EntryRow::builder()
        .title(gettext("Name"))
        .build();
    name_group.add(&name_row);
    name_clamp.set_child(Some(&name_group));
    body.append(&name_clamp);

    // Shape group: Duration + Points.
    let shape_clamp = adw::Clamp::builder()
        .maximum_size(360)
        .tightening_threshold(300)
        .build();
    let shape_group = adw::PreferencesGroup::builder()
        .title(gettext("Shape"))
        .build();

    let duration_row = adw::SpinRow::builder()
        .title(gettext("Duration"))
        .subtitle(gettext("Seconds"))
        .digits(1)
        .build();
    duration_row.set_adjustment(Some(&gtk::Adjustment::new(
        DEFAULT_DURATION_S,
        DURATION_MIN_S,
        DURATION_MAX_S,
        0.1,
        0.5,
        0.0,
    )));

    let points_row = adw::SpinRow::builder()
        .title(gettext("Points"))
        .build();
    points_row.set_adjustment(Some(&gtk::Adjustment::new(
        DEFAULT_POINTS as f64,
        POINTS_MIN as f64,
        POINTS_MAX as f64,
        1.0,
        1.0,
        0.0,
    )));

    shape_group.add(&duration_row);
    shape_group.add(&points_row);
    shape_clamp.set_child(Some(&shape_group));
    body.append(&shape_clamp);

    // Chart card with a Line/Bar toggle pill at the top-right.
    let chart_clamp = adw::Clamp::builder()
        .maximum_size(420)
        .tightening_threshold(360)
        .build();
    let chart_card = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .css_classes(["card"])
        .build();

    // Header strip: "Pattern" heading on its own line, then a row with
    // a dynamic subtitle (changes with the Line / Bar toggle) on the
    // left and the toggle pill on the right.
    let header_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .margin_top(10)
        .margin_start(12)
        .margin_end(8)
        .margin_bottom(4)
        .build();

    let header_title = gtk::Label::builder()
        .label(gettext("Pattern"))
        .css_classes(["heading"])
        .halign(gtk::Align::Start)
        .build();
    header_box.append(&header_title);

    let header_row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .build();

    let chart_kind_subtitle = gtk::Label::builder()
        .label(format!(
            "{}\n{}",
            gettext("Line: Continuous transitions"),
            gettext("Bar: Abrupt transitions"),
        ))
        .css_classes(["dim-label", "caption"])
        .halign(gtk::Align::Start)
        .hexpand(true)
        .xalign(0.0)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .build();
    header_row.append(&chart_kind_subtitle);

    let chart_kind_toggle = adw::ToggleGroup::builder()
        .css_classes(["round"])
        .valign(gtk::Align::Center)
        .build();
    chart_kind_toggle.add(
        adw::Toggle::builder()
            .name("line")
            .label(gettext("Line"))
            .build(),
    );
    chart_kind_toggle.add(
        adw::Toggle::builder()
            .name("bar")
            .label(gettext("Bar"))
            .build(),
    );
    chart_kind_toggle.set_active_name(Some("line"));
    header_row.append(&chart_kind_toggle);

    header_box.append(&header_row);
    chart_card.append(&header_box);

    let drawing_area = gtk::DrawingArea::builder()
        .content_height(CHART_HEIGHT)
        .hexpand(true)
        .margin_start(8)
        .margin_end(8)
        .margin_top(4)
        .margin_bottom(8)
        .build();
    chart_card.append(&drawing_area);
    chart_clamp.set_child(Some(&chart_card));
    body.append(&chart_clamp);

    // Preview button (placeholder — no playback in prototype).
    let preview_clamp = adw::Clamp::builder()
        .maximum_size(360)
        .tightening_threshold(300)
        .build();
    let preview_btn = gtk::Button::builder()
        .label(gettext("Preview"))
        .css_classes(["pill"])
        .halign(gtk::Align::Center)
        .margin_top(8)
        .build();
    preview_clamp.set_child(Some(&preview_btn));
    body.append(&preview_clamp);

    // Banner for laptop / no-haptic devices. Always visible in the
    // prototype since we haven't wired capability detection yet.
    let banner_clamp = adw::Clamp::builder()
        .maximum_size(360)
        .tightening_threshold(300)
        .build();
    let banner = gtk::Label::builder()
        .label(gettext(
            "Prototype: drag handles up/down. Preview is a no-op here.",
        ))
        .css_classes(["dim-label", "caption"])
        .wrap(true)
        .justify(gtk::Justification::Center)
        .halign(gtk::Align::Center)
        .build();
    banner_clamp.set_child(Some(&banner));
    body.append(&banner_clamp);

    // ── Page chrome ───────────────────────────────────────────────────
    let scrolled = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&body)
        .build();
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&scrolled));

    let page = adw::NavigationPage::builder()
        .tag("vibration-pattern-editor")
        .title(gettext("Pattern editor"))
        .child(&toolbar)
        .build();

    // ── Drawing ───────────────────────────────────────────────────────
    let editor_for_draw = editor.clone();
    drawing_area.set_draw_func(move |area, cr, w, h| {
        draw_chart(area, cr, w as f64, h as f64, &editor_for_draw);
    });

    // ── Drag interaction (handle pick + drag-y to adjust intensity) ───
    let drag = gtk::GestureDrag::new();
    drawing_area.add_controller(drag.clone());

    let drag_start_intensity = Rc::new(Cell::new(0.0_f64));

    let editor_for_begin = editor.clone();
    let area_for_begin = drawing_area.clone();
    let dsi_begin = drag_start_intensity.clone();
    drag.connect_drag_begin(move |_, x, y| {
        let (cx, cy, cw, ch) = chart_rect(
            area_for_begin.width() as f64,
            area_for_begin.height() as f64,
        );
        let intensities = editor_for_begin.intensities.borrow();
        let n = intensities.len();
        if n == 0 {
            return;
        }
        // Closest control-point index by 2-D distance to the handle.
        let denom = (n - 1).max(1) as f64;
        let mut best_i = 0usize;
        let mut best_dist = f64::MAX;
        for i in 0..n {
            let px = cx + (i as f64 / denom) * cw;
            let py = cy + (1.0 - intensities[i]) * ch;
            let dist = ((px - x).powi(2) + (py - y).powi(2)).sqrt();
            if dist < best_dist {
                best_dist = dist;
                best_i = i;
            }
        }
        if best_dist < HIT_RADIUS_PX {
            editor_for_begin.selected.set(Some(best_i));
            dsi_begin.set(intensities[best_i]);
            area_for_begin.queue_draw();
        }
    });

    let editor_for_update = editor.clone();
    let area_for_update = drawing_area.clone();
    let dsi_update = drag_start_intensity.clone();
    drag.connect_drag_update(move |_drag, _ox, oy| {
        let Some(i) = editor_for_update.selected.get() else {
            return;
        };
        let (_cx, _cy, _cw, ch) = chart_rect(
            area_for_update.width() as f64,
            area_for_update.height() as f64,
        );
        // Negative oy = drag up = higher intensity. Map drag distance
        // to an intensity delta proportional to chart height, snap to
        // 5%, clamp to [0, 1].
        let raw = dsi_update.get() + (-oy / ch);
        let snapped = (raw / INTENSITY_STEP).round() * INTENSITY_STEP;
        let clamped = snapped.clamp(0.0, 1.0);
        let mut intensities = editor_for_update.intensities.borrow_mut();
        if let Some(slot) = intensities.get_mut(i) {
            if (*slot - clamped).abs() > 1e-6 {
                *slot = clamped;
                area_for_update.queue_draw();
            }
        }
    });

    // ── Spin row wiring ───────────────────────────────────────────────
    let editor_for_dur = editor.clone();
    let area_for_dur = drawing_area.clone();
    duration_row.connect_notify_local(Some("value"), move |row, _| {
        editor_for_dur.duration_s.set(row.value());
        area_for_dur.queue_draw();
    });

    let editor_for_pts = editor.clone();
    let area_for_pts = drawing_area.clone();
    points_row.connect_notify_local(Some("value"), move |row, _| {
        let new_n = row.value().round().clamp(POINTS_MIN as f64, POINTS_MAX as f64) as usize;
        editor_for_pts.resample_to(new_n);
        // Selected index may now be out of range — clear it.
        if let Some(idx) = editor_for_pts.selected.get() {
            if idx >= new_n {
                editor_for_pts.selected.set(None);
            }
        }
        area_for_pts.queue_draw();
    });

    // Line / Bar toggle.
    let editor_for_kind = editor.clone();
    let area_for_kind = drawing_area.clone();
    chart_kind_toggle.connect_active_name_notify(move |tg| {
        let kind = match tg.active_name().as_deref() {
            Some("bar") => ChartKind::Bar,
            _ => ChartKind::Line,
        };
        editor_for_kind.chart_kind.set(kind);
        area_for_kind.queue_draw();
    });

    let editor_for_name = editor.clone();
    name_row.connect_changed(move |row| {
        *editor_for_name.name.borrow_mut() = row.text().to_string();
    });

    // ── Cancel / Save / Preview wiring ────────────────────────────────
    let nav_for_cancel = nav_view.clone();
    cancel_btn.connect_clicked(move |_| {
        nav_for_cancel.pop();
    });

    let nav_for_save = nav_view.clone();
    save_btn.connect_clicked(move |_| {
        // Prototype: no DB write. Just pop.
        nav_for_save.pop();
    });

    preview_btn.connect_clicked(|_| {
        // No-op for prototype. Real implementation will sweep a
        // playhead and (on phone) drive feedbackd.
    });

    nav_view.push(&page);
}

// ── Drawing helpers ──────────────────────────────────────────────────────

fn chart_rect(w: f64, h: f64) -> (f64, f64, f64, f64) {
    let cx = Y_LABEL_W;
    let cy = PAD;
    let cw = (w - Y_LABEL_W - PAD).max(1.0);
    let ch = (h - PAD - X_LABEL_H).max(1.0);
    (cx, cy, cw, ch)
}

fn draw_chart(
    _area: &gtk::DrawingArea,
    cr: &gtk::cairo::Context,
    w: f64,
    h: f64,
    editor: &Rc<Editor>,
) {
    let (cx, cy, cw, ch) = chart_rect(w, h);

    // Hardcoded accent (close to Adwaita default purple-blue) — real
    // implementation pulls from AdwStyleManager.
    let (ar, ag, ab) = (0.32, 0.29, 0.72);

    let intensities = editor.intensities.borrow();
    let n = intensities.len();
    if n == 0 {
        return;
    }
    let denom = (n - 1).max(1) as f64;

    // ── Y axis labels + gridlines (0% / 50% / 100%) ──────────────────
    cr.set_font_size(10.0);
    let levels = [(1.0, "100%"), (0.5, "50%"), (0.0, "0%")];
    for (frac, label) in levels {
        let y = cy + (1.0 - frac) * ch;
        // Right-aligned label.
        cr.set_source_rgba(0.55, 0.55, 0.55, 1.0);
        let extents = cr.text_extents(label).ok();
        let lw = extents.map(|e| e.width()).unwrap_or(0.0);
        cr.move_to(Y_LABEL_W - lw - 4.0, y + 3.5);
        let _ = cr.show_text(label);

        // Faint horizontal gridline.
        cr.set_source_rgba(0.55, 0.55, 0.55, 0.15);
        cr.set_line_width(0.5);
        cr.move_to(cx, y);
        cr.line_to(cx + cw, y);
        let _ = cr.stroke();
    }

    // ── Geometry ──────────────────────────────────────────────────────
    let xs: Vec<f64> = (0..n).map(|i| cx + (i as f64 / denom) * cw).collect();
    let ys: Vec<f64> = intensities.iter().map(|&v| cy + (1.0 - v) * ch).collect();

    // ── Curve ─────────────────────────────────────────────────────────
    match editor.chart_kind.get() {
        ChartKind::Line => {
            // Filled area under the polyline.
            cr.set_source_rgba(ar, ag, ab, 0.22);
            cr.move_to(xs[0], cy + ch);
            for i in 0..n {
                cr.line_to(xs[i], ys[i]);
            }
            cr.line_to(xs[n - 1], cy + ch);
            cr.close_path();
            let _ = cr.fill();

            // Polyline stroke.
            cr.set_source_rgba(ar, ag, ab, 1.0);
            cr.set_line_width(2.0);
            cr.set_line_join(gtk::cairo::LineJoin::Round);
            cr.move_to(xs[0], ys[0]);
            for i in 1..n {
                cr.line_to(xs[i], ys[i]);
            }
            let _ = cr.stroke();
        }
        ChartKind::Bar => {
            // Filled bars centered on each control point. Adjacent bars
            // touch (each is half-step on either side; first/last bar
            // clamps to the chart edge).
            let step = if n > 1 { cw / (n - 1) as f64 } else { cw };
            cr.set_source_rgba(ar, ag, ab, 0.55);
            for i in 0..n {
                let center = xs[i];
                let left = if i == 0 {
                    cx
                } else {
                    center - step / 2.0
                };
                let right = if i == n - 1 {
                    cx + cw
                } else {
                    center + step / 2.0
                };
                let height = intensities[i] * ch;
                let top = cy + ch - height;
                cr.rectangle(left, top, (right - left).max(0.0), height);
            }
            let _ = cr.fill();
        }
    }

    // ── Handles ───────────────────────────────────────────────────────
    let selected = editor.selected.get();
    for i in 0..n {
        let r = if Some(i) == selected {
            HANDLE_R_SELECTED
        } else {
            HANDLE_R
        };
        // Halo for selected.
        if Some(i) == selected {
            cr.set_source_rgba(ar, ag, ab, 0.30);
            cr.arc(xs[i], ys[i], r + 4.0, 0.0, 2.0 * std::f64::consts::PI);
            let _ = cr.fill();
        }
        // White outer ring (so handle reads on the filled background).
        cr.set_source_rgba(1.0, 1.0, 1.0, 1.0);
        cr.arc(xs[i], ys[i], r + 1.5, 0.0, 2.0 * std::f64::consts::PI);
        let _ = cr.fill();
        // Accent core.
        cr.set_source_rgba(ar, ag, ab, 1.0);
        cr.arc(xs[i], ys[i], r, 0.0, 2.0 * std::f64::consts::PI);
        let _ = cr.fill();
    }

    // ── X axis labels — actual seconds at each control point ─────────
    cr.set_source_rgba(0.55, 0.55, 0.55, 1.0);
    cr.set_font_size(10.0);
    let label_y = cy + ch + X_LABEL_H - 4.0;
    let duration_s = editor.duration_s.get();
    for i in 0..n {
        let t = duration_s * (i as f64) / denom;
        let label = format_seconds(t);
        let extents = cr.text_extents(&label).ok();
        let lw = extents.map(|e| e.width()).unwrap_or(0.0);
        let lx = (xs[i] - lw / 2.0).clamp(cx, cx + cw - lw);
        cr.move_to(lx, label_y);
        let _ = cr.show_text(&label);
    }
}

/// Format a time in seconds with one decimal place and an "s" suffix:
/// `0.0s`, `0.5s`, `2.0s`. Keeps every label the same width so the row
/// of X-axis ticks stays visually evenly-spaced.
fn format_seconds(secs: f64) -> String {
    format!("{:.1}s", secs)
}
