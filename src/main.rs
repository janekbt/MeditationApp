mod application;
mod config;
mod window;

use gtk::gio;

fn main() -> glib::ExitCode {
    // Embed GResource bundle at compile time (built by build.rs from Blueprint files)
    gio::resources_register_include!("compiled.gresource")
        .expect("Could not register resources");

    let app = application::MeditateApplication::new();
    app.run()
}
