use std::cell::{Cell, RefCell};
use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::{glib, cairo, CompositeTemplate};

/// Fallback weekly-goal target in minutes if the setting is unset or
/// unparseable. The real value lives in the `weekly_goal_mins` DB setting,
/// exposed in Preferences → Statistics → Weekly goal.
const DEFAULT_WEEKLY_GOAL_MINS: i64 = 150;

// ── GObject impl ──────────────────────────────────────────────────────────────

#[derive(Debug, Default, CompositeTemplate)]
#[template(resource = "/io/github/janekbt/Meditate/ui/stats_view.ui")]
pub struct StatsView {
    // Hero goal
    #[template_child] pub goal_ring:            TemplateChild<gtk::DrawingArea>,
    #[template_child] pub goal_pct_label:       TemplateChild<gtk::Label>,
    #[template_child] pub goal_progress_label:  TemplateChild<gtk::Label>,
    #[template_child] pub goal_sub_label:       TemplateChild<gtk::Label>,
    // Contribution grid
    #[template_child] pub contrib_range_label:  TemplateChild<gtk::Label>,
    #[template_child] pub contrib_grid:         TemplateChild<gtk::Grid>,
    #[template_child] pub contrib_legend_box:   TemplateChild<gtk::Box>,
    // Insights
    #[template_child] pub insights_list:        TemplateChild<gtk::ListBox>,
    // Chart
    #[template_child] pub period_7d_btn:        TemplateChild<gtk::ToggleButton>,
    #[template_child] pub period_4w_btn:        TemplateChild<gtk::ToggleButton>,
    #[template_child] pub period_3m_btn:        TemplateChild<gtk::ToggleButton>,
    #[template_child] pub period_1y_btn:        TemplateChild<gtk::ToggleButton>,
    #[template_child] pub chart_bars_btn:       TemplateChild<gtk::ToggleButton>,
    #[template_child] pub chart_line_btn:       TemplateChild<gtk::ToggleButton>,
    #[template_child] pub chart_container:      TemplateChild<gtk::Box>,
    // Mini-stats
    #[template_child] pub mini_streak_value:    TemplateChild<gtk::Label>,
    #[template_child] pub mini_total_value:     TemplateChild<gtk::Label>,
    #[template_child] pub mini_sessions_value:  TemplateChild<gtk::Label>,

    // State
    /// 91 contribution cells, column-major (col × 7 + row). Each cell is a
    /// Gtk.Label: the background colour comes from .contrib-cell.level-*
    /// and the text holds the optional achievement glyph (✔ / ★).
    pub contrib_cells:  RefCell<Vec<gtk::Label>>,
    /// Current weekly-goal progress ratio (0.0..=1.0) — redrawn each refresh.
    pub goal_pct:       Cell<f64>,
    /// True once the 91 contribution cells + legend swatches have been built.
    cells_built:        Cell<bool>,
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
        self.wire_signals();
        self.install_ring_draw();
    }

    fn dispose(&self) {
        if let Some(w) = self.obj().first_child() { w.unparent() }
    }
}

impl WidgetImpl for StatsView {}

// ── One-time setup ────────────────────────────────────────────────────────────

impl StatsView {
    fn wire_signals(&self) {
        let obj = self.obj();
        for btn in [
            &*self.period_7d_btn, &*self.period_4w_btn,
            &*self.period_3m_btn, &*self.period_1y_btn,
            &*self.chart_bars_btn, &*self.chart_line_btn,
        ] {
            btn.connect_toggled(glib::clone!(
                #[weak(rename_to = this)] obj,
                move |b| if b.is_active() { this.imp().reload_chart(); }
            ));
        }
    }

    fn install_ring_draw(&self) {
        // Draw function reads the current pct from the Cell each redraw so
        // reloading progress just needs queue_draw(), not a new closure.
        let obj = self.obj();
        self.goal_ring.set_draw_func(glib::clone!(
            #[weak(rename_to = this)] obj,
            move |area, cr, w, h| {
                let pct = this.imp().goal_pct.get().clamp(0.0, 1.0);
                draw_goal_ring(area, cr, w, h, pct);
            }
        ));
    }

