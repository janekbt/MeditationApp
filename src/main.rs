mod application;
mod config;
mod data_io;
pub mod db;
pub mod i18n;
pub mod log;
mod preferences;
pub mod sound;
pub mod stats;
pub mod time;
pub mod timer;
pub mod vibration;
mod window;

use gtk::gio;
use gtk::prelude::*;

fn main() -> glib::ExitCode {
    // Renderer must be selected before gtk::init runs, otherwise GTK has
    // already picked one and GSK_RENDERER is ignored.
    select_gsk_renderer();

    // gettext must come up before any user-visible string is generated,
    // otherwise lookups fall back to msgid for the whole first frame.
    setup_gettext();

    gio::resources_register_include!("compiled.gresource")
        .expect("Could not register resources");

    let app = application::MeditateApplication::new();
    app.run()
}

/// On mobile GNOME (Phosh), GTK's default renderer path usually ends up as
/// `GskVulkanRenderer` on lavapipe — software Vulkan — because the Vivante/
/// Mali GPUs on devices like the Librem 5 and PinePhone don't have a real
/// Mesa Vulkan driver, and etnaviv EGL config selection fails. On those
/// devices, forcing Cairo is ~30× faster for first-frame paint than
/// lavapipe. Respects an explicit user-set GSK_RENDERER.
fn select_gsk_renderer() {
    if std::env::var_os("GSK_RENDERER").is_some() { return; }

    let is_phosh = ["XDG_SESSION_DESKTOP", "XDG_CURRENT_DESKTOP"]
        .iter()
        .filter_map(|k| std::env::var(k).ok())
        .any(|v| v.to_ascii_lowercase().contains("phosh"));

    if is_phosh {
        std::env::set_var("GSK_RENDERER", "cairo");
    }
}

/// Bind the `meditate` gettext text domain so every `gettext("…")` call
/// at runtime resolves via LC_MESSAGES catalogs. Honours a LOCALEDIR
/// env-var override so dev runs can point at `build/po` without having
/// to reinstall the app. Failures are non-fatal — the app still works
/// untranslated if the catalog dir can't be bound.
fn setup_gettext() {
    use gettextrs::{bind_textdomain_codeset, bindtextdomain, setlocale, textdomain, LocaleCategory};

    setlocale(LocaleCategory::LcAll, "");
    let locale_dir = std::env::var("LOCALEDIR")
        .unwrap_or_else(|_| config::LOCALEDIR.to_string());
    if let Err(e) = bindtextdomain(config::GETTEXT_DOMAIN, locale_dir.as_str()) {
        eprintln!("note: bindtextdomain failed ({e}); strings will stay in source language.");
        return;
    }
    let _ = bind_textdomain_codeset(config::GETTEXT_DOMAIN, "UTF-8");
    if let Err(e) = textdomain(config::GETTEXT_DOMAIN) {
        eprintln!("note: textdomain failed ({e}); strings will stay in source language.");
    }
}
