use std::cell::{Cell, RefCell};
use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::{glib, CompositeTemplate};

// ── GObject impl ──────────────────────────────────────────────────────────────

#[derive(Debug, Default, CompositeTemplate)]
#[template(resource = "/io/github/janekbt/Meditate/ui/stats_view.ui")]
pub struct StatsView {
    // Calendar
    #[template_child] pub cal_prev_btn:      TemplateChild<gtk::Button>,
    #[template_child] pub cal_next_btn:      TemplateChild<gtk::Button>,
    #[template_child] pub cal_month_label:   TemplateChild<gtk::Label>,
    #[template_child] pub cal_dow_box:       TemplateChild<gtk::Box>,
    #[template_child] pub cal_grid:          TemplateChild<gtk::Grid>,
    // Period toggle
    #[template_child] pub period_7d_btn:     TemplateChild<gtk::ToggleButton>,
    #[template_child] pub period_4w_btn:     TemplateChild<gtk::ToggleButton>,
    #[template_child] pub period_3m_btn:     TemplateChild<gtk::ToggleButton>,
    #[template_child] pub period_1y_btn:     TemplateChild<gtk::ToggleButton>,
    // Chart
    #[template_child] pub chart_container:   TemplateChild<gtk::Box>,
    // Text stats
    #[template_child] pub stat_avg_value:    TemplateChild<gtk::Label>,
    #[template_child] pub stat_streak_value: TemplateChild<gtk::Label>,
    #[template_child] pub stat_total_value:  TemplateChild<gtk::Label>,

    // State
    pub cal_year:   Cell<i32>,
    pub cal_month:  Cell<u32>,
    /// 42 calendar cells (cell box, day-number label), row-major order.
    pub cal_cells:  RefCell<Vec<(gtk::Box, gtk::Label)>>,
}

#[glib::object_subclass]
impl ObjectSubclass for StatsView {
    const NAME: &'static str = "StatsView";
    type Type = super::StatsView;
    type ParentType = gtk::Widget;

    fn class_init(klass: &mut Self::Class) {
        klass.bind_template();
        klass.set_layout_manager_type::<gtk::BinLayout>();
    }

    fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
        obj.init_template();
    }
}

impl ObjectImpl for StatsView {
    fn constructed(&self) {
        self.parent_constructed();

        let now = glib::DateTime::now_local().unwrap();
        self.cal_year.set(now.year());
        self.cal_month.set(now.month() as u32);

        self.build_dow_header();
        self.build_calendar_cells();
        self.wire_signals();
    }

    fn dispose(&self) {
        self.obj().first_child().map(|w| w.unparent());
    }
}

impl WidgetImpl for StatsView {}

// ── One-time setup ────────────────────────────────────────────────────────────

impl StatsView {
    fn build_dow_header(&self) {
        for name in ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"] {
            let label = gtk::Label::builder()
                .label(name)
                .halign(gtk::Align::Center)
                .css_classes(["caption", "dim-label"])
                .build();
            self.cal_dow_box.append(&label);
        }
    }

    fn build_calendar_cells(&self) {
        let mut cells = self.cal_cells.borrow_mut();
        for row in 0..6i32 {
            for col in 0..7i32 {
                let num = gtk::Label::builder()
                    .width_chars(2)
                    .xalign(0.5)
                    .vexpand(true)
                    .build();
                // halign::Fill lets the background extend edge-to-edge so
                // consecutive active days form an unbroken coloured strip.
                let cell = gtk::Box::builder()
                    .orientation(gtk::Orientation::Vertical)
                    .halign(gtk::Align::Fill)
                    .valign(gtk::Align::Center)
                    .height_request(30)
                    .build();
                cell.append(&num);
                self.cal_grid.attach(&cell, col, row, 1, 1);
                cells.push((cell, num));
            }
        }
    }

    fn wire_signals(&self) {
        let obj = self.obj();

        self.cal_prev_btn.connect_clicked(glib::clone!(
            #[weak(rename_to = this)] obj,
            move |_| {
                let imp = this.imp();
                let (y, m) = (imp.cal_year.get(), imp.cal_month.get());
                let (ny, nm) = if m == 1 { (y - 1, 12) } else { (y, m - 1) };
                imp.cal_year.set(ny);
                imp.cal_month.set(nm);
                imp.reload_calendar();
            }
        ));

        self.cal_next_btn.connect_clicked(glib::clone!(
            #[weak(rename_to = this)] obj,
            move |_| {
                let imp = this.imp();
                let (y, m) = (imp.cal_year.get(), imp.cal_month.get());
                let (ny, nm) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
                imp.cal_year.set(ny);
                imp.cal_month.set(nm);
                imp.reload_calendar();
            }
        ));

        for btn in [
            &*self.period_7d_btn, &*self.period_4w_btn,
            &*self.period_3m_btn, &*self.period_1y_btn,
        ] {
            btn.connect_toggled(glib::clone!(
                #[weak(rename_to = this)] obj,
                move |b| {
                    if b.is_active() {
                        this.imp().reload_chart();
                    }
                }
            ));
        }
    }
}

