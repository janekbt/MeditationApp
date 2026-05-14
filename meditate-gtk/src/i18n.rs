//! Translation helpers for user-visible strings.
//!
//! `gettext()` re-exports `gettextrs::gettext` under our project's text
//! domain (set up in `main.rs` at startup). Use it everywhere a string
//! would appear on the UI:
//!
//! ```ignore
//! use crate::i18n::gettext;
//! button.set_label(&gettext("Save"));
//! ```
//!
//! `ngettext()` is the plural-aware counterpart. Pass both forms; the
//! locale's `Plural-Forms` header picks. Use it instead of
//! `gettext("X {n} Y").replace(...)` whenever the rendered string
//! varies on a count — `n` may pick a special form (Russian few/many,
//! Polish 1/few/many, Arabic 0/1/2/few/many/other) that a binary
//! singular/plural split would render wrong:
//!
//! ```ignore
//! use crate::i18n::ngettext;
//! let label = ngettext("1 session", "{n} sessions", n as u32)
//!     .replace("{n}", &n.to_string());
//! ```
//!
//! `xgettext` picks up both `gettext("…")` and `ngettext("…","…",…)`
//! call sites automatically when scanning the files listed in
//! `po/POTFILES.in`.

pub use gettextrs::{gettext, ngettext};
