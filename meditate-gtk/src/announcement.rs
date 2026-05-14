//! GTK shell renderer for [`meditate_core::announcement::Announcement`].
//!
//! Maps each variant to a gettext-localized toast title string at
//! the call site. The toast widget itself (button label, timeout,
//! undo handler) stays in the calling module — the typed key just
//! takes the *message* off the call site so the gtk and Android
//! shells word the same event the same way.

use crate::i18n::{gettext, ngettext};
use meditate_core::announcement::Announcement;

/// Render an `Announcement` to its user-facing title. Variants
/// carrying a name interpolate it via `{name}` so the translation
/// catalog controls argument order across locales.
pub fn title(announcement: &Announcement) -> String {
    match announcement {
        Announcement::SessionRecovered { minutes } => ngettext(
            "Recovered 1 min session",
            "Recovered {n} min session",
            *minutes,
        )
        .replace("{n}", &minutes.to_string()),
        Announcement::SessionDeleted { count } => ngettext(
            "Session deleted",
            "{n} sessions deleted",
            *count,
        )
        .replace("{n}", &count.to_string()),
        Announcement::PresetDeleted { name } => {
            gettext("'{name}' deleted").replace("{name}", name)
        }
        Announcement::PresetOverridden { name } => {
            gettext("'{name}' overridden").replace("{name}", name)
        }
        Announcement::PatternUpdated { name } => {
            format!("{} {}", gettext("Updated"), name)
        }
        Announcement::PatternDeleted { name } => {
            format!("{} {}", gettext("Deleted"), name)
        }
    }
}