// ── Reload ────────────────────────────────────────────────────────────────────

impl StatsView {
    pub fn reload_all(&self) {
        self.reload_calendar();
        self.reload_chart();
        self.reload_text_stats();
    }

    pub fn reload_calendar(&self) {
        let year  = self.cal_year.get();
        let month = self.cal_month.get();

        // Month / year header label
        let month_str = glib::DateTime::new(
            &glib::TimeZone::local(), year, month as i32, 1, 0, 0, 0.0,
        ).ok()
            .and_then(|dt| dt.format("%B %Y").ok())
            .map(|s| s.to_string())
            .unwrap_or_default();
        self.cal_month_label.set_label(&month_str);

        // Days that had at least one session (local day-of-month numbers)
        let active: std::collections::HashSet<u32> = self
            .get_app()
            .and_then(|app| app.with_db(|db| db.get_active_days_in_month(year, month)))
            .and_then(|r| r.ok())
            .unwrap_or_default()
            .into_iter()
            .collect();

        // First weekday of the month (1=Mon … 7=Sun → offset 0–6)
        let offset = glib::DateTime::new(
            &glib::TimeZone::local(), year, month as i32, 1, 0, 0, 0.0,
        ).ok()
            .map(|dt| (dt.day_of_week() as usize).saturating_sub(1))
            .unwrap_or(0);

        let dim = days_in_month(year, month);

        let today = glib::DateTime::now_local().unwrap();
        let (ty, tm, td) = (
            today.year(),
            today.month() as u32,
            today.day_of_month() as u32,
        );

        let cells = self.cal_cells.borrow();
        for (i, (cell, num_lbl)) in cells.iter().enumerate() {
            let day = i as i32 - offset as i32 + 1;
            let col = i % 7;

            if day < 1 || day as u32 > dim {
                num_lbl.set_label("");
                cell.remove_css_class("cal-day-active");
                cell.remove_css_class("cal-streak-prev");
                cell.remove_css_class("cal-streak-next");
                num_lbl.remove_css_class("cal-day-active-label");
                num_lbl.remove_css_class("heading");
            } else {
                num_lbl.set_label(&day.to_string());

                let is_active = active.contains(&(day as u32));
                // Only connect within the same week row, and only within this month.
                let prev_active = is_active && col > 0 && day > 1
                    && active.contains(&((day - 1) as u32));
                let next_active = is_active && col < 6 && (day as u32) < dim
                    && active.contains(&((day + 1) as u32));

                if is_active {
                    cell.add_css_class("cal-day-active");
                    num_lbl.add_css_class("cal-day-active-label");
                } else {
                    cell.remove_css_class("cal-day-active");
                    num_lbl.remove_css_class("cal-day-active-label");
                }
                if prev_active {
                    cell.add_css_class("cal-streak-prev");
                } else {
                    cell.remove_css_class("cal-streak-prev");
                }
                if next_active {
                    cell.add_css_class("cal-streak-next");
                } else {
                    cell.remove_css_class("cal-streak-next");
                }

                if year == ty && month == tm && day as u32 == td {
                    num_lbl.add_css_class("heading");
                } else {
                    num_lbl.remove_css_class("heading");
                }
            }
        }

        // Disable next-month button when already showing the current month
        self.cal_next_btn.set_sensitive(!(year == ty && month == tm));
    }

