// Build-time constants injected by build.rs (overridable by Meson/Flatpak env vars).
pub const APP_ID: &str = env!("APP_ID");
/// User-visible version string (e.g. "26.4.1").
/// Set by build.rs; Meson/Flatpak builds override via the APP_VERSION env var.
pub const VERSION: &str = env!("APP_VERSION");
#[allow(dead_code)]
pub const PKGDATADIR: &str = env!("PKGDATADIR");
/// Install path where gettext finds compiled .mo translation catalogs.
pub const LOCALEDIR: &str = env!("LOCALEDIR");
/// gettext text domain — matches the meson project name and the
/// `meditate.mo` filename the i18n.gettext() target produces.
pub const GETTEXT_DOMAIN: &str = "meditate";
