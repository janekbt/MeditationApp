pub mod app;
#[cfg(target_os = "android")]
mod haptics;
#[cfg(target_os = "android")]
mod drop_file;
#[cfg(target_os = "android")]
mod jni_call;
#[cfg(target_os = "android")]
mod service;
#[cfg(target_os = "android")]
mod sounds;
#[cfg(target_os = "android")]
mod audio;
// android: the live JNI/fs widget bridge. test: the host
// `cargo test --workspace` compiles + runs the pure
// `build_projection_json` unit tests (strict-TDD). A plain host
// build uses neither, so the module is absent there — keeps it
// off the host dead-code path without an `#[allow]`.
#[cfg(any(target_os = "android", test))]
mod widget;
#[cfg(target_os = "android")]
mod guided;
#[cfg(target_os = "android")]
mod keychain;
#[cfg(target_os = "android")]
mod about;
#[cfg(target_os = "android")]
mod insets;
// pub (like `app`): host builds keep its tests without dead code.
pub mod theme;
#[cfg(any(target_os = "android", test))]
mod alarm_volume;
#[cfg(target_os = "android")]
mod screen;
#[cfg(target_os = "android")]
mod sync_runner;
#[cfg(target_os = "android")]
mod ui;

slint::include_modules!();

// `cargo run` on the host shows the bare UI (no database, no
// wiring); the phone enters through `ui::android_main`.
pub fn main() {
    MainWindow::new().unwrap().run().unwrap();
}
