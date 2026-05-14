//! Shell-side rendering helpers for core's typed format keys.
//!
//! Core (`meditate_core::format`) returns typed enums like `HmKey`
//! that capture every choice without picking a localized template;
//! this module maps each variant to gettext / ngettext strings so
//! `.po` files own the visible copy. The shell shims also handle
//! the i64-secs / i64-mins conversion that call sites use.

use crate::i18n::{gettext, ngettext};
use meditate_core::format::HmKey;
use std::time::Duration;

/// Render a core `HmKey` into the user-visible "1h 4m" / "–" /
/// etc. string. Unit suffixes route through `ngettext` even though
/// English uses the same abbreviation in both forms — that lets a
/// plural-rich locale (`ru`, `pl`, `ar`) supply distinct forms
/// without re-touching the renderer.
pub fn render_hm(key: HmKey) -> String {
    match key {
        HmKey::Empty => "–".to_string(),
        HmKey::MinsOnly(m) => ngettext("{n}m", "{n}m", m as u32)
            .replace("{n}", &m.to_string()),
        HmKey::HoursOnly(h) => ngettext("{n}h", "{n}h", h as u32)
            .replace("{n}", &h.to_string()),
        HmKey::HoursMins(h, m) => {
            let hp = ngettext("{n}h", "{n}h", h as u32)
                .replace("{n}", &h.to_string());
            let mp = ngettext("{n}m", "{n}m", m as u32)
                .replace("{n}", &m.to_string());
            // Separate composition template so a locale can change
            // the order or separator (e.g. "{m} {h}" or "{h}-{m}")
            // without touching the per-unit strings.
            gettext("{h} {m}")
                .replace("{h}", &hp)
                .replace("{m}", &mp)
        }
    }
}

fn secs_to_duration(secs: i64) -> Duration {
    Duration::from_secs(secs.max(0) as u64)
}

/// "1h 4m" / "–" for the stats heatmap + mini-stat tiles.
pub fn format_hm_compact(secs: i64) -> String {
    render_hm(meditate_core::format::hm_compact_key(secs_to_duration(secs)))
}

/// "1h 4m" / "–" for the seconds-precision stats sites
/// (insights, weekly comparison, longest session).
pub fn format_hm_secs(secs: i64) -> String {
    render_hm(meditate_core::format::hm_secs_key(secs_to_duration(secs)))
}

/// "1h 4m" / "0m" for the weekly-goal subtitle + progress ring —
/// the variant where 0 is meaningful data, not "no data".
pub fn format_hm_mins(mins: i64) -> String {
    render_hm(meditate_core::format::hm_mins_key(secs_to_duration(
        mins.saturating_mul(60),
    )))
}
