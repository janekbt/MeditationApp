// `cargo run` on the host enters here. The android-activity entry
// point is a separate `android_main` symbol exported from lib.rs.
fn main() {
    meditate_android::main();
}
