//! Vibration-pattern editor — `Adw.NavigationPage` pushed when the
//! user picks "Create custom pattern…" or "Edit" in the chooser. Drives
//! the chart canvas (Cairo polyline / filled-bar rendering with
//! Gtk.GestureDrag handles), Duration / Points spin rows, Line / Bar
//! toggle, and Save → DB insert/update path.
//!
//! Drag is the only intensity input — handles snap to 5% increments
//! and the Points spin row resamples the curve linearly when the
//! point count changes. Save returns the saved pattern's UUID via
//! the caller's `on_saved` callback so the chooser can re-select.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;

use crate::application::MeditateApplication;
use crate::db::{ChartKind, VibrationPattern};
use crate::i18n::gettext;

// ── Tunables ──────────────────────────────────────────────────────────────
const DEFAULT_POINTS: usize       = 7;
const DEFAULT_DURATION_S: f64     = 2.0;
const POINTS_MIN: u32             = 3;
const POINTS_MAX: u32             = 24;
/// Minimum spacing between authored control points, in
/// milliseconds. Below this the LRA can't render the steps as
/// distinct, and feedbackd's chunking math (200 ms overlap, 10
/// segments per chunk) starts to wobble. The Points spin row's
/// upper bound is recomputed on every Duration change to enforce
/// it: `max_points = min(POINTS_MAX, floor(D_secs * 10))`.
const MIN_POINT_SPACING_MS: u32   = 100;

fn max_points_for_duration_s(duration_s: f64) -> u32 {
    let by_spacing = (duration_s * 1000.0 / MIN_POINT_SPACING_MS as f64).floor() as u32;
    POINTS_MAX.min(by_spacing).max(POINTS_MIN)
}
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
const INTENSITY_STEP: f32         = 0.05;

// ── Linear-interpolating resample (free function — pure, testable) ───────

/// Project `old` (an N-sample envelope) onto a new equally-spaced
/// grid of `new_n` samples by linear interpolation. Preserves the
/// curve shape across Points-spinner changes. Returns a fresh Vec.
pub(crate) fn resample(old: &[f32], new_n: usize) -> Vec<f32> {
    if new_n == 0 {
        return Vec::new();
    }
    let old_n = old.len();
    if old_n == 0 {
        return vec![0.5; new_n];
    }
    if old_n == 1 {
        return vec![old[0]; new_n];
    }
    if new_n == old_n {
        return old.to_vec();
    }
    let mut out = Vec::with_capacity(new_n);
    for i in 0..new_n {
        let t = i as f32 / (new_n - 1).max(1) as f32;
        let xf = t * (old_n - 1) as f32;
        let lo = xf.floor() as usize;
        let hi = (lo + 1).min(old_n - 1);
        let frac = xf - lo as f32;
        out.push(old[lo] * (1.0 - frac) + old[hi] * frac);
    }
    out
}

// ── State ────────────────────────────────────────────────────────────────

struct Editor {
    /// `None` = create-new (Save inserts), `Some(uuid)` = edit-existing
    /// (Save updates that uuid in place).
    edit_uuid: RefCell<Option<String>>,
    name: RefCell<String>,
    duration_s: Cell<f64>,
    intensities: RefCell<Vec<f32>>,
    selected: Cell<Option<usize>>,
    chart_kind: Cell<ChartKind>,
}

impl Editor {
    fn new(initial: Option<VibrationPattern>) -> Rc<Self> {
        let (edit_uuid, name, duration_s, intensities, chart_kind) = match initial {
            Some(p) => (
                Some(p.uuid),
                p.name,
                p.duration_ms as f64 / 1000.0,
                p.intensities,
                p.chart_kind,
            ),
            None => (
                None,
                String::new(),
                DEFAULT_DURATION_S,
                vec![0.5; DEFAULT_POINTS],
                ChartKind::Line,
            ),
        };
        Rc::new(Self {
            edit_uuid: RefCell::new(edit_uuid),
            name: RefCell::new(name),
            duration_s: Cell::new(duration_s),
            intensities: RefCell::new(intensities),
            selected: Cell::new(None),
            chart_kind: Cell::new(chart_kind),
        })
    }

    fn resample_to(&self, new_n: usize) {
        let old = self.intensities.borrow().clone();
        *self.intensities.borrow_mut() = resample(&old, new_n);
    }
}

// ── Public entry point ───────────────────────────────────────────────────

