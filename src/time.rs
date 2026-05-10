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
