mod application;
mod config;
mod window;

use gtk::gio;
use gtk::prelude::*;

fn main() -> glib::ExitCode {
    gio::resources_register_include!("compiled.gresource")
        .expect("Could not register resources");

    let app = application::MeditateApplication::new();
    app.run()
}