    fn build_contrib_cells_and_legend(&self) {
        // 13 columns × 7 rows — column-major fills week-by-week
        let mut cells = self.contrib_cells.borrow_mut();
        for col in 0..13i32 {
            for row in 0..7i32 {
                let cell = gtk::Label::builder()
                    .css_classes(["contrib-cell"])
                    .label("")
                    .xalign(0.5)
                    .yalign(0.5)
                    .hexpand(true)
                    .vexpand(true)
                    .width_request(14)
                    .height_request(14)
                    .build();
                self.contrib_grid.attach(&cell, col, row, 1, 1);
                cells.push(cell);
            }
        }
        // Legend swatches — 5 levels from 0 (empty) to 4 (max)
        for level in 0..=4 {
            let sw = gtk::Box::builder()
                .css_classes(["contrib-swatch", &format!("level-{level}")])
                .height_request(10)
                .width_request(10)
                .build();
            self.contrib_legend_box.append(&sw);
        }
    }
}

// ── Reload entry points ───────────────────────────────────────────────────────

impl StatsView {
    pub fn reload_all(&self) {
        if !self.cells_built.get() {
            self.build_contrib_cells_and_legend();
            self.cells_built.set(true);
        }
        self.reload_goal_ring();
        self.reload_contrib_grid();
        self.reload_insights();
        self.reload_chart();
        self.reload_mini_stats();
    }

    fn reload_goal_ring(&self) {
        // Batch: one with_db() call returns (avg_secs, goal_mins).
        let (avg_secs, goal_mins) = self.get_app()
            .and_then(|app| app.with_db(|db| {
                let avg = db.get_running_average_secs(7).unwrap_or(0.0);
                let goal = db.get_setting("weekly_goal_mins", "150")
                    .ok()
                    .and_then(|s| s.parse::<i64>().ok())
                    .filter(|v| *v > 0)
                    .unwrap_or(DEFAULT_WEEKLY_GOAL_MINS);
                (avg, goal)
            }))
            .unwrap_or((0.0, DEFAULT_WEEKLY_GOAL_MINS));
        let week_mins = (avg_secs * 7.0 / 60.0) as i64;
        let pct = week_mins as f64 / goal_mins as f64;
        self.goal_pct.set(pct);
        self.goal_ring.queue_draw();
        self.goal_pct_label.set_label(
            &format!("{}%", (pct.clamp(0.0, 9.99) * 100.0).round() as i32),
        );

        // "1h 48m / 2h 30m"
        self.goal_progress_label.set_markup(&format!(
            "{} <span alpha=\"60%\" size=\"60%\">/ {}</span>",
            format_hm_mins(week_mins),
            format_hm_mins(goal_mins),
        ));
        let remain = (goal_mins - week_mins).max(0);
        let sub = if remain == 0 {
            format!("Goal reached ✓ · {} this week", format_hm_mins(week_mins))
        } else {
            format!("{} to go this week", format_hm_mins(remain))
        };
        self.goal_sub_label.set_label(&sub);
    }

