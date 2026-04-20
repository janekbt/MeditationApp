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
//! `xgettext` picks up `gettext("…")` call sites automatically when
//! scanning the files listed in `po/POTFILES.in`.

pub use gettextrs::gettext;
