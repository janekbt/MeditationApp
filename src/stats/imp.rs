use std::cell::Cell;
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
    /// True once build_dow_header has run (cells are rebuilt every reload).
    cal_built:      Cell<bool>,
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

        // build_dow_header + build_calendar_cells are deferred to the first
        // reload_calendar() call (lazy).  Only wire signals here.
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
        // Build the DoW header once; rebuild the day cells every call so
        // GTK creates fresh render nodes — this avoids GPU-level texture
        // cache residue left by the previous month on some drivers.
        if !self.cal_built.get() {
            self.build_dow_header();
            self.cal_built.set(true);
        }

        // Tear down all existing day cells.
        while let Some(child) = self.cal_grid.first_child() {
            self.cal_grid.remove(&child);
        }

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

        for row in 0..6i32 {
            for col in 0..7i32 {
                let i = (row * 7 + col) as usize;
                let day = i as i32 - offset as i32 + 1;

                let num_lbl = gtk::Label::builder()
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
                cell.append(&num_lbl);

                if day >= 1 && day as u32 <= dim {
                    num_lbl.set_label(&day.to_string());

                    let is_active = active.contains(&(day as u32));
                    let prev_active = is_active && col > 0 && day > 1
                        && active.contains(&((day - 1) as u32));
                    let next_active = is_active && col < 6 && (day as u32) < dim
                        && active.contains(&((day + 1) as u32));

                    if is_active {
                        cell.add_css_class("cal-day-active");
                        num_lbl.add_css_class("cal-day-active-label");
                    }
                    if prev_active { cell.add_css_class("cal-streak-prev"); }
                    if next_active { cell.add_css_class("cal-streak-next"); }
                    if year == ty && month == tm && day as u32 == td {
                        num_lbl.add_css_class("heading");
                    }
                }
                // Out-of-range cells: leave label text empty — the fresh
                // widget has no prior rendered content to leave residue.

                self.cal_grid.attach(&cell, col, row, 1, 1);
            }
        }

        // Disable next-month button when already showing the current month
        self.cal_next_btn.set_sensitive(!(year == ty && month == tm));
    }

    pub fn reload_chart(&self) {
        let days = self.current_chart_days();

        // Use glib for the since-date so the boundary is local midnight,
        // not UTC midnight (avoids ±1 day shift for UTC± timezones).
        let today = glib::DateTime::now_local().unwrap();
        let since = today
            .add_days(-(days as i32 - 1))
            .unwrap()
            .format("%Y-%m-%d")
            .unwrap()
            .to_string();

        let sparse = self
            .get_app()
            .and_then(|app| app.with_db(|db| db.get_daily_totals(&since)))
            .and_then(|r| r.ok())
            .unwrap_or_default();

        // Index sparse results for O(1) lookup instead of O(n) per day.
        let sparse_map: std::collections::HashMap<String, i64> =
            sparse.into_iter().collect();

        // Dense list of (local-date-string, secs) for every day in the period
        let daily: Vec<(String, i64)> = (0..days as i64)
            .map(|i| {
                let dt = today
                    .add_days(-(days as i32 - 1) + i as i32)
                    .unwrap();
                let date_str = dt.format("%Y-%m-%d").unwrap().to_string();
                let dur = sparse_map.get(&date_str).copied().unwrap_or(0);
                (date_str, dur)
            })
            .collect();

        // Aggregate: monthly for 1 year (12 bars), weekly for 3 months (~13 bars)
        let data: Vec<(String, i64)> = if days >= 365 {
            let mut months: Vec<(String, i64)> = Vec::new();
            for (date_str, dur) in &daily {
                let same = months.last().map(|(k, _)| k[..7] == date_str[..7]).unwrap_or(false);
                if same {
                    months.last_mut().unwrap().1 += dur;
                } else {
                    months.push((date_str.clone(), *dur));
                }
            }
            months
        } else if days >= 90 {
            daily.chunks(7)
                .map(|c| (c[0].0.clone(), c.iter().map(|(_, d)| d).sum()))
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

        let bars_h = 148i32;
        let chart_h = bars_h as f64;
        let max_val = data.iter().map(|(_, d)| *d).max().unwrap_or(1).max(1);

        // Y-axis — fixed to the bars height and top-aligned so labels sit
        // within the bars area only, not over the x-axis label row.
        let y_axis = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .width_request(46)
            .height_request(bars_h)
            .valign(gtk::Align::Start)
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

        // Bars row — fixed height so vexpand spacers inside columns work
        let bars_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .hexpand(true)
            .height_request(bars_h)
            .spacing(2)
            .build();

        // X-axis labels row — same spacing so columns stay aligned with bars
        let xlabels_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .hexpand(true)
            .spacing(2)
            .build();

        for (i, (_date_str, dur)) in data.iter().enumerate() {
            // Bar column
            let col = gtk::Box::builder()
                .orientation(gtk::Orientation::Vertical)
                .hexpand(true)
                .vexpand(true)
                .build();
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

            // X-axis label — empty string keeps the column as an invisible spacer
            xlabels_box.append(
                &gtk::Label::builder()
                    .label(&x_label_text(&data, i, days))
                    .css_classes(["caption", "dim-label"])
                    .halign(gtk::Align::Center)
                    .hexpand(true)
                    .build(),
            );
        }

        let right_area = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .hexpand(true)
            .spacing(4)
            .build();
        right_area.append(&bars_box);
        right_area.append(&xlabels_box);

        self.chart_container.append(&y_axis);
        self.chart_container.append(&right_area);
    }

    pub fn reload_text_stats(&self) {
        let Some(app) = self.get_app() else { return; };

        // Batch all four DB reads into a single with_db() call to avoid
        // acquiring and releasing the database lock four times.
        let (avg, best, total) = app
            .with_db(|db| {
                let avg_days = db
                    .get_setting("running_avg_days", "7")
                    .ok()
                    .and_then(|s| s.parse::<u32>().ok())
                    .unwrap_or(7);
                let avg   = db.get_running_average_secs(avg_days).unwrap_or(0.0) as i64;
                let best  = db.get_best_streak().unwrap_or(0);
                let total = db.get_total_duration_secs().unwrap_or(0);
                (avg, best, total)
            })
            .unwrap_or((0, 0, 0));

        self.stat_avg_value.set_label(&format_hm(avg));
        self.stat_streak_value.set_label(
            &if best == 0 { "–".to_string() } else { format!("{best}d") }
        );
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

/// Returns the x-axis label text for bar `i`.  Empty string = no label.
fn x_label_text(data: &[(String, i64)], i: usize, days: u32) -> String {
    let date_str = &data[i].0;
    let month: u32 = date_str[5..7].parse().unwrap_or(0);
    let day_num: u32 = date_str[8..10].parse().unwrap_or(0);

    match days {
        // 7 days: abbreviated weekday under every bar
        7 => weekday_for(date_str).to_string(),
        // 4 weeks: one date label per week (every 7th bar)
        28 => {
            if i % 7 == 0 {
                format!("{} {}", month_abbr(month), day_num)
            } else {
                String::new()
            }
        }
        // 3 months / 1 year (weekly buckets): month name at first bar of each month
        _ => {
            let prev_month: u32 = if i == 0 {
                0
            } else {
                data[i - 1].0[5..7].parse().unwrap_or(0)
            };
            if month != prev_month {
                month_abbr(month).to_string()
            } else {
                String::new()
            }
        }
    }
}

fn weekday_for(date_str: &str) -> &'static str {
    let y: i32 = date_str[0..4].parse().unwrap_or(2000);
    let m: i32 = date_str[5..7].parse().unwrap_or(1);
    let d: i32 = date_str[8..10].parse().unwrap_or(1);
    glib::DateTime::new(&glib::TimeZone::local(), y, m, d, 0, 0, 0.0)
        .ok()
        .map(|dt| match dt.day_of_week() {
            1 => "Mo", 2 => "Tu", 3 => "We", 4 => "Th",
            5 => "Fr", 6 => "Sa", _ => "Su",
        })
        .unwrap_or("")
}

fn month_abbr(month: u32) -> &'static str {
    match month {
        1 => "Jan", 2 => "Feb", 3 => "Mar", 4 => "Apr",
        5 => "May", 6 => "Jun", 7 => "Jul", 8 => "Aug",
        9 => "Sep", 10 => "Oct", 11 => "Nov", _ => "Dec",
    }
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