    fn reload_contrib_grid(&self) {
        let now = glib::DateTime::now_local().unwrap();
        // day_of_week: Mon = 1 … Sun = 7. We want row 0 = Mon, row 6 = Sun.
        let today_dow_idx = now.day_of_week() - 1;
        let cur_monday = now.add_days(-today_dow_idx).unwrap();

        // Fetch 91 days of totals (12 weeks ago Monday through today)
        // and the user's weekly goal in a single DB borrow.
        let since_dt = cur_monday.add_days(-12 * 7).unwrap();
        let since = since_dt.format("%Y-%m-%d").unwrap().to_string();
        let (totals_vec, goal_mins) = self.get_app()
            .and_then(|app| app.with_db(|db| {
                let t = db.get_daily_totals(&since).unwrap_or_default();
                let g = db.get_setting("weekly_goal_mins", "150")
                    .ok()
                    .and_then(|s| s.parse::<i64>().ok())
                    .filter(|v| *v > 0)
                    .unwrap_or(DEFAULT_WEEKLY_GOAL_MINS);
                (t, g)
            }))
            .unwrap_or_else(|| (Vec::new(), DEFAULT_WEEKLY_GOAL_MINS));
        let totals: std::collections::HashMap<String, i64> =
            totals_vec.into_iter().collect();
        // Daily share of the weekly goal — drives the heatmap thresholds so a
        // 10-hour retreat day doesn't make on-target days look washed-out.
        let daily_expected_mins = (goal_mins as f64 / 7.0).round().max(1.0) as i64;

        let cells = self.contrib_cells.borrow();
        let today_unix = now.to_unix();
        for col in 0..13i32 {
            let weeks_ago = 12 - col;
            let week_monday = cur_monday.add_days(-weeks_ago * 7).unwrap();
            for row in 0..7i32 {
                let date = week_monday.add_days(row).unwrap();
                let idx = (col * 7 + row) as usize;
                let cell = &cells[idx];

                // Clear prior level / today classes and any glyph text
                for l in 0..=4 { cell.remove_css_class(&format!("level-{l}")); }
                cell.remove_css_class("today");
                cell.set_label("");

                if date.to_unix() > today_unix + 60 {
                    // Future day — show as empty level-0 with reduced opacity
                    cell.add_css_class("level-0");
                    cell.set_opacity(0.3);
                    continue;
                }
                cell.set_opacity(1.0);

                let date_str = date.format("%Y-%m-%d").unwrap();
                let mins = totals.get(date_str.as_str()).copied().unwrap_or(0) / 60;
                let level = minutes_to_level(mins, daily_expected_mins);
                cell.add_css_class(&format!("level-{level}"));
                // ★ only for days that exceed the daily goal by 20 % or more.
                // On-target days rely on colour intensity alone — a wall of
                // glyphs in a 13×7 grid blurs together and dilutes the signal.
                if level == 4 { cell.set_label("★"); }
                if date.year() == now.year()
                    && date.day_of_year() == now.day_of_year()
                {
                    cell.add_css_class("today");
                }
            }
        }

        // Date-range caption: "<since month> – <current month>"
        let range = format!("{} – {}",
            month_short(since_dt.month() as u32),
            month_short(now.month() as u32),
        );
        self.contrib_range_label.set_label(&range);
    }

