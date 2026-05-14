//! Defensive wrappers around the couple of glib datetime calls that can
//! fail on pathological systems (missing tzdata, exhausted clock).
//!
//! The portable unix↔ISO conversion functions used at the database
//! boundary have moved to `meditate_core::time`; this file is now just
//! the glib-bound helpers that a non-GTK shell can't reuse anyway.

use gtk::glib;

/// Current local time. Falls back to UTC if the tzdata lookup fails —
/// better a slightly wrong clock than a panic on the stats tab.
pub fn now_local() -> glib::DateTime {
    glib::DateTime::now_local()
        .or_else(|_| glib::DateTime::now_utc())
        .expect("system reports no working clock")
}

/// Build a `glib::DateTime` (local) at midnight from a `YYYY-MM-DD`
/// ISO date string. Used by callers (the stats contrib walker) that
/// receive `date_iso` from a core helper and need glib's locale-aware
/// `%A %B %e %b` formatting for the rendered date.
pub fn glib_datetime_from_iso(date_iso: &str) -> Option<glib::DateTime> {
    let mut parts = date_iso.splitn(3, '-');
    let y: i32 = parts.next()?.parse().ok()?;
    let m: i32 = parts.next()?.parse().ok()?;
    let d: i32 = parts.next()?.parse().ok()?;
    glib::DateTime::from_local(y, m, d, 0, 0, 0.0).ok()
}
