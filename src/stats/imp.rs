use std::cell::{Cell, RefCell};
use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::{glib, cairo, CompositeTemplate};


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
    #[template_child] pub period_toggle_group:  TemplateChild<adw::ToggleGroup>,
    #[template_child] pub chart_kind_toggle:    TemplateChild<adw::ToggleGroup>,
    #[template_child] pub chart_container:      TemplateChild<gtk::Box>,
    // Mini-stats
    #[template_child] pub mini_streak_value:    TemplateChild<gtk::Label>,
    #[template_child] pub mini_total_value:     TemplateChild<gtk::Label>,
    #[template_child] pub mini_sessions_value:  TemplateChild<gtk::Label>,
    // By-label breakdown
    #[template_child] pub label_totals_section: TemplateChild<gtk::Box>,
    #[template_child] pub label_totals_list:    TemplateChild<gtk::ListBox>,

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
        let reload = glib::clone!(
            #[weak(rename_to = this)] obj,
            move |_: &adw::ToggleGroup| this.imp().reload_chart()
        );
        self.period_toggle_group.connect_active_name_notify(reload.clone());
        self.chart_kind_toggle.connect_active_name_notify(reload);
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
        self.reload_label_totals();
    }

    fn reload_goal_ring(&self) {
        // Total time logged since the locale's current-week start. A fresh
        // Monday (in a Monday-start locale) resets the ring to 0.
        let now = crate::time::now_local();
        let week_start = now.add_days(-meditate_core::date_math::days_since_week_start(now.day_of_week(), locale_week_start_dow())).unwrap();
        let since = week_start.format("%Y-%m-%d").unwrap().to_string();
        let (week_secs, goal_mins) = self.get_app()
            .and_then(|app| app.with_db(|db| {
                let s = db.get_total_secs_since(&since).unwrap_or(0);
                let goal = meditate_core::goal::weekly_goal_mins_from_db(db.core());
                (s, goal)
            }))
            .unwrap_or((0, meditate_core::goal::WEEKLY_GOAL_DEFAULT));
        let g = meditate_core::goal::compute(week_secs, goal_mins);
        self.goal_pct.set(g.arc_pct);
        self.goal_ring.queue_draw();
        self.goal_pct_label.set_label(&format!("{}%", g.display_pct));

        // "1h 48m / 2h 30m"
        self.goal_progress_label.set_markup(&format!(
            "{} <span alpha=\"60%\" size=\"60%\">/ {}</span>",
            format_hm_mins(g.week_mins),
            format_hm_mins(g.goal_mins),
        ));
        let sub = match g.status {
            GoalStatus::Reached => crate::i18n::gettext("Goal reached ✓ · {duration} this week")
                .replace("{duration}", &format_hm_mins(g.week_mins)),
            GoalStatus::InProgress => crate::i18n::gettext("{duration} to go this week")
                .replace("{duration}", &format_hm_mins(g.remaining_mins)),
        };
        self.goal_sub_label.set_label(&sub);

        // Accessible name for the Cairo-drawn ring — no intrinsic text for
        // screen readers to fall back on.
        let ring_name = crate::i18n::gettext("Weekly goal: {pct}% — {done} of {goal}")
            .replace("{pct}", &g.display_pct.to_string())
            .replace("{done}", &format_hm_mins(g.week_mins))
            .replace("{goal}", &format_hm_mins(g.goal_mins));
        self.goal_ring.update_property(&[gtk::accessible::Property::Label(&ring_name)]);
    }

    fn reload_contrib_grid(&self) {
        let now = crate::time::now_local();

        // Fetch 91 days of totals (12 weeks back through today) and
        // the user's weekly goal in a single DB borrow. Core's
        // `get_daily_totals` returns NaiveDate keys directly.
        let (totals_vec, goal_mins) = self.get_app()
            .and_then(|app| app.with_db(|db| {
                let t = meditate_core::db::get_daily_totals_from_db(db.core()).unwrap_or_default();
                let g = meditate_core::goal::weekly_goal_mins_from_db(db.core());
                (t, g)
            }))
            .unwrap_or_else(|| (Vec::new(), meditate_core::goal::WEEKLY_GOAL_DEFAULT));
        let totals: std::collections::HashMap<chrono::NaiveDate, i64> =
            totals_vec.into_iter().collect();
        let daily_expected_mins = meditate_core::goal::daily_expected_mins(goal_mins);

        // Core owns the cell classification (future / today / past +
        // level dispatch). Shell only renders.
        let today_naive = meditate_core::time::today_local();
        let core_cells = meditate_core::contrib::build_grid(
            today_naive,
            locale_week_start_dow(),
            &totals,
            daily_expected_mins,
        );

        let cells = self.contrib_cells.borrow();
        for (idx, c) in core_cells.iter().enumerate() {
            let cell = &cells[idx];
            for l in 0..=4 { cell.remove_css_class(&format!("level-{l}")); }
            cell.remove_css_class("today");
            cell.set_label("");

            if c.is_future {
                cell.add_css_class("level-0");
                cell.set_opacity(0.3);
                continue;
            }
            cell.set_opacity(1.0);
            cell.add_css_class(&format!("level-{}", c.level));
            // ★ only for days that exceed the daily goal by 20 % or more.
            // On-target days rely on colour intensity alone — a wall of
            // glyphs in a 13×7 grid blurs together and dilutes the signal.
            if c.is_goal_exceeded() { cell.set_label("★"); }
            if c.is_today { cell.add_css_class("today"); }

            // Accessible name — without this the ★ reads as "black star"
            // and empty cells announce nothing useful. %A/%B/%e render
            // through the active locale via glib::DateTime, so
            // translators only own the sentence framing.
            let date_dt = crate::time::glib_datetime_from_iso(&c.date_iso);
            let readable = date_dt
                .as_ref()
                .and_then(|d| d.format("%A, %B %e").ok()).map_or_else(|| c.date_iso.clone(), |s| s.to_string());
            let name = if c.is_goal_exceeded() {
                crate::i18n::gettext("{date} — goal exceeded, {mins} minutes")
                    .replace("{date}", &readable)
                    .replace("{mins}", &c.mins.to_string())
            } else if c.mins > 0 {
                crate::i18n::gettext("{date} — {mins} minutes")
                    .replace("{date}", &readable)
                    .replace("{mins}", &c.mins.to_string())
            } else {
                crate::i18n::gettext("{date} — no sessions")
                    .replace("{date}", &readable)
            };
            cell.update_property(&[gtk::accessible::Property::Label(&name)]);
        }

        // Date-range caption: "<oldest month> – <current month>". %b
        // renders through the locale's LC_TIME so no msgid is needed.
        // The oldest cell's date_iso anchors the left edge.
        let since_dt = core_cells.first()
            .and_then(|c| crate::time::glib_datetime_from_iso(&c.date_iso));
        let range = format!("{} – {}",
            since_dt.as_ref()
                .and_then(|d| d.format("%b").ok())
                .map(|s| s.to_string())
                .unwrap_or_default(),
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
            meditate_core::insights::InsightInput {
                current_streak:  db.get_streak().unwrap_or(0),
                best_streak:     db.get_best_streak().unwrap_or(0),
                this_month_secs: db.month_total_secs(ty, tm).unwrap_or(0),
                last_month_secs: db.month_total_secs(ly, lm).unwrap_or(0),
                daily_totals:    db.get_daily_totals(&fourteen_since).unwrap_or_default(),
                longest:         db.get_longest_session().unwrap_or(None),
                typical_secs:    db.get_median_duration_secs().unwrap_or(None).unwrap_or(0),
                avg_secs_7d:     db.get_running_average_secs(7).unwrap_or(0.0) as i64,
                hour_buckets:    db.hour_buckets().unwrap_or((0, 0, 0)),
                session_count:   db.count_sessions().unwrap_or(0),
            }
        }).unwrap_or_default();

        let keys = meditate_core::insights::compute(
            &data,
            meditate_core::time::unix_now(),
            locale_week_start_dow(),
        );
        for k in keys {
            self.render_insight(k);
        }
    }

    /// Map a `meditate_core::insights::InsightKey` variant to the
    /// gtk shell's gettext-translated card. The portable decision
    /// lives in core; locale-aware strings + glib::DateTime month
    /// formatting stay here.
    fn render_insight(&self, key: meditate_core::insights::InsightKey) {
        use crate::i18n::{gettext, ngettext};
        use meditate_core::insights::{HourBucket, InsightKey};
        let glyph = key.glyph();
        let accent = key.is_accent();
        let (title, body) = match &key {
            InsightKey::CurrentStreak { days, is_record, best } => {
                let body = if *is_record {
                    ngettext("1 day — new record", "{n} days — new record", *days )
                        .replace("{n}", &days.to_string())
                } else if *best > *days {
                    ngettext(
                        "1 day · best was {best}",
                        "{n} days · best was {best}",
                        *days ,
                    )
                        .replace("{n}", &days.to_string())
                        .replace("{best}", &best.to_string())
                } else {
                    gettext("1 day · keep going")
                };
                (gettext("Current streak"), body)
            }
            InsightKey::WeekOverWeek { pct, this_secs, last_secs } => {
                let template = if *pct >= 0 {
                    gettext("{pct}% up vs last week ({this} vs {last})")
                } else {
                    gettext("{pct}% down vs last week ({this} vs {last})")
                };
                let body = template
                    .replace("{pct}", &pct.abs().to_string())
                    .replace("{this}", &format_hm_secs(*this_secs))
                    .replace("{last}", &format_hm_secs(*last_secs));
                (gettext("This week's practice"), body)
            }
            InsightKey::MonthTrend { pct, this_secs, last_secs } => {
                let title = if *pct >= 0 {
                    gettext("Practising more")
                } else {
                    gettext("Practising less")
                };
                let body = gettext("{pct}% vs last month ({this} vs {last})")
                    .replace("{pct}", &format!("{pct:+}"))
                    .replace("{this}", &format_hm_secs(*this_secs))
                    .replace("{last}", &format_hm_secs(*last_secs));
                (title, body)
            }
            InsightKey::PreferredTime { bucket, pct } => {
                let template = match bucket {
                    HourBucket::Morning => gettext("{pct}% of sessions are in the morning"),
                    HourBucket::Afternoon => gettext("{pct}% of sessions are in the afternoon"),
                    HourBucket::Evening => gettext("{pct}% of sessions are in the evening"),
                };
                (gettext("Preferred time"), template.replace("{pct}", &pct.to_string()))
            }
            InsightKey::TypicalSession { duration_secs } => {
                let body = gettext("About {duration}")
                    .replace("{duration}", &format_hm_secs(*duration_secs));
                (gettext("Typical session"), body)
            }
            InsightKey::LongestSession { duration_secs, start_unix } => {
                let when = glib::DateTime::from_unix_local(*start_unix).ok()
                    .and_then(|d| d.format("%b %-d").ok())
                    .map(|s| s.to_string());
                let body = match when {
                    Some(d) => gettext("{duration} on {date}")
                        .replace("{duration}", &format_hm_secs(*duration_secs))
                        .replace("{date}", &d),
                    None => format_hm_secs(*duration_secs),
                };
                (gettext("Longest session"), body)
            }
            InsightKey::NextMilestone { target, remaining } => {
                let body = ngettext(
                    "1 session to your {target}th",
                    "{n} sessions to your {target}th",
                    *remaining as u32,
                )
                    .replace("{n}", &remaining.to_string())
                    .replace("{target}", &target.to_string());
                (gettext("Next milestone"), body)
            }
            InsightKey::DailyRhythm { avg_secs } => {
                let body = gettext("{duration} average over last 7 days")
                    .replace("{duration}", &format_hm_secs(*avg_secs));
                (gettext("Daily rhythm"), body)
            }
            InsightKey::NoData => (
                gettext("No sessions yet"),
                gettext("Complete a meditation to start seeing insights here"),
            ),
        };
        self.append_insight(glyph, &title, &body, accent);
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
            .and_then(std::result::Result::ok)
            .unwrap_or_default();
        let sparse_map: std::collections::HashMap<String, i64> =
            sparse.into_iter().collect();

        let daily: Vec<(String, i64)> = (0..i64::from(days))
            .map(|i| {
                let dt = today.add_days(-(days as i32 - 1) + i as i32).unwrap();
                let date_str = dt.format("%Y-%m-%d").unwrap().to_string();
                let dur = sparse_map.get(&date_str).copied().unwrap_or(0);
                (date_str, dur)
            })
            .collect();

        // Aggregate (monthly for 1y, weekly for 3m, else daily) lives in core.
        let data = meditate_core::date_math::aggregate_for_chart_period(&daily, days);

        while let Some(child) = self.chart_container.first_child() {
            self.chart_container.remove(&child);
        }

        let bars_h = 120i32;
        let chart_h = f64::from(bars_h);
        let series: Vec<i64> = data.iter().map(|(_, d)| *d).collect();
        let ticks = meditate_core::date_math::chart_y_axis_ticks(&series);
        let max_val = ticks.max;

        // Y-axis with max and midpoint labels
        let y_axis = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .width_request(46)
            .height_request(bars_h)
            .valign(gtk::Align::Start)
            .build();
        y_axis.append(&axis_label(format_hm_secs(ticks.max)));
        y_axis.append(&gtk::Box::builder().vexpand(true).build());
        y_axis.append(&axis_label(format_hm_secs(ticks.mid)));
        y_axis.append(&gtk::Box::builder().vexpand(true).build());

        // Plot area — one DrawingArea that can render bars or a line
        // depending on the chart_line_btn state. We snapshot the data +
        // max + mode into the closure so toggling triggers a full
        // reload and a fresh closure.
        let plot = gtk::DrawingArea::builder()
            .height_request(bars_h)
            .hexpand(true)
            .build();
        let is_line = self.chart_kind_toggle.active_name().as_deref() == Some("line");
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
                let total  = db.total_seconds().unwrap_or(0);
                let count  = db.count_sessions().unwrap_or(0);
                (streak, total, count)
            })
            .unwrap_or((0, 0, 0));

        self.mini_streak_value.set_label(
            &if streak == 0 { "–".to_string() } else { format!("{streak}d") }
        );
        self.mini_total_value.set_label(&format_hm_compact(total));
        self.mini_sessions_value.set_label(
            &meditate_core::format::mini_stat_or_dash(sessions),
        );
    }

    fn reload_label_totals(&self) {
        // Clear any prior rows — refresh is called on every tab entry and
        // after label rename/delete, so we can't count on stability.
        while let Some(c) = self.label_totals_list.first_child() {
            self.label_totals_list.remove(&c);
        }

        let totals = self.get_app()
            .and_then(|app| app.with_db(|db| db.get_label_totals().unwrap_or_default()))
            .unwrap_or_default();

        if totals.is_empty() {
            // No labeled sessions yet — hide the section entirely rather
            // than show an empty list.
            self.label_totals_section.set_visible(false);
            return;
        }
        self.label_totals_section.set_visible(true);

        for (name, total_secs, n) in totals {
            // Two-form plural split, matching preferences.rs's
            // pluralize_sessions — catalogs carry both msgids.
            let subtitle = if n == 1 {
                crate::i18n::gettext("{duration} · 1 session")
                    .replace("{duration}", &format_hm_secs(total_secs))
            } else {
                crate::i18n::gettext("{duration} · {count} sessions")
                    .replace("{duration}", &format_hm_secs(total_secs))
                    .replace("{count}", &n.to_string())
            };
            let row = adw::ActionRow::builder()
                .title(&name)
                .subtitle(&subtitle)
                .activatable(false)
                .build();
            self.label_totals_list.append(&row);
        }
    }

    fn current_chart_days(&self) -> u32 {
        let name = self.period_toggle_group.active_name();
        meditate_core::date_math::ChartPeriod::from_db_str(
            name.as_deref().unwrap_or(""),
        )
        .days()
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


/// First day of the week per the active locale (1=Mon..7=Sun). Pure
/// libc bridge — delegates to core so the Android shell inherits
/// the same detection (with its own fallback for bionic).
pub fn locale_week_start_dow() -> i32 {
    meditate_core::date_math::locale_week_start_dow()
}

fn axis_label(text: String) -> gtk::Label {
    gtk::Label::builder()
        .label(text)
        .css_classes(["caption", "dimmed"])
        .halign(gtk::Align::Start)
        .build()
}


/// Render the daily/weekly/monthly data either as bars or as a filled
/// line chart. `values` has one entry per slot along the x-axis.
fn draw_chart_plot(cr: &cairo::Context, w: i32, h: i32, values: &[i64], max_val: i64, is_line: bool) {
    let n = values.len();
    if n == 0 || max_val == 0 { return; }

    let w_f = f64::from(w);
    let h_f = f64::from(h);
    let accent = adw::StyleManager::default().accent_color_rgba();
    let (ar, ag, ab) = (
        f64::from(accent.red()),
        f64::from(accent.green()),
        f64::from(accent.blue()),
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
    let size = f64::from(w.min(h));
    let r = (size - stroke) / 2.0;
    let cx = f64::from(w) / 2.0;
    let cy = f64::from(h) / 2.0;

    // libadwaita 1.6+ resolves the current accent color for us, honouring
    // the system accent preference set in gnome-control-center.
    let _ = area;
    let accent = adw::StyleManager::default().accent_color_rgba();
    let (fr, fg, fb) = (
        f64::from(accent.red()),
        f64::from(accent.green()),
        f64::from(accent.blue()),
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

/// Returns the x-axis label text for bar `i`. The decision (which
/// kind of label to render) lives in core; gtk just dispatches.
fn x_label_text(data: &[(String, i64)], i: usize, days: u32) -> String {
    use meditate_core::date_math::XLabelKind;
    let months: Vec<u32> = data
        .iter()
        .map(|(d, _)| d[5..7].parse().unwrap_or(0))
        .collect();
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
    match meditate_core::date_math::x_label_kind(i, days, &months) {
        XLabelKind::Weekday => weekday_for(date_str),
        XLabelKind::MonthShortDay => format!("{} {}", month_short(month), day_num),
        XLabelKind::MonthLetter => {
            // First char of the locale's abbreviated month — Japanese
            // "1月" → "1", Russian "Янв" → "Я", English "Jan" → "J".
            // Same pattern as `month_short` above, just truncated.
            month_short(month).chars().next().map(|c| c.to_string()).unwrap_or_default()
        }
        XLabelKind::Empty => String::new(),
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

// HmKey-rendering shims live in `crate::format`; this view imports them.
use crate::format::{format_hm_compact, format_hm_mins, format_hm_secs};
use meditate_core::goal::GoalStatus;