    fn reload_insights(&self) {
        while let Some(c) = self.insights_list.first_child() {
            self.insights_list.remove(&c);
        }

        let Some(app) = self.get_app() else { return; };
        let now = glib::DateTime::now_local().unwrap();

        // Batch every insight-driving query into a single DB borrow.
        let data = app.with_db(|db| {
            let (ty, tm) = (now.year(), now.month() as u32);
            let (ly, lm) = if tm == 1 { (ty - 1, 12) } else { (ty, tm - 1) };
            let fourteen_since = now.add_days(-13).unwrap()
                .format("%Y-%m-%d").unwrap().to_string();
            InsightData {
                current_streak: db.get_streak().unwrap_or(0),
                best_streak:    db.get_best_streak().unwrap_or(0),
                this_month:     db.get_month_total_secs(ty, tm).unwrap_or(0),
                last_month:     db.get_month_total_secs(ly, lm).unwrap_or(0),
                daily_totals:   db.get_daily_totals(&fourteen_since).unwrap_or_default(),
                longest:        db.get_longest_session().unwrap_or(None),
                typical:        db.get_median_duration_secs().unwrap_or(None).unwrap_or(0),
                avg_secs:       db.get_running_average_secs(7).unwrap_or(0.0) as i64,
                hour_buckets:   db.get_hour_buckets().unwrap_or((0, 0, 0)),
                session_count:  db.get_session_count().unwrap_or(0),
            }
        }).unwrap_or_default();

        // 1. Current streak — complements the lifetime best shown in mini-stats.
        if data.current_streak > 0 {
            let body = if data.current_streak >= data.best_streak && data.current_streak > 1 {
                format!("{} days — new record", data.current_streak)
            } else if data.best_streak > data.current_streak {
                format!("{} days · best was {}", data.current_streak, data.best_streak)
            } else {
                "1 day · keep going".to_string()
            };
            let peak = data.current_streak >= data.best_streak && data.current_streak > 1;
            self.append_insight("●", "Current streak", &body, peak);
        }

        // 2. This week vs previous 7 days — short-horizon trend.
        let (this_week_secs, last_week_secs) = week_over_week(&data.daily_totals, &now);
        if last_week_secs > 0 {
            let delta = this_week_secs - last_week_secs;
            let pct = (delta as f64 / last_week_secs as f64 * 100.0).round() as i32;
            let (icon, dir) = if pct >= 0 { ("↗", "up") } else { ("↘", "down") };
            self.append_insight(icon, "This week's pace", &format!(
                "{}% {dir} vs last week ({} vs {})",
                pct.abs(),
                format_hm_secs(this_week_secs),
                format_hm_secs(last_week_secs),
            ), false);
        }

        // 3. Trend vs last month.
        if data.last_month > 0 {
            let pct = ((data.this_month - data.last_month) as f64
                / data.last_month as f64 * 100.0).round() as i32;
            let (icon, title) = if pct >= 0 {
                ("↗", "You're meditating more")
            } else {
                ("↘", "You're meditating less")
            };
            self.append_insight(icon, title, &format!(
                "{pct:+}% vs last month ({} vs {})",
                format_hm_secs(data.this_month),
                format_hm_secs(data.last_month),
            ), false);
        }

        // 4. Preferred time of day — only once there's enough data to be
        //    meaningful (≥10 sessions).
        let (morn, afte, even) = data.hour_buckets;
        let bucket_total = morn + afte + even;
        if bucket_total >= 10 {
            let (label, count) = if morn >= afte && morn >= even {
                ("the morning", morn)
            } else if even >= afte {
                ("the evening", even)
            } else {
                ("the afternoon", afte)
            };
            let pct = (count as f64 / bucket_total as f64 * 100.0).round() as i32;
            self.append_insight("◔", "Preferred time", &format!(
                "{pct}% of sessions are in {label}"
            ), false);
        }

        // 5. Typical session length (median) — only after 5+ sessions so it
        //    stops being dominated by the first few outliers.
        if data.session_count >= 5 && data.typical > 0 {
            self.append_insight("≈", "Typical session", &format!(
                "About {}", format_hm_secs(data.typical),
            ), false);
        }

        // 6. Longest session ever.
        if let Some((dur, start)) = data.longest {
            let when = glib::DateTime::from_unix_local(start).ok()
                .and_then(|d| d.format("%b %-d").ok())
                .map(|s| s.to_string());
            let body = match when {
                Some(d) => format!("{} on {d}", format_hm_secs(dur)),
                None => format_hm_secs(dur),
            };
            self.append_insight("◆", "Longest session", &body, true);
        }

        // 7. Next milestone — only if the user has a few sessions under
        //    their belt (otherwise "12 until your 5th" feels patronising).
        if data.session_count >= 5 {
            if let Some((target, remaining)) = next_session_milestone(data.session_count) {
                let body = if remaining == 1 {
                    format!("1 session to your {target}th")
                } else {
                    format!("{remaining} sessions to your {target}th")
                };
                self.append_insight("⚑", "Next milestone", &body, false);
            }
        }

        // 8. Daily rhythm (average over last 7 days) — complements typical
        //    session by including zero-days.
        if data.avg_secs > 0 {
            self.append_insight("◷", "Daily rhythm", &format!(
                "{} average over last 7 days",
                format_hm_secs(data.avg_secs),
            ), false);
        }

        // Fallback when there's no data at all.
        if self.insights_list.first_child().is_none() {
            self.append_insight("✦", "No sessions yet",
                "Complete a meditation to start seeing insights here.", false);
        }
    }

