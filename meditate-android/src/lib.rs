slint::include_modules!();

fn ui() -> MainWindow {
    MainWindow::new().unwrap()
}

pub fn main() {
    let ui = ui();
    ui.run().unwrap();
}

// Android entry point. android-activity calls this after JNI init;
// `slint::android::init` hooks the activity into Slint's event loop.
// `set_disable_hover` matches the canonical Slint Material template —
// hover effects from synthesised pointer-enter events look wrong on
// touch screens, so we turn them off.
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
fn android_main(android_app: slint::android::AndroidApp) {
    slint::android::init(android_app).unwrap();
    let ui = ui();
    MaterialWindowAdapter::get(&ui).set_disable_hover(true);
    ui.run().unwrap();
}
