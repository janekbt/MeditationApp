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
        // Total time logged since the locale's current-week start. A fresh
        // Monday (in a Monday-start locale) resets the ring to 0.
        let now = crate::time::now_local();
        let week_start = now.add_days(-days_since_week_start(&now)).unwrap();
        let since = week_start.format("%Y-%m-%d").unwrap().to_string();
        let (week_secs, goal_mins) = self.get_app()
            .and_then(|app| app.with_db(|db| {
                let s = db.get_total_secs_since(&since).unwrap_or(0);
                let goal = db.get_setting("weekly_goal_mins", "150")
                    .ok()
                    .and_then(|s| s.parse::<i64>().ok())
                    .filter(|v| *v > 0)
                    .unwrap_or(DEFAULT_WEEKLY_GOAL_MINS);
                (s, goal)
            }))
            .unwrap_or((0, DEFAULT_WEEKLY_GOAL_MINS));
        let week_mins = week_secs / 60;
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
            crate::i18n::gettext("Goal reached ✓ · {duration} this week")
                .replace("{duration}", &format_hm_mins(week_mins))
        } else {
            crate::i18n::gettext("{duration} to go this week")
                .replace("{duration}", &format_hm_mins(remain))
        };
        self.goal_sub_label.set_label(&sub);

        // Accessible name for the Cairo-drawn ring — no intrinsic text for
        // screen readers to fall back on.
        let ring_name = crate::i18n::gettext("Weekly goal: {pct}% — {done} of {goal}")
            .replace("{pct}", &((pct.clamp(0.0, 9.99) * 100.0).round() as i32).to_string())
            .replace("{done}", &format_hm_mins(week_mins))
            .replace("{goal}", &format_hm_mins(goal_mins));
        self.goal_ring.update_property(&[gtk::accessible::Property::Label(&ring_name)]);
    }

    fn reload_contrib_grid(&self) {
        let now = crate::time::now_local();
        // Row 0 = locale's first day of week (Monday, or Sunday on en_US etc.)
        let cur_week_start = now.add_days(-days_since_week_start(&now)).unwrap();

        // Fetch 91 days of totals (12 weeks ago through today) and the
        // user's weekly goal in a single DB borrow.
        let since_dt = cur_week_start.add_days(-12 * 7).unwrap();
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
            let week_start = cur_week_start.add_days(-weeks_ago * 7).unwrap();
            for row in 0..7i32 {
                let date = week_start.add_days(row).unwrap();
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

                // Accessible name — without this the ★ reads as "black star"
                // and empty cells announce nothing useful. %A/%B/%e render
                // through the active locale, so translators only own the
                // sentence framing.
                let readable = date.format("%A, %B %e")
                    .map(|s| s.to_string())
                    .unwrap_or_else(|_| date_str.to_string());
                let name = if level == 4 {
                    crate::i18n::gettext("{date} — goal exceeded, {mins} minutes")
                        .replace("{date}", &readable)
                        .replace("{mins}", &mins.to_string())
                } else if mins > 0 {
                    crate::i18n::gettext("{date} — {mins} minutes")
                        .replace("{date}", &readable)
                        .replace("{mins}", &mins.to_string())
                } else {
                    crate::i18n::gettext("{date} — no sessions")
                        .replace("{date}", &readable)
                };
                cell.update_property(&[gtk::accessible::Property::Label(&name)]);
            }
        }

        // Date-range caption: "<since month> – <current month>". %b
        // renders through the locale's LC_TIME so no msgid is needed.
        let range = format!("{} – {}",
            since_dt.format("%b").map(|s| s.to_string()).unwrap_or_default(),
            now.format("%b").map(|s| s.to_string()).unwrap_or_default(),
        );
        self.contrib_range_label.set_label(&range);
    }

    fn reload_insights(&self) {
        while let Some(c) = self.insights_list.first_child() {
            self.insights_list.remove(&c);
        }

        let Some(app) = self.get_app() else { return; };
        let now = crate::time::now_local();

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

        use crate::i18n::gettext;

        // 1. Current streak — complements the lifetime best shown in mini-stats.
        if data.current_streak > 0 {
            let body = if data.current_streak >= data.best_streak && data.current_streak > 1 {
                gettext("{n} days — new record")
                    .replace("{n}", &data.current_streak.to_string())
            } else if data.best_streak > data.current_streak {
                gettext("{n} days · best was {best}")
                    .replace("{n}", &data.current_streak.to_string())
                    .replace("{best}", &data.best_streak.to_string())
            } else {
                gettext("1 day · keep going")
            };
            let peak = data.current_streak >= data.best_streak && data.current_streak > 1;
            self.append_insight("●", &gettext("Current streak"), &body, peak);
        }

        // 2. This week vs previous 7 days — short-horizon trend.
        let (this_week_secs, last_week_secs) = week_over_week(&data.daily_totals, &now);
        if last_week_secs > 0 {
            let delta = this_week_secs - last_week_secs;
            let pct = (delta as f64 / last_week_secs as f64 * 100.0).round() as i32;
            let (icon, template) = if pct >= 0 {
                ("↗", gettext("{pct}% up vs last week ({this} vs {last})"))
            } else {
                ("↘", gettext("{pct}% down vs last week ({this} vs {last})"))
            };
            let body = template
                .replace("{pct}", &pct.abs().to_string())
                .replace("{this}", &format_hm_secs(this_week_secs))
                .replace("{last}", &format_hm_secs(last_week_secs));
            self.append_insight(icon, &gettext("This week's practice"), &body, false);
        }

        // 3. Trend vs last month.
        if data.last_month > 0 {
            let pct = ((data.this_month - data.last_month) as f64
                / data.last_month as f64 * 100.0).round() as i32;
            let (icon, title) = if pct >= 0 {
                ("↗", gettext("Practising more"))
            } else {
                ("↘", gettext("Practising less"))
            };
            let body = gettext("{pct}% vs last month ({this} vs {last})")
                .replace("{pct}", &format!("{pct:+}"))
                .replace("{this}", &format_hm_secs(data.this_month))
                .replace("{last}", &format_hm_secs(data.last_month));
            self.append_insight(icon, &title, &body, false);
        }

        // 4. Preferred time of day — only once there's enough data to be
        //    meaningful (≥10 sessions).
        let (morn, afte, even) = data.hour_buckets;
        let bucket_total = morn + afte + even;
        if bucket_total >= 10 {
            let (template, count) = if morn >= afte && morn >= even {
                (gettext("{pct}% of sessions are in the morning"), morn)
            } else if even >= afte {
                (gettext("{pct}% of sessions are in the evening"), even)
            } else {
                (gettext("{pct}% of sessions are in the afternoon"), afte)
            };
            let pct = (count as f64 / bucket_total as f64 * 100.0).round() as i32;
            let body = template.replace("{pct}", &pct.to_string());
            self.append_insight("◔", &gettext("Preferred time"), &body, false);
        }

        // 5. Typical session length (median) — only after 5+ sessions so it
        //    stops being dominated by the first few outliers.
        if data.session_count >= 5 && data.typical > 0 {
            let body = gettext("About {duration}")
                .replace("{duration}", &format_hm_secs(data.typical));
            self.append_insight("≈", &gettext("Typical session"), &body, false);
        }

        // 6. Longest session ever.
        if let Some((dur, start)) = data.longest {
            let when = glib::DateTime::from_unix_local(start).ok()
                .and_then(|d| d.format("%b %-d").ok())
                .map(|s| s.to_string());
            let body = match when {
                Some(d) => gettext("{duration} on {date}")
                    .replace("{duration}", &format_hm_secs(dur))
                    .replace("{date}", &d),
                None => format_hm_secs(dur),
            };
            self.append_insight("◆", &gettext("Longest session"), &body, true);
        }

        // 7. Next milestone — only if the user has a few sessions under
        //    their belt (otherwise "12 until your 5th" feels patronising).
        if data.session_count >= 5 {
            if let Some((target, remaining)) = next_session_milestone(data.session_count) {
                let body = if remaining == 1 {
                    gettext("1 session to your {target}th")
                        .replace("{target}", &target.to_string())
                } else {
                    gettext("{n} sessions to your {target}th")
                        .replace("{n}", &remaining.to_string())
                        .replace("{target}", &target.to_string())
                };
                self.append_insight("⚑", &gettext("Next milestone"), &body, false);
            }
        }

        // 8. Daily rhythm (average over last 7 days) — complements typical
        //    session by including zero-days.
        if data.avg_secs > 0 {
            let body = gettext("{duration} average over last 7 days")
                .replace("{duration}", &format_hm_secs(data.avg_secs));
            self.append_insight("◷", &gettext("Daily rhythm"), &body, false);
        }

        // Fallback when there's no data at all.
        if self.insights_list.first_child().is_none() {
            self.append_insight(
                "✦",
                &gettext("No sessions yet"),
                &gettext("Complete a meditation to start seeing insights here"),
                false,
            );
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

        let today = crate::time::now_local();
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
                    .css_classes(["caption", "dimmed"])
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
        self.mini_total_value.set_label(&format_hm_compact(total));
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
    pub(crate) fn get_app(&self) -> Option<crate::application::MeditateApplication> {
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

/// First day of the week per the active locale, in GLib's day_of_week
/// numbering (1 = Monday … 7 = Sunday).
///
/// Queries `nl_langinfo(_NL_TIME_FIRST_WEEKDAY)` — a glibc POSIX
/// extension whose returned byte is 1 = Sunday … 7 = Saturday. We
/// translate into GLib's numbering so callers can compare against
/// `GDateTime::day_of_week()` directly. Falls back to Monday if the
/// locale is unset or the call returns a nonsense value.
pub fn locale_week_start_dow() -> i32 {
    // libc-rs doesn't expose glibc-specific _NL_* enumerants as named
    // constants, so we reconstruct the value. _NL_ITEM(category, index)
    // is ((category << 16) | index); on glibc __LC_TIME == 2 and the
    // nl_langinfo.h enum lands _NL_TIME_FIRST_WEEKDAY at index 40 in
    // the LC_TIME block, giving 0x20028 = 131176. Non-glibc libcs
    // don't define this item — nl_langinfo then returns an empty
    // string and we fall back to Monday.
    #[cfg(target_os = "linux")]
    const NL_TIME_FIRST_WEEKDAY: libc::nl_item = 131176;

    #[cfg(not(target_os = "linux"))]
    return 1;

    #[cfg(target_os = "linux")]
    unsafe {
        let ptr = libc::nl_langinfo(NL_TIME_FIRST_WEEKDAY);
        if ptr.is_null() { return 1; }
        let byte = *ptr as u8;
        // 1 = Sun … 7 = Sat (POSIX)  →  1 = Mon … 7 = Sun (GLib)
        match byte {
            1      => 7,              // Sunday
            2..=7  => (byte - 1) as i32,
            _      => 1,              // Unset / empty — default to Monday
        }
    }
}

/// Days between `now` and the most recent start-of-week, inclusive of
/// today. 0 means today is the first day of the week.
fn days_since_week_start(now: &glib::DateTime) -> i32 {
    let today = now.day_of_week();      // 1 = Mon … 7 = Sun
    let start = locale_week_start_dow();
    (today - start + 7) % 7
}

/// Returns (seconds this calendar week so far, seconds in the same
/// portion of last week). Weeks start on the locale's first weekday,
/// matching the goal ring and heatmap. The comparison is apples-to-
/// apples: if it's Wednesday, we compare Mon–Wed to Mon–Wed of the
/// prior week, not a partial week against a full one.
fn week_over_week(daily_totals: &[(String, i64)], now: &glib::DateTime) -> (i64, i64) {
    use std::collections::HashMap;
    let map: HashMap<&str, i64> =
        daily_totals.iter().map(|(k, v)| (k.as_str(), *v)).collect();
    let days_elapsed = days_since_week_start(now) + 1;   // 1..=7
    let sum_range = |start_offset: i32| -> i64 {
        (0..days_elapsed).filter_map(|i| {
            let dt = now.add_days(start_offset - i).ok()?;
            let key = dt.format("%Y-%m-%d").ok()?;
            Some(map.get(key.as_str()).copied().unwrap_or(0))
        }).sum()
    };
    (sum_range(0), sum_range(-7))
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
        .css_classes(["caption", "dimmed"])
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
    // %b is the locale's abbreviated month; auto-translates via LC_TIME.
    let month_short = |m: u32| -> String {
        glib::DateTime::new(&glib::TimeZone::local(), 2000, m as i32, 1, 0, 0, 0.0)
            .ok()
            .and_then(|dt| dt.format("%b").ok())
            .map(|s| s.to_string())
            .unwrap_or_default()
    };
    match days {
        7 => weekday_for(date_str),
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

fn weekday_for(date_str: &str) -> String {
    let y: i32 = date_str[0..4].parse().unwrap_or(2000);
    let m: i32 = date_str[5..7].parse().unwrap_or(1);
    let d: i32 = date_str[8..10].parse().unwrap_or(1);
    // %a is the locale's abbreviated weekday ("Mo"/"Di"/"Mi" on de_DE,
    // "Mon"/"Tue"/… on en_US). Truncate so horizontal labels stay narrow.
    glib::DateTime::new(&glib::TimeZone::local(), y, m, d, 0, 0, 0.0)
        .ok()
        .and_then(|dt| dt.format("%a").ok())
        .map(|s| s.chars().take(2).collect::<String>())
        .unwrap_or_default()
}

/// Compact H/M format for large totals. Drops minutes past 100 h because
/// they're visual noise at that scale and the extra characters force the
/// mini-stat card over the 360 px viewport.
fn format_hm_compact(secs: i64) -> String {
    if secs <= 0 { return "–".to_string(); }
    let h = secs / 3600;
    if h >= 100 { return format!("{h}h"); }
    let m = (secs % 3600) / 60;
    match (h, m) {
        (0, m) => format!("{m}m"),
        (h, 0) => format!("{h}h"),
        (h, m) => format!("{h}h {m}m"),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minutes_to_level_zero_and_negative() {
        assert_eq!(minutes_to_level(0, 30), 0);
        assert_eq!(minutes_to_level(-5, 30), 0);
    }

    #[test]
    fn minutes_to_level_no_goal_clips_to_four() {
        // daily_expected_mins == 0 means the user has no weekly goal; any
        // session minutes should read as "over" (level 4) rather than divide
        // by zero.
        assert_eq!(minutes_to_level(1, 0), 4);
        assert_eq!(minutes_to_level(100, 0), 4);
        assert_eq!(minutes_to_level(1, -1), 4);
    }

    #[test]
    fn minutes_to_level_bands() {
        // daily_expected = 30, so pct = mins * 100 / 30.
        // pct 0..=32 → 1, 33..=79 → 2, 80..=119 → 3, 120+ → 4.
        assert_eq!(minutes_to_level(1, 30), 1);   // pct=3
        assert_eq!(minutes_to_level(9, 30), 1);   // pct=30
        assert_eq!(minutes_to_level(10, 30), 2);  // pct=33 → boundary into 2
        assert_eq!(minutes_to_level(23, 30), 2);  // pct=76
        assert_eq!(minutes_to_level(24, 30), 3);  // pct=80 → boundary into 3
        assert_eq!(minutes_to_level(35, 30), 3);  // pct=116
        assert_eq!(minutes_to_level(36, 30), 4);  // pct=120 → boundary into 4
        assert_eq!(minutes_to_level(600, 30), 4); // retreat day clips
    }

    #[test]
    fn minutes_to_level_saturating_does_not_overflow() {
        // Would overflow an i64 in naive mul, should saturate and still
        // land in the level-4 bucket.
        assert_eq!(minutes_to_level(i64::MAX, 30), 4);
    }

    #[test]
    fn next_session_milestone_boundaries() {
        assert_eq!(next_session_milestone(0), Some((10, 10)));
        assert_eq!(next_session_milestone(9), Some((10, 1)));
        assert_eq!(next_session_milestone(10), Some((25, 15)));
        assert_eq!(next_session_milestone(24), Some((25, 1)));
        assert_eq!(next_session_milestone(499), Some((500, 1)));
        assert_eq!(next_session_milestone(2499), Some((2500, 1)));
        assert_eq!(next_session_milestone(4999), Some((5000, 1)));
    }

    #[test]
    fn next_session_milestone_past_ceiling() {
        assert_eq!(next_session_milestone(5000), None);
        assert_eq!(next_session_milestone(5001), None);
        assert_eq!(next_session_milestone(10_000), None);
    }

    #[test]
    fn format_hm_compact_zero_negative() {
        assert_eq!(format_hm_compact(0), "–");
        assert_eq!(format_hm_compact(-1), "–");
    }

    #[test]
    fn format_hm_compact_shapes() {
        assert_eq!(format_hm_compact(30 * 60), "30m");
        assert_eq!(format_hm_compact(3600), "1h");
        assert_eq!(format_hm_compact(3600 + 30 * 60), "1h 30m");
        assert_eq!(format_hm_compact(59 * 60), "59m");
    }

    #[test]
    fn format_hm_compact_drops_minutes_past_100h() {
        // Below the threshold: minutes still show.
        assert_eq!(format_hm_compact(99 * 3600 + 59 * 60), "99h 59m");
        // At and above the threshold: minutes are dropped.
        assert_eq!(format_hm_compact(100 * 3600), "100h");
        assert_eq!(format_hm_compact(100 * 3600 + 59 * 60), "100h");
        assert_eq!(format_hm_compact(500 * 3600 + 42 * 60), "500h");
    }

    #[test]
    fn format_hm_secs_no_clipping() {
        assert_eq!(format_hm_secs(0), "–");
        assert_eq!(format_hm_secs(-10), "–");
        assert_eq!(format_hm_secs(30 * 60), "30m");
        assert_eq!(format_hm_secs(3600), "1h");
        assert_eq!(format_hm_secs(3600 + 30 * 60), "1h 30m");
        // Unlike format_hm_compact, no clip at 100h.
        assert_eq!(format_hm_secs(100 * 3600 + 42 * 60), "100h 42m");
    }

    #[test]
    fn format_hm_mins_zero_case_is_literal_zero() {
        // Deliberate asymmetry: format_hm_mins returns "0m" for zero/negative,
        // while the seconds-based formatters return "–". Don't regress this.
        assert_eq!(format_hm_mins(0), "0m");
        assert_eq!(format_hm_mins(-3), "0m");
    }

    #[test]
    fn format_hm_mins_shapes() {
        assert_eq!(format_hm_mins(30), "30m");
        assert_eq!(format_hm_mins(59), "59m");
        assert_eq!(format_hm_mins(60), "1h");
        assert_eq!(format_hm_mins(90), "1h 30m");
        assert_eq!(format_hm_mins(24 * 60), "24h");
    }
}