    fn append_insight(&self, icon: &str, title: &str, body: &str, accent: bool) {
        let row = adw::ActionRow::builder()
            .title(title)
            .subtitle(body)
            .activatable(false)
            .build();
        let mut classes = vec!["insight-icon"];
        if accent { classes.push("accent"); }
        // xalign / yalign position the glyph *inside* the label's box;
        // halign / valign only position the label inside its parent. We
        // need both for a visibly centred glyph.
        let bubble = gtk::Label::builder()
            .label(icon)
            .css_classes(classes)
            .width_request(28)
            .height_request(28)
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Center)
            .xalign(0.5)
            .yalign(0.5)
            .build();
        row.add_prefix(&bubble);
        self.insights_list.append(&row);
    }

    fn reload_chart(&self) {
        let days = self.current_chart_days();

        let today = glib::DateTime::now_local().unwrap();
        let since = today
            .add_days(-(days as i32 - 1))
            .unwrap()
            .format("%Y-%m-%d").unwrap()
            .to_string();

        let sparse = self
            .get_app()
            .and_then(|app| app.with_db(|db| db.get_daily_totals(&since)))
            .and_then(|r| r.ok())
            .unwrap_or_default();
        let sparse_map: std::collections::HashMap<String, i64> =
            sparse.into_iter().collect();

        let daily: Vec<(String, i64)> = (0..days as i64)
            .map(|i| {
                let dt = today.add_days(-(days as i32 - 1) + i as i32).unwrap();
                let date_str = dt.format("%Y-%m-%d").unwrap().to_string();
                let dur = sparse_map.get(&date_str).copied().unwrap_or(0);
                (date_str, dur)
            })
            .collect();

        // Aggregate: monthly for 1 year, weekly for 3 months
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

        while let Some(child) = self.chart_container.first_child() {
            self.chart_container.remove(&child);
        }

        let bars_h = 120i32;
        let chart_h = bars_h as f64;
        let max_val = data.iter().map(|(_, d)| *d).max().unwrap_or(0).max(1);

        // Y-axis with max and midpoint labels
        let y_axis = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .width_request(46)
            .height_request(bars_h)
            .valign(gtk::Align::Start)
            .build();
        y_axis.append(&axis_label(format_hm_secs(max_val)));
        y_axis.append(&gtk::Box::builder().vexpand(true).build());
        y_axis.append(&axis_label(format_hm_secs(max_val / 2)));
        y_axis.append(&gtk::Box::builder().vexpand(true).build());

        // Plot area — one DrawingArea that can render bars or a line
        // depending on the chart_line_btn state. We snapshot the data +
        // max + mode into the closure so toggling triggers a full
        // reload and a fresh closure.
        let plot = gtk::DrawingArea::builder()
            .height_request(bars_h)
            .hexpand(true)
            .build();
        let is_line = self.chart_line_btn.is_active();
        let values: Vec<i64> = data.iter().map(|(_, v)| *v).collect();
        let max_snap = max_val;
        plot.set_draw_func(move |_, cr, w, h| {
            draw_chart_plot(cr, w, h, &values, max_snap, is_line);
        });
        let _ = chart_h;

        let xlabels_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .hexpand(true)
            .spacing(2)
            .build();
        for (i, _) in data.iter().enumerate() {
            xlabels_box.append(
                &gtk::Label::builder()
                    .label(x_label_text(&data, i, days))
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
        right_area.append(&plot);
        right_area.append(&xlabels_box);

        self.chart_container.append(&y_axis);
        self.chart_container.append(&right_area);
    }

    fn reload_mini_stats(&self) {
        let Some(app) = self.get_app() else { return; };
        let (streak, total, sessions) = app
            .with_db(|db| {
                let streak = db.get_best_streak().unwrap_or(0);
                let total  = db.get_total_duration_secs().unwrap_or(0);
                let count  = db.get_session_count().unwrap_or(0);
                (streak, total, count)
            })
            .unwrap_or((0, 0, 0));

        self.mini_streak_value.set_label(
            &if streak == 0 { "–".to_string() } else { format!("{streak}d") }
        );
        self.mini_total_value.set_label(&format_hm_secs(total));
        self.mini_sessions_value.set_label(
            &if sessions == 0 { "–".to_string() } else { sessions.to_string() }
        );
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

#[derive(Default)]
struct InsightData {
    current_streak: u32,
    best_streak:    u32,
    this_month:     i64,
    last_month:     i64,
    daily_totals:   Vec<(String, i64)>,
    longest:        Option<(i64, i64)>,
    typical:        i64,
    avg_secs:       i64,
    hour_buckets:   (i64, i64, i64),
    session_count:  i64,
}

/// Returns (seconds in the current rolling 7-day window,
/// seconds in the previous 7-day window). `daily_totals` is the sparse
/// list of (`YYYY-MM-DD`, secs) we already fetched for the last 14 days.
fn week_over_week(daily_totals: &[(String, i64)], now: &glib::DateTime) -> (i64, i64) {
    use std::collections::HashMap;
    let map: HashMap<&str, i64> =
        daily_totals.iter().map(|(k, v)| (k.as_str(), *v)).collect();
    let mut this_week = 0i64;
    let mut last_week = 0i64;
    for i in 0..14 {
        let Ok(dt) = now.add_days(-i) else { continue; };
        let Ok(s) = dt.format("%Y-%m-%d") else { continue; };
        let secs = map.get(s.as_str()).copied().unwrap_or(0);
        if i < 7 { this_week += secs; } else { last_week += secs; }
    }
    (this_week, last_week)
}

/// Next round-number session count above `current`. None once past 5000.
fn next_session_milestone(current: i64) -> Option<(i64, i64)> {
    const TARGETS: &[i64] = &[10, 25, 50, 100, 250, 500, 1000, 2500, 5000];
    TARGETS.iter()
        .copied()
        .find(|&t| t > current)
        .map(|t| (t, t - current))
}

fn axis_label(text: String) -> gtk::Label {
    gtk::Label::builder()
        .label(text)
        .css_classes(["caption", "dim-label"])
        .halign(gtk::Align::Start)
        .build()
}

/// Map a daily session total to a heatmap shade (0..=4) as a fraction of
/// the user's daily share of their weekly goal.
///
///   0      → empty
///   1..33  → minor    (level 1)
///   33..80 → partial  (level 2)
///   80..120→ on-target (level 3)
///   ≥120   → over     (level 4)
///
/// Retreat days clip at level 4, so a single 10-hour Sunday doesn't dilute
/// the colouring of the rest of the week.
fn minutes_to_level(mins: i64, daily_expected_mins: i64) -> u8 {
    if mins <= 0 { return 0; }
    if daily_expected_mins <= 0 { return 4; }
    let pct = mins.saturating_mul(100) / daily_expected_mins;
    match pct {
        0..=32   => 1,
        33..=79  => 2,
        80..=119 => 3,
        _        => 4,
    }
}

/// Render the daily/weekly/monthly data either as bars or as a filled
/// line chart. `values` has one entry per slot along the x-axis.
fn draw_chart_plot(cr: &cairo::Context, w: i32, h: i32, values: &[i64], max_val: i64, is_line: bool) {
    let n = values.len();
    if n == 0 || max_val == 0 { return; }

    let w_f = w as f64;
    let h_f = h as f64;
    let accent = adw::StyleManager::default().accent_color_rgba();
    let (ar, ag, ab) = (
        accent.red()   as f64,
        accent.green() as f64,
        accent.blue()  as f64,
    );
    let slot_w = w_f / n as f64;

    if is_line {
        // Points: centre x of each slot, y inverted from ratio.
        let points: Vec<(f64, f64)> = values.iter().enumerate().map(|(i, v)| {
            let x = slot_w * (i as f64 + 0.5);
            let ratio = (*v as f64 / max_val as f64).min(1.0);
            let y = h_f - ratio * h_f;
            (x, y)
        }).collect();

        // Soft area fill under the line.
        cr.set_source_rgba(ar, ag, ab, 0.18);
        cr.move_to(points[0].0, h_f);
        for (x, y) in &points { cr.line_to(*x, *y); }
        cr.line_to(points[n - 1].0, h_f);
        cr.close_path();
        let _ = cr.fill();

        // Stroked line on top.
        cr.set_source_rgba(ar, ag, ab, 1.0);
        cr.set_line_width(2.0);
        cr.set_line_cap(cairo::LineCap::Round);
        cr.set_line_join(cairo::LineJoin::Round);
        cr.move_to(points[0].0, points[0].1);
        for (x, y) in &points[1..] { cr.line_to(*x, *y); }
        let _ = cr.stroke();

        // Dots at each data point.
        for (x, y) in &points {
            cr.arc(*x, *y, 2.2, 0.0, std::f64::consts::PI * 2.0);
            let _ = cr.fill();
        }
    } else {
        // Bars: 70% of slot width, centred, rounded top corners.
        let gutter = slot_w * 0.15;
        let bar_w = (slot_w - gutter * 2.0).max(1.0);
        let corner_r = (bar_w * 0.2).min(3.0);
        cr.set_source_rgba(ar, ag, ab, 1.0);
        for (i, v) in values.iter().enumerate() {
            if *v == 0 { continue; }
            let ratio = (*v as f64 / max_val as f64).min(1.0);
            let bar_h = (ratio * h_f).max(3.0);
            let x = slot_w * i as f64 + gutter;
            let y = h_f - bar_h;
            // Path: rounded top, square bottom.
            cr.new_sub_path();
            cr.arc(x + corner_r, y + corner_r, corner_r,
                   std::f64::consts::PI, 1.5 * std::f64::consts::PI);
            cr.line_to(x + bar_w - corner_r, y);
            cr.arc(x + bar_w - corner_r, y + corner_r, corner_r,
                   1.5 * std::f64::consts::PI, 2.0 * std::f64::consts::PI);
            cr.line_to(x + bar_w, y + bar_h);
            cr.line_to(x, y + bar_h);
            cr.close_path();
            let _ = cr.fill();
        }
    }
}

fn draw_goal_ring(area: &gtk::DrawingArea, cr: &cairo::Context, w: i32, h: i32, pct: f64) {
    use std::f64::consts::PI;
    let stroke = 8.0f64;
    let size = w.min(h) as f64;
    let r = (size - stroke) / 2.0;
    let cx = w as f64 / 2.0;
    let cy = h as f64 / 2.0;

    // libadwaita 1.6+ resolves the current accent color for us, honouring
    // the system accent preference set in gnome-control-center.
    let _ = area;
    let accent = adw::StyleManager::default().accent_color_rgba();
    let (fr, fg, fb) = (
        accent.red()   as f64,
        accent.green() as f64,
        accent.blue()  as f64,
    );

    // Background track: same hue, 15% alpha
    cr.set_source_rgba(fr, fg, fb, 0.15);
    cr.set_line_width(stroke);
    cr.set_line_cap(cairo::LineCap::Round);
    cr.arc(cx, cy, r, 0.0, 2.0 * PI);
    let _ = cr.stroke();

    if pct > 0.0 {
        cr.set_source_rgba(fr, fg, fb, 1.0);
        cr.set_line_width(stroke);
        cr.set_line_cap(cairo::LineCap::Round);
        let start = -PI / 2.0;
        let end   = start + 2.0 * PI * pct.min(1.0);
        cr.arc(cx, cy, r, start, end);
        let _ = cr.stroke();
    }
}

/// Returns the x-axis label text for bar `i`.
fn x_label_text(data: &[(String, i64)], i: usize, days: u32) -> String {
    let date_str = &data[i].0;
    let month: u32 = date_str[5..7].parse().unwrap_or(0);
    let day_num: u32 = date_str[8..10].parse().unwrap_or(0);
    match days {
        7 => weekday_for(date_str).to_string(),
        28 => if i % 7 == 0 { format!("{} {}", month_short(month), day_num) } else { String::new() },
        // 3-month and 1-year views: single-letter month when it changes,
        // otherwise the 12 monthly labels in 1Y won't fit at 360 px.
        _ => {
            let prev_month: u32 = if i == 0 { 0 } else { data[i - 1].0[5..7].parse().unwrap_or(0) };
            if month != prev_month { month_letter(month).to_string() } else { String::new() }
        }
    }
}

fn month_letter(month: u32) -> &'static str {
    match month {
        1 => "J", 2 => "F", 3 => "M", 4 => "A",
        5 => "M", 6 => "J", 7 => "J", 8 => "A",
        9 => "S", 10 => "O", 11 => "N", _ => "D",
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

fn month_short(month: u32) -> &'static str {
    match month {
        1 => "Jan", 2 => "Feb", 3 => "Mar", 4 => "Apr",
        5 => "May", 6 => "Jun", 7 => "Jul", 8 => "Aug",
        9 => "Sep", 10 => "Oct", 11 => "Nov", _ => "Dec",
    }
}

fn format_hm_secs(secs: i64) -> String {
    if secs <= 0 { return "–".to_string(); }
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    match (h, m) {
        (0, m) => format!("{m}m"),
        (h, 0) => format!("{h}h"),
        (h, m) => format!("{h}h {m}m"),
    }
}

fn format_hm_mins(mins: i64) -> String {
    if mins <= 0 { return "0m".to_string(); }
    let h = mins / 60;
    let m = mins % 60;
    match (h, m) {
        (0, m) => format!("{m}m"),
        (h, 0) => format!("{h}h"),
        (h, m) => format!("{h}h {m}m"),
    }
}