/// Push the editor onto `nav_view`. `initial` populates the form for
/// edit-existing flow (and routes Save through `update_vibration_pattern`);
/// `None` is create-new (routes through `insert_vibration_pattern`).
/// `on_saved` fires with the resulting uuid so the caller (the chooser)
/// can rebuild and re-select.
pub fn push_pattern_editor(
    nav_view: &adw::NavigationView,
    app: &MeditateApplication,
    initial: Option<VibrationPattern>,
    on_saved: impl Fn(String) + 'static,
) {
    let editor = Editor::new(initial);

    // ── Header ────────────────────────────────────────────────────────
    let header = adw::HeaderBar::builder()
        .show_back_button(false)
        .show_start_title_buttons(false)
        .show_end_title_buttons(false)
        .build();

    let cancel_btn = gtk::Button::with_label(&gettext("Cancel"));
    let save_btn = gtk::Button::with_label(&gettext("Save"));
    save_btn.add_css_class("suggested-action");
    save_btn.set_sensitive(false);

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

    // Name field — pre-populated for edit, empty for create.
    let name_clamp = adw::Clamp::builder()
        .maximum_size(360)
        .tightening_threshold(300)
        .build();
    let name_group = adw::PreferencesGroup::new();
    let name_row = adw::EntryRow::builder()
        .title(gettext("Name"))
        .text(&*editor.name.borrow())
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
        editor.duration_s.get(),
        DURATION_MIN_S,
        DURATION_MAX_S,
        0.1,
        0.5,
        0.0,
    )));

    let points_row = adw::SpinRow::builder()
        .title(gettext("Points"))
        .build();
    let initial_max_points = max_points_for_duration_s(editor.duration_s.get());
    points_row.set_adjustment(Some(&gtk::Adjustment::new(
        editor.intensities.borrow().len() as f64,
        POINTS_MIN as f64,
        initial_max_points as f64,
        1.0,
        1.0,
        0.0,
    )));
    // Subtitle communicates the dynamic cap. Adjustment refuses
    // values past it, but the user needs to see *why*.
    let points_subtitle_for = |max: u32| {
        format!("{} (up to {} for this duration)",
            gettext("Min 100 ms between points"),
            max,
        )
    };
    points_row.set_subtitle(&points_subtitle_for(initial_max_points));

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
    // the static two-line subtitle on the left and the toggle pill on
    // the right.
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
    chart_kind_toggle.set_active_name(Some(match editor.chart_kind.get() {
        ChartKind::Line => "line",
        ChartKind::Bar  => "bar",
    }));
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

    // Preview button (placeholder — playback driver lands in step 9).
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

    // No-haptic banner — only shown when the device can't actually
    // play the pattern (laptop authoring path).
    if !app.has_haptic() {
        let banner_clamp = adw::Clamp::builder()
            .maximum_size(360)
            .tightening_threshold(300)
            .build();
        let banner = gtk::Label::builder()
            .label(gettext(
                "This device doesn't support vibration. Patterns sync to phones.",
            ))
            .css_classes(["dim-label", "caption"])
            .wrap(true)
            .justify(gtk::Justification::Center)
            .halign(gtk::Align::Center)
            .build();
        banner_clamp.set_child(Some(&banner));
        body.append(&banner_clamp);
    }

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
        .title(if editor.edit_uuid.borrow().is_some() {
            gettext("Edit pattern")
        } else {
            gettext("New pattern")
        })
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

    let drag_start_intensity = Rc::new(Cell::new(0.0_f32));

    let editor_for_begin = editor.clone();
    let area_for_begin = drawing_area.clone();
    let dsi_begin = drag_start_intensity.clone();
    let drag_for_begin = drag.clone();
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
            let py = cy + (1.0 - intensities[i] as f64) * ch;
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
            // Claim the touch sequence so the surrounding ScrolledWindow
            // doesn't steal the vertical drag once it crosses ITS scroll
            // threshold. Without this, dragging a handle up by ~1 step
            // hands the pointer to the scrolled view and the editor
            // never sees subsequent motion events.
            drag_for_begin.set_state(gtk::EventSequenceState::Claimed);
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
        let raw = dsi_update.get() + (-oy / ch) as f32;
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
    let points_row_for_dur = points_row.clone();
    duration_row.connect_notify_local(Some("value"), move |row, _| {
        let d = row.value();
        editor_for_dur.duration_s.set(d);

        // Update the Points cap. If the new cap is below the
        // current point count, clamp + resample so the chart stays
        // consistent. Otherwise just bump the upper bound.
        let new_max = max_points_for_duration_s(d);
        let adj = points_row_for_dur.adjustment();
        adj.set_upper(new_max as f64);
        if adj.value() > new_max as f64 {
            adj.set_value(new_max as f64);
        }
        points_row_for_dur.set_subtitle(&format!(
            "{} (up to {} for this duration)",
            gettext("Min 100 ms between points"),
            new_max,
        ));
        area_for_dur.queue_draw();
    });

    let editor_for_pts = editor.clone();
    let area_for_pts = drawing_area.clone();
    points_row.connect_notify_local(Some("value"), move |row, _| {
        let max_for_dur = max_points_for_duration_s(editor_for_pts.duration_s.get());
        let new_n = row.value()
            .round()
            .clamp(POINTS_MIN as f64, max_for_dur as f64) as usize;
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

    // Live name validation — Save button gates on (non-empty trimmed)
    // && (no other row holds the same name, case-insensitive).
    let revalidate: Rc<dyn Fn()> = {
        let app = app.clone();
        let editor = editor.clone();
        let save_btn = save_btn.clone();
        Rc::new(move || {
            let name = editor.name.borrow().trim().to_string();
            if name.is_empty() {
                save_btn.set_sensitive(false);
                return;
            }
            let except = editor.edit_uuid.borrow().clone().unwrap_or_default();
            let collision = app
                .with_db(|db| db.is_vibration_pattern_name_taken(&name, &except))
                .and_then(|r| r.ok())
                .unwrap_or(false);
            save_btn.set_sensitive(!collision);
        })
    };

    let editor_for_name = editor.clone();
    let revalidate_for_name = revalidate.clone();
    name_row.connect_changed(move |row| {
        *editor_for_name.name.borrow_mut() = row.text().to_string();
        revalidate_for_name();
    });
    revalidate();

    // ── Cancel / Save / Preview wiring ────────────────────────────────
    let nav_for_cancel = nav_view.clone();
    cancel_btn.connect_clicked(move |_| {
        nav_for_cancel.pop();
    });

    let nav_for_save = nav_view.clone();
    let app_for_save = app.clone();
    let editor_for_save = editor.clone();
    let on_saved = Rc::new(on_saved);
    let on_saved_for_save = on_saved.clone();
    save_btn.connect_clicked(move |_| {
        let name = editor_for_save.name.borrow().trim().to_string();
        if name.is_empty() {
            return;
        }
        let duration_ms = (editor_for_save.duration_s.get() * 1000.0).round() as u32;
        let intensities = editor_for_save.intensities.borrow().clone();
        let chart_kind = editor_for_save.chart_kind.get();

        let saved_uuid = match editor_for_save.edit_uuid.borrow().clone() {
            None => app_for_save
                .with_db_mut(|db| {
                    db.insert_vibration_pattern(
                        &name, duration_ms, &intensities, chart_kind, false,
                    )
                })
                .and_then(|r| r.ok()),
            Some(uuid) => app_for_save
                .with_db_mut(|db| {
                    db.update_vibration_pattern(
                        &uuid, &name, duration_ms, &intensities, chart_kind,
                    )
                })
                .and_then(|r| r.ok())
                .map(|()| uuid),
        };
        if let Some(uuid) = saved_uuid {
            on_saved_for_save(uuid);
        }
        nav_for_save.pop();
    });

    // Preview slot: replacing the previous handle disarms its Drop
    // cancel, so feedbackd's per-app supersede on the new Vibrate is
    // what stops the old preview. A bare drop here would race the
    // new pattern's call_future and silently kill it.
    let preview_slot: Rc<RefCell<Option<crate::vibration::PatternPlayback>>> =
        Rc::new(RefCell::new(None));
    let editor_for_preview = editor.clone();
    let app_for_preview = app.clone();
    preview_btn.connect_clicked(move |_| {
        let intensities = editor_for_preview.intensities.borrow().clone();
        let duration_ms = (editor_for_preview.duration_s.get() * 1000.0) as u32;
        let chart_kind = editor_for_preview.chart_kind.get();
        let pattern = crate::db::VibrationPattern {
            id: 0,
            uuid: String::new(),
            name: String::new(),
            duration_ms,
            intensities,
            chart_kind,
            is_bundled: false,
            created_iso: String::new(),
            updated_iso: String::new(),
        };
        let new_handle = crate::vibration::PatternPlayback::play(&app_for_preview, &pattern);
        let mut slot = preview_slot.borrow_mut();
        if let Some(mut old) = slot.take() {
            old.disarm();
        }
        *slot = Some(new_handle);
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
        cr.set_source_rgba(0.55, 0.55, 0.55, 1.0);
        let extents = cr.text_extents(label).ok();
        let lw = extents.map(|e| e.width()).unwrap_or(0.0);
        cr.move_to(Y_LABEL_W - lw - 4.0, y + 3.5);
        let _ = cr.show_text(label);

        cr.set_source_rgba(0.55, 0.55, 0.55, 0.15);
        cr.set_line_width(0.5);
        cr.move_to(cx, y);
        cr.line_to(cx + cw, y);
        let _ = cr.stroke();
    }

    // ── Geometry ──────────────────────────────────────────────────────
    let xs: Vec<f64> = (0..n).map(|i| cx + (i as f64 / denom) * cw).collect();
    let ys: Vec<f64> = intensities.iter().map(|&v| cy + (1.0 - v as f64) * ch).collect();

    // ── Curve ─────────────────────────────────────────────────────────
    match editor.chart_kind.get() {
        ChartKind::Line => {
            cr.set_source_rgba(ar, ag, ab, 0.22);
            cr.move_to(xs[0], cy + ch);
            for i in 0..n {
                cr.line_to(xs[i], ys[i]);
            }
            cr.line_to(xs[n - 1], cy + ch);
            cr.close_path();
            let _ = cr.fill();

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
                let left = if i == 0 { cx } else { center - step / 2.0 };
                let right = if i == n - 1 { cx + cw } else { center + step / 2.0 };
                let height = intensities[i] as f64 * ch;
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
        if Some(i) == selected {
            cr.set_source_rgba(ar, ag, ab, 0.30);
            cr.arc(xs[i], ys[i], r + 4.0, 0.0, 2.0 * std::f64::consts::PI);
            let _ = cr.fill();
        }
        cr.set_source_rgba(1.0, 1.0, 1.0, 1.0);
        cr.arc(xs[i], ys[i], r + 1.5, 0.0, 2.0 * std::f64::consts::PI);
        let _ = cr.fill();
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

fn format_seconds(secs: f64) -> String {
    format!("{:.1}s", secs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn max_points_floors_to_100ms_spacing() {
        // 0.5 s → 5 max (5 × 100 ms). 0.4 s would round to 4 but the
        // editor's DURATION_MIN is 0.5, so this is the smallest case.
        assert_eq!(max_points_for_duration_s(0.5), 5);
        // 1.0 s → 10 max. 1.5 s → 15. 2.4 s → 24 (the absolute cap).
        assert_eq!(max_points_for_duration_s(1.0), 10);
        assert_eq!(max_points_for_duration_s(1.5), 15);
        assert_eq!(max_points_for_duration_s(2.4), 24);
        // Above 2.4 s, the absolute POINTS_MAX cap kicks in.
        assert_eq!(max_points_for_duration_s(5.0), 24);
        assert_eq!(max_points_for_duration_s(10.0), 24);
    }

    #[test]
    fn max_points_never_drops_below_min() {
        // Floor of 0.5 × 10 = 5, which is above POINTS_MIN=3 — but
        // the math could underflow on shorter durations. Guard.
        assert!(max_points_for_duration_s(0.5) >= POINTS_MIN);
        assert!(max_points_for_duration_s(0.0) >= POINTS_MIN);
    }

    #[test]
    fn resample_passes_through_when_n_unchanged() {
        let v = vec![0.0, 0.5, 1.0, 0.5, 0.0];
        assert_eq!(resample(&v, 5), v);
    }

    #[test]
    fn resample_keeps_endpoints_anchored() {
        // Linear interpolation must preserve the first and last
        // sample exactly, regardless of the new N.
        let v = vec![0.0, 1.0];
        let up = resample(&v, 7);
        assert_eq!(up.len(), 7);
        assert!((up[0] - 0.0).abs() < 1e-6);
        assert!((up[6] - 1.0).abs() < 1e-6);
        // Midpoint of a 0..1 ramp should be ~0.5.
        assert!((up[3] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn resample_shortens_a_curve_by_picking_grid_samples() {
        // Source has 5 samples 0, 0.25, 0.5, 0.75, 1.0; downsample to 3.
        // Grid points land at indices 0, 2, 4 in the original.
        let v = vec![0.0, 0.25, 0.5, 0.75, 1.0];
        let down = resample(&v, 3);
        assert_eq!(down.len(), 3);
        for (got, want) in down.iter().zip([0.0, 0.5, 1.0].iter()) {
            assert!((got - want).abs() < 1e-6, "got {got}, want {want}");
        }
    }

    #[test]
    fn resample_handles_single_sample_input() {
        // Pathological input — fill out to N with the same value.
        let v = vec![0.7];
        let up = resample(&v, 4);
        assert_eq!(up, vec![0.7, 0.7, 0.7, 0.7]);
    }

    #[test]
    fn resample_handles_empty_input() {
        // Pathological — return a default-filled vec, never panic.
        let v: Vec<f32> = vec![];
        let up = resample(&v, 3);
        assert_eq!(up, vec![0.5, 0.5, 0.5]);
    }

    #[test]
    fn resample_to_zero_returns_empty() {
        let v = vec![0.0, 1.0];
        assert!(resample(&v, 0).is_empty());
    }
}
