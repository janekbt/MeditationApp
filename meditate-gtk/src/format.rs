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

/// A number of seconds with the unit, "4s" / "4 с" (Android's msgid).
pub fn secs_text(value: &str) -> String {
    gettext("{}s").replace("{}", value)
}

/// A number of days with the unit, "5d" / "5 T.".
pub fn days_text(n: u32) -> String {
    ngettext("{n}d", "{n}d", n).replace("{n}", &n.to_string())
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

/// "1h 4m" / "0m" for the daily-goal subtitle + progress ring —
/// the variant where 0 is meaningful data, not "no data".
pub fn format_hm_mins(mins: i64) -> String {
    render_hm(meditate_core::format::hm_mins_key(secs_to_duration(
        mins.saturating_mul(60),
    )))
}

/// A date pattern translated into the language of the Formats setting
/// (LC_TIME), which also names the months, not the UI language. Read
/// from that language's catalog directly: glibc's lookup lets LANGUAGE
/// (Ubuntu sets "en_US:en") override LC_TIME.
fn date_pattern(msgid: &'static str) -> String {
    thread_local! {
        static PATTERNS: std::cell::RefCell<std::collections::HashMap<&'static str, String>> =
            Default::default();
    }
    PATTERNS.with(|p| p.borrow_mut().entry(msgid).or_insert_with(|| read_date_pattern(msgid)).clone())
}

// ponytail: plain msgid scan (no msgctxt, no plural), enough for the three
// date patterns; a pattern with a context needs the .mo hash table.
fn read_date_pattern(msgid: &str) -> String {
    // SAFETY: a NULL locale only queries; the returned string is copied
    // before any other setlocale call can replace it.
    let locale = unsafe {
        let current = libc::setlocale(libc::LC_TIME, std::ptr::null());
        if current.is_null() { String::new() } else { std::ffi::CStr::from_ptr(current).to_string_lossy().into_owned() }
    };
    let dir = crate::config::locale_dir();
    formats_languages(&locale)
        .iter()
        .find_map(|lang| {
            let path = format!("{dir}/{lang}/LC_MESSAGES/{}.mo", crate::config::GETTEXT_DOMAIN);
            mo_translation(&std::fs::read(path).ok()?, msgid)
        })
        .unwrap_or_else(|| msgid.to_string())
}

/// Catalog names to try for a locale, most specific first:
/// "de_DE.UTF-8@euro" gives "de_DE", "de".
fn formats_languages(locale: &str) -> Vec<String> {
    let name = locale.split(['.', '@']).next().unwrap_or_default();
    if name.is_empty() || name == "C" || name == "POSIX" {
        return Vec::new();
    }
    let language = name.split('_').next().unwrap_or_default();
    let mut out = vec![name.to_string()];
    if language != name {
        out.push(language.to_string());
    }
    out
}

/// The translation of `msgid` in a compiled .mo catalog.
fn mo_translation(mo: &[u8], msgid: &str) -> Option<String> {
    let big_endian = match mo.get(..4)? {
        [0xde, 0x12, 0x04, 0x95] => false,
        [0x95, 0x04, 0x12, 0xde] => true,
        _ => return None,
    };
    let word = |at: usize| -> Option<usize> {
        let b: [u8; 4] = mo.get(at..at + 4)?.try_into().ok()?;
        Some(if big_endian { u32::from_be_bytes(b) } else { u32::from_le_bytes(b) } as usize)
    };
    let string = |table: usize, i: usize| -> Option<&[u8]> {
        let (len, off) = (word(table + 8 * i)?, word(table + 8 * i + 4)?);
        mo.get(off..off + len)
    };
    let (count, originals, translations) = (word(8)?, word(12)?, word(16)?);
    let i = (0..count).find(|&i| string(originals, i) == Some(msgid.as_bytes()))?;
    String::from_utf8(string(translations, i)?.to_vec()).ok().filter(|t| !t.is_empty())
}

/// The short date ("Oct 9", with year "Oct 9, 2025"), in the
/// Formats language's own order.
pub fn short_date(dt: &glib::DateTime, with_year: bool) -> String {
    let pattern = if with_year {
        // Date with year, as a GLib date format: %b short month, %-d day without padding, %Y year. Put them in your language's order.
        date_pattern("%b %-d, %Y")
    } else {
        // Date without year, as a GLib date format: %b short month, %-d day without padding. Put them in your language's order.
        date_pattern("%b %-d")
    };
    dt.format(&pattern).map(|s| s.to_string()).unwrap_or_default()
}

/// The date a screen reader speaks for a heatmap cell
/// ("Thursday, October 9").
pub fn heatmap_date(dt: &glib::DateTime) -> String {
    // Date a screen reader speaks, as a GLib date format: %A weekday, %B month, %-d day without padding (%-m month number). Put them in your language's order.
    let pattern = date_pattern("%A, %B %-d");
    dt.format(&pattern).map(|s| s.to_string()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Month and weekday names come from the test's LC_TIME; the
    // padding and the order are what the patterns decide.
    fn october_9() -> glib::DateTime {
        glib::DateTime::from_local(2025, 10, 9, 12, 0, 0.0).unwrap()
    }

    fn name(dt: &glib::DateTime, spec: &str) -> String {
        dt.format(spec).unwrap().to_string()
    }

    #[test]
    fn a_locale_names_its_catalogs_most_specific_first() {
        assert_eq!(formats_languages("de_DE.UTF-8@euro"), ["de_DE", "de"]);
        assert_eq!(formats_languages("zh_CN.UTF-8"), ["zh_CN", "zh"]);
        assert_eq!(formats_languages("ru"), ["ru"]);
        assert!(formats_languages("C.UTF-8").is_empty());
        assert!(formats_languages("POSIX").is_empty());
        assert!(formats_languages("").is_empty());
    }

    #[test]
    fn a_date_pattern_is_read_from_a_compiled_catalog() {
        let dir = std::env::temp_dir().join(format!("meditate-mo-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mo = dir.join("de.mo");
        let po = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("po/de.po");
        let ok = std::process::Command::new("msgfmt").arg("-o").arg(&mo).arg(&po).status().unwrap();
        assert!(ok.success());
        let bytes = std::fs::read(&mo).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(mo_translation(&bytes, "%b %-d").as_deref(), Some("%-d. %b"));
        assert_eq!(mo_translation(&bytes, "%A, %B %-d").as_deref(), Some("%A, %-d. %B"));
        assert_eq!(mo_translation(&bytes, "no such msgid"), None);
        assert_eq!(mo_translation(b"not a catalog", "%b %-d"), None);
    }

    #[test]
    fn a_short_date_has_no_padded_day() {
        let dt = october_9();
        assert_eq!(short_date(&dt, true), format!("{} 9, 2025", name(&dt, "%b")));
        assert_eq!(short_date(&dt, false), format!("{} 9", name(&dt, "%b")));
    }

    #[test]
    fn the_spoken_heatmap_date_has_no_padded_day() {
        let dt = october_9();
        assert_eq!(heatmap_date(&dt), format!("{}, {} 9", name(&dt, "%A"), name(&dt, "%B")));
    }
}
