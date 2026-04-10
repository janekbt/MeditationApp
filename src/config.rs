// Build-time constants injected by build.rs (overridable by Meson/Flatpak env vars).
pub const APP_ID: &str = env!("APP_ID");
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const PKGDATADIR: &str = env!("PKGDATADIR");
