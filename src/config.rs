// Build-time constants injected by build.rs (overridable by Meson/Flatpak env vars).
pub const APP_ID: &str = env!("APP_ID");
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
#[allow(dead_code)] // used in Phase 6 for loading sounds
pub const PKGDATADIR: &str = env!("PKGDATADIR");
