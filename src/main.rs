mod application;
mod config;
pub mod db;
pub mod log;
pub mod timer;
mod window;

use gtk::gio;
use gtk::prelude::*;

fn main() -> glib::ExitCode {
    gio::resources_register_include!("compiled.gresource")
        .expect("Could not register resources");

    let app = application::MeditateApplication::new();
    app.run()
}
