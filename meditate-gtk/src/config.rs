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

#[cfg(test)]
mod tests {
    /// `cargo test` builds without an APP_VERSION override, so this is
    /// the build.rs fallback — it must be the crate's own version, which
    /// bump-version.sh stamps, not a hardcoded string that goes stale.
    #[test]
    fn version_fallback_is_the_crate_version() {
        assert_eq!(super::VERSION, env!("CARGO_PKG_VERSION"));
    }

    /// Meson and the Flatpak manifest pass APP_VERSION explicitly; both
    /// must carry the same version as Cargo.toml so every build of the
    /// app reports the same number.
    #[test]
    fn meson_and_flatpak_versions_match_cargo() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let want = env!("CARGO_PKG_VERSION");
        let meson = std::fs::read_to_string(dir.join("meson.build")).unwrap();
        assert!(
            meson.contains(&format!("version: '{want}'")),
            "meson.build project version != {want}",
        );
        let manifest = std::fs::read_to_string(
            dir.join("../build-aux/io.github.janekbt.Meditate.json"),
        )
        .unwrap();
        assert!(
            manifest.contains(&format!("\"APP_VERSION\": \"{want}\"")),
            "Flatpak manifest APP_VERSION != {want}",
        );
    }
}
