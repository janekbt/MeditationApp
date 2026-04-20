//! Defensive wrappers around the couple of glib datetime calls that can
//! fail on pathological systems (missing tzdata, exhausted clock).

use gtk::glib;

/// Current local time. Falls back to UTC if the tzdata lookup fails —
/// better a slightly wrong clock than a panic on the stats tab.
pub fn now_local() -> glib::DateTime {
    glib::DateTime::now_local()
        .or_else(|_| glib::DateTime::now_utc())
        .expect("system reports no working clock")
}