    pub fn reload_chart(&self) {
        let days = self.current_chart_days();

        let sparse = self
            .get_app()
            .and_then(|app| app.with_db(|db| db.get_daily_totals(days)))
            .and_then(|r| r.ok())
            .unwrap_or_default();

        let today_ts = today_unix_day() * 86400;
        let daily: Vec<(i64, i64)> = (0..days as i64)
            .map(|i| {
                let ts = today_ts - (days as i64 - 1 - i) * 86400;
                let dur = sparse.iter()
                    .find(|(t, _)| *t == ts)
                    .map(|(_, d)| *d)
                    .unwrap_or(0);
                (ts, dur)
            })
            .collect();

        // Aggregate into weekly buckets for longer periods
        let data: Vec<(i64, i64)> = if days >= 90 {
            daily.chunks(7)
                .map(|c| (c[0].0, c.iter().map(|(_, d)| d).sum()))
                .collect()
        } else {
            daily
        };

        // Clear previous chart content
        while let Some(child) = self.chart_container.first_child() {
            self.chart_container.remove(&child);
        }

        if data.iter().all(|(_, d)| *d == 0) {
            return;
        }

        let chart_h = 148.0_f64;
        let max_val = data.iter().map(|(_, d)| *d).max().unwrap_or(1).max(1);

        // Y-axis: top label, equal spacers around the mid label
        let y_axis = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .width_request(46)
            .vexpand(true)
            .build();
        y_axis.append(
            &gtk::Label::builder()
                .label(&format_hm(max_val))
                .css_classes(["caption", "dim-label"])
                .halign(gtk::Align::Start)
                .build(),
        );
        y_axis.append(&gtk::Box::builder().vexpand(true).build());
        y_axis.append(
            &gtk::Label::builder()
                .label(&format_hm(max_val / 2))
                .css_classes(["caption", "dim-label"])
                .halign(gtk::Align::Start)
                .build(),
        );
        y_axis.append(&gtk::Box::builder().vexpand(true).build());

        // Bars: one column per data point
        let bars_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .hexpand(true)
            .vexpand(true)
            .spacing(2)
            .build();

        for (_, dur) in &data {
            let col = gtk::Box::builder()
                .orientation(gtk::Orientation::Vertical)
                .hexpand(true)
                .vexpand(true)
                .build();
            // Spacer pushes the bar to the bottom
            col.append(&gtk::Box::builder().vexpand(true).build());
            if *dur > 0 {
                let bar_h = ((*dur as f64 / max_val as f64) * chart_h) as i32;
                col.append(
                    &gtk::Box::builder()
                        .height_request(bar_h.max(2))
                        .hexpand(true)
                        .css_classes(["chart-bar"])
                        .build(),
                );
            }
            bars_box.append(&col);
        }

        self.chart_container.append(&y_axis);
        self.chart_container.append(&bars_box);
    }

    pub fn reload_text_stats(&self) {
        let Some(app) = self.get_app() else { return; };

        let avg = app.with_db(|db| db.get_running_average_secs(30))
            .and_then(|r| r.ok())
            .unwrap_or(0.0) as i64;
        self.stat_avg_value.set_label(&format_hm(avg));

        let best = app.with_db(|db| db.get_best_streak())
            .and_then(|r| r.ok())
            .unwrap_or(0);
        self.stat_streak_value.set_label(
            &if best == 0 { "–".to_string() } else { format!("{best}d") }
        );

        let total = app.with_db(|db| db.get_total_duration_secs())
            .and_then(|r| r.ok())
            .unwrap_or(0);
        self.stat_total_value.set_label(&format_hm(total));
    }

    fn current_chart_days(&self) -> u32 {
        if self.period_4w_btn.is_active() { return 28;  }
        if self.period_3m_btn.is_active() { return 90;  }
        if self.period_1y_btn.is_active() { return 365; }
        7
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

impl StatsView {
    fn get_app(&self) -> Option<crate::application::MeditateApplication> {
        self.obj()
            .root()
            .and_then(|r| r.downcast::<gtk::Window>().ok())
            .and_then(|w| w.application())
            .and_then(|a| a.downcast::<crate::application::MeditateApplication>().ok())
    }
}

fn days_in_month(year: i32, month: u32) -> u32 {
    let (ny, nm) = if month == 12 { (year + 1, 1u32) } else { (year, month + 1) };
    glib::DateTime::new(&glib::TimeZone::local(), ny, nm as i32, 1, 0, 0, 0.0)
        .ok()
        .and_then(|dt| dt.add_days(-1).ok())
        .map(|dt| dt.day_of_month() as u32)
        .unwrap_or(30)
}

fn today_unix_day() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
        / 86400
}

fn format_hm(secs: i64) -> String {
    if secs <= 0 { return "–".to_string(); }
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    match (h, m) {
        (0, m) => format!("{m}m"),
        (h, 0) => format!("{h}h"),
        (h, m) => format!("{h}h {m}m"),
    }
}
