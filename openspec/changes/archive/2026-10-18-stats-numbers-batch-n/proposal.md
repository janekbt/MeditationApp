# Stats numbers agree (batch N)

## Why
The Stats page and the Timer disagree with each other and with the Log
(#55, #59, #60, #114, #115). A 19:30 session reads "20 min" in the Log
but leaves "1 min to go" on the goal ring; every Monday morning the week
card shows -100%; the year chart has two half Octobers; a session dated
tomorrow stretches the best streak; the tile captioned "Streak" shows
the best streak while the Timer shows the current one.

## What Changes
- #55: goal ring, heatmap cells, the Total tile, by-label rows, insights
  and the chart axis round to the nearest minute, like the Log. Under
  30 s still reads as nothing.
- #59: the week card leaves today out until today has practice; on the
  first day of the week without practice it is not shown.
- #60: the year chart covers 12 calendar months, this month included,
  starting on the 1st. One core function builds every chart series;
  both apps call it.
- #114: the best streak ignores days after today.
- #115: the tile shows the best streak under the caption "Best streak";
  the caption wraps if a language needs two lines.
- Left out: the 30-day trend counts today's part too (1 day in 30, a
  small error); the 3-month view's weeks stay rolling, not calendar weeks.

## Decisions
- #115: keep the best streak in the tile, caption it "Best streak"
  (Janek, 2026-10-18).
- #59: today counts once it has practice; until then both weeks are
  compared through yesterday (Janek, 2026-10-18).
- The year window starts on the 1st of the month 11 months back, so the
  current month is the last, partial bar (my call).
- The tile caption wraps instead of being shortened per language (my call).
- Nearest-minute rounding everywhere a total turns into minutes; a Log
  day of three 20 s sessions still reads 3 min in the Log and 1 min on
  the ring, because the Log floors each card at 1 min (my call; that is
  the Log's own rule).

## Capabilities
### Modified Capabilities
- `stats`: rounding, week card, year chart, best streak, tile caption.

## Impact
- Core: `goal.rs`, `contrib.rs`, `format.rs` (`hm_compact_key`,
  `hm_mins_key`, `hm_secs_key`), `date_math.rs` (`week_over_week`, new
  `chart_series` and `ChartPeriod::window_start`), `db/sessions.rs`
  (`best_streak` takes today), `insights.rs`.
- GTK: `stats/imp.rs` (`reload_chart`, mini stats), `db/mod.rs`,
  `data/ui/stats_view.blp` (caption, wrap).
- Android: `src/ui.rs` (`refresh_chart`, `refresh_stats`),
  `ui/main.slint` (caption, wrap).
- Translations: new msgid "Best streak" in both apps, nine languages
  each; "Streak" is dropped where it was only the tile caption.
- Data: none.
- Android build environment: untouched.
- Docs: OPEN-ISSUES.md (remove the five, tick batch N); the stats spec
  through this delta.
